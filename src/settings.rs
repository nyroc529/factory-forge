use bevy::app::AppExit;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "factory-forge.settings";

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub fullscreen: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub bloom: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            music_volume: 1.0,
            sfx_volume: 1.0,
            fullscreen: false,
            window_width: 1280,
            window_height: 720,
            bloom: false,
        }
    }
}

/// Load persisted settings, falling back to defaults on any error.
pub fn load() -> Settings {
    let mut settings: Settings = std::fs::read(SETTINGS_FILE)
        .ok()
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
        .unwrap_or_default();
    clamp_volumes(&mut settings);
    settings
}

/// Write settings to disk for the next launch.
pub fn save(settings: &Settings) {
    if let Ok(bytes) = bincode::serialize(settings) {
        if let Err(e) = std::fs::write(SETTINGS_FILE, bytes) {
            eprintln!("settings save failed: {e}");
        }
    }
}

/// Persist settings whenever the app is exiting.
pub fn save_system(settings: Res<Settings>, mut exit: EventReader<AppExit>) {
    for _ in exit.read() {
        save(&settings);
    }
}

/// Keep all volumes in a valid 0..1 range.
pub fn clamp_volumes(settings: &mut Settings) {
    settings.master_volume = settings.master_volume.clamp(0.0, 1.0);
    settings.music_volume = settings.music_volume.clamp(0.0, 1.0);
    settings.sfx_volume = settings.sfx_volume.clamp(0.0, 1.0);
}
