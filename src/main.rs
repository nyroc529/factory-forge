mod belts;
mod grid;
mod render;
mod sim;
mod ui;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashSet;

use belts::{BeltSim, Dir};
use grid::{Grid, CHUNK_SIZE};
use render::{CameraTarget, TILE};

/// The whole simulation lives in one flat-array resource.
#[derive(Resource)]
pub struct Sim(pub BeltSim);

#[derive(Resource)]
pub struct GameWorld(pub Grid);

fn main() {
    let (sim, grid) = build_demo_world();

    let mut history = ui::History::default();
    history.push(&sim, &grid);

    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Factory Forge".into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin)
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.085)))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(Sim(sim))
        .insert_resource(GameWorld(grid))
        .insert_resource(history)
        .init_resource::<ui::EditorState>()
        .init_resource::<ui::Selection>()
        .init_resource::<ui::Blueprint>()
        .add_systems(Startup, (render::setup_scene, ui::setup_ghost))
        .add_systems(FixedUpdate, run_sim)
        .add_systems(
            Update,
            (
                ui::handle_editor_input,
                ui::save_load,
                render::rebuild_static_mesh,
                render::build_dynamic_mesh,
                render::camera_control,
                render::toggle_bloom,
                render::update_hud,
            )
                .chain(),
        )
        .run();
}

fn run_sim(
    mut sim: ResMut<Sim>,
    world: Res<GameWorld>,
    target: Res<CameraTarget>,
    mut active_chunks: Local<HashSet<(i32, i32)>>,
    mut active_belts: Local<Vec<usize>>,
    mut active_blds: Local<Vec<usize>>,
    mut fluid_roots: Local<Vec<u32>>,
    mut fluid_cap: Local<Vec<f32>>,
    mut fluid_vol: Local<Vec<f32>>,
    mut fluid_prod: Local<Vec<f32>>,
    mut fluid_cons: Local<Vec<f32>>,
    mut fluid_ready: Local<Vec<bool>>,
    mut fluid_net_count: Local<usize>,
) {
    active_chunks.clear();
    let cx = (target.pos.x / (TILE * CHUNK_SIZE as f32)).floor() as i32;
    let cy = (target.pos.y / (TILE * CHUNK_SIZE as f32)).floor() as i32;
    for dy in -1..=1 {
        for dx in -1..=1 {
            active_chunks.insert((cx + dx, cy + dy));
        }
    }
    active_belts.clear();
    active_belts.extend(
        (0..sim.0.belt_count())
            .filter(|&b| sim.0.belt_active[b] && active_chunks.contains(&sim.0.belt_chunk[b])),
    );
    active_blds.clear();
    active_blds.extend(
        (0..sim.0.bld_x.len())
            .filter(|&s| sim.0.bld_active[s] && active_chunks.contains(&sim.0.bld_chunk[s])),
    );
    let net_count = if sim.0.dirty_power {
        sim::rebuild_power(&mut sim.0, &active_blds);
        let count = sim::rebuild_fluid_networks(&mut sim.0, &active_blds, &mut fluid_roots);
        sim.0.dirty_power = false;
        *fluid_net_count = count;
        count
    } else {
        *fluid_net_count
    };
    fluid_cap.resize(net_count, 0.0);
    fluid_vol.resize(net_count, 0.0);
    fluid_prod.resize(net_count, 0.0);
    fluid_cons.resize(net_count, 0.0);
    fluid_ready.resize(net_count, false);
    sim::tick_fluids(
        &mut sim.0,
        &active_blds,
        net_count,
        &mut fluid_cap,
        &mut fluid_vol,
        &mut fluid_prod,
        &mut fluid_cons,
        &mut fluid_ready,
    );
    sim::tick_buildings(&mut sim.0, &world.0, &active_blds);
    sim::tick(&mut sim.0, &active_belts);
}

/// Generate a deterministic world with ore patches and a starter factory
/// that demonstrates the full production loop.
fn build_demo_world() -> (BeltSim, Grid) {
    let mut rng = StdRng::seed_from_u64(0xF4C70_1DEA);
    let mut sim = BeltSim::default();
    let mut grid = Grid::new(120, 120);

    generate_ore_patches(&mut grid, &mut rng);

    // Starter factory at (10, 10):
    // ore (10,10) -> miner (10,10) -> belt -> inserter -> assembler (gear)
    // -> inserter -> belt -> inserter -> storage -> inserter -> shipment
    let b = |sim: &mut BeltSim, grid: &mut Grid, x: i32, y: i32, dir: Dir| {
        let id = sim.add_belt(x, y, dir);
        grid.set_belt(x, y, id);
    };
    let add = |sim: &mut BeltSim, grid: &mut Grid, x: i32, y: i32, dir: Dir, kind: belts::BuildingKind| {
        let id = sim.add_building(x, y, dir, kind);
        grid.set_building(x, y, id);
        id
    };

    // Ore under miner: amber (kind 0).
    grid.set_ore(10, 10, 1);

    // Belt run from miner output to assembler input.
    b(&mut sim, &mut grid, 11, 10, Dir::East);
    b(&mut sim, &mut grid, 12, 10, Dir::East);
    // Inserter feeds assembler.
    let _ins1 = add(&mut sim, &mut grid, 13, 10, Dir::East, belts::BuildingKind::Inserter);
    let asm = add(&mut sim, &mut grid, 14, 10, Dir::East, belts::BuildingKind::Assembler);
    sim.bld_param[asm as usize] = 1; // gear: 2 amber -> 1 rose
    // Inserter pulls gear out.
    let _ins2 = add(&mut sim, &mut grid, 15, 10, Dir::East, belts::BuildingKind::Inserter);
    // Belt run to storage.
    b(&mut sim, &mut grid, 16, 10, Dir::East);
    b(&mut sim, &mut grid, 17, 10, Dir::East);
    // Inserter feeds storage.
    let _ins3 = add(&mut sim, &mut grid, 18, 10, Dir::East, belts::BuildingKind::Inserter);
    let _stor = add(&mut sim, &mut grid, 19, 10, Dir::East, belts::BuildingKind::Storage);
    // Inserter feeds shipment.
    let _ins4 = add(&mut sim, &mut grid, 20, 10, Dir::East, belts::BuildingKind::Inserter);
    let ship = add(&mut sim, &mut grid, 21, 10, Dir::East, belts::BuildingKind::Shipment);
    sim.bld_param[ship as usize] = 3; // target: rose (gear output)
    sim.bld_delivered[ship as usize] = 0;

    // Miner on ore.
    let _miner = add(&mut sim, &mut grid, 10, 10, Dir::East, belts::BuildingKind::Miner);

    sim::rebuild_belt_graph(&mut sim, &grid);
    (sim, grid)
}

/// Place circular ore patches for the five raw resources.
fn generate_ore_patches(grid: &mut Grid, rng: &mut StdRng) {
    // Stored ore kind is item_index + 1: 1 iron, 2 copper, 3 coal, 6 stone, 7 oil.
    let kinds = [1u8, 2, 3, 6, 7];
    for _ in 0..12 {
        let cx: i32 = rng.gen_range(10..(grid.width - 10));
        let cy: i32 = rng.gen_range(10..(grid.height - 10));
        let kind = kinds[rng.gen_range(0..kinds.len())];
        let radius: f32 = rng.gen_range(3.0..6.0);
        for y in (cy - radius as i32 - 1)..(cy + radius as i32 + 1) {
            for x in (cx - radius as i32 - 1)..(cx + radius as i32 + 1) {
                if x < 1 || y < 1 || x >= grid.width - 1 || y >= grid.height - 1 {
                    continue;
                }
                let dx = x - cx;
                let dy = y - cy;
                let d2 = (dx * dx + dy * dy) as f32;
                if d2 <= radius * radius && rng.gen::<f32>() > 0.25 {
                    grid.set_ore(x, y, kind);
                }
            }
        }
    }
    // Ensure the starter patch at (10,10) is iron.
    for y in 8..=12 {
        for x in 8..=12 {
            if (x - 10) * (x - 10) + (y - 10) * (y - 10) <= 8 {
                grid.set_ore(x, y, 1);
            }
        }
    }
}
