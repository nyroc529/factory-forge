//! Runtime-generated top-down building sprites.
//!
//! Instead of colored rectangles, each building type gets a small shaded box
//! with an icon and an output-direction arrow.  The sprites are drawn with
//! tiny-skia so they are antialiased and look closer to a real factory game.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::texture::Image;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::belts::BuildingKind;

const SIZE: u32 = 64;

fn base_color() -> Color { Color::from_rgba8(17, 19, 23, 255) }
fn border_color() -> Color { Color::from_rgba8(55, 65, 81, 255) }
fn shadow_color() -> Color { Color::from_rgba8(0, 0, 0, 120) }
fn white_color() -> Color { Color::from_rgba8(230, 230, 240, 255) }

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

#[derive(Resource)]
pub struct BuildingSpriteSet {
    pub handles: Vec<Handle<Image>>,
}

/// Generate one texture per building kind and store the handles.
pub fn setup_sprites(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let kinds = all_kinds();
    let mut handles = Vec::with_capacity(kinds.len());
    for &kind in &kinds {
        let pixmap = draw_sprite(kind);
        let image = Image::new(
            Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixmap.data().to_vec(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        handles.push(images.add(image));
    }
    commands.insert_resource(BuildingSpriteSet { handles });
}

fn all_kinds() -> Vec<BuildingKind> {
    use BuildingKind::*;
    vec![
        Source, Sink, Assembler, Inserter, Miner, Storage, Shipment, Splitter, Pole, Generator,
        Pipe, Pump, Tank, Lab, RailTrack, RailStation, Turret, ForgeCore,
    ]
}

fn draw_sprite(kind: BuildingKind) -> Pixmap {
    let mut pixmap = Pixmap::new(SIZE, SIZE).unwrap();
    let mut pm = pixmap.as_mut();
    pm.fill(Color::from_rgba8(0, 0, 0, 0));

    let accent = accent_for(kind);

    // Shaded machine body.
    fill_rect(&mut pm, Rect::from_xywh(6.0, 10.0, 56.0, 48.0).unwrap(), shadow_color());
    fill_rect(&mut pm, Rect::from_xywh(4.0, 8.0, 56.0, 48.0).unwrap(), base_color());
    stroke_rect(&mut pm, Rect::from_xywh(4.0, 8.0, 56.0, 48.0).unwrap(), border_color(), 2.0);

    // Colored top band for quick identification.
    fill_rect(&mut pm, Rect::from_xywh(6.0, 10.0, 52.0, 8.0).unwrap(), accent);

    // Type-specific icon.
    draw_icon(&mut pm, kind, accent);

    // Direction arrow (sprite is rotated by the renderer, so it always points east).
    draw_dir_arrow(&mut pm);

    pixmap
}

fn accent_for(kind: BuildingKind) -> Color {
    use BuildingKind::*;
    match kind {
        Source => Color::from_rgba8(45, 212, 191, 255),
        Sink => Color::from_rgba8(217, 119, 6, 255),
        Assembler => Color::from_rgba8(59, 130, 246, 255),
        Inserter => Color::from_rgba8(245, 158, 11, 255),
        Miner => Color::from_rgba8(239, 68, 68, 255),
        Storage => Color::from_rgba8(6, 182, 212, 255),
        Shipment => Color::from_rgba8(34, 197, 94, 255),
        Splitter => Color::from_rgba8(234, 179, 8, 255),
        Pole => Color::from_rgba8(156, 163, 175, 255),
        Generator => Color::from_rgba8(250, 204, 21, 255),
        Pipe => Color::from_rgba8(100, 116, 139, 255),
        Pump => Color::from_rgba8(96, 165, 250, 255),
        Tank => Color::from_rgba8(147, 197, 253, 255),
        Lab => Color::from_rgba8(168, 85, 247, 255),
        RailTrack => Color::from_rgba8(120, 113, 108, 255),
        RailStation => Color::from_rgba8(249, 115, 22, 255),
        Turret => Color::from_rgba8(239, 68, 68, 255),
        ForgeCore => Color::from_rgba8(236, 72, 153, 255),
    }
}

fn fill_rect(pm: &mut tiny_skia::PixmapMut, rect: Rect, color: Color) {
    let paint = paint(color);
    pm.fill_rect(rect, &paint, Transform::identity(), None);
}

fn stroke_rect(pm: &mut tiny_skia::PixmapMut, rect: Rect, color: Color, width: f32) {
    let path = PathBuilder::from_rect(rect);
    let paint = paint(color);
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn fill_path(pm: &mut tiny_skia::PixmapMut, path: &tiny_skia::Path, color: Color) {
    let paint = paint(color);
    pm.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn stroke_path(pm: &mut tiny_skia::PixmapMut, path: &tiny_skia::Path, color: Color, width: f32) {
    let paint = paint(color);
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pm.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

fn draw_line(pm: &mut tiny_skia::PixmapMut, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    if let Some(path) = pb.finish() {
        stroke_path(pm, &path, color, width);
    }
}

fn fill_circle(pm: &mut tiny_skia::PixmapMut, cx: f32, cy: f32, r: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        fill_path(pm, &path, color);
    }
}

fn draw_dir_arrow(pm: &mut tiny_skia::PixmapMut) {
    let mut pb = PathBuilder::new();
    pb.move_to(54.0, 38.0);
    pb.line_to(46.0, 34.0);
    pb.line_to(46.0, 42.0);
    pb.close();
    if let Some(path) = pb.finish() {
        fill_path(pm, &path, white_color());
    }
}

fn draw_icon(pm: &mut tiny_skia::PixmapMut, kind: BuildingKind, accent: Color) {
    use BuildingKind::*;
    let c = (32.0, 38.0);
    match kind {
        Miner => {
            // Pickaxe head.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0, c.1 - 8.0);
            pb.line_to(c.0 - 8.0, c.1 + 8.0);
            pb.line_to(c.0 + 8.0, c.1 + 8.0);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, accent);
            }
            draw_line(pm, c.0, c.1 + 8.0, c.0, c.1 + 16.0, white_color(), 3.0);
        }
        Assembler => {
            // Gear: outer ring with teeth.
            fill_circle(pm, c.0, c.1, 12.0, accent);
            fill_circle(pm, c.0, c.1, 5.0, base_color());
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::TAU / 6.0;
                let x = c.0 + a.cos() * 15.0;
                let y = c.1 + a.sin() * 15.0;
                fill_circle(pm, x, y, 2.5, accent);
            }
        }
        Inserter => {
            // Arm reaching to the right.
            draw_line(pm, c.0 - 12.0, c.1, c.0 + 12.0, c.1, accent, 4.0);
            draw_line(pm, c.0 + 8.0, c.1 - 5.0, c.0 + 14.0, c.1, accent, 3.0);
            draw_line(pm, c.0 + 8.0, c.1 + 5.0, c.0 + 14.0, c.1, accent, 3.0);
        }
        Storage => {
            // Crate X.
            draw_line(pm, c.0 - 10.0, c.1 - 10.0, c.0 + 10.0, c.1 + 10.0, white_color(), 3.0);
            draw_line(pm, c.0 + 10.0, c.1 - 10.0, c.0 - 10.0, c.1 + 10.0, white_color(), 3.0);
        }
        Shipment => {
            // Package outline.
            let rect = Rect::from_xywh(c.0 - 10.0, c.1 - 8.0, 20.0, 16.0).unwrap();
            stroke_rect(pm, rect, white_color(), 2.0);
            draw_line(pm, c.0 - 10.0, c.1, c.0 + 10.0, c.1, white_color(), 2.0);
        }
        Generator => {
            // Lightning bolt.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0 - 5.0, c.1 - 12.0);
            pb.line_to(c.0 + 2.0, c.1 - 1.0);
            pb.line_to(c.0 - 3.0, c.1 - 1.0);
            pb.line_to(c.0 + 5.0, c.1 + 12.0);
            pb.line_to(c.0 - 2.0, c.1 + 1.0);
            pb.line_to(c.0 + 3.0, c.1 + 1.0);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, accent);
            }
        }
        Pole => {
            // Crossbar and pole.
            draw_line(pm, c.0 - 14.0, c.1 - 4.0, c.0 + 14.0, c.1 - 4.0, white_color(), 3.0);
            draw_line(pm, c.0, c.1 - 12.0, c.0, c.1 + 14.0, white_color(), 3.0);
        }
        Pump => {
            // Droplet / triangle.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0, c.1 - 12.0);
            pb.line_to(c.0 - 10.0, c.1 + 8.0);
            pb.line_to(c.0 + 10.0, c.1 + 8.0);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, accent);
            }
        }
        Tank => {
            // Cylinder body with a fluid level line.
            let rect = Rect::from_xywh(c.0 - 12.0, c.1 - 10.0, 24.0, 20.0).unwrap();
            stroke_rect(pm, rect, white_color(), 2.0);
            draw_line(pm, c.0 - 8.0, c.1, c.0 + 8.0, c.1, accent, 3.0);
        }
        Lab => {
            // Flask.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0 - 4.0, c.1 - 12.0);
            pb.line_to(c.0 - 4.0, c.1 - 2.0);
            pb.line_to(c.0 - 10.0, c.1 + 10.0);
            pb.line_to(c.0 + 10.0, c.1 + 10.0);
            pb.line_to(c.0 + 4.0, c.1 - 2.0);
            pb.line_to(c.0 + 4.0, c.1 - 12.0);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, accent);
            }
        }
        Turret => {
            // Crosshair + barrel.
            draw_line(pm, c.0 - 14.0, c.1, c.0 + 14.0, c.1, accent, 3.0);
            draw_line(pm, c.0, c.1 - 14.0, c.0, c.1 + 14.0, accent, 3.0);
            fill_circle(pm, c.0, c.1, 5.0, white_color());
        }
        ForgeCore => {
            // Glowing diamond.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0, c.1 - 14.0);
            pb.line_to(c.0 + 14.0, c.1);
            pb.line_to(c.0, c.1 + 14.0);
            pb.line_to(c.0 - 14.0, c.1);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, accent);
            }
            fill_circle(pm, c.0, c.1, 4.0, white_color());
        }
        Source => {
            // Small ore cube.
            let rect = Rect::from_xywh(c.0 - 7.0, c.1 - 7.0, 14.0, 14.0).unwrap();
            fill_rect(pm, rect, accent);
        }
        Sink => {
            // Downward chute.
            let mut pb = PathBuilder::new();
            pb.move_to(c.0 - 8.0, c.1 - 10.0);
            pb.line_to(c.0 + 8.0, c.1 - 10.0);
            pb.line_to(c.0 + 5.0, c.1 + 10.0);
            pb.line_to(c.0 - 5.0, c.1 + 10.0);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pm, &path, white_color());
            }
        }
        Splitter => {
            // Y splitter.
            draw_line(pm, c.0 - 12.0, c.1 - 4.0, c.0, c.1, accent, 3.0);
            draw_line(pm, c.0, c.1, c.0 + 12.0, c.1 - 4.0, accent, 3.0);
            draw_line(pm, c.0, c.1, c.0 + 12.0, c.1 + 4.0, accent, 3.0);
        }
        Pipe => {
            // Simple pipe joint.
            fill_circle(pm, c.0, c.1, 5.0, accent);
            draw_line(pm, c.0 - 10.0, c.1, c.0 + 10.0, c.1, accent, 3.0);
            draw_line(pm, c.0, c.1 - 10.0, c.0, c.1 + 10.0, accent, 3.0);
        }
        RailTrack => {
            // Two parallel rails.
            draw_line(pm, c.0 - 14.0, c.1 - 4.0, c.0 + 14.0, c.1 - 4.0, white_color(), 2.0);
            draw_line(pm, c.0 - 14.0, c.1 + 4.0, c.0 + 14.0, c.1 + 4.0, white_color(), 2.0);
        }
        RailStation => {
            // Station platform.
            let rect = Rect::from_xywh(c.0 - 14.0, c.1 - 6.0, 28.0, 12.0).unwrap();
            fill_rect(pm, rect, accent);
            fill_circle(pm, c.0 - 8.0, c.1, 2.0, white_color());
            fill_circle(pm, c.0 + 8.0, c.1, 2.0, white_color());
        }
    }
}
