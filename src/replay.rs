use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::belts::BeltSim;
use crate::economy::PlayerState;
use crate::grid::Grid;
use crate::{GameWorld, Sim};

const REPLAY_INTERVAL: u64 = 300; // five seconds at 60 Hz
const MAX_FRAMES: usize = 120; // ten minutes of buffer
const REPLAY_PATH: &str = "factory.replay";

#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayFrame {
    pub tick: u64,
    pub sim: BeltSim,
    pub grid: Grid,
    pub player: PlayerState,
}

#[derive(Resource, Default, Serialize, Deserialize)]
pub struct ReplayLog {
    pub tick: u64,
    pub frames: Vec<ReplayFrame>,
}

impl ReplayLog {
    pub fn record(&mut self, tick: u64, sim: &BeltSim, grid: &Grid, player: &PlayerState) {
        self.tick = tick;
        if tick % REPLAY_INTERVAL == 0 {
            self.frames.push(ReplayFrame {
                tick,
                sim: sim.clone(),
                grid: grid.clone(),
                player: player.clone(),
            });
            if self.frames.len() > MAX_FRAMES {
                self.frames.remove(0);
            }
        }
    }

    pub fn save(&self) {
        match bincode::serialize(self) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(REPLAY_PATH, bytes) {
                    eprintln!("replay save failed: {e}");
                } else {
                    println!("saved replay to {REPLAY_PATH}");
                }
            }
            Err(e) => eprintln!("replay serialize failed: {e}"),
        }
    }
}

pub fn record(
    mut replay: ResMut<ReplayLog>,
    sim: Res<Sim>,
    world: Res<GameWorld>,
    player: Res<PlayerState>,
    mut tick: Local<u64>,
) {
    *tick += 1;
    replay.record(*tick, &sim.0, &world.0, &player);
}
