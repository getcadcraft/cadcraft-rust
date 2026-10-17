//! chest.js — the tool chest: styled markups saved as reusable tools, traveling as a JSON file
//! (`{format: 'markupcraft-tool-chest', version: 1, tools: [...]}`). Imports are untrusted: every field is checked
//! and clamped, unknown fields dropped. Tools are kept as JSON objects (the shape the file and the browser store hold).

use crate::i18n::t0;
use crate::js::{is_ctl_keep_ws, is_hex_color, lower, radix36, slice16, str_or_empty, to_iso, to_number, trim};
use crate::model::{Env, STAMPS, is_type, stamp_label, type_name};
use serde_json::{Map, Value};

pub const CHEST_FORMAT: &str = "markupcraft-tool-chest";
pub const MAX_TOOLS: usize = 500;

fn tool_type(t: &str) -> bool {
    is_type(t) && t != "foreign" && t != "image"
}

/// chest.js `str(v, max)`: control characters (not tab / LF / CR) dropped, ≤ max UTF-16 units.
fn cstr(v: Option<&Value>, max: usize) -> String {
    let s: String = str_or_empty(v).chars().filter(|c| !is_ctl_keep_ws(*c)).collect();
    slice16(&s, max)
}

/// chest.js `num(v, lo, hi, d)`.
fn cnum(v: Option<&Value>, lo: f64, hi: f64, d: f64) -> f64 {
    let n = v.map_or(f64::NAN, to_number);
    let ok = n.is_finite() && !matches!(v, Some(Value::Null)) && v.and_then(Value::as_str) != Some("");
    if ok { crate::js::max(lo, crate::js::min(hi, n)) } else { d }
}

fn num_v(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// `/re/.test(v)` stringifies v; the six default ids and plain strings are what pass in practice.
fn id_test(v: Option<&Value>) -> bool {
    let s = match v {
        None => "undefined".to_string(),
        Some(x) => crate::js::to_js_string(x),
    };
    crate::js::is_id(&s, 60)
}

/// `cleanTool(t, i)` → the clean tool, or None.
pub fn clean_tool(env: &Env, t: &Value, i: usize) -> Option<Value> {
    let o = t.as_object()?;
    let ty = o.get("type").and_then(Value::as_str).filter(|s| tool_type(s))?.to_string();
    let empty = Map::new();
    let s = o.get("style").and_then(Value::as_object).unwrap_or(&empty);
    let mut tool = Map::new();
    if id_test(o.get("id")) {
        // `t.id` as it is: undefined stays undefined (the key is then absent from the JSON)
        if let Some(v) = o.get("id") {
            tool.insert("id".into(), v.clone());
        }
    } else {
        let now = crate::js::f2u(env.now_ms);
        tool.insert("id".into(), Value::String(format!("tool-{}-{i}", radix36(now))));
    }
    let name = trim(&cstr(o.get("name"), 80)).to_string();
    tool.insert("name".into(), Value::String(if name.is_empty() { type_name(&ty) } else { name }));
    tool.insert("type".into(), Value::String(ty.clone()));
    let mut st = Map::new();
    let color = match s.get("color") {
        Some(Value::String(c)) if is_hex_color(c) => lower(c),
        _ => "#d0021b".to_string(),
    };
    st.insert("color".into(), Value::String(color));
    st.insert("width".into(), num_v(cnum(s.get("width"), 0.0, 72.0, 2.0)));
    let fill = match s.get("fill") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(f)) if f.is_empty() => Value::Null,
        Some(Value::String(f)) if is_hex_color(f) => Value::String(lower(f)),
        _ => Value::Null,
    };
    st.insert("fill".into(), fill);
    st.insert("fillOpacity".into(), num_v(cnum(s.get("fillOpacity"), 0.0, 1.0, 0.25)));
    st.insert("opacity".into(), num_v(cnum(s.get("opacity"), 0.05, 1.0, 1.0)));
    st.insert("fontSize".into(), num_v(cnum(s.get("fontSize"), 4.0, 144.0, 12.0)));
    tool.insert("style".into(), Value::Object(st));
    tool.insert("subject".into(), Value::String(cstr(o.get("subject"), 120)));
    if ty == "stamp" {
        let l = trim(&cstr(o.get("label"), 60)).to_string();
        tool.insert("label".into(), Value::String(if l.is_empty() { stamp_label(STAMPS[0]) } else { l }));
    }
    if ty == "count" {
        let l = trim(&cstr(o.get("label"), 60)).to_string();
        tool.insert("label".into(), Value::String(if l.is_empty() { t0("count.default") } else { l }));
    }
    if ty == "text" || ty == "callout" || ty == "note" {
        tool.insert("text".into(), Value::String(cstr(o.get("text"), 2000)));
    }
    Some(Value::Object(tool))
}

/// DEFAULT_CHEST: the six starting tools, in the page's language.
pub fn default_chest() -> Vec<Value> {
    type Def<'a> = (&'a str, &'a str, &'a str, f64, f64, Option<&'a str>);
    let defs: [Def<'_>; 6] = [
        ("revision", "cloudrect", "#d0021b", 1.5, 1.0, None),
        ("question", "cloudrect", "#1e48c8", 1.5, 1.0, None),
        ("highlighter", "highlight", "#f5e000", 14.0, 0.45, None),
        ("rfi", "callout", "#1e48c8", 1.0, 1.0, Some("text")),
        ("verify", "callout", "#c25e00", 1.0, 1.0, Some("text")),
        ("approved", "stamp", "#1e8c3a", 2.5, 1.0, Some("label")),
    ];
    defs.iter()
        .enumerate()
        .map(|(i, (key, ty, color, width, opacity, extra))| {
            let k = format!("chest.default.{key}.");
            let fs = if *ty == "stamp" { 18.0 } else { 12.0 };
            let mut st = Map::new();
            st.insert("color".into(), Value::from(*color));
            st.insert("width".into(), num_v(*width));
            st.insert("fill".into(), Value::Null);
            st.insert("fillOpacity".into(), num_v(0.25));
            st.insert("opacity".into(), num_v(*opacity));
            st.insert("fontSize".into(), num_v(fs));
            let mut t = Map::new();
            t.insert("id".into(), Value::String(format!("default-{i}")));
            t.insert("type".into(), Value::from(*ty));
            t.insert("style".into(), Value::Object(st));
            t.insert("name".into(), Value::String(t0(&format!("{k}name"))));
            t.insert("subject".into(), Value::String(t0(&format!("{k}subject"))));
            match extra {
                Some("text") => {
                    t.insert("text".into(), Value::String(t0(&format!("{k}text"))));
                }
                Some(_) => {
                    t.insert("label".into(), Value::String(stamp_label("Approved as noted")));
                }
                None => {}
            }
            Value::Object(t)
        })
        .collect()
}

/// `toolFromMarkup(m, name)`.
pub fn tool_from_markup(env: &Env, m: &crate::model::Markup, name: &str) -> Option<Value> {
    let mut o = Map::new();
    o.insert("name".into(), Value::from(name));
    o.insert("type".into(), Value::from(m.kind.clone()));
    o.insert("style".into(), crate::json::markup_value(m).get("style").cloned().unwrap_or(Value::Null));
    o.insert("subject".into(), Value::from(m.subject.clone()));
    o.insert("label".into(), Value::from(m.label.clone()));
    o.insert("text".into(), Value::from(if m.kind == "stamp" { String::new() } else { m.text.clone() }));
    clean_tool(env, &Value::Object(o), 0)
}

/// Why a chest file was refused.
#[derive(Debug, Clone, PartialEq)]
pub struct ChestError(pub String);

/// `parseChest(text)` → (tools, skipped), or the plain-language error.
pub fn parse_chest(env: &Env, text: &str) -> Result<(Vec<Value>, usize), ChestError> {
    let d: Value = serde_json::from_str(text).map_err(|_| ChestError(t0("chest.err.json")))?;
    let list = match &d {
        Value::Array(a) => a,
        Value::Object(o) if o.get("format").and_then(Value::as_str) == Some(CHEST_FORMAT) => match o.get("tools") {
            Some(Value::Array(a)) => a,
            _ => return Err(ChestError(t0("chest.err.format"))),
        },
        _ => return Err(ChestError(t0("chest.err.format"))),
    };
    let mut tools = Vec::new();
    let mut skipped = 0usize;
    for (i, t) in list.iter().take(MAX_TOOLS).enumerate() {
        match clean_tool(env, t, i) {
            Some(c) => tools.push(c),
            None => skipped = skipped.saturating_add(1),
        }
    }
    skipped = skipped.saturating_add(list.len().saturating_sub(MAX_TOOLS));
    Ok((tools, skipped))
}

/// `x.id === t.id` (absent ids compare equal, as undefined === undefined).
fn same_id(a: &Value, b: &Value) -> bool {
    let (x, y) = (a.get("id"), b.get("id"));
    match (x, y) {
        (None, None) => true,
        (Some(Value::Number(p)), Some(Value::Number(q))) => p.as_f64() == q.as_f64(),
        (Some(p), Some(q)) => p == q && !p.is_object() && !p.is_array(),
        _ => false,
    }
}

/// `mergeChest(chest, incoming)`: same id replaces, new ones are added.
pub fn merge_chest(chest: &[Value], incoming: &[Value]) -> Vec<Value> {
    let mut out = chest.to_vec();
    for t in incoming {
        match out.iter().position(|x| same_id(x, t)) {
            Some(i) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = t.clone();
                }
            }
            None => out.push(t.clone()),
        }
    }
    out.truncate(MAX_TOOLS);
    out
}

/// `exportChest(tools)`: the file text (`JSON.stringify(…, null, 2)`).
pub fn export_chest(env: &Env, tools: &[Value]) -> String {
    let mut o = Map::new();
    o.insert("format".into(), Value::from(CHEST_FORMAT));
    o.insert("version".into(), num_v(1.0));
    o.insert("exported".into(), Value::String(to_iso(env.now_ms).unwrap_or_default()));
    o.insert("tools".into(), Value::Array(tools.iter().filter_map(|t| clean_tool(env, t, 0)).collect()));
    crate::json::stringify_pretty(&Value::Object(o))
}
