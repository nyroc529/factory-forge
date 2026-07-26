//! Flat 2D grid. One u32 per tile pointing at a belt slot (or INVALID).

use serde::{Deserialize, Serialize};

use crate::belts::INVALID;

use std::collections::HashMap;

pub const CHUNK_SIZE: i32 = 32;
pub const CHUNK_TILES: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

#[derive(Serialize, Deserialize, Clone)]
pub struct Chunk {
    pub tile_belt: Vec<u32>,
    pub tile_building: Vec<u32>,
    pub tile_ore: Vec<u8>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            tile_belt: vec![INVALID; CHUNK_TILES],
            tile_building: vec![INVALID; CHUNK_TILES],
            tile_ore: vec![0; CHUNK_TILES],
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    pub chunks: HashMap<(i32, i32), Chunk>,
}

impl Grid {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            chunks: HashMap::new(),
        }
    }

    #[inline]
    pub fn chunk_key(x: i32, y: i32) -> (i32, i32) {
        (x / CHUNK_SIZE, y / CHUNK_SIZE)
    }

    #[inline]
    pub fn local_index(x: i32, y: i32) -> usize {
        let lx = x.rem_euclid(CHUNK_SIZE) as usize;
        let ly = y.rem_euclid(CHUNK_SIZE) as usize;
        ly * CHUNK_SIZE as usize + lx
    }

    #[inline]
    fn chunk(&self, x: i32, y: i32) -> Option<&Chunk> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            self.chunks.get(&Self::chunk_key(x, y))
        }
    }

    #[inline]
    fn chunk_or_create(&mut self, x: i32, y: i32) -> Option<&mut Chunk> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some(self.chunks.entry(Self::chunk_key(x, y)).or_default())
        }
    }

    pub fn belt_at(&self, x: i32, y: i32) -> u32 {
        self.chunk(x, y)
            .map_or(INVALID, |c| c.tile_belt[Self::local_index(x, y)])
    }

    pub fn set_belt(&mut self, x: i32, y: i32, belt: u32) {
        if let Some(c) = self.chunk_or_create(x, y) {
            c.tile_belt[Self::local_index(x, y)] = belt;
        }
    }

    pub fn building_at(&self, x: i32, y: i32) -> u32 {
        self.chunk(x, y)
            .map_or(INVALID, |c| c.tile_building[Self::local_index(x, y)])
    }

    pub fn set_building(&mut self, x: i32, y: i32, id: u32) {
        if let Some(c) = self.chunk_or_create(x, y) {
            c.tile_building[Self::local_index(x, y)] = id;
        }
    }

    pub fn is_empty(&self, x: i32, y: i32) -> bool {
        self.chunk(x, y).map_or(false, |c| {
            let i = Self::local_index(x, y);
            c.tile_belt[i] == INVALID && c.tile_building[i] == INVALID
        })
    }

    pub fn ore_at(&self, x: i32, y: i32) -> u8 {
        self.chunk(x, y)
            .map_or(0, |c| c.tile_ore[Self::local_index(x, y)])
    }

    pub fn set_ore(&mut self, x: i32, y: i32, kind: u8) {
        if let Some(c) = self.chunk_or_create(x, y) {
            c.tile_ore[Self::local_index(x, y)] = kind;
        }
    }
}
