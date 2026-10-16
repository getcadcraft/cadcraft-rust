//! Picking and inference snapping (infer.js, one for one): `infer` (what the cursor snaps to), `pick` (what a click
//! selects in the open context), `pick_window` (window / crossing selection), the dimension and section-plane
//! layouts and their picking, and `along_line`. Inference kinds: end, mid, center, guidept, origin (points) · lock ·
//! axis-x/y/z · parallel, perp · edge, guide · face · plane · ground · none. A point snap wins inside its radius, then
//! axis/parallel/perpendicular, then on-edge, then on-face; things hidden behind a face do not snap.

use std::rc::Rc;

use cc_m3d::model::{Dim, Guide, Model, RadC, Radial};
use cc_m3d::ops::Sel;
use cc_m3d::vec::{
    IDENT, M4, P3, Plane, add, closest_on_line, cross, dist, dot, len, mul, norm, plane_from, ray_plane, ray_to_segment, sub,
};

use crate::camera::{Camera, Ray};
use crate::i18n::{t, t0};
use crate::js::{hypot2, jmax, jmin, min_of, num_str, round};
use crate::scene::{Hit, ObjRef, SEdge, SFace, Scene, cut_away};

/// `TIPS[kind]`: an inference's tip in the page's language.
pub fn tip(kind: &str) -> String {
    let k: String =
        kind.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '_' }).collect::<String>();
    // /[^a-z0-9]+/g → '_': runs collapse to one
    let mut out = String::new();
    for c in k.chars() {
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    t0(&format!("m3d.tip.{out}"))
}

/// Closest points between two infinite lines; None when parallel: (t, s, p1, p2).
pub fn line_line(o1: P3, d1: P3, o2: P3, d2: P3) -> Option<(f64, f64, P3, P3)> {
    let w = sub(o1, o2);
    let (a, b, c, d, e) = (dot(d1, d1), dot(d1, d2), dot(d2, d2), dot(d1, w), dot(d2, w));
    let den = a * c - b * b;
    if den.abs() < 1e-14 * a * c {
        return None;
    }
    let tt = (b * e - c * d) / den;
    let s = (a * e - b * d) / den;
    Some((tt, s, add(o1, mul(d1, tt)), add(o2, mul(d2, s))))
}

/// The drawing axes in the open context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axes3 {
    pub x: P3,
    pub y: P3,
    pub z: P3,
}
pub const WORLD_AXES: Axes3 = Axes3 { x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };

/// What `skip(o, 'e'|'f')` answers.
pub type SkipFn<'a> = dyn Fn(ObjRef<'_>, char) -> bool + 'a;

/// infer()'s options.
pub struct InferOpts<'a> {
    pub radius: Option<f64>,
    pub axes: Option<Axes3>,
    pub skip: Option<&'a SkipFn<'a>>,
    /// false: no points, axes or edges
    pub snap: bool,
    pub from: Option<P3>,
    pub lock_dir: Option<P3>,
    pub lock_kind: Option<String>,
    /// an edge {a, b} for parallel / perpendicular
    pub ref_edge: Option<(P3, P3)>,
    pub plane: Option<Plane>,
    pub origin: Option<P3>,
    pub ground: Option<Plane>,
}

impl Default for InferOpts<'_> {
    fn default() -> Self {
        InferOpts {
            radius: None,
            axes: None,
            skip: None,
            snap: true,
            from: None,
            lock_dir: None,
            lock_kind: None,
            ref_edge: None,
            plane: None,
            origin: None,
            ground: None,
        }
    }
}

/// What the cursor snaps to.
#[derive(Clone, Debug)]
pub struct Snap {
    pub p: P3,
    pub kind: String,
    pub tip: String,
    pub edge: Option<Rc<SEdge>>,
    pub guide: Option<f64>,
    pub face: Option<Rc<SFace>>,
    pub dir: Option<P3>,
    pub lock_kind: Option<String>,
    /// only a point (a tool's `{p}`)
    pub bare: bool,
}
impl Snap {
    fn new(p: P3, kind: &str, tip: String) -> Snap {
        Snap { p, kind: kind.to_string(), tip, edge: None, guide: None, face: None, dir: None, lock_kind: None, bare: false }
    }
    /// `{p}`: a point and nothing else.
    pub fn bare(p: P3) -> Snap {
        let mut s = Snap::new(p, "", String::new());
        s.bare = true;
        s
    }
    /// For the parity harness: {p, kind, tip, edge?, guide?, face?, dir?, lockKind?} (edge/face as {path, id}).
    pub fn to_json(&self) -> serde_json::Value {
        use crate::outj::{num, nums, p3};
        let mut o = serde_json::Map::new();
        o.insert("p".into(), p3(&self.p));
        if self.bare {
            return serde_json::Value::Object(o);
        }
        o.insert("kind".into(), self.kind.clone().into());
        o.insert("tip".into(), self.tip.clone().into());
        if let Some(e) = &self.edge {
            o.insert("edge".into(), serde_json::json!({"path": nums(&e.path), "id": num(e.id)}));
        }
        if let Some(g) = self.guide {
            o.insert("guide".into(), num(g));
        }
        if let Some(f) = &self.face {
            o.insert("face".into(), serde_json::json!({"path": nums(&f.path), "id": num(f.id)}));
        }
        if let Some(d) = &self.dir {
            o.insert("dir".into(), p3(d));
        }
        if let Some(k) = &self.lock_kind {
            o.insert("lockKind".into(), k.clone().into());
        }
        serde_json::Value::Object(o)
    }
}

struct Cand {
    p: P3,
    kind: &'static str,
    d: f64,
    edge: Option<Rc<SEdge>>,
    face: Option<Rc<SFace>>,
    guide: Option<f64>,
    gline: Option<(P3, P3)>,
}

fn sort_d(list: &mut [Cand]) {
    // V8's sort is stable; so is this one (the keys are finite: only d < R joins)
    list.sort_by(|a, b| a.d.partial_cmp(&b.d).unwrap_or(std::cmp::Ordering::Equal));
}

/// `infer(sc, cam, x, y, opts)`.
pub fn infer(sc: &Scene, cam: &Camera, x: f64, y: f64, opts: &InferOpts<'_>) -> Snap {
    let rr = opts.radius.unwrap_or(12.0);
    let ray = cam.ray(x, y);
    let vp = cam.view_proj();
    let axes = opts.axes.unwrap_or(WORLD_AXES);
    let skip = |o: ObjRef<'_>, k: char| opts.skip.is_some_and(|s| s(o, k));
    let visible = |f: &SFace| !skip(ObjRef { path: &f.path, id: f.id }, 'f');
    let hit = sc.hit(&ray, Some(&visible));
    let close: Vec<Rc<SEdge>> = if opts.snap { sc.near(cam, &ray, rr + 4.0) } else { Vec::new() };
    let ax0 = cam.axes().z;
    let depth_ok = |p: P3| -> bool {
        if hit.is_none() {
            return true;
        }
        let (o, d, tp);
        if cam.persp {
            o = cam.eye;
            tp = dist(p, o);
            d = mul(sub(p, o), 1.0 / tp);
        } else {
            d = mul(ax0, -1.0);
            o = sub(p, mul(d, cam.far));
            tp = cam.far;
        }
        let h2 = sc.hit(&Ray { o, d }, Some(&visible));
        h2.is_none_or(|h| h.t >= tp - (tp * 1e-4 + 1e-5))
    };
    let sd = |p: P3| -> f64 {
        match cam.project_vp(p, &vp) {
            Some(s) => hypot2(s[0] - x, s[1] - y),
            None => f64::INFINITY,
        }
    };

    // points
    let mut pts: Vec<Cand> = Vec::new();
    let mut try_pt = |p: P3, kind: &'static str, bias: f64, edge: Option<Rc<SEdge>>, face: Option<Rc<SFace>>, guide: Option<f64>| {
        if cut_away(sc.keep.as_ref(), p) {
            return;
        }
        let d = sd(p) + bias;
        if d < rr {
            pts.push(Cand { p, kind, d, edge, face, guide, gline: None });
        }
    };
    let snap = opts.snap;
    for e in &close {
        if skip(ObjRef { path: &e.path, id: e.id }, 'e') {
            continue;
        }
        try_pt(e.a, "end", -3.0, Some(e.clone()), None, None);
        try_pt(e.b, "end", -3.0, Some(e.clone()), None, None);
        try_pt(mul(add(e.a, e.b), 0.5), "mid", 0.0, Some(e.clone()), None, None);
    }
    if snap && let Some(h) = &hit {
        try_pt(face_center(&h.f), "center", 0.0, None, Some(h.f.clone()), None);
    }
    if snap {
        for (gid, g) in &sc.guides {
            if let Guide::Point { p } = g {
                try_pt(*p, "guidept", -1.0, None, None, Some(*gid));
            }
        }
        try_pt(opts.origin.unwrap_or([0.0, 0.0, 0.0]), "origin", 1.0, None, None, None);
    }
    sort_d(&mut pts);
    let pt = pts.into_iter().take(6).find(|c| depth_ok(c.p));

    // a locked direction keeps the point on that line
    if let (Some(from), Some(ld)) = (opts.from, opts.lock_dir) {
        let d = norm(ld);
        let p = match &pt {
            Some(c) => closest_on_line(c.p, from, d).1,
            None => line_line(ray.o, ray.d, from, d).map_or(from, |l| l.3),
        };
        let tp = match &pt {
            Some(c) => t("m3d.tip.locked_on", &[("tip", &tip(c.kind))]),
            None => tip("lock"),
        };
        let mut s = Snap::new(p, "lock", tp);
        s.dir = Some(d);
        s.lock_kind = opts.lock_kind.clone();
        return s;
    }
    let face_at = |p: P3| -> Option<Rc<SFace>> { hit.as_ref().filter(|h| on_face(&h.f, p)).map(|h| h.f.clone()) };
    if let Some(c) = pt {
        let mut s = Snap::new(c.p, c.kind, tip(c.kind));
        s.edge = c.edge;
        s.guide = c.guide;
        s.face = c.face.or_else(|| face_at(c.p));
        return s;
    }

    // lines from the start point
    struct Ax {
        p: P3,
        kind: &'static str,
        d: f64,
        dir: P3,
    }
    let mut ax: Option<Ax> = None;
    if let Some(from) = opts.from
        && snap
    {
        let mut cands: Vec<(&'static str, P3)> = vec![("axis-x", axes.x), ("axis-y", axes.y), ("axis-z", axes.z)];
        if let Some((ra, rb)) = opts.ref_edge {
            let u = norm(sub(rb, ra));
            if crate::js::truthy(len(u)) {
                cands.push(("parallel", u));
                let pn = opts.plane.map_or(axes.z, |p| p.n);
                let pp = norm(cross(pn, u));
                if crate::js::truthy(len(pp)) {
                    cands.push(("perp", pp));
                }
            }
        }
        for (kind, dir) in cands {
            let Some(ll) = line_line(ray.o, ray.d, from, dir) else { continue };
            if ll.0 < 0.0 {
                continue;
            }
            let d = sd(ll.3);
            if d < rr && ax.as_ref().is_none_or(|a| d < a.d) && dist(ll.3, from) > 1e-9 {
                ax = Some(Ax { p: ll.3, kind, d, dir: norm(dir) });
            }
        }
    }

    // on an edge or a guide line
    let mut ons: Vec<Cand> = Vec::new();
    for e in &close {
        if skip(ObjRef { path: &e.path, id: e.id }, 'e') {
            continue;
        }
        let r = ray_to_segment(ray.o, ray.d, e.a, e.b);
        if r.0 < 0.0 || cut_away(sc.keep.as_ref(), r.3) {
            continue;
        }
        let d = sd(r.3);
        if d < rr * 0.75 {
            ons.push(Cand { p: r.3, kind: "edge", d, edge: Some(e.clone()), face: None, guide: None, gline: None });
        }
    }
    if snap {
        for (gid, g) in &sc.guides {
            let Guide::Line { p, d: gd } = g else { continue };
            let Some(ll) = line_line(ray.o, ray.d, *p, *gd) else { continue };
            if ll.0 < 0.0 {
                continue;
            }
            let d = sd(ll.3);
            if d < rr * 0.75 {
                ons.push(Cand { p: ll.3, kind: "guide", d, edge: None, face: None, guide: Some(*gid), gline: Some((*p, *gd)) });
            }
        }
    }
    sort_d(&mut ons);
    let on = ons.into_iter().take(6).find(|c| depth_ok(c.p));

    if let (Some(a), Some(o), Some(from)) = (&ax, &on, opts.from) {
        let (oa, od) = match (&o.edge, &o.gline) {
            (Some(e), _) => (e.a, sub(e.b, e.a)),
            (None, Some((gp, gd))) => (*gp, *gd),
            _ => ([f64::NAN; 3], [f64::NAN; 3]),
        };
        if let Some(ll) = line_line(from, a.dir, oa, od)
            && dist(ll.2, ll.3) < 1e-6
            && sd(ll.2) < rr
        {
            let key = if o.kind == "guide" { "m3d.tip.axis_guide" } else { "m3d.tip.axis_edge" };
            let mut s = Snap::new(ll.2, a.kind, t(key, &[("axis", &tip(a.kind))]));
            s.dir = Some(a.dir);
            s.edge = o.edge.clone();
            return s;
        }
    }
    if let Some(a) = &ax
        && on.as_ref().is_none_or(|o| a.d <= o.d)
    {
        let mut s = Snap::new(a.p, a.kind, tip(a.kind));
        s.dir = Some(a.dir);
        return s;
    }
    if let Some(o) = on {
        let mut s = Snap::new(o.p, o.kind, tip(o.kind));
        s.face = face_at(o.p);
        s.edge = o.edge;
        s.guide = o.guide;
        return s;
    }
    if let Some(h) = &hit {
        let mut s = Snap::new(h.p, "face", tip("face"));
        s.face = Some(h.f.clone());
        return s;
    }
    // nothing under the cursor: the drawing plane, or the ground
    let pl = match (opts.plane, opts.from) {
        (Some(p), _) => p,
        (None, Some(from)) => horizontal_or_facing(cam, from),
        (None, None) => opts.ground.unwrap_or(Plane { n: [0.0, 0.0, 1.0], d: 0.0 }),
    };
    if let Some((gt, gp)) = ray_plane(ray.o, ray.d, &pl)
        && gt > 0.0
    {
        let (kind, tp) = if opts.plane.is_some() {
            ("plane", tip("plane"))
        } else {
            ("ground", if opts.from.is_some() { String::new() } else { tip("ground") })
        };
        return Snap::new(gp, kind, tp);
    }
    let fb = ray_plane(ray.o, ray.d, &plane_from(cam.axes().z, opts.from.unwrap_or(cam.target)));
    Snap::new(fb.map_or(cam.target, |f| f.1), "none", String::new())
}

/// The area centroid of a scene face.
pub fn face_center(f: &SFace) -> P3 {
    let mut aa = 0.0;
    let mut c = [0.0, 0.0, 0.0];
    for [a, b, d] in &f.tris {
        let w = len(cross(sub(*b, *a), sub(*d, *a)));
        aa += w;
        c = add(c, mul(add(add(*a, *b), *d), w / 3.0));
    }
    if aa > 0.0 { mul(c, 1.0 / aa) } else { f.tris.first().map_or([f64::NAN; 3], |t| t[0]) }
}

/// Is p on scene face f: in its plane and inside its outline, or on it?
pub fn on_face(f: &SFace, p: P3) -> bool {
    let tol = 1e-6 * jmax(jmax(jmax(1.0, p[0].abs()), p[1].abs()), p[2].abs());
    if (dot(f.n, p) - f.plane.d).abs() > tol {
        return false;
    }
    f.tris.iter().any(|[a, b, c]| tri_dist(p, *a, *b, *c) <= tol)
}

/// Distance from p to triangle abc (Ericson, Real-Time Collision Detection 5.1.5).
fn tri_dist(p: P3, a: P3, b: P3, c: P3) -> f64 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return dist(p, a);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return dist(p, b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return dist(p, add(a, mul(ab, d1 / (d1 - d3))));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return dist(p, c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return dist(p, add(a, mul(ac, d2 / (d2 - d6))));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return dist(p, add(b, mul(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))));
    }
    let den = 1.0 / (va + vb + vc);
    dist(p, add(a, add(mul(ab, vb * den), mul(ac, vc * den))))
}

/// A horizontal plane through p, unless we look at it edge-on — then a vertical one facing us.
fn horizontal_or_facing(cam: &Camera, p: P3) -> Plane {
    let z = cam.axes().z;
    if z[2].abs() > 0.08 {
        return Plane { n: [0.0, 0.0, 1.0], d: p[2] };
    }
    let n = norm([z[0], z[1], 0.0]);
    Plane { n, d: dot(n, p) }
}

fn starts_with(path: &[f64], pre: &[f64]) -> bool {
    pre.iter().enumerate().all(|(i, x)| path.get(i) == Some(x))
}

fn as_sel(path: &[f64], ctx: &[f64], t: &str, id: f64) -> Sel {
    if path.len() > ctx.len() { Sel::new("i", path.get(ctx.len()).copied().unwrap_or(f64::NAN)) } else { Sel::new(t, id) }
}

/// The thing a click selects inside the open context: {t: e|f|i|g, id} or None.
pub fn pick(sc: &Scene, cam: &Camera, x: f64, y: f64, path: &[f64], radius: f64) -> Option<Sel> {
    let ray = cam.ray(x, y);
    let vp = cam.view_proj();
    let in_ctx = |f: &SFace| starts_with(&f.path, path);
    let hit: Option<Hit> = sc.hit(&ray, Some(&in_ctx));
    let mut best: Option<(f64, Sel)> = None;
    for e in sc.near(cam, &ray, radius + 1.0) {
        if !starts_with(&e.path, path) || e.soft {
            continue;
        }
        let r = ray_to_segment(ray.o, ray.d, e.a, e.b);
        if r.0 < 0.0 || cut_away(sc.keep.as_ref(), r.3) {
            continue;
        }
        let Some(s) = cam.project_vp(r.3, &vp) else { continue };
        let d = hypot2(s[0] - x, s[1] - y);
        if d > radius || best.as_ref().is_some_and(|b| d >= b.0) {
            continue;
        }
        if let Some(h) = &hit
            && r.0 > h.t + jmax(1e-4, h.t * 1e-3)
        {
            continue;
        }
        best = Some((d, as_sel(&e.path, path, "e", e.id)));
    }
    if let Some((_, s)) = best {
        return Some(s);
    }
    if let Some(h) = &hit {
        return Some(as_sel(&h.f.path, path, "f", h.f.id));
    }
    for (gid, g) in &sc.guides {
        let q = match g {
            Guide::Point { p } => *p,
            Guide::Line { p, d } => line_line(ray.o, ray.d, *p, *d).map_or(*p, |l| l.3),
        };
        if let Some(s) = cam.project_vp(q, &vp)
            && hypot2(s[0] - x, s[1] - y) <= radius
        {
            return Some(Sel::new("g", *gid));
        }
    }
    None
}

/// A window rectangle in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Window (wholly inside) or crossing (touching) selection.
pub fn pick_window(sc: &Scene, cam: &Camera, rect: Rect, crossing: bool, path: &[f64]) -> Vec<Sel> {
    let vp = cam.view_proj();
    let (x0, x1) = (jmin(rect.x0, rect.x1), jmax(rect.x0, rect.x1));
    let (y0, y1) = (jmin(rect.y0, rect.y1), jmax(rect.y0, rect.y1));
    let inside = |s: &[f64; 3]| s[0] >= x0 && s[0] <= x1 && s[1] >= y0 && s[1] <= y1;
    let seg_in = |a: P3, b: P3| -> bool {
        let (Some(aa), Some(bb)) = (cam.project_vp(a, &vp), cam.project_vp(b, &vp)) else { return false };
        if crossing { seg_rect([aa[0], aa[1]], [bb[0], bb[1]], x0, y0, x1, y1) } else { inside(&aa) && inside(&bb) }
    };
    // insertion-ordered items keyed by t + id
    let mut keys: Vec<String> = Vec::new();
    let mut items: std::collections::HashMap<String, (Sel, bool, bool)> = std::collections::HashMap::new();
    let mut put = |k: String, sel: Sel, all: bool, any: bool| {
        let it = items.entry(k.clone()).or_insert_with(|| {
            keys.push(k);
            (sel, true, false)
        });
        it.1 = it.1 && all;
        it.2 = it.2 || any;
    };
    for e in sc.edges() {
        if !starts_with(&e.path, path) {
            continue;
        }
        let deeper = e.path.len() > path.len();
        let sel = as_sel(&e.path, path, "e", e.id);
        if !deeper && e.soft {
            continue;
        }
        let ok = seg_in(e.a, e.b);
        put(format!("{}{}", sel.t, num_str(sel.id)), sel, ok, ok);
    }
    for f in sc.faces() {
        if !starts_with(&f.path, path) {
            continue;
        }
        let sel = as_sel(&f.path, path, "f", f.id);
        let (mut all, mut any) = (true, false);
        for [a, b, c] in &f.tris {
            for (p, q) in [(*a, *b), (*b, *c), (*c, *a)] {
                let o = seg_in(p, q);
                all = all && o;
                any = any || o;
            }
        }
        if crossing && !any {
            let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
            for [a, b, cc] in &f.tris {
                if let (Some(aa), Some(bb), Some(c3)) = (cam.project_vp(*a, &vp), cam.project_vp(*b, &vp), cam.project_vp(*cc, &vp))
                    && in_tri2(c, [aa[0], aa[1]], [bb[0], bb[1]], [c3[0], c3[1]])
                {
                    any = true;
                    break;
                }
            }
        }
        put(format!("{}{}", sel.t, num_str(sel.id)), sel, all, any);
    }
    keys.iter().filter_map(|k| items.get(k)).filter(|it| if crossing { it.2 } else { it.1 }).map(|it| it.0.clone()).collect()
}

fn in_tri2(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let s = |u: [f64; 2], v: [f64; 2], w: [f64; 2]| (v[0] - u[0]) * (w[1] - u[1]) - (v[1] - u[1]) * (w[0] - u[0]);
    let (d1, d2, d3) = (s(a, b, p), s(b, c, p), s(c, a, p));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Does segment A–B touch the rectangle? (Liang–Barsky)
pub fn seg_rect(a: [f64; 2], b: [f64; 2], x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    let (mut t0, mut t1) = (0.0, 1.0);
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    for (p, q) in [(-dx, a[0] - x0), (dx, x1 - a[0]), (-dy, a[1] - y0), (dy, y1 - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return false;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return false;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    true
}

// ---- dimensions and section planes as drawn ----

/// A dimension as drawn.
#[derive(Clone, Debug, Default)]
pub struct DimLayout {
    pub kind: String,
    /// the length (metres) or the angle (radians)
    pub l: f64,
    /// screen point where the text goes
    pub m: Option<Vec<f64>>,
    /// screen segments (for picking)
    pub segs: Vec<([f64; 3], [f64; 3])>,
    // linear
    pub pa: Option<P3>,
    pub pb: Option<P3>,
    pub qa: Option<P3>,
    pub qb: Option<P3>,
    pub a: Option<[f64; 3]>,
    pub b: Option<[f64; 3]>,
    // radial
    pub c: Option<P3>,
    pub p: Option<P3>,
    pub q: Option<P3>,
    pub t: Option<P3>,
    pub live: bool,
    // radial and angle
    pub lines: Vec<(P3, P3)>,
    pub arrows: Vec<(P3, P3)>,
    // angle
    pub o: Option<P3>,
    pub arc: Vec<P3>,
}

/// `dimLayout(model, dm, cam)`: may re-attach the dimension's ends (as model.dimEnds does in the JS).
pub fn dim_layout(model: &Model, dm: &mut Dim, cam: &Camera, vp: &M4) -> DimLayout {
    match dm {
        Dim::Rad { kind, c, off } => {
            let g = radial_of(model, c);
            radial_layout(g.as_ref(), kind, *off, cam, vp)
        }
        Dim::Ang { off, .. } => {
            let off = *off;
            let e = model.dim_ends(dm);
            let get = |i: usize| e.get(i).copied().unwrap_or([f64::NAN; 3]);
            angle_layout(get(0), get(1), get(2), off, cam, vp)
        }
        Dim::Lin { off, .. } => {
            let off0 = *off;
            let e = model.dim_ends(dm);
            let pa = e.first().copied().unwrap_or([f64::NAN; 3]);
            let pb = e.get(1).copied().unwrap_or([f64::NAN; 3]);
            lin_layout(pa, pb, off0, cam, vp)
        }
    }
}

/// A linear dimension's layout from its two points and offset.
pub fn lin_layout(pa: P3, pb: P3, off0: P3, cam: &Camera, vp: &M4) -> DimLayout {
    let ab = sub(pb, pa);
    let l = len(ab);
    let mut off = off0;
    if l > 1e-12 {
        let u = mul(ab, 1.0 / l);
        off = sub(off, mul(u, dot(off, u)));
    }
    let qa = add(pa, off);
    let qb = add(pb, off);
    let a = cam.project_vp(qa, vp);
    let b = cam.project_vp(qb, vp);
    let (m, segs) = match (a, b) {
        (Some(a), Some(b)) => (Some(vec![(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]), vec![(a, b)]),
        _ => (None, Vec::new()),
    };
    DimLayout {
        kind: "len".into(),
        l,
        m,
        segs,
        pa: Some(pa),
        pb: Some(pb),
        qa: Some(qa),
        qb: Some(qb),
        a,
        b,
        live: true,
        ..Default::default()
    }
}

/// `dm.c.path && model.radialAt ? model.radialAt(dm.c) : {c: dm.c.p, r: dm.c.r, n: dm.c.n}`.
pub fn radial_of(model: &Model, c: &mut RadC) -> Option<Radial> {
    if c.at.is_some() {
        return model.radial_at(c);
    }
    Some(Radial { c: c.p.unwrap_or([f64::NAN; 3]), r: c.r, n: c.n, live: true, flat: false, arc: false })
}

/// Two directions square to n, the first toward `hint` when it is not along n (infer.js `planeAxes`).
fn plane_axes_hint(n: P3, hint: Option<P3>) -> (P3, P3) {
    let h = hint.unwrap_or([1.0, 0.0, 0.0]);
    let mut u = sub(h, mul(n, dot(h, n)));
    if len(u) < 1e-9 {
        u = if n[0].abs() < 0.9 { cross(n, [1.0, 0.0, 0.0]) } else { cross(n, [0.0, 1.0, 0.0]) };
    }
    let u = norm(u);
    (u, cross(n, u))
}

/// A radius or diameter dimension as drawn (g from model.radialAt).
pub fn radial_layout(g: Option<&Radial>, kind: &str, off: P3, cam: &Camera, vp: &M4) -> DimLayout {
    let Some(g) = g else {
        return DimLayout { kind: kind.into(), l: 0.0, live: true, ..Default::default() };
    };
    let n = norm(g.n.unwrap_or([f64::NAN; 3]));
    let mut w = sub(off, mul(n, dot(off, n)));
    if len(w) < 1e-9 {
        w = mul(plane_axes_hint(n, None).0, g.r * 1.5);
    }
    let u = norm(w);
    let p = add(g.c, mul(u, g.r));
    let q = add(g.c, mul(u, -g.r));
    let tt = add(g.c, w);
    let out = len(w) > g.r;
    let dia = kind == "dia";
    let lines = vec![(if dia { q } else { g.c }, if out { tt } else { p })];
    let mut arrows = vec![(p, g.c)];
    if dia {
        arrows.push((q, g.c));
    }
    let segs = lines.iter().filter_map(|(a, b)| Some((cam.project_vp(*a, vp)?, cam.project_vp(*b, vp)?))).collect();
    let m = cam.project_vp(tt, vp).map(|s| s.to_vec());
    DimLayout {
        kind: kind.into(),
        c: Some(g.c),
        p: Some(p),
        q: Some(q),
        t: Some(tt),
        l: if dia { 2.0 * g.r } else { g.r },
        live: g.live,
        lines,
        arrows,
        m,
        segs,
        ..Default::default()
    }
}

/// An angle dimension as drawn: ends [o, a, b] (world); off (world, from o) picks the angle and the arc's radius.
pub fn angle_layout(o: P3, a: P3, b: P3, off: P3, cam: &Camera, vp: &M4) -> DimLayout {
    let u = norm(sub(a, o));
    let v0 = norm(sub(b, o));
    let n = cross(u, v0);
    let empty = DimLayout { kind: "ang".into(), o: Some(o), l: 0.0, live: true, ..Default::default() };
    if len(n) < 1e-9 || !crate::js::truthy(len(u)) || !crate::js::truthy(len(v0)) {
        return empty;
    }
    let n = norm(n);
    let e2 = cross(n, u);
    let th = libm::atan2(dot(v0, e2), dot(v0, u));
    let w = sub(off, mul(n, dot(off, n)));
    let r = jmax(len(w), 1e-6);
    let mut phi = libm::atan2(dot(w, e2), dot(w, u));
    let pi = std::f64::consts::PI;
    if phi < 0.0 {
        phi += 2.0 * pi;
    }
    let ss = [(0.0, th), (th, pi - th), (pi, th), (pi + th, pi - th)];
    let (s0, sw) = ss.iter().copied().find(|(s, k)| phi >= s - 1e-12 && phi <= s + k + 1e-12).unwrap_or(ss[0]);
    let at = |ang: f64, rr: f64| add(o, add(mul(u, rr * cc_m3d::jsmath::fd_cos(ang)), mul(e2, rr * cc_m3d::jsmath::fd_sin(ang))));
    let nn = jmax(4.0, (sw / (pi / 48.0)).ceil());
    // Array.from({length: N + 1}): N is at most a few hundred for a finite sweep; NaN / huge → no points
    let count = if nn.is_finite() && nn < 1e6 { nn as usize } else { 0 };
    let arc: Vec<P3> = (0..=count).map(|i| at(s0 + (sw * i as f64) / nn, r)).collect();
    let lines = vec![(o, at(s0, r * 1.08)), (o, at(s0 + sw, r * 1.08))];
    let g = |i: usize| arc.get(i).copied().unwrap_or([f64::NAN; 3]);
    let arrows = vec![(g(0), g(1)), (g(count), g(count.wrapping_sub(1)))];
    let mut segs = Vec::new();
    for i in 0..count {
        if let (Some(aa), Some(bb)) = (cam.project_vp(g(i), vp), cam.project_vp(g(i + 1), vp)) {
            segs.push((aa, bb));
        }
    }
    let m = cam.project_vp(at(s0 + sw / 2.0, r), vp).map(|s| s.to_vec());
    DimLayout { kind: "ang".into(), o: Some(o), l: sw, arc, lines, arrows, m, segs, live: true, ..Default::default() }
}

/// Degrees to 0.1°, without a trailing .0.
pub fn format_deg(d: f64) -> String {
    num_str(round(d * 10.0) / 10.0)
}

/// A dimension's text.
pub fn dim_text(l: &DimLayout, fmt: &dyn Fn(f64) -> String) -> String {
    match l.kind.as_str() {
        "rad" => t("m3d.dim.radius", &[("len", &fmt(l.l))]),
        "dia" => t("m3d.dim.diameter", &[("len", &fmt(l.l))]),
        "ang" => t("m3d.dim.angle", &[("deg", &format_deg((l.l * 180.0) / std::f64::consts::PI))]),
        _ => fmt(l.l),
    }
}

/// The box of a dimension's text centred on screen point M: (x, y, w, h).
pub fn dim_text_box(m: &[f64], text: &str) -> (f64, f64, f64, f64) {
    let w = text.encode_utf16().count() as f64 * 7.0 + 12.0;
    let (x, y) = (m.first().copied().unwrap_or(f64::NAN), m.get(1).copied().unwrap_or(f64::NAN));
    (x - w / 2.0, y - 10.0, w, 20.0)
}

fn seg_dist2(x: f64, y: f64, a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let l2 = dx * dx + dy * dy;
    let tt = if crate::js::truthy(l2) { jmax(0.0, jmin(1.0, ((x - a[0]) * dx + (y - a[1]) * dy) / l2)) } else { 0.0 };
    hypot2(x - (a[0] + dx * tt), y - (a[1] + dy * tt))
}

/// The dimension under screen point x, y ({t: 'd', id} or None). Re-attaches dimension ends as the JS does.
pub fn pick_dim(model: &mut Model, cam: &Camera, x: f64, y: f64, fmt: &dyn Fn(f64) -> String, radius: f64) -> Option<Sel> {
    if model.dims.is_empty() {
        return None;
    }
    let vp = cam.view_proj();
    let mut best: Option<(f64, f64)> = None;
    for id in model.dims.key_vec() {
        let Some(mut dm) = model.dims.get(id).cloned() else { continue };
        let l = dim_layout(model, &mut dm, cam, &vp);
        model.dims.set(id, dm);
        let Some(m) = l.m.clone() else { continue };
        let (bx, by, bw, bh) = dim_text_box(&m, &dim_text(&l, fmt));
        let d = if x >= bx && x <= bx + bw && y >= by && y <= by + bh {
            0.0
        } else {
            // Math.min(Infinity, ...segs.map(segDist2))
            jmin(f64::INFINITY, min_of(l.segs.iter().map(|(a, b)| seg_dist2(x, y, a, b))))
        };
        if d <= radius && best.is_none_or(|b| d < b.0) {
            best = Some((d, id));
        }
    }
    best.map(|(_, id)| Sel::new("d", id))
}

/// A section plane as drawn: a rectangle on the plane around the model's extent there, and arrows toward the side
/// it cuts away.
#[derive(Clone, Debug)]
pub struct SectionLayout {
    pub corners: [P3; 4],
    pub arrows: [(P3, P3); 4],
}

pub fn section_layout(model: &Model, p: P3, n0: P3) -> SectionLayout {
    let n = norm(n0);
    let (u, v) = plane_axes_hint(n, Some(if n[2].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] }));
    let bb = model.bbox("root", &IDENT);
    let (mut u0, mut u1, mut v0, mut v1) = (-1.0, 1.0, -1.0, 1.0);
    if let Some(bb) = bb {
        u0 = f64::INFINITY;
        v0 = f64::INFINITY;
        u1 = f64::NEG_INFINITY;
        v1 = f64::NEG_INFINITY;
        for i in 0..8u8 {
            let c = [
                if i & 1 != 0 { bb.max[0] } else { bb.min[0] },
                if i & 2 != 0 { bb.max[1] } else { bb.min[1] },
                if i & 4 != 0 { bb.max[2] } else { bb.min[2] },
            ];
            let q = sub(c, p);
            let (a, b) = (dot(q, u), dot(q, v));
            u0 = jmin(u0, a);
            u1 = jmax(u1, a);
            v0 = jmin(v0, b);
            v1 = jmax(v1, b);
        }
        let pad = jmax(jmax(u1 - u0, v1 - v0), 0.5) * 0.08;
        u0 -= pad;
        u1 += pad;
        v0 -= pad;
        v1 += pad;
    }
    let at = |a: f64, b: f64| add(p, add(mul(u, a), mul(v, b)));
    let corners = [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)];
    let k = jmax(u1 - u0, v1 - v0) * 0.07;
    let arrows = corners.map(|c| (c, add(c, mul(n, k))));
    SectionLayout { corners, arrows }
}

/// The section plane under screen point x, y ({t: 's', id} or None): on its outline or its arrows.
pub fn pick_section(model: &Model, cam: &Camera, x: f64, y: f64, radius: f64) -> Option<Sel> {
    if model.sections.is_empty() || !model.section_view.planes {
        return None;
    }
    let vp = cam.view_proj();
    let mut best: Option<(f64, f64)> = None;
    for (id, s) in model.sections.iter() {
        let l = section_layout(model, s.p, s.n);
        let pp: Vec<Option<[f64; 3]>> = l.corners.iter().map(|c| cam.project_vp(*c, &vp)).collect();
        if pp.iter().any(Option::is_none) {
            continue;
        }
        let pq: Vec<[f64; 3]> = pp.into_iter().flatten().collect();
        let mut segs: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for i in 0..4 {
            if let (Some(a), Some(b)) = (pq.get(i), pq.get((i + 1) % 4)) {
                segs.push((*a, *b));
            }
        }
        for (a, b) in &l.arrows {
            if let (Some(aa), Some(bb)) = (cam.project_vp(*a, &vp), cam.project_vp(*b, &vp)) {
                segs.push((aa, bb));
            }
        }
        let d = min_of(segs.iter().map(|(a, b)| seg_dist2(x, y, a, b)));
        if d <= radius && best.is_none_or(|b| d < b.0) {
            best = Some((d, id));
        }
    }
    best.map(|(_, id)| Sel::new("s", id))
}

/// The point where the pick ray meets the line through `o` along `dir`: (p, s).
pub fn along_line(cam: &Camera, x: f64, y: f64, o: P3, dir: P3) -> Option<(P3, f64)> {
    let ray = cam.ray(x, y);
    line_line(ray.o, ray.d, o, dir).map(|l| (l.3, l.1))
}
