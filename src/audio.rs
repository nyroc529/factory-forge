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
    let seconds = 16;
    let count = sample_rate * seconds;
    let tau = std::f32::consts::TAU;

    // D-minor / dark ambient palette.
    let root: f32 = 73.42; // D2
    let bass = [root, root * 1.5]; // D2 + A2 drone
    let chord_progression = [
        [root * 2.0, root * 2.5, root * 3.0],       // Dm
        [root * 1.5, root * 2.0, root * 2.5],       // Am
        [root * 1.75, root * 2.25, root * 2.75],    // Gm-ish
        [root * 2.0, root * 2.5, root * 3.0],       // Dm
    ];
    let melody = [
        (root * 2.0, 1.0), (root * 2.5, 1.0), (0.0, 1.0),
        (root * 2.25, 1.0), (root * 2.0, 1.0), (root * 1.75, 1.0),
        (0.0, 1.0), (root * 1.5, 1.0),
    ];

    let bpm = 70.0;
    let beat = (60.0 / bpm) * sample_rate as f32;

    // Soft triangle oscillator (less buzzy than saw, darker than sine).
    let tri = |phase: f32| -> f32 {
        let p = phase - phase.floor();
        4.0 * (p - 0.5).abs() - 1.0
    };

    // ADSR-ish envelope scaled to note length.
    let amp_env = |t: f32, duration: f32| -> f32 {
        if t < 0.0 || t >= duration {
            return 0.0;
        }
        let attack = duration * 0.15;
        let release = duration * 0.35;
        if t < attack {
            t / attack
        } else if t > duration - release {
            1.0 - (t - (duration - release)) / release
        } else {
            1.0
        }
    };

    let mut out = vec![0.0_f32; count];

    // Bass drone: two low detuned oscillators, slow pulse.
    for i in 0..count {
        let t = i as f32 / sample_rate as f32;
        let mut v = 0.0_f32;
        for f in bass {
            v += 0.6 * (t * f * tau).sin();
            v += 0.4 * tri(t * (f + 0.25) + 0.13);
        }
        let pulse = 0.55 + 0.45 * (t * 0.15 * tau).sin();
        out[i] += v * pulse * 0.18;
    }

    // Dark pad: chord progression, one chord every 4 beats.
    let measures = 4;
    let beats_per_measure = 4;
    let chord_duration = beat * beats_per_measure as f32;
    for m in 0..measures {
        let chord = chord_progression[m % chord_progression.len()];
        let start = (m as f32 * chord_duration) as usize;
        let end = ((m + 1) as f32 * chord_duration) as usize;
        for i in start..end.min(count) {
            let t_local = (i - start) as f32 / sample_rate as f32;
            let env = amp_env(t_local, chord_duration / sample_rate as f32);
            let t = i as f32 / sample_rate as f32;
            let mut v = 0.0_f32;
            for &f in &chord {
                v += tri(t * f + m as f32 * 0.1);
                // slight detune for thickness
                v += 0.5 * tri(t * (f + 0.4));
            }
            out[i] += v * env * 0.10;
        }
    }

    // Sparse melody: half-note phrases.
    let mut melody_time = 0.0_f32;
    for &(freq, beats) in &melody {
        let duration = beats * beat;
        if freq > 0.0 {
            let start = melody_time as usize;
            let end = (melody_time + duration) as usize;
            for i in start..end.min(count) {
                let t_local = (i - start) as f32 / sample_rate as f32;
                let t = i as f32 / sample_rate as f32;
                let env = amp_env(t_local, duration / sample_rate as f32);
                let v = 0.5 * tri(t * freq) + 0.5 * (t * freq * tau).sin();
                out[i] += v * env * 0.12;
            }
        }
        melody_time += duration;
    }

    // Final normalization and output.
    out.into_iter()
        .map(|v| {
            let clamped = v.clamp(-1.0, 1.0);
            (clamped * i16::MAX as f32) as i16
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
