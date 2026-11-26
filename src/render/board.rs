//! The Plane board above the strip: three task columns, the agents beside
//! them, and a card that follows the pointer while it is dragged onto an
//! agent, into a new chat or to another column.
//!
//! The look is the sessions window's, widened: the same card, greys and
//! type; columns are wells a shade under the card, task cards a shade over.

use super::icons::{Icon, icon};
use super::{CARD_BG, EDGE, HAIR, HOVER, INK, LANE, MUTED, PILL, PILL_INK, Painter, Rect, SURFACE, TIME, TextStyle, Weight};
use crate::board::{Assignment, BoardFile, Column, Pending, Target, holder};
use crate::merge::{Row, open_target};
use crate::model::SessionState;
use crate::text::{Lang, ago};

pub const M_TOP: f32 = 16.0;
pub const M_SIDE: f32 = 20.0;
pub const M_BOTTOM: f32 = 36.0;
/// The board's widest; a narrower screen gets a narrower board.
pub const MAX_W: f32 = 1120.0;
const MIN_W: f32 = 760.0;
const RADIUS: f32 = 10.0;
const PAD: f32 = 16.0;
const GAP: f32 = 12.0;
const AGENTS_W: f32 = 272.0;
const LANE_PAD: f32 = 8.0;
const CARD_GAP: f32 = 8.0;
const CARD_PAD: f32 = 10.0;
const BUTTON: f32 = 28.0;
const CHIP_H: f32 = 26.0;
const FOLDER_H: f32 = 28.0;
const OWNER_H: f32 = 22.0;
/// The card in hand shrinks to a chip, so the place under it stays in sight.
const GHOST_W: f32 = 248.0;

const TITLE: TextStyle = TextStyle::new(15.0, Weight::Semibold, INK);
const SMALL: TextStyle = TextStyle::new(12.0, Weight::Regular, MUTED);
const HEAD: TextStyle = TextStyle::new(12.0, Weight::Semibold, MUTED);
const KEY: TextStyle = TextStyle::new(11.5, Weight::Semibold, TIME);
const CARD_TITLE: TextStyle = TextStyle::new(13.0, Weight::Medium, INK);
const ROW_TITLE: TextStyle = TextStyle::new(13.0, Weight::Medium, INK);
const CHIP: TextStyle = TextStyle::new(12.0, Weight::Semibold, MUTED);
const STATUS: TextStyle = TextStyle::new(12.5, Weight::Regular, MUTED);
const STATUS_HOT: TextStyle = TextStyle::new(12.5, Weight::Semibold, INK);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub tone: Tone,
}

pub struct BoardModel<'a> {
    pub board: Option<&'a BoardFile>,
    /// Sessions a task can go to, in the order the column lists them.
    pub agents: Vec<&'a Row>,
    pub assignments: &'a [Assignment],
    pub pending: &'a [Pending],
    /// The project the columns show alone; none: all.
    pub filter: Option<String>,
    /// Folders a new chat may start in: cwd, project name.
    pub folders: Vec<(String, String)>,
    pub now: i64,
    pub lang: Lang,
    pub status: Option<Status>,
}

/// A card in hand: where it came from and where its top-left corner is now,
/// in the board's DIPs.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub key: String,
    pub from: Column,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// What lies under the pointer: a place to drop, or none.
    pub over: Option<Drop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drop {
    Target(Target),
    /// A chat that cannot take a task (no app link).
    Blocked(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardHit {
    Card(Column, String),
    Session(String),
    NewChat(usize),
    Filter(Option<String>),
    Refresh,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardSlot {
    pub column: Column,
    pub key: String,
    /// Window DIPs at scroll 0.
    pub rect: Rect,
    title_h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSlot {
    pub key: String,
    pub rect: Rect,
    /// It has a `local_…` key: a task can be sent to it.
    pub takes: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardLayout {
    pub width: f32,
    pub height: f32,
    pub card: Rect,
    /// The three task wells, then the agents' well.
    pub lanes: [Rect; 4],
    /// The scrolling part of each well.
    pub views: [Rect; 4],
    pub scroll_max: [f32; 4],
    pub cards: Vec<CardSlot>,
    pub sessions: Vec<SessionSlot>,
    /// New-chat folders: index into the model's folders, rect.
    pub folders: Vec<(usize, Rect)>,
    pub filters: Vec<(Option<String>, Rect)>,
    pub refresh: Rect,
    pub close: Rect,
    title_y: f32,
    new_head_y: f32,
    footer_y: f32,
}

impl BoardLayout {
    pub fn hit(&self, x: f32, y: f32, scroll: &[f32; 4]) -> Option<BoardHit> {
        if self.close.contains(x, y) {
            return Some(BoardHit::Close);
        }
        if self.refresh.contains(x, y) {
            return Some(BoardHit::Refresh);
        }
        if let Some((f, _)) = self.filters.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(BoardHit::Filter(f.clone()));
        }
        if let Some((i, _)) = self.folders.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(BoardHit::NewChat(*i));
        }
        for (i, view) in self.views.iter().enumerate() {
            if !view.contains(x, y) {
                continue;
            }
            let y = y + scroll[i];
            if i == 3 {
                return self.sessions.iter().find(|s| s.rect.contains(x, y)).map(|s| BoardHit::Session(s.key.clone()));
            }
            return self.cards.iter().find(|c| c.column.index() == i && c.rect.contains(x, y)).map(|c| BoardHit::Card(c.column, c.key.clone()));
        }
        None
    }

    /// Where a card let go at (`x`, `y`) would land.
    pub fn drop_at(&self, x: f32, y: f32, scroll: &[f32; 4], folders: &[(String, String)]) -> Option<Drop> {
        if let Some((i, _)) = self.folders.iter().find(|(_, r)| r.contains(x, y)) {
            let (cwd, project) = folders.get(*i)?.clone();
            return Some(Drop::Target(Target::NewChat { cwd, project }));
        }
        if self.views[3].contains(x, y) {
            let y = y + scroll[3];
            if let Some(s) = self.sessions.iter().find(|s| s.rect.contains(x, y)) {
                return Some(if s.takes { Drop::Target(Target::Session(s.key.clone())) } else { Drop::Blocked(s.key.clone()) });
            }
            return None;
        }
        Column::ALL.into_iter().find(|c| self.lanes[c.index()].contains(x, y)).map(|c| Drop::Target(Target::Column(c)))
    }

    /// A slot's rect as drawn now, after its well's scroll.
    pub fn card_rect(&self, key: &str, scroll: &[f32; 4]) -> Option<(Column, Rect)> {
        let c = self.cards.iter().find(|c| c.key == key)?;
        Some((c.column, Rect { y: c.rect.y - scroll[c.column.index()], ..c.rect }))
    }
}

struct Heights {
    h12: f32,
    h13: f32,
    h15: f32,
    key: f32,
}

fn heights(p: &Painter) -> Heights {
    Heights { h12: p.measure("Ag", &SMALL).1, h13: p.measure("Ag", &CARD_TITLE).1, h15: p.measure("Ag", &TITLE).1, key: p.measure("Ag", &KEY).1 }
}

fn agent_row_h(h: &Heights) -> f32 {
    8.0 + h.h13 + 2.0 + h.h12 + 8.0
}

/// A task card's title takes at most two lines.
fn title_height(p: &Painter, title: &str, w: f32, h: &Heights) -> f32 {
    p.text_lines(title, &CARD_TITLE, w, 2).map_or(h.h13, |b| b.height)
}

/// A card: key line, title, and a line for who holds it when someone does.
fn card_height(title_h: f32, owner: bool, h: &Heights) -> f32 {
    CARD_PAD + h.key + 5.0 + title_h + if owner { 8.0 + OWNER_H } else { 0.0 } + CARD_PAD
}

/// Who holds a card, or the request on its way, under its title.
fn owner_of(m: &BoardModel, column: Column, key: &str) -> Option<(String, Icon)> {
    if let Some(pd) = m.pending.iter().find(|pd| pd.task.as_deref() == Some(key)) {
        return Some((pd.label.clone(), Icon::Refresh));
    }
    let who = holder(m.assignments, key).map(|a| a.session_title.clone());
    // Done work waiting for a look: its Plane state, and who did it.
    if let Some(card) = m.board.and_then(|b| b.card(key)).filter(|c| c.in_review()) {
        let text = who.map_or_else(|| card.state_name.clone(), |w| format!("{} · {w}", card.state_name));
        return Some((text, Icon::Done));
    }
    if column == Column::Doing {
        return who.map(|w| (w, Icon::Star));
    }
    None
}

fn project_name(cwd: &str, project: &str) -> String {
    if project.is_empty() { cwd.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(cwd).to_owned() } else { project.to_owned() }
}

pub fn layout_board(m: &BoardModel, p: &Painter, max_w: f32, max_h: f32) -> BoardLayout {
    let h = heights(p);
    let w = (max_w - 2.0 * M_SIDE).clamp(MIN_W, MAX_W);
    let card_h = (max_h - M_TOP - M_BOTTOM).clamp(420.0, 720.0);
    let card = Rect::new(M_SIDE, M_TOP, w, card_h);
    let title_y = card.y + PAD;
    let close = Rect::new(card.right() - 10.0 - BUTTON, title_y + h.h15 / 2.0 - BUTTON / 2.0, BUTTON, BUTTON);
    let refresh = Rect::new(close.x - 2.0 - BUTTON, close.y, BUTTON, BUTTON);

    // Filters: «All», then the projects, each with its open count.
    let filter_y = title_y + h.h15 + 12.0;
    let mut filters = Vec::new();
    let mut x = card.x + PAD;
    let mut names: Vec<Option<String>> = vec![None];
    if let Some(b) = m.board {
        names.extend(b.projects.iter().cloned().map(Some));
    }
    for f in names {
        let label = chip_label(m, f.as_deref());
        let cw = p.measure(&label, &CHIP).0 + 22.0;
        if x + cw > refresh.x - 8.0 {
            break;
        }
        filters.push((f, Rect::new(x, filter_y, cw, CHIP_H)));
        x += cw + 6.0;
    }

    let lanes_y = filter_y + CHIP_H + 14.0;
    let footer_h = 14.0 + h.h12 + 14.0;
    let lanes_h = (card.bottom() - footer_h - lanes_y).max(160.0);
    let task_w = ((w - 2.0 * PAD - AGENTS_W - 3.0 * GAP) / 3.0).floor();
    let mut lanes = [Rect::default(); 4];
    for (i, lane) in lanes.iter_mut().enumerate().take(3) {
        *lane = Rect::new(card.x + PAD + i as f32 * (task_w + GAP), lanes_y, task_w, lanes_h);
    }
    lanes[3] = Rect::new(lanes[2].right() + GAP, lanes_y, card.right() - PAD - (lanes[2].right() + GAP), lanes_h);
    let head_h = 12.0 + h.h12 + 10.0;

    // Task cards, in window DIPs at scroll 0.
    let mut views = [Rect::default(); 4];
    let mut scroll_max = [0.0; 4];
    let mut cards = Vec::new();
    for c in Column::ALL {
        let lane = lanes[c.index()];
        let view = Rect::new(lane.x, lane.y + head_h, lane.w, lane.h - head_h - 2.0);
        views[c.index()] = view;
        let mut y = view.y;
        let inner = lane.w - 2.0 * LANE_PAD - 2.0 * CARD_PAD;
        if let Some(b) = m.board {
            for k in b.filtered(c, m.filter.as_deref()) {
                let th = title_height(p, &k.title, inner, &h);
                let ch = card_height(th, owner_of(m, c, &k.key).is_some(), &h);
                cards.push(CardSlot { column: c, key: k.key.clone(), rect: Rect::new(lane.x + LANE_PAD, y, lane.w - 2.0 * LANE_PAD, ch), title_h: th });
                y += ch + CARD_GAP;
            }
        }
        scroll_max[c.index()] = (y - CARD_GAP + LANE_PAD - view.bottom()).max(0.0);
    }

    // The agents' well: chats above, the new-chat folders pinned below.
    let agents = lanes[3];
    let mut folders = Vec::new();
    let chip_w = |i: usize| -> f32 {
        let (cwd, project) = &m.folders[i];
        let name = if cwd.is_empty() { m.lang.words().no_folder.to_owned() } else { project_name(cwd, project) };
        (p.measure(&name, &CHIP).0 + 34.0).min(agents.w - 2.0 * LANE_PAD)
    };
    // Lay chips out bottom-up so the section hugs the well's floor.
    let mut lines: Vec<Vec<(usize, f32)>> = vec![Vec::new()];
    let mut line_w = 0.0;
    for i in 0..m.folders.len() {
        let cw = chip_w(i);
        if line_w + cw > agents.w - 2.0 * LANE_PAD && !lines.last().is_none_or(Vec::is_empty) {
            if lines.len() == 3 {
                break;
            }
            lines.push(Vec::new());
            line_w = 0.0;
        }
        if let Some(l) = lines.last_mut() {
            l.push((i, cw));
        }
        line_w += cw + 6.0;
    }
    let rows_n = lines.iter().filter(|l| !l.is_empty()).count() as f32;
    let section_h = 10.0 + h.h12 + 8.0 + rows_n * (FOLDER_H + 6.0) + 4.0;
    let new_head_y = agents.bottom() - section_h + 10.0;
    let mut fy = new_head_y + h.h12 + 8.0;
    for line in lines.iter().filter(|l| !l.is_empty()) {
        let mut fx = agents.x + LANE_PAD;
        for &(i, cw) in line {
            folders.push((i, Rect::new(fx, fy, cw, FOLDER_H)));
            fx += cw + 6.0;
        }
        fy += FOLDER_H + 6.0;
    }

    let view = Rect::new(agents.x, agents.y + head_h, agents.w, (new_head_y - 10.0 - 1.0 - (agents.y + head_h)).max(40.0));
    views[3] = view;
    let rh = agent_row_h(&h);
    let mut sessions = Vec::new();
    let mut y = view.y;
    for r in &m.agents {
        let takes = r.key.starts_with("local_") && open_target(r).is_some();
        sessions.push(SessionSlot { key: r.key.clone(), rect: Rect::new(agents.x + 4.0, y, agents.w - 8.0, rh), takes });
        y += rh;
    }
    scroll_max[3] = (y + 4.0 - view.bottom()).max(0.0);

    BoardLayout {
        width: w + 2.0 * M_SIDE,
        height: card.bottom() + M_BOTTOM,
        card,
        lanes,
        views,
        scroll_max,
        cards,
        sessions,
        folders,
        filters,
        refresh,
        close,
        title_y,
        new_head_y,
        footer_y: card.bottom() - 14.0 - h.h12,
    }
}

fn chip_label(m: &BoardModel, project: Option<&str>) -> String {
    let words = m.lang.board();
    let Some(b) = m.board else { return words.all.to_owned() };
    let open = |p: Option<&str>| b.todo.iter().chain(&b.doing).filter(|k| p.is_none_or(|p| k.project == p)).count();
    match project {
        None => format!("{} {}", words.all, open(None)),
        Some(p) => format!("{p} {}", open(Some(p))),
    }
}

fn text_right(p: &Painter, text: &str, right: f32, y: f32, style: &TextStyle) -> f32 {
    let (w, _) = p.measure(text, style);
    p.text(text, right - w, y, style);
    w
}

fn paint_button(p: &Painter, r: Rect, glyph: Icon, hot: bool) {
    if hot {
        p.fill_round(r, 6.0, HOVER);
    }
    icon(p, glyph, r.x + r.w / 2.0, r.y + r.h / 2.0, 15.0, INK);
}

fn state_icon(state: SessionState, needs_you: bool) -> Icon {
    match state {
        SessionState::Waiting if needs_you => Icon::Wait,
        SessionState::Done if needs_you => Icon::Done,
        SessionState::Working => Icon::Star,
        _ => Icon::Idle,
    }
}

/// Urgent: a dark dot; high: a grey one; medium: a ring; the rest, nothing.
fn priority_mark(p: &Painter, priority: &str, cx: f32, cy: f32) -> bool {
    match priority {
        "urgent" => p.fill_circle(cx, cy, 3.5, INK),
        "high" => p.fill_circle(cx, cy, 3.5, TIME),
        "medium" => p.stroke_circle(cx, cy, 3.0, 1.3, TIME),
        _ => return false,
    }
    true
}

fn paint_card(p: &Painter, m: &BoardModel, slot: &CardSlot, r: Rect, hot: bool) {
    let Some(card) = m.board.and_then(|b| b.card(&slot.key)) else { return };
    let h = heights(p);
    p.fill_round(r, 8.0, CARD_BG);
    p.stroke_round_rect(Rect::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), 7.5, 1.0, if hot { HAIR } else { EDGE });
    if hot {
        p.fill_round(r, 8.0, HOVER);
    }
    let (x, top) = (r.x + CARD_PAD, r.y + CARD_PAD);
    let mut kx = x;
    if priority_mark(p, &card.priority, x + 3.5, top + h.key / 2.0) {
        kx += 12.0;
    }
    let key_w = p.text(&card.key, kx, top, &KEY);
    let _ = holder;

    let _ = key_w;
    let ty = top + h.key + 5.0;
    let tw = r.w - 2.0 * CARD_PAD;
    if let Some(b) = p.text_lines(&card.title, &CARD_TITLE, tw, 2) {
        p.draw_box(&b, x, ty);
    }

    // Who holds it: a quiet pill under the title; a request on its way says so.
    if let Some((text, glyph)) = owner_of(m, slot.column, &card.key) {
        let busy = glyph == Icon::Refresh;
        let oy = ty + slot.title_h + 8.0;
        let style = if busy { TextStyle { color: INK, ..SMALL } } else { SMALL };
        if let Some(b) = p.text_box(&text, &style, tw - 30.0, false) {
            let pill = Rect::new(x, oy, (b.width + 30.0).min(tw), OWNER_H);
            p.fill_round(pill, OWNER_H / 2.0, HOVER);
            icon(p, glyph, pill.x + 12.0, pill.y + OWNER_H / 2.0, 10.0, if busy { INK } else { MUTED });
            p.draw_box(&b, pill.x + 22.0, pill.y + (OWNER_H - b.height) / 2.0);
        }
    }
}

/// The card in hand: key and title on one line, a lifted chip by the pointer.
pub fn ghost_size(p: &Painter) -> (f32, f32) {
    let h = heights(p);
    (GHOST_W, CARD_PAD + h.key + 3.0 + h.h13 + CARD_PAD)
}

/// Where the chip in hand goes for a pointer at (`px`, `py`): below and to
/// the right, flipped left or up where the board's card would cut it.
pub fn ghost_at(card: Rect, px: f32, py: f32, w: f32, h: f32) -> (f32, f32) {
    const OFF: f32 = 14.0;
    let x = if px + OFF + w > card.right() - 4.0 { px - OFF - w } else { px + OFF };
    let y = if py + OFF + h > card.bottom() - 4.0 { py - OFF - h } else { py + OFF };
    (x, y)
}

fn paint_ghost(p: &Painter, m: &BoardModel, key: &str, r: Rect) {
    let Some(card) = m.board.and_then(|b| b.card(key)) else { return };
    let h = heights(p);
    p.card_shadow(r, 8.0);
    p.fill_round(r, 8.0, CARD_BG);
    p.stroke_round_rect(Rect::new(r.x + 0.75, r.y + 0.75, r.w - 1.5, r.h - 1.5), 7.25, 1.5, INK);
    let (x, top) = (r.x + CARD_PAD, r.y + CARD_PAD);
    let mut kx = x;
    if priority_mark(p, &card.priority, x + 3.5, top + h.key / 2.0) {
        kx += 12.0;
    }
    p.text(&card.key, kx, top, &KEY);
    if let Some(b) = p.text_box(&card.title, &CARD_TITLE, r.w - 2.0 * CARD_PAD, false) {
        p.draw_box(&b, x, top + h.key + 3.0);
    }
}

fn paint_lane_head(p: &Painter, lane: Rect, title: &str, count: usize) {
    p.text(title, lane.x + 12.0, lane.y + 12.0, &HEAD);
    text_right(p, &count.to_string(), lane.right() - 12.0, lane.y + 12.0, &HEAD);
}

fn sub_line(m: &BoardModel, r: &Row) -> String {
    if let Some(a) = m.assignments.iter().rev().find(|a| a.session == r.key) {
        let title = m.board.and_then(|b| b.card(&a.task)).map_or(String::new(), |c| format!(" · {}", c.title));
        return format!("{}{title}", a.task);
    }
    let project = if r.project.is_empty() { m.lang.words().no_folder.to_owned() } else { r.project.clone() };
    match r.state {
        SessionState::Waiting if r.needs_you => r.ask.clone().unwrap_or_else(|| m.lang.words().waiting_one.to_owned()),
        SessionState::Working => r.doing.clone().map_or(project, |d| capitalise(&d)),
        SessionState::Idle => format!("{project} · {}", m.lang.board().idle),
        _ => project,
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn is_target(drag: Option<&Drag>, t: &Target) -> bool {
    drag.and_then(|d| d.over.as_ref()) == Some(&Drop::Target(t.clone()))
}

fn ring(p: &Painter, r: Rect, radius: f32) {
    p.fill_round(r, radius, HOVER);
    p.stroke_round_rect(Rect::new(r.x + 0.75, r.y + 0.75, r.w - 1.5, r.h - 1.5), radius - 0.5, 1.5, INK);
}

pub fn paint_board(p: &Painter, l: &BoardLayout, m: &BoardModel, hover: Option<&BoardHit>, scroll: &[f32; 4], drag: Option<&Drag>) {
    let h = heights(p);
    let card = l.card;
    let words = m.lang.board();
    p.card_shadow(card, RADIUS);
    p.fill_round(card, RADIUS, SURFACE);

    // Header: title, freshness or Plane's error, refresh and close.
    let tw = p.text(words.title, card.x + PAD, l.title_y, &TITLE);
    let fresh = match m.board {
        Some(b) if b.error.is_some() => Some((b.error.clone().unwrap_or_default(), true)),
        Some(b) => Some((crate::text::updated_ago(m.lang, m.now - b.updated_at), false)),
        None => None,
    };
    if let Some((text, bad)) = fresh {
        let style = if bad { STATUS_HOT } else { SMALL };
        let room = l.refresh.x - 12.0 - (card.x + PAD + tw + 12.0);
        if let Some(b) = p.text_box(&text, &style, room.max(40.0), false) {
            p.draw_box(&b, card.x + PAD + tw + 12.0, l.title_y + (h.h15 - b.height) * 0.7);
        }
    }
    paint_button(p, l.refresh, Icon::Refresh, hover == Some(&BoardHit::Refresh));
    paint_button(p, l.close, Icon::Close, hover == Some(&BoardHit::Close));

    for (f, r) in &l.filters {
        let on = &m.filter == f;
        let hot = hover == Some(&BoardHit::Filter(f.clone()));
        if on {
            p.fill_round(*r, r.h / 2.0, PILL);
        } else {
            p.fill_round(*r, r.h / 2.0, if hot { HAIR } else { HOVER });
        }
        let label = chip_label(m, f.as_deref());
        let style = TextStyle { color: if on { PILL_INK } else if hot { INK } else { MUTED }, ..CHIP };
        let (cw, ch) = p.measure(&label, &style);
        p.text(&label, r.x + (r.w - cw) / 2.0, r.y + (r.h - ch) / 2.0, &style);
    }

    // The task wells.
    let hover_card = match hover {
        Some(BoardHit::Card(_, k)) => Some(k.as_str()),
        _ => None,
    };
    for c in Column::ALL {
        let lane = l.lanes[c.index()];
        p.fill_round(lane, RADIUS, LANE);
        let count = m.board.map_or(0, |b| b.filtered(c, m.filter.as_deref()).len());
        paint_lane_head(p, lane, words.columns[c.index()], count);
        let dragging_from_here = drag.is_some_and(|d| d.from == c);
        if is_target(drag, &Target::Column(c)) && !dragging_from_here {
            ring(p, lane, RADIUS);
        }
        let view = l.views[c.index()];
        let s = scroll[c.index()];
        p.clip(view, |p| {
            let mut any = false;
            for slot in l.cards.iter().filter(|k| k.column == c) {
                any = true;
                let r = Rect { y: slot.rect.y - s, ..slot.rect };
                if r.bottom() < view.y || r.y > view.bottom() {
                    continue;
                }
                let held = drag.is_some_and(|d| d.key == slot.key);
                let pending = m.pending.iter().any(|pd| pd.task.as_deref() == Some(slot.key.as_str()));
                let opacity = if held { 0.28 } else if pending { 0.6 } else { 1.0 };
                p.with_opacity(opacity, |p| paint_card(p, m, slot, r, !held && drag.is_none() && hover_card == Some(slot.key.as_str())));
            }
            if !any {
                let text = if m.board.is_none() && c == Column::Todo { words.no_board } else { words.empty_column };
                if let Some(b) = p.text_box(text, &TextStyle { color: TIME, ..SMALL }, view.w - 24.0, true) {
                    p.draw_box(&b, view.x + 12.0, view.y + 6.0);
                }
            }
        });
        paint_fade(p, view, s, l.scroll_max[c.index()]);
    }

    // The agents' well.
    let agents = l.lanes[3];
    p.fill_round(agents, RADIUS, LANE);
    paint_lane_head(p, agents, words.agents, m.agents.len());
    let view = l.views[3];
    let s = scroll[3];
    let hover_row = match hover {
        Some(BoardHit::Session(k)) => Some(k.as_str()),
        _ => None,
    };
    p.clip(view, |p| {
        if m.agents.is_empty() {
            p.text(words.no_agents, view.x + 12.0, view.y + 6.0, &TextStyle { color: TIME, ..SMALL });
        }
        for (slot, row) in l.sessions.iter().zip(&m.agents) {
            let r = Rect { y: slot.rect.y - s, ..slot.rect };
            if r.bottom() < view.y || r.y > view.bottom() {
                continue;
            }
            let target = is_target(drag, &Target::Session(slot.key.clone()));
            let blocked = drag.is_some_and(|d| d.over == Some(Drop::Blocked(slot.key.clone())));
            if target {
                ring(p, r, 8.0);
            } else if drag.is_none() && hover_row == Some(slot.key.as_str()) && slot.takes {
                p.fill_round(r, 8.0, HOVER);
            }
            let opacity = if drag.is_some() && !slot.takes { 0.38 } else { 1.0 };
            p.with_opacity(opacity, |p| {
                let top = r.y + 8.0;
                icon(p, state_icon(row.state, row.needs_you), r.x + 8.0 + 8.0, top + h.h13 / 2.0, 13.0, INK);
                let text_x = r.x + 8.0 + 16.0 + 10.0;
                let time = ago(m.lang, m.now - row.since);
                let time_w = p.measure(&time, &TextStyle { color: TIME, ..SMALL }).0;
                p.text(&time, r.right() - 8.0 - time_w, top + 1.0, &TextStyle { color: TIME, ..SMALL });
                let max_w = (r.right() - 8.0 - time_w - 8.0 - text_x).max(40.0);
                if let Some(b) = p.text_box(&row.title, &ROW_TITLE, max_w, false) {
                    p.draw_box(&b, text_x, top);
                }
                let sub = if blocked { words.cannot_drop.to_owned() } else { sub_line(m, row) };
                if let Some(b) = p.text_box(&sub, &SMALL, r.right() - 8.0 - text_x, false) {
                    p.draw_box(&b, text_x, top + h.h13 + 2.0);
                }
            });
        }
    });
    paint_fade(p, view, s, l.scroll_max[3]);

    // New agent: a folder each, dropped on to start a chat there.
    p.fill_rect(Rect::new(agents.x + 12.0, l.new_head_y - 10.0, agents.w - 24.0, 1.0), HAIR);
    p.text(words.new_agent, agents.x + 12.0, l.new_head_y, &HEAD);
    let hover_folder = match hover {
        Some(BoardHit::NewChat(i)) => Some(*i),
        _ => None,
    };
    for (i, r) in &l.folders {
        let Some((cwd, project)) = m.folders.get(*i) else { continue };
        let target = is_target(drag, &Target::NewChat { cwd: cwd.clone(), project: project.clone() });
        if target {
            ring(p, *r, 8.0);
        } else {
            p.fill_round(*r, 8.0, CARD_BG);
            p.stroke_round_rect(Rect::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), 7.5, 1.0, if hover_folder == Some(*i) { HAIR } else { EDGE });
        }
        icon(p, Icon::Plus, r.x + 8.0 + 6.0, r.y + r.h / 2.0, 11.0, MUTED);
        let name = if cwd.is_empty() { m.lang.words().no_folder.to_owned() } else { project_name(cwd, project) };
        let style = TextStyle { color: INK, ..CHIP };
        if let Some(b) = p.text_box(&name, &style, r.w - 34.0, false) {
            p.draw_box(&b, r.x + 24.0, r.y + (r.h - b.height) / 2.0);
        }
    }

    // Footer: what the drop will do, the last result, or the hint.
    let footer = match drag.and_then(|d| d.over.as_ref()) {
        Some(Drop::Target(t)) => Some(Status { text: drop_words(m, drag.map_or("", |d| d.key.as_str()), t), tone: Tone::Info }),
        Some(Drop::Blocked(_)) => Some(Status { text: words.cannot_drop.to_owned(), tone: Tone::Error }),
        None => m.status.clone(),
    };
    let (text, style) = match &footer {
        Some(s) if drag.is_some() => (s.text.clone(), STATUS_HOT),
        Some(s) => (s.text.clone(), if s.tone == Tone::Info { STATUS } else { STATUS_HOT }),
        None => (words.hint.to_owned(), TextStyle { color: TIME, ..STATUS }),
    };
    let mut fx = card.x + PAD;
    if let Some(s) = &footer
        && s.tone != Tone::Info
    {
        let glyph = if s.tone == Tone::Ok { Icon::Done } else { Icon::Wait };
        icon(p, glyph, fx + 7.0, l.footer_y + h.h12 / 2.0 + 1.0, 13.0, INK);
        fx += 20.0;
    }
    if let Some(b) = p.text_box(&text, &style, card.right() - PAD - fx, false) {
        p.draw_box(&b, fx, l.footer_y);
    }

    // The card in hand, over everything.
    if let Some(d) = drag {
        p.with_opacity(0.97, |p| paint_ghost(p, m, &d.key, Rect::new(d.x, d.y, d.w, d.h)));
    }
}

/// A soft edge where a well's list runs on past its view: the well's own
/// colour, laid over in thin bands of growing strength.
fn paint_fade(p: &Painter, view: Rect, scroll: f32, max: f32) {
    if max <= 0.0 {
        return;
    }
    const BANDS: usize = 10;
    const DEPTH: f32 = 26.0;
    let band = DEPTH / BANDS as f32;
    let well = |p: &Painter, r: Rect, a: f32| {
        p.with_opacity(a, |p| {
            p.fill_rect(r, SURFACE);
            p.fill_rect(r, LANE);
        })
    };
    if scroll < max - 1.0 {
        for i in 0..BANDS {
            let a = (i + 1) as f32 / BANDS as f32;
            well(p, Rect::new(view.x + 1.0, view.bottom() - DEPTH + i as f32 * band, view.w - 2.0, band + 0.5), a * a);
        }
    }
    if scroll > 1.0 {
        for i in 0..BANDS {
            let a = (BANDS - i) as f32 / BANDS as f32;
            well(p, Rect::new(view.x + 1.0, view.y + i as f32 * band, view.w - 2.0, band + 0.5), a * a * 0.9);
        }
    }
}

/// «Отдать WEB-281 → Чистка документации» while the card hovers a target.
pub fn drop_words(m: &BoardModel, key: &str, t: &Target) -> String {
    let words = m.lang.board();
    match t {
        Target::Session(k) => {
            let name = m.agents.iter().find(|r| &r.key == k).map_or(k.as_str(), |r| r.title.as_str());
            format!("{} {key} → {name}", words.drop_session)
        }
        Target::NewChat { cwd, project } => {
            let name = if cwd.is_empty() { m.lang.words().no_folder.to_owned() } else { project_name(cwd, project) };
            format!("{} {name}: {key}", words.drop_new)
        }
        Target::Column(c) => format!("{} «{}»: {key}", words.drop_column, words.columns[c.index()]),
    }
}
