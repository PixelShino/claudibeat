//! The window above the strip: sessions by group, memory, limits.
//! Sizes follow `.fly` in `docs/mockups/base.css` (mock-up v1 flyout).

use super::icons::{Icon, icon};
use super::{HAIR, HOVER, INK, MUTED, Painter, Rect, SURFACE, TIME, TRACK, TextStyle, Weight};
use crate::limits::{LimitView, LimitWindow};
use crate::memory::MemGroups;
use crate::merge::{Groups, Row, open_target};
use crate::text::{Lang, ago, gb_value, limit_caption, mem_total, more, quote, total};

pub const CARD_W: f32 = 384.0;
/// Room around the card for its shadow.
pub const M_TOP: f32 = 16.0;
pub const M_SIDE: f32 = 20.0;
pub const M_BOTTOM: f32 = 36.0;
const RADIUS: f32 = 8.0;
const COLLAPSED_ROWS: usize = 3;
const MIN_LIST: f32 = 120.0;

const TITLE: TextStyle = TextStyle::new(14.0, Weight::Semibold, INK);
const SMALL: TextStyle = TextStyle::new(12.0, Weight::Regular, MUTED);
const HEAD: TextStyle = TextStyle::new(12.0, Weight::Semibold, MUTED);
const ROW_TITLE: TextStyle = TextStyle::new(13.0, Weight::Medium, INK);
const ROW_TITLE_HOT: TextStyle = TextStyle::new(13.0, Weight::Semibold, INK);
const ROW_TIME: TextStyle = TextStyle::new(12.0, Weight::Regular, TIME);
const MEM_NAME: TextStyle = TextStyle::new(12.5, Weight::Regular, INK);
const BIG: TextStyle = TextStyle::new(13.0, Weight::Semibold, INK);

const GROUP_ICONS: [Icon; 3] = [Icon::Wait, Icon::Done, Icon::Star];

pub struct FlyoutModel<'a> {
    pub groups: Groups<'a>,
    pub total: usize,
    /// Which groups show every row: waiting, done, working.
    pub expanded: [bool; 3],
    pub mem: MemGroups,
    pub limits: Vec<LimitView>,
    pub now: i64,
    pub tz: i32,
    pub lang: Lang,
}

impl FlyoutModel<'_> {
    fn group(&self, g: usize) -> &[&Row] {
        match g {
            0 => &self.groups.waiting,
            1 => &self.groups.done,
            _ => &self.groups.working,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Divider,
    GroupHead(usize),
    Row { group: usize, index: usize },
    More { group: usize, hidden: usize },
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlyoutLayout {
    pub width: f32,
    pub height: f32,
    pub card: Rect,
    /// Session rows at scroll 0, window coordinates: key, rect, opens a chat.
    pub rows: Vec<(String, Rect, bool)>,
    /// «и ещё N» lines at scroll 0: group, rect.
    pub more: Vec<(usize, Rect)>,
    /// The scrolling viewport of the session list.
    pub list: Rect,
    pub scroll_max: f32,
    blocks: Vec<(Block, Rect)>,
    title_y: f32,
    bottom_y: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlyoutHit {
    Row(String),
    More(usize),
}

impl FlyoutLayout {
    pub fn hit(&self, x: f32, y: f32, scroll: f32) -> Option<FlyoutHit> {
        if !self.list.contains(x, y) {
            return None;
        }
        let y = y + scroll;
        if let Some((key, _, true)) = self.rows.iter().find(|(_, r, _)| r.contains(x, y)) {
            return Some(FlyoutHit::Row(key.clone()));
        }
        self.more.iter().find(|(_, r)| r.contains(x, y)).map(|(g, _)| FlyoutHit::More(*g))
    }
}

struct Heights {
    h12: f32,
    h125: f32,
    h13: f32,
    h14: f32,
}

fn heights(p: &Painter) -> Heights {
    Heights {
        h12: p.measure("Ag", &SMALL).1,
        h125: p.measure("Ag", &MEM_NAME).1,
        h13: p.measure("Ag", &ROW_TITLE).1,
        h14: p.measure("Ag", &TITLE).1,
    }
}

fn row_height(h: &Heights) -> f32 {
    7.0 + h.h13 + 1.0 + h.h12 + 7.0
}

fn bottom_height(h: &Heights, has_limits: bool) -> f32 {
    let section_gap = 6.0 + 1.0 + 8.0;
    let head = 4.0 + h.h12 + 6.0;
    let memory = section_gap + head + 4.0 + 10.0 + 4.0 * (3.0 + h.h125 + 3.0) + 2.0;
    let limits = if has_limits { section_gap + head + (h.h13 + h.h12).max(22.0) + 2.0 } else { 0.0 };
    memory + limits + 10.0
}

pub fn layout_flyout(m: &FlyoutModel, p: &Painter, max_h: f32) -> FlyoutLayout {
    let h = heights(p);
    let card_x = M_SIDE;
    let card_y = M_TOP;
    let title_y = card_y + 6.0 + 10.0;
    let list_top = title_y + h.h14 + 6.0;

    // The session list, in list coordinates (0 = top of the viewport).
    let mut blocks = Vec::new();
    let mut ly = 0.0;
    let mut first = true;
    for g in 0..3 {
        let rows = m.group(g);
        if rows.is_empty() {
            continue;
        }
        if !first {
            ly += 6.0;
            blocks.push((Block::Divider, Rect::new(card_x, ly, CARD_W, 1.0)));
            ly += 1.0;
        }
        first = false;
        ly += 8.0 + 4.0;
        blocks.push((Block::GroupHead(g), Rect::new(card_x + 16.0, ly, CARD_W - 32.0, h.h12)));
        ly += h.h12 + 6.0;
        let shown = if m.expanded[g] { rows.len() } else { rows.len().min(COLLAPSED_ROWS) };
        for index in 0..shown {
            let rh = row_height(&h);
            blocks.push((Block::Row { group: g, index }, Rect::new(card_x + 8.0, ly, CARD_W - 16.0, rh)));
            ly += rh;
        }
        if shown < rows.len() {
            ly += 4.0;
            blocks.push((Block::More { group: g, hidden: rows.len() - shown }, Rect::new(card_x + 8.0, ly - 4.0, CARD_W - 16.0, h.h12 + 6.0)));
            ly += h.h12 + 2.0;
        }
        ly += 2.0;
    }
    if first {
        ly += 8.0;
        blocks.push((Block::Empty, Rect::new(card_x + 16.0, ly, CARD_W - 32.0, h.h13)));
        ly += h.h13 + 10.0;
    }
    let list_h = ly;

    let bottom_h = bottom_height(&h, !m.limits.is_empty());
    let max_card_h = (max_h - M_TOP - M_BOTTOM).max(0.0);
    let full_card_h = (list_top - card_y) + list_h + bottom_h;
    let (card_h, viewport_h) = if full_card_h > max_card_h {
        let viewport = (max_card_h - (list_top - card_y) - bottom_h).max(MIN_LIST.min(list_h));
        ((list_top - card_y) + viewport + bottom_h, viewport)
    } else {
        (full_card_h, list_h)
    };
    let list = Rect::new(card_x, list_top, CARD_W, viewport_h);

    // Shift list blocks into window coordinates (scroll 0).
    for (_, r) in &mut blocks {
        r.y += list_top;
    }
    let rows = blocks
        .iter()
        .filter_map(|(b, r)| match b {
            Block::Row { group, index } => {
                let row = m.group(*group)[*index];
                Some((row.key.clone(), *r, open_target(row).is_some()))
            }
            _ => None,
        })
        .collect();
    let more = blocks.iter().filter_map(|(b, r)| if let Block::More { group, .. } = b { Some((*group, *r)) } else { None }).collect();

    FlyoutLayout {
        width: CARD_W + 2.0 * M_SIDE,
        height: card_y + card_h + M_BOTTOM,
        card: Rect::new(card_x, card_y, CARD_W, card_h),
        rows,
        more,
        list,
        scroll_max: (list_h - viewport_h).max(0.0),
        blocks,
        title_y,
        bottom_y: list_top + viewport_h,
    }
}

fn sub_line(row: &Row, group: usize, lang: Lang) -> String {
    let w = lang.words();
    let project = || if row.project.is_empty() { w.no_folder.to_owned() } else { row.project.clone() };
    match group {
        0 => row.ask.as_deref().map_or_else(|| w.waiting_one.to_owned(), |a| quote(lang, a)),
        1 => project(),
        _ => row.doing.as_deref().map_or_else(project, capitalise),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Draw `text` so that it ends at `right`.
fn text_right(p: &Painter, text: &str, right: f32, y: f32, style: &TextStyle) {
    let (w, _) = p.measure(text, style);
    p.text(text, right - w, y, style);
}

pub fn paint_flyout(p: &Painter, l: &FlyoutLayout, m: &FlyoutModel, hover_row: Option<&str>, scroll: f32) {
    let h = heights(p);
    let card = l.card;
    p.card_shadow(card, RADIUS);
    p.fill_round(card, RADIUS, SURFACE);

    // Title and total, on one baseline.
    let w = m.lang.words();
    p.text(w.flyout_title, card.x + 16.0, l.title_y, &TITLE);
    text_right(p, &total(m.lang, m.total), card.right() - 16.0, l.title_y + (h.h14 - h.h12) * 0.75, &SMALL);

    let scroll = scroll.clamp(0.0, l.scroll_max);
    p.clip(l.list, |p| {
        for (block, r) in &l.blocks {
            let r = Rect { y: r.y - scroll, ..*r };
            if r.bottom() < l.list.y || r.y > l.list.bottom() {
                continue;
            }
            match block {
                Block::Divider => p.fill_rect(r, HAIR),
                Block::GroupHead(g) => {
                    p.text(w.groups[*g], r.x, r.y, &HEAD);
                    text_right(p, &m.group(*g).len().to_string(), r.right(), r.y, &HEAD);
                }
                Block::Row { group, index } => {
                    let row = m.group(*group)[*index];
                    let clickable = open_target(row).is_some();
                    if clickable && hover_row == Some(row.key.as_str()) {
                        p.fill_round(r, 4.0, HOVER);
                    }
                    let top = r.y + 7.0;
                    icon(p, GROUP_ICONS[*group], r.x + 8.0 + 9.0, top + h.h13 / 2.0, 14.0, INK);
                    let time = ago(m.lang, m.now - row.since);
                    let (time_w, _) = p.measure(&time, &ROW_TIME);
                    p.text(&time, r.right() - 8.0 - time_w, top + 1.0, &ROW_TIME);
                    let text_x = r.x + 8.0 + 18.0 + 8.0;
                    let max_w = (r.right() - 8.0 - time_w - 8.0 - text_x).max(40.0);
                    let style = if *group == 0 { ROW_TITLE_HOT } else { ROW_TITLE };
                    if let Some(b) = p.text_box(&row.title, &style, max_w, false) {
                        p.draw_box(&b, text_x, top);
                    }
                    if let Some(b) = p.text_box(&sub_line(row, *group, m.lang), &SMALL, max_w, false) {
                        p.draw_box(&b, text_x, top + h.h13 + 1.0);
                    }
                }
                Block::More { hidden, .. } => {
                    p.text(&more(m.lang, *hidden), r.x + 34.0, r.y + 4.0, &SMALL);
                }
                Block::Empty => {
                    p.text(w.empty, r.x, r.y, &TextStyle { color: MUTED, ..ROW_TITLE });
                }
            }
        }
    });

    // Memory.
    let mut y = l.bottom_y + 6.0;
    p.fill_rect(Rect::new(card.x, y, CARD_W, 1.0), HAIR);
    y += 1.0 + 8.0 + 4.0;
    let (left, right) = (card.x + 16.0, card.right() - 16.0);
    p.text(w.memory, left, y, &HEAD);
    text_right(p, &mem_total(m.lang, m.mem.used, m.mem.total), right, y, &HEAD);
    y += h.h12 + 6.0;
    let used_frac = if m.mem.total == 0 { 0.0 } else { m.mem.used as f32 / m.mem.total as f32 };
    p.bar(Rect::new(left, y, right - left, 4.0), used_frac, INK, TRACK);
    y += 4.0 + 10.0;
    let share = |v: u64| if m.mem.used == 0 { 0.0 } else { v as f32 / m.mem.used as f32 };
    let values = [m.mem.claude, m.mem.chrome, m.mem.python, m.mem.other];
    for (&(name, note), value) in w.mem_rows.iter().zip(values) {
        let line_y = y + 3.0;
        let w = p.text(name, left, line_y, &MEM_NAME);
        if !note.is_empty() {
            p.text(note, left + w + 4.0, line_y + (h.h125 - h.h12) * 0.75, &SMALL);
        }
        let value_x = right - 56.0;
        p.bar(Rect::new(value_x - 12.0 - 96.0, line_y + h.h125 / 2.0 - 1.5, 96.0, 3.0), share(value), INK, TRACK);
        text_right(p, &gb_value(m.lang, value), right, line_y, &MEM_NAME);
        y += 3.0 + h.h125 + 3.0;
    }
    y += 2.0;

    // Limits.
    if m.limits.is_empty() {
        return;
    }
    y += 6.0;
    p.fill_rect(Rect::new(card.x, y, CARD_W, 1.0), HAIR);
    y += 1.0 + 8.0 + 4.0;
    p.text(w.limits, left, y, &HEAD);
    y += h.h12 + 6.0;
    let mut x = left;
    let mut ordered = m.limits.clone();
    ordered.sort_by_key(|v| v.window != LimitWindow::FiveHour);
    for v in &ordered {
        let ring = 22.0;
        let block_h = (h.h13 + h.h12).max(ring);
        p.ring(x + ring / 2.0, y + block_h / 2.0, 5.5 * ring / 14.0, 1.75 * ring / 14.0, v.used_fraction(), INK, TRACK);
        let tx = x + ring + 8.0;
        let ty = y + (block_h - h.h13 - h.h12) / 2.0;
        p.text(&format!("{}%", v.used), tx, ty, &BIG);
        let caption_w = p.text(&limit_caption(m.lang, v, m.now, m.tz), tx, ty + h.h13, &SMALL);
        x = tx + caption_w + 24.0;
    }
}
