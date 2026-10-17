//! addons.js `cleanAddonData` (the only part of the add-on loader the markup model uses): add-ons' own data on a
//! markup, `{ <add-on id>: { kind, v, data } }`, checked like any file data.

use crate::columns::js_entries;
use crate::json::stringify;
use serde_json::{Map, Value};

/// `/^[a-z][a-z0-9.-]{1,63}$/` and not `/\.\.|--|[.-]$/`.
fn id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    let re = (2..=64).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'.' || *c == b'-');
    re && !id.contains("..") && !id.contains("--") && !id.ends_with('.') && !id.ends_with('-')
}

/// `/^([a-z][a-z0-9-]{0,40})?$/`
fn kind_ok(k: &str) -> bool {
    let b = k.as_bytes();
    b.is_empty()
        || (b.len() <= 41
            && b.first().is_some_and(u8::is_ascii_lowercase)
            && b.iter().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'))
}

/// `cleanAddonData(v)` → the cleaned object, or None (`undefined`).
pub fn clean_addon_data(v: Option<&Value>) -> Option<Value> {
    let Some(Value::Object(o)) = v else { return None };
    let mut out = Map::new();
    for (id, x) in js_entries(o).into_iter().take(20) {
        if !id_ok(id) {
            continue;
        }
        let Some(xo) = x.as_object() else { continue };
        let kind = match xo.get("kind") {
            Some(Value::String(k)) if kind_ok(k) => Some(k.clone()),
            _ => None,
        };
        let ver = xo.get("v").and_then(Value::as_f64).filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= 9999.0);
        let json = stringify(xo.get("data").unwrap_or(&Value::Null));
        let (Some(kind), Some(ver)) = (kind, ver) else { continue };
        if crate::js::len16(&json) > 12000 {
            continue;
        }
        let data: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let mut e = Map::new();
        e.insert("kind".into(), Value::String(kind));
        e.insert("v".into(), serde_json::Number::from_f64(ver).map_or(Value::Null, Value::Number));
        e.insert("data".into(), data);
        out.insert(id.clone(), Value::Object(e));
    }
    if out.is_empty() { None } else { Some(Value::Object(out)) }
}
