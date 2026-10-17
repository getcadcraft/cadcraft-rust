//! refgeom.js — reference geometry (kernel-free): the plane, axis and point features worked out from what they
//! reference, through resolvers each kernel supplies. Every mode fails with a plain message when its references do
//! not fit (parallel planes for an intersection, three points on a line, …); nothing is guessed.

use crate::i18n::t as tr;
use crate::js;
use crate::jv;
use crate::vec::{self as v, Frame, V3};
use serde_json::Value;

const EPS: f64 = 1e-9;

/// A resolved plane: its frame and the centre to draw it at.
#[derive(Clone, Copy, Debug)]
pub struct PlaneRes {
    pub frame: Frame,
    pub center: Option<V3>,
}
/// A resolved face.
#[derive(Clone, Debug)]
pub struct FaceRes {
    pub ty: String,
    pub n: V3,
    pub o: V3,
    pub axis: Option<(V3, V3)>,
    pub r: f64,
}
/// A resolved edge (curved-edge helpers `near` / `mid` / `cross` are the exact kernel's: not offered here).
#[derive(Clone, Debug)]
pub struct EdgeRes {
    pub ty: String,
    pub a: V3,
    pub b: V3,
    pub axis: Option<(V3, V3)>,
}

/// The resolvers `R`.
pub trait Resolve {
    fn plane(&self, r: &Value) -> Result<PlaneRes, String>;
    fn dir(&self, r: &Value) -> Result<(V3, V3), String>;
    fn point(&self, r: &Value) -> Result<V3, String>;
    fn face(&self, r: &Value) -> Result<FaceRes, String>;
    fn edge(&self, r: &Value) -> Result<EdgeRes, String>;
}

/// What a reference feature is.
#[derive(Clone, Debug)]
pub enum Ref {
    Plane { frame: Frame, center: V3 },
    Axis { o: V3, d: V3 },
    Point { p: V3 },
}
impl Ref {
    pub fn to_json(&self) -> Value {
        match self {
            Ref::Plane { frame, center } => {
                jv::obj(vec![("kind", "plane".into()), ("frame", frame.to_json()), ("center", jv::pt3(*center))])
            }
            Ref::Axis { o, d } => jv::obj(vec![("kind", "axis".into()), ("o", jv::pt3(*o)), ("d", jv::pt3(*d))]),
            Ref::Point { p } => jv::obj(vec![("kind", "point".into()), ("p", jv::pt3(*p))]),
        }
    }
}

fn unit(a: V3, what: String) -> Result<V3, String> {
    let l = v::len(a);
    if !(l > EPS) {
        return Err(what);
    }
    Ok(v::mul(a, 1.0 / l))
}
fn project(f: &Frame, p: V3) -> V3 {
    v::sub(p, v::mul(f.n, v::dot(v::sub(p, f.o), f.n)))
}
fn plane_of(n: V3, o: V3, center: Option<V3>) -> Ref {
    Ref::Plane { frame: v::frame_for(n, o), center: center.unwrap_or(o) }
}
/// `rotate(v, u, a)` (Rodrigues).
pub fn rotate(x: V3, u: V3, a: f64) -> V3 {
    let (c, s) = (js::cos(a), js::sin(a));
    v::add(v::add(v::mul(x, c), v::mul(v::cross(u, x), s)), v::mul(u, v::dot(u, x) * (1.0 - c)))
}
fn t(k: &str) -> String {
    tr(k, &[])
}
fn get_n(f: &Value, k: &str, i: usize) -> Option<Value> {
    f.get(k).and_then(|a| a.get(i)).filter(|x| jv::truthy(Some(x))).cloned()
}

/// `refFeature(f, R)`.
pub fn ref_feature(f: &Value, r: &dyn Resolve) -> Result<Ref, String> {
    let pt = |i: usize| -> Result<V3, String> {
        let Some(x) = get_n(f, "points", i) else {
            return Err(tr("model.refgeom.pick_point", &[("n", crate::i18n::F::from(i.saturating_add(1)))]));
        };
        r.point(&x)
    };
    let pl = |i: usize| -> Result<PlaneRes, String> {
        let Some(x) = get_n(f, "planes", i) else {
            return Err(t(if i > 0 { "model.refgeom.pick_second_plane" } else { "model.refgeom.pick_plane" }));
        };
        r.plane(&x)
    };
    let fty = jv::s(f, "type");
    let mode = jv::s(f, "mode");
    let flip_on = jv::truthy(f.get("flip"));
    if fty == "point" {
        return Ok(Ref::Point {
            p: match mode {
                "xyz" => {
                    let a = jv::arr(f.get("p"));
                    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
                    [g(0), g(1), g(2)]
                }
                "at" => pt(0)?,
                "mid" => mid_of(f, r, &pt)?.0,
                "meet" => meet(f, r)?,
                "planes3" => {
                    let a = pl(0)?;
                    let b = pl(1)?;
                    let c = match get_n(f, "planes", 2) {
                        Some(x) => r.plane(&x)?,
                        None => return Err(t("model.refgeom.pick_third_plane")),
                    };
                    let (an, bn, cn) = (a.frame.n, b.frame.n, c.frame.n);
                    let p = v::solve_dense(
                        &[an.to_vec(), bn.to_vec(), cn.to_vec()],
                        &[v::dot(an, a.frame.o), v::dot(bn, b.frame.o), v::dot(cn, c.frame.o)],
                    );
                    let det = v::dot(an, v::cross(bn, cn));
                    match p {
                        Some(p) if det.abs() >= 1e-9 => [
                            p.first().copied().unwrap_or(f64::NAN),
                            p.get(1).copied().unwrap_or(f64::NAN),
                            p.get(2).copied().unwrap_or(f64::NAN),
                        ],
                        _ => return Err(t("model.refgeom.planes3_no_point")),
                    }
                }
                _ => {
                    let Some(er) = f.get("edge").filter(|x| jv::truthy(Some(x))) else { return Err(t("model.refgeom.pick_circular_edge")) };
                    let e = r.edge(er)?;
                    match (e.ty.as_str(), e.axis) {
                        ("circle", Some((o, _))) => o,
                        _ => return Err(t("model.refgeom.centre_needs_circle")),
                    }
                }
            },
        });
    }
    if fty == "axis" {
        let (o, d) = match mode {
            "2pt" => {
                let o = pt(0)?;
                let p1 = pt(1)?;
                (o, unit(v::sub(p1, o), t("model.refgeom.same_point"))?)
            }
            "cyl" => {
                let Some(fr) = f.get("face").filter(|x| jv::truthy(Some(x))) else { return Err(t("model.refgeom.pick_cyl_cone_face")) };
                let fc = r.face(fr)?;
                fc.axis.ok_or_else(|| t("model.refgeom.face_no_axis"))?
            }
            "planes" => {
                let a = pl(0)?;
                let b = pl(1)?;
                let d0 = v::cross(a.frame.n, b.frame.n);
                if v::len(d0) < 1e-9 {
                    return Err(t("model.refgeom.planes_parallel"));
                }
                let d = v::norm(d0);
                let ca = v::dot(a.frame.n, a.frame.o);
                let cb = v::dot(b.frame.n, b.frame.o);
                let cd = v::dot(d, v::mul(v::add(a.frame.o, b.frame.o), 0.5));
                let o = v::solve_dense(&[a.frame.n.to_vec(), b.frame.n.to_vec(), d.to_vec()], &[ca, cb, cd])
                    .ok_or_else(|| t("model.refgeom.planes_no_line"))?;
                ([o.first().copied().unwrap_or(f64::NAN), o.get(1).copied().unwrap_or(f64::NAN), o.get(2).copied().unwrap_or(f64::NAN)], d)
            }
            "edge" => {
                let Some(er) = f.get("edge").filter(|x| jv::truthy(Some(x))) else { return Err(t("model.refgeom.pick_edge_or_circle")) };
                let e = r.edge(er)?;
                if e.ty == "line" {
                    (e.a, unit(v::sub(e.b, e.a), t("model.refgeom.edge_no_length"))?)
                } else if let (true, Some(ax)) = (e.ty == "circle", e.axis) {
                    ax
                } else {
                    return Err(t("model.refgeom.edge_curved_axis"));
                }
            }
            "normal" => {
                let p = pl(0)?;
                (pt(0)?, p.frame.n)
            }
            _ => return Err(t("model.refgeom.unknown_axis")),
        };
        return Ok(Ref::Axis { o, d: v::norm(d) });
    }
    let flip = |n: V3| if flip_on { v::mul(n, -1.0) } else { n };
    match mode {
        "offset" => {
            let b = pl(0)?;
            let dd = jv::as_num(f.get("d")).unwrap_or(f64::NAN);
            let n = b.frame.n;
            let o = v::add(b.frame.o, v::mul(n, dd));
            Ok(plane_of(flip(n), o, Some(v::add(b.center.unwrap_or(b.frame.o), v::mul(n, dd)))))
        }
        "angle" => {
            let b = pl(0)?;
            let Some(axr) = f.get("axis").filter(|x| jv::truthy(Some(x))) else { return Err(t("model.refgeom.pick_turn_axis")) };
            let (ao, ad) = r.dir(axr)?;
            let u = v::norm(ad);
            let perp = v::sub(b.frame.n, v::mul(u, v::dot(b.frame.n, u)));
            if v::len(perp) < 1e-9 {
                return Err(t("model.refgeom.axis_square"));
            }
            let n0 = v::norm(perp);
            let ang = jv::as_num(f.get("angle")).unwrap_or(f64::NAN);
            let n = rotate(n0, u, ang * std::f64::consts::PI / 180.0);
            Ok(plane_of(flip(n), ao, Some(ao)))
        }
        "3pt" => {
            let a = pt(0)?;
            let b = pt(1)?;
            let c = pt(2)?;
            let n = v::cross(v::sub(b, a), v::sub(c, a));
            if v::len(n) < 1e-9 * js::max(1.0, v::len(v::sub(b, a)) * v::len(v::sub(c, a))) {
                return Err(t("model.refgeom.points_on_line"));
            }
            Ok(plane_of(flip(v::norm(n)), a, Some(v::mul(v::add(v::add(a, b), c), 1.0 / 3.0))))
        }
        "mid" => {
            let a = pl(0)?;
            let b = pl(1)?;
            let diff = v::sub(a.frame.n, b.frame.n);
            let (n, o) = if v::len(diff) > 1e-9 {
                let l = v::len(diff);
                let n = v::mul(diff, 1.0 / l);
                let c = (v::dot(a.frame.o, a.frame.n) - v::dot(b.frame.o, b.frame.n)) / l;
                (n, v::mul(n, c))
            } else {
                let n = a.frame.n;
                (n, v::add(a.frame.o, v::mul(n, v::dot(v::sub(b.frame.o, a.frame.o), n) / 2.0)))
            };
            let ca = a.center.unwrap_or(a.frame.o);
            let cb = b.center.unwrap_or(b.frame.o);
            let mid = v::mul(v::add(ca, cb), 0.5);
            let fr = v::frame_for(n, o);
            Ok(plane_of(flip(n), o, Some(project(&fr, mid))))
        }
        "tangent" => {
            let Some(fref) = f.get("face").filter(|x| jv::truthy(Some(x))) else { return Err(t("model.refgeom.pick_cyl_face")) };
            let fc = r.face(fref)?;
            let Some((ao, ad)) = fc.axis.filter(|_| fc.ty == "cylinder") else { return Err(t("model.refgeom.tangent_needs_cyl")) };
            let p = pt(0)?;
            let d = v::norm(ad);
            let w = v::sub(p, ao);
            let radial = v::sub(w, v::mul(d, v::dot(w, d)));
            if v::len(radial) < 1e-9 {
                return Err(t("model.refgeom.point_on_axis"));
            }
            let n = v::norm(radial);
            let foot = v::add(ao, v::mul(d, v::dot(w, d)));
            let o = v::add(foot, v::mul(n, fc.r));
            Ok(plane_of(flip(n), o, Some(o)))
        }
        "edgept" => {
            let ln = line_of(f, r, t("model.refgeom.pick_line_through"))?;
            let p = pt(0)?;
            let n = v::cross(ln.1, v::sub(p, ln.0));
            if v::len(n) < 1e-9 * js::max(1.0, v::len(v::sub(p, ln.0))) {
                return Err(t("model.refgeom.point_on_line"));
            }
            Ok(plane_of(flip(v::norm(n)), ln.0, Some(v::add(ln.0, v::mul(v::sub(p, ln.0), 0.5)))))
        }
        "midnormal" => {
            let (p, tt) = mid_of(f, r, &pt)?;
            if v::len(tt) < 1e-12 {
                return Err(t("model.refgeom.same_point_no_dir"));
            }
            Ok(plane_of(flip(v::norm(tt)), p, Some(p)))
        }
        "normal" => {
            let p = pt(0)?;
            let (at, tt) = if let Some(er) = f.get("edge").filter(|x| jv::truthy(Some(x))) {
                let e = r.edge(er)?;
                if e.ty == "line" {
                    let u = unit(v::sub(e.b, e.a), t("model.refgeom.edge_no_length"))?;
                    (v::add(e.a, v::mul(u, v::dot(v::sub(p, e.a), u))), u)
                } else {
                    return Err(t("model.refgeom.normal_curved_exact"));
                }
            } else if let Some(axr) = f.get("axis").filter(|x| jv::truthy(Some(x))) {
                let (ao, ad) = r.dir(axr)?;
                let tt = v::norm(ad);
                (v::add(ao, v::mul(tt, v::dot(v::sub(p, ao), tt))), tt)
            } else {
                return Err(t("model.refgeom.pick_normal_edge"));
            };
            Ok(plane_of(flip(v::norm(tt)), at, Some(at)))
        }
        _ => Err(t("model.refgeom.unknown_plane")),
    }
}

/// `lineOf(f, R, what)` → (o, d, Some((a, b)) for an edge).
fn line_of(f: &Value, r: &dyn Resolve, what: String) -> Result<(V3, V3, Option<(V3, V3)>), String> {
    if let Some(er) = f.get("edge").filter(|x| jv::truthy(Some(x))) {
        let e = r.edge(er)?;
        if e.ty != "line" {
            return Err(t("model.refgeom.edge_curved_line"));
        }
        return Ok((e.a, unit(v::sub(e.b, e.a), t("model.refgeom.edge_no_length"))?, Some((e.a, e.b))));
    }
    if let Some(axr) = f.get("axis").filter(|x| jv::truthy(Some(x))) {
        let (o, d) = r.dir(axr)?;
        return Ok((o, v::norm(d), None));
    }
    Err(what)
}

/// `midOf(f, R, P)` → (p, direction there).
fn mid_of(f: &Value, r: &dyn Resolve, pt: &dyn Fn(usize) -> Result<V3, String>) -> Result<(V3, V3), String> {
    if let Some(er) = f.get("edge").filter(|x| jv::truthy(Some(x))) {
        let e = r.edge(er)?;
        if e.ty == "line" {
            return Ok((v::mul(v::add(e.a, e.b), 0.5), v::sub(e.b, e.a)));
        }
        return Err(t("model.refgeom.mid_curved_exact"));
    }
    if get_n(f, "points", 0).is_none() || get_n(f, "points", 1).is_none() {
        return Err(t("model.refgeom.pick_mid"));
    }
    let a = pt(0)?;
    let b = pt(1)?;
    Ok((v::mul(v::add(a, b), 0.5), v::sub(b, a)))
}

/// `meet(f, R)`: a line or curved edge with a plane, flat face or cylinder.
fn meet(f: &Value, r: &dyn Resolve) -> Result<V3, String> {
    // surf: { type, o, n, axis, r }
    let (sty, so, sn, saxis, sr) = if let Some(fr) = f.get("face").filter(|x| jv::truthy(Some(x))) {
        let fc = r.face(fr)?;
        (fc.ty, fc.o, fc.n, fc.axis, fc.r)
    } else if let Some(p) = get_n(f, "planes", 0) {
        let pr = r.plane(&p)?;
        ("plane".to_string(), pr.frame.o, pr.frame.n, None, f64::NAN)
    } else {
        return Err(t("model.refgeom.pick_meet_surface"));
    };
    let curved = match f.get("edge").filter(|x| jv::truthy(Some(x))) {
        Some(er) => Some(r.edge(er)?),
        None => None,
    };
    if let Some(c) = &curved
        && c.ty != "line"
    {
        if sty != "plane" {
            return Err(t("model.refgeom.curved_curved"));
        }
        return Err(t("model.refgeom.curved_cross_exact"));
    }
    let ln = line_of(f, r, t("model.refgeom.pick_meet_line"))?;
    if sty == "plane" {
        let dn = v::dot(ln.1, sn);
        if dn.abs() < 1e-12 {
            return Err(t("model.refgeom.line_parallel_plane"));
        }
        return Ok(v::add(ln.0, v::mul(ln.1, v::dot(v::sub(so, ln.0), sn) / dn)));
    }
    if let (true, Some((ao, ad))) = (sty == "cylinder", saxis) {
        let a = v::norm(ad);
        let w0 = v::sub(ln.0, ao);
        let dp = v::sub(ln.1, v::mul(a, v::dot(ln.1, a)));
        let wp = v::sub(w0, v::mul(a, v::dot(w0, a)));
        let aa = v::dot(dp, dp);
        let bb = 2.0 * v::dot(dp, wp);
        let cc = v::dot(wp, wp) - sr * sr;
        if aa < 1e-18 {
            return Err(t("model.refgeom.line_along_axis"));
        }
        let disc = bb * bb - 4.0 * aa * cc;
        if disc < 0.0 {
            return Err(t("model.refgeom.line_misses_cyl"));
        }
        let q = js::sqrt(disc);
        let t0 = ln.2.map_or(0.0, |(ea, eb)| v::dot(v::sub(v::mul(v::add(ea, eb), 0.5), ln.0), ln.1));
        let mut ts = [(-bb - q) / (2.0 * aa), (-bb + q) / (2.0 * aa)];
        // [x, y].sort((x, y) => |x − t0| − |y − t0|): stable, so a tie keeps the order
        if (ts[1] - t0).abs() - (ts[0] - t0).abs() < 0.0 {
            ts.swap(0, 1);
        }
        let flip = jv::truthy(f.get("flip"));
        return Ok(v::add(ln.0, v::mul(ln.1, if flip { ts[1] } else { ts[0] })));
    }
    Err(t("model.refgeom.meet_other_curved"))
}

/// `planeQuad(g, s)`.
pub fn plane_quad(frame: &Frame, center: V3, s: f64) -> Vec<V3> {
    let c = v::to_plane(frame, center);
    [[-s, -s], [s, -s], [s, s], [-s, s]].iter().map(|[u, w]| v::to_world(frame, c[0] + u, c[1] + w)).collect()
}
