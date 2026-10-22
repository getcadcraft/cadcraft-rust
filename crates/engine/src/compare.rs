//! compare.js — compare settings (0.5 WL.12; COMPARESETTINGS): two drawings, or two styles in one drawing, side by
//! side — dimension styles, text styles, layers, sheets (page setup) and units — with what differs marked and a value
//! copied across in one undo step ("COMPARE") of the drawing it lands in.

use cc_cad::entity::Obj;
use cc_cad::{Doc, js};
use serde_json::{Map, Value, json};

/// `COMPARE_CATS`.
pub const CATS: [&str; 5] = ["dimstyle", "textstyle", "layer", "sheet", "units"];
const LAYER_KEYS: [&str; 9] = ["color", "linetype", "lineweight", "tr", "on", "frozen", "locked", "plot", "newVpFrozen"];
const TEXT_KEYS: [&str; 4] = ["font", "h", "wf", "obl"];
const UNIT_KEYS: [&str; 5] = ["units", "lunits", "luprec", "ltscale", "insbase"];
const SHEET_KEYS: [&str; 5] = ["paper", "size", "unit", "orientation", "origin"];

fn dim_keys() -> Vec<String> {
    let mut v: Vec<String> = cc_cad::doc::DIM_DEFAULTS.iter().map(|(k, _)| (*k).to_string()).collect();
    for k in cc_cad_geom::dimstyle::dim_extra().keys() {
        if !v.contains(k) {
            v.push(k.clone());
        }
    }
    v
}

fn keys_of(cat: &str) -> Vec<String> {
    let s = |a: &[&str]| a.iter().map(|k| (*k).to_string()).collect();
    match cat {
        "dimstyle" => dim_keys(),
        "textstyle" => s(&TEXT_KEYS),
        "layer" => s(&LAYER_KEYS),
        "sheet" => s(&SHEET_KEYS),
        "units" => s(&UNIT_KEYS),
        _ => Vec::new(),
    }
}

/// The ASCII punctuation and symbols in ICU's root collation order (they sort after white space, before digits).
const ICU_PUNCT: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// A Latin letter's base letter (accents dropped: "Ü" → 'u'), lower case.
fn base_letter(c: char) -> char {
    const FROM: &str = "àáâãäåāăąçćĉċčďđèéêëēĕėęěĝğġģĥħìíîïĩīĭįıĵķĺļľŀłñńņňòóôõöøōŏőŕŗřśŝşšţťŧùúûüũūŭůűųŵýÿŷźżž";
    const TO: &str = "aaaaaaaaacccccddeeeeeeeeegggghhiiiiiiiiijklllllnnnnooooooooorrrsssstttuuuuuuuuuuwyyyzzz";
    let l = c.to_lowercase().next().unwrap_or(c);
    FROM.chars().position(|x| x == l).and_then(|i| TO.chars().nth(i)).unwrap_or(l)
}

/// A rough `a.localeCompare(b, undefined, { numeric: true })`: white space, then punctuation and symbols (ICU's root
/// order), digit runs by value, then letters by their base letter; on a tie, unaccented before accented and lower
/// case before upper case. Spec gap: the full collation (other scripts, ligatures) is the browser's.
pub fn locale_compare_numeric(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn primary(c: char) -> (u8, u32) {
        if c.is_whitespace() {
            return (0, c as u32);
        }
        if let Some(i) = ICU_PUNCT.chars().position(|x| x == c) {
            return (1, u32::try_from(i).unwrap_or(0));
        }
        if !c.is_alphanumeric() {
            return (2, c as u32);
        }
        if c.is_numeric() {
            return (3, c as u32);
        }
        (4, base_letter(c) as u32)
    }
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    let mut accent = Ordering::Equal;
    let mut case = Ordering::Equal;
    while i < ac.len() && j < bc.len() {
        let (x, y) = (ac.get(i).copied().unwrap_or(' '), bc.get(j).copied().unwrap_or(' '));
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let si = i;
            while ac.get(i).is_some_and(char::is_ascii_digit) {
                i = i.saturating_add(1);
            }
            let sj = j;
            while bc.get(j).is_some_and(char::is_ascii_digit) {
                j = j.saturating_add(1);
            }
            let na: String = ac.get(si..i).unwrap_or(&[]).iter().collect();
            let nb: String = bc.get(sj..j).unwrap_or(&[]).iter().collect();
            let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if o != Ordering::Equal {
                return o;
            }
            continue;
        }
        let o = primary(x).cmp(&primary(y));
        if o != Ordering::Equal {
            return o;
        }
        let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
        if accent == Ordering::Equal && lx != ly {
            // the plain letter first
            accent = if lx == base_letter(lx) {
                Ordering::Less
            } else if ly == base_letter(ly) {
                Ordering::Greater
            } else {
                lx.cmp(&ly)
            };
        }
        if case == Ordering::Equal && x != y && lx == ly {
            case = if x.is_lowercase() { Ordering::Less } else { Ordering::Greater };
        }
        i = i.saturating_add(1);
        j = j.saturating_add(1);
    }
    (ac.len().saturating_sub(i)).cmp(&bc.len().saturating_sub(j)).then(accent).then(case)
}

/// `itemsOf(doc, cat)`.
pub fn items_of(doc: &Doc, cat: &str) -> Vec<String> {
    match cat {
        "dimstyle" => std::iter::once("*CURRENT".to_string()).chain(doc.dim_styles.keys().cloned()).collect(),
        "textstyle" => doc.text_styles.keys().cloned().collect(),
        "layer" => {
            let mut v: Vec<String> = doc.layers.keys().cloned().collect();
            v.sort_by(|a, b| match (a.as_str(), b.as_str()) {
                ("0", "0") => std::cmp::Ordering::Equal,
                ("0", _) => std::cmp::Ordering::Less,
                (_, "0") => std::cmp::Ordering::Greater,
                _ => locale_compare_numeric(a, b),
            });
            v
        }
        "sheet" => doc.layouts.iter().map(|l| l.name.clone()).collect(),
        "units" => vec!["*DRAWING".into()],
        _ => Vec::new(),
    }
}

/// One item's properties as { key: value } (absent = the item doesn't have it).
fn props_of(doc: &Doc, cat: &str, item: Option<&str>) -> Option<Map<String, Value>> {
    // units: the drawing's, whatever the item; the others look the item up
    let item = if cat == "units" { item.unwrap_or("") } else { item? };
    match cat {
        "dimstyle" => {
            let st = if item == "*CURRENT" { Some(&doc.current.dim) } else { doc.dim_styles.get(item) }?;
            let mut m = cc_cad::doc::dim_defaults();
            for (k, v) in cc_cad_geom::dimstyle::dim_extra() {
                m.insert(k, v);
            }
            for (k, v) in st {
                m.insert(k.clone(), v.clone());
            }
            Some(m)
        }
        "textstyle" => doc.text_styles.get(item).cloned(),
        "layer" => {
            let l = doc.layers.get(item)?.to_value();
            let mut m = Map::new();
            for k in LAYER_KEYS {
                let v = l.get(k).cloned();
                let v = if matches!(k, "newVpFrozen" | "frozen" | "locked") { Some(Value::Bool(js::truthy(v.as_ref()))) } else { v };
                if let Some(v) = v {
                    m.insert(k.into(), v);
                }
            }
            Some(m)
        }
        "sheet" => {
            let l = doc.layouts.iter().find(|x| x.name == item)?;
            let p = &l.paper;
            let n = |k: &str| p.get(k).cloned().unwrap_or(Value::Null);
            let or0 = |k: &str| match p.get(k) {
                Some(v) if js::truthy(Some(v)) => v.clone(),
                _ => json!(0),
            };
            let w = js::to_number_opt(p.get("w"));
            let h = js::to_number_opt(p.get("h"));
            let mut m = Map::new();
            m.insert(
                "paper".into(),
                match p.get("key") {
                    Some(v) if js::truthy(Some(v)) => v.clone(),
                    _ => json!(""),
                },
            );
            m.insert("size".into(), json!([n("w"), n("h")]));
            if let Some(u) = p.get("unit") {
                m.insert("unit".into(), u.clone());
            }
            m.insert("orientation".into(), json!(if w >= h { "landscape" } else { "portrait" }));
            m.insert("origin".into(), json!([or0("x0"), or0("y0")]));
            Some(m)
        }
        "units" => {
            let mt = &doc.meta;
            let nn = |k: &str, d: Value| match mt.get(k) {
                None | Some(Value::Null) => d,
                Some(v) => v.clone(),
            };
            let units = match mt.get("units") {
                Some(v) if js::truthy(Some(v)) => v.clone(),
                _ => json!(""),
            };
            let ib = mt.get("insbase").filter(|v| js::truthy(Some(v)));
            let insbase = match ib {
                Some(b) => {
                    let c = |k: &str, i: usize| match b.get(k).filter(|v| !v.is_null()) {
                        Some(v) => v.clone(),
                        None => match b.get(i).filter(|v| !v.is_null()) {
                            Some(v) => v.clone(),
                            None => json!(0),
                        },
                    };
                    json!([c("x", 0), c("y", 1)])
                }
                None => json!([0, 0]),
            };
            let mut m = Map::new();
            m.insert("units".into(), units);
            m.insert("lunits".into(), nn("lunits", json!(2)));
            m.insert("luprec".into(), nn("luprec", json!(4)));
            m.insert("ltscale".into(), nn("ltscale", json!(1)));
            m.insert("insbase".into(), insbase);
            Some(m)
        }
        _ => None,
    }
}

/// compare.js `same(a, b)`: numbers within 1e-9 relative, anything else as its JSON.
fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    if let (Some(Value::Number(x)), Some(Value::Number(y))) = (a, b) {
        let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
        return (x - y).abs() <= 1e-9 * js::max(1.0, js::max(x.abs(), y.abs()));
    }
    let j = |v: Option<&Value>| match v {
        None | Some(Value::Null) => "null".to_string(),
        Some(v) => v.to_string(),
    };
    j(a) == j(b)
}

/// One row of the comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub a: Option<Value>,
    pub b: Option<Value>,
    pub same: bool,
}

/// `compareItems(cat, docA, itemA, docB, itemB)`: every property either side has, in a fixed order.
pub fn compare_items(cat: &str, da: &Doc, a: Option<&str>, db: &Doc, b: Option<&str>) -> Vec<Row> {
    let pa = props_of(da, cat, a);
    let pb = props_of(db, cat, b);
    keys_of(cat)
        .into_iter()
        .filter(|k| pa.as_ref().is_some_and(|m| m.contains_key(k)) || pb.as_ref().is_some_and(|m| m.contains_key(k)))
        .map(|k| {
            let va = pa.as_ref().and_then(|m| m.get(&k)).cloned();
            let vb = pb.as_ref().and_then(|m| m.get(&k)).cloned();
            let s = pa.is_some() && pb.is_some() && same(va.as_ref(), vb.as_ref());
            Row { key: k, a: va, b: vb, same: s }
        })
        .collect()
}

/// `copyValue(doc, cat, item, key, value)`: one value onto an item of `doc` (a layer or style that isn't there is
/// made) in one undo step "COMPARE" → whether something changed.
pub fn copy_value(doc: &mut Doc, cat: &str, item: &str, key: &str, value: &Value) -> bool {
    if let Some(before) = props_of(doc, cat, Some(item))
        && same(before.get(key), Some(value))
    {
        return false;
    }
    let mut changed = false;
    let _ = doc.change("COMPARE", |d| {
        match cat {
            "dimstyle" => {
                if item == "*CURRENT" {
                    // the settings for new dimensions: not an edit of the drawing
                    d.current.dim.insert(key.into(), value.clone());
                    changed = true;
                    return Ok(());
                }
                let mut st = d.dim_styles.get(item).cloned().unwrap_or_default();
                st.insert(key.into(), value.clone());
                d.set_style(cc_cad::doc::StyleTable::DimStyles, item, Some(&Value::Object(st)))?;
                changed = true;
            }
            "textstyle" => {
                let mut st: Obj = [("font", json!("")), ("h", json!(0)), ("wf", json!(1)), ("obl", json!(0))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect();
                for (k, v) in d.text_styles.get(item).cloned().unwrap_or_default() {
                    st.insert(k, v);
                }
                st.insert(key.into(), value.clone());
                d.set_style(cc_cad::doc::StyleTable::TextStyles, item, Some(&Value::Object(st)))?;
                changed = true;
            }
            "layer" => {
                let mut state = d.layer_state();
                let pos = match state.list.iter().position(|l| l.name == item) {
                    Some(p) => p,
                    None => {
                        state.list.push(cc_cad::doc::Layer::from_state_value(&json!({
                            "name": item, "color": 7, "on": true, "frozen": false, "locked": false, "plot": true, "linetype": "Continuous", "lineweight": -3
                        })));
                        state.list.len().saturating_sub(1)
                    }
                };
                if let Some(l) = state.list.get_mut(pos) {
                    let mut o = l.to_value().as_object().cloned().unwrap_or_default();
                    if key == "newVpFrozen" {
                        if js::truthy(Some(value)) {
                            o.insert(key.into(), Value::Bool(true));
                        } else {
                            o.shift_remove(key);
                        }
                    } else {
                        o.insert(key.into(), value.clone());
                    }
                    *l = cc_cad::doc::Layer::from_state_value(&Value::Object(o));
                }
                d.set_layer_state(&state)?;
                changed = true;
            }
            "sheet" => {
                let Some(at) = d.layouts.iter().position(|l| l.name == item) else { return Ok(()) };
                let mut list = d.layouts.clone();
                let Some(lay) = list.get_mut(at) else { return Ok(()) };
                let p = &mut lay.paper;
                let arr = value.as_array();
                match key {
                    "paper" => {
                        p.insert("key".into(), value.clone());
                    }
                    "size" if arr.is_some() => {
                        p.insert("w".into(), arr.and_then(|a| a.first()).cloned().unwrap_or(Value::Null));
                        p.insert("h".into(), arr.and_then(|a| a.get(1)).cloned().unwrap_or(Value::Null));
                    }
                    "unit" => {
                        p.insert("unit".into(), value.clone());
                    }
                    "orientation" => {
                        let (w, h) = (js::to_number_opt(p.get("w")), js::to_number_opt(p.get("h")));
                        if (value.as_str() == Some("landscape")) != (w >= h) {
                            let (vw, vh) = (p.get("w").cloned().unwrap_or(Value::Null), p.get("h").cloned().unwrap_or(Value::Null));
                            p.insert("w".into(), vh);
                            p.insert("h".into(), vw);
                        }
                    }
                    "origin" if arr.is_some() => {
                        p.insert("x0".into(), arr.and_then(|a| a.first()).cloned().unwrap_or(Value::Null));
                        p.insert("y0".into(), arr.and_then(|a| a.get(1)).cloned().unwrap_or(Value::Null));
                    }
                    _ => {}
                }
                d.set_layouts(list)?;
                changed = true;
            }
            "units" => {
                let mut m = Obj::new();
                let v = match (key, value.as_array()) {
                    ("insbase", Some(a)) => {
                        json!({ "x": a.first().cloned().unwrap_or(Value::Null), "y": a.get(1).cloned().unwrap_or(Value::Null) })
                    }
                    _ => value.clone(),
                };
                m.insert(key.into(), v);
                d.set_meta(&m)?;
                changed = true;
            }
            _ => {}
        }
        Ok(())
    });
    changed
}

/// `valueText(cat, key, v)`: a short text for a value (numbers rounded, ✓ / ✗, sizes "w × h").
pub fn value_text(cat: &str, key: &str, v: Option<&Value>) -> String {
    let r = |x: f64, d: usize| js::num_to_string(js::str_to_number(&js::to_fixed(x, d)));
    match v {
        None | Some(Value::Null) => "—".into(),
        Some(Value::String(s)) if s.is_empty() => "—".into(),
        Some(Value::Bool(b)) => if *b { "✓" } else { "✗" }.into(),
        Some(Value::Number(n)) => r(n.as_f64().unwrap_or(f64::NAN), 6),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Number(n) => r(n.as_f64().unwrap_or(f64::NAN), 4),
                // Array.join: null (and undefined) as ""
                Value::Null => String::new(),
                other => js::to_js_string(other),
            })
            .collect::<Vec<_>>()
            .join(if cat == "sheet" && key == "size" { " × " } else { ", " }),
        Some(other) => js::to_js_string(other),
    }
}
