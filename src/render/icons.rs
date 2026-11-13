//! The glyphs of the mock-ups (`docs/mockups/v2/strips-v2.html`), drawn on a
//! 14-unit grid and scaled to any size. Strokes scale with the glyph.

use super::{Color, Painter, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Filled dot: a session waits for the person.
    Wait,
    /// Check in a circle: finished, not opened yet.
    Done,
    /// Three-line asterisk, like Claude Code while it works.
    Star,
    /// Memory chip outline.
    Mem,
    /// Memory chip filled: memory is nearly full.
    MemFull,
    /// Close cross.
    Close,
}

/// Draw `icon` centred on (`cx`, `cy`), `size` DIPs across.
pub fn icon(p: &Painter, icon: Icon, cx: f32, cy: f32, size: f32, color: Color) {
    let k = size / 14.0;
    let (ox, oy) = (cx - size / 2.0, cy - size / 2.0);
    let at = |x: f32, y: f32| (ox + x * k, oy + y * k);
    let line = |x1: f32, y1: f32, x2: f32, y2: f32, w: f32| {
        let (a, b) = (at(x1, y1), at(x2, y2));
        p.line(a.0, a.1, b.0, b.1, w * k, color);
    };
    match icon {
        Icon::Wait => {
            let c = at(7.0, 7.0);
            p.fill_circle(c.0, c.1, 4.5 * k, color);
        }
        Icon::Done => {
            let c = at(7.0, 7.0);
            p.stroke_circle(c.0, c.1, 5.75 * k, 1.5 * k, color);
            p.polyline(&[at(4.6, 7.2), at(6.2, 8.8), at(9.5, 5.4)], 1.5 * k, color);
        }
        Icon::Star => {
            line(7.0, 1.6, 7.0, 12.4, 1.7);
            line(2.3, 4.3, 11.7, 9.7, 1.7);
            line(2.3, 9.7, 11.7, 4.3, 1.7);
        }
        Icon::Mem => {
            let o = at(2.25, 3.25);
            p.stroke_round_rect(Rect::new(o.0, o.1, 9.5 * k, 7.5 * k), 1.5 * k, 1.5 * k, color);
            for x in [5.0, 9.0] {
                line(x, 3.25, x, 1.75, 1.5);
                line(x, 12.25, x, 10.75, 1.5);
            }
        }
        Icon::MemFull => {
            let o = at(1.5, 2.5);
            p.fill_round(Rect::new(o.0, o.1, 11.0 * k, 9.0 * k), 2.0 * k, color);
            for x in [5.0, 9.0] {
                line(x, 2.5, x, 1.5, 1.5);
                line(x, 12.5, x, 11.5, 1.5);
            }
        }
        Icon::Close => {
            line(3.5, 3.5, 10.5, 10.5, 1.25);
            line(10.5, 3.5, 3.5, 10.5, 1.25);
        }
    }
}
