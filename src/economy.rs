//! Economy, costs, and research-point model.

use bevy::prelude::*;

use crate::ui::Tool;
use crate::belts::KINDS;

/// Player wallet and progression currency.
#[derive(Resource, Default)]
pub struct PlayerState {
    pub credits: i32,
    pub research_points: i32,
}

impl PlayerState {
    pub fn with_starting_funds() -> Self {
        Self {
            credits: 500,
            research_points: 0,
        }
    }
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
    cost: Cost { credits: 5 },
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
    cost: Cost { credits: 50 },
};
const INFO_SINK: ToolInfo = ToolInfo {
    name: "Sink",
    description: "Consumes any item and sells it for credits.",
    cost: Cost { credits: 20 },
};
const INFO_ASSEMBLER: ToolInfo = ToolInfo {
    name: "Assembler",
    description: "Crafts recipes from inputs.",
    cost: Cost { credits: 80 },
};
const INFO_MINER: ToolInfo = ToolInfo {
    name: "Miner",
    description: "Extracts ore from the tile underneath.",
    cost: Cost { credits: 60 },
};
const INFO_STORAGE: ToolInfo = ToolInfo {
    name: "Storage",
    description: "Buffers up to 50 of each item.",
    cost: Cost { credits: 30 },
};
const INFO_SHIPMENT: ToolInfo = ToolInfo {
    name: "Shipment",
    description: "Pays a bonus for a specific target item.",
    cost: Cost { credits: 40 },
};
const INFO_POLE: ToolInfo = ToolInfo {
    name: "Power Pole",
    description: "Connects generators to consumers within range.",
    cost: Cost { credits: 10 },
};
const INFO_GENERATOR: ToolInfo = ToolInfo {
    name: "Generator",
    description: "Powers nearby consumers and poles.",
    cost: Cost { credits: 120 },
};
const INFO_PIPE: ToolInfo = ToolInfo {
    name: "Pipe",
    description: "Carries fluids between buildings.",
    cost: Cost { credits: 8 },
};
const INFO_PUMP: ToolInfo = ToolInfo {
    name: "Pump",
    description: "Pumps groundwater into pipes.",
    cost: Cost { credits: 70 },
};
const INFO_TANK: ToolInfo = ToolInfo {
    name: "Fluid Tank",
    description: "Stores fluid for later use.",
    cost: Cost { credits: 35 },
};
const INFO_RESEARCH1: ToolInfo = ToolInfo {
    name: "Research Center T1",
    description: "Produces research points slowly when powered.",
    cost: Cost { credits: 200 },
};
const INFO_RESEARCH2: ToolInfo = ToolInfo {
    name: "Research Center T2",
    description: "Consumes circuits to produce research points faster.",
    cost: Cost { credits: 500 },
};
const INFO_RESEARCH3: ToolInfo = ToolInfo {
    name: "Research Center T3",
    description: "Consumes science packs for the highest research throughput.",
    cost: Cost { credits: 1200 },
};

/// How much money the player receives for shipping one of an item kind.
pub const fn item_value(kind: u16) -> i32 {
    match kind {
        0 => 2,   // iron
        1 => 2,   // copper
        2 => 2,   // coal
        3 => 5,   // gear
        4 => 8,   // steel
        5 => 1,   // stone
        6 => 3,   // oil
        7 => 10,  // plastic
        8 => 25,  // circuit
        9 => 4,   // brick
        10 => 50, // science
        _ => 1,
    }
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
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Logistics,
    Production,
    PowerFluids,
    Tools,
}

impl ToolCategory {
    pub const fn name(self) -> &'static str {
        match self {
            ToolCategory::Logistics => "Logistics",
            ToolCategory::Production => "Production",
            ToolCategory::PowerFluids => "Power & Fluids",
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
