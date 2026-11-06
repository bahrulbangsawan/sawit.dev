//! An artboard as HTML and React: auto layout, text styles, images, an instance with variants,
//! and a frame with children placed absolutely.

use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::{REFERENCE, find_frame, is_artboard, to_html, to_react};
use crate::doc::BoardDoc;

fn design() -> BoardDoc {
    BoardDoc {
        session: Session::with_document(Document::new("Design")),
    }
}

fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

fn set(board: &mut BoardDoc, id: NodeId, props: Value) {
    board
        .execute("node.set", &json!({"ids": [id], "props": props}))
        .unwrap();
}

/// The rule of class `class` in `css`, its declarations.
fn rule<'a>(css: &'a str, class: &str) -> &'a str {
    let open = format!(".{class} {{\n");
    let at = css
        .find(&open)
        .unwrap_or_else(|| panic!("no .{class} in {css}"));
    let body = &css[at + open.len()..];
    &body[..body.find('}').unwrap()]
}

/// A frame in auto layout is a flexbox; its text keeps its type; layer names are classes and
/// data-name; the file says it is a reference.
#[test]
fn auto_layout_and_text_styles() {
    let mut board = design();
    let card = make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 320, "height": 200, "name": "Plan card", "props": {"fill": "#ffffff"}}),
    );
    let title = make(
        &mut board,
        "text.create",
        json!({"x": 0, "y": 0, "text": "Pro plan", "parent": card, "name": "Title",
        "props": {"fontSize": 24, "fontWeight": 600, "fill": "#111827", "letterSpacing": "-2%", "lineHeight": 32}}),
    );
    make(
        &mut board,
        "text.create",
        json!({"x": 0, "y": 40, "text": "For teams", "parent": card, "name": "Body", "props": {"fontSize": 16, "fill": "#4b5563"}}),
    );
    board
        .execute("layout.add", &json!({"ids": [card]}))
        .unwrap();
    set(
        &mut board,
        card,
        json!({"layout": "vertical", "gap": 8, "padding": 24, "sizingV": "hug"}),
    );
    assert!(is_artboard(&board.doc().unwrap(), card));
    assert!(!is_artboard(&board.doc().unwrap(), title));

    let (html, _) = to_html(&mut board, card).unwrap();
    assert!(
        html.starts_with("<!doctype html>\n<!-- Mechanical reference"),
        "{html}"
    );
    assert!(
        html.contains(r#"<div class="plan-card" data-name="Plan card">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<p class="title" data-name="Title">Pro plan</p>"#),
        "{html}"
    );
    let card_css = rule(&html, "plan-card");
    for decl in [
        "display: flex;",
        "flex-direction: column;",
        "gap: 8px;",
        "padding: 24px 24px 24px 24px;",
        "background: #FFFFFF",
    ] {
        assert!(card_css.contains(decl), "{decl} not in {card_css}");
    }
    let title_css = rule(&html, "title");
    for decl in [
        "font-size: 24px;",
        "font-weight: 600;",
        "line-height: 32px;",
        "letter-spacing: -0.02em;",
        "color: #111827",
        "margin: 0;",
    ] {
        assert!(title_css.contains(decl), "{decl} not in {title_css}");
    }
    // A text that hugs has no fixed size; a child in the flow isn't placed.
    assert!(
        !title_css.contains("width:") && !title_css.contains("position: absolute"),
        "{title_css}"
    );

    let react = to_react(&mut board, card, None, "PlanCard.module.css").unwrap();
    keep_for_tsc(
        &react.tsx,
        "PlanCard.tsx",
        &react.css,
        "PlanCard.module.css",
    );
    assert_eq!(react.component, "PlanCard");
    assert!(
        react.tsx.starts_with("// Mechanical reference"),
        "{}",
        react.tsx
    );
    assert!(
        react
            .tsx
            .contains("import styles from \"./PlanCard.module.css\";"),
        "{}",
        react.tsx
    );
    assert!(
        react.tsx.contains("export default function PlanCard() {"),
        "{}",
        react.tsx
    );
    assert!(
        react
            .tsx
            .contains("<p className={styles[\"title\"]} data-name={\"Title\"}>{\"Pro plan\"}</p>"),
        "{}",
        react.tsx
    );
    assert_eq!(
        rule(&react.css, "plan-card"),
        card_css,
        "the same CSS as the HTML"
    );
    assert_eq!(
        find_frame(&board.doc().unwrap(), None, "Plan card"),
        Some(card)
    );
}

/// Children of a frame without auto layout are placed absolutely, from its corner.
#[test]
fn absolute_children() {
    let mut board = design();
    let hero = make(
        &mut board,
        "frame.create",
        json!({"x": 100, "y": 50, "width": 400, "height": 300, "name": "Hero"}),
    );
    let badge = make(
        &mut board,
        "shape.rectangle",
        json!({"x": 16, "y": 32, "width": 80, "height": 24, "parent": hero, "name": "Badge"}),
    );
    set(
        &mut board,
        badge,
        json!({"fill": "#fde68a", "cornerRadius": 12}),
    );
    let (html, _) = to_html(&mut board, hero).unwrap();
    let css = rule(&html, "badge");
    for decl in [
        "position: absolute;",
        "left: 16px;",
        "top: 32px;",
        "width: 80px;",
        "height: 24px;",
        "border-radius: 12px;",
    ] {
        assert!(css.contains(decl), "{decl} not in {css}");
    }
    assert!(rule(&html, "hero").contains("position: relative;"));
}

/// An image fill is embedded as a data: URL.
#[test]
fn images_are_embedded() {
    let mut board = design();
    let frame = make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 200, "height": 200, "name": "Gallery"}),
    );
    let mut png = Vec::new();
    let pixels = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255]));
    pixels
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(&png);
    make(
        &mut board,
        "image.place",
        json!({"data": data, "x": 10, "y": 10, "width": 100, "height": 100, "parent": frame, "name": "Photo"}),
    );
    let (html, _) = to_html(&mut board, frame).unwrap();
    let css = rule(&html, "photo");
    assert!(css.contains("url(\"data:image/png;base64,"), "{css}");
    assert!(!html.contains("url(image.png)"), "{html}");
}

/// An instance of a component with variants is a React component with a prop for the variant
/// property and its text overrides; HTML inlines it.
#[test]
fn an_instance_with_variants() {
    let mut board = design();
    let variant = |board: &mut BoardDoc, x: f64, name: &str, fill: &str| {
        let f = make(
            board,
            "frame.create",
            json!({"x": x, "y": 0, "width": 120, "height": 40, "name": name, "props": {"fill": fill}}),
        );
        make(
            board,
            "text.create",
            json!({"x": 12, "y": 10, "text": "Save", "parent": f, "name": "Label", "props": {"fill": "#ffffff"}}),
        );
        board
            .execute("component.create", &json!({"ids": [f]}))
            .unwrap();
        f
    };
    let small = variant(&mut board, 0.0, "Size=Small", "#1d4ed8");
    let large = variant(&mut board, 200.0, "Size=Large", "#1e40af");
    let set_id = make(
        &mut board,
        "component.combineVariants",
        json!({"ids": [small, large]}),
    );
    board
        .execute(
            "node.set",
            &json!({"ids": [set_id], "props": {"name": "Button"}}),
        )
        .unwrap();
    let page = make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 300, "width": 400, "height": 200, "name": "Form"}),
    );
    let one = make(
        &mut board,
        "instance.create",
        json!({"component": large, "x": 20, "y": 20, "parent": page}),
    );
    let doc = board.doc().unwrap();
    let label = doc
        .subtree(one)
        .into_iter()
        .find(|id| doc.node(*id).is_some_and(|n| n.name == "Label"))
        .unwrap();
    set(&mut board, label, json!({"characters": "Send"}));

    let react = to_react(&mut board, page, None, "Form.module.css").unwrap();
    keep_for_tsc(&react.tsx, "Form.tsx", &react.css, "Form.module.css");
    let tsx = &react.tsx;
    assert!(tsx.contains("type ButtonProps = {\n  size?: \"Small\" | \"Large\";\n  texts?: Partial<Record<\"Label\", string>>;\n  className?: string;\n};"), "{tsx}");
    assert!(tsx.contains("export function Button({"), "{tsx}");
    assert!(tsx.contains("{texts?.[\"Label\"] ?? \"Save\"}"), "{tsx}");
    assert!(
        tsx.contains("size={\"Large\"} texts={{ \"Label\": \"Send\" }} />"),
        "{tsx}"
    );
    let (html, _) = to_html(&mut board, page).unwrap();
    assert!(html.contains(">Send</p>"), "{html}");
    assert!(
        !html.contains("<Button"),
        "HTML inlines the instance: {html}"
    );
    assert!(REFERENCE.contains("not production code"));
}

/// The `frame_code` op finds a frame by name and answers its code; FigCraft's own `inspect.code`
/// runs on a board too, so rule cnv.3 names a command that is there.
#[test]
fn the_op_and_inspect_code() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let frame = make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 200, "height": 100, "name": "Banner"}),
    );
    let html = crate::ops::run(&mut board, &json!({"op": "frame_code", "id": "Banner"}));
    assert_eq!(html["ok"], true, "{html}");
    assert!(
        html["html"]
            .as_str()
            .unwrap()
            .contains("data-name=\"Banner\"")
    );
    let react = crate::ops::run(
        &mut board,
        &json!({"op": "frame_code", "id": frame.to_string(), "format": "react"}),
    );
    assert_eq!(react["component"], "Banner");
    assert_eq!(react["css_file"], "Banner.module.css");
    for language in ["html", "react", "tailwind"] {
        let code = crate::ops::run(
            &mut board,
            &json!({"op": "command", "command": "inspect.code", "params": {"id": frame.0, "language": language}}),
        );
        assert_eq!(code["ok"], true, "{language}: {code}");
    }
}

/// With SAWIT_TSX_OUT set, writes a `.tsx` and its CSS module there, for `tsc --noEmit` in a fresh
/// Vite + React + TypeScript app (the check item 25 asks for).
fn keep_for_tsc(tsx: &str, tsx_name: &str, css: &str, css_name: &str) {
    if let Some(dir) = std::env::var_os("SAWIT_TSX_OUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(tsx_name), tsx).unwrap();
        std::fs::write(dir.join(css_name), css).unwrap();
    }
}
