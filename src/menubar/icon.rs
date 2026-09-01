//! The status-item mark, rasterized from the monk logo geometry.
//!
//! macOS wants a template image — black plus alpha, tinted by the system for
//! light/dark menu bars and for the highlighted state — so the logo's colors
//! and its dark rounded plate are dropped and only the stroke survives.
//!
//! The cursor after the `m` carries the state: absent while idle, drawn as
//! the logo's dash during a session, and as a filled block in hard mode. The
//! canvas keeps the cursor's width in every state so the menu bar item never
//! changes size when the state does.

use tray_icon::Icon;

/// Stroke geometry, in the 32×32 units of `assets/logo.svg`.
const HALF_STROKE: f32 = 1.25;
const ORIGIN_X: f32 = 6.65;
const ORIGIN_Y: f32 = 10.45;
const VIEW_W: f32 = 20.2;
const VIEW_H: f32 = 11.4;

/// 3× the 24×18pt the status bar renders it at.
const PX_W: u32 = 72;
const PX_H: u32 = 54;
/// Share of the canvas width the mark itself takes; the rest is breathing
/// room against the neighbouring menu bar items.
const FILL: f32 = 0.92;

/// Idle is drawn at partial alpha: a template image is a mask, so less alpha
/// reads as a dimmer mark rather than a different color.
const IDLE_ALPHA: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Idle,
    Active,
    Hard,
}

pub fn render(mark: Mark) -> Icon {
    let scale = PX_W as f32 * FILL / VIEW_W;
    let off_x = (PX_W as f32 - VIEW_W * scale) / 2.0;
    let off_y = (PX_H as f32 - VIEW_H * scale) / 2.0;
    let strokes = letter_m();
    let alpha_scale = if mark == Mark::Idle { IDLE_ALPHA } else { 1.0 };

    let mut rgba = Vec::with_capacity((PX_W * PX_H * 4) as usize);
    for y in 0..PX_H {
        for x in 0..PX_W {
            let px = ((x as f32 + 0.5) - off_x) / scale + ORIGIN_X;
            let py = ((y as f32 + 0.5) - off_y) / scale + ORIGIN_Y;
            let mut d = f32::MAX;
            for pts in &strokes {
                d = d.min(polyline_distance(pts, px, py) - HALF_STROKE);
            }
            match mark {
                Mark::Idle => {}
                Mark::Active => {
                    d = d.min(segment_distance(22.3, 20.6, 25.6, 20.6, px, py) - HALF_STROKE);
                }
                Mark::Hard => d = d.min(block_cursor_distance(px, py)),
            }
            // One pixel of coverage across the edge, in device space.
            let alpha = ((0.5 - d * scale).clamp(0.0, 1.0)) * alpha_scale;
            rgba.extend_from_slice(&[0, 0, 0, (alpha * 255.0).round() as u8]);
        }
    }
    Icon::from_rgba(rgba, PX_W, PX_H).expect("mark dimensions are valid")
}

/// The two shoulders of the `m`, flattened into polylines. Taken verbatim
/// from the logo's path data.
fn letter_m() -> [Vec<[f32; 2]>; 2] {
    let mut left = vec![[7.9, 20.6], [7.9, 14.9]];
    cubic(&mut left, [7.9, 12.7], [9.1, 11.7], [10.75, 11.7]);
    cubic(&mut left, [12.4, 11.7], [13.6, 12.7], [13.6, 14.9]);
    left.push([13.6, 20.6]);

    let mut right = vec![[13.6, 14.9]];
    cubic(&mut right, [13.6, 12.7], [14.8, 11.7], [16.45, 11.7]);
    cubic(&mut right, [18.1, 11.7], [19.3, 12.7], [19.3, 14.9]);
    right.push([19.3, 20.6]);

    [left, right]
}

/// Appends a cubic Bézier that starts at the polyline's current end.
fn cubic(out: &mut Vec<[f32; 2]>, c1: [f32; 2], c2: [f32; 2], to: [f32; 2]) {
    const STEPS: u32 = 16;
    let p0 = *out.last().expect("cubic needs a current point");
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let n = 1.0 - t;
        let (a, b, c, d) = (n * n * n, 3.0 * n * n * t, 3.0 * n * t * t, t * t * t);
        out.push([
            a * p0[0] + b * c1[0] + c * c2[0] + d * to[0],
            a * p0[1] + b * c1[1] + c * c2[1] + d * to[1],
        ]);
    }
}

fn polyline_distance(pts: &[[f32; 2]], px: f32, py: f32) -> f32 {
    let mut d = f32::MAX;
    for w in pts.windows(2) {
        d = d.min(segment_distance(w[0][0], w[0][1], w[1][0], w[1][1], px, py));
    }
    d
}

/// Distance to a segment; unioning these gives round caps and joins for free,
/// which is what the logo's `stroke-linecap="round"` asks for.
fn segment_distance(x0: f32, y0: f32, x1: f32, y1: f32, px: f32, py: f32) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    let t =
        if len2 == 0.0 { 0.0 } else { (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (x0 + t * dx, y0 + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Signed distance to the hard-mode block cursor: the dash's footprint grown
/// upwards into a filled, slightly rounded rectangle.
fn block_cursor_distance(px: f32, py: f32) -> f32 {
    const CENTER: [f32; 2] = [23.95, 19.15];
    const HALF: [f32; 2] = [2.9, 2.7];
    const RADIUS: f32 = 0.7;
    let qx = (px - CENTER[0]).abs() - (HALF[0] - RADIUS);
    let qy = (py - CENTER[1]).abs() - (HALF[1] - RADIUS);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - RADIUS
}
