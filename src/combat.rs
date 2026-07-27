use bevy::prelude::*;
use std::collections::HashMap;

use crate::belts::{BeltSim, BuildingKind, INVALID};
use crate::grid::Grid;
use crate::render::TILE;

const WAVE_INTERVAL: u16 = 900;
const ENEMY_HEALTH: f32 = 60.0;
const ENEMY_SPEED: f32 = 0.025;
const ENEMY_ATTACK_RANGE: f32 = 0.7;
const ENEMY_ATTACK_COOLDOWN: u16 = 45;
const ENEMY_DAMAGE: f32 = 25.0;
const BUILDING_HEALTH: f32 = 100.0;
const TURRET_RANGE: f32 = 7.0;
const TURRET_COOLDOWN: u16 = 20;
const TURRET_DAMAGE: f32 = 20.0;
const AMMO_KIND: usize = 8;
const REPAIR_KIND: usize = 4;
const REPAIR_AMOUNT: f32 = 25.0;

#[derive(Clone, Copy)]
pub enum EnemyKind {
    Runner,
    Raider,
    Siege,
}

#[derive(Clone)]
pub struct Enemy {
    pub position: Vec2,
    pub health: f32,
    pub attack_cooldown: u16,
    pub kind: EnemyKind,
}

#[derive(Resource)]
pub struct CombatState {
    pub enemies: Vec<Enemy>,
    pub wave: u32,
    pub next_wave: u16,
    pub threat: f32,
    pub building_health: HashMap<u32, f32>,
}

impl Default for CombatState {
    fn default() -> Self {
        Self {
            enemies: Vec::new(),
            wave: 0,
            next_wave: 300,
            threat: 0.0,
            building_health: HashMap::new(),
        }
    }
}

#[derive(Component)]
pub struct EnemySprite;

fn targetable(kind: BuildingKind) -> bool {
    !matches!(kind, BuildingKind::RailTrack)
}

fn target_priority(enemy: EnemyKind, kind: BuildingKind) -> u8 {
    match (enemy, kind) {
        (EnemyKind::Runner, BuildingKind::Generator | BuildingKind::Pole) => 0,
        (EnemyKind::Raider, BuildingKind::RailStation | BuildingKind::Storage | BuildingKind::Shipment) => 0,
        (EnemyKind::Siege, BuildingKind::Turret | BuildingKind::Assembler) => 0,
        _ => 1,
    }
}

impl CombatState {
    pub fn tick(
        &mut self,
        sim: &mut BeltSim,
        grid: &mut Grid,
        active_blds: &[usize],
        factory_load: usize,
        enabled: bool,
    ) -> bool {
        if !enabled {
            return false;
        }

        let targets: Vec<usize> = active_blds
            .iter()
            .copied()
            .filter(|&s| sim.bld_active[s] && targetable(sim.bld_kind[s]))
            .collect();
        self.building_health.retain(|&id, _| sim.bld_active[id as usize]);
        if targets.is_empty() {
            return false;
        }

        self.threat = (self.threat * 0.995 + factory_load as f32 * 0.015).min(120.0);
        if self.next_wave > 0 {
            self.next_wave -= 1;
        } else {
            self.wave += 1;
            self.next_wave = WAVE_INTERVAL.saturating_sub((self.threat * 3.0) as u16).max(240);
            let center = targets.iter().fold(Vec2::ZERO, |sum, &s| {
                sum + Vec2::new(sim.bld_x[s] as f32, sim.bld_y[s] as f32)
            }) / targets.len() as f32;
            let count = 2 + self.wave.min(8) as usize + (self.threat as usize / 30);
            for i in 0..count {
                let offset = 18.0 + (i % 3) as f32 * 1.5;
                let position = match (self.wave as usize + i) % 4 {
                    0 => center + Vec2::new(offset, i as f32 - count as f32 / 2.0),
                    1 => center + Vec2::new(-offset, i as f32 - count as f32 / 2.0),
                    2 => center + Vec2::new(i as f32 - count as f32 / 2.0, offset),
                    _ => center + Vec2::new(i as f32 - count as f32 / 2.0, -offset),
                };
                let kind = match (self.wave as usize + i) % 3 {
                    0 => EnemyKind::Runner,
                    1 => EnemyKind::Raider,
                    _ => EnemyKind::Siege,
                };
                let health_bonus = match kind {
                    EnemyKind::Runner => 0.0,
                    EnemyKind::Raider => 20.0,
                    EnemyKind::Siege => 45.0,
                };
                self.enemies.push(Enemy {
                    position,
                    health: ENEMY_HEALTH + self.wave as f32 * 8.0 + health_bonus,
                    attack_cooldown: 0,
                    kind,
                });
            }
        }

        for &s in active_blds {
            if !sim.bld_active[s] || sim.bld_kind[s] != BuildingKind::Turret || !sim.bld_powered[s] {
                continue;
            }
            if sim.bld_timer[s] > 0 {
                sim.bld_timer[s] -= 1;
                continue;
            }
            let origin = Vec2::new(sim.bld_x[s] as f32, sim.bld_y[s] as f32);
            let target = self
                .enemies
                .iter()
                .enumerate()
                .filter_map(|(i, enemy)| {
                    let distance = origin.distance_squared(enemy.position);
                    (distance <= TURRET_RANGE * TURRET_RANGE).then_some((i, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            if let Some(enemy) = target {
                if sim.bld_in[s][AMMO_KIND] > 0 {
                    sim.bld_in[s][AMMO_KIND] -= 1;
                    self.enemies[enemy].health -= TURRET_DAMAGE;
                    sim.bld_timer[s] = TURRET_COOLDOWN;
                }
            } else if sim.bld_in[s][REPAIR_KIND] > 0 {
                let health = self.building_health.entry(s as u32).or_insert(BUILDING_HEALTH);
                if *health < BUILDING_HEALTH {
                    sim.bld_in[s][REPAIR_KIND] -= 1;
                    *health = (*health + REPAIR_AMOUNT).min(BUILDING_HEALTH);
                    sim.bld_timer[s] = TURRET_COOLDOWN;
                }
            }
        }
        self.enemies.retain(|enemy| enemy.health > 0.0);

        let mut destroyed = Vec::new();
        let mut world_changed = false;
        for enemy in &mut self.enemies {
            let target = targets
                .iter()
                .copied()
                .filter(|&s| sim.bld_active[s])
                .min_by(|&a, &b| {
                    let ap = Vec2::new(sim.bld_x[a] as f32, sim.bld_y[a] as f32);
                    let bp = Vec2::new(sim.bld_x[b] as f32, sim.bld_y[b] as f32);
                    target_priority(enemy.kind, sim.bld_kind[a])
                        .cmp(&target_priority(enemy.kind, sim.bld_kind[b]))
                        .then_with(|| enemy.position.distance_squared(ap).total_cmp(&enemy.position.distance_squared(bp)))
                });
            let Some(target) = target else { continue };
            let destination = Vec2::new(sim.bld_x[target] as f32, sim.bld_y[target] as f32);
            let delta = destination - enemy.position;
            let distance = delta.length();
            if distance > ENEMY_ATTACK_RANGE {
                enemy.position += delta / distance * ENEMY_SPEED;
                continue;
            }
            if enemy.attack_cooldown > 0 {
                enemy.attack_cooldown -= 1;
            } else {
                let health = self.building_health.entry(target as u32).or_insert(BUILDING_HEALTH);
                *health -= ENEMY_DAMAGE;
                if *health <= 0.0 {
                    destroyed.push(target as u32);
                }
                enemy.attack_cooldown = ENEMY_ATTACK_COOLDOWN;
            }
        }

        destroyed.sort_unstable();
        destroyed.dedup();
        for id in destroyed {
            let s = id as usize;
            if !sim.bld_active[s] {
                continue;
            }
            let x = sim.bld_x[s];
            let y = sim.bld_y[s];
            self.building_health.remove(&id);
            sim.remove_building(id);
            grid.set_building(x, y, INVALID);
            world_changed = true;
        }
        world_changed
    }
}

pub fn update_enemy_visuals(
    mut commands: Commands,
    combat: Res<CombatState>,
    mut existing: Query<(Entity, &mut Transform, &mut Sprite), With<EnemySprite>>,
) {
    let mut sprites = existing.iter_mut();
    for enemy in &combat.enemies {
        let health_ratio = (enemy.health / ENEMY_HEALTH).clamp(0.25, 1.0);
        let color = Color::srgb(0.75, 0.12 + health_ratio * 0.2, 0.16);
        let translation = Vec3::new(enemy.position.x * TILE, enemy.position.y * TILE, 4.0);
        if let Some((_, mut transform, mut sprite)) = sprites.next() {
            transform.translation = translation;
            sprite.color = color;
        } else {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::splat(TILE * 0.38)),
                        ..default()
                    },
                    transform: Transform::from_translation(translation),
                    ..default()
                },
                EnemySprite,
            ));
        }
    }
    for (entity, _, _) in sprites {
        commands.entity(entity).despawn();
    }
}
