//! The Properties panel's rows that are not object fields (props.js): a reference's attributes and library fields, an
//! attribute definition's tag / prompt / default and modes, a viewport's scale, the open sheet's name / paper /
//! orientation / delete, and a point picked for a point field (`_PROPPICK`). Each change is one undo step with 0.5's
//! label ("ATTRIBUTE", "PROPERTIES", "VIEWPORT", "RENAME SHEET", "SHEET SIZE", "DELETE SHEET"); the panel and the CLI
//! both call these, so they stay in step with the JS.

use cc_cad::entity::{Entity, Obj};
use cc_cad::{Doc, js};
use serde_json::Value;

use crate::lang::Lang;
use crate::props::{PCtx, defs_for};

/// libplace.js `LIB_TAGS` (the ones the panel reads).
pub const LIB_UUID: &str = "CC-ITEM-UUID";
pub const LIB_ITEM: &str = "CC-ITEM";
pub const LIB_QTY: &str = "CC-QTY";
pub const LIB_LICENSE: &str = "CC-LICENSE";
pub const LIB_ATTRIB: &str = "CC-ATTRIB";

/// One attribute row as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrField {
    pub tag: String,
    pub value: String,
    pub invisible: bool,
    pub constant: bool,
    /// shown, not editable (a constant one; a library item's fields other than its number, quantity and material)
    pub locked: bool,
}

/// props.js `attributeRows(e)`: the reference's attributes, and (a library item) its licence and credit.
pub fn attribute_rows(doc: &Doc, e: &Entity) -> (Vec<AttrField>, Option<(String, String)>) {
    let cc_cad::entity::Geo::Insert(ins) = &e.geo else { return (Vec::new(), None) };
    let rows = crate::cmd::inquiry::attr_rows(e, doc.blocks.get(&ins.block));
    if rows.is_empty() {
        return (Vec::new(), None);
    }
    let attr = |k: &str| -> Option<String> { ins.attrs.as_ref().and_then(|a| a.get(k)).map(js::to_js_string) };
    let is_lib = attr(LIB_UUID).is_some_and(|s| !s.is_empty());
    let out = rows
        .into_iter()
        .map(|a| {
            let locked = a.constant || (is_lib && a.tag != LIB_ITEM && a.tag != LIB_QTY && a.tag != "CC-PART-MATERIAL");
            AttrField { tag: a.tag, value: a.value, invisible: a.invisible, constant: a.constant, locked }
        })
        .collect();
    let lib = is_lib.then(|| (attr(LIB_LICENSE).unwrap_or_default(), attr(LIB_ATTRIB).unwrap_or_default()));
    (out, lib)
}

fn upd(doc: &mut Doc, id: u64, label: &str, f: impl FnOnce(&mut Obj)) -> Result<bool, String> {
    let Some(cur) = doc.get(id).cloned() else { return Ok(false) };
    if !doc.editable(&cur) {
        return Ok(false);
    }
    let mut o = cur.to_obj();
    f(&mut o);
    let e = Entity::from_value(&Value::Object(o));
    doc.change(label, |d| d.update(id, e)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// A reference's attribute typed into the panel (one undo step "ATTRIBUTE") → the status line ("TAG = value"), or
/// None when nothing was changed (the object is gone or on a locked or hidden layer, or the row is not editable).
pub fn set_attribute(doc: &mut Doc, id: u64, tag: &str, value: &str) -> Result<Option<String>, String> {
    let Some(e) = doc.get(id).cloned() else { return Ok(None) };
    let (rows, _) = attribute_rows(doc, &e);
    if !rows.iter().any(|r| r.tag == tag && !r.locked) {
        return Err(format!("props.attr: {tag} is not an attribute that can be changed here"));
    }
    let v = crate::cmd::block::one_line(value);
    let vv = v.clone();
    let done = upd(doc, id, "ATTRIBUTE", |o| {
        let mut a = o.get("attrs").and_then(Value::as_object).cloned().unwrap_or_default();
        a.insert(tag.to_string(), Value::String(vv));
        o.insert("attrs".into(), Value::Object(a));
    })?;
    Ok(done.then(|| format!("{tag} = {v}")))
}

/// An attribute definition's row (props.js extrasFor, isAttdefText): `tag` (upper case, spaces → "_", the text
/// follows), `prompt`, `def`, or a mode `invisible` / `constant` / `preset` ("true" / "false"). One undo step
/// "PROPERTIES"; an empty tag changes nothing.
pub fn set_attdef(doc: &mut Doc, id: u64, key: &str, value: &str) -> Result<bool, String> {
    let Some(e) = doc.get(id).cloned() else { return Ok(false) };
    if !crate::cmd::block::is_attdef_text(&e) {
        return Err("props.attdef: the object is not an attribute definition".into());
    }
    let v: Value = match key {
        "invisible" | "constant" | "preset" => Value::Bool(matches!(value, "true" | "1" | "on" | "yes")),
        "tag" => {
            let t = value.trim().to_uppercase().split_whitespace().collect::<Vec<_>>().join("_");
            // `/\s+/g` on the trimmed text: runs of white space inside become one "_"
            if t.is_empty() {
                return Ok(false);
            }
            Value::String(t)
        }
        "prompt" | "def" => Value::String(value.to_string()),
        _ => return Err(format!("props.attdef: no field {key}")),
    };
    upd(doc, id, "PROPERTIES", |o| {
        let mut a = o.get("attdef").and_then(Value::as_object).cloned().unwrap_or_default();
        a.insert(key.to_string(), v.clone());
        o.insert("attdef".into(), Value::Object(a));
        if key == "tag" {
            o.insert("s".into(), v);
        }
    })
}

/// A viewport's scale from the panel: `n` (1:n), "fit" (the model's extents exactly) or "showall" (the largest
/// standard scale that shows them). One undo step "VIEWPORT"; a display-locked viewport is left alone.
pub fn set_viewport_scale(doc: &mut Doc, id: u64, choice: &str) -> Result<bool, String> {
    let Some(unit) = doc.layout().and_then(|l| l.unit()).map(str::to_string) else {
        return Err("props.vp: open a sheet first".into());
    };
    let Some(e) = doc.get(id).cloned() else { return Ok(false) };
    let cc_cad::entity::Geo::Viewport(vp) = &e.geo else { return Err("props.vp: the object is not a viewport".into()) };
    if js::truthy(e.get("locked")) {
        return Ok(false);
    }
    let (w, h) = (vp.w, vp.h);
    let patch: Vec<(&str, f64)> = match choice {
        "fit" | "showall" => {
            let ext = crate::cmd::layout::model_extents(doc);
            let (cx, cy, s) = if choice == "fit" {
                crate::cmd::layout::aim_viewport_exact(doc, &unit, w, h, &ext)
            } else {
                crate::cmd::layout::aim_viewport(doc, &unit, w, h, &ext)
            };
            vec![("cx", cx), ("cy", cy), ("s", s)]
        }
        n => {
            let n =
                n.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0).ok_or("props.vp: scale = n (1:n), fit or showall")?;
            vec![("s", cc_io_cad::plot::s_of(doc, &unit, n))]
        }
    };
    upd(doc, id, "VIEWPORT", |o| {
        for (k, v) in patch {
            o.insert(k.into(), cc_cad::entity::num(v));
        }
    })
}

/// The open sheet renamed (props.js sheetSettings): an empty name, "Model" or another sheet's name is refused with
/// 0.5's status line. One undo step "RENAME SHEET".
pub fn rename_sheet(doc: &mut Doc, name: &str, lang: &Lang) -> Result<bool, String> {
    let space = doc.space;
    let Some(cur) = doc.layout().cloned() else { return Err("props.sheet: open a sheet first".into()) };
    let v = name.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("model") || doc.layouts.iter().any(|l| l.space != space && l.name == v) {
        return Err(lang.s("props.sheet.badname"));
    }
    if v == cur.name {
        // `name.value` unchanged: the input's change event does not fire
        return Ok(false);
    }
    let list: Vec<cc_cad::doc::Layout> = doc
        .layouts
        .iter()
        .map(|l| if l.space == space { cc_cad::doc::Layout { name: v.to_string(), ..l.clone() } } else { l.clone() })
        .collect();
    doc.change("RENAME SHEET", |d| d.set_layouts(list)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// The open sheet's paper (`key`; "" keeps a custom size and only turns it) and orientation. One undo step
/// "SHEET SIZE" → the status line when the paper's unit changed.
pub fn set_sheet_paper(doc: &mut Doc, key: &str, portrait: bool, lang: &Lang) -> Result<Option<String>, String> {
    let space = doc.space;
    let Some(cur) = doc.layout().cloned() else { return Err("props.sheet: open a sheet first".into()) };
    let p = &cur.paper;
    let num = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let key = if key.is_empty() { p.get("key").and_then(Value::as_str).unwrap_or("").to_string() } else { key.to_string() };
    let mut msg = None;
    let paper: Obj = if key.is_empty() {
        let (w, h) = (num("w"), num("h"));
        if portrait == (h > w) {
            return Ok(None);
        }
        let mut np = p.clone();
        np.insert("w".into(), cc_cad::entity::num(h));
        np.insert("h".into(), cc_cad::entity::num(w));
        np
    } else {
        let mut np = crate::cmd::layout::paper_obj(&key, portrait);
        for k in ["x0", "y0"] {
            // `{ ...np, x0: lay.paper.x0 }`: an absent one stays absent
            match p.get(k) {
                Some(v) => {
                    np.insert(k.into(), v.clone());
                }
                None => {
                    np.shift_remove(k);
                }
            }
        }
        let unit = np.get("unit").and_then(Value::as_str).unwrap_or("").to_string();
        if Some(unit.as_str()) != cur.unit() {
            let u = lang.s(if unit == "in" { "props.unit.in" } else { "props.unit.mm" });
            msg = Some(lang.t("props.sheet.newunit", &[("unit", u)]));
        }
        np
    };
    let list: Vec<cc_cad::doc::Layout> = doc
        .layouts
        .iter()
        .map(|l| if l.space == space { cc_cad::doc::Layout { paper: paper.clone(), ..l.clone() } } else { l.clone() })
        .collect();
    doc.change("SHEET SIZE", |d| d.set_layouts(list)).map_err(|e| e.to_string())?;
    Ok(msg)
}

/// The open sheet deleted (no question: one undo step "DELETE SHEET") → the status line.
pub fn delete_sheet(doc: &mut Doc, lang: &Lang) -> Result<String, String> {
    let space = doc.space;
    let Some(cur) = doc.layout().cloned() else { return Err("props.sheet: open a sheet first".into()) };
    let list: Vec<cc_cad::doc::Layout> = doc.layouts.iter().filter(|l| l.space != space).cloned().collect();
    doc.change("DELETE SHEET", |d| d.set_layouts(list)).map_err(|e| e.to_string())?;
    Ok(lang.t("props.sheet.deleted", &[("name", cur.name)]))
}

/// props.js `picked(p)`: the point field `pt` (start, end, center, …) of the group's objects moved to a picked point —
/// every axis of that point in ONE undo step "PROPERTIES"; → the first refusal, if any.
pub fn pick_point(doc: &mut Doc, group: &str, ids: &[u64], pt: &str, p: (f64, f64), vertex: i64, lang: &Lang) -> Option<String> {
    let ents: Vec<Entity> = ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    if ents.is_empty() {
        return None;
    }
    let axes: Vec<(String, &'static str)> = {
        let c = PCtx { doc, ents: &ents, vertex, lang };
        defs_for(group, &c).into_iter().filter_map(|d| d.pt.filter(|(n, _)| *n == pt).map(|(_, a)| (d.key.clone(), a))).collect()
    };
    let mut err = None;
    let _ = doc.change("PROPERTIES", |d| {
        for (key, axis) in &axes {
            let v = match *axis {
                "x" => p.0,
                "y" => p.1,
                _ => continue,
            };
            let r = crate::props::apply_value(d, key, group, ids, &cc_cad::entity::num(v), vertex, lang);
            if let Some(e) = r.error {
                err = Some(e);
            }
        }
        Ok(())
    });
    err
}

/// Nothing selected: one of "what new objects get" (props.js renderNone) — `layer` (made current: on, thawed, one
/// undo step "LAYER"), `color` / `lt` / `lw` / `th` (text height) for new objects, `lunits` / `luprec` (the length
/// format, UNITS), `ltscale` ("LTSCALE"), `laylt` / `laylw` (the current layer's linetype / lineweight, "LAYER"), and
/// new dimensions' `dim.txt` / `dim.asz` / `dim.dec` / `dim.scale`. Numbers come read (props.js `read`). → the status
/// line, if any.
pub fn set_current(doc: &mut Doc, key: &str, v: &Value, lang: &Lang) -> Result<Option<String>, String> {
    let f = || v.as_f64().filter(|x| x.is_finite());
    let s = || match v {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        other => Some(js::to_js_string(other)),
    };
    match key {
        "layer" => {
            let name = s().ok_or("props.current: layer = a name")?;
            doc.set_layer(&name, &Obj::new(), true).map_err(|e| e.to_string())?;
            Ok(Some(lang.t("app.layer.current_status", &[("name", name)])))
        }
        "color" => {
            doc.current.color = cc_cad::entity::Color::from_value(v).unwrap_or(cc_cad::entity::Color::Null);
            Ok(None)
        }
        "lt" => {
            doc.current.linetype = s();
            Ok(None)
        }
        "lw" => {
            doc.current.lineweight = f();
            Ok(None)
        }
        "th" => {
            doc.current.text_height = f().filter(|x| *x > 0.0).ok_or("props.current: th = a height > 0")?;
            Ok(None)
        }
        "lunits" | "luprec" => {
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            let n = f().ok_or("props.current: a number")?;
            let (c, p) = if key == "lunits" { (n, f64::from(prec)) } else { (f64::from(code), n) };
            let (c, p) = crate::cmd::units::set_length_format(doc, c, p).map_err(|e| e.to_string())?;
            let shown = cc_cad_geom::units::format_length(150.5, f64::from(c), Some(&Value::from(p)));
            Ok(Some(format!("{}: {shown}", crate::cmd::units::length_format_label(lang, c, p))))
        }
        "ltscale" => {
            let n = f().filter(|x| *x > 0.0).ok_or("props.current: ltscale > 0")?;
            let mut m = Obj::new();
            m.insert("ltscale".into(), cc_cad::entity::num(n));
            doc.change("LTSCALE", |d| d.set_meta(&m)).map_err(|e| e.to_string())?;
            Ok(None)
        }
        "laylt" | "laylw" => {
            let cur = doc.current.layer.clone();
            let mut m = Obj::new();
            if key == "laylt" {
                m.insert("linetype".into(), Value::String(s().unwrap_or_else(|| "Continuous".into())));
            } else {
                m.insert("lineweight".into(), f().map_or(Value::Null, cc_cad::entity::num));
            }
            doc.set_layer(&cur, &m, false).map_err(|e| e.to_string())?;
            Ok(None)
        }
        k if k.starts_with("dim.") => {
            let dk = k.trim_start_matches("dim.");
            if !matches!(dk, "txt" | "asz" | "dec" | "scale") {
                return Err(format!("props.current: no field {k}"));
            }
            let n = f().ok_or("props.current: a number")?;
            let n = if dk == "dec" { js::min(8.0, js::round(n)) } else { n };
            doc.current.dim.insert(dk.into(), cc_cad::entity::num(n));
            let lunit = doc.current.dim.get("lunit").and_then(Value::as_f64).unwrap_or(2.0);
            let what = match dk {
                "txt" => lang.s("props.dim.txt"),
                "asz" => lang.s("props.dim.asz"),
                "dec" if lunit == 4.0 || lunit == 5.0 => lang.s("props.dim.frac"),
                "dec" => lang.s("props.dim.dec"),
                _ => lang.s("props.dim.scale"),
            };
            Ok(Some(lang.t("props.cur.dimset", &[("what", what), ("v", crate::input::fmt(n))])))
        }
        _ => Err(format!("props.current: no field {key}")),
    }
}
