//! assembly.js `interference` (0.5 WL.33) and `motionCheck` on the solid library: pairwise common volume of the placed
//! instances, each overlap classified by the cosmetic thread zones (tubes from the minor to the major radius along each
//! thread) as 'thread', 'thread-depth' or 'overlap', touching pairs by the minimum gap; and a joint stepped through its
//! range with the interference at each step. Same calls in the same order as the JS.

use crate::asm::{self, Build, Builds};
use crate::js;
use crate::jv;
use crate::kernel_section::solid_of;
use crate::vec::{self as v, M4, V3};
use cc_manifold::{CrossSection, Manifold};
use serde_json::Value;
use std::rc::Rc;

/// Largest number of instances the pairwise check looks at (500 is the file reader's cap).
pub const MAX_INSTANCES: usize = 500;

/// What interference needs of a part's build: its mesh (positions as f32 values) and its threads (a JSON object of
/// `regen.threads`).
#[derive(Clone, Debug, Default)]
pub struct SolidBuild {
    pub positions: Option<Vec<f64>>,
    pub indices: Vec<u32>,
    pub threads: Value,
}

impl SolidBuild {
    /// From `{ mesh: { positions, indices } | null, threads: {...} }`.
    pub fn from_json(b: &Value) -> SolidBuild {
        let m = b.get("mesh").filter(|m| jv::truthy(Some(m)));
        SolidBuild {
            positions: m.map(|m| jv::arr(m.get("positions")).iter().map(|x| jv::as_num(Some(x)).unwrap_or(f64::NAN)).collect()),
            indices: m
                .map(|m| {
                    jv::arr(m.get("indices")).iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX)).collect()
                })
                .unwrap_or_default(),
            threads: b.get("threads").cloned().unwrap_or(Value::Null),
        }
    }
}

struct Solid {
    id: String,
    m: Rc<Manifold>,
    zones: Option<Option<Rc<Manifold>>>,
    b: SolidBuild,
    w: M4,
}

fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}

/// The arena (`T`): every solid of the check alive until it ends.
#[derive(Default)]
struct Arena(Vec<Rc<Manifold>>);
impl Arena {
    fn t(&mut self, m: Manifold) -> Rc<Manifold> {
        let h = Rc::new(m);
        self.0.push(Rc::clone(&h));
        h
    }
}

fn zones_of(sd: &mut Solid, ar: &mut Arena) -> Option<Rc<Manifold>> {
    if let Some(z) = &sd.zones {
        return z.clone();
    }
    let mut tubes: Vec<Rc<Manifold>> = Vec::new();
    if let Some(o) = sd.b.threads.as_object() {
        for th in o.values() {
            if jv::truthy(th.get("modelled")) {
                continue;
            }
            for z in jv::arr(th.get("list")) {
                let r1v = jv::as_num(z.get("r1")).unwrap_or(f64::NAN);
                let depth = jv::as_num(z.get("depth")).unwrap_or(f64::NAN);
                if !(r1v > 0.0) || !(depth > 0.0) {
                    continue;
                }
                let r0z = if jv::nullish(z.get("r0")) { r1v * 0.8 } else { jv::as_num(z.get("r0")).unwrap_or(f64::NAN) };
                let r0 = js::max(0.0, r0z * 0.97);
                let r1 = r1v * 1.03 + 1e-3;
                let m = js::max(0.05 * r1v, 1e-3);
                let d = v::norm(p3(z.get("d")));
                let x = v::norm(if d[0].abs() < 0.9 { v::cross(d, [1.0, 0.0, 0.0]) } else { v::cross(d, [0.0, 1.0, 0.0]) });
                let y = v::cross(d, x);
                let ring = CrossSection::circle(r1, 48).subtract(&CrossSection::circle(r0, 48));
                let e = ar.t(Manifold::extrude(&ring.to_polygons(), depth + 2.0 * m, 0, 0.0, [1.0, 1.0]));
                let tube = ar.t(e.translate([0.0, 0.0, -m]));
                let t1 = ar.t(tube.transform_mat4(&v::basis_matrix(x, y, d, p3(z.get("o")))));
                tubes.push(ar.t(t1.transform_mat4(&sd.w)));
            }
        }
    }
    let z = match tubes.as_slice() {
        [] => None,
        [one] => Some(Rc::clone(one)),
        _ => {
            let l: Vec<Manifold> = tubes.iter().map(|h| (**h).clone()).collect();
            Some(ar.t(Manifold::union(&l)))
        }
    };
    sd.zones = Some(z.clone());
    z
}

/// `interference(asm, builds, world, M, { tol, touch, touchTol })`.
pub fn interference(
    asm: &Value,
    builds: &[(String, SolidBuild)],
    world: &[(String, M4)],
    tol: f64,
    touch: bool,
    touch_tol: f64,
) -> Result<Vec<Value>, String> {
    let mut ar = Arena::default();
    let mut solids: Vec<Solid> = Vec::new();
    for i in jv::arr(asm.get("instances")).iter().take(MAX_INSTANCES) {
        let pid = js::str_of(i.get("part"));
        let Some(b) = builds.iter().find(|(k, _)| *k == pid).map(|(_, b)| b) else { continue };
        let Some(pos) = &b.positions else { continue };
        let m = ar.t(solid_of(pos, &b.indices)?);
        let iid = js::str_of(i.get("id"));
        let Some(w) = world.iter().find(|(k, _)| *k == iid).map(|(_, m)| *m) else {
            return Err("Cannot convert undefined to a JS array".into());
        };
        let placed = ar.t(m.transform_mat4(&w));
        solids.push(Solid { id: iid, m: placed, zones: None, b: b.clone(), w });
    }
    let mut out = Vec::new();
    let pad = if touch { touch_tol } else { 0.0 };
    for x in 0..solids.len() {
        for y in x.saturating_add(1)..solids.len() {
            let (Some(sa), Some(sb)) = (solids.get(x), solids.get(y)) else { continue };
            let (a_lo, a_hi) = sa.m.bounding_box();
            let (b_lo, b_hi) = sb.m.bounding_box();
            if a_hi
                .iter()
                .zip(b_lo.iter())
                .zip(b_hi.iter().zip(a_lo.iter()))
                .any(|((&ah, &bl), (&bh, &al))| ah < bl - tol - pad || bh < al - tol - pad)
            {
                continue;
            }
            let (am, bm) = (Rc::clone(&sa.m), Rc::clone(&sb.m));
            let (aid, bid) = (sa.id.clone(), sb.id.clone());
            let common = ar.t(am.intersect(&bm));
            let vol = common.volume();
            if vol > tol {
                let za = solids.get_mut(x).and_then(|s| zones_of(s, &mut ar));
                let zb = solids.get_mut(y).and_then(|s| zones_of(s, &mut ar));
                let small = js::max(tol, 1e-4 * vol);
                let mut kind = "overlap";
                if za.is_some() || zb.is_some() {
                    let any = match (&za, &zb) {
                        (Some(a), Some(b)) => ar.t(a.add(b)),
                        (Some(a), None) => Rc::clone(a),
                        (None, Some(b)) => Rc::clone(b),
                        (None, None) => continue,
                    };
                    let rest = ar.t(common.subtract(&any));
                    if rest.volume() <= small {
                        let both = match (&za, &zb) {
                            (Some(a), Some(b)) => {
                                let i2 = ar.t(a.intersect(b));
                                let r2 = ar.t(common.subtract(&i2));
                                r2.volume() <= small
                            }
                            _ => false,
                        };
                        kind = if both { "thread" } else { "thread-depth" };
                    }
                }
                out.push(jv::obj(vec![("a", aid.into()), ("b", bid.into()), ("volume", jv::num(vol)), ("kind", kind.into())]));
            } else if touch && am.min_gap(&bm, touch_tol) <= touch_tol {
                out.push(jv::obj(vec![("a", aid.into()), ("b", bid.into()), ("volume", 0.into()), ("kind", "touch".into())]));
            }
        }
    }
    Ok(out)
}

/// Largest number of motion steps.
pub const MAX_STEPS: usize = 1000;

/// `motionCheck(asm, builds, M, jointId, from, to, steps, key)` → `[{ value, pairs }]` (the joint's value restored).
pub fn motion_check(
    asm: &Value,
    builds: &Builds,
    solids: &[(String, SolidBuild)],
    joint_id: &str,
    from: f64,
    to: f64,
    steps: f64,
    key: Option<&str>,
) -> Result<Vec<Value>, String> {
    let joints = jv::arr(asm.get("joints"));
    let Some(ji) = joints.iter().position(|j| jv::is_str(j.get("id"), joint_id)) else {
        return Err(crate::i18n::t("model.assembly.no_joint", &[]));
    };
    let j = joints.get(ji).cloned().unwrap_or(Value::Null);
    let key = key
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| asm::joint_dof(jv::s(&j, "type")).and_then(|d| d.first().copied()).unwrap_or("angle").to_string());
    let (mut from, mut to) = (from, to);
    if let Some((lo, hi)) = asm::limit_of(&j, Some(&key)) {
        from = js::max(lo, js::min(hi, from));
        to = js::max(lo, js::min(hi, to));
    }
    // `for (s = 0; s <= steps; s++)`: none for a negative or NaN count
    let mut work = asm.clone();
    let mut out = Vec::new();
    if !(steps >= 0.0) {
        return Ok(out);
    }
    let n = js::count(steps.floor() + 1.0, MAX_STEPS + 1);
    for s in 0..n {
        let value = from + (to - from) * js::f(s) / steps;
        if let Some(jj) = work.get_mut("joints").and_then(Value::as_array_mut).and_then(|a| a.get_mut(ji)) {
            jv::set(jj, &key, jv::num(value));
        }
        let (world, _) = asm::place_all(&work, builds);
        let pairs = interference(&work, solids, &world, 1e-6, false, 1e-3)?;
        out.push(jv::obj(vec![("value", jv::num(value)), ("pairs", Value::Array(pairs))]));
    }
    Ok(out)
}

/// A build for placement and interference from one JSON object (`build` fields + `mesh` + `threads`).
pub fn builds_from_json(o: &Value) -> (Builds, Vec<(String, SolidBuild)>) {
    let mut b = Builds::new();
    let mut s = Vec::new();
    if let Some(m) = o.as_object() {
        for (k, x) in m {
            b.insert(k.clone(), Build::from_json(x));
            s.push((k.clone(), SolidBuild::from_json(x)));
        }
    }
    (b, s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_assembly_has_no_pairs() {
        let asm = serde_json::json!({ "instances": [], "joints": [] });
        assert!(super::interference(&asm, &[], &[], 1e-6, false, 1e-3).is_ok_and(|v| v.is_empty()));
    }
}
