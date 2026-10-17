//! part.js — the part document: `{format:'partcraft', version, name, units:'mm', resolution, seq, rollback, features[],
//! drawing?, material?, bodyInfo?, studies?, params?, configs?}`. Adding, moving and deleting features (with the
//! dependency rules), the file version a part needs, `toJSON` (byte-identical text) and the untrusted reader `readPart`
//! (every field range-checked, every reference to an earlier feature; ≤ 2000 features).
//!
//! Studies: no workspace registers a study reader in this port yet, so a file carrying studies is refused with the JS's
//! "study data that this PartCraft does not read" message (what the JS does with only part.js loaded).

use crate::bom;
use crate::configs;
use crate::form;
use crate::gdt;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::params_part;
use crate::re;
use crate::readers::{self as rd, Ctx};
use crate::sketch;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const FORMAT: &str = "partcraft";
pub const VERSION: f64 = 3.0;
pub const DRAWING_VIEWS: [&str; 3] = ["front", "top", "right"];
pub const SHEET_KEYS: [&str; 10] = ["a4", "a3", "a2", "a1", "a0", "ansi-a", "ansi-b", "ansi-c", "ansi-d", "ansi-e"];
pub const TITLE_FIELDS: [(&str, usize); 10] = [
    ("title", 60),
    ("number", 40),
    ("rev", 8),
    ("company", 60),
    ("drawnBy", 40),
    ("checkedBy", 40),
    ("approvedBy", 40),
    ("date", 10),
    ("finish", 40),
    ("tolerance", 40),
];
pub const MAX_REVISIONS: usize = 20;
pub const DETAIL_SCALES: [f64; 4] = [2.0, 4.0, 5.0, 10.0];

/// Every feature type, in `FEATURE_LABELS` order.
pub const TYPES: [&str; 34] = [
    "sketch",
    "extrude",
    "revolve",
    "hole",
    "lpattern",
    "cpattern",
    "mirror",
    "fillet",
    "chamfer",
    "shell",
    "import",
    "plane",
    "axis",
    "point",
    "sweep",
    "loft",
    "draft",
    "form",
    "patch",
    "surfloft",
    "stitch",
    "thicken",
    "offsetsurf",
    "extendsurf",
    "trimsurf",
    "gordon",
    "combine",
    "movebody",
    "scalebody",
    "pathpattern",
    "sheetbase",
    "sheetflange",
    "rib",
    "thread",
];
/// `FEATURE_LABELS[type]` is truthy (a known type, or an `Object.prototype` name).
pub fn known_type(ty: &str) -> bool {
    TYPES.contains(&ty) || sketch::proto_key(ty)
}
/// `FEATURE_LABELS[type]`.
pub fn feature_label(ty: &str) -> String {
    if TYPES.contains(&ty) { t(&format!("model.feature.{ty}"), &[]) } else { "undefined".into() }
}
/// `EXACT_ONLY`.
pub fn exact_only(ty: &str) -> bool {
    matches!(
        ty,
        "fillet"
            | "chamfer"
            | "shell"
            | "import"
            | "sweep"
            | "loft"
            | "draft"
            | "form"
            | "patch"
            | "surfloft"
            | "stitch"
            | "thicken"
            | "offsetsurf"
            | "extendsurf"
            | "trimsurf"
            | "gordon"
            | "combine"
            | "movebody"
            | "scalebody"
            | "pathpattern"
            | "sheetbase"
            | "sheetflange"
            | "rib"
    )
}

fn base_name(ty: &str) -> &'static str {
    match ty {
        "sketch" => "Sketch",
        "extrude" => "Extrude",
        "revolve" => "Revolve",
        "hole" => "Hole",
        "lpattern" => "LPattern",
        "cpattern" => "CirPattern",
        "mirror" => "Mirror",
        "fillet" => "Fillet",
        "chamfer" => "Chamfer",
        "shell" => "Shell",
        "import" => "Imported",
        "plane" => "Plane",
        "axis" => "Axis",
        "point" => "Point",
        "sweep" => "Sweep",
        "loft" => "Loft",
        "draft" => "Draft",
        "form" => "Form",
        "patch" => "Patch",
        "surfloft" => "LoftSurface",
        "stitch" => "Stitch",
        "thicken" => "Thicken",
        "offsetsurf" => "OffsetSurface",
        "extendsurf" => "Extend",
        "trimsurf" => "Trim",
        "gordon" => "Gordon",
        "combine" => "Combine",
        "movebody" => "Move",
        "scalebody" => "Scale",
        "pathpattern" => "PathPattern",
        "sheetbase" => "BaseFlange",
        "sheetflange" => "EdgeFlange",
        "rib" => "Rib",
        "thread" => "Thread",
        _ => "undefined",
    }
}

fn base(name: &str) -> Value {
    jv::obj(vec![("kind", "base".into()), ("name", name.into())])
}
fn world(axis: &str) -> Value {
    jv::obj(vec![("kind", "world".into()), ("axis", axis.into())])
}
fn arr0() -> Value {
    Value::Array(Vec::new())
}

/// `DEFAULTS[type]()`.
pub fn defaults(ty: &str) -> Option<Vec<(&'static str, Value)>> {
    let n = jv::num;
    Some(match ty {
        "sketch" => vec![("plane", base("front")), ("sketch", sketch::new_sketch())],
        "extrude" => {
            vec![("sketch", Value::Null), ("op", "add".into()), ("end", "blind".into()), ("depth", n(10.0)), ("reverse", false.into())]
        }
        "revolve" => vec![("sketch", Value::Null), ("op", "add".into()), ("axis", Value::Null), ("angle", n(360.0))],
        "hole" => vec![
            ("sketch", Value::Null),
            ("kind", "simple".into()),
            ("d", n(6.0)),
            ("depth", n(10.0)),
            ("through", true.into()),
            ("cboreD", n(11.0)),
            ("cboreDepth", n(6.0)),
            ("reverse", false.into()),
        ],
        "lpattern" => vec![("seeds", arr0()), ("dir", world("x")), ("count", n(3.0)), ("spacing", n(20.0)), ("reverse", false.into())],
        "cpattern" => vec![("seeds", arr0()), ("axis", world("y")), ("count", n(6.0)), ("angle", n(360.0)), ("reverse", false.into())],
        "mirror" => vec![("seeds", arr0()), ("plane", base("right"))],
        "fillet" => vec![("edges", arr0()), ("r", n(2.0))],
        "chamfer" => vec![("edges", arr0()), ("d", n(1.0))],
        "shell" => vec![("faces", arr0()), ("t", n(2.0)), ("outward", false.into())],
        "import" => vec![("file", "".into()), ("step", "".into()), ("up", "z".into())],
        "plane" => vec![
            ("mode", "offset".into()),
            ("planes", Value::Array(vec![base("top")])),
            ("d", n(20.0)),
            ("angle", n(30.0)),
            ("axis", Value::Null),
            ("points", arr0()),
            ("face", Value::Null),
            ("edge", Value::Null),
            ("flip", false.into()),
        ],
        "axis" => vec![
            ("mode", "planes".into()),
            ("planes", Value::Array(vec![base("front"), base("right")])),
            ("points", arr0()),
            ("face", Value::Null),
            ("edge", Value::Null),
        ],
        "point" => vec![("mode", "xyz".into()), ("points", arr0()), ("edge", Value::Null), ("p", jv::nums(&[0.0, 0.0, 0.0]))],
        "sweep" => vec![
            ("sketch", Value::Null),
            ("path", arr0()),
            ("op", "add".into()),
            ("orient", "corrected".into()),
            ("binormal", Value::Null),
            ("guide", Value::Null),
            ("scale", n(1.0)),
        ],
        "loft" => vec![("sections", arr0()), ("op", "add".into()), ("ruled", false.into())],
        "draft" => vec![("faces", arr0()), ("neutral", base("top")), ("pull", Value::Null), ("angle", n(3.0))],
        "form" => vec![("cage", form::box_form(2.0, 2.0, 2.0, 100.0, 100.0, 100.0)), ("op", "new".into()), ("level", Value::Null)],
        "patch" => vec![("edges", arr0()), ("sheet", Value::Null), ("at", Value::Null), ("cont", "G0".into())],
        "surfloft" => vec![("sketches", arr0())],
        "stitch" => vec![("sheets", arr0()), ("op", "new".into()), ("tol", n(0.01))],
        "thicken" => vec![("sheet", Value::Null), ("t", n(2.0)), ("side", "out".into()), ("op", "new".into())],
        "offsetsurf" => vec![("sheet", Value::Null), ("d", n(5.0))],
        "extendsurf" => vec![("sheet", Value::Null), ("d", n(10.0)), ("sides", jv::strs(&["umin", "umax", "vmin", "vmax"]))],
        "trimsurf" => vec![("sheet", Value::Null), ("tool", Value::Null), ("at", Value::Null), ("keep", "near".into())],
        "gordon" => vec![("profiles", arr0()), ("guides", arr0()), ("tol", n(0.01)), ("solid", false.into()), ("op", "new".into())],
        "combine" => vec![("op", "join".into()), ("target", Value::Null), ("tools", arr0()), ("keep", false.into())],
        "movebody" => vec![
            ("bodies", arr0()),
            ("mode", "translate".into()),
            ("dx", n(0.0)),
            ("dy", n(0.0)),
            ("dz", n(0.0)),
            ("from", Value::Null),
            ("to", Value::Null),
            ("axis", world("y")),
            ("edge", Value::Null),
            ("face", Value::Null),
            ("angle", n(90.0)),
            ("planes", arr0()),
            ("flip", false.into()),
            ("copy", false.into()),
        ],
        "scalebody" => vec![
            ("bodies", arr0()),
            ("uniform", true.into()),
            ("s", n(2.0)),
            ("sx", n(1.0)),
            ("sy", n(1.0)),
            ("sz", n(1.0)),
            ("point", Value::Null),
        ],
        "pathpattern" => vec![
            ("seeds", arr0()),
            ("bodies", arr0()),
            ("path", arr0()),
            ("count", n(4.0)),
            ("spacing", "equal".into()),
            ("d", n(20.0)),
            ("orient", "path".into()),
            ("reverse", false.into()),
        ],
        "sheetbase" => vec![("t", n(2.0)), ("r", n(2.0)), ("k", n(0.44)), ("depth", n(50.0)), ("reverse", false.into())],
        "sheetflange" => vec![
            ("edge", Value::Null),
            ("angle", n(90.0)),
            ("length", n(20.0)),
            ("flip", false.into()),
            ("offsets", jv::nums(&[0.0, 0.0])),
            ("relief", "rect".into()),
            ("reliefWidth", n(2.0)),
        ],
        "rib" => vec![("sketch", Value::Null), ("t", n(3.0)), ("side", "mid".into()), ("flip", false.into())],
        "thread" => vec![
            ("faces", arr0()),
            ("form", "iso".into()),
            ("size", "auto".into()),
            ("length", Value::Null),
            ("flip", false.into()),
            ("modelled", false.into()),
            ("clearance", n(crate::threads::DEFAULT_CLEARANCE)),
            ("hand", "right".into()),
        ],
        _ => return None,
    })
}

/// `newPart(name)`.
pub fn new_part(name: &str) -> Value {
    jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", 2.into()),
        ("name", name.into()),
        ("units", "mm".into()),
        ("resolution", 64.into()),
        ("seq", 1.into()),
        ("rollback", Value::Null),
        ("features", arr0()),
    ])
}

pub fn features(part: &Value) -> &[Value] {
    jv::arr(part.get("features"))
}
fn features_mut(part: &mut Value) -> Option<&mut Vec<Value>> {
    part.get_mut("features").and_then(Value::as_array_mut)
}
/// `featureById(part, id)`.
pub fn feature_by_id<'a>(part: &'a Value, id: &str) -> Option<&'a Value> {
    features(part).iter().find(|f| jv::is_str(f.get("id"), id))
}
/// `indexOf(part, id)`.
pub fn index_of(part: &Value, id: &str) -> Option<usize> {
    features(part).iter().position(|f| jv::is_str(f.get("id"), id))
}
/// `builtCount(part)` (NaN when `rollback` is missing, as in the JS).
pub fn built_count(part: &Value) -> f64 {
    let n = js::f(features(part).len());
    match part.get("rollback") {
        Some(Value::Null) => n,
        Some(v) => js::min(v.as_f64().unwrap_or(f64::NAN), n),
        None => f64::NAN,
    }
}
/// The rollback value as JS reads it (`null` = None).
fn rollback(part: &Value) -> Option<f64> {
    match part.get("rollback") {
        Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().unwrap_or(f64::NAN)),
        None => Some(f64::NAN),
    }
}
/// `splice(at, …)`'s start for a JS number.
fn splice_at(at: f64, len: usize) -> usize {
    if at.is_nan() {
        return 0;
    }
    let a = at.trunc();
    if a < 0.0 {
        let r = js::f(len) + a;
        if r <= 0.0 { 0 } else { js::idx(r).unwrap_or(0) }
    } else {
        js::idx(js::min(a, js::f(len))).unwrap_or(len)
    }
}

fn next_name(part: &Value, ty: &str) -> String {
    let b = base_name(ty);
    let names: HashSet<String> = features(part).iter().map(|f| js::str_of(f.get("name"))).collect();
    let mut n = 1u64;
    while names.contains(&format!("{b}{n}")) {
        n = n.saturating_add(1);
    }
    format!("{b}{n}")
}

/// `addFeature(part, type, props)` → the new feature.
pub fn add_feature(part: &mut Value, ty: &str, props: &Value) -> Result<Value, String> {
    let defs = defaults(ty).ok_or_else(|| "DEFAULTS[type] is not a function".to_string())?;
    let seq = jv::n(part, "seq");
    jv::set(part, "seq", jv::num(seq + 1.0));
    let mut f = jv::obj(vec![("id", format!("f{}", js::num_str(seq)).into()), ("type", ty.into()), ("name", next_name(part, ty).into())]);
    for (k, v) in defs {
        jv::set(&mut f, k, v);
    }
    if let Some(o) = props.as_object() {
        for (k, v) in o {
            jv::set(&mut f, k, v.clone());
        }
    }
    let at = built_count(part);
    let len = features(part).len();
    let pos = splice_at(at, len);
    if let Some(fs) = features_mut(part) {
        fs.insert(pos.min(fs.len()), f.clone());
    }
    if let Some(rb) = rollback(part) {
        let _ = rb;
        jv::set(part, "rollback", jv::num(at + 1.0));
        let n = js::f(features(part).len());
        if rollback(part).is_some_and(|r| r >= n) {
            jv::set(part, "rollback", Value::Null);
        }
    }
    Ok(f)
}

/// `dependsOn(f)`.
pub fn depends_on(f: &Value) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    let mut add = |x: &str| {
        if !d.iter().any(|y| y == x) {
            d.push(x.to_string());
        }
    };
    if let Some(s) = f.get("sketch").and_then(Value::as_str) {
        add(s);
    }
    let ty = jv::s(f, "type");
    let plane = if ty == "sketch" || ty == "mirror" { f.get("plane") } else { None };
    if let Some(p) = plane.filter(|p| jv::truthy(Some(p)))
        && jv::is_str(p.get("kind"), "face")
    {
        add(&js::string_of(p.get("feature")));
        if let Some(n) = p.get("name").filter(|n| jv::truthy(Some(n))) {
            for x in re::name_features(&js::str_of(Some(n))) {
                add(&x);
            }
        }
    }
    if let Some(es) = jv::as_arr(f.get("edges")) {
        for e in es {
            for n in jv::arr(e.get("faces")) {
                for x in re::name_features(&js::str_of(Some(n))) {
                    add(&x);
                }
            }
        }
    }
    if let Some(fs) = jv::as_arr(f.get("faces")) {
        for r in fs {
            for x in re::name_features(&js::str_of(r.get("name"))) {
                add(&x);
            }
        }
    }
    if let Some(ss) = jv::as_arr(f.get("seeds")) {
        for s in ss {
            add(&js::string_of(Some(s)));
        }
    }
    let axis_obj = f.get("axis").filter(|a| jv::truthy(Some(a)) && jv::is_obj_like(Some(a)));
    for r in [f.get("dir").filter(|v| jv::truthy(Some(v))), axis_obj] {
        let Some(r) = r else { continue };
        if jv::is_str(r.get("kind"), "line") {
            add(&js::string_of(r.get("sketch")));
        }
        if jv::is_str(r.get("kind"), "axis") {
            add(&js::string_of(r.get("feature")));
        }
    }
    for x in rd::design_deps(f) {
        add(&x);
    }
    for x in rd::surface_deps(f) {
        add(&x);
    }
    for x in rd::bodies_deps(f) {
        add(&x);
    }
    for x in rd::sheet_deps(f) {
        add(&x);
    }
    if let Some(p) = f.get("plane").filter(|p| jv::truthy(Some(p)))
        && jv::is_str(p.get("kind"), "ref")
    {
        add(&js::string_of(p.get("feature")));
    }
    d
}

/// `dependents(part, id)`.
pub fn dependents(part: &Value, id: &str) -> Vec<Value> {
    features(part).iter().filter(|f| depends_on(f).iter().any(|d| d == id)).cloned().collect()
}

/// `canMove(part, id, to)` → Err(reason).
pub fn can_move(part: &Value, id: &str, to: f64) -> Result<(), String> {
    let n = features(part).len();
    let Some(from) = index_of(part, id) else { return Err(t("model.part.move.at_end", &[])) };
    if to < 0.0 || to >= js::f(n) || to.is_nan() {
        return Err(t("model.part.move.at_end", &[]));
    }
    if js::f(from) == to {
        return Err(t("model.part.move.there", &[]));
    }
    let fs = features(part);
    let mut order: Vec<&Value> = fs.iter().enumerate().filter(|(i, _)| *i != from).map(|(_, g)| g).collect();
    let Some(f) = fs.get(from) else { return Err(t("model.part.move.at_end", &[])) };
    let at = splice_at(to, order.len());
    order.insert(at.min(order.len()), f);
    let mut pos: HashMap<String, usize> = HashMap::new();
    for (i, g) in order.iter().enumerate() {
        pos.insert(js::str_of(g.get("id")), i);
    }
    for g in &order {
        let gid = js::str_of(g.get("id"));
        for d in depends_on(g) {
            if let (Some(pd), Some(pg)) = (pos.get(&d), pos.get(&gid))
                && pd > pg
            {
                let dep = feature_by_id(part, &d).map(|x| js::str_of(x.get("name"))).unwrap_or_default();
                return Err(t("model.part.move.uses", &[("name", F::S(js::str_of(g.get("name")))), ("dep", F::S(dep))]));
            }
        }
    }
    Ok(())
}

/// `moveFeature(part, id, to)`.
pub fn move_feature(part: &mut Value, id: &str, to: f64) -> Result<(), String> {
    can_move(part, id, to)?;
    let Some(from) = index_of(part, id) else { return Ok(()) };
    let built = built_count(part);
    if let Some(fs) = features_mut(part) {
        let f = fs.remove(from);
        let at = splice_at(to, fs.len());
        fs.insert(at.min(fs.len()), f);
    }
    if rollback(part).is_some() {
        jv::set(part, "rollback", jv::num(built));
    }
    Ok(())
}

/// `deleteFeatures(part, ids)` → the names removed.
pub fn delete_features(part: &mut Value, ids: &[String]) -> Vec<String> {
    let mut gone: HashSet<String> = ids.iter().cloned().collect();
    let mut grew = true;
    let mut guard = 0usize;
    while grew && guard <= features(part).len() {
        grew = false;
        guard = guard.saturating_add(1);
        for f in features(part) {
            let id = js::str_of(f.get("id"));
            if !gone.contains(&id) && depends_on(f).iter().any(|d| gone.contains(d)) {
                gone.insert(id);
                grew = true;
            }
        }
    }
    let built = built_count(part);
    let take = splice_at(built, features(part).len());
    let before = features(part).iter().take(take).filter(|f| gone.contains(&js::str_of(f.get("id")))).count();
    let names: Vec<String> =
        features(part).iter().filter(|f| gone.contains(&js::str_of(f.get("id")))).map(|f| js::str_of(f.get("name"))).collect();
    if let Some(fs) = features_mut(part) {
        fs.retain(|f| !gone.contains(&js::str_of(f.get("id"))));
    }
    if rollback(part).is_some() {
        let r = built - js::f(before);
        jv::set(part, "rollback", jv::num(r));
        if r >= js::f(features(part).len()) {
            jv::set(part, "rollback", Value::Null);
        }
    }
    names
}

fn has_configs(part: &Value) -> bool {
    part.get("configs")
        .filter(|c| jv::truthy(Some(c)))
        .and_then(|c| c.get("rows"))
        .filter(|r| jv::truthy(Some(r)))
        .is_some_and(|r| jv::truthy(r.get("length")) || jv::as_arr(Some(r)).is_some_and(|a| !a.is_empty()))
}
fn uses_sheet(part: &Value) -> bool {
    if jv::truthy(part.get("material")) {
        return true;
    }
    let Some(d) = part.get("drawing").filter(|d| jv::truthy(Some(d))) else { return false };
    let sheet = d.get("sheet");
    (jv::truthy(sheet) && !jv::is_str(sheet, "a3"))
        || jv::truthy(d.get("title"))
        || jv::as_arr(d.get("revisions")).is_some_and(|a| !a.is_empty())
        || jv::truthy(d.get("holeTable"))
}
fn has_params_key(part: &Value) -> bool {
    part.get("params").filter(|p| jv::truthy(Some(p))).is_some_and(|p| {
        jv::as_arr(p.get("vars")).is_some_and(|a| !a.is_empty()) || jv::as_arr(p.get("links")).is_some_and(|a| !a.is_empty())
    })
}
fn uses_rib(part: &Value) -> bool {
    features(part).iter().any(|f| jv::s(f, "type") == "rib")
}
fn uses_thread(part: &Value) -> bool {
    features(part).iter().any(|f| jv::s(f, "type") == "thread" || (jv::s(f, "type") == "hole" && jv::truthy(f.get("modelled"))))
}
fn uses_sheet_metal(part: &Value) -> bool {
    features(part).iter().any(|f| rd::is_sheet_type(jv::s(f, "type")))
}

/// `fileVersion(part)`.
pub fn file_version(part: &Value) -> f64 {
    let studies = part
        .get("studies")
        .filter(|s| jv::truthy(Some(s)))
        .is_some_and(|s| s.as_object().is_some_and(|o| !o.is_empty()) || s.as_array().is_some_and(|a| !a.is_empty()));
    if studies
        || rd::uses_design(part)
        || rd::uses_surface(part)
        || rd::uses_bodies(part)
        || has_params_key(part)
        || uses_sheet(part)
        || uses_sheet_metal(part)
        || has_configs(part)
        || uses_rib(part)
        || uses_thread(part)
    {
        3.0
    } else {
        2.0
    }
}

/// `toJSON(part)`: `JSON.stringify(…, null, 1)` with params / configs only when they hold something.
pub fn to_json(part: &Value) -> String {
    let mut out = jv::Obj::new();
    if let Some(o) = part.as_object() {
        for (k, v) in o {
            if k != "params" && k != "configs" {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if has_params_key(part)
        && let Some(p) = part.get("params")
    {
        out.insert("params".into(), p.clone());
    }
    if has_configs(part)
        && let Some(c) = part.get("configs")
    {
        out.insert("configs".into(), c.clone());
    }
    out.insert("version".into(), jv::num(file_version(part)));
    jv::stringify_pretty(&Value::Object(out), 1)
}

// ---------------------------------------------------------------- reading untrusted files

fn num(v: Option<&Value>, lo: f64, hi: f64) -> bool {
    jv::finite(v).is_some_and(|x| x >= lo && x <= hi)
}

/// `readPart(raw)`: a clean part, or the message.
pub fn read_part(raw: &Value) -> Result<Value, String> {
    let parsed;
    let j = match raw {
        Value::String(s) => {
            parsed = jv::parse(s).ok_or_else(|| t("model.part.file.not_json", &[]))?;
            &parsed
        }
        v => v,
    };
    if !jv::truthy(Some(j)) || !jv::is_str(j.get("format"), FORMAT) {
        return Err(t("model.part.file.not_partcraft", &[]));
    }
    let ver = match jv::int(j.get("version")) {
        Some(v) if v >= 1.0 => v,
        _ => return Err(t("model.part.file.no_version", &[])),
    };
    if ver > VERSION {
        return Err(t("model.part.file.newer", &[("v", F::N(ver)), ("max", F::N(VERSION))]));
    }
    let Some(feats) = jv::as_arr(j.get("features")).filter(|a| a.len() <= 2000) else { return Err(t("model.part.file.no_features", &[])) };
    let name = js::text(j.get("name"), 120);
    let mut part = new_part(if name.is_empty() { "Part1" } else { &name });
    if num(j.get("resolution"), 8.0, 512.0) {
        jv::set(&mut part, "resolution", jv::num(js::round(jv::n(j, "resolution"))));
    }
    let mut ids: HashSet<String> = HashSet::new();
    let mut types: HashMap<String, String> = HashMap::new();
    let mut max_n: f64 = 0.0;
    let mut out_feats: Vec<Value> = Vec::new();
    for f in feats {
        let fid = f.get("id").and_then(Value::as_str);
        let ok = jv::truthy(Some(f))
            && fid.is_some_and(|i| re::feature_id(i) && !ids.contains(i))
            && f.get("type").and_then(Value::as_str).is_some_and(known_type);
        if !ok {
            return Err(t("model.part.file.feature_bad", &[]));
        }
        let fid = fid.unwrap_or("").to_string();
        let ty = jv::s(f, "type").to_string();
        let fname = {
            let n = js::text(f.get("name"), 60);
            if n.is_empty() { fid.clone() } else { n }
        };
        let mut g = jv::obj(vec![("id", fid.as_str().into()), ("type", ty.as_str().into()), ("name", fname.as_str().into())]);
        if jv::is_true(f.get("suppressed")) {
            jv::set(&mut g, "suppressed", true.into());
        }
        let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(t(key, &[("name", F::from(fname.as_str()))])) };
        let c = Ctx { ids: &ids, types: &types };
        let sketch_ref = |out: &Vec<Value>| -> Result<Value, String> {
            let sk = f.get("sketch");
            let ok = c.ids.contains(jv::as_str(sk).unwrap_or("\u{0}"))
                && out.iter().find(|x| jv::strict_eq(x.get("id"), sk)).is_some_and(|x| jv::s(x, "type") == "sketch");
            need(ok, "model.file.bad.its_sketch")?;
            Ok(sk.cloned().unwrap_or(Value::Null))
        };
        let in_list = |v: Option<&Value>, l: &[&str]| v.and_then(Value::as_str).is_some_and(|s| l.contains(&s));
        let set_all = |g: &mut Value, kv: Vec<(&str, Value)>| {
            for (k, v) in kv {
                jv::set(g, k, v);
            }
        };
        let fv = |k: &str| f.get(k).cloned().unwrap_or(Value::Null);
        match ty.as_str() {
            "sketch" => {
                let pl = rd::read_plane(f.get("plane"), &c, true)?;
                jv::set(&mut g, "plane", pl);
                let sk = sketch::read_sketch(f.get("sketch"))?;
                jv::set(&mut g, "sketch", sk);
            }
            "extrude" => {
                let sk = sketch_ref(&out_feats)?;
                jv::set(&mut g, "sketch", sk);
                need(in_list(f.get("op"), &["add", "cut", "intersect", "new"]), "model.file.bad.the_operation")?;
                need(
                    in_list(f.get("end"), &["blind", "mid", "through", "upto", "next", "vertex", "offset"]),
                    "model.file.bad.the_end_condition",
                )?;
                need(num(f.get("depth"), 1e-6, 1e6), "model.file.bad.the_depth")?;
                set_all(
                    &mut g,
                    vec![("op", fv("op")), ("end", fv("end")), ("depth", fv("depth")), ("reverse", jv::is_true(f.get("reverse")).into())],
                );
                rd::read_design(f, &mut g, &c)?;
            }
            "revolve" => {
                let sk = sketch_ref(&out_feats)?;
                jv::set(&mut g, "sketch", sk);
                need(in_list(f.get("op"), &["add", "cut", "intersect", "new"]), "model.file.bad.the_operation")?;
                need(num(f.get("angle"), 1e-3, 360.0), "model.file.bad.the_angle")?;
                need(
                    matches!(f.get("axis"), None | Some(Value::Null | Value::String(_) | Value::Object(_) | Value::Array(_))),
                    "model.file.bad.the_axis",
                )?;
                let axis = match f.get("axis") {
                    Some(a @ (Value::Object(_) | Value::Array(_))) => rd::read_dir(Some(a), &c, true)?,
                    Some(Value::String(s)) => Value::String(s.clone()),
                    _ => Value::Null,
                };
                set_all(&mut g, vec![("op", fv("op")), ("angle", fv("angle")), ("axis", axis)]);
            }
            "hole" => {
                let sk = sketch_ref(&out_feats)?;
                jv::set(&mut g, "sketch", sk);
                need(in_list(f.get("kind"), &rd::HOLE_KINDS), "model.file.bad.the_hole_type")?;
                for k in ["d", "depth", "cboreD", "cboreDepth"] {
                    if !num(f.get(k), 1e-6, 1e6) {
                        return Err(t("model.file.bad.field", &[("name", F::from(fname.as_str())), ("field", F::from(k))]));
                    }
                }
                set_all(
                    &mut g,
                    vec![
                        ("kind", fv("kind")),
                        ("d", fv("d")),
                        ("depth", fv("depth")),
                        ("through", jv::is_true(f.get("through")).into()),
                        ("cboreD", fv("cboreD")),
                        ("cboreDepth", fv("cboreDepth")),
                        ("reverse", jv::is_true(f.get("reverse")).into()),
                    ],
                );
                rd::read_design(f, &mut g, &c)?;
                rd::read_hole_thread(f, &mut g)?;
            }
            "lpattern" | "cpattern" => {
                let seeds_ok = jv::as_arr(f.get("seeds")).is_some_and(|a| a.iter().all(|s| s.as_str().is_some_and(|s| ids.contains(s))));
                need(seeds_ok, "model.file.bad.the_features_to_pattern")?;
                need(jv::int(f.get("count")).is_some_and(|n| (1.0..=1000.0).contains(&n)), "model.file.bad.the_count")?;
                if ty == "lpattern" {
                    need(num(f.get("spacing"), -1e6, 1e6), "model.file.bad.the_spacing")?;
                    let dir = rd::read_dir(f.get("dir"), &c, false)?;
                    set_all(
                        &mut g,
                        vec![
                            ("seeds", Value::Array(jv::arr(f.get("seeds")).to_vec())),
                            ("dir", dir),
                            ("count", fv("count")),
                            ("spacing", fv("spacing")),
                            ("reverse", jv::is_true(f.get("reverse")).into()),
                        ],
                    );
                } else {
                    need(num(f.get("angle"), 1e-3, 360.0), "model.file.bad.the_angle")?;
                    let axis = rd::read_dir(f.get("axis"), &c, false)?;
                    set_all(
                        &mut g,
                        vec![
                            ("seeds", Value::Array(jv::arr(f.get("seeds")).to_vec())),
                            ("axis", axis),
                            ("count", fv("count")),
                            ("angle", fv("angle")),
                            ("reverse", jv::is_true(f.get("reverse")).into()),
                        ],
                    );
                }
                rd::read_bodies(f, &mut g, &c)?;
            }
            "mirror" => {
                let body = jv::is_str(f.get("seeds"), "body");
                let ok = body || jv::as_arr(f.get("seeds")).is_some_and(|a| a.iter().all(|s| s.as_str().is_some_and(|s| ids.contains(s))));
                need(ok, "model.file.bad.what_to_mirror")?;
                let seeds = if body { "body".into() } else { Value::Array(jv::arr(f.get("seeds")).to_vec()) };
                let plane = rd::read_plane(f.get("plane"), &c, false)?;
                set_all(&mut g, vec![("seeds", seeds), ("plane", plane)]);
            }
            "fillet" | "chamfer" => {
                need(jv::as_arr(f.get("edges")).is_some_and(|a| a.len() <= 2000), "model.file.bad.its_edge_list")?;
                let mut edges = Vec::new();
                for e in jv::arr(f.get("edges")) {
                    let ok = jv::truthy(Some(e))
                        && jv::as_arr(e.get("faces")).is_some_and(|fs| {
                            fs.len() == 2
                                && fs.iter().all(|n| rd::good_name(Some(n)))
                                && fs.iter().all(|n| re::name_features(jv::as_str(Some(n)).unwrap_or("")).iter().all(|x| ids.contains(x)))
                        });
                    need(ok, "model.file.bad.an_edge_reference")?;
                    need(e.get("at").is_none() || rd::vec3(e.get("at")), "model.file.bad.an_edge_position")?;
                    let mut o = jv::obj(vec![("faces", Value::Array(jv::arr(e.get("faces")).to_vec()))]);
                    if jv::truthy(e.get("at")) {
                        jv::set(&mut o, "at", Value::Array(jv::arr(e.get("at")).to_vec()));
                    }
                    edges.push(o);
                }
                jv::set(&mut g, "edges", Value::Array(edges));
                let key = if ty == "fillet" { "r" } else { "d" };
                need(num(f.get(key), 1e-6, 1e5), if ty == "fillet" { "model.file.bad.the_radius" } else { "model.file.bad.the_distance" })?;
                jv::set(&mut g, key, fv(key));
                rd::read_design(f, &mut g, &c)?;
            }
            "form" | "patch" | "surfloft" | "stitch" | "thicken" | "offsetsurf" | "extendsurf" | "trimsurf" | "gordon" => {
                rd::read_surface(f, &mut g, &c)?
            }
            "plane" | "axis" | "point" | "sweep" | "loft" | "draft" => {
                rd::read_design(f, &mut g, &c)?;
            }
            "combine" | "movebody" | "scalebody" | "pathpattern" => {
                rd::read_bodies(f, &mut g, &c)?;
            }
            "sheetbase" | "sheetflange" => {
                rd::read_sheet(f, &mut g, &c)?;
            }
            "rib" => {
                rd::read_rib(f, &mut g, &c)?;
            }
            "thread" => {
                rd::read_thread(f, &mut g, &c)?;
            }
            "shell" => {
                need(jv::as_arr(f.get("faces")).is_some_and(|a| a.len() <= 500), "model.file.bad.its_face_list")?;
                let mut faces = Vec::new();
                for r in jv::arr(f.get("faces")) {
                    let ok = jv::truthy(Some(r))
                        && rd::good_name(r.get("name"))
                        && re::name_features(jv::s(r, "name")).iter().all(|x| ids.contains(x));
                    need(ok, "model.file.bad.a_face_reference")?;
                    need(r.get("at").is_none() || rd::vec3(r.get("at")), "model.file.bad.a_face_position")?;
                    let mut o = jv::obj(vec![("name", r.get("name").cloned().unwrap_or(Value::Null))]);
                    if jv::truthy(r.get("at")) {
                        jv::set(&mut o, "at", Value::Array(jv::arr(r.get("at")).to_vec()));
                    }
                    faces.push(o);
                }
                jv::set(&mut g, "faces", Value::Array(faces));
                need(num(f.get("t"), 1e-6, 1e5), "model.file.bad.the_thickness")?;
                set_all(&mut g, vec![("t", fv("t")), ("outward", jv::is_true(f.get("outward")).into())]);
            }
            "import" => {
                let ok = match f.get("step") {
                    Some(Value::String(s)) => js::len16(s) <= 60 * 1024 * 1024 && js::trim_start(s).starts_with("ISO-10303-21;"),
                    _ => false,
                };
                need(ok, "model.file.bad.its_step_data")?;
                set_all(
                    &mut g,
                    vec![
                        ("file", js::text(f.get("file"), 200).into()),
                        ("step", fv("step")),
                        ("up", if jv::is_str(f.get("up"), "y") { "y" } else { "z" }.into()),
                    ],
                );
            }
            _ => {}
        }
        ids.insert(fid.clone());
        types.insert(fid.clone(), ty.clone());
        max_n = js::max(max_n, js::str_num(fid.get(1..).unwrap_or("")));
        out_feats.push(g);
    }
    jv::set(&mut part, "features", Value::Array(out_feats));
    jv::set(&mut part, "seq", jv::num(max_n + 1.0));
    if !jv::nullish(j.get("drawing")) {
        let d = read_drawing(j.get("drawing"))?;
        jv::set(&mut part, "drawing", d);
    }
    if !jv::nullish(j.get("studies")) {
        read_studies(j.get("studies"))?;
    }
    if !jv::nullish(j.get("material")) {
        let m = bom::read_material(j.get("material"))?;
        jv::set(&mut part, "material", m.unwrap_or(Value::Null));
    }
    if let Some(bi) = j.get("bodyInfo").filter(|v| !v.is_null())
        && let Some(v) = rd::read_body_info(bi, &ids)?
    {
        jv::set(&mut part, "bodyInfo", v);
    }
    if let Some(raw) = j.get("params").filter(|v| !v.is_null())
        && let Some(p) = params_part::read_part_params(raw, &part)?
    {
        jv::set(&mut part, "params", p.to_json());
    }
    if let Some(raw) = j.get("configs").filter(|v| !v.is_null()) {
        let pp = part.get("params").map(crate::params::Params::from_json);
        if let Some(cf) = configs::read_configs(Some(raw), pp.as_ref())? {
            jv::set(&mut part, "configs", cf.to_json());
        }
    }
    match j.get("rollback") {
        None | Some(Value::Null) => jv::set(&mut part, "rollback", Value::Null),
        Some(v) => {
            if let Some(r) = jv::int(Some(v))
                && r >= 0.0
                && r < js::f(features(&part).len())
            {
                jv::set(&mut part, "rollback", jv::num(r));
            }
        }
    }
    Ok(part)
}

/// `readStudies(s, part, ids)` with no study reader registered.
fn read_studies(s: Option<&Value>) -> Result<(), String> {
    let Some(o) = s.and_then(Value::as_object) else { return Err(t("model.part.file.studies_bad", &[])) };
    if o.is_empty() {
        return Ok(());
    }
    if o.len() > 32 {
        return Err(t("model.part.file.too_many_studies", &[]));
    }
    if let Some((k, _)) = o.iter().next() {
        return Err(t("model.part.file.unknown_study", &[("key", F::S(js::slice16(&js::strip_ctrl(k), 40)))]));
    }
    Ok(())
}

/// `readDrawing(d)`.
pub fn read_drawing(d: Option<&Value>) -> Result<Value, String> {
    let Some(d) = d.filter(|d| jv::truthy(Some(d)) && jv::is_obj_like(Some(d))) else { return Err(t("model.part.file.drawing_bad", &[])) };
    let mut out = jv::obj(vec![("section", Value::Null), ("detail", Value::Null), ("notes", Value::Array(Vec::new()))]);
    if let Some(s) = d.get("section").filter(|s| jv::truthy(Some(s))) {
        let of_ok = s.get("of").and_then(Value::as_str).is_some_and(|o| DRAWING_VIEWS.contains(&o));
        if !of_ok || (s.get("at").is_some() && !num(s.get("at"), -1e9, 1e9)) {
            return Err(t("model.part.file.section_bad", &[]));
        }
        let mut sec = jv::obj(vec![("of", s.get("of").cloned().unwrap_or(Value::Null))]);
        if s.get("at").is_some() {
            jv::set(&mut sec, "at", s.get("at").cloned().unwrap_or(Value::Null));
        }
        jv::set(&mut out, "section", sec);
    }
    if let Some(dt) = d.get("detail").filter(|s| jv::truthy(Some(s))) {
        let of_ok = dt.get("of").and_then(Value::as_str).is_some_and(|o| DRAWING_VIEWS.contains(&o) || o == "section");
        let at_ok = jv::as_arr(dt.get("at")).is_some_and(|a| a.len() == 2 && a.iter().all(|v| num(Some(v), -1e9, 1e9)));
        let sc_ok = jv::as_num(dt.get("scale")).is_some_and(|s| DETAIL_SCALES.contains(&s));
        if !of_ok || !at_ok || !num(dt.get("r"), 1e-3, 1e6) || !sc_ok {
            return Err(t("model.part.file.detail_bad", &[]));
        }
        jv::set(
            &mut out,
            "detail",
            jv::obj(vec![
                ("of", dt.get("of").cloned().unwrap_or(Value::Null)),
                ("at", Value::Array(jv::arr(dt.get("at")).to_vec())),
                ("r", dt.get("r").cloned().unwrap_or(Value::Null)),
                ("scale", dt.get("scale").cloned().unwrap_or(Value::Null)),
            ]),
        );
    }
    if d.get("notes").is_some() {
        let notes = gdt::read_notes(d.get("notes"))?;
        jv::set(&mut out, "notes", Value::Array(notes));
    }
    if let Some(s) = d.get("sheet").filter(|v| !v.is_null()) {
        let k = s.as_str().filter(|k| SHEET_KEYS.contains(k));
        let Some(k) = k else { return Err(t("file.drawing.sheet_bad", &[])) };
        if k != "a3" {
            jv::set(&mut out, "sheet", k.into());
        }
    }
    if let Some(tt) = d.get("title").filter(|v| !v.is_null()) {
        let Some(to) = tt.as_object() else { return Err(t("file.drawing.title_bad", &[])) };
        let mut o = jv::obj(vec![]);
        for (k, max) in TITLE_FIELDS {
            let v = to.get(k);
            match v {
                None | Some(Value::Null) => continue,
                Some(Value::String(s)) if s.is_empty() => continue,
                Some(Value::String(s)) => {
                    let c = js::trim(&js::slice16(&js::strip_ctrl(s), max)).to_string();
                    if k == "date" && !c.is_empty() && !re::ymd(&c) {
                        return Err(t("file.drawing.date_bad", &[]));
                    }
                    if !c.is_empty() {
                        jv::set(&mut o, k, c.into());
                    }
                }
                Some(_) => return Err(t("file.drawing.title_bad", &[])),
            }
        }
        if o.as_object().is_some_and(|m| !m.is_empty()) {
            jv::set(&mut out, "title", o);
        }
    }
    if let Some(h) = d.get("holeTable").filter(|v| !v.is_null()) {
        let ok = jv::is_obj_like(Some(h)) && h.get("view").and_then(Value::as_str).is_some_and(|v| DRAWING_VIEWS.contains(&v));
        if !ok {
            return Err(t("file.drawing.holes_bad", &[]));
        }
        jv::set(&mut out, "holeTable", jv::obj(vec![("view", h.get("view").cloned().unwrap_or(Value::Null))]));
    }
    if let Some(r) = d.get("revisions").filter(|v| !v.is_null()) {
        let Some(list) = r.as_array().filter(|a| a.len() <= MAX_REVISIONS) else { return Err(t("file.drawing.revisions_bad", &[])) };
        let mut rs = Vec::new();
        for x in list {
            let fields_ok = ["rev", "desc", "date", "by"].iter().all(|k| matches!(x.get(*k), None | Some(Value::String(_))));
            if !jv::truthy(Some(x)) || !jv::is_obj_like(Some(x)) || !fields_ok {
                return Err(t("file.drawing.revisions_bad", &[]));
            }
            let cl = |k: &str, m: usize| js::trim(&js::text(x.get(k), m)).to_string();
            let date = cl("date", 10);
            if !date.is_empty() && !re::ymd(&date) {
                return Err(t("file.drawing.date_bad", &[]));
            }
            rs.push(jv::obj(vec![
                ("rev", cl("rev", 8).into()),
                ("desc", cl("desc", 120).into()),
                ("date", date.into()),
                ("by", cl("by", 40).into()),
            ]));
        }
        if !rs.is_empty() {
            jv::set(&mut out, "revisions", Value::Array(rs));
        }
    }
    Ok(out)
}
