//! The untrusted-file readers of the feature types part.js hands off: design-features.js (`makeReaders`,
//! `readDesign`, `designDeps`, `usesDesign`, ISO coarse threads), bodies.js (`readBodies`, `bodiesDeps`, `readBodyInfo`,
//! `usesBodies`), surface.js (`readSurface`, `surfaceDeps`, `usesSurface`), sheetmetal.js (`readSheet`, `sheetDeps`),
//! rib.js (`readRib`) and threads.js (`readThread`, `readHoleThread`). Every reader returns a clean copy or the
//! message; each check comes in the JS's order, so the first problem found is the one named.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::re;
use crate::sketch::proto_key;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// part.js's reader context: the feature ids read so far and their types.
pub struct Ctx<'a> {
    pub ids: &'a HashSet<String>,
    pub types: &'a HashMap<String, String>,
    /// assembly.js: `{ has: (id) => /^f\d{1,9}$/.test(id) }` — any feature id is let through
    pub any_id: bool,
}
impl Ctx<'_> {
    fn has(&self, v: Option<&Value>) -> bool {
        v.and_then(Value::as_str).is_some_and(|s| self.has_str(s))
    }
    fn has_str(&self, s: &str) -> bool {
        if self.any_id { re::feature_id(s) } else { self.ids.contains(s) }
    }
    fn ty(&self, v: Option<&Value>) -> Option<&str> {
        v.and_then(Value::as_str).and_then(|s| self.types.get(s)).map(String::as_str)
    }
}

fn num(v: Option<&Value>, lo: f64, hi: f64) -> bool {
    jv::finite(v).is_some_and(|x| x >= lo && x <= hi)
}
fn num_d(v: Option<&Value>) -> bool {
    num(v, -1e9, 1e9)
}
/// part.js / design-features.js `vec3`: three numbers in ±1e9, |v| ≤ 1e9.
pub fn vec3(v: Option<&Value>) -> bool {
    jv::as_arr(v).is_some_and(|a| {
        a.len() == 3
            && a.iter().all(|x| num_d(Some(x)))
            && js::hypot(&a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect::<Vec<_>>()) <= 1e9
    })
}
/// surface.js `vec3` (±1e7).
fn vec3s(v: Option<&Value>) -> bool {
    jv::as_arr(v).is_some_and(|a| a.len() == 3 && a.iter().all(|x| num(Some(x), -1e7, 1e7)))
}
/// `goodName(n)`.
pub fn good_name(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| js::len16(s) <= 400 && re::face_name(s))
}
fn ent_ok(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty() && s.len() <= 40 && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'-')))
}
fn arr_copy(v: Option<&Value>) -> Value {
    Value::Array(jv::arr(v).to_vec())
}
fn err(key: &str, name: &str) -> String {
    t(key, &[("name", F::from(name))])
}
fn wrap<T>(r: Result<T, String>, key: &str, name: &str) -> Result<T, String> {
    r.map_err(|e| t(key, &[("name", F::from(name)), ("problem", F::S(js::drop_final_dot(&e)))]))
}

// ---------------------------------------------------------------- PlaneRef / DirRef (part.js)

/// `readPlane(p, ids, types)` (types: Some = check `ref` targets are planes).
pub fn read_plane(p: Option<&Value>, c: &Ctx, with_types: bool) -> Result<Value, String> {
    let Some(p) = p.filter(|p| jv::truthy(Some(p)) && jv::is_obj_like(Some(p))) else {
        return Err(t("model.part.file.plane_missing", &[]));
    };
    let kind = p.get("kind").and_then(Value::as_str).unwrap_or("\u{0}");
    if kind == "ref" && c.has(p.get("feature")) && (!with_types || c.ty(p.get("feature")) == Some("plane")) {
        return Ok(jv::obj(vec![("kind", "ref".into()), ("feature", p.get("feature").cloned().unwrap_or(Value::Null))]));
    }
    if kind == "base" && matches!(p.get("name").and_then(Value::as_str), Some("front" | "top" | "right")) {
        return Ok(jv::obj(vec![("kind", "base".into()), ("name", p.get("name").cloned().unwrap_or(Value::Null))]));
    }
    let face = p.get("face").and_then(Value::as_str);
    if kind == "face" && c.has(p.get("feature")) && matches!(face, Some("start" | "end" | "side")) {
        let mut r = jv::obj(vec![
            ("kind", "face".into()),
            ("feature", p.get("feature").cloned().unwrap_or(Value::Null)),
            ("face", face.unwrap_or("").into()),
        ]);
        if face == Some("side") {
            match p.get("entity") {
                Some(Value::String(e)) if js::len16(e) <= 40 => jv::set(&mut r, "entity", e.as_str().into()),
                _ => return Err(t("model.part.file.face_bad", &[])),
            }
        }
        return Ok(r);
    }
    if kind == "face" && face == Some("named") && good_name(p.get("name")) {
        let name = jv::s(p, "name");
        let first = re::name_features(name).into_iter().next();
        if let Some(f0) = first.filter(|f| c.has_str(f)) {
            return Ok(jv::obj(vec![("kind", "face".into()), ("feature", f0.into()), ("face", "named".into()), ("name", name.into())]));
        }
    }
    if kind == "fixed" && vec3(p.get("o")) && vec3(p.get("n")) {
        let n: Vec<f64> = jv::arr(p.get("n")).iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
        if js::hypot(&n) > 1e-9 {
            return Ok(jv::obj(vec![("kind", "fixed".into()), ("o", arr_copy(p.get("o"))), ("n", arr_copy(p.get("n")))]));
        }
    }
    Err(t("model.part.file.plane_bad", &[]))
}

/// `readDir(d, ids, types)`.
pub fn read_dir(d: Option<&Value>, c: &Ctx, with_types: bool) -> Result<Value, String> {
    let Some(d) = d.filter(|d| jv::truthy(Some(d)) && jv::is_obj_like(Some(d))) else { return Err(t("model.part.file.dir_missing", &[])) };
    let kind = d.get("kind").and_then(Value::as_str).unwrap_or("\u{0}");
    if kind == "ref" && c.has(d.get("feature")) && (!with_types || matches!(c.ty(d.get("feature")), Some("axis" | "plane"))) {
        return Ok(jv::obj(vec![("kind", "ref".into()), ("feature", d.get("feature").cloned().unwrap_or(Value::Null))]));
    }
    if kind == "world" && matches!(d.get("axis").and_then(Value::as_str), Some("x" | "y" | "z")) {
        return Ok(jv::obj(vec![("kind", "world".into()), ("axis", d.get("axis").cloned().unwrap_or(Value::Null))]));
    }
    if kind == "line" && c.has(d.get("sketch")) && d.get("entity").and_then(Value::as_str).is_some_and(|e| js::len16(e) <= 40) {
        return Ok(jv::obj(vec![
            ("kind", "line".into()),
            ("sketch", d.get("sketch").cloned().unwrap_or(Value::Null)),
            ("entity", d.get("entity").cloned().unwrap_or(Value::Null)),
        ]));
    }
    if kind == "axis" && c.has(d.get("feature")) {
        return Ok(jv::obj(vec![("kind", "axis".into()), ("feature", d.get("feature").cloned().unwrap_or(Value::Null))]));
    }
    if kind == "fixed" && vec3(d.get("o")) && vec3(d.get("d")) {
        let v: Vec<f64> = jv::arr(d.get("d")).iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
        if js::hypot(&v) > 1e-9 {
            return Ok(jv::obj(vec![("kind", "fixed".into()), ("o", arr_copy(d.get("o"))), ("d", arr_copy(d.get("d")))]));
        }
    }
    Err(t("model.part.file.dir_bad", &[]))
}

// ---------------------------------------------------------------- design-features.js

pub const HOLE_KINDS: [&str; 4] = ["simple", "cbore", "csink", "tapped"];
const PLANE_MODES: [&str; 8] = ["offset", "angle", "3pt", "mid", "tangent", "normal", "edgept", "midnormal"];
const AXIS_MODES: [&str; 5] = ["2pt", "cyl", "planes", "edge", "normal"];
const POINT_MODES: [&str; 6] = ["at", "center", "xyz", "mid", "meet", "planes3"];
const OPS: [&str; 4] = ["add", "cut", "intersect", "new"];
const ORIENTS: [&str; 5] = ["frenet", "corrected", "fixed", "binormal", "guide"];

/// ISO metric coarse threads M2–M24: (size, nominal d, pitch, tap drill).
pub const ISO_COARSE: [(&str, f64, f64, f64); 15] = [
    ("M2", 2.0, 0.4, 1.6),
    ("M2.5", 2.5, 0.45, 2.05),
    ("M3", 3.0, 0.5, 2.5),
    ("M4", 4.0, 0.7, 3.3),
    ("M5", 5.0, 0.8, 4.2),
    ("M6", 6.0, 1.0, 5.0),
    ("M8", 8.0, 1.25, 6.8),
    ("M10", 10.0, 1.5, 8.5),
    ("M12", 12.0, 1.75, 10.2),
    ("M14", 14.0, 2.0, 12.0),
    ("M16", 16.0, 2.0, 14.0),
    ("M18", 18.0, 2.5, 15.5),
    ("M20", 20.0, 2.5, 17.5),
    ("M22", 22.0, 2.5, 19.5),
    ("M24", 24.0, 3.0, 21.0),
];
/// `ISO_COARSE[size]` → [d, pitch, tap drill].
pub fn iso_coarse(size: &str) -> Option<[f64; 3]> {
    ISO_COARSE.iter().find(|r| r.0 == size).map(|r| [r.1, r.2, r.3])
}

/// design-features.js `threadCallout(f, depth)`.
pub fn hole_callout(f: &Value, depth: Option<f64>) -> String {
    let Some(tt) = iso_coarse(jv::s(f, "size")) else { return String::new() };
    let fmt = |v: f64| js::fmt_fixed(v, 3);
    let size = js::str_of(f.get("size"));
    if jv::truthy(f.get("through")) && !jv::truthy(f.get("threadDepth")) {
        return t("model.design_features.callout.through", &[("size", F::S(size)), ("pitch", F::S(fmt(tt[1])))]);
    }
    let td = jv::as_num(f.get("threadDepth")).filter(|x| *x != 0.0 && !x.is_nan());
    let d = td.or(depth.filter(|x| *x != 0.0 && !x.is_nan())).unwrap_or_else(|| jv::n(f, "depth"));
    t("model.design_features.callout.deep", &[("size", F::S(size)), ("pitch", F::S(fmt(tt[1]))), ("depth", F::S(fmt(d)))])
}

/// `refDeps(r, add, nameFeatures)`.
fn ref_deps(r: Option<&Value>, add: &mut dyn FnMut(&str), depth: u32) {
    let Some(r) = r.filter(|r| jv::is_obj_like(Some(r))) else { return };
    if depth > 16 {
        return;
    }
    let kind = r.get("kind").and_then(Value::as_str);
    let has_kind = jv::truthy(r.get("kind"));
    let feat = |add: &mut dyn FnMut(&str)| {
        if let Some(s) = r.get("feature").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            add(s);
        }
    };
    if kind == Some("ref") || (kind == Some("face") && jv::truthy(r.get("feature"))) || kind == Some("axis") {
        feat(add);
    }
    if kind == Some("face")
        && let Some(n) = r.get("name").filter(|n| jv::truthy(Some(n)))
    {
        for x in re::name_features(&js::str_of(Some(n))) {
            add(&x);
        }
    }
    if matches!(kind, Some("line" | "sketch"))
        && let Some(s) = r.get("sketch").and_then(Value::as_str).filter(|s| !s.is_empty())
    {
        add(s);
    }
    if kind == Some("vertex") {
        ref_deps(r.get("edge"), add, depth.saturating_add(1));
    }
    if let Some(fs) = jv::as_arr(r.get("faces"))
        && fs.iter().all(Value::is_string)
    {
        for n in fs {
            for x in re::name_features(jv::as_str(Some(n)).unwrap_or("")) {
                add(&x);
            }
        }
    }
    if let Some(n) = r.get("name").and_then(Value::as_str)
        && !has_kind
    {
        for x in re::name_features(n) {
            add(&x);
        }
    }
    if !has_kind && let Some(s) = r.get("sketch").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        add(s);
    }
    if !has_kind && jv::truthy(r.get("edge")) {
        ref_deps(r.get("edge"), add, depth.saturating_add(1));
    }
    if !has_kind && jv::truthy(r.get("face")) {
        ref_deps(r.get("face"), add, depth.saturating_add(1));
    }
    if !has_kind && jv::truthy(r.get("point")) {
        ref_deps(r.get("point"), add, depth.saturating_add(1));
    }
}

/// `designDeps(f)`.
pub fn design_deps(f: &Value) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    let mut add = |x: &str| {
        if !x.is_empty() && !d.iter().any(|y| y == x) {
            d.push(x.to_string());
        }
    };
    let each = |v: Option<&Value>, add: &mut dyn FnMut(&str)| match v {
        Some(Value::Array(a)) => {
            for r in a {
                ref_deps(Some(r), add, 0);
            }
        }
        other => ref_deps(other, add, 0),
    };
    for k in ["planes", "points", "path", "guide", "sections", "rails"] {
        each(f.get(k), &mut add);
    }
    for k in ["face", "edge", "upto", "vertex", "neutral", "pull", "binormal"] {
        each(f.get(k), &mut add);
    }
    let ty = jv::s(f, "type");
    if matches!(ty, "plane" | "axis" | "point") {
        each(f.get("axis"), &mut add);
    }
    if ty == "revolve" && f.get("axis").is_some_and(|a| jv::truthy(Some(a)) && jv::is_obj_like(Some(a))) {
        each(f.get("axis"), &mut add);
    }
    d
}

/// `makeReaders(c).edgeRef`.
pub fn edge_ref(e: Option<&Value>, c: &Ctx) -> Result<Value, String> {
    let known = |n: &Value| good_name(Some(n)) && re::name_features(jv::as_str(Some(n)).unwrap_or("")).iter().all(|x| c.ids.contains(x));
    let ok = e.filter(|e| jv::truthy(Some(e))).is_some_and(|e| {
        jv::as_arr(e.get("faces")).is_some_and(|fs| fs.len() == 2 && fs.iter().all(known)) && (e.get("at").is_none() || vec3(e.get("at")))
    });
    let Some(e) = e.filter(|_| ok) else { return Err(t("model.design_features.ref.an_edge_reference_is_bad", &[])) };
    let mut o = jv::obj(vec![("faces", arr_copy(e.get("faces")))]);
    if jv::truthy(e.get("at")) {
        jv::set(&mut o, "at", arr_copy(e.get("at")));
    }
    Ok(o)
}
/// `makeReaders(c).faceRef`.
pub fn face_ref(r: Option<&Value>, c: &Ctx) -> Result<Value, String> {
    let ok = r.filter(|r| jv::truthy(Some(r))).is_some_and(|r| {
        good_name(r.get("name"))
            && re::name_features(jv::s(r, "name")).iter().all(|x| c.ids.contains(x))
            && (r.get("at").is_none() || vec3(r.get("at")))
    });
    let Some(r) = r.filter(|_| ok) else { return Err(t("model.design_features.ref.a_face_reference_is_bad", &[])) };
    let mut o = jv::obj(vec![("name", r.get("name").cloned().unwrap_or(Value::Null))]);
    if jv::truthy(r.get("at")) {
        jv::set(&mut o, "at", arr_copy(r.get("at")));
    }
    Ok(o)
}
/// `makeReaders(c).pointRef`.
pub fn point_ref(p: Option<&Value>, c: &Ctx) -> Result<Value, String> {
    let Some(p) = p.filter(|p| jv::truthy(Some(p)) && jv::is_obj_like(Some(p))) else {
        return Err(t("model.design_features.ref.a_point_reference_is_missing", &[]));
    };
    let kind = p.get("kind").and_then(Value::as_str).unwrap_or("\u{0}");
    if kind == "sketch" && c.ty(p.get("sketch")) == Some("sketch") && ent_ok(p.get("entity")) {
        return Ok(jv::obj(vec![
            ("kind", "sketch".into()),
            ("sketch", p.get("sketch").cloned().unwrap_or(Value::Null)),
            ("entity", p.get("entity").cloned().unwrap_or(Value::Null)),
        ]));
    }
    if kind == "vertex" && vec3(p.get("p")) {
        let e = edge_ref(p.get("edge"), c)?;
        return Ok(jv::obj(vec![("kind", "vertex".into()), ("edge", e), ("p", arr_copy(p.get("p")))]));
    }
    if kind == "ref" && c.ty(p.get("feature")) == Some("point") {
        return Ok(jv::obj(vec![("kind", "ref".into()), ("feature", p.get("feature").cloned().unwrap_or(Value::Null))]));
    }
    if kind == "fixed" && vec3(p.get("p")) {
        return Ok(jv::obj(vec![("kind", "fixed".into()), ("p", arr_copy(p.get("p")))]));
    }
    Err(t("model.design_features.ref.a_point_reference_is_bad", &[]))
}
/// `makeReaders(c).pathItem`.
pub fn path_item(it: Option<&Value>, c: &Ctx) -> Result<Value, String> {
    if let Some(it) = it.filter(|i| jv::truthy(Some(i)))
        && let Some(sk) = it.get("sketch").and_then(Value::as_str)
    {
        if c.types.get(sk).map(String::as_str) != Some("sketch") {
            return Err(t("model.design_features.ref.a_path_sketch_is_bad", &[]));
        }
        let ents = it.get("entities");
        if !jv::nullish(ents) && !jv::as_arr(ents).is_some_and(|a| a.len() <= 500 && a.iter().all(|e| ent_ok(Some(e)))) {
            return Err(t("model.design_features.ref.a_path_curve_list_is_bad", &[]));
        }
        let mut o = jv::obj(vec![("sketch", sk.into())]);
        if jv::as_arr(ents).is_some() {
            jv::set(&mut o, "entities", arr_copy(ents));
        }
        return Ok(o);
    }
    if let Some(it) = it.filter(|i| jv::truthy(Some(i)))
        && jv::truthy(it.get("edge"))
    {
        return Ok(jv::obj(vec![("edge", edge_ref(it.get("edge"), c)?)]));
    }
    Err(t("model.design_features.ref.a_path_piece_is_bad", &[]))
}
/// `makeReaders(c).list`.
fn list(v: Option<&Value>, max: usize, key: &str, f: &mut dyn FnMut(&Value) -> Result<Value, String>) -> Result<Value, String> {
    let Some(a) = jv::as_arr(v).filter(|a| a.len() <= max) else { return Err(t(key, &[])) };
    let mut out = Vec::with_capacity(a.len());
    for x in a {
        out.push(f(x)?);
    }
    Ok(Value::Array(out))
}

fn opt(v: Option<&Value>, f: &dyn Fn(Option<&Value>) -> Result<Value, String>, key: &str, name: &str) -> Result<Value, String> {
    if jv::nullish(v) {
        return Ok(Value::Null);
    }
    wrap(f(v), key, name)
}

fn in_list(v: Option<&Value>, l: &[&str]) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| l.contains(&s))
}

/// `readDesign(f, g, c)` → handled the type.
pub fn read_design(f: &Value, g: &mut Value, c: &Ctx) -> Result<bool, String> {
    let name = js::str_of(g.get("name"));
    let name = name.as_str();
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, name)) };
    let empty = Value::Array(Vec::new());
    let planes = |n: usize| -> Result<Value, String> {
        let src = f.get("planes").filter(|v| !v.is_null()).unwrap_or(&empty);
        wrap(list(Some(src), n, "model.file.list.the_plane_list", &mut |p| read_plane(Some(p), c, true)), "model.file.in.its_planes", name)
    };
    let points = |n: usize| -> Result<Value, String> {
        let src = f.get("points").filter(|v| !v.is_null()).unwrap_or(&empty);
        wrap(list(Some(src), n, "model.file.list.the_point_list", &mut |p| point_ref(Some(p), c)), "model.file.in.its_points", name)
    };
    let dirf = |a: Option<&Value>| read_dir(a, c, true);
    let facef = |a: Option<&Value>| face_ref(a, c);
    let edgef = |a: Option<&Value>| edge_ref(a, c);
    let ty = jv::s(f, "type");
    let or0 = |v: Option<&Value>, d: f64| v.filter(|x| !x.is_null()).cloned().unwrap_or_else(|| jv::num(d));
    match ty {
        "plane" => {
            need(in_list(f.get("mode"), &PLANE_MODES), "model.file.bad.the_plane_type")?;
            need(num_d(Some(&or0(f.get("d"), 0.0))), "model.file.bad.the_distance")?;
            need(num(Some(&or0(f.get("angle"), 0.0)), -360.0, 360.0), "model.file.bad.the_angle")?;
            let pl = planes(2)?;
            let ax = opt(f.get("axis"), &dirf, "model.file.in.its_axis", name)?;
            let pts = points(3)?;
            let fc = opt(f.get("face"), &facef, "model.file.in.its_face", name)?;
            let ed = opt(f.get("edge"), &edgef, "model.file.in.its_edge", name)?;
            for (k, v) in [
                ("mode", f.get("mode").cloned().unwrap_or(Value::Null)),
                ("planes", pl),
                ("d", or0(f.get("d"), 0.0)),
                ("angle", or0(f.get("angle"), 0.0)),
                ("axis", ax),
                ("points", pts),
                ("face", fc),
                ("edge", ed),
                ("flip", jv::is_true(f.get("flip")).into()),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "axis" => {
            need(in_list(f.get("mode"), &AXIS_MODES), "model.file.bad.the_axis_type")?;
            let pl = planes(2)?;
            let pts = points(2)?;
            let fc = opt(f.get("face"), &facef, "model.file.in.its_face", name)?;
            let ed = opt(f.get("edge"), &edgef, "model.file.in.its_edge", name)?;
            for (k, v) in
                [("mode", f.get("mode").cloned().unwrap_or(Value::Null)), ("planes", pl), ("points", pts), ("face", fc), ("edge", ed)]
            {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "point" => {
            need(in_list(f.get("mode"), &POINT_MODES), "model.file.bad.the_point_type")?;
            need(f.get("p").is_none() || vec3(f.get("p")), "model.file.bad.the_coordinates")?;
            let pts = points(2)?;
            let ed = opt(f.get("edge"), &edgef, "model.file.in.its_edge", name)?;
            let p = if jv::truthy(f.get("p")) { arr_copy(f.get("p")) } else { jv::nums(&[0.0, 0.0, 0.0]) };
            for (k, v) in [("mode", f.get("mode").cloned().unwrap_or(Value::Null)), ("points", pts), ("edge", ed), ("p", p)] {
                jv::set(g, k, v);
            }
            if !jv::nullish(f.get("planes")) {
                let pl = planes(3)?;
                jv::set(g, "planes", pl);
            }
            if !jv::nullish(f.get("axis")) {
                let ax = opt(f.get("axis"), &dirf, "model.file.in.its_axis", name)?;
                jv::set(g, "axis", ax);
            }
            if !jv::nullish(f.get("face")) {
                let fc = opt(f.get("face"), &facef, "model.file.in.its_face", name)?;
                jv::set(g, "face", fc);
            }
            if jv::is_true(f.get("flip")) {
                jv::set(g, "flip", true.into());
            }
            Ok(true)
        }
        "sweep" => {
            need(c.ty(f.get("sketch")) == Some("sketch"), "model.file.bad.its_profile_sketch")?;
            need(in_list(f.get("op"), &OPS), "model.file.bad.the_operation")?;
            need(in_list(f.get("orient"), &ORIENTS), "model.file.bad.the_orientation")?;
            need(num(Some(&or0(f.get("scale"), 1.0)), 1e-3, 1e3), "model.file.bad.the_end_scale")?;
            let path =
                wrap(list(f.get("path"), 200, "model.file.list.the_path", &mut |p| path_item(Some(p), c)), "model.file.in.its_path", name)?;
            let bin = opt(f.get("binormal"), &dirf, "model.file.in.its_direction", name)?;
            let guide = opt(
                f.get("guide"),
                &|v| list(v, 200, "model.file.list.the_guide", &mut |p| path_item(Some(p), c)),
                "model.file.in.its_guide",
                name,
            )?;
            for (k, v) in [
                ("sketch", f.get("sketch").cloned().unwrap_or(Value::Null)),
                ("path", path),
                ("op", f.get("op").cloned().unwrap_or(Value::Null)),
                ("orient", f.get("orient").cloned().unwrap_or(Value::Null)),
                ("binormal", bin),
                ("guide", guide),
                ("scale", or0(f.get("scale"), 1.0)),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "loft" => {
            need(in_list(f.get("op"), &OPS), "model.file.bad.the_operation")?;
            let secs = wrap(
                list(f.get("sections"), 100, "model.file.list.the_section_list", &mut |s| {
                    if jv::truthy(Some(s))
                        && let Some(sk) = s.get("sketch").and_then(Value::as_str)
                    {
                        if c.types.get(sk).map(String::as_str) != Some("sketch") {
                            return Err(t("model.design_features.ref.a_section_sketch_is_bad", &[]));
                        }
                        return Ok(jv::obj(vec![("sketch", sk.into())]));
                    }
                    if jv::truthy(Some(s)) && jv::truthy(s.get("face")) {
                        return Ok(jv::obj(vec![("face", face_ref(s.get("face"), c)?)]));
                    }
                    if jv::truthy(Some(s)) && jv::truthy(s.get("point")) {
                        return Ok(jv::obj(vec![("point", point_ref(s.get("point"), c)?)]));
                    }
                    Err(t("model.design_features.ref.a_section_is_bad", &[]))
                }),
                "model.file.in.its_sections",
                name,
            )?;
            jv::set(g, "sections", secs);
            jv::set(g, "op", f.get("op").cloned().unwrap_or(Value::Null));
            jv::set(g, "ruled", jv::is_true(f.get("ruled")).into());
            if !jv::nullish(f.get("rails")) {
                let r = wrap(
                    list(f.get("rails"), 200, "model.file.list.the_rail_list", &mut |p| path_item(Some(p), c)),
                    "model.file.in.its_guide_rails",
                    name,
                )?;
                if !jv::arr(Some(&r)).is_empty() {
                    jv::set(g, "rails", r);
                }
            }
            if f.get("railTol").is_some() {
                need(num(f.get("railTol"), 1e-7, 10.0), "model.file.bad.the_rail_tolerance")?;
                jv::set(g, "railTol", f.get("railTol").cloned().unwrap_or(Value::Null));
            }
            Ok(true)
        }
        "draft" => {
            need(num(f.get("angle"), -89.0, 89.0), "model.file.bad.the_angle")?;
            let faces = wrap(
                list(f.get("faces"), 500, "model.file.list.the_face_list", &mut |r| face_ref(Some(r), c)),
                "model.file.in.its_faces",
                name,
            )?;
            let neutral = wrap(read_plane(f.get("neutral"), c, true), "model.file.in.its_neutral_plane", name)?;
            let pull = opt(f.get("pull"), &dirf, "model.file.in.its_pull_direction", name)?;
            for (k, v) in
                [("faces", faces), ("neutral", neutral), ("pull", pull), ("angle", f.get("angle").cloned().unwrap_or(Value::Null))]
            {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "extrude" => {
            need(f.get("offset").is_none() || num(f.get("offset"), -1e6, 1e6), "model.file.bad.the_offset")?;
            need(f.get("depth2").is_none() || num(f.get("depth2"), 1e-6, 1e6), "model.file.bad.the_second_depth")?;
            need(f.get("taper").is_none() || num(f.get("taper"), -89.0, 89.0), "model.file.bad.the_taper_angle")?;
            let regions_ok = match f.get("regions") {
                None | Some(Value::Null) => true,
                Some(Value::Array(a)) => {
                    a.len() <= 200
                        && a.iter().all(|p| jv::as_arr(Some(p)).is_some_and(|q| q.len() == 2 && q.iter().all(|v| num_d(Some(v)))))
                }
                _ => false,
            };
            need(regions_ok, "model.file.bad.the_picked_regions")?;
            let upto = opt(f.get("upto"), &|p| read_plane(p, c, true), "model.file.in.the_face_it_goes_up_to", name)?;
            let vertex = opt(f.get("vertex"), &|p| point_ref(p, c), "model.file.in.its_vertex", name)?;
            let regions = match f.get("regions") {
                Some(Value::Array(a)) => Value::Array(a.iter().map(|p| arr_copy(Some(p))).collect()),
                _ => Value::Null,
            };
            let extra = [
                ("upto", upto, Value::Null),
                ("vertex", vertex, Value::Null),
                ("offset", or0(f.get("offset"), 0.0), jv::num(0.0)),
                ("two", jv::is_true(f.get("two")).into(), false.into()),
                ("depth2", or0(f.get("depth2"), 10.0), jv::num(10.0)),
                ("taper", or0(f.get("taper"), 0.0), jv::num(0.0)),
                ("regions", regions, Value::Null),
            ];
            for (k, v, d) in extra {
                if jv::stringify(&v) != jv::stringify(&d) {
                    jv::set(g, k, v);
                }
            }
            Ok(true)
        }
        "hole" => {
            need(f.get("csinkD").is_none() || num(f.get("csinkD"), 1e-6, 1e6), "model.file.bad.the_countersink_diameter")?;
            need(
                f.get("csinkAngle").is_none() || matches!(jv::as_num(f.get("csinkAngle")), Some(x) if x == 90.0 || x == 82.0),
                "model.file.bad.the_countersink_angle",
            )?;
            need(
                f.get("size").is_none() || f.get("size").and_then(Value::as_str).is_some_and(|s| iso_coarse(s).is_some()),
                "model.file.bad.the_thread_size",
            )?;
            need(jv::nullish(f.get("threadDepth")) || num(f.get("threadDepth"), 1e-6, 1e6), "model.file.bad.the_thread_depth")?;
            let kind = jv::s(f, "kind");
            if kind == "csink" || kind == "tapped" || f.get("csinkD").is_some() || f.get("size").is_some() {
                let nn = |k: &str, d: Value| f.get(k).filter(|v| !v.is_null()).cloned().unwrap_or(d);
                jv::set(g, "csinkD", nn("csinkD", jv::num(12.0)));
                jv::set(g, "csinkAngle", nn("csinkAngle", jv::num(90.0)));
                jv::set(g, "size", nn("size", "M6".into()));
                jv::set(g, "threadDepth", nn("threadDepth", Value::Null));
            }
            Ok(true)
        }
        "chamfer" => {
            need(f.get("mode").is_none() || in_list(f.get("mode"), &["equal", "two", "angle"]), "model.file.bad.the_chamfer_type")?;
            need(f.get("d2").is_none() || num(f.get("d2"), 1e-6, 1e5), "model.file.bad.the_second_distance")?;
            need(f.get("angle").is_none() || num(f.get("angle"), 1e-3, 89.999), "model.file.bad.the_angle")?;
            if jv::truthy(f.get("mode")) && !jv::is_str(f.get("mode"), "equal") {
                let d2 = f.get("d2").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| g.get("d").cloned().unwrap_or(Value::Null));
                jv::set(g, "mode", f.get("mode").cloned().unwrap_or(Value::Null));
                jv::set(g, "d2", d2);
                jv::set(g, "angle", f.get("angle").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| jv::num(45.0)));
                jv::set(g, "flip", jv::is_true(f.get("flip")).into());
            }
            Ok(true)
        }
        "fillet" => {
            need(jv::nullish(f.get("r2")) || num(f.get("r2"), 1e-6, 1e5), "model.file.bad.the_end_radius")?;
            if jv::as_num(f.get("r2")).is_some() {
                jv::set(g, "r2", f.get("r2").cloned().unwrap_or(Value::Null));
                jv::set(g, "flip", jv::is_true(f.get("flip")).into());
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `DESIGN_LABELS[type]` (truthy).
pub fn is_design_type(ty: &str) -> bool {
    matches!(ty, "plane" | "axis" | "point" | "sweep" | "loft" | "draft") || proto_key(ty)
}
/// `SURFACE_LABELS[type]`.
pub fn is_surface_type(ty: &str) -> bool {
    matches!(ty, "form" | "patch" | "surfloft" | "stitch" | "thicken" | "offsetsurf" | "extendsurf" | "trimsurf" | "gordon")
        || proto_key(ty)
}
/// `BODY_LABELS[type]`.
pub fn is_body_type(ty: &str) -> bool {
    matches!(ty, "combine" | "movebody" | "scalebody" | "pathpattern") || proto_key(ty)
}
/// `SHEET_LABELS[type]`.
pub fn is_sheet_type(ty: &str) -> bool {
    matches!(ty, "sheetbase" | "sheetflange") || proto_key(ty)
}

/// `usesDesign(part)`.
pub fn uses_design(part: &Value) -> bool {
    jv::arr(part.get("features")).iter().any(|f| {
        let ty = jv::s(f, "type");
        let obj_ref =
            |d: Option<&Value>| d.is_some_and(|d| jv::truthy(Some(d)) && jv::is_obj_like(Some(d)) && jv::is_str(d.get("kind"), "ref"));
        is_design_type(ty)
            || in_list(f.get("end"), &["upto", "next", "vertex", "offset"])
            || jv::truthy(f.get("two"))
            || jv::truthy(f.get("taper"))
            || jv::truthy(f.get("regions"))
            || jv::is_str(f.get("kind"), "csink")
            || jv::is_str(f.get("kind"), "tapped")
            || (jv::truthy(f.get("mode")) && ty == "chamfer" && !jv::is_str(f.get("mode"), "equal"))
            || jv::as_num(f.get("r2")).is_some()
            || (ty == "revolve" && f.get("axis").is_some_and(|a| jv::truthy(Some(a)) && jv::is_obj_like(Some(a))))
            || f.get("plane").is_some_and(|p| jv::truthy(Some(p)) && jv::is_str(p.get("kind"), "ref"))
            || obj_ref(f.get("dir"))
            || obj_ref(f.get("axis"))
    })
}

// ---------------------------------------------------------------- bodies.js

/// `goodBody(id)`.
pub fn good_body(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| js::len16(s) <= 300 && re::body_id(s))
}

/// `bodiesDeps(f)`.
pub fn bodies_deps(f: &Value) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    let mut add = |x: &str| {
        if !d.iter().any(|y| y == x) {
            d.push(x.to_string());
        }
    };
    let ty = jv::s(f, "type");
    let mut ids: Vec<&Value> = Vec::new();
    ids.extend(jv::arr(f.get("bodies")).iter());
    ids.extend(jv::arr(f.get("tools")).iter());
    if let Some(tg) = f.get("target").filter(|v| v.is_string()) {
        ids.push(tg);
    }
    if is_body_type(ty) || ty == "lpattern" || ty == "cpattern" {
        for b in ids {
            if good_body(Some(b)) {
                for x in re::body_features(jv::as_str(Some(b)).unwrap_or("")) {
                    add(&x);
                }
            }
        }
    }
    if ty == "movebody" || ty == "scalebody" {
        for k in ["axis", "from", "to", "point"] {
            let Some(r) = f.get(k).filter(|r| jv::truthy(Some(r)) && jv::is_obj_like(Some(r))) else { continue };
            let kind = r.get("kind").and_then(Value::as_str);
            let s = |k: &str| r.get(k).map(|v| js::string_of(Some(v))).unwrap_or_else(|| "undefined".into());
            if matches!(kind, Some("ref" | "axis")) {
                add(&s("feature"));
            }
            if matches!(kind, Some("line" | "sketch")) {
                add(&s("sketch"));
            }
            if kind == Some("vertex")
                && let Some(e) = r.get("edge").filter(|e| jv::truthy(Some(e)))
                && let Some(fs) = jv::as_arr(e.get("faces"))
            {
                for n in fs {
                    for x in re::body_features(&js::string_of(Some(n))) {
                        add(&x);
                    }
                }
            }
        }
    }
    d
}

/// `readBodies(f, g, c, R)` → handled the type.
pub fn read_bodies(f: &Value, g: &mut Value, c: &Ctx) -> Result<bool, String> {
    let name = js::str_of(g.get("name"));
    let name = name.as_str();
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, name)) };
    let body = |b: &Value| -> Result<Value, String> {
        if !good_body(Some(b)) || !re::body_features(jv::as_str(Some(b)).unwrap_or("")).iter().all(|x| c.ids.contains(x)) {
            return Err(t("model.bodies.bad_ref", &[]));
        }
        Ok(b.clone())
    };
    let bodies = |v: Option<&Value>| -> Result<Value, String> {
        let Some(a) = jv::as_arr(v).filter(|a| a.len() <= 500) else { return Err(err("model.file.bad.its_body_list", name)) };
        let r: Result<Vec<Value>, String> = a.iter().map(body).collect();
        Ok(Value::Array(wrap(r, "model.file.in.its_bodies", name)?))
    };
    let or_d = |k: &str, d: f64| f.get(k).filter(|v| !v.is_null()).cloned().unwrap_or_else(|| jv::num(d));
    match jv::s(f, "type") {
        "combine" => {
            need(in_list(f.get("op"), &["join", "cut", "intersect"]), "model.file.bad.the_operation")?;
            let target = if jv::nullish(f.get("target")) {
                Value::Null
            } else {
                wrap(body(f.get("target").unwrap_or(&Value::Null)), "model.file.in.its_target", name)?
            };
            let tools = bodies(f.get("tools"))?;
            jv::set(g, "op", f.get("op").cloned().unwrap_or(Value::Null));
            jv::set(g, "target", target);
            jv::set(g, "tools", tools);
            jv::set(g, "keep", jv::is_true(f.get("keep")).into());
            Ok(true)
        }
        "movebody" => {
            need(in_list(f.get("mode"), &["translate", "points", "rotate", "align"]), "model.file.bad.the_move_type")?;
            for k in ["dx", "dy", "dz"] {
                if !num(Some(&or_d(k, 0.0)), -1e6, 1e6) {
                    return Err(t("model.file.bad.field", &[("name", F::from(name)), ("field", F::from(k))]));
                }
            }
            need(num(Some(&or_d("angle", 0.0)), -3600.0, 3600.0), "model.file.bad.the_angle")?;
            need(
                f.get("planes").is_none() || jv::as_arr(f.get("planes")).is_some_and(|a| a.len() <= 2),
                "model.file.bad.its_align_planes",
            )?;
            let bs = bodies(f.get("bodies"))?;
            let from = opt(f.get("from"), &|p| point_ref(p, c), "model.file.in.its_from_point", name)?;
            let to = opt(f.get("to"), &|p| point_ref(p, c), "model.file.in.its_to_point", name)?;
            let axis = opt(f.get("axis"), &|a| read_dir(a, c, true), "model.file.in.its_axis", name)?;
            let edge = opt(f.get("edge"), &|a| edge_ref(a, c), "model.file.in.its_axis_edge", name)?;
            let face = opt(f.get("face"), &|a| face_ref(a, c), "model.file.in.its_axis_face", name)?;
            let planes = {
                let r: Result<Vec<Value>, String> = if jv::truthy(f.get("planes")) {
                    jv::arr(f.get("planes")).iter().map(|p| read_plane(Some(p), c, true)).collect()
                } else {
                    Ok(Vec::new())
                };
                Value::Array(wrap(r, "model.file.in.its_align_planes", name)?)
            };
            for (k, v) in [
                ("bodies", bs),
                ("mode", f.get("mode").cloned().unwrap_or(Value::Null)),
                ("dx", or_d("dx", 0.0)),
                ("dy", or_d("dy", 0.0)),
                ("dz", or_d("dz", 0.0)),
                ("from", from),
                ("to", to),
                ("axis", axis),
                ("edge", edge),
                ("face", face),
                ("angle", or_d("angle", 0.0)),
                ("planes", planes),
                ("flip", jv::is_true(f.get("flip")).into()),
                ("copy", jv::is_true(f.get("copy")).into()),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "scalebody" => {
            for k in ["s", "sx", "sy", "sz"] {
                if !num(Some(&or_d(k, 1.0)), 1e-6, 1e6) {
                    return Err(t("model.file.bad.the_scale", &[("name", F::from(name)), ("k", F::from(k))]));
                }
            }
            let bs = bodies(f.get("bodies"))?;
            let point = opt(f.get("point"), &|p| point_ref(p, c), "model.file.in.its_point", name)?;
            for (k, v) in [
                ("bodies", bs),
                ("uniform", (!matches!(f.get("uniform"), Some(Value::Bool(false)))).into()),
                ("s", or_d("s", 1.0)),
                ("sx", or_d("sx", 1.0)),
                ("sy", or_d("sy", 1.0)),
                ("sz", or_d("sz", 1.0)),
                ("point", point),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "pathpattern" => {
            need(
                jv::as_arr(f.get("seeds")).is_some_and(|a| a.len() <= 500 && a.iter().all(|s| c.has(Some(s)))),
                "model.file.bad.the_features_to_pattern",
            )?;
            need(jv::int(f.get("count")).is_some_and(|n| (1.0..=1000.0).contains(&n)), "model.file.bad.the_count")?;
            need(in_list(f.get("spacing"), &["equal", "distance"]), "model.file.bad.the_spacing_type")?;
            need(num(f.get("d"), 1e-6, 1e6), "model.file.bad.the_spacing")?;
            need(in_list(f.get("orient"), &["path", "fixed"]), "model.file.bad.the_orientation")?;
            let empty = Value::Array(Vec::new());
            let bs = bodies(Some(f.get("bodies").filter(|v| !v.is_null()).unwrap_or(&empty)))?;
            let path =
                wrap(list(f.get("path"), 200, "model.file.list.the_path", &mut |p| path_item(Some(p), c)), "model.file.in.its_path", name)?;
            for (k, v) in [
                ("seeds", arr_copy(f.get("seeds"))),
                ("bodies", bs),
                ("path", path),
                ("count", f.get("count").cloned().unwrap_or(Value::Null)),
                ("spacing", f.get("spacing").cloned().unwrap_or(Value::Null)),
                ("d", f.get("d").cloned().unwrap_or(Value::Null)),
                ("orient", f.get("orient").cloned().unwrap_or(Value::Null)),
                ("reverse", jv::is_true(f.get("reverse")).into()),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        "lpattern" | "cpattern" => {
            if !jv::nullish(f.get("bodies")) {
                let b = bodies(f.get("bodies"))?;
                if !jv::arr(Some(&b)).is_empty() {
                    jv::set(g, "bodies", b);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// `readBodyInfo(raw, ids)`: None when empty.
pub fn read_body_info(raw: &Value, ids: &HashSet<String>) -> Result<Option<Value>, String> {
    let Some(o) = raw.as_object() else { return Err(t("file.bodies.bad", &[])) };
    if o.len() > 500 {
        return Err(t("file.bodies.too_many", &[]));
    }
    let mut out = jv::Obj::new();
    for (id, v) in o {
        if !good_body(Some(&Value::String(id.clone()))) || !v.is_object() {
            return Err(t("file.bodies.entry_bad", &[]));
        }
        if v.get("name").is_some_and(|n| !n.is_string()) {
            return Err(t("file.bodies.name_bad", &[]));
        }
        if !re::body_features(id).iter().all(|x| ids.contains(x)) {
            continue;
        }
        let mut e = jv::obj(vec![]);
        let name = js::slice16(js::trim(&js::strip_ctrl(&js::str_of(v.get("name")))), 60);
        if !name.is_empty() {
            jv::set(&mut e, "name", name.into());
        }
        if jv::is_true(v.get("hidden")) {
            jv::set(&mut e, "hidden", true.into());
        }
        if e.get("name").is_some() || e.get("hidden").is_some() {
            out.insert(id.clone(), e);
        }
    }
    Ok(if out.is_empty() { None } else { Some(Value::Object(out)) })
}

/// `usesBodies(part)`.
pub fn uses_bodies(part: &Value) -> bool {
    jv::as_obj(part.get("bodyInfo")).is_some_and(|o| !o.is_empty())
        || jv::arr(part.get("features")).iter().any(|f| {
            let ty = jv::s(f, "type");
            is_body_type(ty)
                || ((ty == "extrude" || ty == "revolve") && in_list(f.get("op"), &["new", "intersect"]))
                || jv::as_arr(f.get("bodies")).is_some_and(|a| !a.is_empty())
        })
}

// ---------------------------------------------------------------- surface.js

/// `surfaceDeps(f)`.
pub fn surface_deps(f: &Value) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    let ty = jv::s(f, "type");
    if !is_surface_type(ty) {
        return d;
    }
    let s = |v: &Value| js::string_of(Some(v));
    if let Some(x) = f.get("sheet").filter(|v| v.is_string()) {
        d.push(s(x));
    }
    if let Some(a) = jv::as_arr(f.get("sheets")) {
        d.extend(a.iter().map(s));
    }
    if let Some(x) = f.get("tool").and_then(Value::as_str).filter(|x| *x != "body") {
        d.push(x.to_string());
    }
    if let Some(a) = jv::as_arr(f.get("sketches")) {
        d.extend(a.iter().map(s));
    }
    if ty == "gordon" {
        for k in ["profiles", "guides"] {
            for cv in jv::arr(f.get(k)) {
                if let Some(sk) = cv.get("sketch").filter(|v| jv::truthy(Some(v))) {
                    d.push(s(sk));
                }
                if let Some(e) = cv.get("edge").filter(|v| jv::truthy(Some(v))) {
                    for n in jv::arr(e.get("faces")) {
                        d.extend(re::name_features(&js::str_of(Some(n))));
                    }
                }
            }
        }
        for p in jv::arr(f.get("tangent")) {
            let kind = p.get("kind").and_then(Value::as_str);
            if kind == Some("ref") {
                d.push(js::string_of(p.get("feature")));
            }
            if kind == Some("face") {
                d.push(js::string_of(p.get("feature")));
                if let Some(n) = p.get("name").filter(|v| jv::truthy(Some(v))) {
                    d.extend(re::name_features(&js::str_of(Some(n))));
                }
            }
        }
    }
    d
}

/// `usesSurface(part)`.
pub fn uses_surface(part: &Value) -> bool {
    jv::arr(part.get("features")).iter().any(|f| is_surface_type(jv::s(f, "type")))
}

/// `readSurface(f, g, c)`.
pub fn read_surface(f: &Value, g: &mut Value, c: &Ctx) -> Result<(), String> {
    let name = js::str_of(g.get("name"));
    let name = name.as_str();
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, name)) };
    let sheet_ref = |id: Option<&Value>| -> Result<Value, String> {
        need(c.has(id) && c.ty(id).is_some_and(is_surface_type), "model.file.bad.its_surface")?;
        Ok(id.cloned().unwrap_or(Value::Null))
    };
    let op = || -> Result<Value, String> {
        need(in_list(f.get("op"), &["new", "add", "cut", "intersect"]), "model.file.bad.the_operation")?;
        Ok(f.get("op").cloned().unwrap_or(Value::Null))
    };
    let edge_list = |v: Option<&Value>| -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for e in jv::arr(v) {
            need(
                jv::truthy(Some(e))
                    && jv::as_arr(e.get("faces")).is_some_and(|fs| {
                        fs.len() == 2
                            && fs.iter().all(|n| good_name(Some(n)))
                            && fs.iter().all(|n| re::name_features(jv::as_str(Some(n)).unwrap_or("")).iter().all(|x| c.ids.contains(x)))
                    }),
                "model.file.bad.an_edge_reference",
            )?;
            need(e.get("at").is_none() || vec3s(e.get("at")), "model.file.bad.an_edge_position")?;
            let mut o = jv::obj(vec![("faces", arr_copy(e.get("faces")))]);
            if jv::truthy(e.get("at")) {
                jv::set(&mut o, "at", arr_copy(e.get("at")));
            }
            out.push(o);
        }
        Ok(out)
    };
    match jv::s(f, "type") {
        "form" => {
            let cage = crate::form::read_form(f.get("cage"))
                .map_err(|e| t("model.surface.form_bad", &[("name", F::from(name)), ("problem", F::S(e))]))?;
            need(!jv::arr(cage.get("faces")).is_empty(), "model.file.bad.its_cage")?;
            need(
                jv::nullish(f.get("level")) || jv::int(f.get("level")).is_some_and(|l| (0.0..=3.0).contains(&l)),
                "model.file.bad.the_level",
            )?;
            let o = op()?;
            jv::set(g, "cage", cage);
            jv::set(g, "op", o);
            jv::set(g, "level", f.get("level").filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null));
        }
        "patch" => {
            need(jv::as_arr(f.get("edges")).is_some_and(|a| a.len() <= 500), "model.file.bad.its_edge_list")?;
            let edges = edge_list(f.get("edges"))?;
            let n_edges = edges.len();
            jv::set(g, "edges", Value::Array(edges));
            let sheet = if jv::nullish(f.get("sheet")) { Value::Null } else { sheet_ref(f.get("sheet"))? };
            let has_sheet = jv::truthy(Some(&sheet));
            jv::set(g, "sheet", sheet);
            need(has_sheet != (n_edges > 0) || (!has_sheet && n_edges == 0), "model.file.bad.its_boundary_edges_or_a_surface_not_both")?;
            need(jv::nullish(f.get("at")) || vec3s(f.get("at")), "model.file.bad.its_position")?;
            let cont = f.get("cont").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| "G0".into());
            need(in_list(Some(&cont), &["G0", "G1"]), "model.file.bad.the_continuity")?;
            jv::set(g, "at", if jv::truthy(f.get("at")) { arr_copy(f.get("at")) } else { Value::Null });
            jv::set(g, "cont", cont);
        }
        "surfloft" => {
            let ok = jv::as_arr(f.get("sketches")).is_some_and(|a| {
                let uniq: HashSet<String> = a.iter().map(|v| v.to_string()).collect();
                a.len() <= 50 && a.iter().all(|s| c.has(Some(s)) && c.ty(Some(s)) == Some("sketch")) && uniq.len() == a.len()
            });
            need(ok, "model.file.bad.its_sketches")?;
            jv::set(g, "sketches", arr_copy(f.get("sketches")));
        }
        "stitch" => {
            let ok = jv::as_arr(f.get("sheets")).is_some_and(|a| {
                let uniq: HashSet<String> = a.iter().map(|v| v.to_string()).collect();
                a.len() <= 200 && uniq.len() == a.len()
            });
            need(ok, "model.file.bad.its_surfaces")?;
            let mut sheets = Vec::new();
            for s in jv::arr(f.get("sheets")) {
                sheets.push(sheet_ref(Some(s))?);
            }
            jv::set(g, "sheets", Value::Array(sheets));
            let tol = f.get("tol").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| jv::num(0.01));
            need(num(Some(&tol), 1e-7, 10.0), "model.file.bad.the_tolerance")?;
            let o = op()?;
            jv::set(g, "op", o);
            jv::set(g, "tol", tol);
        }
        "thicken" => {
            let sheet = if matches!(f.get("sheet"), Some(Value::Null)) { Value::Null } else { sheet_ref(f.get("sheet"))? };
            jv::set(g, "sheet", sheet);
            need(num(f.get("t"), 1e-6, 1e5), "model.file.bad.the_thickness")?;
            need(in_list(f.get("side"), &["out", "in", "both"]), "model.file.bad.the_side")?;
            let o = op()?;
            jv::set(g, "t", f.get("t").cloned().unwrap_or(Value::Null));
            jv::set(g, "side", f.get("side").cloned().unwrap_or(Value::Null));
            jv::set(g, "op", o);
        }
        "offsetsurf" => {
            let sheet = if matches!(f.get("sheet"), Some(Value::Null)) { Value::Null } else { sheet_ref(f.get("sheet"))? };
            jv::set(g, "sheet", sheet);
            need(num(f.get("d"), -1e5, 1e5) && jv::as_num(f.get("d")) != Some(0.0), "model.file.bad.the_distance")?;
            jv::set(g, "d", f.get("d").cloned().unwrap_or(Value::Null));
        }
        "extendsurf" => {
            let sheet = if matches!(f.get("sheet"), Some(Value::Null)) { Value::Null } else { sheet_ref(f.get("sheet"))? };
            jv::set(g, "sheet", sheet);
            need(num(f.get("d"), 1e-6, 1e5), "model.file.bad.the_distance")?;
            let sides_ok = jv::as_arr(f.get("sides"))
                .is_some_and(|a| !a.is_empty() && a.iter().all(|s| in_list(Some(s), &["umin", "umax", "vmin", "vmax"])));
            need(sides_ok, "model.file.bad.the_sides")?;
            let mut sides: Vec<Value> = Vec::new();
            for s in jv::arr(f.get("sides")) {
                if !sides.contains(s) {
                    sides.push(s.clone());
                }
            }
            jv::set(g, "d", f.get("d").cloned().unwrap_or(Value::Null));
            jv::set(g, "sides", Value::Array(sides));
        }
        "trimsurf" => {
            let sheet = if matches!(f.get("sheet"), Some(Value::Null)) { Value::Null } else { sheet_ref(f.get("sheet"))? };
            jv::set(g, "sheet", sheet);
            let tool = f.get("tool");
            let tool_ok = matches!(tool, Some(Value::Null))
                || jv::is_str(tool, "body")
                || (tool.is_some_and(Value::is_string)
                    && c.has(tool)
                    && c.ty(tool).is_some_and(is_surface_type)
                    && !jv::strict_eq(tool, f.get("sheet")));
            need(tool_ok, "model.file.bad.its_cutting_tool")?;
            need(jv::nullish(f.get("at")) || vec3s(f.get("at")), "model.file.bad.the_point_to_keep")?;
            need(in_list(f.get("keep"), &["near", "far"]), "model.file.bad.which_piece_to_keep")?;
            jv::set(g, "tool", tool.cloned().unwrap_or(Value::Null));
            jv::set(g, "at", if jv::truthy(f.get("at")) { arr_copy(f.get("at")) } else { Value::Null });
            jv::set(g, "keep", f.get("keep").cloned().unwrap_or(Value::Null));
        }
        "gordon" => {
            for (k, bads) in [
                (
                    "profiles",
                    [
                        "model.file.bad.its_profiles",
                        "model.file.bad.a_curve_of_its_profiles",
                        "model.file.bad.an_edge_of_its_profiles",
                        "model.file.bad.a_sketch_of_its_profiles",
                        "model.file.bad.the_curves_picked_from_a_sketch_of_its_profiles",
                    ],
                ),
                (
                    "guides",
                    [
                        "model.file.bad.its_guides",
                        "model.file.bad.a_curve_of_its_guides",
                        "model.file.bad.an_edge_of_its_guides",
                        "model.file.bad.a_sketch_of_its_guides",
                        "model.file.bad.the_curves_picked_from_a_sketch_of_its_guides",
                    ],
                ),
            ] {
                let [b_list, b_curve, b_edge, b_sketch, b_picked] = bads;
                need(jv::as_arr(f.get(k)).is_some_and(|a| a.len() <= 200), b_list)?;
                let mut out = Vec::new();
                for x in jv::arr(f.get(k)) {
                    need(jv::truthy(Some(x)) && jv::is_obj_like(Some(x)), b_curve)?;
                    if x.get("edge").is_some() {
                        let e = x.get("edge");
                        let ok = e.filter(|e| jv::truthy(Some(e))).is_some_and(|e| {
                            jv::as_arr(e.get("faces")).is_some_and(|fs| {
                                fs.len() == 2
                                    && fs.iter().all(|n| good_name(Some(n)))
                                    && fs
                                        .iter()
                                        .all(|n| re::name_features(jv::as_str(Some(n)).unwrap_or("")).iter().all(|y| c.ids.contains(y)))
                            }) && (e.get("at").is_none() || vec3s(e.get("at")))
                        });
                        need(ok, b_edge)?;
                        let e = e.unwrap_or(&Value::Null);
                        let mut eo = jv::obj(vec![("faces", arr_copy(e.get("faces")))]);
                        if jv::truthy(e.get("at")) {
                            jv::set(&mut eo, "at", arr_copy(e.get("at")));
                        }
                        out.push(jv::obj(vec![("edge", eo)]));
                        continue;
                    }
                    need(
                        x.get("sketch").is_some_and(Value::is_string) && c.has(x.get("sketch")) && c.ty(x.get("sketch")) == Some("sketch"),
                        b_sketch,
                    )?;
                    let ents = x.get("entities");
                    need(
                        ents.is_none()
                            || jv::as_arr(ents).is_some_and(|a| {
                                !a.is_empty() && a.len() <= 5000 && a.iter().all(|y| y.as_str().is_some_and(|s| js::len16(s) <= 40))
                            }),
                        b_picked,
                    )?;
                    let mut o = jv::obj(vec![("sketch", x.get("sketch").cloned().unwrap_or(Value::Null))]);
                    if jv::truthy(ents) {
                        jv::set(&mut o, "entities", arr_copy(ents));
                    }
                    out.push(o);
                }
                jv::set(g, k, Value::Array(out));
            }
            need(
                f.get("tangent").is_none() || jv::as_arr(f.get("tangent")).is_some_and(|a| a.len() <= 4),
                "model.file.bad.its_tangent_planes",
            )?;
            if let Some(a) = jv::as_arr(f.get("tangent")).filter(|a| !a.is_empty()) {
                let mut out = Vec::new();
                for p in a {
                    out.push(read_plane(Some(p), c, true)?);
                }
                jv::set(g, "tangent", Value::Array(out));
            }
            let tol = f.get("tol").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| jv::num(0.01));
            need(num(Some(&tol), 1e-7, 10.0), "model.file.bad.the_tolerance")?;
            let o = op()?;
            jv::set(g, "tol", tol);
            jv::set(g, "solid", jv::is_true(f.get("solid")).into());
            jv::set(g, "op", o);
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------- sheetmetal.js, rib.js, threads.js

/// `sheetDeps(f)`.
pub fn sheet_deps(f: &Value) -> Vec<String> {
    let mut d = Vec::new();
    if jv::s(f, "type") == "sheetflange"
        && let Some(e) = f.get("edge").filter(|e| jv::truthy(Some(e)))
        && let Some(fs) = jv::as_arr(e.get("faces"))
    {
        for n in fs {
            d.extend(re::name_features(&js::str_of(Some(n))));
        }
    }
    d
}

/// `readSheet(f, g, c, R)`.
pub fn read_sheet(f: &Value, g: &mut Value, c: &Ctx) -> Result<bool, String> {
    let name = js::str_of(g.get("name"));
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, &name)) };
    match jv::s(f, "type") {
        "sheetbase" => {
            need(c.has(f.get("sketch")) && c.ty(f.get("sketch")) == Some("sketch"), "model.file.bad.its_sketch")?;
            need(
                num(f.get("t"), 0.01, 100.0) && num(f.get("r"), 0.0, 1000.0) && num(f.get("k"), 0.0, 0.5) && num(f.get("depth"), 0.01, 1e5),
                "model.file.bad.its_thickness_radius_k_factor_or_depth",
            )?;
            for k in ["sketch", "t", "r", "k", "depth"] {
                jv::set(g, k, f.get(k).cloned().unwrap_or(Value::Null));
            }
            jv::set(g, "reverse", jv::is_true(f.get("reverse")).into());
            Ok(true)
        }
        "sheetflange" => {
            need(num(f.get("angle"), 1.0, 180.0) && num(f.get("length"), 0.01, 1e5), "model.file.bad.its_angle_or_length")?;
            need(
                jv::as_arr(f.get("offsets")).is_some_and(|a| a.len() == 2 && a.iter().all(|o| num(Some(o), 0.0, 1e5))),
                "model.file.bad.its_offsets",
            )?;
            need(in_list(f.get("relief"), &["rect", "none"]) && num(f.get("reliefWidth"), 0.01, 100.0), "model.file.bad.its_relief")?;
            let edge = if jv::nullish(f.get("edge")) { Value::Null } else { edge_ref(f.get("edge"), c)? };
            for (k, v) in [
                ("edge", edge),
                ("angle", f.get("angle").cloned().unwrap_or(Value::Null)),
                ("length", f.get("length").cloned().unwrap_or(Value::Null)),
                ("flip", jv::is_true(f.get("flip")).into()),
                ("offsets", arr_copy(f.get("offsets"))),
                ("relief", f.get("relief").cloned().unwrap_or(Value::Null)),
                ("reliefWidth", f.get("reliefWidth").cloned().unwrap_or(Value::Null)),
            ] {
                jv::set(g, k, v);
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `readRib(f, g, c)`.
pub fn read_rib(f: &Value, g: &mut Value, c: &Ctx) -> Result<bool, String> {
    if jv::s(f, "type") != "rib" {
        return Ok(false);
    }
    let name = js::str_of(g.get("name"));
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, &name)) };
    need(c.has(f.get("sketch")) && c.ty(f.get("sketch")) == Some("sketch"), "model.file.bad.its_sketch")?;
    need(num(f.get("t"), 1e-4, 1e5), "model.file.bad.its_thickness")?;
    need(in_list(f.get("side"), &["mid", "front", "back"]), "model.file.bad.its_side")?;
    for k in ["sketch", "t", "side"] {
        jv::set(g, k, f.get(k).cloned().unwrap_or(Value::Null));
    }
    jv::set(g, "flip", jv::is_true(f.get("flip")).into());
    Ok(true)
}

/// `readThread(f, g, c, R)`.
pub fn read_thread(f: &Value, g: &mut Value, c: &Ctx) -> Result<bool, String> {
    if jv::s(f, "type") != "thread" {
        return Ok(false);
    }
    let name = js::str_of(g.get("name"));
    let need = |cond: bool, key: &str| if cond { Ok(()) } else { Err(err(key, &name)) };
    let form = f.get("form").and_then(Value::as_str);
    need(matches!(form, Some("iso" | "tr")), "thread.file.bad.form")?;
    let size_ok = jv::is_str(f.get("size"), "auto")
        || f.get("size").and_then(Value::as_str).is_some_and(|s| crate::threads::table_row(form.unwrap_or(""), s).is_some());
    need(size_ok, "model.file.bad.the_thread_size")?;
    need(jv::nullish(f.get("length")) || num(f.get("length"), 1e-3, 1e5), "thread.file.bad.length")?;
    need(f.get("clearance").is_none() || num(f.get("clearance"), 0.0, 5.0), "thread.file.bad.clearance")?;
    need(f.get("hand").is_none() || in_list(f.get("hand"), &["right", "left"]), "thread.file.bad.hand")?;
    need(jv::as_arr(f.get("faces")).is_some_and(|a| a.len() <= 100), "model.file.list.the_face_list")?;
    let faces: Result<Vec<Value>, String> = jv::arr(f.get("faces")).iter().map(|r| face_ref(Some(r), c)).collect();
    let faces = wrap(faces, "model.file.in.its_faces", &name)?;
    let nn = |k: &str, d: Value| f.get(k).filter(|v| !v.is_null()).cloned().unwrap_or(d);
    for (k, v) in [
        ("faces", Value::Array(faces)),
        ("form", f.get("form").cloned().unwrap_or(Value::Null)),
        ("size", f.get("size").cloned().unwrap_or(Value::Null)),
        ("length", nn("length", Value::Null)),
        ("flip", jv::is_true(f.get("flip")).into()),
        ("modelled", jv::is_true(f.get("modelled")).into()),
        ("clearance", nn("clearance", jv::num(crate::threads::DEFAULT_CLEARANCE))),
        ("hand", nn("hand", "right".into())),
    ] {
        jv::set(g, k, v);
    }
    Ok(true)
}

/// `readHoleThread(f, g)`.
pub fn read_hole_thread(f: &Value, g: &mut Value) -> Result<(), String> {
    if f.get("modelled").is_none() && f.get("clearance").is_none() {
        return Ok(());
    }
    if f.get("clearance").is_some() && !num(f.get("clearance"), 0.0, 5.0) {
        return Err(t("thread.file.bad.clearance", &[("name", F::S(js::str_of(g.get("name"))))]));
    }
    if jv::is_true(f.get("modelled")) {
        jv::set(g, "modelled", true.into());
        jv::set(
            g,
            "clearance",
            f.get("clearance").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| jv::num(crate::threads::DEFAULT_CLEARANCE)),
        );
    }
    Ok(())
}
