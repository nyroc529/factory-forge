//! Minimal rail logistics: rail tracks connect stations, trains move cargo.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::belts::{BeltSim, BuildingKind};
use crate::render::TILE;
use crate::sim::STORAGE_CAP;

pub const TRAIN_CAPACITY: u16 = 20;
/// Rail tiles traveled per simulation tick.
pub const TRAIN_SPEED: f32 = 0.4;

#[derive(Resource, Default)]
pub struct RailNetwork {
    pub components: Vec<RailComponent>,
    pub trains: Vec<Train>,
    paths: HashMap<(u32, u32), Vec<(i32, i32)>>,
}

pub struct RailComponent {
    pub stations: Vec<u32>,
    /// All traversable tiles for this component, including stations.
    pub track_positions: HashSet<(i32, i32)>,
}

#[derive(Clone)]
pub struct Train {
    pub from: u32,
    pub to: u32,
    pub kind: u16,
    pub count: u16,
    pub path: Vec<(i32, i32)>,
    pub progress: f32,
}

#[derive(Component)]
pub struct TrainSprite;

const NEIGHBOURS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn pos_of(sim: &BeltSim, bld: u32) -> (i32, i32) {
    let i = bld as usize;
    (sim.bld_x[i], sim.bld_y[i])
}

fn path_between(track_positions: &HashSet<(i32, i32)>, start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
    if start == end {
        return vec![start];
    }
    let mut parent: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut queue = VecDeque::new();
    queue.push_back(start);
    parent.insert(start, start);
    while let Some(p) = queue.pop_front() {
        if p == end {
            break;
        }
        for (dx, dy) in NEIGHBOURS {
            let n = (p.0 + dx, p.1 + dy);
            if !track_positions.contains(&n) {
                continue;
            }
            if parent.contains_key(&n) {
                continue;
            }
            parent.insert(n, p);
            queue.push_back(n);
        }
    }
    if !parent.contains_key(&end) {
        return vec![start];
    }
    let mut path = vec![end];
    let mut cur = end;
    while cur != start {
        cur = parent[&cur];
        path.push(cur);
    }
    path.reverse();
    path
}

impl RailNetwork {
    pub fn rebuild(&mut self, sim: &BeltSim, active_blds: &[usize]) {
        self.components.clear();
        self.paths.clear();
        self.trains.retain(|t| sim.bld_active[t.from as usize] && sim.bld_active[t.to as usize]);

        let mut track_positions = Vec::new();
        let mut stations: Vec<(u32, (i32, i32))> = Vec::new();
        for &s in active_blds {
            if !sim.bld_active[s] {
                continue;
            }
            match sim.bld_kind[s] {
                BuildingKind::RailTrack => track_positions.push((sim.bld_x[s], sim.bld_y[s])),
                BuildingKind::RailStation => stations.push((s as u32, (sim.bld_x[s], sim.bld_y[s]))),
                _ => {}
            }
        }

        let mut component_map: HashMap<(i32, i32), usize> = HashMap::new();
        let mut station_component: HashMap<u32, usize> = HashMap::new();
        let mut components: Vec<RailComponent> = Vec::new();

        let track_set: HashSet<(i32, i32)> = track_positions.iter().copied().collect();

        for &pos in &track_positions {
            if component_map.contains_key(&pos) {
                continue;
            }
            let mut comp_tracks = HashSet::new();
            let comp_stations = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(pos);
            component_map.insert(pos, components.len());
            while let Some(p) = queue.pop_front() {
                comp_tracks.insert(p);
                for &(dx, dy) in &NEIGHBOURS {
                    let n = (p.0 + dx, p.1 + dy);
                    if component_map.contains_key(&n) {
                        continue;
                    }
                    if track_set.contains(&n) {
                        component_map.insert(n, components.len());
                        queue.push_back(n);
                    }
                }
            }
            components.push(RailComponent {
                stations: comp_stations,
                track_positions: comp_tracks,
            });
        }

        // Add station tiles to their nearest connected component if adjacent to a rail.
        for (id, spos) in stations {
            let mut found = None;
            for &(dx, dy) in &NEIGHBOURS {
                let n = (spos.0 + dx, spos.1 + dy);
                if let Some(&c) = component_map.get(&n) {
                    found = Some(c);
                    break;
                }
            }
            // Stations must touch a rail to be usable.
            if let Some(c) = found {
                components[c].track_positions.insert(spos);
                components[c].stations.push(id);
                station_component.insert(id, c);
            }
        }

        self.components = components;
    }

    pub fn tick_trains(&mut self, sim: &mut BeltSim) {
        let mut paths = std::mem::take(&mut self.paths);
        // Move existing trains.
        let mut arrived = Vec::new();
        for (idx, train) in self.trains.iter_mut().enumerate() {
            train.progress += TRAIN_SPEED;
            let segments = (train.path.len().saturating_sub(1)) as f32;
            if segments <= 0.0 || train.progress >= segments {
                arrived.push(idx);
            }
        }
        // Unload in reverse order to keep indices valid.
        for &idx in arrived.iter().rev() {
            let train = self.trains.swap_remove(idx);
            let to = train.to as usize;
            if sim.bld_active[to] && train.kind < sim.bld_in[to].len() as u16 {
                let cap = STORAGE_CAP - sim.bld_in[to][train.kind as usize];
                let unload = train.count.min(cap);
                sim.bld_in[to][train.kind as usize] += unload;
            }
        }

        // Load new trains from stations.
        let mut loaded_stations: HashSet<u32> = HashSet::new();
        for comp in &self.components {
            if comp.stations.len() < 2 {
                continue;
            }
            for &src in &comp.stations {
                if !sim.bld_active[src as usize] || loaded_stations.contains(&src) {
                    continue;
                }
                let mut cargo_kind: Option<u16> = None;
                let mut available = 0u16;
                for k in 0..sim.bld_in[src as usize].len() {
                    if sim.bld_in[src as usize][k] > 0 {
                        cargo_kind = Some(k as u16);
                        available = sim.bld_in[src as usize][k];
                        break;
                    }
                }
                let kind = match cargo_kind {
                    Some(k) => k,
                    None => continue,
                };

                // Find a destination station with free space for this item kind.
                let mut best_dst: Option<u32> = None;
                let mut best_space = 0u16;
                for &dst in &comp.stations {
                    if dst == src || !sim.bld_active[dst as usize] {
                        continue;
                    }
                    let space = STORAGE_CAP - sim.bld_in[dst as usize][kind as usize];
                    if space > best_space {
                        best_space = space;
                        best_dst = Some(dst);
                    }
                }
                let dst = match best_dst {
                    Some(d) if best_space > 0 => d,
                    _ => continue,
                };

                let src_pos = pos_of(sim, src);
                let dst_pos = pos_of(sim, dst);
                let route = (src, dst);
                let path = paths
                    .entry(route)
                    .or_insert_with(|| path_between(&comp.track_positions, src_pos, dst_pos))
                    .clone();
                if path.len() < 2 {
                    continue;
                }
                let count = TRAIN_CAPACITY.min(available).min(best_space);
                sim.bld_in[src as usize][kind as usize] -= count;
                self.trains.push(Train {
                    from: src,
                    to: dst,
                    kind,
                    count,
                    path,
                    progress: 0.0,
                });
                loaded_stations.insert(src);
            }
        }
        self.paths = paths;
    }

    /// Current world-space position for a train, if any.
    pub fn train_position(&self, train: &Train) -> Option<Vec2> {
        if train.path.is_empty() {
            return None;
        }
        let segments = (train.path.len() - 1) as f32;
        let t = train.progress.max(0.0).min(segments);
        let i = t.floor() as usize;
        let frac = t - i as f32;
        if i + 1 >= train.path.len() {
            let last = train.path[train.path.len() - 1];
            return Some(Vec2::new(last.0 as f32 * TILE, last.1 as f32 * TILE));
        }
        let a = train.path[i];
        let b = train.path[i + 1];
        Some(Vec2::new(
            (a.0 as f32 + (b.0 as f32 - a.0 as f32) * frac) * TILE,
            (a.1 as f32 + (b.1 as f32 - a.1 as f32) * frac) * TILE,
        ))
    }
}

pub fn update_train_visuals(
    mut commands: Commands,
    rail: Res<RailNetwork>,
    mut existing: Query<(Entity, &mut Transform, &mut Sprite), With<TrainSprite>>,
) {
    let mut sprites = existing.iter_mut();
    for train in &rail.trains {
        let Some(pos) = rail.train_position(train) else { continue };
        let color = match train.kind {
            0 => Color::srgb(0.8, 0.3, 0.3),
            1 => Color::srgb(0.8, 0.5, 0.2),
            2 => Color::srgb(0.15, 0.15, 0.15),
            _ => Color::srgb(0.7, 0.7, 0.75),
        };
        let translation = Vec3::new(pos.x, pos.y, 3.0);
        if let Some((_, mut transform, mut sprite)) = sprites.next() {
            transform.translation = translation;
            sprite.color = color;
        } else {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::splat(TILE * 0.5)),
                        ..default()
                    },
                    transform: Transform::from_translation(translation),
                    ..default()
                },
                TrainSprite,
            ));
        }
    }
    for (entity, _, _) in sprites {
        commands.entity(entity).despawn();
    }
}
