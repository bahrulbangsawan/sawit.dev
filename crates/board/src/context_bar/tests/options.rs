//! The options of each control: what each runs, which one is selected, and that they run.

use super::*;

/// What picking each option of `control` runs, and what the option is called: `(command,
/// params, label)`.
fn picks(control: &Control) -> Vec<(&'static str, Value, String)> {
    let options = options(control).into_iter();
    options.map(|o| (o.command, o.params, o.label)).collect()
}

/// The labels of the options of `control` that show as selected.
fn selected(control: &Control) -> Vec<String> {
    let options = options(control).into_iter();
    options.filter(|o| o.selected).map(|o| o.label).collect()
}

#[test]
fn controls_run() {
    let mut b = board();
    let items = [
        sticky(&mut b),
        shape(&mut b),
        connector(&mut b),
        ink(&mut b),
    ];
    let mut run = 0;
    for item in items {
        select(&mut b, &[item]);
        let before = bar(&b);
        for (at, control) in before.controls.iter().enumerate() {
            // The first option that is not the item's value now: the engine refuses to give a
            // shape the outline it has.
            let choices = options(control);
            let picked = choices.iter().position(|option| !option.selected).unwrap();
            let ControlOption {
                command, params, ..
            } = &choices[picked];
            b.execute(command, params)
                .unwrap_or_else(|e| panic!("{item:?} {control:?} {command} {params}: {e}"));
            // And the bar shows what was picked, and nothing else, as the value now.
            let after = bar(&b);
            assert_eq!(kinds(&after), kinds(&before));
            let on: Vec<usize> = options(&after.controls[at])
                .iter()
                .enumerate()
                .filter(|(_, option)| option.selected)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(on, [picked], "{item:?} {command} {params}");
            run += 1;
        }
    }
    // 1 for the sticky, 3 for the shape, 4 for the connector and 1 for the ink.
    assert_eq!(run, 9);
}

#[test]
fn options_are_the_commands_the_controls_run() {
    // Written out here, not read from the lists the engine keeps them in, so that a change to
    // one of those has to be seen.
    let colour = |name: &str| ("board.setColor", json!({ "color": name }), name.to_owned());
    let style = |key: &str, value: Value, label: &str| {
        ("board.setStyle", json!({ key: value }), label.to_owned())
    };

    let colours = [
        "yellow", "orange", "red", "pink", "violet", "blue", "green", "gray",
    ];
    assert_eq!(picks(&colors(None)), colours.map(colour));

    let shapes = [
        ("rectangle", "Rectangle"),
        ("roundedRectangle", "Rounded rectangle"),
        ("ellipse", "Ellipse"),
        ("diamond", "Diamond"),
        ("triangle", "Triangle"),
        ("speechBubble", "Speech bubble"),
        ("chevron", "Chevron"),
    ];
    assert_eq!(
        picks(&Control::Shape { current: None }),
        shapes.map(|(id, label)| style("shape", json!(id), label))
    );

    // The size is the number, never its name.
    let sizes = [("S", 12.0), ("M", 16.0), ("L", 24.0), ("XL", 36.0)];
    assert_eq!(
        picks(&Control::FontSize { current: None }),
        sizes.map(|(name, size)| style("fontSize", json!(size), name))
    );

    let routings = [
        ("straight", "Straight"),
        ("elbow", "Elbow"),
        ("curved", "Curved"),
    ];
    assert_eq!(
        picks(&Control::Routing { current: None }),
        routings.map(|(id, label)| style("style", json!(id), label))
    );

    let caps = [
        ("none", "None"),
        ("lineArrow", "Line arrow"),
        ("triangleArrow", "Triangle arrow"),
        ("reversedTriangle", "Reversed triangle"),
        ("circleFilled", "Circle"),
        ("diamondFilled", "Diamond"),
    ];
    assert_eq!(
        picks(&Control::StartCap { current: None }),
        caps.map(|(name, label)| style("startCap", json!(name), label))
    );
    assert_eq!(
        picks(&Control::EndCap { current: None }),
        caps.map(|(name, label)| style("endCap", json!(name), label))
    );

    // The colours are the only options with a swatch, and it is the control's own.
    let swatches: Vec<_> = options(&colors(None)).iter().map(|o| o.swatch).collect();
    let palette: Vec<_> = palette().iter().map(|(_, rgba)| Some(*rgba)).collect();
    assert_eq!(swatches, palette);
    for control in [
        Control::Shape { current: None },
        Control::FontSize { current: None },
        Control::Routing { current: None },
        Control::StartCap { current: None },
        Control::EndCap { current: None },
    ] {
        assert!(options(&control).iter().all(|o| o.swatch.is_none()));
    }

    // The colours are the control's own, whole: all five fields of an option.
    let own = Control::Colors {
        items: vec![("mauve", [1, 2, 3, 255])],
        current: Some("mauve"),
    };
    assert_eq!(
        options(&own),
        [ControlOption {
            command: "board.setColor",
            params: json!({ "color": "mauve" }),
            label: "mauve".to_owned(),
            swatch: Some([1, 2, 3, 255]),
            selected: true,
        }]
    );
}

#[test]
fn the_selected_option_is_the_value_the_control_shows() {
    assert_eq!(selected(&colors(Some("blue"))), ["blue"]);
    assert_eq!(
        selected(&Control::Shape {
            current: Some(BoardShape::Ellipse)
        }),
        ["Ellipse"]
    );
    assert_eq!(selected(&Control::FontSize { current: Some("L") }), ["L"]);
    assert_eq!(
        selected(&Control::Routing {
            current: Some(Routing::Curved)
        }),
        ["Curved"]
    );
    let cap = |name: &str| Some(name.to_owned());
    assert_eq!(
        selected(&Control::StartCap {
            current: cap("lineArrow")
        }),
        ["Line arrow"]
    );
    assert_eq!(
        selected(&Control::EndCap {
            current: cap("diamondFilled")
        }),
        ["Diamond"]
    );

    // No value (the items differ), or one the bar has no option for (a plain round cap, which
    // FigCraft has but the bar doesn't offer): nothing shows as selected.
    for control in [
        colors(None),
        Control::Shape { current: None },
        Control::FontSize { current: None },
        Control::Routing { current: None },
        Control::StartCap { current: None },
        Control::EndCap {
            current: cap("round"),
        },
    ] {
        assert_eq!(selected(&control), Vec::<String>::new(), "{control:?}");
    }

    // A real selection: a connector is elbow, has no cap at its start and a triangle at its
    // end, and its line is in no board colour.
    let mut b = board();
    connector(&mut b);
    let on: Vec<_> = bar(&b).controls.iter().map(selected).collect();
    assert_eq!(
        on,
        [vec![], vec!["Elbow"], vec!["None"], vec!["Triangle arrow"]]
    );
}

#[test]
fn every_option_runs_but_the_shape_the_item_has() {
    let mut b = board();
    let items = [
        sticky(&mut b),
        shape(&mut b),
        connector(&mut b),
        ink(&mut b),
    ];
    let mut run = 0;
    for item in items {
        select(&mut b, &[item]);
        for at in 0..bar(&b).controls.len() {
            for pick in 0..options(&bar(&b).controls[at]).len() {
                // What is selected moves with each pick, so ask the bar again every time.
                let control = bar(&b).controls[at].clone();
                let option = options(&control).swap_remove(pick);
                let result = b.execute(option.command, &option.params);
                // The engine refuses a shape the item has already. Everything else works,
                // whether or not it is what the item has now.
                let refused = matches!(control, Control::Shape { .. }) && option.selected;
                assert_eq!(
                    result.is_err(),
                    refused,
                    "{item:?} {} {}: {result:?}",
                    option.command,
                    option.params
                );
                run += 1;
            }
        }
    }
    // The 8 colours for each of the 4 items, 7 shapes, 4 sizes, 3 routings, and 6 caps twice.
    assert_eq!(run, 4 * 8 + 7 + 4 + 3 + 2 * 6);
}
