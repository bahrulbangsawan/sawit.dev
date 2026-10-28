mod spike;

use gpui_kit::*;
use pgdesk_core::rt::Runtime;

fn main() {
    let runtime = Runtime::start();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.set_global(spike::Rt(runtime));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(900.), px(500.)), cx)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("PG Desk — spike");
            cx.new(|cx| {
                let mut spike = spike::Spike::new(window, cx);
                if std::env::var_os("PGDESK_SPIKE_AUTORUN").is_some() {
                    spike.autorun(cx);
                }
                spike
            })
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}
