//! explode.js — exploded views of an assembly: steps that move sets of instances along a direction, played one after
//! another; the offsets, the exploded transforms, the trails, the automatic steps (radial, or down the joint tree) and
//! the untrusted-file reader. A view only moves what is drawn; joints and placement are untouched.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::vec::{self as v, M4, V3};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const MAX_VIEWS: usize = 50;
pub const MAX_STEPS: usize = 200;

fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}
fn seq_next(asm: &mut Value) -> f64 {
    let s = jv::as_num(asm.get("seq")).unwrap_or(f64::NAN);
    jv::set(asm, "seq", jv::num(s + 1.0));
    s
}

/// `newView(asm, name)`.
pub fn new_view(asm: &mut Value, name: Option<&str>) -> Value {
    let id = format!("v{}", js::num_str(seq_next(asm)));
    let n = jv::arr(asm.get("views")).len().saturating_add(1);
    let nm = match name.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => t("xch.explode.default_name", &[("n", F::from(n))]),
    };
    let view = jv::obj(vec![("id", id.into()), ("name", nm.into()), ("steps", Value::Array(Vec::new()))]);
    if !asm.get("views").is_some_and(|x| jv::truthy(Some(x))) {
        jv::set(asm, "views", Value::Array(Vec::new()));
    }
    if let Some(a) = asm.get_mut("views").and_then(Value::as_array_mut) {
        a.push(view.clone());
    }
    view
}

/// `addStep(asm, view, insts, dir, dist)`; the view is found by id in `asm.views`.
pub fn add_step(asm: &mut Value, view_id: &str, insts: &[String], dir: V3, dist: f64) -> Result<Value, String> {
    if insts.is_empty() {
        return Err(t("xch.explode.pick_parts", &[]));
    }
    let d = v::norm(dir);
    if !dir.iter().all(|x| x.is_finite()) || v::len(dir) < 1e-12 {
        return Err(t("xch.explode.need_dir", &[]));
    }
    if !dist.is_finite() {
        return Err(t("xch.explode.need_dist", &[]));
    }
    let mut uniq: Vec<String> = Vec::new();
    for i in insts {
        if !uniq.contains(i) {
            uniq.push(i.clone());
        }
    }
    let id = format!("s{}", js::num_str(seq_next(asm)));
    let s = jv::obj(vec![("id", id.into()), ("insts", jv::strs(&uniq)), ("dir", jv::pt3(d)), ("dist", jv::num(dist))]);
    let view = asm.get_mut("views").and_then(Value::as_array_mut).and_then(|vs| vs.iter_mut().find(|x| jv::is_str(x.get("id"), view_id)));
    let Some(view) = view else { return Err("Cannot read properties of undefined (reading 'steps')".into()) };
    if let Some(st) = view.get_mut("steps").and_then(Value::as_array_mut) {
        st.push(s.clone());
    }
    Ok(s)
}

/// `stepProgress(k, n, t)`.
pub fn step_progress(k: usize, n: usize, tt: f64) -> f64 {
    js::max(0.0, js::min(1.0, tt * js::f(n) - js::f(k)))
}

/// `offsets(view, t)` → insertion-ordered (inst, offset).
pub fn offsets(view: Option<&Value>, tt: f64) -> Vec<(String, V3)> {
    let mut out: Vec<(String, V3)> = Vec::new();
    let Some(view) = view.filter(|x| jv::truthy(Some(x))) else { return out };
    let steps = jv::arr(view.get("steps"));
    let n = steps.len();
    for (k, s) in steps.iter().enumerate() {
        let p = step_progress(k, n, tt);
        if p == 0.0 || p.is_nan() {
            continue;
        }
        let dir = p3(s.get("dir"));
        let dist = jv::as_num(s.get("dist")).unwrap_or(f64::NAN);
        for id in jv::arr(s.get("insts")) {
            let id = js::string_of(Some(id));
            let mv = v::mul(dir, dist * p);
            match out.iter_mut().find(|(k2, _)| *k2 == id) {
                Some(e) => e.1 = v::add(e.1, mv),
                None => out.push((id, v::add([0.0; 3], mv))),
            }
        }
    }
    out
}

/// `exploded(world, offs)`.
pub fn exploded(world: &[(String, M4)], offs: &[(String, V3)]) -> Vec<(String, M4)> {
    world
        .iter()
        .map(|(id, m)| match offs.iter().find(|(k, _)| k == id) {
            Some((_, o)) => (id.clone(), v::mat_mul(&v::translation(*o), m)),
            None => (id.clone(), *m),
        })
        .collect()
}

/// `trails(view, centres, t)`.
pub fn trails(view: Option<&Value>, centres: &[(String, V3)], tt: f64) -> Vec<(String, Vec<V3>)> {
    let mut out = Vec::new();
    let Some(view) = view.filter(|x| jv::truthy(Some(x))) else { return out };
    let steps = jv::arr(view.get("steps"));
    let n = steps.len();
    for (id, c) in centres {
        let mut pts = vec![*c];
        let mut at = *c;
        for (k, s) in steps.iter().enumerate() {
            if !jv::arr(s.get("insts")).iter().any(|x| jv::is_str(Some(x), id)) {
                continue;
            }
            let p = step_progress(k, n, tt);
            if p == 0.0 || p.is_nan() {
                continue;
            }
            at = v::add(at, v::mul(p3(s.get("dir")), jv::as_num(s.get("dist")).unwrap_or(f64::NAN) * p));
            pts.push(at);
        }
        if pts.len() > 1 {
            out.push((id.clone(), pts));
        }
    }
    out
}

/// A world box.
#[derive(Clone, Copy, Debug)]
pub struct Box3 {
    pub min: V3,
    pub max: V3,
}

fn extent_along(boxes: &[Box3], d: V3) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for b in boxes {
        for i in 0..8u32 {
            let p = [
                if i & 1 != 0 { b.max[0] } else { b.min[0] },
                if i & 2 != 0 { b.max[1] } else { b.min[1] },
                if i & 4 != 0 { b.max[2] } else { b.min[2] },
            ];
            let x = v::dot(p, d);
            lo = js::min(lo, x);
            hi = js::max(hi, x);
        }
    }
    if hi > lo { hi - lo } else { 0.0 }
}

/// One automatic step.
#[derive(Clone, Debug)]
pub struct AutoStep {
    pub insts: Vec<String>,
    pub dir: V3,
    pub dist: f64,
}

/// `autoSteps(items, mode, { tree, factor })`; tree = (parent, child, normal).
pub fn auto_steps(items: &[(String, Box3)], mode: &str, tree: &[(String, String, V3)], factor: f64) -> Result<Vec<AutoStep>, String> {
    let bx: HashMap<&str, Box3> = items.iter().map(|(k, b)| (k.as_str(), *b)).collect();
    let mut size: f64 = 1.0;
    for (_, b) in items {
        size = js::max(size, v::len(v::sub(b.max, b.min)));
    }
    let gap = size * 0.05;
    let centre = |b: &Box3| v::mul(v::add(b.min, b.max), 0.5);
    let mut steps = Vec::new();
    match mode {
        "radial" => {
            let sum = items.iter().fold([0.0; 3], |s, (_, b)| v::add(s, centre(b)));
            let c = v::mul(sum, 1.0 / js::f(items.len()));
            for (id, b) in items.iter().skip(1) {
                let mut d = v::sub(centre(b), c);
                if v::len(d) < 1e-6 * size {
                    d = [0.0, 1.0, 0.0];
                }
                d = v::norm(d);
                steps.push(AutoStep { insts: vec![id.clone()], dir: d, dist: factor * extent_along(&[*b], d) + gap });
            }
            Ok(steps)
        }
        "joints" => {
            let mut kids: Vec<(String, Vec<String>)> = Vec::new();
            for (p, c, _) in tree {
                match kids.iter_mut().find(|(k, _)| k == p) {
                    Some(e) => e.1.push(c.clone()),
                    None => kids.push((p.clone(), vec![c.clone()])),
                }
            }
            for (_, child, normal) in tree {
                // below(id): depth-first, a seen-set stops loops; an explicit stack keeps the JS order
                let mut seen: HashSet<String> = HashSet::new();
                let mut insts = Vec::new();
                let mut stack: Vec<String> = vec![child.clone()];
                while let Some(id) = stack.pop() {
                    if !seen.insert(id.clone()) {
                        continue;
                    }
                    insts.push(id.clone());
                    if let Some((_, ks)) = kids.iter().find(|(k, _)| *k == id) {
                        for k in ks.iter().rev() {
                            stack.push(k.clone());
                        }
                    }
                }
                let d = v::norm(*normal);
                let bs: Vec<Box3> = bx.get(child.as_str()).copied().into_iter().collect();
                steps.push(AutoStep { insts, dir: d, dist: factor * extent_along(&bs, d) + gap });
            }
            Ok(steps)
        }
        _ => Err(t("xch.explode.unknown_method", &[])),
    }
}

fn num_ok(x: Option<&Value>) -> bool {
    jv::finite(x).is_some_and(|y| y.abs() < 1e9)
}

/// `readViews(raw, instIds, take)`.
pub fn read_views(
    raw: Option<&Value>,
    inst_ids: &HashSet<String>,
    take: &mut dyn FnMut(Option<&Value>, char) -> Result<String, String>,
) -> Result<Value, String> {
    let Some(raw) = raw.filter(|r| !r.is_null()) else { return Ok(Value::Array(Vec::new())) };
    let Some(list) = raw.as_array().filter(|a| a.len() <= MAX_VIEWS) else { return Err(t("xch.explode.views_bad", &[])) };
    let mut out = Vec::new();
    for vw in list {
        let steps = vw.get("steps").and_then(Value::as_array).filter(|s| jv::truthy(Some(vw)) && s.len() <= MAX_STEPS);
        let Some(steps) = steps else { return Err(t("xch.explode.view_bad", &[])) };
        let id = take(vw.get("id"), 'v')?;
        let nm = js::slice16(js::trim(&js::strip_ctrl(&js::str_of(vw.get("name")))), 60);
        let name = if nm.is_empty() { t("xch.explode.name_fallback", &[]) } else { nm };
        let mut st_out = Vec::new();
        for s in steps {
            let ins = s.get("insts").and_then(Value::as_array);
            let ok =
                ins.is_some_and(|a| !a.is_empty() && a.len() <= 500 && a.iter().all(|i| i.as_str().is_some_and(|x| inst_ids.contains(x))));
            if !ok {
                return Err(t("xch.explode.step_missing_part", &[]));
            }
            let dir_ok = jv::as_arr(s.get("dir")).is_some_and(|d| d.len() == 3 && d.iter().all(|x| num_ok(Some(x))));
            if !dir_ok || v::len(p3(s.get("dir"))) < 1e-9 || !num_ok(s.get("dist")) {
                return Err(t("xch.explode.step_bad", &[]));
            }
            let sid = take(s.get("id"), 's')?;
            let mut uniq: Vec<Value> = Vec::new();
            for i in ins.into_iter().flatten() {
                if !uniq.contains(i) {
                    uniq.push(i.clone());
                }
            }
            st_out.push(jv::obj(vec![
                ("id", sid.into()),
                ("insts", Value::Array(uniq)),
                ("dir", jv::pt3(v::norm(p3(s.get("dir"))))),
                ("dist", s.get("dist").cloned().unwrap_or(Value::Null)),
            ]));
        }
        out.push(jv::obj(vec![("id", id.into()), ("name", name.into()), ("steps", Value::Array(st_out))]));
    }
    Ok(Value::Array(out))
}
