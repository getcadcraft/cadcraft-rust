//! `clipboard.js` — copy, cut, paste and duplicate for marks and items. A clip read back (`readClip`) is untrusted:
//! it is wrapped in a one-sheet project and run through `readProject`, so it gets exactly the same checks.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::i18n::{F, t, tf};
use crate::jv::{self, Obj, n, s};
use crate::model::{
    self, Ch, ExtrasCtx, MARK_EXTRAS, Res, Takeoff, arr_of, kinds_of, limits, mark_extras, normal_item, read_extras, read_extras_by,
};

pub const CLIP_FORMAT: &str = "takeoffcraft-clip";
pub const CLIP_VERSION: f64 = 1.0;
pub const CLIP_MARKS: usize = 20000;
pub const CLIP_ITEMS: usize = 500;
const SCALE_TOL: f64 = 0.001;
const SIZE_TOL: f64 = 1.0;

fn zero_hash() -> String {
    "0".repeat(64)
}
fn clip_sheet() -> String {
    format!("{}:0", zero_hash())
}

/// Middle of the marks' points (bounding box).
pub fn anchor_of(marks: &[Value]) -> Value {
    anchor_of_refs(marks.iter())
}

/// [`anchor_of`] over borrowed marks.
fn anchor_of_refs<'a>(marks: impl Iterator<Item = &'a Value>) -> Value {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for m in marks {
        let mut rings: Vec<&Value> = Vec::new();
        if let Some(p) = m.get("pts") {
            rings.push(p);
        }
        for h in jv::arr(m.get("holes")).map_or(&[][..], Vec::as_slice) {
            rings.push(h);
        }
        for ring in rings {
            for p in jv::arr(Some(ring)).map_or(&[][..], Vec::as_slice) {
                let (x, y) = match p {
                    Value::Array(a) => (jv::num(a.first()), jv::num(a.get(1))),
                    _ => (jv::num(p.get("x")), jv::num(p.get("y"))),
                };
                if let Some(x) = x {
                    if x < x0 {
                        x0 = x;
                    }
                    if x > x1 {
                        x1 = x;
                    }
                }
                if let Some(y) = y {
                    if y < y0 {
                        y0 = y;
                    }
                    if y > y1 {
                        y1 = y;
                    }
                }
            }
        }
    }
    if x0.is_finite() { json!({ "x": n((x0 + x1) / 2.0), "y": n((y0 + y1) / 2.0) }) } else { json!({ "x": 0, "y": 0 }) }
}

/// The sheet a copy came from: `{ view: [x0, y0, x1, y1], userUnit }`, plus source and time.
#[derive(Debug, Clone, Default)]
pub struct CopyCtx {
    pub view: Option<Value>,
    pub user_unit: Option<Value>,
    pub source: String,
    pub now: f64,
}

fn sheet_info(state: &Value, sheet: &str, ctx: &CopyCtx) -> Value {
    let sc = state.get("sheets").and_then(|x| x.get(sheet)).and_then(|x| x.get("scale")).filter(|x| jv::truthy(Some(x)));
    let fpu = sc.and_then(|s| jv::to_number(s.get("ftPerUnit")).gt(&0.0).then(|| s.get("ftPerUnit").cloned()).flatten());
    let label = sc.and_then(|s| s.get("label")).filter(|l| jv::truthy(Some(l))).cloned().unwrap_or(s(""));
    let view = match &ctx.view {
        Some(Value::Array(a)) if a.len() == 4 => Value::Array(a.clone()),
        _ => Value::Null,
    };
    let uu = jv::finite(ctx.user_unit.as_ref()).filter(|u| *u > 0.0).unwrap_or(1.0);
    json!({ "ftPerUnit": fpu.unwrap_or(Value::Null), "label": label, "view": view, "userUnit": n(uu) })
}

/// Copy marks (and the items they belong to); only marks on the first mark's sheet.
pub fn copy_marks(state: &Value, ids: &[String], ctx: &CopyCtx) -> Option<Value> {
    let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let all = jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice);
    let marks: Vec<&Value> = all.iter().filter(|m| jv::str(m.get("id")).is_some_and(|i| want.contains(i))).collect();
    let first = marks.first()?;
    let sheet = first.get("sheet").cloned().unwrap_or(Value::Null);
    let marks: Vec<&Value> = marks.into_iter().filter(|m| jv::strict_eq(m.get("sheet"), Some(&sheet))).collect();
    let mut item_ids: Vec<String> = Vec::new();
    for m in &marks {
        let i = jv::to_string(m.get("item"));
        if !item_ids.contains(&i) {
            item_ids.push(i);
        }
    }
    let items_all = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let items: Vec<Value> =
        item_ids.iter().filter_map(|id| items_all.iter().find(|i| jv::str(i.get("id")) == Some(id.as_str())).cloned()).collect();
    let out_marks: Vec<Value> = marks
        .iter()
        .map(|m| {
            let mut o = Obj::new();
            o.insert("item".into(), m.get("item").cloned().unwrap_or(Value::Null));
            o.insert("kind".into(), m.get("kind").cloned().unwrap_or(Value::Null));
            o.insert("pts".into(), Value::Array(jv::arr(m.get("pts")).map(|p| p.iter().map(arr_of).collect()).unwrap_or_default()));
            if let Some(h) = jv::arr(m.get("holes")).filter(|h| !h.is_empty()) {
                o.insert(
                    "holes".into(),
                    Value::Array(
                        h.iter()
                            .map(|r| Value::Array(jv::arr(Some(r)).map(|p| p.iter().map(arr_of).collect()).unwrap_or_default()))
                            .collect(),
                    ),
                );
            }
            jv::assign(&mut o, &mark_extras(m));
            Value::Object(o)
        })
        .collect();
    // json! would copy the lists through serde (`to_value(&list)`): they move in instead
    let mut out = json!({
        "format": CLIP_FORMAT, "version": n(CLIP_VERSION), "source": ctx.source, "copiedAt": n(ctx.now),
        "sheet": sheet_info(state, &jv::to_string(Some(&sheet)), ctx),
        "anchor": anchor_of_refs(marks.iter().copied()),
    });
    if let Value::Object(o) = &mut out {
        o.insert("items".into(), Value::Array(items));
        o.insert("marks".into(), Value::Array(out_marks));
    }
    Some(out)
}

/// Copy items only (with their assemblies).
pub fn copy_items(state: &Value, item_ids: &[String], ctx: &CopyCtx) -> Option<Value> {
    let all = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let items: Vec<Value> =
        item_ids.iter().filter_map(|id| all.iter().find(|i| jv::str(i.get("id")) == Some(id.as_str())).cloned()).collect();
    if items.is_empty() {
        return None;
    }
    Some(json!({
        "format": CLIP_FORMAT, "version": n(CLIP_VERSION), "source": ctx.source, "copiedAt": n(ctx.now),
        "sheet": null, "anchor": { "x": 0, "y": 0 }, "items": items, "marks": [],
    }))
}

/// A read clip and its warnings.
#[derive(Debug, Clone)]
pub struct ReadClip {
    pub clip: Value,
    pub warnings: Vec<String>,
}

/// Validate a clip (from storage, i.e. anything).
pub fn read_clip(json: &Value) -> Result<ReadClip, String> {
    let j = clip_head(json)?;
    let raw_items = jv::arr(j.get("items")).cloned().unwrap_or_default();
    let raw_marks = jv::arr(j.get("marks")).map_or(&[][..], Vec::as_slice);
    if raw_items.len() > CLIP_ITEMS || raw_marks.len() > CLIP_MARKS {
        return Err(t("clip.err.many"));
    }
    let marks_in: Vec<Value> = raw_marks.iter().enumerate().map(|(i, m)| clip_mark_in(i, m, |k| jv::field(m, k).cloned())).collect();
    clip_tail(j, raw_items, marks_in, !raw_marks.is_empty())
}

/// [`read_clip`] of a clip that is given away (a copy just made): its lists and fields move instead of being copied.
pub fn read_clip_owned(mut json: Value) -> Result<ReadClip, String> {
    clip_head(&json)?;
    let mut take = |k: &str| match json.get_mut(k).map(Value::take) {
        Some(Value::Array(a)) => Some(a),
        Some(other) => {
            // not a list: put it back as it was (read as no list)
            if let Some(slot) = json.get_mut(k) {
                *slot = other;
            }
            None
        }
        None => None,
    };
    let raw_items = take("items").unwrap_or_default();
    let mut raw_marks = take("marks").unwrap_or_default();
    if raw_items.len() > CLIP_ITEMS || raw_marks.len() > CLIP_MARKS {
        return Err(t("clip.err.many"));
    }
    let any = !raw_marks.is_empty();
    let marks_in: Vec<Value> = raw_marks
        .iter_mut()
        .enumerate()
        .map(|(i, m)| {
            let is = jv::is_objlike(Some(m));
            let mut cell = std::mem::take(m);
            if !is {
                return Value::Null;
            }
            clip_mark_in(i, &Value::Null, |k| cell.get_mut(k).map(Value::take))
        })
        .collect();
    let Value::Object(j) = &json else { return Err(t("clip.err.format")) };
    clip_tail(j, raw_items, marks_in, any)
}

/// `readClip(copyMarks(state, ids, ctx))` in one pass (Copy, Cut, Duplicate): the takeoff's own marks go into the clip
/// as they are — their points are already clean — instead of out to the clip's lists and back. Marks or points that are
/// not in the shape the takeoff keeps them take the two steps, so the result is always theirs (test
/// `copy_read_is_copy_then_read`). None when nothing is selected.
pub fn copy_read(state: &Value, ids: &[String], ctx: &CopyCtx) -> Option<Result<ReadClip, String>> {
    let slow = || copy_marks(state, ids, ctx).map(read_clip_fresh);
    let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let all = jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice);
    let marks: Vec<&Value> = all.iter().filter(|m| jv::str(m.get("id")).is_some_and(|i| want.contains(i))).collect();
    let first = marks.first()?;
    let sheet = first.get("sheet").cloned().unwrap_or(Value::Null);
    let marks: Vec<&Value> = marks.into_iter().filter(|m| jv::strict_eq(m.get("sheet"), Some(&sheet))).collect();
    if marks.len() > CLIP_MARKS {
        return slow();
    }
    let mut item_ids: Vec<String> = Vec::new();
    for m in &marks {
        let i = jv::to_string(m.get("item"));
        if !item_ids.contains(&i) {
            item_ids.push(i);
        }
    }
    let items_all = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let items_raw: Vec<Value> =
        item_ids.iter().filter_map(|id| items_all.iter().find(|i| jv::str(i.get("id")) == Some(id.as_str())).cloned()).collect();
    if items_raw.len() > CLIP_ITEMS {
        return slow();
    }
    let proj = json!({
        "format": model::FORMAT, "version": 1, "name": "Clipboard",
        "files": [{ "hash": zero_hash(), "name": "clipboard.pdf", "size": 0, "pages": 1 }],
        "items": items_raw,
    });
    let Ok(r) = model::read_project(&proj) else { return slow() };
    let items: Vec<Value> = jv::arr(r.state.get("items")).cloned().unwrap_or_default();
    if items.is_empty() {
        return slow();
    }
    // the takeoff's own shape: points {x, y} or {x, y, b} with b set, finite and within reach — and readProject's checks
    let fin = |v: Option<&Value>| jv::finite(v).is_some_and(|x| x.abs() < 1e7);
    let pt_ok = |p: &Value| -> bool {
        let Value::Object(o) = p else { return false };
        let b_ok = match o.get("b") {
            None => o.len() == 2,
            Some(b) => o.len() == 3 && jv::truthy(Some(b)) && jv::finite(Some(b)).is_some_and(|x| x.abs() <= 1000.0),
        };
        b_ok && fin(o.get("x")) && fin(o.get("y"))
    };
    let pts_ok = |a: Option<&Value>, min: usize| match a {
        Some(Value::Array(l)) => l.len() >= min && l.len() <= limits::POINTS && l.iter().all(pt_ok),
        _ => false,
    };
    let ring_ok = |r: Option<&Value>| pts_ok(r, 2) && jv::arr(r).is_some_and(|l| l.len() >= 3 || l.iter().any(|p| p.get("b").is_some()));
    let kinds: Vec<(&str, Vec<String>)> = items.iter().filter_map(|i| Some((jv::str(i.get("id"))?, kinds_of(Some(i))))).collect();
    for m in &marks {
        let (Some(item), Some(kind)) = (jv::str(m.get("item")), jv::str(m.get("kind"))) else { return slow() };
        let Some((_, ks)) = kinds.iter().find(|(id, _)| *id == item) else { return slow() };
        if !ks.iter().any(|k| k == kind) {
            return slow();
        }
        let min = if kind == "area" || kind == "linear" { 2 } else { 1 };
        let pr = m.get("pts");
        if !pts_ok(pr, min)
            || (kind == "area" && !ring_ok(pr))
            || (kind == "count" && jv::arr(pr).is_some_and(|l| l.iter().any(|p| p.get("b").is_some())))
            || (kind == "area" && jv::arr(m.get("holes")).is_some_and(|h| h.iter().any(|r| !ring_ok(Some(r)))))
        {
            return slow();
        }
    }
    let ctx0 = ExtrasCtx::default();
    let mut used: HashSet<String> = HashSet::new();
    let clip_marks: Vec<Value> = marks
        .iter()
        .map(|m| {
            let kind = jv::str(m.get("kind")).unwrap_or("");
            let item = jv::str(m.get("item")).unwrap_or("");
            if !used.contains(item) {
                used.insert(item.to_string());
            }
            let holes: Vec<Value> = if kind == "area" {
                jv::arr(m.get("holes")).map(|hl| hl.iter().take(limits::HOLES).cloned().collect()).unwrap_or_default()
            } else {
                Vec::new()
            };
            let mut o = Obj::new();
            o.insert("item".into(), s(item));
            o.insert("kind".into(), s(kind));
            o.insert("pts".into(), m.get("pts").cloned().unwrap_or(Value::Array(Vec::new())));
            o.insert("holes".into(), Value::Array(holes));
            // what the copy carries (mark_extras), read back as readProject reads it, then as the clip keeps it
            let ex0 = mark_extras(m);
            if !ex0.is_empty() {
                let ex = read_extras_by(|k| ex0.get(k), &o, &ctx0);
                let mut km = Obj::new();
                km.insert("kind".into(), s(kind));
                jv::assign(&mut km, &ex);
                jv::assign(&mut o, &mark_extras(&Value::Object(km)));
            }
            Value::Object(o)
        })
        .collect();
    let mut head = Obj::new();
    head.insert("source".into(), s(ctx.source.clone()));
    head.insert("copiedAt".into(), n(ctx.now));
    head.insert("sheet".into(), sheet_info(state, &jv::to_string(Some(&sheet)), ctx));
    head.insert("anchor".into(), anchor_of_refs(marks.iter().copied()));
    Some(clip_assemble(&head, items, clip_marks, &used, r.warnings))
}

/// [`read_clip_owned`] of a clip [`copy_marks`] has just made from a takeoff's own marks (Copy, Cut): those marks are
/// clean already, so after the same checks they are read straight into the clip instead of through a whole clip
/// project; the items still go through `read_project`. A mark the checks would drop sends the clip down the full read,
/// so the result is always `read_clip_owned`'s (test `fresh_read_is_the_full_read`).
pub fn read_clip_fresh(json: Value) -> Result<ReadClip, String> {
    let fast = (|| {
        let j = clip_head(&json).ok()?;
        let items_raw = jv::arr(j.get("items"))?;
        let marks_raw = jv::arr(j.get("marks"))?;
        if items_raw.len() > CLIP_ITEMS || marks_raw.len() > CLIP_MARKS {
            return None;
        }
        // the items as the clip project reads them (with the file and no marks)
        let proj = json!({
            "format": model::FORMAT, "version": 1, "name": "Clipboard",
            "files": [{ "hash": zero_hash(), "name": "clipboard.pdf", "size": 0, "pages": 1 }],
            "items": items_raw,
        });
        let r = model::read_project(&proj).ok()?;
        let items: Vec<Value> = jv::arr(r.state.get("items")).cloned().unwrap_or_default();
        if items.is_empty() {
            return None;
        }
        // readProject's checks of each mark: a known item and one of its kinds, finite points within reach
        let finite = |v: Option<&Value>| jv::finite(v).filter(|x| x.abs() < 1e7);
        let pt_ok = |p: &Value| -> bool {
            let Value::Array(a) = p else { return false };
            (a.len() == 2 || (a.len() == 3 && finite(a.get(2)).is_some_and(|b| b.abs() <= 1000.0)))
                && finite(a.first()).is_some()
                && finite(a.get(1)).is_some()
        };
        let pts_ok = |a: Option<&Value>, min: usize| match a {
            Some(Value::Array(l)) => l.len() >= min && l.len() <= limits::POINTS && l.iter().all(pt_ok),
            _ => false,
        };
        let ring_ok =
            |r: Option<&Value>| pts_ok(r, 2) && jv::arr(r).is_some_and(|l| l.len() >= 3 || l.iter().any(|p| jv::truthy(p.get(2))));
        for m in marks_raw {
            let o = m.as_object()?;
            let item = jv::str(o.get("item"))?;
            let kind = jv::str(o.get("kind"))?;
            let it = items.iter().find(|i| jv::str(i.get("id")) == Some(item))?;
            if !kinds_of(Some(it)).iter().any(|k| k == kind) {
                return None;
            }
            let min = if kind == "area" || kind == "linear" { 2 } else { 1 };
            let pr = o.get("pts");
            if !pts_ok(pr, min)
                || (kind == "area" && !ring_ok(pr))
                || (kind == "count" && jv::arr(pr).is_some_and(|l| l.iter().any(|p| jv::truthy(p.get(2)))))
            {
                return None;
            }
        }
        let ctx = ExtrasCtx::default();
        let sheet = s(clip_sheet());
        let state_marks: Vec<Value> = marks_raw
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let kind = jv::str(m.get("kind")).unwrap_or("");
                let ring = |r: Option<&Value>| Value::Array(jv::arr(r).map(|r| r.iter().map(model::pt).collect()).unwrap_or_default());
                let holes: Vec<Value> = if kind == "area" {
                    jv::arr(m.get("holes"))
                        .map(|hl| hl.iter().take(limits::HOLES).filter(|h| ring_ok(Some(h))).map(|h| ring(Some(h))).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut nm = Obj::new();
                nm.insert("id".into(), s(format!("k{}", i.saturating_add(1))));
                nm.insert("item".into(), s(jv::str(m.get("item")).unwrap_or("")));
                nm.insert("sheet".into(), sheet.clone());
                nm.insert("kind".into(), s(kind));
                nm.insert("pts".into(), ring(m.get("pts")));
                nm.insert("holes".into(), Value::Array(holes));
                let ex = read_extras(Some(m), &nm, &ctx);
                jv::assign(&mut nm, &ex);
                Value::Object(nm)
            })
            .collect();
        Some((items, state_marks, !marks_raw.is_empty(), r.warnings))
    })();
    match (fast, &json) {
        (Some((items, marks, any, warnings)), Value::Object(j)) => clip_finish(j, items, marks, any, warnings),
        _ => read_clip_owned(json),
    }
}

/// The clip's format and version checks → its object.
fn clip_head(json: &Value) -> Result<&Obj, String> {
    let Value::Object(j) = json else { return Err(t("clip.err.format")) };
    if jv::str(j.get("format")) != Some(CLIP_FORMAT) {
        return Err(t("clip.err.format"));
    }
    let ver = jv::to_number(j.get("version"));
    if !(ver >= 1.0) {
        return Err(t("clip.err.version"));
    }
    if ver > CLIP_VERSION {
        return Err(t("clip.err.newer"));
    }
    Ok(j)
}

/// One copied mark as the clip project's mark `i` (`get` hands over each field it has).
fn clip_mark_in(i: usize, m: &Value, mut get: impl FnMut(&str) -> Option<Value>) -> Value {
    if !m.is_null() && !jv::is_objlike(Some(m)) {
        return Value::Null;
    }
    let mut o = Obj::new();
    for k in MARK_EXTRAS {
        if let Some(v) = get(k) {
            o.insert(k.into(), v);
        }
    }
    for k in ["item", "kind", "pts", "holes"] {
        if let Some(v) = get(k) {
            o.insert(k.into(), v);
        }
    }
    o.insert("id".into(), s(format!("k{}", i.saturating_add(1))));
    o.insert("sheet".into(), s(clip_sheet()));
    Value::Object(o)
}

/// The rest of [`read_clip`]: the clip project read back, then the clip as a paste uses it.
fn clip_tail(j: &Obj, raw_items: Vec<Value>, marks_in: Vec<Value>, any_marks: bool) -> Result<ReadClip, String> {
    let mut proj = json!({
        "format": model::FORMAT, "version": 1, "name": "Clipboard",
        "files": [{ "hash": zero_hash(), "name": "clipboard.pdf", "size": 0, "pages": 1 }],
    });
    if let Value::Object(o) = &mut proj {
        o.insert("items".into(), Value::Array(raw_items));
        o.insert("marks".into(), Value::Array(marks_in));
    }
    let mut r = model::read_project(&proj)?;
    let items = match r.state.get_mut("items").map(Value::take) {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    if items.is_empty() {
        return Err(t("clip.err.noitems"));
    }
    let state_marks = match r.state.get_mut("marks").map(Value::take) {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    clip_finish(j, items, state_marks, any_marks, r.warnings)
}

/// The end of a clip's read: its marks as the clip project read them back, then the clip as a paste uses it.
fn clip_finish(j: &Obj, items: Vec<Value>, state_marks: Vec<Value>, any_marks: bool, warnings: Vec<String>) -> Result<ReadClip, String> {
    let used: HashSet<String> = state_marks.iter().map(|m| jv::to_string(m.get("item"))).collect();
    if any_marks && state_marks.is_empty() {
        return Err(t("clip.err.damaged"));
    }
    // the read project's marks are ours: their fields move into the clip
    let marks: Vec<Value> = state_marks
        .into_iter()
        .map(|mut m| {
            let ex = mark_extras(&m);
            let mut o = Obj::new();
            for k in ["item", "kind", "pts", "holes"] {
                o.insert(k.into(), m.get_mut(k).map_or(Value::Null, Value::take));
            }
            jv::assign(&mut o, &ex);
            Value::Object(o)
        })
        .collect();
    clip_assemble(j, items, marks, &used, warnings)
}

/// The clip from its head (sheet, anchor, source, time), the items read back, its marks and the items they use.
fn clip_assemble(j: &Obj, items: Vec<Value>, marks: Vec<Value>, used: &HashSet<String>, warnings: Vec<String>) -> Result<ReadClip, String> {
    let sheet = match j.get("sheet") {
        Some(sv @ (Value::Object(_) | Value::Array(_))) => {
            let g = |k: &str| match sv {
                Value::Object(o) => o.get(k),
                _ => None,
            };
            let view = match g("view") {
                Some(Value::Array(a)) if a.len() == 4 && a.iter().all(|v| jv::finite(Some(v)).is_some_and(|x| x.abs() < 1e7)) => {
                    Value::Array(a.clone())
                }
                _ => Value::Null,
            };
            let fpu = jv::finite(g("ftPerUnit")).filter(|x| *x > 0.0 && *x < 1e6).map_or(Value::Null, n);
            let uu = jv::finite(g("userUnit")).filter(|x| *x > 0.0 && *x < 1e4).unwrap_or(1.0);
            json!({ "ftPerUnit": fpu, "label": model::clean_str(g("label"), 80), "view": view, "userUnit": n(uu) })
        }
        _ => Value::Null,
    };
    let anchor = match j.get("anchor") {
        Some(a) if jv::finite(a.get("x")).is_some_and(|x| x.abs() < 1e7) && jv::finite(a.get("y")).is_some_and(|y| y.abs() < 1e7) => {
            json!({ "x": a.get("x"), "y": a.get("y") })
        }
        _ => anchor_of(&marks),
    };
    let copied_at = jv::finite(j.get("copiedAt")).unwrap_or(0.0);
    let items_out: Vec<Value> =
        if marks.is_empty() { items } else { items.into_iter().filter(|i| used.contains(&jv::to_string(i.get("id")))).collect() };
    Ok(ReadClip {
        warnings,
        clip: {
            let mut c = json!({
                "format": CLIP_FORMAT, "version": n(CLIP_VERSION), "source": model::clean_str(j.get("source"), 64), "copiedAt": n(copied_at),
            });
            if let Value::Object(o) = &mut c {
                o.insert("sheet".into(), sheet);
                o.insert("anchor".into(), anchor);
                o.insert("items".into(), Value::Array(items_out));
                o.insert("marks".into(), Value::Array(marks));
            }
            c
        },
    })
}

/// True when the clip has lengths or areas.
pub fn measured(clip: &Value) -> bool {
    jv::arr(clip.get("marks")).is_some_and(|l| l.iter().any(|m| jv::str(m.get("kind")) != Some("count")))
}

/// Can the clip's marks go on a sheet with this scale? → None | the reason.
pub fn scale_problem(clip: &Value, target: Option<&Value>) -> Option<String> {
    if !measured(clip) {
        return None;
    }
    let from = clip.get("sheet").and_then(|x| x.get("ftPerUnit")).filter(|v| jv::truthy(Some(v))).map(|v| jv::to_number(Some(v)));
    let Some(from) = from else { return Some(t("clip.scale.fromnone")) };
    let to = target.map_or(f64::NAN, |x| jv::to_number(x.get("ftPerUnit")));
    if !(to > 0.0) {
        return Some(t("clip.scale.tonone"));
    }
    if (to - from).abs() / from > SCALE_TOL {
        let fl = clip
            .get("sheet")
            .and_then(|x| x.get("label"))
            .filter(|v| jv::truthy(Some(v)))
            .map_or_else(|| t("clip.scale.another"), |v| jv::to_string(Some(v)));
        let tl = target
            .and_then(|x| x.get("label"))
            .filter(|v| jv::truthy(Some(v)))
            .map_or_else(|| t("clip.scale.different"), |v| jv::to_string(Some(v)));
        return Some(tf("clip.scale.other", &[("from", F::S(fl)), ("to", F::S(tl))]));
    }
    None
}

/// Paste in place keeps PDF coordinates: the target sheet must be the same size.
pub fn size_problem(clip: &Value, target: Option<&Value>) -> Option<String> {
    let a = clip.get("sheet").and_then(|x| x.get("view")).filter(|v| jv::truthy(Some(v)));
    let b = target.and_then(|x| x.get("view")).filter(|v| jv::truthy(Some(v)));
    let (Some(a), Some(b)) = (a, b) else { return Some(t("clip.size.unknown")) };
    let uu = |v: Option<&Value>| v.filter(|x| jv::truthy(Some(x))).map_or(1.0, |x| jv::to_number(Some(x)));
    let ua = uu(clip.get("sheet").and_then(|x| x.get("userUnit")));
    let ub = uu(target.and_then(|x| x.get("userUnit")));
    let av = jv::arr(Some(a)).cloned().unwrap_or_default();
    let at = |v: &Value, i: usize| match v {
        Value::Array(l) => jv::to_number(l.get(i)),
        Value::Object(o) => jv::to_number(o.get(&i.to_string())),
        _ => f64::NAN,
    };
    let same = av.iter().enumerate().all(|(i, v)| (jv::to_number(Some(v)) * ua - at(b, i) * ub).abs() <= SIZE_TOL);
    if same {
        return None;
    }
    let inch = |v: &Value, u: f64| {
        let w = cc_m3d::js::round((at(v, 2) - at(v, 0)).abs() * u / 72.0 * 10.0) / 10.0;
        let h = cc_m3d::js::round((at(v, 3) - at(v, 1)).abs() * u / 72.0 * 10.0) / 10.0;
        format!("{}\" × {}\"", crate::jv::num_str(w), crate::jv::num_str(h))
    };
    Some(tf("clip.size.other", &[("from", F::S(inch(a, ua))), ("to", F::S(inch(b, ub)))]))
}

/// "Name", or "Name (copy)", "Name (copy 2)"… when the takeoff already has that name.
pub fn free_name(name: &str, items: &[Value]) -> String {
    let taken: HashSet<String> = items.iter().map(|i| jv::to_string(i.get("name"))).collect();
    if !taken.contains(name) {
        return name.to_string();
    }
    let word = t("clip.copy.word");
    // name.replace(/ \((?:copy|word)(?: \d+)?\)$/, '')
    let mut base = name.to_string();
    for w in ["copy", word.as_str()] {
        let pre = format!(" ({w}");
        if let Some(body) = name.strip_suffix(')')
            && let Some(i) = body.rfind(&pre)
        {
            let rest = body.get(i.saturating_add(pre.len())..).unwrap_or("");
            let ok = rest.is_empty() || (rest.starts_with(' ') && rest.len() > 1 && rest.bytes().skip(1).all(|c| c.is_ascii_digit()));
            if ok {
                base = name.get(..i).unwrap_or("").to_string();
                break;
            }
        }
    }
    let base = cc_m3d::js::slice16(&base, 180);
    let mut k = 1u64;
    loop {
        let c = if k == 1 { format!("{base} ({word})") } else { format!("{base} ({word} {k})") };
        if !taken.contains(&c) || k > 1_000_000 {
            return c;
        }
        k = k.saturating_add(1);
    }
}

/// What a paste added.
#[derive(Debug, Clone, Default)]
pub struct Pasted {
    pub marks: Vec<String>,
    pub items: Vec<String>,
    pub reused: Vec<String>,
}

/// Paste options: the sheet, the move, the undo label and the bid area (None: each mark keeps its own; Some(None):
/// in none).
#[derive(Debug, Clone)]
pub struct PasteOpts {
    pub sheet: Option<String>,
    pub dx: f64,
    pub dy: f64,
    pub label: String,
    pub area: Option<Option<String>>,
}
impl Default for PasteOpts {
    fn default() -> Self {
        PasteOpts { sheet: None, dx: 0.0, dy: 0.0, label: "Paste".into(), area: None }
    }
}

/// Paste the clip, moved by (dx, dy), as ONE undo step. → None when nothing was pasted.
pub fn paste_clip(model: &mut Takeoff, clip: &Value, o: &PasteOpts) -> Res<Option<Pasted>> {
    let items = jv::arr(clip.get("items")).map_or(&[][..], Vec::as_slice);
    if items.is_empty() {
        return Ok(None);
    }
    let marks = jv::arr(clip.get("marks")).map_or(&[][..], Vec::as_slice);
    if !marks.is_empty() && o.sheet.as_deref().is_none_or(str::is_empty) {
        return Err(t("clip.err.nosheet"));
    }
    let sheet = o.sheet.clone().unwrap_or_default();
    model.change(&o.label, |s| {
        let mut out = Pasted::default();
        let mut map: Vec<(String, String)> = Vec::new();
        for ci in items {
            let ci_id = jv::to_string(ci.get("id"));
            let mut target = None;
            if !marks.is_empty() {
                let st_items = jv::arr(s.get("items")).map_or(&[][..], Vec::as_slice);
                target = st_items
                    .iter()
                    .find(|i| {
                        jv::strict_eq(i.get("id"), ci.get("id"))
                            && jv::strict_eq(i.get("name"), ci.get("name"))
                            && jv::strict_eq(i.get("type"), ci.get("type"))
                    })
                    .or_else(|| {
                        st_items
                            .iter()
                            .find(|i| jv::strict_eq(i.get("name"), ci.get("name")) && jv::strict_eq(i.get("type"), ci.get("type")))
                    })
                    .map(|i| jv::to_string(i.get("id")));
            }
            if let Some(tid) = target {
                map.push((ci_id, tid.clone()));
                out.reused.push(tid);
                continue;
            }
            if jv::arr(s.get("items")).map_or(0, Vec::len) >= limits::ITEMS {
                return Err(t("err.items.many"));
            }
            let id = model::next_id(s, "i");
            let name = free_name(&jv::to_string(ci.get("name")), jv::arr(s.get("items")).map_or(&[][..], Vec::as_slice));
            let comps: Vec<Value> = jv::arr(ci.get("components"))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|c| {
                    let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                    co.insert("id".into(), s_(&model::next_id(s, "c")));
                    Value::Object(co)
                })
                .collect();
            let mut f = jv::obj(Some(ci)).cloned().unwrap_or_default();
            f.insert("id".into(), s_(&id));
            f.insert("name".into(), s_(&name));
            f.insert("components".into(), Value::Array(comps));
            let item = Value::Object(normal_item(&f));
            if let Value::Object(so) = s
                && let Some(Value::Array(l)) = so.get_mut("items")
            {
                l.push(item);
            }
            map.push((ci_id, id.clone()));
            out.items.push(id);
        }
        if jv::arr(s.get("marks")).map_or(0, Vec::len).saturating_add(marks.len()) > limits::MARKS {
            return Err(t("err.marks.many"));
        }
        let mv = |p: &Value| -> Res<Value> {
            let x = jv::to_number(jv::field(p, "x")) + o.dx;
            let y = jv::to_number(jv::field(p, "y")) + o.dy;
            if !(x.abs() < 1e7 && y.abs() < 1e7) {
                return Err(t("clip.err.far"));
            }
            let mut q = Obj::new();
            q.insert("x".into(), n(x));
            q.insert("y".into(), n(y));
            if jv::truthy(jv::field(p, "b"))
                && let Some(b) = jv::field(p, "b")
            {
                q.insert("b".into(), b.clone());
            }
            Ok(Value::Object(q))
        };
        // the areas do not change while the marks go in
        let areas: HashSet<String> =
            jv::arr(s.get("areas")).map(|l| l.iter().map(|a| jv::to_string(a.get("id"))).collect()).unwrap_or_default();
        let ctx = ExtrasCtx { areas, groups: HashSet::new() };
        // each clip item's target and its kinds, looked up once (the items do not change while the marks go in)
        let targets: Vec<(String, String, Vec<String>)> = {
            let st_items = jv::arr(s.get("items")).map_or(&[][..], Vec::as_slice);
            map.iter()
                .map(|(k, v)| {
                    let it = st_items.iter().find(|i| jv::str(i.get("id")) == Some(v.as_str()));
                    (k.clone(), v.clone(), if it.is_some() { kinds_of(it) } else { Vec::new() })
                })
                .collect()
        };
        for m in marks {
            let mi = jv::to_string(jv::field(m, "item"));
            let Some((_, item, kinds)) = targets.iter().find(|(k, _, _)| *k == mi) else { continue };
            let item = item.clone();
            let mk = jv::to_string(jv::field(m, "kind"));
            if !kinds.contains(&mk) {
                continue;
            }
            let id = model::next_id(s, "m");
            let mut pts = Vec::new();
            for p in jv::arr(jv::field(m, "pts")).map_or(&[][..], Vec::as_slice) {
                pts.push(mv(p)?);
            }
            let mut holes = Vec::new();
            if mk == "area" {
                for h in jv::arr(jv::field(m, "holes")).map_or(&[][..], Vec::as_slice) {
                    let mut r = Vec::new();
                    for p in jv::arr(Some(h)).map_or(&[][..], Vec::as_slice) {
                        r.push(mv(p)?);
                    }
                    holes.push(Value::Array(r));
                }
            }
            let mut nm = Obj::new();
            nm.insert("id".into(), s_(&id));
            nm.insert("item".into(), s_(&item));
            nm.insert("sheet".into(), s_(&sheet));
            nm.insert("kind".into(), s_(&mk));
            nm.insert("pts".into(), Value::Array(pts));
            nm.insert("holes".into(), Value::Array(holes));
            // `{ ...m, area }` when the paste names an area, read without copying the mark
            let area_v = o.area.as_ref().map(|a| a.as_ref().map_or(Value::Null, |x| s_(x)));
            let ex = read_extras_by(
                |k: &str| match (&area_v, k) {
                    (Some(a), "area") => Some(a),
                    _ => jv::field(m, k),
                },
                &nm,
                &ctx,
            );
            jv::assign(&mut nm, &ex);
            if let Value::Object(so) = s
                && let Some(Value::Array(l)) = so.get_mut("marks")
            {
                l.push(Value::Object(nm));
            }
            out.marks.push(id);
        }
        if out.marks.is_empty() && out.items.is_empty() {
            return Ok(Ch::Cancel);
        }
        Ok(Ch::Commit(out))
    })
}

fn s_(x: &str) -> Value {
    Value::String(x.to_string())
}
