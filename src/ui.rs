//! Editor: place belts/sources/sinks with the mouse, rotate with R,
//! erase with right-click. A ghost sprite previews the placement.

use bevy::prelude::*;

use crate::belts::{BuildingKind, Dir, INVALID};
use crate::render::{dir_angle, WorldDirty, TILE};
use crate::sim::rebuild_belt_graph;
use crate::{GameWorld, Sim};

/// Inspector / selection-box state.
#[derive(Resource, Default)]
pub struct Selection {
    pub start: Option<(i32, i32)>,
    pub end: Option<(i32, i32)>,
    pub building: Option<u32>,
}

/// Undo/redo snapshots of the whole world.
#[derive(Resource, Default)]
pub struct History {
    pub states: Vec<(crate::belts::BeltSim, crate::grid::Grid)>,
    pub idx: usize,
}

impl History {
    pub fn push(&mut self, sim: &crate::belts::BeltSim, grid: &crate::grid::Grid) {
        self.states.truncate(self.idx + 1);
        self.states.push((sim.clone(), grid.clone()));
        self.idx += 1;
    }
    pub fn can_undo(&self) -> bool {
        self.idx > 0
    }
    pub fn can_redo(&self) -> bool {
        self.idx + 1 < self.states.len()
    }
    pub fn undo(&mut self) -> Option<&(crate::belts::BeltSim, crate::grid::Grid)> {
        if self.can_undo() {
            self.idx -= 1;
            self.states.get(self.idx)
        } else {
            None
        }
    }
    pub fn redo(&mut self) -> Option<&(crate::belts::BeltSim, crate::grid::Grid)> {
        if self.can_redo() {
            self.idx += 1;
            self.states.get(self.idx)
        } else {
            None
        }
    }
}

/// A copied set of tiles ready to be pasted as a blueprint.
#[derive(Clone)]
pub struct BlueprintTile {
    pub dx: i32,
    pub dy: i32,
    pub is_belt: bool,
    pub dir: Dir,
    pub kind: BuildingKind,
    pub param: u16,
}

#[derive(Resource, Default)]
pub struct Blueprint {
    pub tiles: Vec<BlueprintTile>,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Belt,
    Source,
    Sink,
    Assembler,
    Inserter,
    Miner,
    Storage,
    Shipment,
    Splitter,
    Select,
    Paste,
    Pole,
    Generator,
    Pipe,
    Pump,
    Tank,
    Lab,
}

#[derive(Resource)]
pub struct EditorState {
    pub tool: Tool,
    pub dir: Dir,
    /// Recipe index to use for newly placed assemblers.
    pub recipe: u16,
    /// Last tile a belt was painted on during the current drag.
    pub last_tile: Option<(i32, i32)>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            tool: Tool::Belt,
            dir: Dir::East,
            recipe: 0,
            last_tile: None,
        }
    }
}

impl EditorState {
    pub fn tool_name(&self) -> &'static str {
        match self.tool {
            Tool::Belt => "belt",
            Tool::Source => "source",
            Tool::Sink => "sink",
            Tool::Assembler => "assembler",
            Tool::Inserter => "inserter",
            Tool::Miner => "miner",
            Tool::Storage => "storage",
            Tool::Shipment => "shipment",
            Tool::Splitter => "splitter",
            Tool::Select => "select",
            Tool::Paste => "paste",
            Tool::Pole => "pole",
            Tool::Generator => "generator",
            Tool::Pipe => "pipe",
            Tool::Pump => "pump",
            Tool::Tank => "tank",
            Tool::Lab => "lab",
        }
    }
}

fn delta_dir(from: (i32, i32), to: (i32, i32)) -> Option<Dir> {
    match (to.0 - from.0, to.1 - from.1) {
        (1, 0) => Some(Dir::East),
        (-1, 0) => Some(Dir::West),
        (0, 1) => Some(Dir::North),
        (0, -1) => Some(Dir::South),
        _ => None,
    }
}

#[derive(Component)]
pub struct Ghost;

pub fn setup_ghost(mut commands: Commands) {
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgba(0.5, 0.9, 0.6, 0.35),
                custom_size: Some(Vec2::splat(TILE * 0.9)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 2.0),
            visibility: Visibility::Hidden,
            ..default()
        },
        Ghost,
    ));
}

fn cursor_tile(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<(i32, i32, Vec2)> {
    let cursor = window.cursor_position()?;
    let world = camera.viewport_to_world_2d(cam_tf, cursor)?;
    let tx = (world.x / TILE).round() as i32;
    let ty = (world.y / TILE).round() as i32;
    Some((tx, ty, world))
}

pub fn handle_editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform)>,
    mut editor: ResMut<EditorState>,
    mut sim: ResMut<Sim>,
    mut world: ResMut<GameWorld>,
    mut dirty: ResMut<WorldDirty>,
    mut history: ResMut<History>,
    mut selection: ResMut<Selection>,
    mut blueprint: ResMut<Blueprint>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Sprite), With<Ghost>>,
) {
    if keys.just_pressed(KeyCode::Digit0) {
        editor.tool = Tool::Select;
    }
    if keys.just_pressed(KeyCode::Digit1) {
        editor.tool = Tool::Belt;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        editor.tool = Tool::Source;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        editor.tool = Tool::Sink;
    }
    if keys.just_pressed(KeyCode::Digit4) {
        editor.tool = Tool::Assembler;
    }
    if keys.just_pressed(KeyCode::Digit5) {
        editor.tool = Tool::Inserter;
    }
    if keys.just_pressed(KeyCode::Digit6) {
        editor.tool = Tool::Miner;
    }
    if keys.just_pressed(KeyCode::Digit7) {
        editor.tool = Tool::Storage;
    }
    if keys.just_pressed(KeyCode::Digit8) {
        editor.tool = Tool::Shipment;
    }
    if keys.just_pressed(KeyCode::Digit9) {
        editor.tool = Tool::Splitter;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        if editor.tool == Tool::Assembler {
            editor.recipe = ((editor.recipe as usize + 1) % crate::sim::RECIPES.len()) as u16;
        } else if editor.tool == Tool::Select {
            if let Some(b) = selection.building {
                let i = b as usize;
                if sim.0.bld_active[i] && sim.0.bld_kind[i] == BuildingKind::Assembler {
                    let next = (sim.0.bld_param[i] as usize + 1) % crate::sim::RECIPES.len();
                    sim.0.bld_param[i] = next as u16;
                    editor.recipe = sim.0.bld_param[i];
                    dirty.0 = true;
                }
            }
        } else {
            editor.dir = editor.dir.rotated();
        }
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        if let Some(state) = history.undo() {
            sim.0 = state.0.clone();
            world.0 = state.1.clone();
            dirty.0 = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyY) {
        if let Some(state) = history.redo() {
            sim.0 = state.0.clone();
            world.0 = state.1.clone();
            dirty.0 = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyC) {
        copy_blueprint(&sim.0, &world.0, &mut blueprint, &selection);
    }
    if keys.just_pressed(KeyCode::KeyV) {
        if !blueprint.tiles.is_empty() {
            editor.tool = Tool::Paste;
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        editor.tool = Tool::Pole;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        editor.tool = Tool::Generator;
    }
    if keys.just_pressed(KeyCode::KeyU) {
        editor.tool = Tool::Pipe;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        editor.tool = Tool::Pump;
    }
    if keys.just_pressed(KeyCode::KeyK) {
        editor.tool = Tool::Tank;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        editor.tool = Tool::Lab;
    }
    if keys.just_pressed(KeyCode::Escape) {
        editor.tool = Tool::Belt;
        selection.start = None;
        selection.end = None;
        selection.building = None;
    }
    if !buttons.pressed(MouseButton::Left) {
        editor.last_tile = None;
    }

    // Snapshot before any world-mutating stroke (paste, drag-paint, erase).
    let edit_stroke = (buttons.just_pressed(MouseButton::Left) && editor.tool != Tool::Select)
        || buttons.just_pressed(MouseButton::Right);
    if edit_stroke {
        history.push(&sim.0, &world.0);
    }

    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok((camera, cam_tf)) = cam.get_single() else {
        return;
    };
    let Ok((mut gtf, mut gvis, mut gsprite)) = ghost.get_single_mut() else {
        return;
    };

    let Some((tx, ty, _world)) = cursor_tile(window, camera, cam_tf) else {
        *gvis = Visibility::Hidden;
        return;
    };

    // Ghost preview.
    *gvis = Visibility::Visible;
    let show_box = match editor.tool {
        Tool::Select => buttons.pressed(MouseButton::Left) && selection.start.is_some(),
        Tool::Paste => !blueprint.tiles.is_empty(),
        _ => false,
    };
    if show_box {
        let (box_min, box_max) = match editor.tool {
            Tool::Select => {
                let (sx, sy) = selection.start.unwrap_or((tx, ty));
                let (ex, ey) = (tx, ty);
                selection.end = Some((tx, ty));
                ((sx.min(ex), sy.min(ey)), (sx.max(ex), sy.max(ey)))
            }
            Tool::Paste => ((tx, ty), (tx + blueprint.width - 1, ty + blueprint.height - 1)),
            _ => unreachable!(),
        };
        let w = (box_max.0 - box_min.0 + 1) as f32 * TILE;
        let h = (box_max.1 - box_min.1 + 1) as f32 * TILE;
        gtf.translation = Vec3::new(
            (box_min.0 + box_max.0) as f32 * 0.5 * TILE,
            (box_min.1 + box_max.1) as f32 * 0.5 * TILE,
            2.0,
        );
        gtf.rotation = Quat::IDENTITY;
        gsprite.custom_size = Some(Vec2::new(w, h));
    } else {
        gtf.translation = Vec3::new(tx as f32 * TILE, ty as f32 * TILE, 2.0);
        gtf.rotation = Quat::from_rotation_z(dir_angle(editor.dir));
        gsprite.custom_size = Some(Vec2::splat(TILE * 0.9));
    }
    let free = world.0.is_empty(tx, ty);
    let mut paste_valid = true;
    if editor.tool == Tool::Paste && !blueprint.tiles.is_empty() {
        for t in &blueprint.tiles {
            if !world.0.is_empty(tx + t.dx, ty + t.dy) {
                paste_valid = false;
                break;
            }
        }
    }
    let valid = match editor.tool {
        Tool::Miner => free && world.0.ore_at(tx, ty) != 0,
        Tool::Paste => paste_valid,
        Tool::Select => true,
        _ => free,
    };
    gsprite.color = if valid {
        Color::srgba(0.5, 0.9, 0.6, 0.35)
    } else {
        Color::srgba(0.95, 0.4, 0.4, 0.35)
    };

    let mut changed = false;

    // Place (held: drag-paint).
    if buttons.pressed(MouseButton::Left) {
        match editor.tool {
            Tool::Belt => {
                if free {
                    // Auto-turn: while dragging, direction follows the stroke and
                    // the previous belt turns to keep the line connected.
                    if let Some(last) = editor.last_tile {
                        if let Some(d) = delta_dir(last, (tx, ty)) {
                            editor.dir = d;
                            let prev = world.0.belt_at(last.0, last.1);
                            if prev != INVALID {
                                sim.0.belt_dir[prev as usize] = d;
                            }
                        }
                    }
                    let id = sim.0.add_belt(tx, ty, editor.dir);
                    world.0.set_belt(tx, ty, id);
                    editor.last_tile = Some((tx, ty));
                    changed = true;
                }
            }
            Tool::Source if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Source);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Sink if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Sink);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Assembler if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Assembler);
                sim.0.bld_param[id as usize] = editor.recipe;
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Inserter if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Inserter);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Miner if free && world.0.ore_at(tx, ty) != 0 => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Miner);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Storage if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Storage);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Shipment if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Shipment);
                // Cycle target item kind with R while placing shipments.
                sim.0.bld_param[id as usize] = editor.dir as u16;
                sim.0.bld_delivered[id as usize] = 0;
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Splitter if free => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Splitter);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Select => {
                if buttons.just_pressed(MouseButton::Left) {
                    selection.start = Some((tx, ty));
                    selection.end = Some((tx, ty));
                    selection.building = None;
                }
            }
            Tool::Paste => {
                if buttons.just_pressed(MouseButton::Left) && paste_valid {
                    for t in &blueprint.tiles {
                        let x = tx + t.dx;
                        let y = ty + t.dy;
                        if !world.0.is_empty(x, y) {
                            continue;
                        }
                        if t.is_belt {
                            let id = sim.0.add_belt(x, y, t.dir);
                            world.0.set_belt(x, y, id);
                        } else {
                            let id = sim.0.add_building(x, y, t.dir, t.kind);
                            world.0.set_building(x, y, id);
                            sim.0.bld_param[id as usize] = t.param;
                            if t.kind == BuildingKind::Shipment {
                                sim.0.bld_delivered[id as usize] = 0;
                            }
                        }
                    }
                    changed = true;
                }
            }
            Tool::Pole => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pole);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Generator => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Generator);
                    sim.0.bld_param[id as usize] = 10; // one generator powers up to 10 consumers
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Pipe => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pipe);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Pump => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pump);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Tank => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Tank);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Lab => {
                if free {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Lab);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            _ => {}
        }
    }

    if buttons.just_released(MouseButton::Left) && editor.tool == Tool::Select {
        if let (Some(s), Some(e)) = (selection.start, selection.end) {
            if s == e {
                let b = world.0.building_at(tx, ty);
                selection.building =
                    if b != INVALID && sim.0.bld_active[b as usize] { Some(b) } else { None };
            }
        }
    }

    // Erase (held: drag-erase).
    if buttons.pressed(MouseButton::Right) {
        let belt = world.0.belt_at(tx, ty);
        if belt != INVALID {
            sim.0.remove_belt(belt);
            world.0.set_belt(tx, ty, INVALID);
            changed = true;
        }
        let bld = world.0.building_at(tx, ty);
        if bld != INVALID {
            sim.0.remove_building(bld);
            world.0.set_building(tx, ty, INVALID);
            changed = true;
        }
    }

    if changed {
        rebuild_belt_graph(&mut sim.0, &world.0);
        dirty.0 = true;
    }
}

fn copy_blueprint(
    sim: &crate::belts::BeltSim,
    grid: &crate::grid::Grid,
    blueprint: &mut Blueprint,
    selection: &Selection,
) {
    blueprint.tiles.clear();
    if let (Some(start), Some(end)) = (selection.start, selection.end) {
        let min_x = start.0.min(end.0);
        let max_x = start.0.max(end.0);
        let min_y = start.1.min(end.1);
        let max_y = start.1.max(end.1);
        blueprint.width = max_x - min_x + 1;
        blueprint.height = max_y - min_y + 1;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let belt = grid.belt_at(x, y);
                if belt != INVALID {
                    let id = belt as usize;
                    if sim.belt_active[id] {
                        blueprint.tiles.push(BlueprintTile {
                            dx: x - min_x,
                            dy: y - min_y,
                            is_belt: true,
                            dir: sim.belt_dir[id],
                            kind: BuildingKind::Source,
                            param: 1,
                        });
                    }
                }
                let bld = grid.building_at(x, y);
                if bld != INVALID {
                    let id = bld as usize;
                    if sim.bld_active[id] {
                        blueprint.tiles.push(BlueprintTile {
                            dx: x - min_x,
                            dy: y - min_y,
                            is_belt: false,
                            dir: sim.bld_dir[id],
                            kind: sim.bld_kind[id],
                            param: sim.bld_param[id],
                        });
                    }
                }
            }
        }
    } else {
        blueprint.width = 1;
        blueprint.height = 1;
    }
}

const SAVE_PATH: &str = "factory.save";

/// F5 saves the whole world (flat arrays serialize directly); F9 loads it.
pub fn save_load(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut world: ResMut<GameWorld>,
    mut dirty: ResMut<WorldDirty>,
) {
    if keys.just_pressed(KeyCode::F5) {
        match bincode::serialize(&(&sim.0, &world.0)) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(SAVE_PATH, bytes) {
                    error!("save failed: {e}");
                } else {
                    info!("saved to {SAVE_PATH}");
                }
            }
            Err(e) => error!("serialize failed: {e}"),
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        match std::fs::read(SAVE_PATH) {
            Ok(bytes) => match bincode::deserialize::<(crate::belts::BeltSim, crate::grid::Grid)>(&bytes) {
                Ok((s, g)) => {
                    sim.0 = s;
                    sim.0.dirty_power = true;
                    world.0 = g;
                    dirty.0 = true;
                    info!("loaded {SAVE_PATH}");
                }
                Err(e) => error!("deserialize failed: {e}"),
            },
            Err(e) => error!("load failed: {e}"),
        }
    }
}
