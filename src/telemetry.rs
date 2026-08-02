use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::belts::KINDS;
use crate::economy::{ProductionStats, ITEM_NAMES};

const TELEMETRY_INTERVAL: u64 = 60; // one second at 60 Hz
const MAX_SAMPLES: usize = 120; // two minutes of history

#[derive(Resource)]
pub struct GraphVisible(pub bool);

impl Default for GraphVisible {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Component)]
pub struct GraphOverlay;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TelemetrySample {
    pub tick: u64,
    pub sold: [u64; KINDS],
    pub shipped: [u64; KINDS],
}

#[derive(Resource, Default, Clone, Serialize, Deserialize)]
pub struct Telemetry {
    pub tick: u64,
    pub samples: Vec<TelemetrySample>,
    pub last_sold: [u64; KINDS],
    pub last_shipped: [u64; KINDS],
    pub sold_rate: [f32; KINDS],
    pub shipped_rate: [f32; KINDS],
}

impl Telemetry {
    pub fn record(&mut self, tick: u64, stats: &ProductionStats) {
        self.tick = tick;
        if tick % TELEMETRY_INTERVAL == 0 {
            self.samples.push(TelemetrySample {
                tick,
                sold: stats.sold,
                shipped: stats.shipped,
            });
            if self.samples.len() > MAX_SAMPLES {
                self.samples.remove(0);
            }

            for k in 0..KINDS {
                let sold_delta = stats.sold[k].saturating_sub(self.last_sold[k]) as f32;
                let shipped_delta = stats.shipped[k].saturating_sub(self.last_shipped[k]) as f32;
                self.sold_rate[k] = sold_delta * 60.0; // items per minute
                self.shipped_rate[k] = shipped_delta * 60.0;
            }
            self.last_sold = stats.sold;
            self.last_shipped = stats.shipped;
        }
    }
}

pub fn record(
    mut telemetry: ResMut<Telemetry>,
    stats: Res<ProductionStats>,
    mut tick: Local<u64>,
) {
    *tick += 1;
    telemetry.record(*tick, &stats);
}

pub fn setup_graph(mut commands: Commands) {
    commands.spawn((
        TextBundle::from_section(
            "",
            TextStyle {
                font_size: 14.0,
                color: Color::srgb(0.8, 0.85, 0.92),
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            top: Val::Px(80.0),
            ..default()
        }),
        GraphOverlay,
    ));
}

pub fn toggle_graph(keys: Res<ButtonInput<KeyCode>>, mut visible: ResMut<GraphVisible>) {
    if keys.just_pressed(KeyCode::KeyG) {
        visible.0 = !visible.0;
    }
}

pub fn update_graph_overlay(
    telemetry: Res<Telemetry>,
    visible: Res<GraphVisible>,
    mut q: Query<&mut Text, With<GraphOverlay>>,
    mut counter: Local<u32>,
) {
    *counter += 1;
    if *counter % 6 != 0 {
        return;
    }
    if let Ok(mut text) = q.get_single_mut() {
        if !visible.0 {
            text.sections[0].value = String::new();
            return;
        }
        text.sections[0].value = format_graph(&telemetry);
    }
}

fn format_graph(telemetry: &Telemetry) -> String {
    let mut rates: Vec<(usize, f32)> = telemetry
        .shipped_rate
        .iter()
        .enumerate()
        .map(|(i, &r)| (i, r))
        .collect();
    rates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let max = rates.iter().map(|(_, r)| *r).fold(0.0, f32::max).max(1.0);

    let mut out = String::from("PRODUCTION / min\n");
    let bar_len = 10;
    for (kind, rate) in rates.iter().take(5) {
        let filled = ((rate / max) * bar_len as f32).round() as usize;
        let bar = "█".repeat(filled) + &"░".repeat(bar_len - filled);
        out.push_str(&format!("{:8} |{}| {:.1}\n", ITEM_NAMES[*kind], bar, rate));
    }
    out
}
