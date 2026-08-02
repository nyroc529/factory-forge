//! Batched rendering. Instead of one entity per item/belt, the whole world
//! is drawn with two meshes:
//!   - a static mesh for belts + buildings, rebuilt only on edits
//!   - a dynamic mesh for items + chevrons, rebuilt each frame from the SoA
//!     arrays with camera-rect culling
//! This collapses tens of thousands of entities into 2 draw calls.

use bevy::core_pipeline::bloom::BloomSettings;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::view::NoFrustumCulling;
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};

use crate::belts::{BeltSim, BuildingKind, Dir, INVALID, ITEM_NAMES};
use crate::combat::CombatState;
use crate::economy::{ContractState, PlayerState, ProductionStats, VictoryState};
use crate::settings::Settings;
use crate::sim::{INSERTER_COOLDOWN, RECIPES, is_consumer, is_power_node, POWER_RADIUS2};
use crate::ui::{Blueprint, EditorState, Selection};
use crate::{GameWorld, Sim};

pub const TILE: f32 = 48.0;
const LANE_OFFSET: f32 = 0.22;

pub const ITEM_COLORS: [Color; 11] = [
    Color::srgb(0.99, 0.76, 0.18), // 0 iron
    Color::srgb(0.35, 0.78, 0.98), // 1 copper
    Color::srgb(0.20, 0.20, 0.22), // 2 coal
    Color::srgb(0.96, 0.45, 0.55), // 3 gear
    Color::srgb(0.75, 0.55, 0.98), // 4 steel
    Color::srgb(0.62, 0.62, 0.60), // 5 stone
    Color::srgb(0.25, 0.15, 0.35), // 6 oil
    Color::srgb(0.95, 0.92, 0.85), // 7 plastic
    Color::srgb(0.20, 0.80, 0.35), // 8 circuit
    Color::srgb(0.72, 0.40, 0.22), // 9 brick
    Color::srgb(0.25, 0.85, 0.95), // 10 science
];

pub fn dir_angle(dir: Dir) -> f32 {
    match dir {
        Dir::East => 0.0,
        Dir::North => std::f32::consts::FRAC_PI_2,
        Dir::West => std::f32::consts::PI,
        Dir::South => -std::f32::consts::FRAC_PI_2,
    }
}

fn item_pos(sim: &BeltSim, belt: u32, lane: usize, dist: f32) -> Vec2 {
    let b = belt as usize;
    let center = Vec2::new(sim.belt_x[b] as f32 * TILE, sim.belt_y[b] as f32 * TILE);
    let (dx, dy) = sim.belt_dir[b].fvec();
    let (px, py) = sim.belt_dir[b].perp();
    let along = Vec2::new(dx, dy) * (dist - 0.5) * TILE;
    let side = Vec2::new(px, py) * if lane == 0 { LANE_OFFSET } else { -LANE_OFFSET } * TILE;
    center + along + side
}

// ------------------------------------------------------------- mesh builder

struct MeshBatch {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Default for MeshBatch {
    fn default() -> Self {
        // Preallocate a reasonable amount so the dynamic mesh doesn't reallocate
        // every frame while growing.
        Self {
            positions: Vec::with_capacity(16384),
            colors: Vec::with_capacity(16384),
            indices: Vec::with_capacity(24576),
        }
    }
}

impl MeshBatch {
    /// Push a rotated quad centered at `c` with half-extents `hw`/`hh`.
    fn quad(&mut self, c: Vec2, hw: f32, hh: f32, angle: f32, color: [f32; 4]) {
        let (s, co) = angle.sin_cos();
        let ex = Vec2::new(co, s);
        let ey = Vec2::new(-s, co);
        let base = self.positions.len() as u32;
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = c + ex * (hw * sx) + ey * (hh * sy);
            self.positions.push([p.x, p.y, 0.0]);
            self.colors.push(color);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Push a regular `sides`-gon polygon centered at `c` with radius `r`.
    fn ngon(&mut self, c: Vec2, r: f32, sides: u32, angle: f32, color: [f32; 4]) {
        let sides = sides.max(3);
        let base = self.positions.len() as u32;
        let step = std::f32::consts::TAU / sides as f32;
        for s in 0..sides {
            let a = angle + s as f32 * step;
            let p = c + Vec2::new(a.cos(), a.sin()) * r;
            self.positions.push([p.x, p.y, 0.0]);
            self.colors.push(color);
        }
        for s in 0..sides {
            let i = base + s;
            let j = base + (s + 1) % sides;
            self.positions.push([c.x, c.y, 0.0]);
            self.colors.push(color);
            self.indices.extend_from_slice(&[i, j, base + sides + s]);
        }
    }

    /// Thin line from `a` to `b` with the given thickness and color.
    fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: [f32; 4]) {
        let mid = (a + b) * 0.5;
        let d = b - a;
        let len = d.length() * 0.5;
        let angle = d.y.atan2(d.x);
        self.quad(mid, len, thickness * 0.5, angle, color);
    }

    fn write_to(self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
    }
}

fn lin(c: Color) -> [f32; 4] {
    c.to_linear().to_f32_array()
}

/// HDR-boosted color so bloom picks it up.
fn glow(c: Color, boost: f32) -> [f32; 4] {
    let mut v = c.to_linear().to_f32_array();
    v[0] *= boost;
    v[1] *= boost;
    v[2] *= boost;
    v
}

fn lin_a(c: Color, a: f32) -> [f32; 4] {
    let mut v = c.to_linear().to_f32_array();
    v[3] = a;
    v
}

/// Draw a small symbolic icon on top of each building so players can tell
/// machines apart at a glance. Unpowered buildings get a dimmed icon.
fn draw_building_icon(
    batch: &mut MeshBatch,
    c: Vec2,
    half: f32,
    kind: BuildingKind,
    dir: Dir,
    powered: bool,
) {
    let alpha = if powered { 1.0 } else { 0.35 };
    let s = |r: f32, g: f32, b: f32| lin_a(Color::srgb(r, g, b), alpha);
    let pi = std::f32::consts::PI;

    match kind {
        BuildingKind::Miner => {
            // Yellow drill head on a dark bar.
            batch.ngon(
                c + Vec2::new(0.0, half * 0.1),
                half * 0.35,
                3,
                pi,
                s(0.95, 0.85, 0.25),
            );
            batch.line(
                c + Vec2::new(-half * 0.35, -half * 0.25),
                c + Vec2::new(half * 0.35, -half * 0.25),
                half * 0.12,
                s(0.5, 0.5, 0.55),
            );
        }
        BuildingKind::Assembler => {
            // Gear-like hexagon with a center hub.
            let gear = s(0.75, 0.75, 0.8);
            batch.ngon(c, half * 0.35, 6, 0.0, gear);
            batch.ngon(c, half * 0.12, 6, 0.0, s(0.25, 0.25, 0.3));
        }
        BuildingKind::Inserter => {
            // Arm reaching toward the output direction.
            let (dx, dy) = dir.fvec();
            let tip = c + Vec2::new(dx, dy) * half * 0.55;
            let base = c - Vec2::new(dx, dy) * half * 0.4;
            batch.line(base, tip, half * 0.12, s(0.9, 0.9, 0.95));
        }
        BuildingKind::Storage => {
            // Crate "X".
            let x = s(0.8, 0.8, 0.85);
            batch.line(
                c + Vec2::new(-half * 0.3, -half * 0.3),
                c + Vec2::new(half * 0.3, half * 0.3),
                half * 0.1,
                x,
            );
            batch.line(
                c + Vec2::new(half * 0.3, -half * 0.3),
                c + Vec2::new(-half * 0.3, half * 0.3),
                half * 0.1,
                x,
            );
        }
        BuildingKind::Shipment => {
            // Arrow pointing the output direction.
            let (dx, dy) = dir.fvec();
            let tip = c + Vec2::new(dx, dy) * half * 0.5;
            let back = c - Vec2::new(dx, dy) * half * 0.2;
            let perp = Vec2::new(-dy, dx) * half * 0.25;
            let arr = s(0.95, 0.95, 1.0);
            batch.line(back + perp, tip, half * 0.1, arr);
            batch.line(back - perp, tip, half * 0.1, arr);
        }
        BuildingKind::Generator => {
            // Lightning bolt symbol.
            let bolt = s(0.15, 0.12, 0.04);
            let a = c + Vec2::new(-half * 0.15, half * 0.35);
            let b = c + Vec2::new(half * 0.05, half * 0.0);
            let d = c + Vec2::new(-half * 0.05, half * 0.0);
            let e = c + Vec2::new(half * 0.15, -half * 0.35);
            batch.line(a, b, half * 0.13, bolt);
            batch.line(d, e, half * 0.13, bolt);
        }
        BuildingKind::Pole => {
            // Crossbar on the power pole.
            let bar = s(0.85, 0.85, 0.9);
            batch.line(
                c + Vec2::new(-half * 0.3, 0.0),
                c + Vec2::new(half * 0.3, 0.0),
                half * 0.1,
                bar,
            );
            batch.line(
                c + Vec2::new(0.0, -half * 0.3),
                c + Vec2::new(0.0, half * 0.3),
                half * 0.1,
                bar,
            );
        }
        BuildingKind::Pump => {
            // Water-drop triangle.
            batch.ngon(c, half * 0.3, 3, 0.0, s(0.35, 0.6, 0.85));
        }
        BuildingKind::Tank => {
            // Fluid level line.
            batch.line(
                c + Vec2::new(-half * 0.35, 0.0),
                c + Vec2::new(half * 0.35, 0.0),
                half * 0.15,
                s(0.35, 0.6, 0.85),
            );
        }
        BuildingKind::Lab => {
            // Flask triangle.
            batch.ngon(
                c + Vec2::new(0.0, -half * 0.1),
                half * 0.35,
                3,
                0.0,
                s(0.5, 0.8, 0.7),
            );
        }
        BuildingKind::Turret => {
            // Crosshair + center square.
            let red = s(0.9, 0.2, 0.2);
            batch.line(
                c + Vec2::new(-half * 0.35, 0.0),
                c + Vec2::new(half * 0.35, 0.0),
                half * 0.1,
                red,
            );
            batch.line(
                c + Vec2::new(0.0, -half * 0.35),
                c + Vec2::new(0.0, half * 0.35),
                half * 0.1,
                red,
            );
            batch.ngon(c, half * 0.15, 4, 0.0, red);
        }
        BuildingKind::ForgeCore => {
            // Glowing diamond core.
            batch.ngon(
                c,
                half * 0.35,
                4,
                std::f32::consts::FRAC_PI_4,
                glow(Color::srgb(0.9, 0.4, 0.8), 2.0),
            );
        }
        _ => {}
    }
}

fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    MeshBatch::default().write_to(&mut mesh);
    mesh
}

// ---------------------------------------------------------------- resources

#[derive(Resource)]
pub struct WorldMeshes {
    pub static_mesh: Handle<Mesh>,
    pub dynamic_mesh: Handle<Mesh>,
}

/// Precomputed HDR-boosted item colors so the dynamic mesh doesn't rebuild a
/// palette vector every frame.
#[derive(Resource)]
pub struct ItemPalette(pub Vec<[f32; 4]>);

/// Set to true whenever belts/buildings change so the static mesh rebuilds.
#[derive(Resource)]
pub struct WorldDirty(pub bool);

#[derive(Resource)]
pub struct CameraTarget {
    pub pos: Vec2,
    pub zoom: f32,
}

#[derive(Component)]
pub struct Hud;

#[derive(Component)]
pub struct VictoryOverlay;

// -------------------------------------------------------------------- setup

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    sim: Res<Sim>,
    settings: Res<Settings>,
) {
    // Bloom is opt-in (B key): HDR costs FPS on integrated GPUs, so default off.
    let mut camera = commands.spawn(Camera2dBundle {
        camera: Camera {
            hdr: settings.bloom,
            ..default()
        },
        ..default()
    });
    if settings.bloom {
        camera.insert(BloomSettings::NATURAL);
    }
    commands.insert_resource(CameraTarget {
        pos: Vec2::new(
            sim.0.belt_x.iter().sum::<i32>() as f32 / sim.0.belt_count() as f32 * TILE,
            sim.0.belt_y.iter().sum::<i32>() as f32 / sim.0.belt_count() as f32 * TILE,
        ),
        zoom: 8.0,
    });

    let static_mesh = meshes.add(empty_mesh());
    let dynamic_mesh = meshes.add(empty_mesh());
    let material = materials.add(ColorMaterial::default());

    commands.spawn((
        MaterialMesh2dBundle {
            mesh: Mesh2dHandle(static_mesh.clone()),
            material: material.clone(),
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            ..default()
        },
        NoFrustumCulling,
    ));
    commands.spawn((
        MaterialMesh2dBundle {
            mesh: Mesh2dHandle(dynamic_mesh.clone()),
            material,
            transform: Transform::from_xyz(0.0, 0.0, 1.0),
            ..default()
        },
        NoFrustumCulling,
    ));

    commands.insert_resource(WorldMeshes {
        static_mesh,
        dynamic_mesh,
    });
    commands.insert_resource(ItemPalette(
        ITEM_COLORS.iter().map(|c| glow(*c, 2.2)).collect(),
    ));
    commands.insert_resource(WorldDirty(true));

    commands.spawn((
        TextBundle::from_section(
            "",
            TextStyle {
                font_size: 18.0,
                color: Color::srgb(0.8, 0.85, 0.92),
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(10.0),
            ..default()
        }),
        Hud,
    ));
    commands.spawn(TextBundle::from_section(
        "OUTPOST ZONES  |  NE: Oil Basin  |  SE: Copper Expanse  |  SW: Coal Frontier\nSecure remote mines with turrets; connect them by rail.",
        TextStyle {
            font_size: 13.0,
            color: Color::srgb(0.55, 0.7, 0.82),
            ..default()
        },
    )
    .with_style(Style {
        position_type: PositionType::Absolute,
        left: Val::Px(12.0),
        bottom: Val::Px(92.0),
        ..default()
    }));
    let mut victory_text = TextBundle::from_section(
        "FORGE ASCENSION COMPLETE\nYour factory has forged a new industrial age.",
        TextStyle {
            font_size: 30.0,
            color: Color::srgb(0.95, 0.78, 0.35),
            ..default()
        },
    )
    .with_style(Style {
        position_type: PositionType::Absolute,
        left: Val::Percent(24.0),
        top: Val::Percent(40.0),
        ..default()
    });
    victory_text.visibility = Visibility::Hidden;
    commands.spawn((victory_text, VictoryOverlay));
}

pub fn update_victory_overlay(
    victory: Res<VictoryState>,
    mut overlay: Query<&mut Visibility, With<VictoryOverlay>>,
) {
    if !victory.is_changed() {
        return;
    }
    for mut visibility in overlay.iter_mut() {
        *visibility = if victory.achieved {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

// ------------------------------------------------------------------ systems

/// Rebuild the static world mesh (belts + buildings) when edited.
pub fn rebuild_static_mesh(
    mut dirty: ResMut<WorldDirty>,
    handles: Res<WorldMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    world: Res<GameWorld>,
) {
    if !dirty.0 {
        return;
    }
    dirty.0 = false;

    let mut batch = MeshBatch::default();
    let plate = lin(Color::srgb(0.13, 0.15, 0.19));
    let track = lin(Color::srgb(0.19, 0.22, 0.28));

    // Ore patches.
    for y in 0..world.0.height {
        for x in 0..world.0.width {
            let ore = world.0.ore_at(x, y);
            if ore == 0 {
                continue;
            }
            let c = Vec2::new((x as f32 - 0.5) * TILE, (y as f32 - 0.5) * TILE);
            let ore_color = match ore {
                1 => lin(Color::srgb(0.42, 0.22, 0.10)), // iron
                2 => lin(Color::srgb(0.09, 0.28, 0.42)), // copper
                3 => lin(Color::srgb(0.15, 0.15, 0.16)), // coal
                6 => lin(Color::srgb(0.55, 0.55, 0.53)), // stone
                7 => lin(Color::srgb(0.30, 0.15, 0.40)), // oil
                _ => lin(Color::srgb(0.35, 0.25, 0.18)),
            };
            batch.quad(c, TILE * 0.43, TILE * 0.43, 0.0, ore_color);
        }
    }

    // Subtle tile-grid backdrop.
    let gline = [1.0, 1.0, 1.0, 0.025];
    let (w, h) = (world.0.width as f32, world.0.height as f32);
    let (cx, cy) = ((w - 1.0) * 0.5 * TILE, (h - 1.0) * 0.5 * TILE);
    for gx in 0..world.0.width + 1 {
        let x = (gx as f32 - 0.5) * TILE;
        batch.quad(Vec2::new(x, cy), 0.5, h * TILE * 0.5, 0.0, gline);
    }
    for gy in 0..world.0.height + 1 {
        let y = (gy as f32 - 0.5) * TILE;
        batch.quad(Vec2::new(cx, y), w * TILE * 0.5, 0.5, 0.0, gline);
    }

    for b in 0..sim.0.belt_count() {
        if !sim.0.belt_active[b] {
            continue;
        }
        let c = Vec2::new(sim.0.belt_x[b] as f32 * TILE, sim.0.belt_y[b] as f32 * TILE);
        let angle = dir_angle(sim.0.belt_dir[b]);
        batch.quad(c, TILE * 0.48, TILE * 0.48, 0.0, plate);
        batch.quad(c, TILE * 0.46, TILE * 0.39, angle, track);
    }

    let outline = lin(Color::srgb(0.04, 0.05, 0.06));
    for i in 0..sim.0.bld_x.len() {
        if !sim.0.bld_active[i] {
            continue;
        }
        let c = Vec2::new(sim.0.bld_x[i] as f32 * TILE, sim.0.bld_y[i] as f32 * TILE);
        let angle = dir_angle(sim.0.bld_dir[i]);
        let half = match sim.0.bld_kind[i] {
            BuildingKind::Inserter => TILE * 0.32,
            BuildingKind::Pipe | BuildingKind::Pump => TILE * 0.22,
            BuildingKind::Tank => TILE * 0.45,
            BuildingKind::RailTrack => TILE * 0.15,
            _ => TILE * 0.45,
        };
        // Dark outline makes buildings pop off the belts/ore.
        batch.quad(c, half + TILE * 0.03, half + TILE * 0.03, 0.0, outline);
        let body = match sim.0.bld_kind[i] {
            BuildingKind::Source => lin(Color::srgb(0.16, 0.45, 0.42)),
            BuildingKind::Sink => lin(Color::srgb(0.5, 0.3, 0.14)),
            BuildingKind::Assembler => lin(Color::srgb(0.32, 0.26, 0.45)),
            BuildingKind::Inserter => lin(Color::srgb(0.55, 0.42, 0.12)),
            BuildingKind::Miner => lin(Color::srgb(0.38, 0.18, 0.42)),
            BuildingKind::Storage => lin(Color::srgb(0.22, 0.32, 0.40)),
            BuildingKind::Shipment => lin(Color::srgb(0.20, 0.55, 0.40)),
            BuildingKind::Splitter => lin(Color::srgb(0.55, 0.50, 0.18)),
            BuildingKind::Pole => lin(Color::srgb(0.55, 0.55, 0.60)),
            BuildingKind::Generator => lin(Color::srgb(0.90, 0.80, 0.25)),
            BuildingKind::Pipe => lin(Color::srgb(0.45, 0.45, 0.55)),
            BuildingKind::Pump => lin(Color::srgb(0.25, 0.45, 0.65)),
            BuildingKind::Tank => lin(Color::srgb(0.45, 0.50, 0.55)),
            BuildingKind::Lab => lin(Color::srgb(0.20, 0.55, 0.45)),
            BuildingKind::RailTrack => lin(Color::srgb(0.25, 0.25, 0.28)),
            BuildingKind::RailStation => lin(Color::srgb(0.45, 0.35, 0.25)),
            BuildingKind::Turret => lin(Color::srgb(0.65, 0.25, 0.25)),
            BuildingKind::ForgeCore => lin(Color::srgb(0.85, 0.35, 0.75)),
        };
        batch.quad(c, half, half, 0.0, body);
        // Direction notch on the output/input edge.
        let (dx, dy) = sim.0.bld_dir[i].fvec();
        let notch = c + Vec2::new(dx, dy) * TILE * 0.32;
        batch.quad(notch, TILE * 0.10, TILE * 0.16, angle, lin(Color::srgb(0.9, 0.9, 0.95)));

        // Simple iconic silhouettes so buildings read as machinery.
        draw_building_icon(
            &mut batch,
            c,
            half,
            sim.0.bld_kind[i],
            sim.0.bld_dir[i],
            sim.0.bld_powered[i],
        );

        // Pipe connections between adjacent fluid nodes.
        if sim.0.bld_fluid_capacity[i] > 0 {
            let pipe_color = lin(Color::srgb(0.35, 0.35, 0.45));
            for (dx, dy) in [(1, 0), (0, 1)] {
                let nx = sim.0.bld_x[i] + dx;
                let ny = sim.0.bld_y[i] + dy;
                let nb = world.0.building_at(nx, ny);
                if nb != INVALID {
                    let j = nb as usize;
                    if sim.0.bld_active[j] && sim.0.bld_fluid_capacity[j] > 0 {
                        let nc = Vec2::new(nx as f32 * TILE, ny as f32 * TILE);
                        batch.line(c, nc, TILE * 0.08, pipe_color);
                    }
                }
            }
        }
    }

    // Power wires between nearby power nodes (poles/generators/consumers).
    let wire = lin(Color::srgb(0.35, 0.35, 0.45));
    for i in 0..sim.0.bld_x.len() {
        if !sim.0.bld_active[i] || !is_power_node(sim.0.bld_kind[i]) {
            continue;
        }
        let a = Vec2::new(sim.0.bld_x[i] as f32 * TILE, sim.0.bld_y[i] as f32 * TILE);
        for j in (i + 1)..sim.0.bld_x.len() {
            if !sim.0.bld_active[j] || !is_power_node(sim.0.bld_kind[j]) {
                continue;
            }
            let dx = sim.0.bld_x[i] - sim.0.bld_x[j];
            let dy = sim.0.bld_y[i] - sim.0.bld_y[j];
            if (dx * dx + dy * dy) as f32 > POWER_RADIUS2 {
                continue;
            }
            let b = Vec2::new(sim.0.bld_x[j] as f32 * TILE, sim.0.bld_y[j] as f32 * TILE);
            batch.line(a, b, TILE * 0.02, wire);
        }
    }

    if let Some(mesh) = meshes.get_mut(&handles.static_mesh) {
        batch.write_to(mesh);
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn emit_item(
    sim: &BeltSim,
    batch: &mut MeshBatch,
    i: usize,
    belt: u32,
    alpha: f32,
    t: f32,
    detail: bool,
    palette: &[[f32; 4]],
) {
    let lane = sim.item_lane[i] as usize;
    let cur = item_pos(sim, belt, lane, sim.item_dist[i]);
    let prev = if sim.item_prev_belt[i] != INVALID {
        item_pos(sim, sim.item_prev_belt[i], lane, sim.item_prev_dist[i])
    } else {
        cur
    };
    let pos = prev.lerp(cur, alpha);
    let kind = sim.item_type[i] as usize;
    // Per-item-kind shape and rotation. Sides are capped at 6 to keep the
    // dynamic mesh vertex count low; distinct colors already identify items.
    let (sides, base_rot) = match kind {
        0 => (6u32, 1.0_f32),                        // iron: hexagon/circle
        1 => (4u32, 0.0_f32),                        // copper: square
        2 => (3u32, 0.0_f32),                        // coal: triangle
        3 => (5u32, 0.0_f32),                        // gear: pentagon
        4 => (4u32, std::f32::consts::FRAC_PI_4),    // steel: diamond
        _ => (6u32, kind as f32 * 0.7),              // everything else: hexagon, varied rotation
    };
    let phase = i as f32 * 0.37;
    let pulse = if detail {
        1.0 + (t * 2.0 + phase).sin() * 0.05
    } else {
        1.0
    };
    let size = TILE * 0.14 * pulse;
    let color = palette[kind % palette.len()];
    // Soft outer glow (visible under bloom) + inner core.
    if detail {
        let glow_color = [color[0], color[1], color[2], 0.18];
        batch.ngon(pos, size * 1.7, 6, 0.0, glow_color);
    }
    batch.ngon(pos, size, sides, base_rot, color);
}

/// Rebuild the dynamic mesh (items + chevrons) every frame, camera-culled,
/// with positions interpolated between fixed ticks.
pub fn build_dynamic_mesh(
    handles: Res<WorldMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    world: Res<GameWorld>,
    palette: Res<ItemPalette>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    windows: Query<&Window>,
    cam: Query<(&Transform, &OrthographicProjection), With<Camera>>,
) {
    let Ok((cam_tf, proj)) = cam.get_single() else {
        return;
    };
    let Ok(window) = windows.get_single() else {
        return;
    };
    // Visible world rect with a one-tile margin.
    let half = Vec2::new(window.width(), window.height()) * 0.5 * proj.scale + TILE;
    let center = cam_tf.translation.truncate();
    let min = center - half;
    let max = center + half;

    let alpha = fixed.overstep_fraction();
    let t = time.elapsed_seconds();
    let mut batch = MeshBatch::default();

    // Level of detail: skip decorative overlays when zoomed out.
    let detail = proj.scale < 2.5;

    // Walk only the grid tiles inside the view instead of scanning every
    // item/belt/building in the world.
    let gw = world.0.width;
    let tx0 = ((min.x / TILE).floor() as i32).max(0);
    let tx1 = ((max.x / TILE).ceil() as i32).min(gw - 1);
    let ty0 = ((min.y / TILE).floor() as i32).max(0);
    let ty1 = ((max.y / TILE).ceil() as i32).min(world.0.height - 1);

    let chevron_base = Color::srgb(0.55, 0.65, 0.80);
    let arm_color = lin(Color::srgb(0.85, 0.68, 0.25));

    // Hybrid iteration: a small view walks only visible tiles; a large view
    // scans the item arrays linearly (cache-friendly, no pointer chasing).
    let visible_tiles = ((tx1 - tx0 + 1).max(0) as i64) * ((ty1 - ty0 + 1).max(0) as i64);
    if visible_tiles >= 8192 {
        for i in 0..sim.0.item_capacity() {
            if !sim.0.item_active[i] {
                continue;
            }
            let belt = sim.0.item_belt[i];
            let b = belt as usize;
            let cx = sim.0.belt_x[b] as f32 * TILE;
            let cy = sim.0.belt_y[b] as f32 * TILE;
            if cx < min.x || cx > max.x || cy < min.y || cy > max.y {
                continue;
            }
            emit_item(&sim.0, &mut batch, i, belt, alpha, t, detail, &palette.0);
        }
        if let Some(mesh) = meshes.get_mut(&handles.dynamic_mesh) {
            batch.write_to(mesh);
        }
        return;
    }

    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            // ---- belt on this tile: chevrons + items ----
            let belt = world.0.belt_at(tx, ty);
            if belt != INVALID && sim.0.belt_active[belt as usize] {
                let b = belt as usize;
                let c = Vec2::new(sim.0.belt_x[b] as f32 * TILE, sim.0.belt_y[b] as f32 * TILE);
                if detail {
                    let angle = dir_angle(sim.0.belt_dir[b]);
                    let (dx, dy) = sim.0.belt_dir[b].fvec();
                    for k in 0..2u32 {
                        let s = (t * 1.4 + k as f32 * 0.5).fract();
                        let pos = c + Vec2::new(dx, dy) * (s - 0.5) * TILE * 0.8;
                        let fade = (s * (1.0 - s) * 4.0).clamp(0.0, 1.0);
                        batch.quad(pos, TILE * 0.04, TILE * 0.27, angle, lin_a(chevron_base, 0.35 * fade));
                    }
                }
                for lane in 0..crate::belts::LANES {
                    let mut cur_item = sim.0.belt_head[b][lane];
                    while cur_item != INVALID {
                        let i = cur_item as usize;
                        emit_item(&sim.0, &mut batch, i, belt, alpha, t, detail, &palette.0);
                        cur_item = sim.0.item_behind[i];
                    }
                }
            }

            // ---- inserter arm on this tile ----
            if detail {
                let bld = world.0.building_at(tx, ty);
                if bld != INVALID
                    && sim.0.bld_active[bld as usize]
                    && sim.0.bld_kind[bld as usize] == BuildingKind::Inserter
                {
                    let s = bld as usize;
                    let c = Vec2::new(sim.0.bld_x[s] as f32 * TILE, sim.0.bld_y[s] as f32 * TILE);
                    let base = dir_angle(sim.0.bld_dir[s]);
                    // Swing PI as the cooldown elapses; holding = ahead, empty = behind.
                    let swing = sim.0.bld_timer[s] as f32 / INSERTER_COOLDOWN as f32;
                    let angle = if sim.0.bld_held[s] != 0 {
                        base + std::f32::consts::PI * (1.0 - swing)
                    } else {
                        base + std::f32::consts::PI * swing
                    };
                    let tip = c + Vec2::from_angle(angle) * TILE * 0.22;
                    batch.quad(tip, TILE * 0.20, TILE * 0.06, angle, arm_color);
                    if sim.0.bld_held[s] != 0 {
                        let kind = (sim.0.bld_held[s] - 1) as usize;
                        let hand = c + Vec2::from_angle(angle) * TILE * 0.40;
                        batch.quad(
                            hand,
                            TILE * 0.10,
                            TILE * 0.10,
                            angle,
                            glow(ITEM_COLORS[kind % ITEM_COLORS.len()], 2.2),
                        );
                    }
                }
            }
            // ---- power status overlay ----
            if detail {
                let bld = world.0.building_at(tx, ty);
                if bld != INVALID
                    && sim.0.bld_active[bld as usize]
                    && is_consumer(sim.0.bld_kind[bld as usize])
                    && !sim.0.bld_powered[bld as usize]
                {
                    let s = bld as usize;
                    let c = Vec2::new(sim.0.bld_x[s] as f32 * TILE, sim.0.bld_y[s] as f32 * TILE);
                    batch.quad(c, TILE * 0.07, TILE * 0.07, 0.0, lin(Color::srgb(0.95, 0.15, 0.15)));
                }
            }

            // ---- fluid fill overlay ----
            if detail {
                let bld = world.0.building_at(tx, ty);
                if bld != INVALID && sim.0.bld_active[bld as usize] {
                    let s = bld as usize;
                    let cap = sim.0.bld_fluid_capacity[s];
                    if cap > 0 {
                        let vol = sim.0.bld_fluid_volume[s];
                        let ratio = (vol / cap as f32).clamp(0.0, 1.0);
                        let c = Vec2::new(sim.0.bld_x[s] as f32 * TILE, sim.0.bld_y[s] as f32 * TILE);
                        let mut water = lin(Color::srgb(0.15, 0.55, 0.95));
                        water[3] *= ratio;
                        let size = match sim.0.bld_kind[s] {
                            BuildingKind::Tank => TILE * 0.35,
                            _ => TILE * 0.12,
                        };
                        batch.quad(c, size, size, 0.0, water);
                    }
                }
            }
        }
    }

    if let Some(mesh) = meshes.get_mut(&handles.dynamic_mesh) {
        batch.write_to(mesh);
    }
}

/// Smooth camera: WASD/arrows pan, wheel zoom, exponential easing.
pub fn camera_control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: EventReader<MouseWheel>,
    mut target: ResMut<CameraTarget>,
    mut q: Query<(&mut Transform, &mut OrthographicProjection), With<Camera>>,
) {
    let dt = time.delta_seconds();
    let mut pan = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        pan.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        pan.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        pan.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        pan.x += 1.0;
    }
    let zoom = target.zoom;
    target.pos += pan.normalize_or_zero() * 600.0 * zoom * dt;

    for ev in wheel.read() {
        // Clamp each wheel event so trackpads don't leap multiple levels at once.
        let delta = ev.y.clamp(-1.0, 1.0);
        target.zoom = (target.zoom * (1.0 - delta * 0.08)).clamp(0.5, 10.0);
    }

    let ease = 1.0 - (-10.0 * dt).exp();
    if let Ok((mut tf, mut proj)) = q.get_single_mut() {
        let cur = tf.translation.truncate();
        let next = cur.lerp(target.pos, ease);
        tf.translation = next.extend(tf.translation.z);
        proj.scale += (target.zoom - proj.scale) * ease;
    }
}

/// Toggle bloom with B (to measure its GPU cost).
pub fn toggle_bloom(
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut cam: Query<(Entity, &mut Camera, Option<&BloomSettings>), With<Camera>>,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        if let Ok((e, mut camera, bloom)) = cam.get_single_mut() {
            if bloom.is_some() {
                camera.hdr = false;
                commands.entity(e).remove::<BloomSettings>();
                settings.bloom = false;
            } else {
                camera.hdr = true;
                commands.entity(e).insert(BloomSettings::NATURAL);
                settings.bloom = true;
            }
        }
    }
}

pub fn update_hud(
    sim: Res<Sim>,
    editor: Res<EditorState>,
    selection: Res<Selection>,
    blueprint: Res<Blueprint>,
    diagnostics: Res<DiagnosticsStore>,
    player: Res<PlayerState>,
    contract: Res<ContractState>,
    stats: Res<ProductionStats>,
    combat: Res<CombatState>,
    mut q: Query<&mut Text, With<Hud>>,
    mut counter: Local<u32>,
) {
    *counter += 1;
    // HUD text layout is expensive; refresh at ~10 Hz instead of every frame.
    if *counter % 6 != 0 {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let inspect = if let Some(b) = selection.building {
        let i = b as usize;
        let fluid = if sim.0.bld_fluid_capacity[i] > 0 {
            format!(
                " fluid:{:.2}/{} ready:{}",
                sim.0.bld_fluid_volume[i],
                sim.0.bld_fluid_capacity[i],
                sim.0.bld_fluid_ready[i] as u8
            )
        } else {
            String::new()
        };
        let recipe = if sim.0.bld_kind[i] == BuildingKind::Assembler {
            RECIPES
                .get(sim.0.bld_param[i] as usize)
                .map(|r| r.name)
                .unwrap_or("none")
        } else {
            ""
        };
        let held = if sim.0.bld_held[i] == 0 {
            "none".to_string()
        } else {
            ITEM_NAMES[(sim.0.bld_held[i] as usize - 1) % ITEM_NAMES.len()].to_string()
        };
        let inventory = |inv: &[u16]| -> String {
            inv.iter()
                .enumerate()
                .filter(|(_, &c)| c > 0)
                .map(|(k, &c)| format!("{}:{}", ITEM_NAMES[k], c))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!(
            "sel: {:?} @({},{}) timer:{} held:{} in:{} out:{} recipe:{} param:{} delivered:{}{}",
            sim.0.bld_kind[i],
            sim.0.bld_x[i],
            sim.0.bld_y[i],
            sim.0.bld_timer[i],
            held,
            inventory(&sim.0.bld_in[i]),
            inventory(&sim.0.bld_out[i]),
            recipe,
            sim.0.bld_param[i],
            sim.0.bld_delivered[i],
            fluid
        )
    } else if !blueprint.tiles.is_empty() {
        format!("clipboard: {} tiles {}x{}", blueprint.tiles.len(), blueprint.width, blueprint.height)
    } else if let (Some(s), Some(e)) = (selection.start, selection.end) {
        format!("selection: ({}, {}) -> ({}, {})", s.0, s.1, e.0, e.1)
    } else {
        String::new()
    };
    let recipe_hint = if editor.tool == crate::ui::Tool::Assembler {
        RECIPES
            .get(editor.recipe as usize)
            .map(|r| format!(" recipe:{}", r.name))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let most_shipped = stats
        .shipped
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| **count > 0)
        .map(|(kind, count)| format!("  top ship: {} {}", ITEM_NAMES[kind], count))
        .unwrap_or_default();
    let forge_progress = (0..sim.0.bld_x.len())
        .find(|&i| sim.0.bld_active[i] && sim.0.bld_kind[i] == BuildingKind::ForgeCore)
        .map(|i| format!("  forge: stage {} delivery {}", sim.0.bld_param[i] + 1, sim.0.bld_delivered[i]))
        .unwrap_or_default();
    if let Ok(mut text) = q.get_single_mut() {
        text.sections[0].value = format!(
            "credits: ${}  research: {}  items: {}  fps: {:.0}  tool: {} ({:?}){}\ncontract: {} {}/{}  completed: {}{}\nthreat: {:.1}  wave: {}{}\n{}",
            player.credits,
            player.research_points,
            sim.0.active_item_count(),
            fps,
            editor.tool_name(),
            editor.dir,
            recipe_hint,
            ITEM_NAMES[contract.item_kind as usize % ITEM_NAMES.len()],
            contract.delivered,
            contract.required,
            contract.completed,
            most_shipped,
            combat.threat,
            combat.wave,
            forge_progress,
            inspect,
        );
    }
}
