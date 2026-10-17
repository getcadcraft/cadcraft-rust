//! form.js — the parts a part file needs: the default form cage (`box`) and the untrusted reader (`readForm` with its
//! topology check). Subdivision, patches and editing are exact-kernel / Form-workspace work, not ported yet.

use crate::i18n::{F, t};
use crate::jv;
use crate::sketch::proto_key;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const FORMAT: &str = "partcraft-form";
pub const VERSION: f64 = 1.0;
pub const SHARP: f64 = 10.0;
pub const MAX_VERTS: usize = 20000;
pub const MAX_FACES: usize = 20000;

fn fail(key: &str, fills: &[(&str, F)]) -> String {
    t(key, fills)
}

/// `ek(a, b)`.
fn ek(a: i64, b: i64) -> String {
    if a < b { format!("{a},{b}") } else { format!("{b},{a}") }
}

/// `emptyForm()` with verts and faces.
fn form_value(verts: Value, faces: Value, creases: Value, corners: Value, sym: Value) -> Value {
    jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", 1.into()),
        ("verts", verts),
        ("faces", faces),
        ("creases", creases),
        ("corners", corners),
        ("sym", sym),
    ])
}

/// `box(nx, ny, nz, sx, sy, sz)`: a box of nx × ny × nz faces per side, centred on the origin.
pub fn box_form(nx: f64, ny: f64, nz: f64, sx: f64, sy: f64, sz: f64) -> Value {
    let n: [i64; 3] = [nx, ny, nz].map(|x| {
        let r = crate::js::round(x).clamp(1.0, 32.0);
        if r.is_nan() { 1 } else { crate::js::i64_of(r) }
    });
    let mut verts: Vec<Value> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut faces: Vec<Value> = Vec::new();
    let size = [sx, sy, sz];
    let mut vid = |l: [i64; 3]| -> usize {
        let k = format!("{},{},{}", l[0], l[1], l[2]);
        if let Some(&i) = index.get(&k) {
            return i;
        }
        let i = verts.len();
        index.insert(k, i);
        let c = |a: usize| {
            ((l.get(a).copied().unwrap_or(0) as f64) / (n.get(a).copied().unwrap_or(1) as f64) - 0.5) * size.get(a).copied().unwrap_or(0.0)
        };
        verts.push(jv::nums(&[c(0), c(1), c(2)]));
        i
    };
    const SIDES: [[usize; 4]; 6] = [[0, 1, 1, 2], [0, 0, 2, 1], [1, 1, 2, 0], [1, 0, 0, 2], [2, 1, 0, 1], [2, 0, 1, 0]];
    for [ax, side, sa, ta] in SIDES {
        let ns = n.get(sa).copied().unwrap_or(1);
        let nt = n.get(ta).copied().unwrap_or(1);
        for i in 0..ns {
            for j in 0..nt {
                let mut at = |s: i64, tt: i64| {
                    let mut l = [0i64; 3];
                    if let Some(x) = l.get_mut(ax) {
                        *x = if side == 1 { n.get(ax).copied().unwrap_or(1) } else { 0 };
                    }
                    if let Some(x) = l.get_mut(sa) {
                        *x = s;
                    }
                    if let Some(x) = l.get_mut(ta) {
                        *x = tt;
                    }
                    vid(l)
                };
                let (i1, j1) = (i.saturating_add(1), j.saturating_add(1));
                let f = [at(i, j), at(i1, j), at(i1, j1), at(i, j1)];
                faces.push(Value::Array(f.iter().map(|&v| jv::unum(v)).collect()));
            }
        }
    }
    form_value(Value::Array(verts), Value::Array(faces), Value::Array(Vec::new()), Value::Array(Vec::new()), Value::Null)
}

fn num(v: Option<&Value>) -> Option<f64> {
    jv::as_num(v).filter(|x| x.is_finite() && x.abs() <= 1e6)
}

/// `readForm(raw)`.
pub fn read_form(raw: Option<&Value>) -> Result<Value, String> {
    let parsed;
    let j = match raw {
        Some(Value::String(s)) => {
            parsed = jv::parse(s).ok_or_else(|| fail("form.file.not_json", &[]))?;
            &parsed
        }
        Some(v) => v,
        None => return Err(fail("form.file.not_form", &[])),
    };
    if !jv::is_obj_like(Some(j)) || !jv::is_str(j.get("format"), FORMAT) {
        return Err(fail("form.file.not_form", &[]));
    }
    let ver = match jv::int(j.get("version")) {
        Some(v) if v >= 1.0 => v,
        _ => return Err(fail("form.file.no_version", &[])),
    };
    if ver > VERSION {
        return Err(fail("form.file.newer", &[("v", F::N(ver)), ("mine", F::N(VERSION))]));
    }
    let Some(vs) = jv::as_arr(j.get("verts")).filter(|a| a.len() <= MAX_VERTS) else {
        return Err(fail("form.file.max_verts", &[("n", F::from(MAX_VERTS))]));
    };
    let Some(fs) = jv::as_arr(j.get("faces")).filter(|a| a.len() <= MAX_FACES) else {
        return Err(fail("form.file.max_faces", &[("n", F::from(MAX_FACES))]));
    };
    let mut verts = Vec::with_capacity(vs.len());
    for p in vs {
        match jv::as_arr(Some(p)) {
            Some(a) if a.len() == 3 && a.iter().all(|x| num(Some(x)).is_some()) => verts.push(Value::Array(a.clone())),
            _ => return Err(fail("form.file.vertex", &[])),
        }
    }
    let nv = verts.len();
    let vid = |v: &Value| v.as_f64().is_some_and(|x| crate::js::is_integer(x) && x >= 0.0 && x < crate::js::f(nv));
    let mut faces: Vec<Vec<i64>> = Vec::with_capacity(fs.len());
    for f in fs {
        let ok = jv::as_arr(Some(f)).filter(|a| a.len() >= 3 && a.len() <= 64 && a.iter().all(vid));
        let Some(a) = ok else { return Err(fail("form.file.face", &[])) };
        let ids: Vec<i64> = a.iter().map(|x| crate::js::i64_of(x.as_f64().unwrap_or(0.0))).collect();
        let uniq: HashSet<i64> = ids.iter().copied().collect();
        if uniq.len() != ids.len() {
            return Err(fail("form.file.face", &[]));
        }
        faces.push(ids);
    }
    // checked(f)
    let mut dir: HashSet<(i64, i64)> = HashSet::new();
    let mut cnt: HashMap<String, usize> = HashMap::new();
    for g in &faces {
        let n = g.len();
        for (k, &v) in g.iter().enumerate() {
            let w = g.get(crate::js::rem(k.saturating_add(1), n)).copied().unwrap_or(v);
            if !dir.insert((v, w)) {
                return Err(fail("form.err.flip", &[]));
            }
            let e = cnt.entry(ek(v, w)).or_insert(0);
            *e = e.saturating_add(1);
        }
    }
    if cnt.values().any(|&c| c > 2) {
        return Err(fail("form.err.nonmanifold", &[]));
    }
    let edges: HashSet<&String> = cnt.keys().collect();
    let mut creases: Vec<(String, [Value; 3])> = Vec::new();
    if let Some(cr) = j.get("creases") {
        let Some(list) = jv::as_arr(Some(cr)).filter(|a| a.len() <= 4 * MAX_FACES) else { return Err(fail("form.file.creases", &[])) };
        for c in list {
            let a = jv::as_arr(Some(c));
            let good = a.filter(|a| {
                a.len() == 3
                    && a.first().is_some_and(vid)
                    && a.get(1).is_some_and(vid)
                    && edges.contains(&ek(
                        crate::js::i64_of(a.first().and_then(Value::as_f64).unwrap_or(0.0)),
                        crate::js::i64_of(a.get(1).and_then(Value::as_f64).unwrap_or(0.0)),
                    ))
                    && a.get(2).and_then(Value::as_f64).is_some_and(|w| w > 0.0 && w <= SHARP)
            });
            let Some(a) = good else { return Err(fail("form.file.crease", &[])) };
            let x = crate::js::i64_of(a.first().and_then(Value::as_f64).unwrap_or(0.0));
            let y = crate::js::i64_of(a.get(1).and_then(Value::as_f64).unwrap_or(0.0));
            let w = a.get(2).cloned().unwrap_or(Value::Null);
            let key = ek(x, y);
            let val = [jv::unum(crate::js::usize_of(x.min(y))), jv::unum(crate::js::usize_of(x.max(y))), w];
            match creases.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = val,
                None => creases.push((key, val)),
            }
        }
    }
    let mut corners: Vec<Value> = Vec::new();
    if let Some(co) = j.get("corners") {
        let Some(list) = jv::as_arr(Some(co)).filter(|a| a.len() <= nv && a.iter().all(vid)) else {
            return Err(fail("form.file.corners", &[]));
        };
        let mut seen = HashSet::new();
        for c in list {
            let k = crate::js::i64_of(c.as_f64().unwrap_or(0.0));
            if seen.insert(k) {
                corners.push(c.clone());
            }
        }
    }
    let mut sym = Value::Null;
    if let Some(s) = j.get("sym").filter(|s| !s.is_null()) {
        let axis_ok = s.get("axis").is_some_and(|a| {
            let k = crate::js::str_of(Some(a));
            matches!(k.as_str(), "x" | "y" | "z") || proto_key(&k)
        });
        let side = match s.get("side") {
            None | Some(Value::Null) => Some(1.0),
            Some(v) => v.as_f64().filter(|x| *x == 1.0 || *x == -1.0),
        };
        if !jv::is_obj_like(Some(s)) || !axis_ok || num(s.get("at")).is_none() || side.is_none() {
            return Err(fail("form.file.symmetry", &[]));
        }
        sym = jv::obj(vec![
            ("axis", s.get("axis").cloned().unwrap_or(Value::Null)),
            ("at", s.get("at").cloned().unwrap_or(Value::Null)),
            ("side", jv::num(side.unwrap_or(1.0))),
        ]);
    }
    let faces_v = Value::Array(faces.iter().map(|f| Value::Array(f.iter().map(|&v| jv::unum(crate::js::usize_of(v))).collect())).collect());
    Ok(form_value(
        Value::Array(verts),
        faces_v,
        Value::Array(creases.into_iter().map(|(_, v)| Value::Array(v.to_vec())).collect()),
        Value::Array(corners),
        sym,
    ))
}
