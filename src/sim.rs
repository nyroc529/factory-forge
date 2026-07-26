//! Fixed-timestep simulation: building production, advance pass, transfer pass.

use rand::Rng;

use crate::belts::{BeltSim, BuildingKind, BELT_SPEED, INVALID, KINDS, LANES, MIN_SPACING};
use crate::grid::Grid;

pub const ITEM_KINDS: u16 = KINDS as u16;

/// A craftable recipe.
pub struct Recipe {
    pub name: &'static str,
    pub ticks: u16,
    pub input: [u16; KINDS],
    pub output_kind: usize,
    pub output_count: u16,
    pub fluid_input: u16,
    pub fluid_input_type: u8,
    pub fluid_output: u16,
    pub fluid_output_type: u8,
}

/// Recipe table. Index 0 is the default starter recipe for new assemblers.
pub const RECIPES: &[Recipe] = &[
    // Steel: 2 iron + 1 copper -> 1 steel
    Recipe {
        name: "steel",
        ticks: 90,
        input: [2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        output_kind: 4,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Gear: 2 iron -> 1 gear
    Recipe {
        name: "gear",
        ticks: 60,
        input: [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        output_kind: 3,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Water steel: 1 iron + 1 water -> 1 steel
    Recipe {
        name: "water steel",
        ticks: 100,
        input: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        output_kind: 4,
        output_count: 1,
        fluid_input: 1,
        fluid_input_type: 1,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Brick: 2 stone -> 1 brick
    Recipe {
        name: "brick",
        ticks: 80,
        input: [0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0],
        output_kind: 9,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Plastic: 1 coal + 2 oil -> 1 plastic
    Recipe {
        name: "plastic",
        ticks: 120,
        input: [0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0],
        output_kind: 7,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Circuit: 1 iron + 2 copper + 1 plastic -> 1 circuit
    Recipe {
        name: "circuit",
        ticks: 150,
        input: [1, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0],
        output_kind: 8,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
    // Science pack: 1 steel + 1 plastic + 1 circuit -> 1 science
    Recipe {
        name: "science pack",
        ticks: 200,
        input: [0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0],
        output_kind: 10,
        output_count: 1,
        fluid_input: 0,
        fluid_input_type: 0,
        fluid_output: 0,
        fluid_output_type: 0,
    },
];

pub const INSERTER_COOLDOWN: u16 = 12;
pub const MINER_COOLDOWN: u16 = 30;
pub const SPLITTER_COOLDOWN: u16 = 6;
pub const STORAGE_CAP: u16 = 50;
pub const FLUID_PUMP_RATE: f32 = 0.5;
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
            | BuildingKind::Pump
            | BuildingKind::Lab
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
            | BuildingKind::Pump
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

    // Per-root (0..n) aggregates. Roots are valid indices into these arrays.
    let mut comp_supply = vec![0.0f32; n];
    let mut comp_demand = vec![0.0f32; n];
    for idx in 0..n {
        let root = uf.find(idx);
        comp_supply[root] += supply[idx];
        comp_demand[root] += demand[idx];
    }

    for (idx, &s) in nodes.iter().enumerate() {
        if !is_consumer(sim.bld_kind[s]) {
            continue;
        }
        let root = uf.find(idx);
        if comp_supply[root] >= comp_demand[root] {
            sim.bld_powered[s] = true;
        }
    }
}

/// Recompute connected fluid networks among active buildings (call each tick).
/// `root_map` is a scratch buffer that will be reused across frames.
/// Returns the number of contiguous network ids (0..count-1).
pub fn rebuild_fluid_networks(sim: &mut BeltSim, active_blds: &[usize], root_map: &mut Vec<u32>) -> usize {
    // Refresh assembler capacities in case the recipe changed.
    for &s in active_blds {
        let cap = match sim.bld_kind[s] {
            BuildingKind::Pipe => 1,
            BuildingKind::Pump => 1,
            BuildingKind::Tank => 50,
            BuildingKind::Assembler => {
                let recipe_idx = sim.bld_param[s] as usize;
                if let Some(r) = RECIPES.get(recipe_idx) {
                    if r.fluid_input > 0 || r.fluid_output > 0 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            _ => 0,
        };
        sim.bld_fluid_capacity[s] = cap;
        if cap == 0 {
            sim.bld_fluid_network[s] = INVALID;
        }
    }

    let nodes: Vec<usize> = active_blds
        .iter()
        .copied()
        .filter(|&s| sim.bld_fluid_capacity[s] > 0)
        .collect();
    let n = nodes.len();
    let mut uf = UnionFind::new(n);
    for i in 0..n {
        let a = nodes[i];
        for j in (i + 1)..n {
            let b = nodes[j];
            let dx = sim.bld_x[a] - sim.bld_x[b];
            let dy = sim.bld_y[a] - sim.bld_y[b];
            if dx.abs() + dy.abs() == 1 {
                uf.union(i, j);
            }
        }
    }

    root_map.resize(n, INVALID);
    root_map.fill(INVALID);
    let mut net_id = 0u32;
    for idx in 0..n {
        let root = uf.find(idx);
        if root_map[root] == INVALID {
            root_map[root] = net_id;
            net_id += 1;
        }
        sim.bld_fluid_network[nodes[idx]] = root_map[root];
    }
    net_id as usize
}

/// Resolve fluid production/consumption per network for this tick.
/// All scratch slices must be at least `net_count` long.
pub fn tick_fluids(
    sim: &mut BeltSim,
    active_blds: &[usize],
    net_count: usize,
    cap: &mut [f32],
    vol: &mut [f32],
    prod: &mut [f32],
    cons: &mut [f32],
    ready: &mut [bool],
) {
    // Clear per-network and per-assembler ready state.
    for x in cap.iter_mut().take(net_count) {
        *x = 0.0;
    }
    for x in vol.iter_mut().take(net_count) {
        *x = 0.0;
    }
    for x in prod.iter_mut().take(net_count) {
        *x = 0.0;
    }
    for x in cons.iter_mut().take(net_count) {
        *x = 0.0;
    }
    for x in ready.iter_mut().take(net_count) {
        *x = false;
    }
    for &s in active_blds {
        sim.bld_fluid_ready[s] = false;
    }

    // First pass: aggregate capacity, current volume, production and planned consumption.
    for &s in active_blds {
        let capacity = sim.bld_fluid_capacity[s];
        if capacity == 0 || sim.bld_fluid_network[s] == INVALID {
            continue;
        }
        let net = sim.bld_fluid_network[s] as usize;
        cap[net] += capacity as f32;
        vol[net] += sim.bld_fluid_volume[s];
        // Pumps and assemblers only move fluid when powered.
        if !sim.bld_powered[s] {
            continue;
        }
        match sim.bld_kind[s] {
            BuildingKind::Pump => {
                prod[net] += FLUID_PUMP_RATE;
            }
            BuildingKind::Assembler => {
                let recipe_idx = sim.bld_param[s] as usize;
                if let Some(r) = RECIPES.get(recipe_idx) {
                    if r.fluid_input > 0
                        && sim.bld_timer[s] == 0
                        && (0..KINDS).all(|k| sim.bld_in[s][k] >= r.input[k])
                        && sim.bld_out[s][r.output_kind] < STORAGE_CAP
                    {
                        cons[net] += r.fluid_input as f32;
                    }
                }
            }
            _ => {}
        }
    }

    // Decide which networks can satisfy their assemblers this tick.
    for net in 0..net_count {
        ready[net] = (vol[net] + prod[net]) >= cons[net];
    }

    // Mark assemblers as fluid-ready.
    for &s in active_blds {
        if sim.bld_fluid_capacity[s] == 0 || sim.bld_fluid_network[s] == INVALID {
            continue;
        }
        if sim.bld_kind[s] == BuildingKind::Assembler && sim.bld_timer[s] == 0 {
            let net = sim.bld_fluid_network[s] as usize;
            let recipe_idx = sim.bld_param[s] as usize;
            if let Some(r) = RECIPES.get(recipe_idx) {
                if r.fluid_input > 0
                    && (0..KINDS).all(|k| sim.bld_in[s][k] >= r.input[k])
                    && sim.bld_out[s][r.output_kind] < STORAGE_CAP
                    && ready[net]
                {
                    sim.bld_fluid_ready[s] = true;
                }
            }
        }
    }

    // Apply production/consumption and clamp to network capacity.
    for net in 0..net_count {
        let after = if ready[net] {
            vol[net] + prod[net] - cons[net]
        } else {
            vol[net] + prod[net]
        };
        vol[net] = after.clamp(0.0, cap[net]);
    }

    // Redistribute network volume proportionally by node capacity.
    for &s in active_blds {
        let capacity = sim.bld_fluid_capacity[s];
        if capacity == 0 || sim.bld_fluid_network[s] == INVALID {
            continue;
        }
        let net = sim.bld_fluid_network[s] as usize;
        let total_cap = cap[net];
        if total_cap > 0.0 {
            sim.bld_fluid_volume[s] = vol[net] * (capacity as f32 / total_cap);
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
        BuildingKind::Lab => {
            const SCIENCE: usize = 10;
            if k == SCIENCE && sim.bld_in[b][SCIENCE] < STORAGE_CAP {
                sim.bld_in[b][SCIENCE] += 1;
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
                        if r.fluid_output > 0 {
                            sim.bld_fluid_volume[s] += r.fluid_output as f32;
                        }
                        sim.bld_timer[s] = 0;
                    } else {
                        let fluid_ok = r.fluid_input == 0 || sim.bld_fluid_ready[s];
                        let can_craft = (0..KINDS).all(|k| sim.bld_in[s][k] >= r.input[k])
                            && sim.bld_out[s][r.output_kind] < STORAGE_CAP
                            && fluid_ok;
                        if can_craft {
                            for k in 0..KINDS {
                                sim.bld_in[s][k] -= r.input[k];
                            }
                            sim.bld_fluid_volume[s] -= r.fluid_input as f32;
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
            BuildingKind::Pole | BuildingKind::Generator | BuildingKind::Pipe | BuildingKind::Pump | BuildingKind::Tank => {}
            BuildingKind::Lab => {
                if sim.bld_timer[s] > 0 {
                    sim.bld_timer[s] -= 1;
                    continue;
                }
                const SCIENCE: usize = 10;
                if sim.bld_in[s][SCIENCE] > 0 {
                    sim.bld_in[s][SCIENCE] -= 1;
                    if sim.bld_delivered[s] < u32::MAX {
                        sim.bld_delivered[s] += 1;
                    }
                    sim.bld_timer[s] = 6;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::belts::{BeltSim, Dir};
    use crate::grid::Grid;

    #[test]
    fn fluid_network_powers_crafting() {
        let mut sim = BeltSim::default();
        let grid = Grid::new(20, 20);

        let gen = sim.add_building(0, 0, Dir::East, BuildingKind::Generator);
        sim.bld_param[gen as usize] = 10;
        let pump = sim.add_building(2, 0, Dir::East, BuildingKind::Pump);
        let _pipe = sim.add_building(3, 0, Dir::East, BuildingKind::Pipe);
        let tank = sim.add_building(4, 0, Dir::East, BuildingKind::Tank);
        let asm = sim.add_building(5, 0, Dir::East, BuildingKind::Assembler);
        sim.bld_param[asm as usize] = 2; // water alloy: 1 amber + 1 water -> 1 violet
        sim.bld_in[asm as usize][0] = 20; // preload amber

        let active_blds: Vec<usize> = (0..sim.bld_x.len())
            .filter(|&s| sim.bld_active[s])
            .collect();
        let active_belts: Vec<usize> = vec![];

        rebuild_power(&mut sim, &active_blds);
        assert!(sim.bld_powered[pump as usize], "pump must be powered");

        let mut fluid_roots = vec![];
        let mut fluid_cap = vec![];
        let mut fluid_vol = vec![];
        let mut fluid_prod = vec![];
        let mut fluid_cons = vec![];
        let mut fluid_ready = vec![];
        for _ in 0..250 {
            let net_count =
                rebuild_fluid_networks(&mut sim, &active_blds, &mut fluid_roots);
            fluid_cap.resize(net_count, 0.0);
            fluid_vol.resize(net_count, 0.0);
            fluid_prod.resize(net_count, 0.0);
            fluid_cons.resize(net_count, 0.0);
            fluid_ready.resize(net_count, false);
            tick_fluids(
                &mut sim,
                &active_blds,
                net_count,
                &mut fluid_cap,
                &mut fluid_vol,
                &mut fluid_prod,
                &mut fluid_cons,
                &mut fluid_ready,
            );
            tick_buildings(&mut sim, &grid, &active_blds);
            tick(&mut sim, &active_belts);
        }

        assert!(
            sim.bld_fluid_volume[tank as usize] > 0.0,
            "tank should contain water"
        );
        assert!(
            sim.bld_out[asm as usize][4] > 0,
            "assembler should have produced violet"
        );
    }

    #[test]
    fn unpowered_pump_produces_no_water() {
        let mut sim = BeltSim::default();
        let grid = Grid::new(20, 20);

        let _pump = sim.add_building(0, 0, Dir::East, BuildingKind::Pump);
        let _pipe = sim.add_building(1, 0, Dir::East, BuildingKind::Pipe);
        let tank = sim.add_building(2, 0, Dir::East, BuildingKind::Tank);

        let active_blds: Vec<usize> = (0..sim.bld_x.len())
            .filter(|&s| sim.bld_active[s])
            .collect();
        let active_belts: Vec<usize> = vec![];

        rebuild_power(&mut sim, &active_blds);

        let mut fluid_roots = vec![];
        let mut fluid_cap = vec![];
        let mut fluid_vol = vec![];
        let mut fluid_prod = vec![];
        let mut fluid_cons = vec![];
        let mut fluid_ready = vec![];
        for _ in 0..100 {
            let net_count =
                rebuild_fluid_networks(&mut sim, &active_blds, &mut fluid_roots);
            fluid_cap.resize(net_count, 0.0);
            fluid_vol.resize(net_count, 0.0);
            fluid_prod.resize(net_count, 0.0);
            fluid_cons.resize(net_count, 0.0);
            fluid_ready.resize(net_count, false);
            tick_fluids(
                &mut sim,
                &active_blds,
                net_count,
                &mut fluid_cap,
                &mut fluid_vol,
                &mut fluid_prod,
                &mut fluid_cons,
                &mut fluid_ready,
            );
            tick_buildings(&mut sim, &grid, &active_blds);
            tick(&mut sim, &active_belts);
        }

        assert_eq!(
            sim.bld_fluid_volume[tank as usize], 0.0,
            "tank should be empty without power"
        );
    }

    #[test]
    fn plastic_recipe_crafts() {
        let mut sim = BeltSim::default();
        let grid = Grid::new(20, 20);

        let asm = sim.add_building(0, 0, Dir::East, BuildingKind::Assembler);
        sim.bld_powered[asm as usize] = true;
        let recipe_idx = RECIPES.iter().position(|r| r.name == "plastic").unwrap();
        sim.bld_param[asm as usize] = recipe_idx as u16;
        sim.bld_in[asm as usize][2] = 1; // coal
        sim.bld_in[asm as usize][6] = 2; // oil

        let active_blds: Vec<usize> = (0..sim.bld_x.len())
            .filter(|&s| sim.bld_active[s])
            .collect();

        for _ in 0..130 {
            tick_buildings(&mut sim, &grid, &active_blds);
        }

        assert!(sim.bld_out[asm as usize][7] > 0, "assembler should produce plastic");
    }

    #[test]
    fn science_pack_recipe_crafts() {
        let mut sim = BeltSim::default();
        let grid = Grid::new(20, 20);

        let asm = sim.add_building(0, 0, Dir::East, BuildingKind::Assembler);
        sim.bld_powered[asm as usize] = true;
        let recipe_idx = RECIPES.iter().position(|r| r.name == "science pack").unwrap();
        sim.bld_param[asm as usize] = recipe_idx as u16;
        sim.bld_in[asm as usize][4] = 1; // steel
        sim.bld_in[asm as usize][7] = 1; // plastic
        sim.bld_in[asm as usize][8] = 1; // circuit

        let active_blds: Vec<usize> = (0..sim.bld_x.len())
            .filter(|&s| sim.bld_active[s])
            .collect();

        for _ in 0..220 {
            tick_buildings(&mut sim, &grid, &active_blds);
        }

        assert!(
            sim.bld_out[asm as usize][10] > 0,
            "assembler should produce science packs"
        );
    }

    #[test]
    fn lab_consumes_science_packs() {
        let mut sim = BeltSim::default();
        let grid = Grid::new(20, 20);

        let lab = sim.add_building(0, 0, Dir::East, BuildingKind::Lab);
        sim.bld_powered[lab as usize] = true;
        sim.bld_in[lab as usize][10] = 5; // science packs

        let active_blds: Vec<usize> = (0..sim.bld_x.len())
            .filter(|&s| sim.bld_active[s])
            .collect();

        for _ in 0..50 {
            tick_buildings(&mut sim, &grid, &active_blds);
        }

        assert!(
            sim.bld_delivered[lab as usize] > 0,
            "lab should consume science packs for research"
        );
        assert!(
            sim.bld_in[lab as usize][10] < 5,
            "lab should have consumed at least one science pack"
        );
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
