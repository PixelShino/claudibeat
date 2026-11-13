//! `claudibeat --render <dir> [--bg <png>]`: every surface drawn to PNG with
//! sample data, to compare against the mock-ups without touching the desktop.

use crate::render::strip::{STRIP_H, StripItem, StripModel, layout_strip, paint_strip};
use crate::render::{Rect, Renderer, TRANSPARENT};
use std::path::Path;

const SCALE: f32 = 2.0;
const CANVAS_W: f32 = 960.0;

struct StripSample {
    name: &'static str,
    model: StripModel,
    hover: Option<StripItem>,
}

fn strip_samples() -> Vec<StripSample> {
    let base = StripModel { waiting: 1, done: 2, working: 4, mem_percent: 76, mem_alert: false, limit: Some((66, 0.66)) };
    vec![
        StripSample { name: "strip-wait", model: base, hover: None },
        StripSample { name: "strip-calm", model: StripModel { waiting: 0, done: 0, working: 2, mem_percent: 59, ..base }, hover: None },
        StripSample { name: "strip-12", model: StripModel { waiting: 12, done: 3, mem_percent: 91, mem_alert: true, ..base }, hover: None },
        StripSample { name: "strip-hover", model: base, hover: Some(StripItem::Working) },
        StripSample { name: "strip-nolimit", model: StripModel { limit: None, ..base }, hover: None },
    ]
}

pub fn render_samples(dir: &Path, bg: Option<&Path>) -> windows::core::Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let r = Renderer::new()?;
    for s in strip_samples() {
        let frame = r.render(CANVAS_W, STRIP_H, SCALE, TRANSPARENT, |p| {
            if let Some(bg) = bg {
                p.image(bg, Rect::new(0.0, 0.0, CANVAS_W, STRIP_H));
            }
            let l = layout_strip(&s.model, p);
            paint_strip(p, &l, &s.model, s.hover);
        })?;
        r.save_png(&frame, &dir.join(format!("{}.png", s.name)))?;
    }
    Ok(())
}
