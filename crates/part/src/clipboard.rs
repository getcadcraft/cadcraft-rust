//! clipboard.js — PartCraft's copy and paste: sketch geometry (with the relations among it), a sketch or a
//! sketch-based feature with its sketch, an assembly part; clips are plain JSON and whatever comes back from storage
//! goes through `read_clip`, checked as strictly as a file.

use crate::asm;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::part;
use crate::re;
use crate::readers::{self, Ctx};
use crate::sketch::{self as s, Ent, ORIGIN};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const FORMAT: &str = "partcraft-clip";
pub const VERSION: f64 = 1.0;
pub const COPYABLE: [&str; 4] = ["sketch", "extrude", "revolve", "hole"];

fn prefix(ty: &str) -> &'static str {
    match ty {
        "point" => "p",
        "line" => "l",
        "circle" => "c",
        "arc" => "a",
        "spline" => "s",
        _ => "undefined",
    }
}
fn num_ok(v: Option<&Value>) -> bool {
    jv::finite(v).is_some_and(|x| x.abs() < 1e9)
}

/// `copySketch(sk, ids)`.
pub fn copy_sketch(sk: &Value, ids: &[String]) -> Option<Value> {
    let mut take: HashSet<String> = HashSet::new();
    for id in ids {
        if id == ORIGIN {
            continue;
        }
        let Some(e) = s::entities(sk).iter().find(|x| jv::is_str(x.get("id"), id)) else { continue };
        take.insert(js::str_of(e.get("id")));
        if jv::s(e, "type") != "point" {
            for p in s::points_of(&Ent::E(e)) {
                take.insert(p);
            }
        }
    }
    let entities: Vec<Value> = s::entities(sk).iter().filter(|e| take.contains(&js::str_of(e.get("id")))).map(jv::json_copy).collect();
    if entities.is_empty() {
        return None;
    }
    let mut constraints: Vec<Value> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for c in s::constraints(sk) {
        let refs = jv::arr(c.get("refs"));
        let inside = refs.iter().filter(|r| take.contains(&js::str_of(Some(r)))).count();
        if inside == refs.len() {
            constraints.push(jv::json_copy(c));
        } else if inside > 0 {
            dropped.push(js::str_of(c.get("id")));
        }
    }
    let mut i = constraints.len();
    while i > 0 {
        i = i.saturating_sub(1);
        let drop = constraints
            .get(i)
            .is_some_and(|c| jv::s(c, "type") == "offsetlink" && !constraints.iter().any(|m| jv::strict_eq(m.get("id"), c.get("of"))));
        if drop {
            let c = constraints.remove(i);
            dropped.push(js::str_of(c.get("id")));
        }
    }
    let sketch = jv::obj(vec![("seq", 1.into()), ("entities", Value::Array(entities.clone())), ("constraints", Value::Array(constraints))]);
    let anchor = s::bounds(&sketch).map_or([0.0, 0.0], |b| [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]);
    let curves = entities.iter().filter(|e| jv::s(e, "type") != "point").count();
    let points = s::loose_points(&sketch).len();
    Some(jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", jv::num(VERSION)),
        ("kind", "sketch".into()),
        ("sketch", sketch),
        ("anchor", jv::pt2(anchor)),
        ("dropped", jv::unum(dropped.len())),
        ("curves", jv::unum(curves)),
        ("points", jv::unum(points)),
    ]))
}

fn shift_fix(c: &mut Value, ents: &HashMap<String, String>, dx: f64, dy: f64) {
    let r0 = js::str_of(jv::arr(c.get("refs")).first());
    let Some(ty) = ents.get(&r0) else { return };
    let Some(vals) = c.get_mut("value").and_then(Value::as_array_mut) else { return };
    let pairs = if ty == "circle" { 1.0 } else { js::f(vals.len()) / 2.0 };
    let mut i = 0usize;
    while js::f(i) < pairs {
        for (k, d) in [(i.saturating_mul(2), dx), (i.saturating_mul(2).saturating_add(1), dy)] {
            match vals.get_mut(k) {
                Some(x) => *x = jv::num(jv::as_num(Some(x)).unwrap_or(f64::NAN) + d),
                None => {
                    // `c.value[k] += d` past the end: undefined + d = NaN, stored (JSON null)
                    while vals.len() < k {
                        vals.push(Value::Null);
                    }
                    vals.push(jv::num(f64::NAN));
                }
            }
        }
        i = i.saturating_add(1);
        if i > 1_000_000 {
            break;
        }
    }
}

/// `pasteSketch(sk, clip, d)` → { ids, constraints }.
pub fn paste_sketch(sk: &mut Value, clip: &Value, d: [f64; 2]) -> Value {
    let [dx, dy] = d;
    let mut map: HashMap<String, String> = HashMap::from([(ORIGIN.to_string(), ORIGIN.to_string())]);
    let src = clip.get("sketch").cloned().unwrap_or(Value::Null);
    let mut seq = jv::as_num(sk.get("seq")).unwrap_or(f64::NAN);
    for e in s::entities(&src) {
        map.insert(js::str_of(e.get("id")), format!("{}{}", prefix(jv::s(e, "type")), js::num_str(seq)));
        seq += 1.0;
    }
    let mut ids = Vec::new();
    let mut ents: HashMap<String, String> = HashMap::new();
    let order: Vec<&Value> = s::entities(&src)
        .iter()
        .filter(|x| jv::s(x, "type") == "point")
        .chain(s::entities(&src).iter().filter(|x| jv::s(x, "type") != "point"))
        .collect();
    let mapped = |m: &HashMap<String, String>, k: &str| m.get(k).map_or(Value::Null, |x| Value::String(x.clone()));
    let mut new_ents = Vec::new();
    for e in order {
        let mut n = jv::json_copy(e);
        let nid = mapped(&map, &js::str_of(e.get("id")));
        jv::set(&mut n, "id", nid.clone());
        let ty = jv::s(&n, "type").to_string();
        if ty == "point" {
            let (x, y) = (jv::as_num(n.get("x")).unwrap_or(f64::NAN), jv::as_num(n.get("y")).unwrap_or(f64::NAN));
            jv::set(&mut n, "x", jv::num(x + dx));
            jv::set(&mut n, "y", jv::num(y + dy));
        }
        if ty != "point" {
            for k in ["p1", "p2", "c", "s", "e"] {
                if let Some(Value::String(r)) = n.get(k).cloned() {
                    match map.get(&r) {
                        Some(m) => jv::set(&mut n, k, m.as_str().into()),
                        None => jv::del(&mut n, k),
                    }
                }
            }
        }
        if ty == "spline" {
            let pts: Vec<Value> = jv::arr(n.get("pts")).iter().map(|p| mapped(&map, &js::str_of(Some(p)))).collect();
            jv::set(&mut n, "pts", Value::Array(pts));
        }
        let id_s = js::str_of(Some(&nid));
        ids.push(id_s.clone());
        ents.insert(id_s, ty);
        new_ents.push(n);
    }
    let mut cons = Vec::new();
    let mut kmap: HashMap<String, String> = HashMap::new();
    let mut new_cons = Vec::new();
    for c in s::constraints(&src) {
        let refs = jv::arr(c.get("refs"));
        if !refs.iter().all(|r| map.contains_key(&js::str_of(Some(r)))) {
            continue;
        }
        let mut n = jv::json_copy(c);
        let nid = format!("k{}", js::num_str(seq));
        seq += 1.0;
        jv::set(&mut n, "id", nid.as_str().into());
        kmap.insert(js::str_of(c.get("id")), nid.clone());
        if jv::s(&n, "type") == "offsetlink" {
            let of = js::str_of(c.get("of"));
            let Some(m) = kmap.get(&of) else { continue };
            jv::set(&mut n, "of", m.as_str().into());
        }
        jv::set(&mut n, "refs", Value::Array(refs.iter().map(|r| mapped(&map, &js::str_of(Some(r)))).collect()));
        if jv::s(&n, "type") == "fix" {
            shift_fix(&mut n, &ents, dx, dy);
        }
        if let Some(at) = n.get("at").and_then(Value::as_array).cloned() {
            let g = |i: usize| jv::as_num(at.get(i)).unwrap_or(f64::NAN);
            jv::set(&mut n, "at", jv::nums(&[g(0) + dx, g(1) + dy]));
        }
        cons.push(nid);
        new_cons.push(n);
    }
    jv::set(sk, "seq", jv::num(seq));
    if let Some(a) = s::entities_mut(sk) {
        a.extend(new_ents);
    }
    if let Some(a) = s::constraints_mut(sk) {
        a.extend(new_cons);
    }
    jv::obj(vec![("ids", jv::strs(&ids)), ("constraints", jv::strs(&cons))])
}

/// `copyFeature(part, id)`.
pub fn copy_feature(pt: &Value, id: &str) -> Result<Value, String> {
    let Some(f) = part::feature_by_id(pt, id) else { return Err(t("xch.clip.select_feature", &[])) };
    let name = js::string_of(f.get("name"));
    if !COPYABLE.contains(&jv::s(f, "type")) {
        return Err(t("xch.clip.not_copyable", &[("name", F::S(name))]));
    }
    let mut feats = Vec::new();
    if jv::s(f, "type") != "sketch" {
        let Some(sf) = part::feature_by_id(pt, &js::str_of(f.get("sketch"))) else {
            return Err(t("xch.clip.no_sketch", &[("name", F::S(name))]));
        };
        feats.push(jv::json_copy(sf));
    }
    feats.push(jv::json_copy(f));
    Ok(jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", jv::num(VERSION)),
        ("kind", "features".into()),
        ("from", pt.get("name").cloned().unwrap_or(Value::Null)),
        ("features", Value::Array(feats)),
    ]))
}

fn plane_fits(pt: &Value, plane: &Value) -> bool {
    if jv::s(plane, "kind") != "face" {
        return true;
    }
    let mut need = vec![js::str_of(plane.get("feature"))];
    if jv::truthy(plane.get("name")) {
        need.extend(re::name_features(&js::str_of(plane.get("name"))));
    }
    need.iter().all(|id| part::feature_by_id(pt, id).is_some())
}

/// `pasteFeatures(part, clip)` → { features, note }.
pub fn paste_features(pt: &mut Value, clip: &Value) -> Result<Value, String> {
    let fs = jv::arr(clip.get("features"));
    let first = fs.first().cloned().unwrap_or(Value::Null);
    let (s0, solid) = if jv::s(&first, "type") == "sketch" { (Some(first), fs.get(1).cloned()) } else { (None, Some(first)) };
    let mut out = Vec::new();
    let mut note = String::new();
    let mut sketch_id = Value::Null;
    if let Some(s0) = &s0 {
        let mut plane = jv::json_copy(s0.get("plane").unwrap_or(&Value::Null));
        if !plane_fits(pt, &plane) {
            plane = jv::obj(vec![("kind", "base".into()), ("name", "front".into())]);
            note = t("xch.clip.face_missing", &[("name", F::S(js::string_of(s0.get("name"))))]);
        }
        let ns = part::add_feature(
            pt,
            "sketch",
            &jv::obj(vec![("plane", plane), ("sketch", jv::json_copy(s0.get("sketch").unwrap_or(&Value::Null)))]),
        )?;
        sketch_id = ns.get("id").cloned().unwrap_or(Value::Null);
        out.push(ns);
    }
    if let Some(solid) = solid.filter(|x| jv::s(x, "type") != "sketch") {
        let mut props = jv::json_copy(&solid);
        jv::del(&mut props, "id");
        jv::del(&mut props, "name");
        jv::del(&mut props, "type");
        jv::set(&mut props, "sketch", sketch_id);
        out.push(part::add_feature(pt, jv::s(&solid, "type"), &props)?);
    }
    Ok(jv::obj(vec![("features", Value::Array(out)), ("note", note.into())]))
}

/// `copyInstance(asm, id)`.
pub fn copy_instance(a: &Value, id: &str) -> Value {
    let Some(inst) = jv::arr(a.get("instances")).iter().find(|i| jv::is_str(i.get("id"), id)) else { return Value::Null };
    let p = a.get("parts").and_then(|ps| ps.get(js::str_of(inst.get("part")))).cloned().unwrap_or(Value::Null);
    jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", jv::num(VERSION)),
        ("kind", "instance".into()),
        ("name", p.get("name").cloned().unwrap_or(Value::Null)),
        ("part", jv::json_copy(p.get("part").unwrap_or(&Value::Null))),
        ("place", Value::Array(jv::arr(inst.get("place")).to_vec())),
    ])
}

/// `pasteInstance(asm, clip, place)`.
pub fn paste_instance(a: &mut Value, clip: &Value, place: &Value) -> Result<Value, String> {
    let read = part::read_part(clip.get("part").unwrap_or(&Value::Null))?;
    let want = part::to_json(&read);
    let pid = a.get("parts").and_then(Value::as_object).and_then(|ps| {
        ps.iter()
            .find(|(_, p)| part::read_part(p.get("part").unwrap_or(&Value::Null)).is_ok_and(|x| part::to_json(&x) == want))
            .map(|(k, _)| k.clone())
    });
    match pid {
        Some(pid) => asm::insert_part(a, None, Some(place), Some(&pid)),
        None => asm::insert_part(a, Some(&read), Some(place), None),
    }
}

/// `readClip(raw)`.
pub fn read_clip(raw: &Value) -> Result<Value, String> {
    let parsed;
    let j = match raw {
        Value::String(st) => {
            if js::len16(st) > 80 * 1024 * 1024 {
                return Err(t("xch.clip.too_big", &[]));
            }
            parsed = jv::parse(st).ok_or_else(|| t("xch.clip.not_partcraft", &[]))?;
            &parsed
        }
        x => x,
    };
    if !jv::truthy(Some(j)) || !jv::is_obj_like(Some(j)) || !jv::is_str(j.get("format"), FORMAT) {
        return Err(t("xch.clip.not_partcraft", &[]));
    }
    if !jv::strict_eq(j.get("version"), Some(&jv::num(VERSION))) {
        return Err(t("xch.clip.other_version", &[]));
    }
    match j.get("kind").and_then(Value::as_str) {
        Some("sketch") => {
            let sk = s::read_sketch(j.get("sketch"))?;
            if s::entities(&sk).is_empty() {
                return Err(t("xch.clip.sketch_empty", &[]));
            }
            if s::constraints(&sk).iter().any(|c| jv::arr(c.get("refs")).iter().any(|r| jv::is_str(Some(r), ORIGIN))) {
                return Err(t("xch.clip.sketch_origin", &[]));
            }
            let ok = jv::as_arr(j.get("anchor")).is_some_and(|a| a.len() == 2 && a.iter().all(|x| num_ok(Some(x))));
            if !ok {
                return Err(t("xch.clip.sketch_no_position", &[]));
            }
            let dropped = jv::int(j.get("dropped")).filter(|d| *d >= 0.0 && *d < 1e6).unwrap_or(0.0);
            let curves = s::entities(&sk).iter().filter(|e| jv::s(e, "type") != "point").count();
            let points = s::loose_points(&sk).len();
            Ok(jv::obj(vec![
                ("format", FORMAT.into()),
                ("version", jv::num(VERSION)),
                ("kind", "sketch".into()),
                ("sketch", sk),
                ("anchor", Value::Array(jv::arr(j.get("anchor")).to_vec())),
                ("dropped", jv::num(dropped)),
                ("curves", jv::unum(curves)),
                ("points", jv::unum(points)),
            ]))
        }
        Some("features") => {
            let f = j.get("features");
            let fl = jv::as_arr(f).filter(|a| !a.is_empty() && a.len() <= 2).ok_or_else(|| t("xch.clip.features_bad", &[]))?;
            let ty =
                |i: usize| fl.get(i).filter(|x| jv::truthy(Some(x))).and_then(|x| x.get("type")).and_then(Value::as_str).unwrap_or("\u{0}");
            let ok_shape = (fl.len() == 1 && ty(0) == "sketch")
                || (fl.len() == 2
                    && ty(0) == "sketch"
                    && matches!(ty(1), "extrude" | "revolve" | "hole")
                    && jv::strict_eq(fl.get(1).and_then(|x| x.get("sketch")), fl.first().and_then(|x| x.get("id"))));
            if !ok_shape {
                return Err(t("xch.clip.features_bad", &[]));
            }
            let plane = fl.first().and_then(|x| x.get("plane"));
            let mut ids: HashSet<String> = HashSet::new();
            if let Some(p) = plane.filter(|p| jv::truthy(Some(p)) && jv::s(p, "kind") == "face") {
                let mut cand = vec![js::string_of(p.get("feature"))];
                if jv::truthy(p.get("name")) {
                    cand.extend(re::name_features(&js::string_of(p.get("name"))));
                }
                for c in cand {
                    if re::feature_id(&c) {
                        ids.insert(c);
                    }
                }
            }
            let types = HashMap::new();
            let ctx = Ctx { ids: &ids, types: &types, any_id: false };
            let clean_plane = readers::read_plane(plane, &ctx, false)?;
            let feats: Vec<Value> = fl
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    if i == 0 {
                        let mut y = x.clone();
                        jv::set(&mut y, "plane", jv::obj(vec![("kind", "base".into()), ("name", "front".into())]));
                        y
                    } else {
                        x.clone()
                    }
                })
                .collect();
            let raw_part = jv::obj(vec![
                ("format", part::FORMAT.into()),
                ("version", jv::num(part::VERSION)),
                ("name", "clip".into()),
                ("features", Value::Array(feats)),
            ]);
            let mut tmp = part::read_part(&raw_part)?;
            if let Some(f0) = tmp.get_mut("features").and_then(|f| f.get_mut(0)) {
                jv::set(f0, "plane", clean_plane);
            }
            let from = js::slice16(
                &js::strip_ctrl(&match j.get("from") {
                    None | Some(Value::Null) => String::new(),
                    x => js::string_of(x),
                }),
                120,
            );
            Ok(jv::obj(vec![
                ("format", FORMAT.into()),
                ("version", jv::num(VERSION)),
                ("kind", "features".into()),
                ("from", from.into()),
                ("features", tmp.get("features").cloned().unwrap_or(Value::Null)),
            ]))
        }
        Some("instance") => {
            let pt = part::read_part(j.get("part").unwrap_or(&Value::Null))?;
            let ok = jv::as_arr(j.get("place")).is_some_and(|a| a.len() == 16 && a.iter().all(|x| num_ok(Some(x))));
            if !ok {
                return Err(t("xch.clip.part_position", &[]));
            }
            Ok(jv::obj(vec![
                ("format", FORMAT.into()),
                ("version", jv::num(VERSION)),
                ("kind", "instance".into()),
                ("name", pt.get("name").cloned().unwrap_or(Value::Null)),
                ("part", jv::parse(&part::to_json(&pt)).unwrap_or(Value::Null)),
                ("place", Value::Array(jv::arr(j.get("place")).to_vec())),
            ]))
        }
        _ => Err(t("xch.clip.unknown_kind", &[])),
    }
}

/// `describeClip(c)`.
pub fn describe_clip(c: &Value) -> String {
    if !jv::truthy(Some(c)) {
        return String::new();
    }
    let and = |a: String, b: String| t("xch.clip.and", &[("a", F::S(a)), ("b", F::S(b))]);
    match jv::s(c, "kind") {
        "sketch" => {
            let mut parts = Vec::new();
            let cv = jv::as_num(c.get("curves")).unwrap_or(0.0);
            let pv = jv::as_num(c.get("points")).unwrap_or(0.0);
            if cv != 0.0 && !cv.is_nan() {
                parts.push(t("xch.clip.curves", &[("n", F::N(cv))]));
            }
            if pv != 0.0 && !pv.is_nan() {
                parts.push(t("xch.clip.points", &[("n", F::N(pv))]));
            }
            parts.into_iter().reduce(and).unwrap_or_else(|| t("xch.clip.sketch_geometry", &[]))
        }
        "features" => jv::arr(c.get("features")).iter().map(|f| js::string_of(f.get("name"))).reduce(and).unwrap_or_default(),
        _ => js::string_of(c.get("name")),
    }
}
