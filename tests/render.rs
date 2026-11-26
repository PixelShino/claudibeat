use claudibeat::render::strip::{StripItem, StripModel, layout_strip, paint_strip};
use claudibeat::render::{Renderer, TRANSPARENT};

fn model(waiting: usize) -> StripModel {
    StripModel { waiting, done: 2, working: 4, tasks: None, mem_percent: 76, mem_alert: false, limit: Some((34, 0.34)) }
}

#[test]
fn wider_number_wider_item_and_no_overlap() {
    let r = Renderer::new().unwrap();
    let mut widths = Vec::new();
    for n in [1, 12] {
        r.render(400.0, 48.0, 1.0, TRANSPARENT, |p| {
            let l = layout_strip(&model(n), p);
            let rects: Vec<_> = l.items.iter().map(|(_, rect)| *rect).collect();
            for pair in rects.windows(2) {
                assert!(pair[0].x + pair[0].w <= pair[1].x + 0.01, "items overlap: {pair:?}");
            }
            assert_eq!(l.items.first().map(|(k, _)| *k), Some(StripItem::Waiting));
            assert_eq!(l.items.last().map(|(k, _)| *k), Some(StripItem::Limit));
            widths.push(l.items[0].1.w);
        })
        .unwrap();
    }
    assert!(widths[1] > widths[0], "{widths:?}");
}

#[test]
fn transparent_background_and_visible_ink() {
    let r = Renderer::new().unwrap();
    let frame = r
        .render(300.0, 48.0, 1.0, TRANSPARENT, |p| {
            let m = model(1);
            let l = layout_strip(&m, p);
            paint_strip(p, &l, &m, None);
        })
        .unwrap();
    assert_eq!((frame.width, frame.height), (300, 48));
    let alpha = |px: u32| px >> 24;
    assert!(frame.pixels.iter().any(|&px| alpha(px) == 0), "background stays transparent");
    assert!(frame.pixels.iter().any(|&px| alpha(px) == 255), "the black pill is opaque");
    // Premultiplied: no channel may exceed alpha.
    assert!(frame.pixels.iter().all(|&px| {
        let a = alpha(px);
        (px >> 16 & 0xFF) <= a && (px >> 8 & 0xFF) <= a && (px & 0xFF) <= a
    }));
}

#[test]
fn scale_doubles_pixels() {
    let r = Renderer::new().unwrap();
    let frame = r.render(100.0, 48.0, 2.0, TRANSPARENT, |_| {}).unwrap();
    assert_eq!((frame.width, frame.height), (200, 96));
}
