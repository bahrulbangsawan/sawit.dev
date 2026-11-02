//! The ops' wire format, and which ones the app answers itself.

use serde_json::json;

use crate::browser::{BrowserOp, Target};

#[test]
fn ops_wire_format() {
    let op: BrowserOp =
        serde_json::from_str(r#"{"op":"click","ref":"e3","double":false}"#).unwrap();
    assert_eq!(
        op,
        BrowserOp::Click {
            target: Target {
                reference: Some("e3".into()),
                selector: None
            },
            double: false,
            button: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"op": "click", "ref": "e3", "double": false})
    );
    let op: BrowserOp = serde_json::from_str(r#"{"op":"type","text":"hi"}"#).unwrap();
    assert!(matches!(op, BrowserOp::Type { ref target, clear: false, .. } if target.is_empty()));
    assert!(BrowserOp::State.is_native() && !BrowserOp::Snapshot.is_native());
    assert!(serde_json::from_str::<BrowserOp>(r#"{"op":"fly"}"#).is_err());
}

#[test]
fn annotations_op_is_native_and_stays_in_the_background() {
    let op: BrowserOp = serde_json::from_str(r#"{"op":"annotations","clear":true}"#).unwrap();
    assert_eq!(op, BrowserOp::Annotations { clear: true });
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"op": "annotations", "clear": true})
    );
    let bare: BrowserOp = serde_json::from_str(r#"{"op":"annotations"}"#).unwrap();
    assert_eq!(bare, BrowserOp::Annotations { clear: false });
    assert!(op.is_native() && !op.shows_tab());
}

#[test]
fn design_ops_are_native_and_round_trip() {
    let target = Target {
        reference: Some("e3".into()),
        selector: None,
    };
    for (op, wire) in [
        (
            BrowserOp::Inspect {
                target: target.clone(),
                format: Some("css".into()),
            },
            json!({"op": "inspect", "ref": "e3", "format": "css"}),
        ),
        (
            BrowserOp::Inspect {
                target: Target {
                    reference: None,
                    selector: Some("h1".into()),
                },
                format: None,
            },
            json!({"op": "inspect", "selector": "h1"}),
        ),
        (
            BrowserOp::Design {
                part: Some("colors".into()),
            },
            json!({"op": "design", "part": "colors"}),
        ),
        (BrowserOp::Design { part: None }, json!({"op": "design"})),
        (
            BrowserOp::ExportDesign {
                include_videos: true,
                replace: true,
            },
            json!({"op": "export_design", "include_videos": true, "replace": true}),
        ),
        (BrowserOp::ExportStatus, json!({"op": "export_status"})),
    ] {
        assert_eq!(serde_json::to_value(&op).unwrap(), wire);
        assert_eq!(serde_json::from_value::<BrowserOp>(wire).unwrap(), op);
        assert!(op.is_native(), "{op:?}");
        assert!(!op.shows_tab(), "{op:?}");
    }
    // An export replaces nothing unless asked to.
    let bare: BrowserOp = serde_json::from_str(r#"{"op":"export_design"}"#).unwrap();
    assert_eq!(
        bare,
        BrowserOp::ExportDesign {
            include_videos: false,
            replace: false,
        }
    );
}
