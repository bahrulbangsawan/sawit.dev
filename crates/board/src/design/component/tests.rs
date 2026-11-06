//! The Component section's model.

use serde_json::json;

use super::*;
use crate::design::lists::testing::{apply, design_file, rectangle};

fn id_of(value: &Value) -> NodeId {
    figcraft_engine::params::id_of(value).unwrap()
}

#[test]
fn a_main_gets_properties_and_its_instance_takes_values() {
    let mut board = design_file();
    let r = rectangle(&mut board, "Button", 0.0);
    let made = board
        .execute("component.create", &json!({"ids": [r]}))
        .unwrap();
    let main = made.get("id").map(id_of).unwrap_or(r);
    let doc = board.doc().unwrap();
    let p = panel(&doc, &[main]).unwrap();
    assert!(matches!(p.role, Role::Main { set: None }));
    assert!(p.props.is_empty());

    apply(&mut board, &add_property(main, "Disabled", "boolean"));
    apply(&mut board, &add_property(main, "Label", "text"));
    let doc = board.doc().unwrap();
    let names: Vec<String> = panel(&doc, &[main])
        .unwrap()
        .props
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, ["Disabled", "Label"]);
    apply(&mut board, &rename_property(main, "Label", "Title"));

    let made = board
        .execute(
            "instance.create",
            &json!({"component": main, "x": 400, "y": 0}),
        )
        .unwrap();
    let instance = id_of(&made["id"]);
    apply(&mut board, &set_value(instance, "Disabled", json!(true)));
    let doc = board.doc().unwrap();
    let p = panel(&doc, &[instance]).unwrap();
    assert!(matches!(&p.role, Role::Instance { main: m, .. } if *m == main));
    let disabled = p.props.iter().find(|p| p.name == "Disabled").unwrap();
    assert_eq!(
        (disabled.kind.clone(), disabled.value.clone()),
        (PropKind::Boolean, json!(true))
    );
    assert!(p.props.iter().any(|p| p.name == "Title"));

    apply(&mut board, &delete_property(main, "Disabled"));
    let doc = board.doc().unwrap();
    assert!(
        panel(&doc, &[instance])
            .unwrap()
            .props
            .iter()
            .all(|p| p.name != "Disabled")
    );
    assert_eq!(
        new_property_name(&panel(&doc, &[main]).unwrap()),
        "Property 1"
    );
}

#[test]
fn a_set_lists_its_variants_and_an_instance_switches_between_them() {
    let mut board = design_file();
    let r = rectangle(&mut board, "Size=Small", 0.0);
    let made = board
        .execute("component.create", &json!({"ids": [r]}))
        .unwrap();
    let main = made.get("id").map(id_of).unwrap_or(r);
    apply(&mut board, &add_variant(main));
    let doc = board.doc().unwrap();
    let set = match panel(&doc, &[main]).unwrap().role {
        Role::Main { set } => set.expect("a set of itself and the new variant"),
        other => panic!("{other:?}"),
    };
    let p = panel(&doc, &[set]).unwrap();
    assert_eq!(p.role, Role::Set);
    let size = p
        .props
        .iter()
        .find(|p| p.name == "Size")
        .expect("the variant property");
    let PropKind::Variant(options) = &size.kind else {
        panic!("{size:?}")
    };
    assert_eq!(options.len(), 2, "{options:?}");
    assert!(swap_targets(&doc).iter().any(|(id, _)| *id == set));
}
