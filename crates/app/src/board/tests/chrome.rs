//! The chrome around the canvas: the toolbar with its flyouts and picker, the top bar, the toast,
//! and hiding the UI.

use super::*;

/// The tool the canvas's pointer events go to.
fn active_tool(view: &Entity<BoardView>, cx: &TestAppContext) -> ToolId {
    view.read_with(cx, |view, _| {
        sawit_board::toolbar::active(&view.board().unwrap().session)
    })
}

#[gpui_kit::test]
fn toolbar_click_selects_tool(cx: &mut TestAppContext) {
    let tmp = Scratch::new("toolbar-click");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    assert_eq!(active_tool(&view, &cx), ToolId::Move);
    // What the Sticky button does.
    view.update(&mut cx, |view, cx| {
        view.choose("tool.sticky", json!({}), cx)
    });
    assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
    cx.run_until_parked();
    // A click on a tool's button chooses it.
    click(&mut cx, "tool.hand");
    assert_eq!(active_tool(&view, &cx), ToolId::Hand);
    // The toolbar takes its clicks: with the sticky tool on, a click on a button puts no
    // sticky on the canvas under it.
    click(&mut cx, "tool.sticky");
    assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
    click(&mut cx, "tool.hand");
    assert_eq!(active_tool(&view, &cx), ToolId::Hand);
    assert!(layers(&view, &cx).is_empty());
}

#[gpui_kit::test]
fn toggle_ui_hides_chrome(cx: &mut TestAppContext) {
    let tmp = Scratch::new("toggle-ui");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    // ⌘\, as the canvas gets it while it has the keyboard (ruling P5-C3).
    focus_canvas(&view, &mut cx);
    let toggle = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("cmd-\\");
        cx.run_until_parked();
    };
    let hidden = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.board().unwrap().session.ds().unwrap().view.ui_hidden
        })
    };
    let chrome = |cx: &mut VisualTestContext| {
        ["board-top-bar", "board-toolbar"].map(|selector| cx.debug_bounds(selector).is_some())
    };
    let canvas_height =
        |cx: &VisualTestContext| view.read_with(cx, |view, _| f32::from(view.bounds.size.height));
    assert!(!hidden(&cx));
    assert_eq!(chrome(&mut cx), [true, true]);
    // The canvas is the window, less the top bar.
    assert_eq!(canvas_height(&cx), 600. - top_bar::HEIGHT);

    // Only the canvas renders, and it fills the window.
    toggle(&mut cx);
    assert!(hidden(&cx));
    assert_eq!(chrome(&mut cx), [false, false]);
    assert_eq!(canvas_height(&cx), 600.);

    toggle(&mut cx);
    assert!(!hidden(&cx));
    assert_eq!(chrome(&mut cx), [true, true]);
    assert_eq!(canvas_height(&cx), 600. - top_bar::HEIGHT);
}

#[gpui_kit::test]
fn flyout_closes_on_kind_change(cx: &mut TestAppContext) {
    let tmp = Scratch::new("flyout-kind");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let open = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.open_flyout);
    view.update(&mut cx, |view, cx| {
        view.open_flyout = Some(Flyout::StickyColors);
        cx.notify();
    });
    cx.run_until_parked();
    // Drawn over the board whose tool it belongs to, it stays open.
    assert_eq!(open(&cx), Some(Flyout::StickyColors));
    assert!(cx.debug_bounds("board-flyout").is_some());

    // A design file takes the board's place: drawn again, the flyout is closed, and the
    // toolbar is the design file's.
    view.update(&mut cx, |view, cx| {
        let design = figcraft_engine::Session::with_document(Document::new("Design"));
        view.replace_doc(BoardDoc { session: design });
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(open(&cx), None);
    assert!(cx.debug_bounds("board-flyout").is_none());
    assert!(cx.debug_bounds("tool.frame").is_some());
    assert!(cx.debug_bounds("tool.sticky").is_none());
}

#[gpui_kit::test]
fn a_flyout_choice_chooses_its_tool_and_closes_the_flyout(cx: &mut TestAppContext) {
    let tmp = Scratch::new("flyout-choice");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let open = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.open_flyout);
    let sticky_colour = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            let session = &view.board().unwrap().session;
            let items = sawit_board::toolbar::flyout(session, Flyout::StickyColors);
            let chosen = items.into_iter().find(|item| item.selected);
            chosen.map(|item| item.label)
        })
    };
    assert_eq!(sticky_colour(&cx).as_deref(), Some("yellow"));
    // The chevron beside the Sticky button opens the colours of new stickies.
    click(&mut cx, "board-flyout-StickyColors");
    cx.run_until_parked();
    assert_eq!(open(&cx), Some(Flyout::StickyColors));
    // A colour picked chooses the tool with that colour, and the flyout closes.
    click(&mut cx, "board-flyout-item-blue");
    cx.run_until_parked();
    assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
    assert_eq!(sticky_colour(&cx).as_deref(), Some("blue"));
    assert_eq!(open(&cx), None);
    assert!(cx.debug_bounds("board-flyout").is_none());
    // A list of labels works the same way.
    click(&mut cx, "board-flyout-Shapes");
    cx.run_until_parked();
    click(&mut cx, "board-flyout-item-Ellipse");
    cx.run_until_parked();
    assert_eq!(active_tool(&view, &cx), ToolId::BoardShape);
    let shape = view.read_with(&cx, |view, _| {
        view.board().unwrap().session.tool.board.shape
    });
    assert_eq!(shape, figcraft_doc::BoardShape::Ellipse);
    assert_eq!(open(&cx), None);
}

/// The user's ask: the frame and every kind of layer on a board's toolbar. The shape tools
/// share one button, the one used last, and a flyout that lists them with Place image…;
/// picking Ellipse there chooses it, and the button shows it from then on, as it does for a
/// tool chosen by its key.
#[gpui_kit::test]
fn a_groups_button_shows_the_tool_used_last(cx: &mut TestAppContext) {
    let tmp = Scratch::new("tool-groups");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let open = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.open_flyout);
    // A board's toolbar has the frame, the shapes and the pens.
    for selector in ["tool.frame", "tool.rectangle", "tool.pen", "tool.sticky"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "{selector} isn't drawn"
        );
    }
    assert!(cx.debug_bounds("tool.ellipse").is_none());
    click(&mut cx, "board-flyout-geometry");
    cx.run_until_parked();
    assert_eq!(open(&cx), Some(Flyout::Tools("geometry")));
    assert!(cx.debug_bounds("board-flyout-item-Place image…").is_some());
    click(&mut cx, "board-flyout-item-Ellipse");
    cx.run_until_parked();
    assert_eq!(active_tool(&view, &cx), ToolId::Ellipse);
    assert_eq!(open(&cx), None);
    assert!(cx.debug_bounds("tool.ellipse").is_some());
    assert!(cx.debug_bounds("tool.rectangle").is_none());
    // Another tool in use: the group's button still shows the ellipse, the one used last.
    click(&mut cx, "tool.hand");
    cx.run_until_parked();
    assert!(cx.debug_bounds("tool.ellipse").is_some());
    // A key chooses a tool of the group, and the button shows it.
    focus_canvas(&view, &mut cx);
    // ⇧8, which GPUI reports as `*`.
    cx.simulate_keystrokes("*");
    cx.run_until_parked();
    assert_eq!(active_tool(&view, &cx), ToolId::Star);
    assert!(cx.debug_bounds("tool.star").is_some());
}

#[gpui_kit::test]
fn the_top_bar_buttons_run_their_commands(cx: &mut TestAppContext) {
    let tmp = Scratch::new("top-bar");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let zoom = |cx: &VisualTestContext| view_state(&view, cx).zoom;
    assert_eq!(zoom(&cx), 1.0);
    // The zoom menu.
    click(&mut cx, "board-zoom");
    cx.run_until_parked();
    click(&mut cx, "board-zoom-view.zoom200");
    cx.run_until_parked();
    assert_eq!(zoom(&cx), 2.0);
    // Maximize asks the workspace to fill the window with the canvas.
    let asked = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let asked = asked.clone();
        cx.subscribe(
            &view,
            move |_, _: &super::super::ToggleCanvasMaximize, _| asked.set(asked.get() + 1),
        )
        .detach();
    });
    click(&mut cx, "board-maximize");
    assert_eq!(asked.get(), 1);
    // Hide UI hides the chrome, itself with it.
    click(&mut cx, "board-hide-ui");
    cx.run_until_parked();
    let hidden = view.read_with(&cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().view.ui_hidden
    });
    assert!(hidden);
    assert!(cx.debug_bounds("board-top-bar").is_none());
}

#[gpui_kit::test]
fn the_toast_sits_over_the_toolbar(cx: &mut TestAppContext) {
    let tmp = Scratch::new("toast-place");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    view.update(&mut cx, |view, cx| view.choose("no.such", json!({}), cx));
    cx.run_until_parked();
    let toast = cx.debug_bounds("board-toast").expect("a toast");
    let toolbar = cx.debug_bounds("board-toolbar").expect("the toolbar");
    assert_eq!(toast.bottom(), toolbar.top() - px(8.));
    // With the UI hidden, it is at the bottom of the canvas, where the toolbar was.
    view.update(&mut cx, |view, cx| {
        view.choose("view.toggleUi", json!({}), cx)
    });
    cx.run_until_parked();
    let toast = cx.debug_bounds("board-toast").expect("a toast");
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    assert_eq!(toast.bottom(), canvas.bottom() - px(16.));
}

#[gpui_kit::test]
fn the_toolbar_scrolls_inside_a_narrow_canvas(cx: &mut TestAppContext) {
    let tmp = Scratch::new("toolbar-narrow");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let drawn = |cx: &mut VisualTestContext, selector: &'static str| {
        let drawn = cx.debug_bounds(selector);
        drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"))
    };
    let resize = |cx: &mut VisualTestContext, width: f32| {
        cx.simulate_resize(size(px(width), px(600.)));
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        view.read_with(&*cx, |view, _| view.bounds)
    };
    // Its whole width shows: the first and the last buttons just inside its border and
    // padding, as the context bar's controls are, and in the middle of its height.
    let whole = |cx: &mut VisualTestContext| {
        let toolbar = drawn(cx, "board-toolbar");
        let (first, last) = (drawn(cx, "tool.move"), drawn(cx, "board-picker"));
        assert_eq!(first.left(), toolbar.left() + px(7.));
        assert_eq!(last.right(), toolbar.right() - px(7.));
        assert_eq!(first.center().y, toolbar.center().y);
        toolbar
    };
    // The 800 × 600 window has room for all of it, centred in the canvas.
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    let toolbar = whole(&mut cx);
    assert_eq!(toolbar.center().x, canvas.center().x);
    assert!(toolbar.size.width < canvas.size.width - px(16.));

    // A canvas narrower than the toolbar: it is as wide as the canvas less 8 px at each side,
    // and its buttons keep their size, the last ones past its right edge.
    let canvas = resize(&mut cx, 400.);
    let inside = |bounds: Bounds<Pixels>| {
        bounds.left() >= canvas.left() + px(8.) && bounds.right() <= canvas.right() - px(8.)
    };
    let toolbar = drawn(&mut cx, "board-toolbar");
    assert_eq!(toolbar.left(), canvas.left() + px(8.));
    assert_eq!(toolbar.right(), canvas.right() - px(8.));
    assert!(inside(drawn(&mut cx, "tool.move")));
    assert!(!inside(drawn(&mut cx, "board-picker")));
    // The wheel over it scrolls them sideways, and the picker's button comes into sight. The
    // toolbar takes the scroll: the canvas under it doesn't pan.
    let before = view_state(&view, &cx);
    cx.simulate_event(ScrollEvent {
        position: toolbar.center(),
        delta: ScrollDelta::Lines(point(0., -50.)),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
    let picker = drawn(&mut cx, "board-picker");
    assert!(inside(picker), "{picker:?} in {canvas:?}");
    // Scrolled to its end, it sits where it does in a toolbar of full width.
    assert_eq!(picker.right(), toolbar.right() - px(7.));
    assert!(!inside(drawn(&mut cx, "tool.move")));
    let after = view_state(&view, &cx);
    assert_eq!(
        (after.cx, after.cy, after.zoom),
        (before.cx, before.cy, before.zoom)
    );

    // With room again, it shows its whole width once more, from its first button.
    resize(&mut cx, 800.);
    whole(&mut cx);
}

#[gpui_kit::test]
fn the_picker_lists_the_templates_once_and_places_one(cx: &mut TestAppContext) {
    let tmp = Scratch::new("picker");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    click(&mut cx, "board-picker");
    cx.run_until_parked();
    assert!(cx.debug_bounds("board-picker-content").is_some());
    let listed = view.read_with(&cx, |view, _| {
        view.picker.as_ref().map(|picker| picker.templates.len())
    });
    assert_eq!(listed, Some(TEMPLATES.len()));
    // The frames drawn while it is open ask the engine for nothing more: the list is the one
    // it gave when the picker opened (ruling P3-C14).
    let revision =
        |cx: &VisualTestContext| view.read_with(cx, |view, _| view.board().unwrap().revision());
    let before = revision(&cx);
    for _ in 0..3 {
        view.update(&mut cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert_eq!(revision(&cx), before);
    // A template picked is placed, and the picker closes.
    view.update(&mut cx, |view, cx| view.place_template(TEMPLATES[0].id, cx));
    cx.run_until_parked();
    assert!(!layers(&view, &cx).is_empty());
    assert!(view.read_with(&cx, |view, _| view.picker.is_none()));
    assert!(cx.debug_bounds("board-picker-content").is_none());
    // And so does a table.
    let placed = layers(&view, &cx).len();
    click(&mut cx, "board-picker");
    cx.run_until_parked();
    view.update(&mut cx, |view, cx| view.place_table(2, 3, cx));
    cx.run_until_parked();
    assert_eq!(layers(&view, &cx).len(), placed + 1);
    assert!(view.read_with(&cx, |view, _| view.picker.is_none()));
}

#[test]
fn every_icon_of_the_chrome_is_embedded() {
    let tools = [DocKind::Board, DocKind::Design]
        .into_iter()
        .flat_map(sawit_board::toolbar::groups)
        .flat_map(|group| group.tools)
        .map(|tool| toolbar::tool_icon(tool.icon));
    let chrome = [
        // Place image…, at the end of the shape tools' flyout.
        toolbar::tool_icon(sawit_board::toolbar::ToolIcon::Image),
        toolbar::CHEVRON_ICON,
        toolbar::PICKER_ICON,
        toolbar::CHECK_ICON,
        top_bar::HIDE_UI_ICON,
        top_bar::MAXIMIZE_ICON,
        top_bar::MAIN_MENU_ICON,
        menus::SEARCH_ICON,
    ];
    for icon in tools.chain(chrome) {
        let path = icon.path();
        let svg = crate::assets::AppAssets.load(&path).unwrap();
        assert!(svg.is_some(), "{path} isn't embedded");
    }
}

#[test]
fn the_top_bar_names_the_canvas_and_shows_its_zoom() {
    let name = |path: &str| top_bar::document_name(Path::new(path));
    // The canvas of a project, and the one of a window without a project.
    assert_eq!(name("/p/.nyawit/canvas.sawit").as_ref(), "Canvas");
    assert_eq!(name("/sawit/canvas.sawit").as_ref(), "Canvas");
    assert_eq!(
        name("/p/design/ridgeline-app.sawit").as_ref(),
        "ridgeline-app.sawit"
    );
    assert_eq!(top_bar::zoom_label(1.0), "100%");
    assert_eq!(top_bar::zoom_label(0.5), "50%");
    assert_eq!(top_bar::zoom_label(1.234), "123%");
}
