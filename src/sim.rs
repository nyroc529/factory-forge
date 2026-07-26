//! Fixed-timestep simulation: building production, advance pass, transfer pass.

use rand::Rng;

use crate::belts::{BeltSim, BuildingKind, BELT_SPEED, INVALID, KINDS, LANES, MIN_SPACING};
use crate::grid::Grid;

pub const ITEM_KINDS: u16 = KINDS as u16;

/// A craftable recipe.
pub struct Recipe {
    pub ticks: u16,
    pub input: [u16; KINDS],
    pub output_kind: usize,
    pub output_count: u16,
}

/// Recipe table. Index 0 is the default starter recipe for new assemblers.
pub const RECIPES: &[Recipe] = &[
    // Steel plate: 2 iron (amber) + 1 copper (sky) -> 1 violet (processed alloy)
    Recipe {
        ticks: 90,
        input: [2, 1, 0, 0, 0],
        output_kind: 4,
        output_count: 1,
    },
    // Gear: 2 iron -> 1 rose
    Recipe {
        ticks: 60,
        input: [2, 0, 0, 0, 0],
        output_kind: 3,
        output_count: 1,
    },
];

pub const INSERTER_COOLDOWN: u16 = 12;
pub const MINER_COOLDOWN: u16 = 30;
pub const SPLITTER_COOLDOWN: u16 = 6;
pub const STORAGE_CAP: u16 = 50;
pub const POWER_RADIUS: f32 = 7.0;
pub const POWER_RADIUS2: f32 = POWER_RADIUS * POWER_RADIUS;

#[inline]
pub fn is_consumer(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::Inserter
            | BuildingKind::Assembler
            | BuildingKind::Miner
            | BuildingKind::Storage
            | BuildingKind::Shipment
            | BuildingKind::Splitter
    )
}

#[inline]
pub fn is_power_node(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::Pole
            | BuildingKind::Generator
            | BuildingKind::Inserter
            | BuildingKind::Assembler
            | BuildingKind::Miner
            | BuildingKind::Storage
            | BuildingKind::Shipment
            | BuildingKind::Splitter
    )
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Recompute which active buildings are powered by generators within the power radius.
pub fn rebuild_power(sim: &mut BeltSim, active_blds: &[usize]) {
    // Non-consumers are trivially powered; consumers start unpowered.
    for &s in active_blds {
        sim.bld_powered[s] = !is_consumer(sim.bld_kind[s]);
    }

    let nodes: Vec<usize> = active_blds
        .iter()
        .copied()
        .filter(|&s| is_power_node(sim.bld_kind[s]))
        .collect();
    let n = nodes.len();
    let mut uf = UnionFind::new(n);
    for i in 0..n {
        let a = nodes[i];
        for j in (i + 1)..n {
            let b = nodes[j];
            let dx = (sim.bld_x[a] - sim.bld_x[b]) as f32;
            let dy = (sim.bld_y[a] - sim.bld_y[b]) as f32;
            if dx * dx + dy * dy <= POWER_RADIUS2 {
                uf.union(i, j);
            }
        }
    }

    let mut supply = vec![0.0f32; n];
    let mut demand = vec![0.0f32; n];
    for (idx, &s) in nodes.iter().enumerate() {
        match sim.bld_kind[s] {
            BuildingKind::Generator => supply[idx] = sim.bld_param[s].max(1) as f32,
            _ if is_consumer(sim.bld_kind[s]) => demand[idx] = 1.0,
            _ => {}
        }
    }

    let mut comp_supply = std::collections::HashMap::<usize, f32>::new();
    let mut comp_demand = std::collections::HashMap::<usize, f32>::new();
    for idx in 0..n {
        let root = uf.find(idx);
        *comp_supply.entry(root).or_default() += supply[idx];
        *comp_demand.entry(root).or_default() += demand[idx];
    }

    for (idx, &s) in nodes.iter().enumerate() {
        if !is_consumer(sim.bld_kind[s]) {
            continue;
        }
        let root = uf.find(idx);
        let have = comp_supply.get(&root).copied().unwrap_or(0.0);
        let need = comp_demand.get(&root).copied().unwrap_or(0.0);
        if have >= need {
            sim.bld_powered[s] = true;
        }
    }
}

/// Recompute belt_next / belt_to_sink pointers from the grid (call after edits).
pub fn rebuild_belt_graph(sim: &mut BeltSim, grid: &Grid) {
    for b in 0..sim.belt_count() {
        if !sim.belt_active[b] {
            continue;
        }
        let (dx, dy) = sim.belt_dir[b].vec();
        let nx = sim.belt_x[b] + dx;
        let ny = sim.belt_y[b] + dy;
        let next = grid.belt_at(nx, ny);
        sim.belt_next[b] = if next != INVALID && sim.belt_active[next as usize] {
            next
        } else {
            INVALID
        };
        let bld = grid.building_at(nx, ny);
        sim.belt_to_sink[b] =
            bld != INVALID && sim.bld_active[bld as usize] && sim.bld_kind[bld as usize] == BuildingKind::Sink;
    }
}

/// Take the head item of any lane on `belt` if it is far enough along.
fn pick_from_belt(sim: &mut BeltSim, belt: u32) -> Option<u16> {
    for lane in 0..LANES {
        let head = sim.belt_head[belt as usize][lane];
        if head != INVALID && sim.item_dist[head as usize] >= 0.3 {
            let id = sim.unlink_head(belt, lane);
            let kind = sim.item_type[id as usize];
            sim.free_item(id);
            return Some(kind);
        }
    }
    None
}

/// Pick any one item from a storage/assembler output inventory behind an inserter.
fn pick_from_building(sim: &mut BeltSim, bld: u32) -> Option<u16> {
    let b = bld as usize;
    // Prefer output buffer, then storage.
    if sim.bld_kind[b] == BuildingKind::Assembler || sim.bld_kind[b] == BuildingKind::Storage {
        if let Some(k) = (0..KINDS).find(|&k| sim.bld_out[b][k] > 0) {
            sim.bld_out[b][k] -= 1;
            return Some(k as u16);
        }
    }
    if sim.bld_kind[b] == BuildingKind::Storage {
        if let Some(k) = (0..KINDS).find(|&k| sim.bld_in[b][k] > 0) {
            sim.bld_in[b][k] -= 1;
            return Some(k as u16);
        }
    }
    None
}

/// Attempt to deliver an inserter-held item to a building ahead.
fn drop_to_building(sim: &mut BeltSim, bld: u32, kind: u16) -> bool {
    let b = bld as usize;
    let k = kind as usize;
    match sim.bld_kind[b] {
        BuildingKind::Sink => true,
        BuildingKind::Source => false,
        BuildingKind::Storage => {
            if sim.bld_in[b][k] < STORAGE_CAP {
                sim.bld_in[b][k] += 1;
                true
            } else {
                false
            }
        }
        BuildingKind::Shipment => {
            let target = sim.bld_param[b] as usize;
            if k == target && sim.bld_delivered[b] < u32::MAX {
                sim.bld_delivered[b] += 1;
                true
            } else {
                false
            }
        }
        BuildingKind::Assembler => {
            if let Some(r) = RECIPES.get(sim.bld_param[b] as usize) {
                if r.input[k] > 0 && sim.bld_in[b][k] < STORAGE_CAP {
                    sim.bld_in[b][k] += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Sources inject, miners mine, assemblers craft, inserters move items.
/// One pass over the active building arrays per tick.
pub fn tick_buildings(sim: &mut BeltSim, grid: &Grid, active_blds: &[usize]) {
    let mut rng = rand::thread_rng();
    for &s in active_blds {
        if !sim.bld_active[s] || !sim.bld_powered[s] {
            continue;
        }
        let dir = sim.bld_dir[s];
        let (dx, dy) = dir.vec();
        let (fx, fy) = (sim.bld_x[s] + dx, sim.bld_y[s] + dy); // tile ahead
        let (bx, by) = (sim.bld_x[s] - dx, sim.bld_y[s] - dy); // tile behind

        match sim.bld_kind[s] {
            BuildingKind::Source => {
                let belt = grid.belt_at(fx, fy);
                if belt != INVALID && sim.belt_active[belt as usize] {
                    for lane in 0..LANES {
                        let kind = rng.gen_range(0..ITEM_KINDS);
                        sim.try_spawn_item(belt, lane, kind);
                    }
                }
            }
            BuildingKind::Sink => {}
            BuildingKind::Miner => {
                let belt = grid.belt_at(fx, fy);
                if belt == INVALID || !sim.belt_active[belt as usize] {
                    continue;
                }
                if sim.bld_timer[s] > 0 {
                    sim.bld_timer[s] -= 1;
                    continue;
                }
                let ore_kind = grid.ore_at(sim.bld_x[s], sim.bld_y[s]).saturating_sub(1) as u16;
                if ore_kind >= ITEM_KINDS {
                    continue;
                }
                for lane in 0..LANES {
                    if sim.try_spawn_item(belt, lane, ore_kind).is_some() {
                        sim.bld_timer[s] = MINER_COOLDOWN;
                        break;
                    }
                }
            }
            BuildingKind::Storage => {}
            BuildingKind::Shipment => {}
            BuildingKind::Splitter => {
                if sim.bld_timer[s] > 0 {
                    sim.bld_timer[s] -= 1;
                    continue;
                }
                let (lx, ly) = (dir.perp().0 as i32, dir.perp().1 as i32);
                if sim.bld_held[s] == 0 {
                    let belt = grid.belt_at(bx, by);
                    if belt != INVALID && sim.belt_active[belt as usize] {
                        if let Some(kind) = pick_from_belt(sim, belt) {
                            sim.bld_held[s] = kind + 1;
                        }
                    }
                }
                if sim.bld_held[s] != 0 {
                    let kind = sim.bld_held[s] - 1;
                    let side = sim.bld_param[s] as usize;
                    let (sx, sy) = if side == 0 {
                        (sim.bld_x[s] + lx, sim.bld_y[s] + ly)
                    } else {
                        (sim.bld_x[s] - lx, sim.bld_y[s] - ly)
                    };
                    let belt = grid.belt_at(sx, sy);
                    if belt != INVALID && sim.belt_active[belt as usize] {
                        for lane in 0..LANES {
                            if sim.try_spawn_item(belt, lane, kind).is_some() {
                                sim.bld_held[s] = 0;
                                sim.bld_timer[s] = SPLITTER_COOLDOWN;
                                sim.bld_param[s] = 1 - sim.bld_param[s];
                                break;
                            }
                        }
                    }
                }
            }
            BuildingKind::Assembler => {
                let recipe_idx = sim.bld_param[s] as usize;
                if let Some(r) = RECIPES.get(recipe_idx) {
                    if sim.bld_timer[s] > 1 {
                        sim.bld_timer[s] -= 1;
                    } else if sim.bld_timer[s] == 1 {
                        sim.bld_out[s][r.output_kind] += r.output_count;
                        sim.bld_timer[s] = 0;
                    } else {
                        let can_craft = (0..KINDS).all(|k| sim.bld_in[s][k] >= r.input[k])
                            && sim.bld_out[s][r.output_kind] < STORAGE_CAP;
                        if can_craft {
                            for k in 0..KINDS {
                                sim.bld_in[s][k] -= r.input[k];
                            }
                            sim.bld_timer[s] = r.ticks;
                        }
                    }
                }
            }
            BuildingKind::Inserter => {
                if sim.bld_timer[s] > 0 {
                    sim.bld_timer[s] -= 1;
                    continue;
                }
                // Empty hand: pick from behind (belt head, storage, or assembler output).
                if sim.bld_held[s] == 0 {
                    let belt = grid.belt_at(bx, by);
                    if belt != INVALID && sim.belt_active[belt as usize] {
                        if let Some(kind) = pick_from_belt(sim, belt) {
                            sim.bld_held[s] = kind + 1;
                        }
                    } else {
                        let bld = grid.building_at(bx, by);
                        if bld != INVALID && sim.bld_active[bld as usize] {
                            if let Some(kind) = pick_from_building(sim, bld) {
                                sim.bld_held[s] = kind + 1;
                            }
                        }
                    }
                }
                // Holding: drop ahead (belt tail, storage, assembler input, shipment, sink).
                if sim.bld_held[s] != 0 {
                    let kind = sim.bld_held[s] - 1;
                    let belt = grid.belt_at(fx, fy);
                    if belt != INVALID && sim.belt_active[belt as usize] {
                        for lane in 0..LANES {
                            if sim.try_spawn_item(belt, lane, kind).is_some() {
                                sim.bld_held[s] = 0;
                                sim.bld_timer[s] = INSERTER_COOLDOWN;
                                break;
                            }
                        }
                    } else {
                        let bld = grid.building_at(fx, fy);
                        if bld != INVALID && sim.bld_active[bld as usize] {
                            if drop_to_building(sim, bld, kind) {
                                sim.bld_held[s] = 0;
                                sim.bld_timer[s] = INSERTER_COOLDOWN;
                            }
                        }
                    }
                }
            }
            BuildingKind::Pole | BuildingKind::Generator => {}
        }
    }
}

/// Pass 1: move every item forward, clamped behind the item ahead.
/// Pass 2: transfer head items that crossed 1.0 onto the next belt.
pub fn tick(sim: &mut BeltSim, active_belts: &[usize]) {
    // Snapshot previous state for render interpolation.
    for i in 0..sim.item_capacity() {
        if sim.item_active[i] {
            sim.item_prev_belt[i] = sim.item_belt[i];
            sim.item_prev_dist[i] = sim.item_dist[i];
        }
    }

    // ---- Pass 1: advance ----
    for &b in active_belts {
        if !sim.belt_active[b] {
            continue;
        }
        for lane in 0..LANES {
            let mut cur = sim.belt_head[b][lane];
            let mut limit = f32::INFINITY; // head may overshoot 1.0; clamped in pass 2
            while cur != INVALID {
                let i = cur as usize;
                let new_dist = (sim.item_dist[i] + BELT_SPEED).min(limit);
                sim.item_dist[i] = new_dist;
                limit = new_dist - MIN_SPACING;
                cur = sim.item_behind[i];
            }
        }
    }

    // ---- Pass 2: transfer heads across belt boundaries ----
    for &b in active_belts {
        if !sim.belt_active[b] {
            continue;
        }
        let next = sim.belt_next[b];
        let to_sink = sim.belt_to_sink[b];
        for lane in 0..LANES {
            let head = sim.belt_head[b][lane];
            if head == INVALID {
                continue;
            }
            let i = head as usize;
            if sim.item_dist[i] < 1.0 {
                continue;
            }
            if to_sink {
                let id = sim.unlink_head(b as u32, lane);
                sim.free_item(id);
                continue;
            }
            let overshoot = sim.item_dist[i] - 1.0;
            let mut moved = false;
            if next != INVALID {
                let tail = sim.belt_tail[next as usize][lane];
                let room = if tail == INVALID {
                    f32::INFINITY
                } else {
                    sim.item_dist[tail as usize] - MIN_SPACING
                };
                if room >= 0.0 {
                    let id = sim.unlink_head(b as u32, lane);
                    let idx = id as usize;
                    sim.item_belt[idx] = next;
                    sim.item_dist[idx] = overshoot.min(room);
                    sim.link_at_tail(id, next, lane);
                    moved = true;
                }
            }
            if !moved {
                // Blocked: park at the end of this belt.
                sim.item_dist[i] = 1.0;
                // Re-clamp the queue behind the parked head.
                let mut limit = 1.0 - MIN_SPACING;
                let mut cur = sim.item_behind[i];
                while cur != INVALID {
                    let j = cur as usize;
                    if sim.item_dist[j] > limit {
                        sim.item_dist[j] = limit;
                    }
                    limit = sim.item_dist[j] - MIN_SPACING;
                    cur = sim.item_behind[j];
                }
            }
        }
    }
}
