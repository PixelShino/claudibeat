//! The strip on the taskbar: glyphs and numbers only (variant C, mock-up v2).
//! Sizes follow `.c2` in `docs/mockups/base.css`, in DIPs.

use super::icons::{Icon, icon};
use super::{HAIR, HOVER, INK, PILL, PILL_INK, Painter, Rect, TRACK, TextStyle, Weight, ZERO_OPACITY};

/// Height of the strip's window: the taskbar's own height at 100 %.
pub const STRIP_H: f32 = 48.0;
const ITEM_H: f32 = 32.0;
const PAD: f32 = 8.0;
const GAP: f32 = 6.0;
const RADIUS: f32 = 6.0;
const LEFT: f32 = 8.0;
const SEP_MARGIN: f32 = 6.0;
const SEP_H: f32 = 22.0;
const NUMBER: TextStyle = TextStyle::new(13.0, Weight::Semibold, INK);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StripItem {
    Waiting,
    Done,
    Working,
    Memory,
    Limit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripModel {
    pub waiting: usize,
    pub done: usize,
    pub working: usize,
    pub mem_percent: u8,
    pub mem_alert: bool,
    /// Percent left and the ring's fraction; `None` while unknown.
    pub limit: Option<(u8, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripLayout {
    pub width: f32,
    pub items: Vec<(StripItem, Rect)>,
    pub separator: Rect,
}

impl StripLayout {
    pub fn hit(&self, x: f32, y: f32) -> Option<StripItem> {
        self.items.iter().find(|(_, r)| r.contains(x, y)).map(|(k, _)| *k)
    }
}

fn glyph_size(item: StripItem) -> f32 {
    match item {
        StripItem::Waiting => 12.0,
        StripItem::Done | StripItem::Working => 13.0,
        StripItem::Memory | StripItem::Limit => 14.0,
    }
}

fn label(item: StripItem, m: &StripModel) -> String {
    match item {
        StripItem::Waiting => m.waiting.to_string(),
        StripItem::Done => m.done.to_string(),
        StripItem::Working => m.working.to_string(),
        StripItem::Memory => format!("{}%", m.mem_percent),
        StripItem::Limit => m.limit.map_or("–".to_owned(), |(left, _)| format!("{left}%")),
    }
}

pub fn layout_strip(m: &StripModel, p: &Painter) -> StripLayout {
    let y = (STRIP_H - ITEM_H) / 2.0;
    let mut x = LEFT;
    let mut items = Vec::with_capacity(5);
    let mut separator = Rect::default();
    for item in [StripItem::Waiting, StripItem::Done, StripItem::Working, StripItem::Memory, StripItem::Limit] {
        if item == StripItem::Memory {
            x += SEP_MARGIN;
            separator = Rect::new(x, (STRIP_H - SEP_H) / 2.0, 1.0, SEP_H);
            x += 1.0 + SEP_MARGIN;
        }
        let (text_w, _) = p.measure(&label(item, m), &NUMBER);
        let w = (PAD + glyph_size(item) + GAP + text_w + PAD).round();
        items.push((item, Rect::new(x, y, w, ITEM_H)));
        x += w;
    }
    StripLayout { width: x + LEFT, items, separator }
}

pub fn paint_strip(p: &Painter, l: &StripLayout, m: &StripModel, hover: Option<StripItem>) {
    p.fill_rect(l.separator, HAIR);
    for &(item, rect) in &l.items {
        let hot = item == StripItem::Waiting && m.waiting > 0;
        let quiet = match item {
            StripItem::Waiting => m.waiting == 0,
            StripItem::Done => m.done == 0,
            StripItem::Working => m.working == 0,
            StripItem::Memory => false,
            StripItem::Limit => m.limit.is_none(),
        };
        if hot {
            p.fill_round(rect, RADIUS, PILL);
        } else if hover == Some(item) {
            p.fill_round(rect, RADIUS, HOVER);
        }
        let ink = if hot { PILL_INK } else { INK };
        p.with_opacity(if quiet { ZERO_OPACITY } else { 1.0 }, |p| {
            let size = glyph_size(item);
            let (cx, cy) = (rect.x + PAD + size / 2.0, rect.y + rect.h / 2.0);
            match item {
                StripItem::Waiting => icon(p, Icon::Wait, cx, cy, size, ink),
                StripItem::Done => icon(p, Icon::Done, cx, cy, size, ink),
                StripItem::Working => icon(p, Icon::Star, cx, cy, size, ink),
                StripItem::Memory => icon(p, if m.mem_alert { Icon::MemFull } else { Icon::Mem }, cx, cy, size, ink),
                StripItem::Limit => p.ring(cx, cy, 5.5 * size / 14.0, 2.0 * size / 14.0, m.limit.map_or(0.0, |(_, f)| f), ink, TRACK),
            }
            if let Some(b) = p.text_box(&label(item, m), &TextStyle { color: ink, ..NUMBER }, 1_000.0, false) {
                p.draw_box(&b, rect.x + PAD + size + GAP, (cy - b.height / 2.0).round());
            }
        });
    }
}
