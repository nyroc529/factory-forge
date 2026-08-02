use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::settings::Settings;

pub const ASSETS_DIR: &str = "assets/audio";

#[derive(Clone, Copy)]
pub enum SfxKind {
    Place,
}

#[derive(Resource, Default)]
pub struct SfxQueue(pub Vec<SfxKind>);

#[derive(Resource)]
pub struct AudioAssets {
    #[allow(dead_code)]
    pub ambient: Handle<AudioSource>,
    pub click: Handle<AudioSource>,
}

#[derive(Component)]
pub struct AmbientMusic;

fn write_wav(path: &str, samples: &[i16], sample_rate: u32) {
    let file = File::create(path).expect("failed to create audio asset");
    let mut w = BufWriter::new(file);
    let data_size = (samples.len() * 2) as u32;

    w.write_all(b"RIFF").unwrap();
    w.write_all(&(36 + data_size).to_le_bytes()).unwrap();
    w.write_all(b"WAVE").unwrap();
    w.write_all(b"fmt ").unwrap();
    w.write_all(&16u32.to_le_bytes()).unwrap();
    w.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    w.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    w.write_all(&sample_rate.to_le_bytes()).unwrap();
    w.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
    w.write_all(&2u16.to_le_bytes()).unwrap();
    w.write_all(&16u16.to_le_bytes()).unwrap();
    w.write_all(b"data").unwrap();
    w.write_all(&data_size.to_le_bytes()).unwrap();
    for sample in samples {
        w.write_all(&sample.to_le_bytes()).unwrap();
    }
}

fn ambient_samples() -> Vec<i16> {
    let sample_rate = 44100;
    let seconds = 8;
    let count = sample_rate * seconds;
    let tau = std::f32::consts::TAU;
    // C-minor chord drone.
    let chord = [130.81_f32, 155.56, 196.00];
    // Slow pentatonic arpeggio over the chord (half-second notes).
    let melody = [130.81_f32, 155.56, 196.00, 233.08, 261.63, 233.08, 196.00, 155.56];
    let step = sample_rate / 2;
    (0..count)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let mut v = 0.0_f32;
            for f in chord {
                v += (t * f * tau).sin();
            }
            let note_idx = (i / step) % melody.len();
            let f = melody[note_idx];
            v += 0.6 * (t * f * tau).sin();
            // Gentle swell so it breathes instead of droning flatly.
            let pulse = 0.65 + 0.35 * (t * 0.2 * tau).sin();
            v *= pulse * 0.12 / chord.len() as f32;
            (v * i16::MAX as f32) as i16
        })
        .collect()
}

fn click_samples() -> Vec<i16> {
    let sample_rate = 44100;
    let duration = 0.06;
    let count = (sample_rate as f32 * duration) as usize;
    let freq = 880.0;
    (0..count)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let envelope = 1.0 - (i as f32 / count as f32);
            let v = (t * freq * std::f32::consts::TAU).sin() * envelope * 0.4 * i16::MAX as f32;
            v as i16
        })
        .collect()
}

pub fn ensure_audio_assets() {
    let base = std::env::var("BEVY_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."))
        });

    let dir = base.join(ASSETS_DIR);
    fs::create_dir_all(&dir).ok();

    let ambient_path = dir.join("ambient.wav");
    let click_path = dir.join("click.wav");

    if !ambient_path.exists() {
        write_wav(ambient_path.to_str().unwrap(), &ambient_samples(), 44100);
    }
    if !click_path.exists() {
        write_wav(click_path.to_str().unwrap(), &click_samples(), 44100);
    }
}

pub fn setup_audio(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Res<Settings>,
) {
    let ambient = asset_server.load("audio/ambient.wav");
    let click = asset_server.load("audio/click.wav");

    commands.insert_resource(AudioAssets {
        ambient: ambient.clone(),
        click,
    });

    commands.spawn((
        AudioBundle {
            source: ambient,
            settings: PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::new(settings.master_volume * settings.music_volume),
                ..default()
            },
        },
        AmbientMusic,
    ));
}

pub fn update_volumes(settings: Res<Settings>, sinks: Query<&AudioSink, With<AmbientMusic>>) {
    if settings.is_changed() {
        let volume = settings.master_volume * settings.music_volume;
        for sink in sinks.iter() {
            sink.set_volume(volume);
        }
    }
}

pub fn play_dirty_sfx(
    dirty: Res<crate::render::WorldDirty>,
    mut queue: ResMut<SfxQueue>,
    mut state: Local<(bool, u32)>,
) {
    state.1 += 1;
    if state.1 == 1 {
        state.0 = dirty.0;
        return;
    }
    if dirty.0 && !state.0 {
        queue.0.push(SfxKind::Place);
    }
    state.0 = dirty.0;
}

pub fn play_sfx(
    mut commands: Commands,
    mut queue: ResMut<SfxQueue>,
    assets: Res<AudioAssets>,
    settings: Res<Settings>,
) {
    for _kind in queue.0.drain(..) {
        commands.spawn(AudioBundle {
            source: assets.click.clone(),
            settings: PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::new(settings.master_volume * settings.sfx_volume),
                ..default()
            },
        });
    }
}
