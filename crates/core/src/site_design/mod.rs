pub mod assets;
pub mod color;
pub mod export;
pub mod inspect;
pub mod scan;
pub mod tailwind;
pub mod tokens;

/// Parses `json` as `T` and checks that serializing it back gives the same JSON (numbers
/// compared by value), so a misspelled, missing or extra field fails.
#[cfg(test)]
pub(crate) fn assert_wire_exact<T>(json: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    use serde_json::Value;
    fn same(a: &Value, b: &Value, path: &str) {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                assert_eq!(x.as_f64(), y.as_f64(), "number at {path}")
            }
            (Value::Array(x), Value::Array(y)) => {
                assert_eq!(x.len(), y.len(), "array length at {path}");
                for (i, (x, y)) in x.iter().zip(y).enumerate() {
                    same(x, y, &format!("{path}[{i}]"));
                }
            }
            (Value::Object(x), Value::Object(y)) => {
                let mut kx: Vec<_> = x.keys().collect();
                let mut ky: Vec<_> = y.keys().collect();
                kx.sort();
                ky.sort();
                assert_eq!(kx, ky, "fields at {path}");
                for (k, v) in x {
                    same(v, &y[k], &format!("{path}.{k}"));
                }
            }
            _ => assert_eq!(a, b, "value at {path}"),
        }
    }
    let sent: Value = serde_json::from_str(json).unwrap();
    let parsed: T = serde_json::from_str(json).unwrap();
    same(&sent, &serde_json::to_value(&parsed).unwrap(), "$");
}
