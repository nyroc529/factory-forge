//! Editor: place belts/sources/sinks with the mouse, rotate with R,
//! erase with right-click. A ghost sprite previews the placement.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::Overflow;
use serde::{Deserialize, Serialize};

use crate::belts::{BuildingKind, Dir, INVALID};
use crate::render::{dir_angle, WorldDirty, TILE};
use crate::sim::rebuild_belt_graph;
use crate::economy::{
    is_tech_unlocked, is_tool_unlocked, tech_for_tool, tool_category, tool_info, unlock_tech,
    ContractState, PlayerState, ProductionStats, Tech, ToolCategory, VictoryState,
};
use crate::{GameWorld, Sim};

/// Inspector / selection-box state.
#[derive(Resource, Default)]
pub struct Selection {
    pub start: Option<(i32, i32)>,
    pub end: Option<(i32, i32)>,
    pub building: Option<u32>,
}

/// Customizable bottom hotbar: slots 0-9 mapped to number keys.
#[derive(Resource)]
pub struct Hotbar {
    pub slots: [Option<Tool>; 10],
    pub selected: usize,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            slots: [
                Some(Tool::Belt),
                Some(Tool::Inserter),
                Some(Tool::Generator),
                Some(Tool::Miner),
                Some(Tool::Sink),
                Some(Tool::Select),
                None,
                None,
                None,
                None,
            ],
            selected: 0,
        }
    }
}

/// Inventory/build menu toggle state.
#[derive(Resource, Default)]
pub struct BuildMenu {
    pub visible: bool,
    /// Current vertical scroll offset in px for the tool cards panel.
    pub scroll_y: f32,
    /// Estimated total content height (set when menu opens).
    pub content_height: f32,
}

#[derive(Component)]
pub struct HotbarSlot(pub usize);

#[derive(Component)]
pub struct MenuItem(pub Tool);

#[derive(Component)]
pub struct MenuUnlock(pub Tech);

#[derive(Component)]
pub struct MenuContract(pub u16);

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct MenuScrollPanel;

#[derive(Component)]
pub struct MenuScrollContent;

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
    Research1,
    Research2,
    Research3,
    RailTrack,
    RailStation,
    Turret,
    ForgeCore,
}

impl From<BuildingKind> for Tool {
    fn from(kind: BuildingKind) -> Self {
        match kind {
            BuildingKind::Source => Tool::Source,
            BuildingKind::Sink => Tool::Sink,
            BuildingKind::Assembler => Tool::Assembler,
            BuildingKind::Inserter => Tool::Inserter,
            BuildingKind::Miner => Tool::Miner,
            BuildingKind::Storage => Tool::Storage,
            BuildingKind::Shipment => Tool::Shipment,
            BuildingKind::Splitter => Tool::Splitter,
            BuildingKind::Pole => Tool::Pole,
            BuildingKind::Generator => Tool::Generator,
            BuildingKind::Pipe => Tool::Pipe,
            BuildingKind::Pump => Tool::Pump,
            BuildingKind::Tank => Tool::Tank,
            BuildingKind::Lab => Tool::Research1,
            BuildingKind::RailTrack => Tool::RailTrack,
            BuildingKind::RailStation => Tool::RailStation,
            BuildingKind::Turret => Tool::Turret,
            BuildingKind::ForgeCore => Tool::ForgeCore,
        }
    }
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
            Tool::Research1 => "research 1",
            Tool::Research2 => "research 2",
            Tool::Research3 => "research 3",
            Tool::RailTrack => "rail track",
            Tool::RailStation => "rail station",
            Tool::Turret => "turret",
            Tool::ForgeCore => "forge core",
        }
    }
}


pub fn tool_cost(tool: Tool) -> i32 {
    tool_info(tool).cost.credits
}

fn try_pay(cost: i32, player: &mut PlayerState) -> bool {
    if player.credits >= cost {
        player.credits -= cost;
        true
    } else {
        false
    }
}

fn blueprint_cost(blueprint: &Blueprint) -> i32 {
    blueprint.tiles.iter().map(|t| tool_cost(if t.is_belt { Tool::Belt } else { t.kind.into() })).sum()
}

pub fn tool_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Belt => "belt",
        Tool::Source => "source",
        Tool::Sink => "sink",
        Tool::Assembler => "asm",
        Tool::Inserter => "ins",
        Tool::Miner => "miner",
        Tool::Storage => "store",
        Tool::Shipment => "ship",
        Tool::Splitter => "split",
        Tool::Select => "sel",
        Tool::Paste => "paste",
        Tool::Pole => "pole",
        Tool::Generator => "gen",
        Tool::Pipe => "pipe",
        Tool::Pump => "pump",
        Tool::Tank => "tank",
        Tool::Research1 => "rc1",
        Tool::Research2 => "rc2",
        Tool::Research3 => "rc3",
        Tool::RailTrack => "rail",
        Tool::RailStation => "station",
        Tool::Turret => "turret",
        Tool::ForgeCore => "core",
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
    mut hotbar: ResMut<Hotbar>,
    menu: Res<BuildMenu>,
    ui_interactions: Query<&Interaction>,
    mut player: ResMut<PlayerState>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Sprite), With<Ghost>>,
) {
    fn select_slot(hotbar: &mut Hotbar, editor: &mut EditorState, slot: usize, tech_flags: u64) {
        hotbar.selected = slot;
        if let Some(tool) = hotbar.slots[slot] {
            if is_tool_unlocked(tool, tech_flags) {
                editor.tool = tool;
            }
        }
    }
    let player = &mut *player;
    let tech_flags = player.tech_flags;
    let ui_hovered = ui_interactions
        .iter()
        .any(|i| *i != Interaction::None);
    if keys.just_pressed(KeyCode::Digit1) {
        select_slot(&mut hotbar, &mut editor, 0, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        select_slot(&mut hotbar, &mut editor, 1, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit3) {
        select_slot(&mut hotbar, &mut editor, 2, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit4) {
        select_slot(&mut hotbar, &mut editor, 3, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit5) {
        select_slot(&mut hotbar, &mut editor, 4, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit6) {
        select_slot(&mut hotbar, &mut editor, 5, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit7) {
        select_slot(&mut hotbar, &mut editor, 6, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit8) {
        select_slot(&mut hotbar, &mut editor, 7, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit9) {
        select_slot(&mut hotbar, &mut editor, 8, tech_flags);
    }
    if keys.just_pressed(KeyCode::Digit0) {
        select_slot(&mut hotbar, &mut editor, 9, tech_flags);
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
    if keys.just_pressed(KeyCode::Escape) && !menu.visible {
        editor.tool = Hotbar::default().slots[0].unwrap_or(Tool::Belt);
        selection.start = None;
        selection.end = None;
        selection.building = None;
    }
    if !buttons.pressed(MouseButton::Left) {
        editor.last_tile = None;
    }

    // Block world interaction when the menu is open or the cursor is over UI.
    if menu.visible || ui_hovered {
        if let Ok((_, mut gvis, _)) = ghost.get_single_mut() {
            *gvis = Visibility::Hidden;
        }
        return;
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
    if buttons.pressed(MouseButton::Left) && is_tool_unlocked(editor.tool, player.tech_flags) {
        match editor.tool {
            Tool::Belt => {
                if free && try_pay(tool_cost(Tool::Belt), player) {
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
            Tool::Source if free && try_pay(tool_cost(Tool::Source), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Source);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Sink if free && try_pay(tool_cost(Tool::Sink), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Sink);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Assembler if free && try_pay(tool_cost(Tool::Assembler), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Assembler);
                sim.0.bld_param[id as usize] = editor.recipe;
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Inserter if free && try_pay(tool_cost(Tool::Inserter), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Inserter);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Miner if free && world.0.ore_at(tx, ty) != 0 && try_pay(tool_cost(Tool::Miner), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Miner);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Storage if free && try_pay(tool_cost(Tool::Storage), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Storage);
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Shipment if free && try_pay(tool_cost(Tool::Shipment), player) => {
                let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Shipment);
                // Cycle target item kind with R while placing shipments.
                sim.0.bld_param[id as usize] = editor.recipe;
                sim.0.bld_delivered[id as usize] = 0;
                world.0.set_building(tx, ty, id);
                changed = true;
            }
            Tool::Splitter if free && try_pay(tool_cost(Tool::Splitter), player) => {
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
                if buttons.just_pressed(MouseButton::Left)
                    && paste_valid
                    && try_pay(blueprint_cost(&blueprint), player)
                {
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
                if free && try_pay(tool_cost(Tool::Pole), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pole);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Generator => {
                if free && try_pay(tool_cost(Tool::Generator), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Generator);
                    sim.0.bld_param[id as usize] = 10; // one generator powers up to 10 consumers
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Pipe => {
                if free && try_pay(tool_cost(Tool::Pipe), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pipe);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Pump => {
                if free && try_pay(tool_cost(Tool::Pump), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Pump);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Tank => {
                if free && try_pay(tool_cost(Tool::Tank), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Tank);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Research1 | Tool::Research2 | Tool::Research3 => {
                if free {
                    let cost = tool_cost(editor.tool);
                    if try_pay(cost, player) {
                        let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Lab);
                        let tier = match editor.tool {
                            Tool::Research1 => 0,
                            Tool::Research2 => 1,
                            Tool::Research3 => 2,
                            _ => 0,
                        };
                        sim.0.bld_param[id as usize] = tier;
                        world.0.set_building(tx, ty, id);
                        changed = true;
                    }
                }
            }
            Tool::RailTrack => {
                if free && try_pay(tool_cost(Tool::RailTrack), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::RailTrack);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::RailStation => {
                if free && try_pay(tool_cost(Tool::RailStation), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::RailStation);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::Turret => {
                if free && try_pay(tool_cost(Tool::Turret), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::Turret);
                    world.0.set_building(tx, ty, id);
                    changed = true;
                }
            }
            Tool::ForgeCore => {
                if free && try_pay(tool_cost(Tool::ForgeCore), player) {
                    let id = sim.0.add_building(tx, ty, editor.dir, BuildingKind::ForgeCore);
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

#[derive(Serialize, Deserialize)]
struct SaveGame {
    sim: crate::belts::BeltSim,
    grid: crate::grid::Grid,
    player: PlayerState,
    contract: ContractState,
    stats: ProductionStats,
    #[serde(default)]
    victory: VictoryState,
}

pub fn save_load(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut world: ResMut<GameWorld>,
    mut player: ResMut<PlayerState>,
    mut contract: ResMut<ContractState>,
    mut stats: ResMut<ProductionStats>,
    mut victory: ResMut<VictoryState>,
    mut rail: ResMut<crate::rail::RailNetwork>,
    mut combat: ResMut<crate::combat::CombatState>,
    mut dirty: ResMut<WorldDirty>,
    replay: Res<crate::replay::ReplayLog>,
) {
    if keys.just_pressed(KeyCode::F5) {
        let save = SaveGame {
            sim: sim.0.clone(),
            grid: world.0.clone(),
            player: player.clone(),
            contract: contract.clone(),
            stats: stats.clone(),
            victory: victory.clone(),
        };
        match bincode::serialize(&save) {
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
        let loaded = std::fs::read(SAVE_PATH).and_then(|bytes| {
            bincode::deserialize::<SaveGame>(&bytes)
                .map(|save| (save.sim, save.grid, Some((save.player, save.contract, save.stats, save.victory))))
                .or_else(|_| {
                    bincode::deserialize::<(crate::belts::BeltSim, crate::grid::Grid)>(&bytes)
                        .map(|(sim, grid)| (sim, grid, None))
                })
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        });
        match loaded {
            Ok((saved_sim, saved_grid, saved_player)) => {
                sim.0 = saved_sim;
                sim.0.dirty_power = true;
                sim.0.dirty_rail = true;
                world.0 = saved_grid;
                if let Some((saved_player, saved_contract, saved_stats, saved_victory)) = saved_player {
                    *player = saved_player;
                    *contract = saved_contract;
                    *stats = saved_stats;
                    *victory = saved_victory;
                }
                *rail = crate::rail::RailNetwork::default();
                *combat = crate::combat::CombatState::default();
                dirty.0 = true;
                info!("loaded {SAVE_PATH}");
            }
            Err(e) => error!("load failed: {e}"),
        }
    }
    if keys.just_pressed(KeyCode::F6) {
        replay.save();
    }
}

// ---------------------------------------------------------------- UI systems

#[derive(Component)]
pub struct ToolInfoPanel;

#[derive(Component)]
pub struct TutorialPrompt;

pub fn setup_hotbar(mut commands: Commands) {
    commands
        .spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                bottom: Val::Px(8.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ..default()
        })
        .with_children(|parent| {
            for i in 0..10 {
                let key = if i == 9 { "0" } else { &["1","2","3","4","5","6","7","8","9"][i] };
                parent
                    .spawn((
                        ButtonBundle {
                            style: Style {
                                width: Val::Px(72.0),
                                height: Val::Px(56.0),
                                margin: UiRect::all(Val::Px(2.0)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            background_color: BackgroundColor(Color::srgb(0.25, 0.27, 0.30)),
                            ..default()
                        },
                        HotbarSlot(i),
                    ))
                    .with_children(|p| {
                        p.spawn(TextBundle::from_section(
                            format!("{key}\n?"),
                            TextStyle {
                                font_size: 10.0,
                                color: Color::srgb(0.95, 0.95, 0.95),
                                ..default()
                            },
                        ));
                    });
            }
        });

    // Info bar above the hotbar: name, description, cost, and unlock status.
    commands.spawn((
        TextBundle {
            text: Text::from_section(
                "",
                TextStyle {
                    font_size: 14.0,
                    color: Color::srgb(0.9, 0.9, 0.95),
                    ..default()
                },
            ),
            style: Style {
                position_type: PositionType::Absolute,
                bottom: Val::Px(64.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ..default()
        },
        ToolInfoPanel,
    ));

    // Persistent objective hint in the top-right to avoid overlapping the resource HUD.
    commands.spawn((
        TextBundle {
            text: Text::from_section(
                "Place a Miner on ore -> Belt it to a Scrap Pit -> Earn credits",
                TextStyle {
                    font_size: 14.0,
                    color: Color::srgb(0.85, 0.9, 0.95),
                    ..default()
                },
            ),
            style: Style {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                right: Val::Px(8.0),
                ..default()
            },
            ..default()
        },
        TutorialPrompt,
    ));
}

pub fn update_tutorial(
    player: Res<PlayerState>,
    stats: Res<ProductionStats>,
    victory: Res<VictoryState>,
    mut query: Query<&mut Text, With<TutorialPrompt>>,
    mut last_msg: Local<&'static str>,
) {
    let msg = if victory.achieved {
        "Victory achieved! Sandbox mode unlocked."
    } else if stats.sold.iter().sum::<u64>() == 0 {
        "Place a Miner on ore -> Belt it to a Scrap Pit -> Earn credits"
    } else if player.research_points == 0 {
        "Earning credits! Complete contracts (Q menu) to earn Research Points"
    } else if player.tech_flags.count_ones() <= 1 {
        "You have RP! Open the tech tree (Q) and unlock new buildings"
    } else {
        "Keep expanding: more production -> more contracts -> Forge Core to win!"
    };
    if *last_msg == msg {
        return;
    }
    *last_msg = msg;
    if let Ok(mut text) = query.get_single_mut() {
        text.sections[0].value = msg.to_string();
    }
}

pub fn update_tool_info(
    hotbar: Res<Hotbar>,
    player: Res<PlayerState>,
    mut query: Query<&mut Text, With<ToolInfoPanel>>,
) {
    if !hotbar.is_changed() && !player.is_changed() {
        return;
    }
    if let Ok(mut text) = query.get_single_mut() {
        let value = if let Some(tool) = hotbar.slots[hotbar.selected] {
            let info = tool_info(tool);
            let rp = if let Some(tech) = tech_for_tool(tool) {
                if is_tech_unlocked(player.tech_flags, tech) {
                    "unlocked".to_string()
                } else {
                    format!("{} RP to unlock", tech.cost())
                }
            } else {
                "unlocked".to_string()
            };
            format!(
                "{}: {}    |    Cost: ${}    |    {}",
                info.name, info.description, info.cost.credits, rp
            )
        } else {
            "Empty slot — open the build menu (Q / Tab) to assign a tool.".to_string()
        };
        text.sections[0].value = value;
    }
}

pub fn menu_scroll(
    mut scroll_events: EventReader<MouseWheel>,
    mut content: Query<&mut Style, With<MenuScrollContent>>,
    mut menu: ResMut<BuildMenu>,
) {
    if !menu.visible {
        return;
    }
    let mut delta = 0.0_f32;
    for ev in scroll_events.read() {
        delta += match ev.unit {
            MouseScrollUnit::Line => ev.y * 50.0,
            MouseScrollUnit::Pixel => ev.y,
        };
    }
    if delta == 0.0 {
        return;
    }
    // The max scroll depends on content vs visible panel.
    // We estimate max from pre-computed content_height vs a generous panel size.
    let max_offset = (menu.content_height - 400.0).max(0.0);
    menu.scroll_y = (menu.scroll_y - delta).clamp(0.0, max_offset);
    for mut style in content.iter_mut() {
        style.top = Val::Px(-menu.scroll_y);
    }
}

pub fn update_hotbar(
    hotbar: Res<Hotbar>,
    mut slots: Query<(&HotbarSlot, &mut BackgroundColor, &Children)>,
    mut texts: Query<&mut Text>,
) {
    if !hotbar.is_changed() {
        return;
    }
    for (slot, mut bg, children) in slots.iter_mut() {
        let (label, is_filled) = if let Some(tool) = hotbar.slots[slot.0] {
            (tool_info(tool).name, true)
        } else {
            ("", false)
        };
        *bg = if is_filled {
            Color::srgb(0.22, 0.24, 0.27).into()
        } else {
            Color::srgb(0.12, 0.13, 0.15).into()
        };
        let key = if slot.0 == 9 { "0" } else { &["1","2","3","4","5","6","7","8","9"][slot.0] };
        for &child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.sections[0].value = format!("{key}\n{label}");
            }
        }
        // Highlight selected slot.
        if slot.0 == hotbar.selected {
            *bg = bg.0.mix(&Color::WHITE, 0.3).into();
        }
    }
}

fn menu_button(parent: &mut ChildBuilder, tool: Tool, player: &PlayerState) {
    let info = tool_info(tool);
    let unlocked = is_tool_unlocked(tool, player.tech_flags);
    if !unlocked {
        if let Some(tech) = tech_for_tool(tool) {
            let can_afford = player.research_points >= tech.cost();
            let name_color = if can_afford {
                Color::srgb(0.85, 0.95, 1.0)
            } else {
                Color::srgb(0.95, 0.5, 0.5)
            };
            let cost_color = if can_afford {
                Color::srgb(0.65, 0.85, 0.95)
            } else {
                Color::srgb(0.95, 0.5, 0.5)
            };
            parent
                .spawn((
                    ButtonBundle {
                        style: Style {
                            width: Val::Px(210.0),
                            height: Val::Px(160.0),
                            margin: UiRect::all(Val::Px(8.0)),
                            padding: UiRect::all(Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::FlexStart,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.12, 0.14, 0.16, 0.98).into(),
                        ..default()
                    },
                    MenuUnlock(tech),
                ))
                .with_children(|p| {
                    p.spawn(TextBundle::from_section(
                        format!("[LOCKED] {}", info.name),
                        TextStyle {
                            font_size: 15.0,
                            color: name_color,
                            ..default()
                        },
                    ));
                    p.spawn(TextBundle::from_section(
                        format!("{} RP to unlock", tech.cost()),
                        TextStyle {
                            font_size: 14.0,
                            color: cost_color,
                            ..default()
                        },
                    ));
                    p.spawn(TextBundle::from_section(
                        info.description.to_string(),
                        TextStyle {
                            font_size: 12.0,
                            color: Color::srgb(0.6, 0.65, 0.72),
                            ..default()
                        },
                    ));
                    p.spawn(TextBundle::from_section(
                        if can_afford { "Click to research" } else { "Not enough RP" },
                        TextStyle {
                            font_size: 11.0,
                            color: Color::srgb(0.75, 0.75, 0.8),
                            ..default()
                        },
                    ));
                });
            return;
        }
    }

    let affordable = player.credits >= info.cost.credits;
    let mut color = Color::srgba(0.18, 0.20, 0.24, 0.98);
    if !affordable {
        color = color.with_alpha(0.45);
    }
    parent
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(210.0),
                    height: Val::Px(160.0),
                    margin: UiRect::all(Val::Px(8.0)),
                    padding: UiRect::all(Val::Px(10.0)),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: color.into(),
                ..default()
            },
            MenuItem(tool),
        ))
        .with_children(|p| {
            p.spawn(TextBundle::from_section(
                info.name.to_string(),
                TextStyle {
                    font_size: 15.0,
                    color: Color::srgb(0.95, 0.95, 0.95),
                    ..default()
                },
            ));
            p.spawn(TextBundle::from_section(
                format!("${}", info.cost.credits),
                TextStyle {
                    font_size: 14.0,
                    color: if affordable {
                        Color::srgb(0.85, 0.95, 0.7)
                    } else {
                        Color::srgb(0.95, 0.5, 0.5)
                    },
                    ..default()
                },
            ));
            p.spawn(TextBundle::from_section(
                info.description.to_string(),
                TextStyle {
                    font_size: 12.0,
                    color: Color::srgb(0.6, 0.65, 0.72),
                    ..default()
                },
            ));
            p.spawn(TextBundle::from_section(
                if affordable { "Click to assign" } else { "Not enough credits" },
                TextStyle {
                    font_size: 11.0,
                    color: Color::srgb(0.75, 0.75, 0.8),
                    ..default()
                },
            ));
        });
}

fn contract_button(parent: &mut ChildBuilder, item_kind: u16, contract: &ContractState) {
    let selected = contract.item_kind == item_kind;
    let disabled = contract.delivered > 0;
    let required = crate::economy::contract_requirement(item_kind, contract.completed);
    parent
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(130.0),
                    height: Val::Px(54.0),
                    margin: UiRect::all(Val::Px(4.0)),
                    padding: UiRect::all(Val::Px(4.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: if selected {
                    Color::srgb(0.18, 0.48, 0.38).into()
                } else if disabled {
                    Color::srgb(0.16, 0.17, 0.2).into()
                } else {
                    Color::srgb(0.22, 0.28, 0.38).into()
                },
                ..default()
            },
            MenuContract(item_kind),
        ))
        .with_children(|p| {
            p.spawn(TextBundle::from_section(
                format!("Contract: {}", crate::belts::ITEM_NAMES[item_kind as usize]),
                TextStyle {
                    font_size: 10.0,
                    color: Color::srgb(0.95, 0.95, 0.95),
                    ..default()
                },
            ));
            p.spawn(TextBundle::from_section(
                format!("{} units  +{} RP", required, 25 + contract.completed as i32 * 10),
                TextStyle {
                    font_size: 9.0,
                    color: Color::srgb(0.7, 0.84, 0.96),
                    ..default()
                },
            ));
        });
}

fn collect_by_category(tools: &[Tool]) -> Vec<(ToolCategory, Vec<Tool>)> {
    let mut groups: Vec<(ToolCategory, Vec<Tool>)> = vec![
        (ToolCategory::Logistics, vec![]),
        (ToolCategory::Production, vec![]),
        (ToolCategory::PowerFluids, vec![]),
        (ToolCategory::Rail, vec![]),
        (ToolCategory::Combat, vec![]),
        (ToolCategory::Tools, vec![]),
    ];
    for &tool in tools {
        let cat = tool_category(tool);
        if let Some(g) = groups.iter_mut().find(|(c, _)| *c == cat) {
            g.1.push(tool);
        }
    }
    groups.retain(|(_, v)| !v.is_empty());
    groups
}

pub fn open_build_menu(
    mut commands: Commands,
    hotbar: &Hotbar,
    player: &PlayerState,
    contract: &ContractState,
    victory: &VictoryState,
    menu: &mut BuildMenu,
) {
    let selected_label = hotbar
        .slots[hotbar.selected]
        .map(tool_label)
        .unwrap_or("empty");
    const ALL_TOOLS: &[Tool] = &[
        Tool::Select,
        Tool::Paste,
        Tool::Belt,
        Tool::Inserter,
        Tool::Splitter,
        Tool::Source,
        Tool::Sink,
        Tool::Assembler,
        Tool::Miner,
        Tool::Storage,
        Tool::Shipment,
        Tool::Pole,
        Tool::Generator,
        Tool::Pipe,
        Tool::Pump,
        Tool::Tank,
        Tool::Research1,
        Tool::Research2,
        Tool::Research3,
        Tool::RailTrack,
        Tool::RailStation,
        Tool::Turret,
        Tool::ForgeCore,
    ];
    let available_tools: Vec<Tool> = if victory.achieved {
        ALL_TOOLS.to_vec()
    } else {
        ALL_TOOLS
            .iter()
            .copied()
            .filter(|&t| tech_for_tool(t) != Some(Tech::Creative))
            .collect()
    };
    let groups = collect_by_category(&available_tools);

    // Estimate content height for scroll clamping.
    // Each category header ~36px, each tool card row ~176px (card 160 + margin).
    // Assume ~4 cards fit per row on a 90% width panel.
    let mut est_height = 0.0_f32;
    for (_, tools) in &groups {
        est_height += 36.0; // category header
        let rows = ((tools.len() as f32) / 4.0).ceil();
        est_height += rows * 176.0;
    }
    menu.scroll_y = 0.0;
    menu.content_height = est_height;

    commands
        .spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.0, 0.0, 0.0, 0.75).into(),
                ..default()
            },
            MenuRoot,
        ))
        .with_children(|root| {
            // Main card.
            root.spawn(NodeBundle {
                style: Style {
                    width: Val::Percent(90.0),
                    height: Val::Percent(85.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                background_color: Color::srgba(0.08, 0.09, 0.11, 0.98).into(),
                ..default()
            })
            .with_children(|panel| {
                let slot_key = if hotbar.selected == 9 {
                    "0"
                } else {
                    &["1", "2", "3", "4", "5", "6", "7", "8", "9"][hotbar.selected]
                };
                panel.spawn(TextBundle::from_section(
                    format!(
                        "Build Menu (Q/Esc to close)   |   Selected slot {}: {}",
                        slot_key, selected_label
                    ),
                    TextStyle {
                        font_size: 18.0,
                        color: Color::srgb(0.9, 0.9, 0.95),
                        ..default()
                    },
                ));

                // Contract row.
                panel
                    .spawn(NodeBundle {
                        style: Style {
                            flex_direction: FlexDirection::Row,
                            margin: UiRect::vertical(Val::Px(8.0)),
                            ..default()
                        },
                        ..default()
                    })
                    .with_children(|contracts| {
                        for item_kind in crate::economy::CONTRACT_ITEMS {
                            contract_button(contracts, item_kind, contract);
                        }
                    });

                // Scrollable list of categories and tool cards.
                panel
                    .spawn((
                        NodeBundle {
                            style: Style {
                                height: Val::Percent(100.0),
                                overflow: Overflow::clip_y(),
                                ..default()
                            },
                            background_color: Color::srgba(0.04, 0.05, 0.06, 0.95).into(),
                            ..default()
                        },
                        MenuScrollPanel,
                    ))
                    .with_children(|scroll| {
                        scroll
                            .spawn((
                                NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Absolute,
                                        top: Val::Px(0.0),
                                        left: Val::Px(0.0),
                                        right: Val::Px(0.0),
                                        flex_direction: FlexDirection::Column,
                                        ..default()
                                    },
                                    ..default()
                                },
                                MenuScrollContent,
                            ))
                            .with_children(|body| {
                                for (cat, tools) in groups {
                                    body.spawn(TextBundle::from_section(
                                        cat.name(),
                                        TextStyle {
                                            font_size: 16.0,
                                            color: Color::srgb(0.85, 0.9, 0.95),
                                            ..default()
                                        },
                                    )
                                    .with_style(Style {
                                        margin: UiRect::new(
                                            Val::Px(8.0),
                                            Val::Px(0.0),
                                            Val::Px(16.0),
                                            Val::Px(4.0),
                                        ),
                                        ..default()
                                    }));
                                    body.spawn(NodeBundle {
                                        style: Style {
                                            flex_direction: FlexDirection::Row,
                                            flex_wrap: FlexWrap::Wrap,
                                            ..default()
                                        },
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        for tool in tools {
                                            menu_button(row, tool, player);
                                        }
                                    });
                                }
                            });
                    });
            });
        });
}

pub fn handle_menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<BuildMenu>,
    mut commands: Commands,
    root: Query<Entity, With<MenuRoot>>,
    hotbar: Res<Hotbar>,
    player: Res<PlayerState>,
    contract: Res<ContractState>,
    victory: Res<VictoryState>,
) {
    let toggle = keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::Tab);
    let close = keys.just_pressed(KeyCode::Escape);
    if toggle {
        menu.visible = !menu.visible;
    } else if close && menu.visible {
        menu.visible = false;
    } else {
        return;
    }

    for e in root.iter() {
        commands.entity(e).despawn_recursive();
    }
    if menu.visible {
        open_build_menu(commands, &hotbar, &player, &contract, &victory, &mut menu);
    }
}

pub fn handle_menu_clicks(
    mut interactions: Query<(&Interaction, &MenuItem), Changed<Interaction>>,
    mut hotbar: ResMut<Hotbar>,
    mut editor: ResMut<EditorState>,
    mut menu: ResMut<BuildMenu>,
    mut commands: Commands,
    root: Query<Entity, With<MenuRoot>>,
    player: Res<PlayerState>,
    mut sfx: ResMut<crate::audio::SfxQueue>,
) {
    for (interaction, item) in interactions.iter_mut() {
        if *interaction == Interaction::Pressed {
            if !is_tool_unlocked(item.0, player.tech_flags) {
                sfx.0.push(crate::audio::SfxKind::Error);
                continue;
            }
            let info = tool_info(item.0);
            if player.credits < info.cost.credits {
                sfx.0.push(crate::audio::SfxKind::Error);
                continue;
            }
            sfx.0.push(crate::audio::SfxKind::Place);
            let slot = hotbar.selected;
            hotbar.slots[slot] = Some(item.0);
            editor.tool = item.0;
            menu.visible = false;
            for e in root.iter() {
                commands.entity(e).despawn_recursive();
            }
        }
    }
}

pub fn handle_menu_contracts(
    mut interactions: Query<(&Interaction, &MenuContract), Changed<Interaction>>,
    mut contract: ResMut<ContractState>,
) {
    for (interaction, offer) in interactions.iter_mut() {
        if *interaction == Interaction::Pressed {
            contract.select(offer.0);
        }
    }
}

pub fn handle_menu_unlocks(
    mut interactions: Query<(&Interaction, &MenuUnlock), Changed<Interaction>>,
    mut player: ResMut<PlayerState>,
    mut menu: ResMut<BuildMenu>,
    mut commands: Commands,
    root: Query<Entity, With<MenuRoot>>,
    mut sfx: ResMut<crate::audio::SfxQueue>,
) {
    for (interaction, unlock) in interactions.iter_mut() {
        if *interaction == Interaction::Pressed {
            let cost = unlock.0.cost();
            if player.research_points >= cost {
                player.research_points -= cost;
                unlock_tech(&mut player.tech_flags, unlock.0);
                sfx.0.push(crate::audio::SfxKind::Research);
                menu.visible = false;
                for e in root.iter() {
                    commands.entity(e).despawn_recursive();
                }
            } else {
                sfx.0.push(crate::audio::SfxKind::Error);
            }
        }
    }
}

pub fn handle_hotbar_clicks(
    mut interactions: Query<(&Interaction, &HotbarSlot), Changed<Interaction>>,
    mut hotbar: ResMut<Hotbar>,
    mut editor: ResMut<EditorState>,
    player: Res<PlayerState>,
) {
    for (interaction, slot) in interactions.iter_mut() {
        if *interaction == Interaction::Pressed {
            hotbar.selected = slot.0;
            if let Some(tool) = hotbar.slots[slot.0] {
                if is_tool_unlocked(tool, player.tech_flags) {
                    editor.tool = tool;
                }
            }
        }
    }
}
