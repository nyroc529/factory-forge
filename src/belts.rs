//! Data-oriented belt + item storage.
//!
//! Everything lives in flat parallel arrays (SoA). No per-item allocation,
//! no per-item entities in the simulation. Items on a belt lane form an
//! intrusive doubly-linked list via index fields, ordered head (output side)
//! to tail (input side).

use serde::{Deserialize, Serialize};

use crate::grid::Grid;

pub const INVALID: u32 = u32::MAX;
pub const LANES: usize = 2;
/// Number of distinct item kinds.
pub const KINDS: usize = 11;

/// Human-readable names for item kinds, indexed by kind.
pub const ITEM_NAMES: [&str; KINDS] = [
    "iron",
    "copper",
    "coal",
    "gear",
    "steel",
    "stone",
    "oil",
    "plastic",
    "circuit",
    "brick",
    "science",
];
/// Minimum center-to-center spacing between items on a lane (belt-lengths).
pub const MIN_SPACING: f32 = 0.28;
/// Belt speed in belt-lengths per simulation tick.
pub const BELT_SPEED: f32 = 0.045;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BuildingKind {
    Source,
    Sink,
    Assembler,
    Inserter,
    Miner,
    Storage,
    Shipment,
    Splitter,
    Pole,
    Generator,
    Pipe,
    Pump,
    Tank,
    Lab,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Dir {
    East,
    North,
    West,
    South,
}

impl Dir {
    pub fn vec(self) -> (i32, i32) {
        match self {
            Dir::East => (1, 0),
            Dir::North => (0, 1),
            Dir::West => (-1, 0),
            Dir::South => (0, -1),
        }
    }
    pub fn fvec(self) -> (f32, f32) {
        let (x, y) = self.vec();
        (x as f32, y as f32)
    }
    /// Perpendicular (left of travel) used for lane offsets.
    pub fn perp(self) -> (f32, f32) {
        let (x, y) = self.fvec();
        (-y, x)
    }
    pub fn rotated(self) -> Dir {
        match self {
            Dir::East => Dir::North,
            Dir::North => Dir::West,
            Dir::West => Dir::South,
            Dir::South => Dir::East,
        }
    }
}

/// SoA storage for belts and items.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BeltSim {
    // ---- belts (parallel arrays, one slot per belt tile) ----
    pub belt_x: Vec<i32>,
    pub belt_y: Vec<i32>,
    pub belt_dir: Vec<Dir>,
    /// Belt segment this one feeds into, or INVALID.
    pub belt_next: Vec<u32>,
    /// True if the tile ahead holds a sink building.
    pub belt_to_sink: Vec<bool>,
    pub belt_active: Vec<bool>,
    /// Frontmost item per lane (closest to output), or INVALID.
    pub belt_head: Vec<[u32; LANES]>,
    /// Rearmost item per lane (closest to input), or INVALID.
    pub belt_tail: Vec<[u32; LANES]>,
    /// Chunk coordinate (cx, cy) for each belt, for active-chunk filtering.
    pub belt_chunk: Vec<(i32, i32)>,
    /// Chunk coordinate (cx, cy) for each building.
    pub bld_chunk: Vec<(i32, i32)>,

    // ---- items (parallel arrays, one slot per item) ----
    pub item_type: Vec<u16>,
    pub item_belt: Vec<u32>,
    pub item_lane: Vec<u8>,
    /// Progress along the belt tile, 0.0 (input edge) .. 1.0 (output edge).
    pub item_dist: Vec<f32>,
    /// Previous tick's belt/dist, kept for render interpolation.
    pub item_prev_belt: Vec<u32>,
    pub item_prev_dist: Vec<f32>,
    /// Index of the item ahead (closer to output), or INVALID.
    pub item_ahead: Vec<u32>,
    /// Index of the item behind (closer to input), or INVALID.
    pub item_behind: Vec<u32>,
    pub item_active: Vec<bool>,

    /// Recycled item slots.
    pub free_items: Vec<u32>,
    /// Recycled belt slots.
    pub free_belts: Vec<u32>,

    // ---- buildings (parallel arrays) ----
    pub bld_x: Vec<i32>,
    pub bld_y: Vec<i32>,
    pub bld_dir: Vec<Dir>,
    pub bld_kind: Vec<BuildingKind>,
    pub bld_active: Vec<bool>,
    /// Generic cooldown / craft timer.
    pub bld_timer: Vec<u16>,
    /// Item held by an inserter (kind + 1, 0 = empty hand).
    pub bld_held: Vec<u16>,
    /// Assembler input inventory, count per item kind.
    pub bld_in: Vec<[u16; KINDS]>,
    /// Assembler output inventory, count per item kind.
    pub bld_out: Vec<[u16; KINDS]>,
    /// Generic building parameter: recipe index for assemblers, target item kind for shipments.
    pub bld_param: Vec<u16>,
    /// Shipment target count (for Shipment buildings).
    pub bld_delivered: Vec<u32>,
    /// Whether this building is receiving enough power this tick.
    pub bld_powered: Vec<bool>,
    /// Current fluid in this building's fluid box (if any).
    pub bld_fluid_volume: Vec<f32>,
    /// Max fluid this building can hold.
    pub bld_fluid_capacity: Vec<u16>,
    /// Fluid kind (0 empty/none, 1 water, ...).
    pub bld_fluid_type: Vec<u8>,
    /// Network id for connected fluid nodes, or INVALID.
    pub bld_fluid_network: Vec<u32>,
    /// True if this assembler's fluid input is satisfied this tick.
    pub bld_fluid_ready: Vec<bool>,
    pub free_blds: Vec<u32>,
    /// Rebuild power/fluid networks next tick when the world changes.
    #[serde(default = "default_true")]
    pub dirty_power: bool,
}

fn default_true() -> bool {
    true
}

impl BeltSim {
    pub fn add_belt(&mut self, x: i32, y: i32, dir: Dir) -> u32 {
        if let Some(id) = self.free_belts.pop() {
            let b = id as usize;
            self.belt_x[b] = x;
            self.belt_y[b] = y;
            self.belt_dir[b] = dir;
            self.belt_next[b] = INVALID;
            self.belt_to_sink[b] = false;
            self.belt_active[b] = true;
            self.belt_head[b] = [INVALID; LANES];
            self.belt_tail[b] = [INVALID; LANES];
            self.belt_chunk[b] = Grid::chunk_key(x, y);
            return id;
        }
        let id = self.belt_x.len() as u32;
        self.belt_x.push(x);
        self.belt_y.push(y);
        self.belt_dir.push(dir);
        self.belt_next.push(INVALID);
        self.belt_to_sink.push(false);
        self.belt_active.push(true);
        self.belt_head.push([INVALID; LANES]);
        self.belt_tail.push([INVALID; LANES]);
        self.belt_chunk.push(Grid::chunk_key(x, y));
        id
    }

    /// Deactivate a belt and free every item on it.
    pub fn remove_belt(&mut self, belt: u32) {
        let b = belt as usize;
        for lane in 0..LANES {
            let mut cur = self.belt_head[b][lane];
            while cur != INVALID {
                let next = self.item_behind[cur as usize];
                self.free_item(cur);
                cur = next;
            }
            self.belt_head[b][lane] = INVALID;
            self.belt_tail[b][lane] = INVALID;
        }
        self.belt_active[b] = false;
        self.free_belts.push(belt);
    }

    pub fn add_building(&mut self, x: i32, y: i32, dir: Dir, kind: BuildingKind) -> u32 {
        if let Some(id) = self.free_blds.pop() {
            let i = id as usize;
            self.bld_x[i] = x;
            self.bld_y[i] = y;
            self.bld_dir[i] = dir;
            self.bld_kind[i] = kind;
            self.bld_active[i] = true;
            self.bld_timer[i] = 0;
            self.bld_held[i] = 0;
            self.bld_in[i] = [0; KINDS];
            self.bld_out[i] = [0; KINDS];
            self.bld_param[i] = 0;
            self.bld_delivered[i] = 0;
            self.bld_powered[i] = false;
            self.bld_fluid_volume[i] = 0.0;
            self.bld_fluid_capacity[i] = match kind {
                BuildingKind::Pipe => 1,
                BuildingKind::Pump => 1,
                BuildingKind::Tank => 50,
                _ => 0,
            };
            self.bld_fluid_type[i] = if kind == BuildingKind::Pump { 1 } else { 0 };
            self.bld_fluid_network[i] = INVALID;
            self.bld_fluid_ready[i] = false;
            self.bld_chunk[i] = Grid::chunk_key(x, y);
            self.dirty_power = true;
            return id;
        }
        let id = self.bld_x.len() as u32;
        self.bld_x.push(x);
        self.bld_y.push(y);
        self.bld_dir.push(dir);
        self.bld_kind.push(kind);
        self.bld_active.push(true);
        self.bld_timer.push(0);
        self.bld_held.push(0);
        self.bld_in.push([0; KINDS]);
        self.bld_out.push([0; KINDS]);
        self.bld_param.push(0);
        self.bld_delivered.push(0);
        self.bld_powered.push(false);
        self.bld_fluid_volume.push(0.0);
        self.bld_fluid_capacity.push(match kind {
            BuildingKind::Pipe => 1,
            BuildingKind::Pump => 1,
            BuildingKind::Tank => 50,
            _ => 0,
        });
        self.bld_fluid_type.push(if kind == BuildingKind::Pump { 1 } else { 0 });
        self.bld_fluid_network.push(INVALID);
        self.bld_fluid_ready.push(false);
        self.bld_chunk.push(Grid::chunk_key(x, y));
        self.dirty_power = true;
        id
    }

    pub fn remove_building(&mut self, id: u32) {
        self.bld_active[id as usize] = false;
        self.free_blds.push(id);
        self.dirty_power = true;
    }

    pub fn free_item(&mut self, id: u32) {
        let i = id as usize;
        self.item_active[i] = false;
        self.item_ahead[i] = INVALID;
        self.item_behind[i] = INVALID;
        self.free_items.push(id);
    }

    pub fn belt_count(&self) -> usize {
        self.belt_x.len()
    }

    pub fn item_capacity(&self) -> usize {
        self.item_type.len()
    }

    pub fn active_item_count(&self) -> usize {
        self.item_type.len() - self.free_items.len()
    }

    fn alloc_item(&mut self) -> u32 {
        if let Some(id) = self.free_items.pop() {
            id
        } else {
            let id = self.item_type.len() as u32;
            self.item_type.push(0);
            self.item_belt.push(INVALID);
            self.item_lane.push(0);
            self.item_dist.push(0.0);
            self.item_prev_belt.push(INVALID);
            self.item_prev_dist.push(0.0);
            self.item_ahead.push(INVALID);
            self.item_behind.push(INVALID);
            self.item_active.push(false);
            id
        }
    }

    /// Room at the tail (input side) of a lane for a new item at dist 0?
    pub fn tail_has_room(&self, belt: u32, lane: usize) -> bool {
        let tail = self.belt_tail[belt as usize][lane];
        tail == INVALID || self.item_dist[tail as usize] >= MIN_SPACING
    }

    /// Try to spawn a new item at the input edge of a belt lane.
    pub fn try_spawn_item(&mut self, belt: u32, lane: usize, kind: u16) -> Option<u32> {
        if !self.tail_has_room(belt, lane) {
            return None;
        }
        let id = self.alloc_item();
        let i = id as usize;
        self.item_type[i] = kind;
        self.item_belt[i] = belt;
        self.item_lane[i] = lane as u8;
        self.item_dist[i] = 0.0;
        self.item_prev_belt[i] = belt;
        self.item_prev_dist[i] = 0.0;
        self.item_active[i] = true;
        self.link_at_tail(id, belt, lane);
        Some(id)
    }

    /// Insert an (already configured) item as the new tail of a lane.
    pub fn link_at_tail(&mut self, id: u32, belt: u32, lane: usize) {
        let b = belt as usize;
        let old_tail = self.belt_tail[b][lane];
        self.item_ahead[id as usize] = old_tail;
        self.item_behind[id as usize] = INVALID;
        if old_tail == INVALID {
            self.belt_head[b][lane] = id;
        } else {
            self.item_behind[old_tail as usize] = id;
        }
        self.belt_tail[b][lane] = id;
    }

    /// Detach the current head item of a lane.
    pub fn unlink_head(&mut self, belt: u32, lane: usize) -> u32 {
        let b = belt as usize;
        let head = self.belt_head[b][lane];
        debug_assert!(head != INVALID);
        let behind = self.item_behind[head as usize];
        self.belt_head[b][lane] = behind;
        if behind == INVALID {
            self.belt_tail[b][lane] = INVALID;
        } else {
            self.item_ahead[behind as usize] = INVALID;
        }
        self.item_ahead[head as usize] = INVALID;
        self.item_behind[head as usize] = INVALID;
        head
    }
}
