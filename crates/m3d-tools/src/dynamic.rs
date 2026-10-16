//! Components that stretch or repeat their parts (dynamic.js, one for one). Each part (a group or component inside
//! the component) follows a rule per axis kept on its placement (`inst.rule`): stretch, start, end, move, scale,
//! repeat, or auto. `stretch_def` makes a new component definition placing the parts again for a new size; it
//! remembers `dyn = {src, size}` so it can be stretched again from the original. Call inside `Model::change`.

use cc_m3d::file::{Fail, def_from_json, def_value};
use cc_m3d::model::{Model, place};
use cc_m3d::vec::{BBox, IDENT, M4, P3, mmul, translation};
use cc_m3d::{Error, Res};

use crate::js::{g3, jmax, usize_of};

const EPS: f64 = 1e-6;
/// Most copies one stretch makes. Spec gap: the JS has no cap, so a huge size over a small repeat step would place
/// millions of parts and hang the page; the port stops at this many placements per part.
pub const MAX_REPEAT: usize = 100_000;

/// A part: its placement id, and its box in the component's coordinates.
#[derive(Clone, Debug)]
pub struct Part {
    pub iid: f64,
    pub inst: cc_m3d::def::Inst,
    pub bx: BBox,
}

/// `partsOf(model, def)`.
pub fn parts_of(model: &Model, def_id: &str) -> Vec<Part> {
    let Some(def) = model.def(def_id) else { return Vec::new() };
    def.insts
        .iter()
        .filter_map(|(iid, inst)| {
            model.def(&inst.def)?;
            let bx = model.bbox(&inst.def, &inst.m)?;
            Some(Part { iid, inst: inst.clone(), bx })
        })
        .collect()
}

fn at3(p: &P3, k: usize) -> f64 {
    p.get(k).copied().unwrap_or(f64::NAN)
}

/// `ruleOf(part, k, B)`: the rule used along axis k (auto worked out).
pub fn rule_of(part: &Part, k: usize, b: &BBox) -> String {
    let r = part.inst.rule.as_ref().and_then(|r| match k {
        0 => r.x.clone(),
        1 => r.y.clone(),
        _ => r.z.clone(),
    });
    if let Some(r) = r.filter(|r| !r.is_empty() && r != "auto") {
        return r;
    }
    let l = at3(&b.max, k) - at3(&b.min, k);
    let (p0, p1) = (at3(&part.bx.min, k), at3(&part.bx.max, k));
    if l < EPS {
        return "start".into();
    }
    let ds = p0 - at3(&b.min, k);
    let de = at3(&b.max, k) - p1;
    if ds < l * 0.02 + EPS && de < l * 0.02 + EPS {
        return "stretch".into();
    }
    if (ds - de).abs() < l * 0.1 {
        return "move".into();
    }
    if ds < de { "start".into() } else { "end".into() }
}

fn axis_vec(k: usize, v: f64) -> P3 {
    match k {
        0 => [v, 0.0, 0.0],
        1 => [0.0, v, 0.0],
        _ => [0.0, 0.0, v],
    }
}

/// Scale about point o along axis k by f.
fn scale_axis(k: usize, f: f64, o: P3) -> M4 {
    let s = [if k == 0 { f } else { 1.0 }, if k == 1 { f } else { 1.0 }, if k == 2 { f } else { 1.0 }];
    mmul(
        &translation(o),
        &mmul(&[s[0], 0.0, 0.0, 0.0, 0.0, s[1], 0.0, 0.0, 0.0, 0.0, s[2], 0.0, 0.0, 0.0, 0.0, 1.0], &translation([-o[0], -o[1], -o[2]])),
    )
}

/// `stretchDef(model, src, size, name)` → the new component definition's id.
pub fn stretch_def(model: &mut Model, src: &str, size: [f64; 3], name: &str) -> Res<String> {
    let b = model.bbox(src, &IDENT).ok_or_else(|| Error::Msg("empty".into()))?;
    let l = [b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]];
    let s = [0, 1, 2].map(|k| if g3(&l, k) < EPS { g3(&l, k) } else { jmax(g3(&size, k), EPS) });
    let out = model.new_def("component", Some(name));
    // its own loose geometry is kept as it is
    let mut j = model.def(src).map(def_value).ok_or(Error::Type)?;
    if let Some(o) = j.as_object_mut() {
        o.insert("insts".into(), serde_json::Value::Array(Vec::new()));
    }
    {
        let d = model.def_mut(&out).ok_or(Error::Type)?;
        def_from_json(d, &j, Fail::Plain)?;
        d.starter = None;
        d.dyn_ = Some(cc_m3d::def::Dyn { src: src.to_string(), size: s.to_vec() });
    }
    for part in parts_of(model, src) {
        let mut m = part.inst.m;
        let mut offsets: Vec<P3> = vec![[0.0; 3]];
        for k in 0..3 {
            let (lk, sk, bk) = (g3(&l, k), g3(&s, k), g3(&b.min, k));
            if lk < EPS || (sk - lk).abs() < EPS {
                continue;
            }
            let r = rule_of(&part, k, &b);
            let (p0, p1) = (at3(&part.bx.min, k), at3(&part.bx.max, k));
            let w = p1 - p0;
            let d = sk - lk;
            match r.as_str() {
                "stretch" => {
                    let f = if w > EPS { (w + d) / w } else { 1.0 };
                    m = mmul(&scale_axis(k, jmax(f, EPS), axis_vec(k, p0)), &m);
                }
                "end" => m = mmul(&translation(axis_vec(k, d)), &m),
                "move" => m = mmul(&translation(axis_vec(k, ((p0 + p1) / 2.0 - bk) * (sk / lk - 1.0))), &m),
                "scale" => m = mmul(&scale_axis(k, sk / lk, axis_vec(k, bk)), &m),
                "repeat" => {
                    let rs = part.inst.rule.as_ref().and_then(|r| match k {
                        0 => r.sx,
                        1 => r.sy,
                        _ => r.sz,
                    });
                    let step = match rs {
                        Some(x) if x > EPS => x,
                        _ => jmax(w * 2.0, EPS),
                    };
                    let room = sk - (p0 - bk) - w;
                    let n = jmax(1.0, (room / step + 1e-9).floor() + 1.0);
                    let n = if n.is_finite() { usize_of(n.min(MAX_REPEAT as f64)) } else { 1 };
                    let mut more = Vec::new();
                    for o in &offsets {
                        for i in 0..n {
                            if more.len() >= MAX_REPEAT {
                                break;
                            }
                            let shift = axis_vec(k, i as f64 * step);
                            more.push([o[0] + shift[0], o[1] + shift[1], o[2] + shift[2]]);
                        }
                    }
                    offsets = more;
                }
                _ => {}
            }
        }
        for o in offsets {
            let mm = mmul(&translation(o), &m);
            let d = model.def_mut(&out).ok_or(Error::Type)?;
            let iid = place(d, &part.inst.def, &mm);
            if let Some(ni) = d.insts.get_mut(iid) {
                ni.name = part.inst.name.clone();
                ni.color = part.inst.color.clone();
                ni.hidden = part.inst.hidden;
                ni.tag = part.inst.tag.clone();
                ni.rule = part.inst.rule.clone();
            }
        }
    }
    Ok(out)
}
