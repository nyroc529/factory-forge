//! Economy, costs, and research-point model.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ui::Tool;
use crate::belts::KINDS;

/// Player wallet and progression currency.
#[derive(Resource, Default, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub credits: i32,
    pub research_points: i32,
    pub tech_flags: u64,
}

impl PlayerState {
    pub fn with_starting_funds() -> Self {
        let mut flags = 0u64;
        unlock_tech(&mut flags, Tech::PowerFluids);
        Self {
            credits: 500,
            research_points: 0,
            tech_flags: flags,
        }
    }
}

#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct ContractState {
    pub item_kind: u16,
    pub delivered: u32,
    pub required: u32,
    pub completed: u32,
}

impl Default for ContractState {
    fn default() -> Self {
        Self {
            item_kind: 3,
            delivered: 0,
            required: 40,
            completed: 0,
        }
    }
}

pub const CONTRACT_ITEMS: [u16; 3] = [3, 8, 10];

pub const fn contract_requirement(kind: u16, completed: u32) -> u32 {
    let base = match kind {
        3 => 60,
        8 => 30,
        10 => 12,
        _ => 40,
    };
    base + completed * 10
}

impl ContractState {
    pub fn select(&mut self, item_kind: u16) {
        if CONTRACT_ITEMS.contains(&item_kind) && self.delivered == 0 {
            self.item_kind = item_kind;
            self.required = contract_requirement(item_kind, self.completed);
        }
    }

    pub fn record_delivery(&mut self, kind: u16, count: u32, player: &mut PlayerState) {
        if kind != self.item_kind {
            return;
        }
        self.delivered = self.delivered.saturating_add(count);
        if self.delivered < self.required {
            return;
        }
        player.credits += item_value(self.item_kind) * self.required as i32 * 2;
        player.research_points += 25 + self.completed as i32 * 10;
        self.completed += 1;
        let stage = self.completed % CONTRACT_ITEMS.len() as u32;
        self.item_kind = CONTRACT_ITEMS[stage as usize];
        self.required = contract_requirement(self.item_kind, self.completed);
        self.delivered = 0;
    }
}

#[derive(Resource, Default, Clone, Serialize, Deserialize)]
pub struct ProductionStats {
    pub sold: [u64; KINDS],
    pub shipped: [u64; KINDS],
}

impl ProductionStats {
    pub fn record_sale(&mut self, kind: usize, count: u16) {
        self.sold[kind] += count as u64;
    }

    pub fn record_shipment(&mut self, kind: usize, count: u32) {
        self.shipped[kind] += count as u64;
    }
}

#[derive(Resource, Default, Clone, Serialize, Deserialize)]
pub struct VictoryState {
    pub achieved: bool,
}

/// A price tag for placing a tool. For now costs are credits-only;
/// item ingredient costs can be added later.
#[derive(Clone, Copy, Default)]
pub struct Cost {
    pub credits: i32,
}

impl Cost {
    pub const fn free() -> Self {
        Self { credits: 0 }
    }

    pub const fn new(credits: i32) -> Self {
        Self { credits }
    }
}

/// Static info for each buildable tool.
pub struct ToolInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub cost: Cost,
}

const INFO_SELECT: ToolInfo = ToolInfo {
    name: "Select",
    description: "Drag to select an area. Use C to copy, V to paste.",
    cost: Cost { credits: 0 },
};
const INFO_PASTE: ToolInfo = ToolInfo {
    name: "Paste Blueprint",
    description: "Place the copied blueprint.",
    cost: Cost { credits: 0 },
};
const INFO_BELT: ToolInfo = ToolInfo {
    name: "Transport Belt",
    description: "Moves items in two lanes.",
    cost: Cost { credits: 3 },
};
const INFO_INSERTER: ToolInfo = ToolInfo {
    name: "Inserter",
    description: "Picks up items from behind and drops them ahead.",
    cost: Cost { credits: 15 },
};
const INFO_SPLITTER: ToolInfo = ToolInfo {
    name: "Splitter",
    description: "Splits one belt into two side belts.",
    cost: Cost { credits: 25 },
};
const INFO_SOURCE: ToolInfo = ToolInfo {
    name: "Source",
    description: "Generates random raw resources.",
    cost: Cost { credits: 100 },
};
const INFO_SINK: ToolInfo = ToolInfo {
    name: "Scrap Pit",
    description: "A hole in the floor that buys any item for credits.",
    cost: Cost { credits: 40 },
};
const INFO_ASSEMBLER: ToolInfo = ToolInfo {
    name: "Assembler",
    description: "Crafts recipes from inputs.",
    cost: Cost { credits: 120 },
};
const INFO_MINER: ToolInfo = ToolInfo {
    name: "Miner",
    description: "Extracts ore from the tile underneath.",
    cost: Cost { credits: 80 },
};
const INFO_STORAGE: ToolInfo = ToolInfo {
    name: "Storage",
    description: "Buffers up to 50 of each item.",
    cost: Cost { credits: 30 },
};
const INFO_SHIPMENT: ToolInfo = ToolInfo {
    name: "Shipment",
    description: "Pays a 1.5x bonus for a specific target item.",
    cost: Cost { credits: 60 },
};
const INFO_POLE: ToolInfo = ToolInfo {
    name: "Power Pole",
    description: "Connects generators to consumers within range.",
    cost: Cost { credits: 10 },
};
const INFO_GENERATOR: ToolInfo = ToolInfo {
    name: "Generator",
    description: "Powers nearby consumers and poles.",
    cost: Cost { credits: 200 },
};
const INFO_PIPE: ToolInfo = ToolInfo {
    name: "Pipe",
    description: "Carries fluids between buildings.",
    cost: Cost { credits: 5 },
};
const INFO_PUMP: ToolInfo = ToolInfo {
    name: "Pump",
    description: "Pumps groundwater into pipes.",
    cost: Cost { credits: 120 },
};
const INFO_TANK: ToolInfo = ToolInfo {
    name: "Fluid Tank",
    description: "Stores fluid for later use.",
    cost: Cost { credits: 35 },
};
const INFO_RESEARCH1: ToolInfo = ToolInfo {
    name: "Research Center T1",
    description: "Produces research points slowly when powered.",
    cost: Cost { credits: 250 },
};
const INFO_RESEARCH2: ToolInfo = ToolInfo {
    name: "Research Center T2",
    description: "Consumes circuits to produce research points faster.",
    cost: Cost { credits: 700 },
};
const INFO_RESEARCH3: ToolInfo = ToolInfo {
    name: "Research Center T3",
    description: "Consumes science packs for the highest research throughput.",
    cost: Cost { credits: 1800 },
};
const INFO_RAILTRACK: ToolInfo = ToolInfo {
    name: "Rail Track",
    description: "Connects rail stations into a logistics network.",
    cost: Cost { credits: 15 },
};
const INFO_RAILSTATION: ToolInfo = ToolInfo {
    name: "Rail Station",
    description: "Loads and unloads cargo for trains on the same network.",
    cost: Cost { credits: 120 },
};
const INFO_TURRET: ToolInfo = ToolInfo {
    name: "Turret",
    description: "Powered defense. Feed circuits as ammo and steel for repairs.",
    cost: Cost { credits: 250 },
};
const INFO_FORGECORE: ToolInfo = ToolInfo {
    name: "Forge Core",
    description: "Endgame project. Deliver steel, circuits, then science packs.",
    cost: Cost { credits: 5000 },
};

/// How much money the player receives for shipping one of an item kind.
pub const fn item_value(kind: u16) -> i32 {
    match kind {
        0 => 3,    // iron
        1 => 3,    // copper
        2 => 2,    // coal
        3 => 10,   // gear
        4 => 18,   // steel
        5 => 1,    // stone
        6 => 4,    // oil
        7 => 20,   // plastic
        8 => 80,   // circuit
        9 => 6,    // brick
        10 => 250, // science
        _ => 1,
    }
}

/// Payout for a shipment delivery (bonus over normal item value).
pub const fn shipment_value(kind: u16) -> i32 {
    item_value(kind) + item_value(kind) / 2
}

/// Human-readable item names for economy UI.
pub const ITEM_NAMES: [&str; KINDS] = [
    "iron", "copper", "coal", "gear", "steel", "stone", "oil", "plastic", "circuit", "brick",
    "science",
];

pub fn tool_info(tool: Tool) -> &'static ToolInfo {
    match tool {
        Tool::Select => &INFO_SELECT,
        Tool::Paste => &INFO_PASTE,
        Tool::Belt => &INFO_BELT,
        Tool::Inserter => &INFO_INSERTER,
        Tool::Splitter => &INFO_SPLITTER,
        Tool::Source => &INFO_SOURCE,
        Tool::Sink => &INFO_SINK,
        Tool::Assembler => &INFO_ASSEMBLER,
        Tool::Miner => &INFO_MINER,
        Tool::Storage => &INFO_STORAGE,
        Tool::Shipment => &INFO_SHIPMENT,
        Tool::Pole => &INFO_POLE,
        Tool::Generator => &INFO_GENERATOR,
        Tool::Pipe => &INFO_PIPE,
        Tool::Pump => &INFO_PUMP,
        Tool::Tank => &INFO_TANK,
        Tool::Research1 => &INFO_RESEARCH1,
        Tool::Research2 => &INFO_RESEARCH2,
        Tool::Research3 => &INFO_RESEARCH3,
        Tool::RailTrack => &INFO_RAILTRACK,
        Tool::RailStation => &INFO_RAILSTATION,
        Tool::Turret => &INFO_TURRET,
        Tool::ForgeCore => &INFO_FORGECORE,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Logistics,
    Production,
    PowerFluids,
    Rail,
    Combat,
    Tools,
}

impl ToolCategory {
    pub const fn name(self) -> &'static str {
        match self {
            ToolCategory::Logistics => "Logistics",
            ToolCategory::Production => "Production",
            ToolCategory::PowerFluids => "Power & Fluids",
            ToolCategory::Rail => "Rail",
            ToolCategory::Combat => "Combat",
            ToolCategory::Tools => "Tools",
        }
    }
}

pub fn tool_category(tool: Tool) -> ToolCategory {
    match tool {
        Tool::Belt | Tool::Inserter | Tool::Splitter => ToolCategory::Logistics,
        Tool::Miner
        | Tool::Assembler
        | Tool::Source
        | Tool::Sink
        | Tool::Storage
        | Tool::Shipment => ToolCategory::Production,
        Tool::Pole | Tool::Generator | Tool::Pipe | Tool::Pump | Tool::Tank => {
            ToolCategory::PowerFluids
        }
        Tool::Research1 | Tool::Research2 | Tool::Research3 => ToolCategory::Production,
        Tool::RailTrack | Tool::RailStation => ToolCategory::Rail,
        Tool::Turret => ToolCategory::Combat,
        Tool::ForgeCore => ToolCategory::Production,
        Tool::Select | Tool::Paste => ToolCategory::Tools,
    }
}

/// Fuel requirements and research-point yield for each research-center tier.
/// Tier is 0-based (param 0,1,2 on the Lab building).
pub struct ResearchTier {
    pub fuel_kind: u16,
    pub fuel_amount: u16,
    pub points: u32,
    pub cooldown: u16,
}

pub const RESEARCH_TIERS: [ResearchTier; 3] = [
    // T1: powered only, very slow
    ResearchTier {
        fuel_kind: 0,
        fuel_amount: 0,
        points: 1,
        cooldown: 60,
    },
    // T2: consumes one circuit per cycle
    ResearchTier {
        fuel_kind: 8, // circuit
        fuel_amount: 1,
        points: 5,
        cooldown: 30,
    },
    // T3: consumes one science pack per cycle
    ResearchTier {
        fuel_kind: 10, // science
        fuel_amount: 1,
        points: 25,
        cooldown: 15,
    },
];

/// Technologies that can be unlocked by spending research points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tech {
    Splitter,
    Shipment,
    PowerFluids,
    AdvancedResearch,
    RailLogistics,
    Combat,
    ForgeCore,
    Creative,
}

impl Tech {
    pub const fn idx(self) -> u32 {
        match self {
            Tech::Splitter => 0,
            Tech::Shipment => 1,
            Tech::PowerFluids => 2,
            Tech::AdvancedResearch => 3,
            Tech::RailLogistics => 4,
            Tech::Combat => 5,
            Tech::ForgeCore => 6,
            Tech::Creative => 7,
        }
    }

    pub const fn cost(self) -> i32 {
        match self {
            Tech::Splitter => 50,
            Tech::Shipment => 100,
            Tech::PowerFluids => 150,
            Tech::AdvancedResearch => 300,
            Tech::RailLogistics => 500,
            Tech::Combat => 400,
            Tech::ForgeCore => 1200,
            Tech::Creative => 0,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Tech::Splitter => "Fast Logistics",
            Tech::Shipment => "Shipment Contracts",
            Tech::PowerFluids => "Power & Fluids",
            Tech::AdvancedResearch => "Advanced Research",
            Tech::RailLogistics => "Rail Logistics",
            Tech::Combat => "Defensive Systems",
            Tech::ForgeCore => "Forge Ascension",
            Tech::Creative => "Sandbox Mode",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Tech::Splitter => "Unlocks splitters for belt distribution.",
            Tech::Shipment => "Unlocks shipments that pay a premium for target items.",
            Tech::PowerFluids => "Unlocks generators, pumps, and tanks.",
            Tech::AdvancedResearch => "Unlocks Research Center T2 and T3.",
            Tech::RailLogistics => "Unlocks rail tracks and train stations.",
            Tech::Combat => "Unlocks turrets for base defense.",
            Tech::ForgeCore => "Unlocks the staged Forge Core endgame project.",
            Tech::Creative => "Unlocked by winning. Enables Source and Sink.",
        }
    }
}

/// Which technology gates a given tool. Basic tools return `None`.
pub const fn tech_for_tool(tool: Tool) -> Option<Tech> {
    match tool {
        Tool::Splitter => Some(Tech::Splitter),
        Tool::Shipment => Some(Tech::Shipment),
        Tool::Generator | Tool::Pump | Tool::Tank => Some(Tech::PowerFluids),
        Tool::Source => Some(Tech::Creative),
        Tool::Research2 | Tool::Research3 => Some(Tech::AdvancedResearch),
        Tool::RailTrack | Tool::RailStation => Some(Tech::RailLogistics),
        Tool::Turret => Some(Tech::Combat),
        Tool::ForgeCore => Some(Tech::ForgeCore),
        Tool::Select | Tool::Paste | Tool::Belt | Tool::Inserter | Tool::Assembler
        | Tool::Miner | Tool::Storage | Tool::Pole | Tool::Pipe | Tool::Sink | Tool::Research1 => None,
    }
}

pub fn is_tech_unlocked(flags: u64, tech: Tech) -> bool {
    flags & (1u64 << tech.idx()) != 0
}

pub fn unlock_tech(flags: &mut u64, tech: Tech) {
    *flags |= 1u64 << tech.idx();
}

pub fn is_tool_unlocked(tool: Tool, flags: u64) -> bool {
    match tech_for_tool(tool) {
        Some(t) => is_tech_unlocked(flags, t),
        None => true,
    }
}
