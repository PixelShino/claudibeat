//! Toasts above the strip, and the hover tooltip. Sizes follow `.toast` and
//! `.tip` in `docs/mockups/base.css`.

use super::icons::{Icon, icon};
use super::{Color, HOVER, INK, MUTED, Painter, Rect, SURFACE, TextStyle, Weight};
use crate::merge::{Row, open_target};
use crate::notify::{ToastEvent, ToastKind};
use crate::text::{Lang, ago, quote, title_done_batch, title_more_waiting};

pub const TOAST_W: f32 = 340.0;
/// Room around a toast card for its shadow.
pub const T_TOP: f32 = 16.0;
pub const T_SIDE: f32 = 20.0;
pub const T_BOTTOM: f32 = 36.0;
const RADIUS: f32 = 8.0;
const PAD_L: f32 = 14.0;
const PAD_R: f32 = 6.0;
const PAD_V: f32 = 12.0;
const ICON_COL: f32 = 18.0;
const COL_GAP: f32 = 10.0;
const CLOSE: f32 = 28.0;

const LABEL: TextStyle = TextStyle::new(12.0, Weight::Regular, MUTED);
const TITLE: TextStyle = TextStyle::new(13.0, Weight::Semibold, INK);
const ACTION: TextStyle = TextStyle::new(12.0, Weight::Semibold, INK);
const TIP: TextStyle = TextStyle::new(12.0, Weight::Regular, INK);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastModel {
    pub kind: ToastKind,
    pub label: String,
    pub title: String,
    pub sub: Option<String>,
    pub action: Option<&'static str>,
    pub age: String,
}

/// The words of a toast for `ev`, looked up in the current rows.
pub fn toast_model(ev: &ToastEvent, rows: &[Row], now: i64, lang: Lang) -> ToastModel {
    let w = lang.words();
    let find = |k: &str| rows.iter().find(|r| r.key == k);
    let first = ev.keys.first().and_then(|k| find(k));
    let titles = || {
        let names: Vec<&str> = ev.keys.iter().filter_map(|k| find(k)).map(|r| r.title.as_str()).collect();
        (!names.is_empty()).then(|| names.join(", "))
    };
    let opens = first.and_then(open_target).is_some();
    let (label, title, sub, action) = match ev.kind {
        ToastKind::Waiting => (
            w.waiting_one,
            first.map_or_else(String::new, |r| r.title.clone()),
            first.and_then(|r| r.ask.as_deref()).map(|a| quote(lang, a)),
            opens.then_some(w.open_chat),
        ),
        ToastKind::Done => (w.done, first.map_or_else(String::new, |r| r.title.clone()), None, opens.then_some(w.open_chat)),
        ToastKind::DoneBatch => (w.done, title_done_batch(lang, ev.count), titles(), Some(w.show_all)),
        ToastKind::MoreWaiting => (w.waiting_many, title_more_waiting(lang, ev.count), titles(), Some(w.show_all)),
    };
    ToastModel { kind: ev.kind, label: label.to_owned(), title, sub, action, age: ago(lang, now - ev.at) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToastLayout {
    pub width: f32,
    pub height: f32,
    pub card: Rect,
    pub close: Rect,
    pub action: Option<Rect>,
}

fn content_w() -> f32 {
    TOAST_W - PAD_L - ICON_COL - COL_GAP - COL_GAP - CLOSE - PAD_R
}

pub fn layout_toast(m: &ToastModel, p: &Painter) -> ToastLayout {
    let h12 = p.measure("Ag", &LABEL).1;
    let h13 = p.measure("Ag", &TITLE).1;
    let sub_h = m.sub.as_deref().and_then(|s| p.text_box(s, &LABEL, content_w(), true)).map_or(0.0, |b| (b.height.min(h12 * 2.0)) + 1.0);
    let content_h = h12 + 3.0 + h13 + sub_h + m.action.map_or(0.0, |_| 8.0 + h12);
    let card = Rect::new(T_SIDE, T_TOP, TOAST_W, PAD_V + content_h + PAD_V);
    let content_x = card.x + PAD_L + ICON_COL + COL_GAP;
    let action = m.action.map(|a| {
        let (w, _) = p.measure(a, &ACTION);
        Rect::new(content_x - 4.0, card.bottom() - PAD_V - h12 - 4.0, w + 8.0, h12 + 8.0)
    });
    ToastLayout {
        width: TOAST_W + 2.0 * T_SIDE,
        height: T_TOP + card.h + T_BOTTOM,
        card,
        close: Rect::new(card.right() - PAD_R - CLOSE, card.y + PAD_V - 6.0, CLOSE, CLOSE),
        action,
    }
}

pub fn paint_toast(p: &Painter, l: &ToastLayout, m: &ToastModel, hover_close: bool) {
    let card = l.card;
    p.card_shadow(card, RADIUS);
    p.fill_round(card, RADIUS, SURFACE);
    let h12 = p.measure("Ag", &LABEL).1;
    let h13 = p.measure("Ag", &TITLE).1;
    let content_x = card.x + PAD_L + ICON_COL + COL_GAP;
    let content_r = content_x + content_w();
    let mut y = card.y + PAD_V;

    let glyph = match m.kind {
        ToastKind::Waiting | ToastKind::MoreWaiting => Icon::Wait,
        ToastKind::Done | ToastKind::DoneBatch => Icon::Done,
    };
    icon(p, glyph, card.x + PAD_L + ICON_COL / 2.0, y + h12 / 2.0 + 1.0, 16.0, INK);

    p.text(&m.label, content_x, y, &LABEL);
    let (age_w, _) = p.measure(&m.age, &LABEL);
    p.text(&m.age, content_r - age_w, y, &LABEL);
    y += h12 + 3.0;
    if let Some(b) = p.text_box(&m.title, &TITLE, content_w(), false) {
        p.draw_box(&b, content_x, y);
    }
    y += h13;
    if let Some(sub) = &m.sub {
        p.clip(Rect::new(content_x, y + 1.0, content_w(), h12 * 2.0), |p| {
            if let Some(b) = p.text_box(sub, &LABEL, content_w(), true) {
                p.draw_box(&b, content_x, y + 1.0);
            }
        });
    }
    if let (Some(text), Some(r)) = (m.action, l.action) {
        let ty = r.y + 4.0;
        let w = p.text(text, content_x, ty, &ACTION);
        if let Some(b) = p.text_box(text, &ACTION, 1_000.0, false) {
            p.fill_rect(Rect::new(content_x, ty + b.baseline + 2.0, w, 1.0), INK);
        }
    }
    if hover_close {
        p.fill_round(l.close, 4.0, HOVER);
    }
    icon(p, Icon::Close, l.close.x + l.close.w / 2.0, l.close.y + l.close.h / 2.0, 12.0, MUTED);
}

/// Size of a tooltip window for `text`, shadow room included.
pub fn tooltip_size(p: &Painter, text: &str) -> (f32, f32) {
    let (w, h) = p.measure(text, &TIP);
    (w + 18.0 + 16.0, h + 10.0 + 16.0)
}

pub fn paint_tooltip(p: &Painter, text: &str) {
    let (w, h) = p.measure(text, &TIP);
    let card = Rect::new(8.0, 6.0, w + 18.0, h + 10.0);
    for i in 0..4 {
        let g = 8.0 - 2.0 * i as f32;
        p.fill_round(Rect::new(card.x - g / 2.0, card.y + 2.0 - g / 2.0, card.w + g, card.h + g), 5.0 + g / 2.0, Color(0x0500_0000));
    }
    p.fill_round(card, 5.0, Color(0xFFFB_FBFB));
    p.stroke_round_rect(Rect::new(card.x - 0.5, card.y - 0.5, card.w + 1.0, card.h + 1.0), 5.5, 1.0, Color(0x1400_0000));
    p.text(text, card.x + 9.0, card.y + 5.0, &TIP);
}
