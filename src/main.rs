mod belts;
mod combat;
mod economy;
mod grid;
mod rail;
mod render;
mod settings;
mod settings_ui;
mod sim;
mod ui;
mod replay;
mod telemetry;
mod audio;
mod sprites;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::window::{WindowMode, WindowResolution};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use belts::{BeltSim, BuildingKind, Dir, KINDS};
use crate::economy::{
    is_tech_unlocked, item_value, shipment_value, unlock_tech, ContractState, PlayerState,
    ProductionStats, Tech, VictoryState,
};
use grid::{Grid, CHUNK_SIZE};
use render::{CameraTarget, TILE};

/// The whole simulation lives in one flat-array resource.
#[derive(Resource)]
pub struct Sim(pub BeltSim);

#[derive(Resource)]
pub struct GameWorld(pub Grid);

#[derive(Default)]
struct FluidScratch {
    roots: Vec<u32>,
    cap: Vec<f32>,
    vol: Vec<f32>,
    prod: Vec<f32>,
    cons: Vec<f32>,
    ready: Vec<bool>,
    net_count: usize,
}

fn setup_paths() {
    let project_root = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let _ = std::env::set_current_dir(&project_root);

    if std::env::var("BEVY_ASSET_ROOT").is_err() {
        std::env::set_var("BEVY_ASSET_ROOT", &project_root);
    }
}

fn main() {
    setup_paths();
    audio::ensure_audio_assets();

    let game_settings = settings::load();
    let (sim, grid) = build_demo_world();

    let mut history = ui::History::default();
    history.push(&sim, &grid);

    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Factory Forge".into(),
                    resolution: WindowResolution::new(
                        game_settings.window_width as f32,
                        game_settings.window_height as f32,
                    ),
                    mode: if game_settings.fullscreen {
                        WindowMode::BorderlessFullscreen
                    } else {
                        WindowMode::Windowed
                    },
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin)
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.085)))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(Sim(sim))
        .insert_resource(GameWorld(grid))
        .insert_resource(history)
        .insert_resource(economy::PlayerState::with_starting_funds())
        .init_resource::<ContractState>()
        .init_resource::<ProductionStats>()
        .init_resource::<VictoryState>()
        .insert_resource(game_settings)
        .init_resource::<ui::EditorState>()
        .init_resource::<ui::Selection>()
        .init_resource::<ui::Blueprint>()
        .init_resource::<ui::Hotbar>()
        .init_resource::<ui::BuildMenu>()
        .init_resource::<rail::RailNetwork>()
        .init_resource::<combat::CombatState>()
        .init_resource::<replay::ReplayLog>()
        .init_resource::<telemetry::Telemetry>()
        .init_resource::<telemetry::GraphVisible>()
        .init_resource::<settings_ui::SettingsMenuVisible>()
        .init_resource::<audio::SfxQueue>()
        .add_systems(Startup, (sprites::setup_sprites, setup_main_menu, render::setup_scene, ui::setup_ghost, ui::setup_hotbar, telemetry::setup_graph, settings_ui::setup_settings_ui, audio::setup_audio))
        .add_systems(
            FixedUpdate,
            (run_sim, telemetry::record, replay::record)
                .chain()
                .run_if(in_state(AppState::Playing)),
        )
        .add_systems(Update, (menu_input, update_main_menu_visibility))
        .add_systems(
            Update,
            (
                ui::handle_menu_input,
                ui::handle_menu_clicks,
                ui::handle_menu_contracts,
                ui::handle_menu_unlocks,
                ui::handle_hotbar_clicks,
                ui::handle_editor_input,
                ui::save_load,
                ui::update_hotbar,
                ui::update_tool_info,
            )
                .chain()
                .in_set(UpdateInputSet)
                .run_if(in_state(AppState::Playing)),
        )
        .add_systems(
            Update,
            (
                render::rebuild_static_mesh,
                render::build_dynamic_mesh,
                render::camera_control,
                render::toggle_bloom,
                settings_ui::toggle_settings_ui,
                settings_ui::update_settings_ui,
                settings_ui::apply_settings,
                audio::play_dirty_sfx,
                audio::play_sfx,
                audio::update_volumes,
                render::update_hud,
                render::update_victory_overlay,
                unlock_creative_on_victory,
                rail::update_train_visuals,
                combat::update_enemy_visuals,
                telemetry::toggle_graph,
                telemetry::update_graph_overlay,
            )
                .chain()
                .in_set(UpdateOutputSet)
                .after(UpdateInputSet)
                .run_if(in_state(AppState::Playing)),
        )
        .add_systems(Update, settings::save_system)
        .run();
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct UpdateInputSet;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct UpdateOutputSet;

#[derive(States, Clone, Copy, Default, Eq, PartialEq, Hash, Debug)]
pub enum AppState {
    #[default]
    MainMenu,
    Playing,
}

#[derive(Component)]
struct MainMenuOverlay;

fn setup_main_menu(mut commands: Commands) {
    commands.spawn((
        TextBundle::from_section(
            "FACTORY FORGE\n\nPress Enter to start\nPress Esc to return here\nPress Q to quit",
            TextStyle {
                font_size: 32.0,
                color: Color::srgb(0.9, 0.92, 0.96),
                ..default()
            },
        )
        .with_text_justify(JustifyText::Center)
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Percent(30.0),
            left: Val::Percent(25.0),
            width: Val::Percent(50.0),
            ..default()
        }),
        MainMenuOverlay,
    ));
}

fn menu_input(
    state: Res<State<AppState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: EventWriter<AppExit>,
) {
    if *state.get() == AppState::MainMenu {
        if keys.just_pressed(KeyCode::Enter) {
            next_state.set(AppState::Playing);
        }
        if keys.just_pressed(KeyCode::KeyQ) {
            exit.send(AppExit::Success);
        }
    } else if *state.get() == AppState::Playing {
        if keys.just_pressed(KeyCode::Escape) {
            next_state.set(AppState::MainMenu);
        }
    }
}

fn update_main_menu_visibility(
    state: Res<State<AppState>>,
    mut query: Query<&mut Visibility, With<MainMenuOverlay>>,
) {
    let target = if *state.get() == AppState::MainMenu {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in query.iter_mut() {
        *vis = target;
    }
}

fn run_sim(
    mut sim: ResMut<Sim>,
    mut world: ResMut<GameWorld>,
    target: Res<CameraTarget>,
    mut rail: ResMut<rail::RailNetwork>,
    mut combat: ResMut<combat::CombatState>,
    mut player: ResMut<PlayerState>,
    mut contract: ResMut<ContractState>,
    mut stats: ResMut<ProductionStats>,
    mut victory: ResMut<VictoryState>,
    mut world_dirty: ResMut<render::WorldDirty>,
    mut active_belts: Local<Vec<usize>>,
    mut active_blds: Local<Vec<usize>>,
    mut fluids: Local<FluidScratch>,
) {
    let cx = (target.pos.x / (TILE * CHUNK_SIZE as f32)).floor() as i32;
    let cy = (target.pos.y / (TILE * CHUNK_SIZE as f32)).floor() as i32;
    let min_chunk_x = cx - 1;
    let max_chunk_x = cx + 1;
    let min_chunk_y = cy - 1;
    let max_chunk_y = cy + 1;
    active_belts.clear();
    active_belts.extend((0..sim.0.belt_count()).filter(|&b| {
        let (x, y) = sim.0.belt_chunk[b];
        sim.0.belt_active[b]
            && x >= min_chunk_x
            && x <= max_chunk_x
            && y >= min_chunk_y
            && y <= max_chunk_y
    }));
    active_blds.clear();
    active_blds.extend((0..sim.0.bld_x.len()).filter(|&s| {
        let (x, y) = sim.0.bld_chunk[s];
        sim.0.bld_active[s]
            && x >= min_chunk_x
            && x <= max_chunk_x
            && y >= min_chunk_y
            && y <= max_chunk_y
    }));
    let net_count = if sim.0.dirty_power {
        sim::rebuild_power(&mut sim.0, &active_blds);
        let count = sim::rebuild_fluid_networks(&mut sim.0, &active_blds, &mut fluids.roots);
        sim.0.dirty_power = false;
        fluids.net_count = count;
        count
    } else {
        fluids.net_count
    };
    fluids.cap.resize(net_count, 0.0);
    fluids.vol.resize(net_count, 0.0);
    fluids.prod.resize(net_count, 0.0);
    fluids.cons.resize(net_count, 0.0);
    fluids.ready.resize(net_count, false);
    let FluidScratch {
        cap,
        vol,
        prod,
        cons,
        ready,
        ..
    } = &mut *fluids;
    sim::tick_fluids(
        &mut sim.0,
        &active_blds,
        net_count,
        cap,
        vol,
        prod,
        cons,
        ready,
    );
    sim::tick_buildings(&mut sim.0, &world.0, &active_blds);
    sim::tick(&mut sim.0, &active_belts);

    if sim.0.dirty_rail {
        rail.rebuild(&sim.0, &active_blds);
        sim.0.dirty_rail = false;
    }
    rail.tick_trains(&mut sim.0);
    let factory_load = active_blds
        .iter()
        .filter(|&&s| {
            sim.0.bld_powered[s]
                && matches!(
                    sim.0.bld_kind[s],
                    BuildingKind::Assembler | BuildingKind::Miner | BuildingKind::Generator | BuildingKind::Lab
                )
        })
        .count();
    if combat.tick(
        &mut sim.0,
        &mut world.0,
        &active_blds,
        factory_load,
        is_tech_unlocked(player.tech_flags, Tech::Combat),
    ) {
        world_dirty.0 = true;
    }

    if !victory.achieved
        && active_blds.iter().any(|&s| {
            sim.0.bld_active[s]
                && sim.0.bld_kind[s] == BuildingKind::ForgeCore
                && sim.0.bld_param[s] >= 3
        })
    {
        victory.achieved = true;
    }

    // Economy: sinks sell stored items, shipments pay for target deliveries,
    // and research centers convert consumed fuel into research points.
    for &s in active_blds.iter() {
        if !sim.0.bld_active[s] {
            continue;
        }
        match sim.0.bld_kind[s] {
            BuildingKind::Sink => {
                for k in 0..KINDS {
                    let n = sim.0.bld_in[s][k];
                    if n > 0 {
                        player.credits += n as i32 * item_value(k as u16);
                        stats.record_sale(k, n);
                        sim.0.bld_in[s][k] = 0;
                    }
                }
            }
            BuildingKind::Shipment => {
                let target = sim.0.bld_param[s] as usize;
                let delivered = sim.0.bld_delivered[s];
                if delivered > 0 && target < KINDS {
                    player.credits += delivered as i32 * shipment_value(target as u16);
                    stats.record_shipment(target, delivered);
                    contract.record_delivery(target as u16, delivered, &mut player);
                    sim.0.bld_delivered[s] = 0;
                }
            }
            BuildingKind::Lab => {
                let points = sim.0.bld_delivered[s];
                if points > 0 {
                    player.research_points += points as i32;
                    sim.0.bld_delivered[s] = 0;
                }
            }
            _ => {}
        }
    }
}

/// Generate a deterministic world with ore patches and a starter factory
/// that demonstrates the full production loop.
fn build_demo_world() -> (BeltSim, Grid) {
    let mut rng = StdRng::seed_from_u64(0xF4C70_1DEA);
    let mut sim = BeltSim::default();
    let mut grid = Grid::new(160, 160);

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

    // Starter power so the factory actually runs.
    let gen = add(&mut sim, &mut grid, 10, 12, Dir::East, belts::BuildingKind::Generator);
    sim.bld_param[gen as usize] = 20;
    let _pole = add(&mut sim, &mut grid, 16, 11, Dir::East, belts::BuildingKind::Pole);

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
    stamp_ore_zone(grid, 130, 26, 7, 7);
}

/// Beating the game unlocks Creative/Sandbox tools (Source and Sink).
fn unlock_creative_on_victory(
    mut player: ResMut<economy::PlayerState>,
    victory: Res<VictoryState>,
) {
    if victory.achieved && !is_tech_unlocked(player.tech_flags, Tech::Creative) {
        unlock_tech(&mut player.tech_flags, Tech::Creative);
        info!("Creative / sandbox mode unlocked!");
    }
}

/// Place a circular ore patch.
fn stamp_ore_zone(grid: &mut Grid, cx: i32, cy: i32, radius: i32, kind: u8) {
    for y in (cy - radius)..=(cy + radius) {
        for x in (cx - radius)..=(cx + radius) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= radius * radius {
                grid.set_ore(x, y, kind);
            }
        }
    }
}
