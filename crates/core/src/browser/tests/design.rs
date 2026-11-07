//! The design module: the contract's fields. Its colour math is Rust's now
//! (`site_design::computed`, tested against the page's old JavaScript).

use serde_json::Value;

use crate::browser::DESIGN_JS;

/// Every field of the design contract fixtures is written by the design module.
#[test]
fn design_module_writes_the_contract_fields() {
    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    out.push(k.clone());
                    // Its keys are colours the page read, not fields.
                    if k != "color_fallbacks" {
                        keys(v, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for fixture in [
        include_str!("../../site_design/fixtures/page_scan.json"),
        include_str!("../../site_design/fixtures/page_samples.json"),
        include_str!("../../site_design/fixtures/page_rules.json"),
        include_str!("../../site_design/fixtures/page_style.json"),
    ] {
        keys(&serde_json::from_str(fixture).unwrap(), &mut all);
    }
    for key in all {
        let written = [
            format!("{key}:"),
            format!("{{ {key},"),
            format!(" {key},"),
            format!(" {key} }}"),
        ];
        assert!(
            written.iter().any(|w| DESIGN_JS.contains(w.as_str())),
            "DESIGN_JS never writes `{key}`"
        );
    }
}
