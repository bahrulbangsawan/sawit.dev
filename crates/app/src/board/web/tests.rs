use figcraft_doc::NodeId;
use figcraft_engine::ViewState;
use gpui_kit::{point, px};
use serde_json::json;

use super::bench::{corners, screen_bounds, stats};
use super::live::live_place;
use super::{Source, Spec, Viewport, WebNode, spec};
use sawit_board::routes::WEB_KEY as KEY;

fn node_with(data: Option<serde_json::Value>) -> figcraft_doc::Node {
    let kind = figcraft_doc::NodeKind::Frame;
    let mut node = figcraft_doc::Node::new(NodeId(3), kind, "Web", 1440.0, 900.0);
    if let Some(data) = data {
        node.plugin_data.insert(KEY.into(), data.to_string());
    }
    node
}

/// A frame with route data is a web frame, on by default: a page of this Mac loads, and one
/// elsewhere is a frame that is refused, so it says why rather than load.
#[test]
fn a_frame_with_web_data_is_a_web_frame() {
    let data = json!({"url": "http://localhost:8765/", "viewport": {"w": 1440, "h": 900}});
    assert_eq!(
        spec(&node_with(Some(data))),
        Some(Spec {
            source: Source::Url("http://localhost:8765/".into()),
            viewport: Viewport {
                w: 1440.0,
                h: 900.0
            },
        })
    );
    let ipv6 = json!({"url": "http://[::1]:5173/changelog", "viewport": {"w": 390, "h": 844}});
    assert!(matches!(
        spec(&node_with(Some(ipv6))).unwrap().source,
        Source::Url(_)
    ));
    assert_eq!(spec(&node_with(None)), None, "no data");
    let remote = json!({"url": "https://example.com/", "viewport": {"w": 1440, "h": 900}});
    assert_eq!(
        spec(&node_with(Some(remote))).map(|s| s.source),
        Some(Source::Remote("https://example.com/".into())),
        "a remote page is refused"
    );
    let empty = json!({"url": "http://localhost:1/", "viewport": {"w": 0, "h": 900}});
    assert_eq!(spec(&node_with(Some(empty))), None, "no viewport");
}

/// A screen's frame is a web frame whether URL frames are on or not; one whose name couldn't be a
/// folder's isn't.
#[test]
fn a_screens_frame_is_always_a_web_frame() {
    let mut node = node_with(None);
    let data = json!({"screen": "sign-in", "viewport": {"w": 390, "h": 844}});
    node.plugin_data
        .insert(sawit_board::screens::SCREEN_KEY.into(), data.to_string());
    let found = spec(&node).unwrap();
    assert_eq!(found.source, Source::Screen("sign-in".into()));
    assert_eq!(found.viewport, Viewport { w: 390.0, h: 844.0 });
    let bad = json!({"screen": "../x", "viewport": {"w": 390, "h": 844}});
    node.plugin_data
        .insert(sawit_board::screens::SCREEN_KEY.into(), bad.to_string());
    assert_eq!(spec(&node), None);
}

fn web_node(rect: [f64; 4], w: f64) -> WebNode {
    WebNode {
        id: NodeId(7),
        spec: Spec {
            source: Source::Url("http://localhost:1/".into()),
            viewport: Viewport { w, h: 900.0 },
        },
        rect,
        radii: [12.0, 0.0, 6.0, 0.0],
    }
}

#[test]
fn the_live_view_is_the_node_times_the_zoom_at_page_zoom() {
    let view = ViewState {
        cx: 720.0,
        cy: 450.0,
        zoom: 0.5,
        vw: 1000.0,
        vh: 800.0,
        ..ViewState::default()
    };
    // A 1440 × 900 frame at the page origin, the view's centre on its centre.
    let (frame, zoom) = live_place(&web_node([0.0, 0.0, 1440.0, 900.0], 1440.0), &view);
    assert_eq!(
        (frame.x, frame.y, frame.width, frame.height),
        (140.0, 175.0, 720.0, 450.0)
    );
    assert_eq!(zoom, 0.5);
    // A frame resized to twice its viewport's width lays the page out at the viewport's width.
    let (_, zoom) = live_place(&web_node([0.0, 0.0, 2880.0, 900.0], 1440.0), &view);
    assert_eq!(zoom, 1.0);
}

#[test]
fn textures_go_where_their_frames_are_on_screen() {
    let view = ViewState {
        zoom: 2.0,
        vw: 800.0,
        vh: 600.0,
        ..ViewState::default()
    };
    let at = screen_bounds([10.0, 20.0, 110.0, 70.0], &view, point(px(5.0), px(50.0)));
    // The page origin is the canvas's centre: (400, 300) on the canvas.
    assert_eq!(
        at.origin,
        point(px(5.0 + 400.0 + 20.0), px(50.0 + 300.0 + 40.0))
    );
    assert_eq!(
        (f32::from(at.size.width), f32::from(at.size.height)),
        (200.0, 100.0)
    );
    let radii = corners([12.0, 0.0, 6.0, 0.0], 2.0);
    assert_eq!((radii.top_left, radii.bottom_right), (px(24.0), px(12.0)));
}

#[test]
fn bench_stats_read_the_sorted_samples() {
    let mut values = vec![4.0, 1.0, 3.0, 2.0, 100.0];
    let report = stats(&mut values);
    assert_eq!(report["p50"], json!(3.0));
    assert_eq!(report["max"], json!(100.0));
    assert_eq!(report["mean"], json!(22.0));
    assert_eq!(stats(&mut []), json!(null));
}
