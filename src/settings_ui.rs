use bevy::prelude::*;

use crate::settings::{save, Settings};

#[derive(Resource, Default)]
pub struct SettingsMenuVisible(pub bool);

#[derive(Component)]
pub struct SettingsOverlay;

const VOLUME_STEP: f32 = 0.05;

pub fn setup_settings_ui(mut commands: Commands) {
    commands.spawn((
        TextBundle::from_section(
            "",
            TextStyle {
                font_size: 18.0,
                color: Color::srgb(0.95, 0.95, 0.98),
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(100.0),
            ..default()
        })
        .with_background_color(Color::srgba(0.0, 0.0, 0.0, 0.85)),
        SettingsOverlay,
    ));
}

pub fn toggle_settings_ui(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<SettingsMenuVisible>,
) {
    if keys.just_pressed(KeyCode::KeyO) {
        visible.0 = !visible.0;
    }
}

fn active_volume(keys: &ButtonInput<KeyCode>) -> Option<&'static str> {
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        Some("music")
    } else if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        Some("sfx")
    } else if keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight) {
        None
    } else {
        Some("master")
    }
}

pub fn update_settings_ui(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<Settings>,
    visible: Res<SettingsMenuVisible>,
    mut query: Query<&mut Text, With<SettingsOverlay>>,
) {
    if let Ok(mut text) = query.get_single_mut() {
        if !visible.0 {
            if !text.sections[0].value.is_empty() {
                text.sections[0].value.clear();
            }
            return;
        }

        let minus = keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract);
        let plus = keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd);

        if minus || plus {
            let delta = if minus { -VOLUME_STEP } else { VOLUME_STEP };
            match active_volume(&keys) {
                Some("master") => settings.master_volume += delta,
                Some("music") => settings.music_volume += delta,
                Some("sfx") => settings.sfx_volume += delta,
                _ => {}
            }
        }

        if keys.just_pressed(KeyCode::KeyF) {
            settings.fullscreen = !settings.fullscreen;
        }

        settings.master_volume = settings.master_volume.clamp(0.0, 1.0);
        settings.music_volume = settings.music_volume.clamp(0.0, 1.0);
        settings.sfx_volume = settings.sfx_volume.clamp(0.0, 1.0);

        let pct = |v: f32| (v * 100.0).round() as i32;
        text.sections[0].value = format!(
            " SETTINGS (O to close)\n\
             \n\
              Master:  {:3}%   [ - / = ]\n\
              Music:   {:3}%   [ Shift + - / = ]\n\
              SFX:     {:3}%   [ Ctrl + - / = ]\n\
             \n\
              Fullscreen: {}   (F)\n\
              Bloom:      {}   (B)\n ",
            pct(settings.master_volume),
            pct(settings.music_volume),
            pct(settings.sfx_volume),
            if settings.fullscreen { "ON " } else { "OFF" },
            if settings.bloom { "ON " } else { "OFF" },
        );
    }
}

pub fn apply_settings(
    mut settings: ResMut<Settings>,
    mut windows: Query<&mut Window>,
) {
    if settings.is_changed() {
        settings.master_volume = settings.master_volume.clamp(0.0, 1.0);
        settings.music_volume = settings.music_volume.clamp(0.0, 1.0);
        settings.sfx_volume = settings.sfx_volume.clamp(0.0, 1.0);

        let target_mode = if settings.fullscreen {
            bevy::window::WindowMode::BorderlessFullscreen
        } else {
            bevy::window::WindowMode::Windowed
        };
        for mut window in windows.iter_mut() {
            if window.mode != target_mode {
                window.mode = target_mode;
            }
        }

        save(&*settings);
    }
}
