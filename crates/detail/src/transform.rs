//! transform.js — move, rotate (about z) and mirror (in a vertical plane) a selection, inside a model change; a member
//! keeps its real section orientation (its rotation is worked out again from where its (u, v) axes go, a mirror
//! re-places the section by its own symmetry); dragging (`snapOffset`), one property on many members
//! (`manyPatchable` / `applyToMany`), ghost lines, the selection's centre, a stair's outline.

use crate::geom::{V3, dot, frame, sub, voff_for};
use crate::js::{Obj, Res, arr, atan2, cos, f, hypot, jnum, max, min, num, num_or0, round, sin, string_of, truthy};
use crate::model::{Tx, strict_eq};
use crate::profiles::{Profile, get_profile, plate_name};
use crate::state::members_of_v;
use serde_json::{Map, Value, json};
use std::f64::consts::PI;

const R: f64 = PI / 180.0;
const I3: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A transform: p → A·p + t.
#[derive(Clone, Debug)]
pub struct Xf {
    pub kind: &'static str,
    pub a: [[f64; 3]; 3],
    pub t: V3,
    pub det: f64,
    pub deg: Option<f64>,
    pub phi: Option<f64>,
}
impl Xf {
    pub fn j(&self) -> Value {
        let a: Vec<Value> = self.a.iter().map(|r| json!([jnum(r[0]), jnum(r[1]), jnum(r[2])])).collect();
        Obj::new()
            .set("kind", self.kind)
            .set("A", Value::Array(a))
            .set("t", crate::js::p3v(self.t))
            .set("det", self.det)
            .opt("deg", self.deg)
            .opt("phi", self.phi)
            .done()
    }
}

/// `moveXf(dx, dy, dz)`.
pub fn move_xf(d: [Option<&Value>; 3]) -> Res<Xf> {
    let t = d.map(num);
    if t.iter().any(|n| !n.is_finite() || n.abs() > 1e6) {
        return Err("bad-offset".into());
    }
    Ok(Xf { kind: "move", a: I3, t, det: 1.0, deg: None, phi: None })
}
/// `rotateXf(center, deg)`.
pub fn rotate_xf(center: &[Value], deg: Option<&Value>) -> Res<Xf> {
    let a = num(deg) * R;
    let (cx, cy) = (num(center.first()), num(center.get(1)));
    if !a.is_finite() || !cx.is_finite() || !cy.is_finite() {
        return Err("bad-angle".into());
    }
    let (c, s) = (cos(a), sin(a));
    Ok(Xf {
        kind: "rotate",
        a: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
        t: [cx - (c * cx - s * cy), cy - (s * cx + c * cy), 0.0],
        det: 1.0,
        deg: Some(num(deg)),
        phi: None,
    })
}
/// `mirrorXf(p1, p2)`.
pub fn mirror_xf(p1: &[Value], p2: &[Value]) -> Res<Xf> {
    let (x1, y1) = (num(p1.first()), num(p1.get(1)));
    let (x2, y2) = (num(p2.first()), num(p2.get(1)));
    let (dx, dy) = (x2 - x1, y2 - y1);
    let l = hypot(dx, dy);
    if !(l > 1e-6) || !x1.is_finite() || !y1.is_finite() {
        return Err("bad-mirror".into());
    }
    let (ux, uy) = (dx / l, dy / l);
    let a = [[2.0 * ux * ux - 1.0, 2.0 * ux * uy, 0.0], [2.0 * ux * uy, 2.0 * uy * uy - 1.0, 0.0], [0.0, 0.0, 1.0]];
    let t = [x1 - (a[0][0] * x1 + a[0][1] * y1), y1 - (a[1][0] * x1 + a[1][1] * y1), 0.0];
    Ok(Xf { kind: "mirror", a, t, det: -1.0, deg: None, phi: Some(atan2(uy, ux) / R) })
}
/// A transform from the harness's `{ kind, args }` form (the same calls the page makes).
pub fn xf_of(spec: &Value) -> Res<Xf> {
    let a = arr(spec.get("args"));
    match spec.get("kind").and_then(Value::as_str) {
        Some("move") => move_xf([a.first(), a.get(1), a.get(2)]),
        Some("rotate") => rotate_xf(arr(a.first()), a.get(1)),
        Some("mirror") => mirror_xf(arr(a.first()), arr(a.get(1))),
        _ => Err("bad-transform".into()),
    }
}

/// `applyPt(xf, p)`.
pub fn apply_pt(xf: &Xf, p: V3) -> V3 {
    let [a0, a1, a2] = xf.a;
    [
        a0[0] * p[0] + a0[1] * p[1] + a0[2] * p[2] + xf.t[0],
        a1[0] * p[0] + a1[1] * p[1] + a1[2] * p[2] + xf.t[1],
        a2[0] * p[0] + a2[1] * p[1] + a2[2] * p[2] + xf.t[2],
    ]
}
/// `applyDir(xf, d)`.
pub fn apply_dir(xf: &Xf, d: V3) -> V3 {
    let [a0, a1, a2] = xf.a;
    [a0[0] * d[0] + a0[1] * d[1] + a0[2] * d[2], a1[0] * d[0] + a1[1] * d[1] + a1[2] * d[2], a2[0] * d[0] + a2[1] * d[1] + a2[2] * d[2]]
}
fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
/// `clean(n)`: snapped to an integer within 1e-6, else to 6 decimals.
pub fn clean(n: f64) -> f64 {
    if (n - round(n)).abs() < 1e-6 { round(n) } else { round(n * 1e6) / 1e6 }
}
fn clean_pt(p: V3) -> V3 {
    p.map(clean)
}
/// `norm360(d)`.
pub fn norm360(d: f64) -> f64 {
    clean(((d % 360.0) + 360.0) % 360.0) % 360.0
}
fn rotation_for(start: V3, end: V3, want: V3) -> f64 {
    let f0 = frame(start, end, 0.0);
    norm360(atan2(-dot(want, f0.u), dot(want, f0.v)) / R)
}

/// `transformMember(m, xf, kind)` → the patch (start, end, rotation, position, holes, cuts) and a note.
pub struct XfOut {
    pub start: V3,
    pub end: V3,
    pub rotation: f64,
    pub position: Value,
    pub holes: Vec<Value>,
    pub cuts: Vec<Value>,
    pub note: Option<&'static str>,
}
fn negate(v: Option<&Value>) -> Value {
    // `-h.y`
    jnum(-num(v))
}
pub fn transform_member(m: &Value, xf: &Xf, kind: Option<&str>) -> XfOut {
    let (ms, me) = (crate::js::p3_of(m.get("start")), crate::js::p3_of(m.get("end")));
    let fr = frame(ms, me, f(m.get("rotation")));
    let mut start = apply_pt(xf, ms);
    let mut end = apply_pt(xf, me);
    let uu = apply_dir(xf, fr.u);
    let vv = apply_dir(xf, fr.v);
    let mut want_v = vv;
    let mut position = m.get("position").cloned().unwrap_or(Value::Null);
    let mut holes: Vec<Map<String, Value>> = arr(m.get("holes")).iter().map(|h| crate::js::spread(Some(h), None)).collect();
    let mut cuts: Vec<Map<String, Value>> = arr(m.get("cuts")).iter().map(|c| crate::js::spread(Some(c), None)).collect();
    let mut note = None;
    if xf.det < 0.0 {
        if kind == Some("C") {
            want_v = neg(vv);
            position = match m.get("position").and_then(Value::as_str) {
                Some("top") => Value::from("bottom"),
                Some("bottom") => Value::from("top"),
                _ => m.get("position").cloned().unwrap_or(Value::Null),
            };
            for h in &mut holes {
                match h.get("face").and_then(Value::as_str) {
                    Some("web") => {
                        let y = negate(h.get("y"));
                        h.insert("y".into(), y);
                    }
                    Some("top") => {
                        h.insert("face".into(), Value::from("bottom"));
                    }
                    Some("bottom") => {
                        h.insert("face".into(), Value::from("top"));
                    }
                    _ => {}
                }
            }
            for c in &mut cuts {
                let side = if c.get("side").and_then(Value::as_str) == Some("top") { "bottom" } else { "top" };
                c.insert("side".into(), Value::from(side));
            }
        } else if kind == Some("L") {
            want_v = uu;
            if m.get("position").and_then(Value::as_str) != Some("middle") {
                note = Some("angle-offset");
            }
        } else {
            for h in &mut holes {
                if h.get("face").and_then(Value::as_str) != Some("web") {
                    let y = negate(h.get("y"));
                    h.insert("y".into(), y);
                }
            }
        }
    }
    start = clean_pt(start);
    end = clean_pt(end);
    let rotation = rotation_for(start, end, want_v);
    for h in &mut holes {
        let y = jnum(clean(num(h.get("y"))));
        h.insert("y".into(), y);
    }
    XfOut {
        start,
        end,
        rotation,
        position,
        holes: holes.into_iter().map(Value::Object).collect(),
        cuts: cuts.into_iter().map(Value::Object).collect(),
        note,
    }
}
fn angle_shift(m: &Value, prof: &Profile, out: &mut XfOut, xf: &Xf) {
    let fr = frame(crate::js::p3_of(m.get("start")), crate::js::p3_of(m.get("end")), f(m.get("rotation")));
    let pos = crate::js::s(m.get("position"));
    let off = voff_for(pos, prof.depth);
    let want = apply_dir(xf, fr.v).map(|n| n * off);
    let nf = frame(out.start, out.end, out.rotation);
    let have = nf.v.map(|n| n * voff_for(pos, prof.depth));
    let d = sub(want, have);
    out.start = clean_pt([out.start[0] + d[0], out.start[1] + d[1], out.start[2] + d[2]]);
    out.end = clean_pt([out.end[0] + d[0], out.end[1] + d[1], out.end[2] + d[2]]);
}

/// V8's TypeError for `r.kind` on a null ref.
fn null_ref(refs: &[Value]) -> Res<()> {
    if refs.iter().any(Value::is_null) { Err("Cannot read properties of null (reading 'kind')".into()) } else { Ok(()) }
}
fn ids_of(refs: &[Value], kind: &str) -> Res<Vec<Value>> {
    null_ref(refs)?;
    Ok(refs
        .iter()
        .filter(|r| r.get("kind").and_then(Value::as_str) == Some(kind))
        .map(|r| r.get("id").cloned().unwrap_or(Value::Null))
        .collect())
}
fn has(ids: &[Value], v: Option<&Value>) -> bool {
    ids.iter().any(|x| strict_eq(Some(x), v))
}

/// `transformRefs(tx, state, refs, xf)`.
pub fn transform_refs(tx: &mut Tx, state: &Value, refs: &[Value], xf: &Xf) -> Res<Value> {
    let member_ids = ids_of(refs, "member")?;
    let footing_ids = ids_of(refs, "footing")?;
    let stair_ids = ids_of(refs, "stair")?;
    let custom = arr(state.get("profiles")).to_vec();
    let (mut n, mut nf, mut ns) = (0usize, 0usize, 0usize);
    for m in arr(state.get("members")) {
        if !has(&member_ids, m.get("id")) {
            continue;
        }
        let prof = get_profile(m.get("profile").unwrap_or(&Value::Null), &custom);
        let mut out = transform_member(m, xf, prof.as_ref().map(|p| p.kind.as_str()));
        if out.note == Some("angle-offset")
            && let Some(p) = &prof
        {
            angle_shift(m, p, &mut out, xf);
        }
        let patch = json!({
            "start": crate::js::p3v(out.start), "end": crate::js::p3v(out.end), "rotation": jnum(out.rotation), "position": out.position,
            "holes": out.holes, "cuts": out.cuts,
        });
        tx.update_member(m.get("id"), Some(&patch))?;
        n = n.saturating_add(1);
    }
    for fv in arr(state.get("footings")) {
        if !has(&footing_ids, fv.get("id")) {
            continue;
        }
        let p = clean_pt(apply_pt(xf, [num(fv.get("x")), num(fv.get("y")), num(fv.get("top"))]));
        let mut rotation = num_or0(fv.get("rotation"));
        if xf.kind == "rotate" {
            rotation = norm360(rotation + xf.deg.unwrap_or(f64::NAN));
        } else if xf.kind == "mirror" {
            rotation = norm360(2.0 * xf.phi.unwrap_or(f64::NAN) - rotation);
        }
        tx.update_footing(fv.get("id"), Some(&json!({ "x": jnum(p[0]), "y": jnum(p[1]), "top": jnum(p[2]), "rotation": jnum(rotation) })))?;
        nf = nf.saturating_add(1);
    }
    for x in arr(state.get("stairs")) {
        if !has(&stair_ids, x.get("id")) {
            continue;
        }
        let p = clean_pt(apply_pt(xf, [num(x.get("x")), num(x.get("y")), num(x.get("z"))]));
        let mut dir = num_or0(x.get("dir"));
        if xf.kind == "rotate" {
            dir = norm360(dir + xf.deg.unwrap_or(f64::NAN));
        } else if xf.kind == "mirror" {
            dir = norm360(2.0 * xf.phi.unwrap_or(f64::NAN) - dir);
        }
        tx.update_stair(x.get("id"), Some(&json!({ "x": jnum(p[0]), "y": jnum(p[1]), "z": jnum(p[2]), "dir": jnum(dir) })))?;
        ns = ns.saturating_add(1);
    }
    let mut split = 0usize;
    for c in arr(tx.s.get("connections")) {
        let parts = members_of_v(c);
        let moved = parts.iter().filter(|id| has(&member_ids, **id)).count();
        if moved > 0 && moved < parts.len() {
            split = split.saturating_add(1);
        }
    }
    let notes = if split > 0 { json!([{ "key": "xf.note.split", "n": split }]) } else { json!([]) };
    Ok(json!({ "members": n, "footings": nf, "stairs": ns, "connections": split, "notes": notes }))
}

fn ref_keys(refs: &[Value]) -> Res<Vec<String>> {
    null_ref(refs)?;
    Ok(refs.iter().map(|r| format!("{}:{}", string_of(r.get("kind")), string_of(r.get("id")))).collect())
}

/// `stairOutline(x)` → segments.
pub fn stair_outline(x: &Value) -> Vec<[V3; 2]> {
    let a = num_or0(x.get("dir")) * R;
    let (c, s) = (cos(a), sin(a));
    let g = |k: &str| num(x.get(k));
    let n = max(2.0, (g("rise") / g("maxRiser") - 1e-9).ceil());
    let run = (n - 1.0) * g("going");
    let pp = |al: f64, ac: f64, up: f64| -> V3 { [g("x") + c * al - s * ac, g("y") + s * al + c * ac, g("z") + up] };
    let w = g("width");
    let q = [pp(0.0, -w / 2.0, 0.0), pp(run, -w / 2.0, 0.0), pp(run, w / 2.0, 0.0), pp(0.0, w / 2.0, 0.0)];
    let mut out: Vec<[V3; 2]> = (0..4).map(|i| [crate::js::g3(q, i), crate::js::g3(q, (i + 1) % 4)]).collect();
    out.push([pp(0.0, 0.0, 0.0), pp(run, 0.0, g("rise"))]);
    out
}

/// `ghostLines(state, refs, base)`.
pub fn ghost_lines(state: &Value, refs: &[Value], base: V3) -> Res<Vec<[V3; 2]>> {
    let ids = ref_keys(refs)?;
    let rel = |p: V3| -> V3 { [p[0] - base[0], p[1] - base[1], p[2] - base[2]] };
    let mut out = Vec::new();
    for m in arr(state.get("members")) {
        if ids.contains(&format!("member:{}", string_of(m.get("id")))) {
            out.push([rel(crate::js::p3_of(m.get("start"))), rel(crate::js::p3_of(m.get("end")))]);
        }
    }
    for fv in arr(state.get("footings")) {
        if !ids.contains(&format!("footing:{}", string_of(fv.get("id")))) {
            continue;
        }
        let a = num_or0(fv.get("rotation")) * R;
        let (c, s) = (cos(a), sin(a));
        let g = |k: &str| num(fv.get(k));
        let k: Vec<V3> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .iter()
            .map(|&[i, j]| {
                [g("x") + c * i * g("L") / 2.0 - s * j * g("B") / 2.0, g("y") + s * i * g("L") / 2.0 + c * j * g("B") / 2.0, g("top")]
            })
            .collect();
        for i in 0..4 {
            if let (Some(a), Some(b)) = (k.get(i), k.get((i + 1) % 4)) {
                out.push([rel(*a), rel(*b)]);
            }
        }
    }
    for x in arr(state.get("stairs")) {
        if ids.contains(&format!("stair:{}", string_of(x.get("id")))) {
            out.extend(stair_outline(x).into_iter().map(|[a, b]| [rel(a), rel(b)]));
        }
    }
    Ok(out)
}

/// `selectionCenter(state, refs)`.
pub fn selection_center(state: &Value, refs: &[Value]) -> Res<Option<V3>> {
    let ids = ref_keys(refs)?;
    let (mut x0, mut y0, mut z0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut take = |p: V3| {
        x0 = min(x0, p[0]);
        y0 = min(y0, p[1]);
        z0 = min(z0, p[2]);
        x1 = max(x1, p[0]);
        y1 = max(y1, p[1]);
    };
    for m in arr(state.get("members")) {
        if ids.contains(&format!("member:{}", string_of(m.get("id")))) {
            take(crate::js::p3_of(m.get("start")));
            take(crate::js::p3_of(m.get("end")));
        }
    }
    for fv in arr(state.get("footings")) {
        if ids.contains(&format!("footing:{}", string_of(fv.get("id")))) {
            take([num(fv.get("x")), num(fv.get("y")), num(fv.get("top"))]);
        }
    }
    for x in arr(state.get("stairs")) {
        if ids.contains(&format!("stair:{}", string_of(x.get("id")))) {
            for [a, b] in stair_outline(x) {
                take(a);
                take(b);
            }
        }
    }
    if !x0.is_finite() {
        return Ok(None);
    }
    Ok(Some([clean((x0 + x1) / 2.0), clean((y0 + y1) / 2.0), clean(z0)]))
}

/// A screen point (`project(p)` → { x, y, behind }).
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
    pub behind: bool,
}
/// `snapOffset(raw, points, nodes, project, { mask, px, step })` → (offset, node, point).
pub fn snap_offset(
    raw: V3,
    points: &[V3],
    nodes: &[V3],
    project: &dyn Fn(V3) -> Screen,
    mask: [f64; 3],
    px: f64,
    step: f64,
) -> (V3, Option<V3>, Option<V3>) {
    let mut best: Option<(f64, V3, V3, V3)> = None;
    for p in points {
        let q = [p[0] + raw[0], p[1] + raw[1], p[2] + raw[2]];
        let sq = project(q);
        if sq.behind {
            continue;
        }
        for n in nodes {
            let d = [n[0] - p[0], n[1] - p[1], n[2] - p[2]];
            if d.iter().zip(mask).any(|(v, m)| (m == 0.0 || m.is_nan()) && v.abs() > 0.5) {
                continue;
            }
            let sn = project(*n);
            if sn.behind {
                continue;
            }
            let k = hypot(sn.x - sq.x, sn.y - sq.y);
            if k <= px && best.as_ref().is_none_or(|b| k < b.0) {
                best = Some((k, d, *n, *p));
            }
        }
    }
    let on = |m: f64| m != 0.0 && !m.is_nan();
    match best {
        Some((_, d, n, p)) => {
            ([0, 1, 2].map(|i| if on(crate::js::g3(mask, i)) { clean(crate::js::g3(d, i)) } else { 0.0 }), Some(n), Some(p))
        }
        None => (
            [0, 1, 2].map(|i| if on(crate::js::g3(mask, i)) { clean(round(crate::js::g3(raw, i) / step) * step) } else { 0.0 }),
            None,
            None,
        ),
    }
}

/// `MANY_FIELDS`.
pub const MANY_FIELDS: [&str; 6] = ["profile", "plateT", "plateW", "material", "position", "rotation"];

/// `manyPatchable(state, ids)`.
pub fn many_patchable(state: &Value, ids: &[Value]) -> Value {
    let custom = arr(state.get("profiles")).to_vec();
    let mem: Vec<&Value> = arr(state.get("members")).iter().filter(|m| has(ids, m.get("id"))).collect();
    let is_plate = |m: &Value| m.get("type").and_then(Value::as_str) == Some("plate");
    let plate_dim = |m: &Value, k: &str| -> Option<f64> {
        let p = get_profile(m.get("profile").unwrap_or(&Value::Null), &custom)?;
        match k {
            "t" => p.t,
            _ => p.b,
        }
    };
    let mut out = Map::new();
    let mut gather = |field: &str, ok: &dyn Fn(&Value) -> bool, get: &dyn Fn(&Value) -> Option<Value>| {
        let use_: Vec<&&Value> = mem.iter().filter(|m| ok(m)).collect();
        let mut vals: Vec<Option<Value>> = Vec::new();
        for m in &use_ {
            let v = get(m).map(|v| match v.as_f64() {
                Some(x) if v.is_number() => jnum(clean(x)),
                _ => v,
            });
            if !vals.iter().any(|x| match (x, &v) {
                (None, None) => true,
                (Some(a), Some(b)) => strict_eq(Some(a), Some(b)) || (a.is_null() && b.is_null()),
                _ => false,
            }) {
                vals.push(v);
            }
        }
        let mut o =
            Obj::new().set("applies", use_.len()).set("skipped", mem.len().saturating_sub(use_.len())).set("varies", vals.len() > 1);
        if vals.len() == 1 {
            if let Some(Some(v)) = vals.first() {
                o.put("value", v.clone());
            }
        } else {
            o.put("value", Value::Null);
        }
        out.insert(field.to_string(), o.done());
    };
    gather("profile", &|m| !is_plate(m), &|m| m.get("profile").cloned());
    gather("plateT", &|m| is_plate(m), &|m| plate_dim(m, "t").map(jnum));
    gather("plateW", &|m| is_plate(m), &|m| plate_dim(m, "b").map(jnum));
    gather("material", &|_| true, &|m| m.get("material").cloned());
    gather("position", &|m| !is_plate(m), &|m| m.get("position").cloned());
    gather("rotation", &|_| true, &|m| m.get("rotation").cloned());
    Value::Object(out)
}

/// `applyToMany(tx, state, ids, field, value)`.
pub fn apply_to_many(tx: &mut Tx, state: &Value, ids: &[Value], field: &str, value: Option<&Value>) -> Res<Value> {
    if !MANY_FIELDS.contains(&field) {
        return Err("unknown-field".into());
    }
    let custom = arr(state.get("profiles")).to_vec();
    let (mut changed, mut skipped) = (0usize, 0usize);
    let v = value.cloned().unwrap_or(Value::Null);
    for m in arr(state.get("members")) {
        if !has(ids, m.get("id")) {
            continue;
        }
        let plate = m.get("type").and_then(Value::as_str) == Some("plate");
        let patch = if field == "profile" && !plate {
            Some(json!({ "profile": v }))
        } else if (field == "plateT" || field == "plateW") && plate {
            let p = get_profile(m.get("profile").unwrap_or(&Value::Null), &custom);
            let tk = if field == "plateT" { num(value) } else { p.as_ref().and_then(|p| p.t).unwrap_or(20.0) };
            let w = if field == "plateW" { num(value) } else { p.as_ref().and_then(|p| p.b).unwrap_or(200.0) };
            if !(tk > 0.0 && w > 0.0) {
                return Err("bad-plate".into());
            }
            Some(json!({ "profile": plate_name(tk, w) }))
        } else if field == "material" {
            Some(json!({ "material": v }))
        } else if field == "position" && !plate {
            Some(json!({ "position": v }))
        } else if field == "rotation" {
            Some(json!({ "rotation": jnum(num(value)) }))
        } else {
            None
        };
        let Some(patch) = patch else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        tx.update_member(m.get("id"), Some(&patch))?;
        changed = changed.saturating_add(1);
    }
    Ok(json!({ "changed": changed, "skipped": skipped }))
}

/// `worldSection(m, prof, x)`.
pub fn world_section(m: &Value, prof: &Profile, x: f64) -> Vec<V3> {
    let fr = frame(crate::js::p3_of(m.get("start")), crate::js::p3_of(m.get("end")), f(m.get("rotation")));
    let voff = voff_for(crate::js::s(m.get("position")), prof.depth);
    let first = prof.loops.first().map_or(&[][..], Vec::as_slice);
    first
        .iter()
        .map(|&[u, v]| {
            [0, 1, 2].map(|i| {
                crate::js::g3(fr.o, i) + crate::js::g3(fr.x, i) * x + crate::js::g3(fr.u, i) * u + crate::js::g3(fr.v, i) * (v + voff)
            })
        })
        .collect()
}

/// Whether a truthy flag is set (`opts.k`).
pub fn flag(o: Option<&Value>, k: &str) -> bool {
    truthy(o.and_then(|o| o.get(k)))
}
