//! The window above the strip: sessions by group, memory, limits, and a
//! footer with "do not disturb" and the language. The gear in the header
//! turns the same window into the settings page, with a way back.
//! Sizes follow `.fly` in `docs/mockups/base.css` (mock-up v1 flyout).

use super::icons::{Icon, icon};
use super::{HAIR, HOVER, INK, MUTED, PILL, PILL_INK, Painter, Rect, SURFACE, TIME, TRACK, TextStyle, Weight};
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
const SEGMENT: TextStyle = TextStyle::new(12.0, Weight::Semibold, MUTED);

/// Header buttons (gear, back): a 28-DIP square.
const BUTTON: f32 = 28.0;
/// The footer: gap, hairline, padding, a 28-DIP control row, padding.
const FOOTER_H: f32 = 6.0 + 1.0 + FOOTER_PAD + 28.0 + FOOTER_PAD;
const FOOTER_PAD: f32 = 8.0;
const CHECK: f32 = 16.0;

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
    pub view: FlyoutView,
    pub prefs: Prefs,
}

/// Which page the window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlyoutView {
    #[default]
    Sessions,
    Settings,
}

/// The switches the window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    pub dnd: bool,
    pub sound: bool,
    pub autostart: bool,
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
    /// Buttons that never scroll: gear or back, the switches, the languages.
    pub controls: Vec<(FlyoutHit, Rect)>,
    blocks: Vec<(Block, Rect)>,
    title_y: f32,
    bottom_y: f32,
    /// Settings page: the language row, whose two halves are controls.
    lang_row: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlyoutHit {
    Row(String),
    More(usize),
    Gear,
    Back,
    Dnd,
    Sound,
    Autostart,
    Lang(Lang),
}

impl FlyoutLayout {
    pub fn hit(&self, x: f32, y: f32, scroll: f32) -> Option<FlyoutHit> {
        if let Some((hit, _)) = self.controls.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(hit.clone());
        }
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
    memory + limits + FOOTER_H
}

/// The header button on the title line: `left` or right-aligned.
fn header_button(card: Rect, title_y: f32, h14: f32, left: bool) -> Rect {
    // The glyph lines up with the text and counts, 16 DIPs in from the edge.
    let x = if left { card.x + 8.0 } else { card.right() - 10.0 - BUTTON };
    Rect::new(x, title_y + h14 / 2.0 - BUTTON / 2.0, BUTTON, BUTTON)
}

/// Two segments, English and Russian, ending at `right` and centred on `cy`.
fn language_segments(p: &Painter, right: f32, cy: f32) -> [(FlyoutHit, Rect); 2] {
    let h = 24.0;
    let width = |l: Lang| p.measure(l.name(), &SEGMENT).0 + 20.0;
    let ru = Rect::new(right - 2.0 - width(Lang::Ru), cy - h / 2.0, width(Lang::Ru), h);
    let en = Rect::new(ru.x - width(Lang::En), cy - h / 2.0, width(Lang::En), h);
    [(FlyoutHit::Lang(Lang::En), en), (FlyoutHit::Lang(Lang::Ru), ru)]
}

fn settings_row_height(h: &Heights) -> f32 {
    9.0 + h.h13 + 2.0 + h.h12 + 9.0
}

fn layout_settings(p: &Painter) -> FlyoutLayout {
    let h = heights(p);
    let (card_x, card_y) = (M_SIDE, M_TOP);
    let title_y = card_y + 6.0 + 10.0;
    let probe = Rect::new(card_x, card_y, CARD_W, 0.0);
    let mut controls = vec![(FlyoutHit::Back, header_button(probe, title_y, h.h14, true))];
    let mut y = title_y + h.h14 + 8.0 + 1.0 + 4.0;
    let rh = settings_row_height(&h);
    for hit in [FlyoutHit::Dnd, FlyoutHit::Sound, FlyoutHit::Autostart] {
        controls.push((hit, Rect::new(card_x + 8.0, y, CARD_W - 16.0, rh)));
        y += rh;
    }
    let lang_row = Rect::new(card_x + 8.0, y, CARD_W - 16.0, rh);
    controls.extend(language_segments(p, lang_row.right() - 8.0, lang_row.y + rh / 2.0));
    y += rh;
    let card_h = y + 8.0 - card_y;
    FlyoutLayout {
        width: CARD_W + 2.0 * M_SIDE,
        height: card_y + card_h + M_BOTTOM,
        card: Rect::new(card_x, card_y, CARD_W, card_h),
        rows: Vec::new(),
        more: Vec::new(),
        list: Rect::new(card_x, y, CARD_W, 0.0),
        scroll_max: 0.0,
        controls,
        blocks: Vec::new(),
        title_y,
        bottom_y: y,
        lang_row,
    }
}

pub fn layout_flyout(m: &FlyoutModel, p: &Painter, max_h: f32) -> FlyoutLayout {
    if m.view == FlyoutView::Settings {
        return layout_settings(p);
    }
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

    let card = Rect::new(card_x, card_y, CARD_W, card_h);
    let mut controls = vec![(FlyoutHit::Gear, header_button(card, title_y, h.h14, false))];
    // Footer row: "do not disturb" on the left, the language on the right.
    let row_y = card.bottom() - FOOTER_PAD - 28.0;
    let dnd_w = 8.0 + CHECK + 8.0 + p.measure(m.lang.words().dnd, &MEM_NAME).0 + 10.0;
    controls.push((FlyoutHit::Dnd, Rect::new(card.x + 8.0, row_y, dnd_w, 28.0)));
    controls.extend(language_segments(p, card.right() - 16.0, row_y + 14.0));

    FlyoutLayout {
        width: CARD_W + 2.0 * M_SIDE,
        height: card_y + card_h + M_BOTTOM,
        card,
        rows,
        more,
        list,
        scroll_max: (list_h - viewport_h).max(0.0),
        controls,
        blocks,
        title_y,
        bottom_y: list_top + viewport_h,
        lang_row: Rect::default(),
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

fn control(l: &FlyoutLayout, hit: &FlyoutHit) -> Option<Rect> {
    l.controls.iter().find(|(h, _)| h == hit).map(|(_, r)| *r)
}

/// A square check box, `CHECK` DIPs, its top-left at (`x`, `y`).
fn check_box(p: &Painter, x: f32, y: f32, on: bool) {
    let r = Rect::new(x, y, CHECK, CHECK);
    if on {
        p.fill_round(r, 4.0, PILL);
        p.polyline(&[(x + 4.2, y + 8.3), (x + 6.9, y + 10.9), (x + 11.8, y + 5.6)], 1.8, PILL_INK);
    } else {
        p.stroke_round_rect(Rect::new(x + 0.75, y + 0.75, CHECK - 1.5, CHECK - 1.5), 3.5, 1.5, MUTED);
    }
}

/// English and Russian side by side; the current one is a black pill.
fn paint_languages(p: &Painter, l: &FlyoutLayout, m: &FlyoutModel, hover: Option<&FlyoutHit>) {
    let (Some(en), Some(ru)) = (control(l, &FlyoutHit::Lang(Lang::En)), control(l, &FlyoutHit::Lang(Lang::Ru))) else { return };
    p.fill_round(Rect::new(en.x - 2.0, en.y - 2.0, ru.right() - en.x + 4.0, en.h + 4.0), 7.0, HOVER);
    for (lang, r) in [(Lang::En, en), (Lang::Ru, ru)] {
        let on = m.lang == lang;
        if on {
            p.fill_round(r, 5.0, PILL);
        } else if hover == Some(&FlyoutHit::Lang(lang)) {
            p.fill_round(r, 5.0, HOVER);
        }
        let style = TextStyle { color: if on { PILL_INK } else if hover == Some(&FlyoutHit::Lang(lang)) { INK } else { MUTED }, ..SEGMENT };
        let (tw, th) = p.measure(lang.name(), &style);
        p.text(lang.name(), r.x + (r.w - tw) / 2.0, r.y + (r.h - th) / 2.0, &style);
    }
}

fn paint_button(p: &Painter, r: Rect, glyph: Icon, hot: bool) {
    if hot {
        p.fill_round(r, 6.0, HOVER);
    }
    icon(p, glyph, r.x + r.w / 2.0, r.y + r.h / 2.0, 16.0, INK);
}

fn paint_settings(p: &Painter, l: &FlyoutLayout, m: &FlyoutModel, hover: Option<&FlyoutHit>) {
    let h = heights(p);
    let card = l.card;
    let w = m.lang.words();
    if let Some(back) = control(l, &FlyoutHit::Back) {
        paint_button(p, back, Icon::Back, hover == Some(&FlyoutHit::Back));
        p.text(w.settings, back.right() + 4.0, l.title_y, &TITLE);
    }
    p.fill_rect(Rect::new(card.x, l.title_y + h.h14 + 8.0, CARD_W, 1.0), HAIR);
    for (hit, title, note, on) in [
        (FlyoutHit::Dnd, w.dnd, w.dnd_note, m.prefs.dnd),
        (FlyoutHit::Sound, w.menu_sound, w.sound_note, m.prefs.sound),
        (FlyoutHit::Autostart, w.menu_autostart, w.autostart_note, m.prefs.autostart),
    ] {
        let Some(r) = control(l, &hit) else { continue };
        if hover == Some(&hit) {
            p.fill_round(r, 6.0, HOVER);
        }
        let top = r.y + 9.0;
        p.text(title, r.x + 8.0, top, &ROW_TITLE);
        p.text(note, r.x + 8.0, top + h.h13 + 2.0, &SMALL);
        check_box(p, r.right() - 8.0 - CHECK, r.y + (r.h - CHECK) / 2.0, on);
    }
    let row = l.lang_row;
    p.text(w.menu_language, row.x + 8.0, row.y + (row.h - h.h13) / 2.0, &ROW_TITLE);
    paint_languages(p, l, m, hover);
}

pub fn paint_flyout(p: &Painter, l: &FlyoutLayout, m: &FlyoutModel, hover: Option<&FlyoutHit>, scroll: f32) {
    let h = heights(p);
    let card = l.card;
    p.card_shadow(card, RADIUS);
    p.fill_round(card, RADIUS, SURFACE);
    if m.view == FlyoutView::Settings {
        paint_settings(p, l, m, hover);
        return;
    }
    let hover_row = match hover {
        Some(FlyoutHit::Row(key)) => Some(key.as_str()),
        _ => None,
    };

    // Title, then the total and the gear on the same line.
    let w = m.lang.words();
    p.text(w.flyout_title, card.x + 16.0, l.title_y, &TITLE);
    let gear = control(l, &FlyoutHit::Gear).unwrap_or_default();
    paint_button(p, gear, Icon::Gear, hover == Some(&FlyoutHit::Gear));
    text_right(p, &total(m.lang, m.total), gear.x - 4.0, l.title_y + (h.h14 - h.h12) * 0.75, &SMALL);

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

    paint_footer(p, l, m, hover);

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

/// The footer: "do not disturb" as a check box on the left, the language on the right.
fn paint_footer(p: &Painter, l: &FlyoutLayout, m: &FlyoutModel, hover: Option<&FlyoutHit>) {
    let card = l.card;
    let Some(dnd) = control(l, &FlyoutHit::Dnd) else { return };
    p.fill_rect(Rect::new(card.x, dnd.y - FOOTER_PAD - 1.0, CARD_W, 1.0), HAIR);
    if hover == Some(&FlyoutHit::Dnd) {
        p.fill_round(dnd, 6.0, HOVER);
    }
    check_box(p, dnd.x + 8.0, dnd.y + (dnd.h - CHECK) / 2.0, m.prefs.dnd);
    let (_, th) = p.measure("Ag", &MEM_NAME);
    p.text(m.lang.words().dnd, dnd.x + 8.0 + CHECK + 8.0, dnd.y + (dnd.h - th) / 2.0, &MEM_NAME);
    paint_languages(p, l, m, hover);
}
