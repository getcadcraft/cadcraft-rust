//! res-foundation.js (0.5 part 4, RC.2), the half that follows the walls: a foundation (slab on grade with thickened
//! edges, stem walls and slab, crawlspace, basement) read back from its pset and made again from the outline round its
//! seed — footings sized from the walls' stories and type (labelled defaults, not engineering), stepped to the grade
//! where a terrain is loaded — and `syncFoundations`.

use cc_io_ifc::model::Id;

use crate::api::R;
use crate::draw::levels::read_levels;
use crate::draw::m::{self, V2, at, hypot, nx};
use crate::draw::res::{ResCtx, agg_rel, inset_poly, levels_or, num, parts_of, s_or};
use crate::draw::resterrain::{ground, ground_at, read_terrain};
use crate::draw::rooms::{Outline, label_point, loop_at, plan_loops};
use crate::draw::slabs::{K, kpts, shape_key};
use crate::draw::tr::t0;
use crate::draw::walls::{Seg, Wall, read_walls, wall_outlines};
use crate::draw::writer::{QRow, Row, W, round6, row};
use crate::wapi::{JV, P};

pub const FOUND_PSET: &str = "CadCraft_Foundation";
const IN: f64 = 25.4;
pub const FOUNDATION_KINDS: [&str; 4] = ["slab", "stemslab", "crawl", "basement"];
/// The stored size fields and their keys.
pub(crate) const FIELDS: [(&str, &str); 9] = [
    ("SlabThickness", "slabT"),
    ("EdgeWidth", "edgeW"),
    ("EdgeDepth", "edgeD"),
    ("StemThickness", "stemT"),
    ("FootingWidth", "footW"),
    ("FootingDepth", "footD"),
    ("Bottom", "bottom"),
    ("StemDrop", "stemDrop"),
    ("BasementHeight", "basementH"),
];
/// `foundationParams`' own order of the sizes.
pub const PKEYS: [&str; 9] = ["slabT", "edgeW", "edgeD", "stemT", "footW", "footD", "bottom", "stemDrop", "basementH"];

/// Every size a foundation needs (`foundationParams`).
#[derive(Clone, Debug)]
pub struct Params {
    pub kind: String,
    /// the sizes in PKEYS order
    pub v: [f64; 9],
    pub auto: [bool; 9],
    pub stem_top: f64,
}
impl Params {
    fn g(&self, k: &str) -> f64 {
        PKEYS.iter().position(|x| *x == k).and_then(|i| self.v.get(i)).copied().unwrap_or(0.0)
    }
    pub fn slab_t(&self) -> f64 {
        self.g("slabT")
    }
    pub fn edge_w(&self) -> f64 {
        self.g("edgeW")
    }
    pub fn edge_d(&self) -> f64 {
        self.g("edgeD")
    }
    pub fn stem_t(&self) -> f64 {
        self.g("stemT")
    }
    pub fn foot_w(&self) -> f64 {
        self.g("footW")
    }
    pub fn foot_d(&self) -> f64 {
        self.g("footD")
    }
    pub fn bottom(&self) -> f64 {
        self.g("bottom")
    }
    pub fn basement_h(&self) -> f64 {
        self.g("basementH")
    }
    /// JSON.stringify of the params object, as the shape key holds it.
    fn key(&self) -> K {
        let mut o: Vec<(&'static str, K)> = PKEYS.iter().zip(self.v.iter()).map(|(k, v)| (*k, K::N(*v))).collect();
        o.push(("kind", K::S(self.kind.clone())));
        o.push(("stemTop", K::N(self.stem_top)));
        o.push(("auto", K::O(PKEYS.iter().zip(self.auto.iter()).map(|(k, a)| (*k, K::B(*a))).collect())));
        K::O(o)
    }
}

/// `footingSize({ kind, stories, heavy })` → (width, depth)
pub fn footing_size(kind: &str, stories: f64, heavy: bool) -> (f64, f64) {
    let n = m::max(1.0, m::min(3.0, m::round(stories)));
    let t = if kind == "crawl" || kind == "basement" { [18.0, 22.0, 27.0] } else { [12.0, 15.0, 23.0] };
    let i = if n >= 3.0 {
        2
    } else if n >= 2.0 {
        1
    } else {
        0
    };
    let base = t.get(i).copied().unwrap_or(12.0) + if heavy { 4.0 } else { 0.0 };
    (base * IN, if base >= 24.0 { 10.0 } else { 8.0 } * IN)
}

/// `foundationParams(kind, stored, { stories, heavy })`; `stored` in PKEYS order (None = not stored).
pub fn foundation_params(kind: &str, stored: &[Option<f64>; 9], stories: f64, heavy: bool) -> Params {
    let (fw, fd) = footing_size(kind, stories, heavy);
    let deep = kind == "crawl" || kind == "basement";
    let d = [
        4.0 * IN,
        12.0 * IN,
        12.0 * IN,
        if heavy { 10.0 } else { 8.0 } * IN,
        fw,
        fd,
        if kind == "crawl" { 36.0 * IN } else { 24.0 * IN },
        if deep { 12.0 * IN } else { 0.0 },
        96.0 * IN,
    ];
    let mut v = d;
    let mut auto = [true; 9];
    for i in 0..9usize {
        let s = stored.get(i).copied().flatten();
        if let Some(x) = s
            && x.is_finite()
            && x > 0.0
            && let Some(slot) = v.get_mut(i)
        {
            *slot = x;
        }
        if let Some(a) = auto.get_mut(i) {
            *a = !s.is_some_and(|x| x > 0.0);
        }
    }
    let stem_top = -v.get(7).copied().unwrap_or(0.0);
    Params { kind: kind.to_string(), v, auto, stem_top }
}

/// A part of a foundation: role, outer / inner rings, z range (from the floor).
#[derive(Clone, Debug)]
pub struct Part {
    pub role: &'static str,
    pub outer: Vec<V2>,
    pub inner: Option<Vec<V2>>,
    pub z0: f64,
    pub z1: f64,
}

/// Every part a kind needs is there (`complete`).
pub fn complete(kind: &str, geo: &[Part]) -> bool {
    let need: &[&str] = match kind {
        "slab" => &["slab", "edge"],
        "stemslab" => &["stem", "footing", "slab"],
        "crawl" => &["stem", "footing"],
        "basement" => &["stem", "footing", "slab"],
        _ => &[],
    };
    need.iter().all(|r| geo.iter().any(|g| g.role == *r))
}

pub const STEP: f64 = 8.0 * IN;

/// `band(poly, o, u, s0, s1)`: the part of a polygon between two lines square to u.
fn band(poly: &[V2], o: V2, u: V2, s0: f64, s1: f64) -> Vec<V2> {
    let clip = |p: &[V2], sgn: f64, c: f64| -> Vec<V2> {
        let f = |q: V2| sgn * ((q[0] - o[0]) * u[0] + (q[1] - o[1]) * u[1] - c);
        let mut out = Vec::new();
        let n = p.len();
        for i in 0..n {
            let (a, b) = (at(p, i), at(p, nx(i, n)));
            let (fa, fb) = (f(a), f(b));
            if fa >= -1e-9 {
                out.push(a);
            }
            if (fa < -1e-9 && fb > 1e-9) || (fa > 1e-9 && fb < -1e-9) {
                let t = fa / (fa - fb);
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        out
    };
    clip(&clip(poly, 1.0, s0), -1.0, s1)
}

/// `steppedRings(outline, p, grade)`
fn stepped_rings(outline: &[V2], p: &Params, grade: &dyn Fn(f64, f64) -> Option<f64>) -> Vec<Part> {
    let mut parts = Vec::new();
    let n = outline.len();
    let so = outline;
    let si = inset_poly(outline, p.stem_t());
    let fo = inset_poly(outline, (p.stem_t() - p.foot_w()) / 2.0);
    let fi = inset_poly(outline, (p.stem_t() + p.foot_w()) / 2.0);
    for i in 0..n {
        let (a, b) = (at(outline, i), at(outline, nx(i, n)));
        let l = hypot(b[0] - a[0], b[1] - a[1]);
        if l < 1.0 {
            continue;
        }
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let quad = |o: &[V2], inn: &[V2]| vec![at(o, i), at(o, nx(i, n)), at(inn, nx(i, n)), at(inn, i)];
        let stem_q = quad(so, &si);
        let foot_q = quad(&fo, &fi);
        let at_s = |s: f64| -> f64 {
            let g = grade(a[0] + u[0] * s, a[1] + u[1] * s);
            ((g.unwrap_or(-200.0) - p.bottom()) / STEP).floor() * STEP
        };
        let mut ss = Vec::new();
        let mut s = 0.0;
        while s <= l && ss.len() < 100_000 {
            ss.push(s);
            s += 600.0;
        }
        if ss.last().is_some_and(|x| *x < l) {
            ss.push(l);
        }
        let mut r0 = 0.0;
        let mut z0 = m::min(at_s(0.0), -p.bottom());
        let mut runs: Vec<(f64, f64, f64)> = Vec::new();
        for k in 1..ss.len() {
            let (sk, sp) = (ss.get(k).copied().unwrap_or(0.0), ss.get(k.wrapping_sub(1)).copied().unwrap_or(0.0));
            let z = m::min(at_s(sk), -p.bottom());
            if z != z0 {
                let mid = (sp + sk) / 2.0;
                runs.push((r0, mid, z0));
                r0 = mid;
                z0 = z;
            }
        }
        runs.push((r0, l, z0));
        let one = runs.len() == 1;
        for &(s0, s1, zb) in &runs {
            let lo = if one { -1e9 } else { s0 };
            let hi = if one { 1e9 } else { s1 };
            let b0 = if s0 == 0.0 { -1e9 } else { lo };
            let b1 = if s1 == l { 1e9 } else { hi };
            let fq = band(&foot_q, a, u, b0, b1);
            let sq = band(&stem_q, a, u, b0, b1);
            if fq.len() >= 3 {
                parts.push(Part { role: "footing", outer: fq, inner: None, z0: zb, z1: zb + p.foot_d() });
            }
            if sq.len() >= 3 && p.stem_top > zb + p.foot_d() {
                parts.push(Part { role: "stem", outer: sq, inner: None, z0: zb + p.foot_d(), z1: p.stem_top });
            }
        }
    }
    parts
}

/// `foundationGeometry(outline, p, { grade })` → the parts, heights from the floor.
pub fn foundation_geometry(outline: &[V2], p: &Params, grade: Option<&dyn Fn(f64, f64) -> Option<f64>>) -> Vec<Part> {
    let mut parts = Vec::new();
    if let Some(g) = grade
        && (p.kind == "stemslab" || p.kind == "crawl")
    {
        parts.extend(stepped_rings(outline, p, g));
        if p.kind == "stemslab" {
            let poly = inset_poly(outline, p.stem_t());
            if m::area(&poly) > 1e4 {
                parts.push(Part { role: "slab", outer: poly, inner: None, z0: -p.slab_t(), z1: 0.0 });
            }
        }
        return parts;
    }
    let ring = |parts: &mut Vec<Part>, role: &'static str, d_out: f64, d_in: f64, z0: f64, z1: f64| {
        let outer = inset_poly(outline, d_out);
        let inner = inset_poly(outline, d_in);
        if m::area(&inner) > 1e4 && z1 > z0 {
            parts.push(Part { role, outer, inner: Some(inner), z0, z1 });
        }
    };
    let slab = |parts: &mut Vec<Part>, d: f64, z0: f64, z1: f64| {
        let poly = inset_poly(outline, d);
        if m::area(&poly) > 1e4 {
            parts.push(Part { role: "slab", outer: poly, inner: None, z0, z1 });
        }
    };
    let foot_under = |parts: &mut Vec<Part>, top: f64| {
        ring(parts, "footing", (p.stem_t() - p.foot_w()) / 2.0, (p.stem_t() + p.foot_w()) / 2.0, top - p.foot_d(), top);
    };
    match p.kind.as_str() {
        "slab" => {
            slab(&mut parts, 0.0, -p.slab_t(), 0.0);
            ring(&mut parts, "edge", 0.0, p.edge_w(), -p.edge_d(), -p.slab_t());
        }
        "stemslab" | "crawl" => {
            let foot_top = -p.bottom() + p.foot_d();
            ring(&mut parts, "stem", 0.0, p.stem_t(), foot_top, p.stem_top);
            foot_under(&mut parts, foot_top);
            if p.kind == "stemslab" {
                slab(&mut parts, p.stem_t(), -p.slab_t(), 0.0);
            }
        }
        "basement" => {
            let slab_top = p.stem_top - p.basement_h();
            let foot_top = slab_top - p.slab_t();
            ring(&mut parts, "stem", 0.0, p.stem_t(), foot_top, p.stem_top);
            foot_under(&mut parts, foot_top);
            slab(&mut parts, p.stem_t(), slab_top - p.slab_t(), slab_top);
        }
        _ => {}
    }
    parts
}

/// A foundation read back (`readFoundations`).
#[derive(Clone, Debug)]
pub struct Foundation {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub seed: V2,
    pub kind: String,
    pub stored: [Option<f64>; 9],
    pub key: String,
    pub parts: Vec<Id>,
}

pub fn read_foundations(w: &mut W<'_>) -> R<Vec<Foundation>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCELEMENTASSEMBLY") {
        let Some(ps) = w.read_pset(id, FOUND_PSET)? else { continue };
        let l = w.must(id)?;
        let mut stored = [None; 9];
        for (i, (f, _)) in FIELDS.iter().enumerate() {
            if let Some(v) = ps.get(*f)
                && !matches!(v, P::Undef | P::Null)
                && !v.as_str().is_some_and(|s| s.is_empty())
                && let Some(slot) = stored.get_mut(i)
            {
                *slot = Some(v.number() * um);
            }
        }
        let k = s_or(&ps, "Kind");
        let kind = match ps.get("Kind") {
            Some(P::Str(s)) if FOUNDATION_KINDS.contains(&&*s.lossy()) => k,
            _ => "slab".into(),
        };
        out.push(Foundation {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&cc_io_ifc::model::id_key(id)).copied(),
            seed: [num(&ps, "SeedX") * um, num(&ps, "SeedY") * um],
            kind,
            stored,
            key: s_or(&ps, "ShapeKey"),
            parts: parts_of(w, id)?,
        });
    }
    Ok(out)
}

/// `loadsOn(w, storey, L)` → (stories, heavy)
pub fn loads_on(w: &mut W<'_>, storey: Id, walls: Option<&[Wall]>) -> R<(f64, bool)> {
    let levels = read_levels(w)?;
    let i = levels.iter().position(|l| l.id == storey).unwrap_or(0);
    let mut stories = 0.0;
    for l in levels.iter().skip(i) {
        if !read_walls(w, Some(l.id))?.is_empty() {
            stories += 1.0;
        }
    }
    let heavy_of = |ws: &[Wall]| ws.iter().any(|x| ["brick", "cmu", "concrete", "ext-300"].iter().any(|p| x.type_key.starts_with(p)));
    let heavy = match walls {
        Some(ws) => heavy_of(ws),
        None => heavy_of(&read_walls(w, Some(storey))?),
    };
    Ok((m::max(1.0, stories), heavy))
}

/// The walls' outlines of a level (`storeyOutlines`).
pub struct Outlines {
    pub walls: Vec<Wall>,
    pub outlines: Vec<Outline>,
}
pub fn storey_outlines(w: &mut W<'_>, storey: Option<Id>) -> R<Outlines> {
    let walls = read_walls(w, storey)?;
    let segs: Vec<Seg> = walls.iter().map(|x| x.seg()).collect();
    let polys: Vec<Vec<V2>> = wall_outlines(&segs, 1.0).into_iter().map(|(_, p)| p).collect();
    Ok(Outlines { outlines: plan_loops(&polys).outlines, walls })
}

fn role_of(role: &str) -> (&'static str, &'static str, &'static str) {
    match role {
        "slab" => ("IFCSLAB", "BASESLAB", "res.fd.part.slab"),
        "edge" => ("IFCFOOTING", "STRIP_FOOTING", "res.fd.part.edge"),
        "footing" => ("IFCFOOTING", "STRIP_FOOTING", "res.fd.part.footing"),
        _ => ("IFCWALL", "STANDARD", "res.fd.part.stem"),
    }
}

/// `writeFoundation(w, f, { L })` → true when it was made again.
pub(crate) fn write_foundation(w: &mut W<'_>, f: &Foundation, given: Option<&Outlines>) -> R<bool> {
    let Some(storey) = f.storey else { return Ok(false) };
    let own;
    let l = match given {
        Some(l) => l,
        None => {
            own = storey_outlines(w, f.storey)?;
            &own
        }
    };
    let Some(outline) = loop_at(&l.outlines, f.seed) else { return Ok(false) };
    let poly = outline.poly.clone();
    let (stories, heavy) = loads_on(w, storey, Some(&l.walls))?;
    let p = foundation_params(&f.kind, &f.stored, stories, heavy);
    // RC.10: with a terrain, stem walls and crawlspace footings follow the grade
    let mut grade_on = false;
    let mut gkey: Option<Vec<Option<f64>>> = None;
    let mut gsurf = None;
    let mut mw = [0.0; 16];
    if (f.kind == "stemslab" || f.kind == "crawl") && read_terrain(w)?.is_some() {
        mw = w.storey_world(storey)?;
        gsurf = ground(w)?;
        grade_on = true;
    }
    let grade = |x: f64, y: f64| -> Option<f64> {
        let qx = mw[0] * x + mw[4] * y + mw[12];
        let qy = mw[1] * x + mw[5] * y + mw[13];
        ground_at(gsurf.as_ref(), qx, qy).map(|g| g - mw[14])
    };
    if grade_on {
        gkey = Some(poly.iter().map(|q| grade(q[0], q[1])).collect());
    }
    let key = shape_key(vec![
        kpts(&poly),
        p.key(),
        match &gkey {
            Some(g) => K::A(g.iter().map(|v| v.map_or(K::Null, K::N)).collect()),
            None => K::Null,
        },
    ]);
    if f.key == key && !f.parts.is_empty() {
        return Ok(false);
    }
    if !f.parts.is_empty() {
        w.drop_product(&f.parts)?;
    }
    let st = w.must(storey)?;
    let st_lp = st.a("ObjectPlacement").href();
    let conc = w.material(&t0("model.mat.concrete"))?;
    let style = w.surface_style(&t0("res.style.concrete"), [0.72, 0.72, 0.7], 0.0)?;
    let mut parts: Vec<Id> = Vec::new();
    // one element per role: a stepped footing's pieces are items of the same IfcFooting
    let geo = foundation_geometry(&poly, &p, if grade_on { Some(&grade) } else { None });
    let mut by_role: Vec<(&'static str, Vec<Part>)> = Vec::new();
    for g in geo {
        match by_role.iter_mut().find(|(r, _)| *r == g.role) {
            Some((_, v)) => v.push(g),
            None => by_role.push((g.role, vec![g])),
        }
    }
    for (role, list) in &by_role {
        let many = list.len() > 1;
        let (cls, ptype, name) = role_of(role);
        let nm_key = if *role == "slab" && f.kind == "basement" { "res.fd.part.bslab" } else { name };
        let nm = w.l(&t0(nm_key));
        let lp = w.place_at(st_lp, 0.0, 0.0, 0.0, 0.0)?;
        let g = w.guid();
        let oh = w.owner_history();
        let mut args = vec![g, oh, nm.clone(), JV::Null, nm, JV::H(lp), JV::Null, JV::Null];
        if cls == "IFCWALL" {
            if w.v4 {
                args.push(JV::s("STANDARD"));
            }
        } else {
            args.push(JV::s(ptype));
        }
        let id = w.make(cls, args)?;
        let mut items = Vec::new();
        for q in list {
            let holes: Vec<Vec<V2>> = q.inner.iter().cloned().collect();
            let pr = w.profile_voids(&q.outer, &holes, None)?;
            let it = w.extrude(pr, q.z0, q.z1 - q.z0)?;
            w.style_item(it, style)?;
            items.push(it);
        }
        let rep = w.rep("body", "Body", "SweptSolid", &items)?;
        w.set_shape(id, &[rep])?;
        w.associate_material(id, conc)?;
        if many {
            // quantities of a stepped ring: summed over its pieces
            let vol = list.iter().fold(0.0, |s, q| s + m::area(&q.outer).abs() * (q.z1 - q.z0));
            let len = list.iter().fold(0.0, |s, q| s + m::perim(&q.outer) / 2.0);
            let qn = if *role == "stem" { "Qto_WallBaseQuantities" } else { "Qto_FootingBaseQuantities" };
            let rows: Vec<QRow> =
                vec![("Length".into(), "length", len), ("GrossVolume".into(), "volume", vol), ("NetVolume".into(), "volume", vol)];
            w.upsert_qto(id, qn, &rows)?;
            if *role == "stem" {
                w.upsert_pset(
                    id,
                    "Pset_WallCommon",
                    &[row("IsExternal", "IFCBOOLEAN", P::Bool(true)), row("LoadBearing", "IFCBOOLEAN", P::Bool(true))],
                )?;
            }
            let mut zs: Vec<f64> = Vec::new();
            for q in list {
                let r = m::round(q.z0);
                let r = if r == 0.0 { 0.0 } else { r };
                if !zs.contains(&r) {
                    zs.push(r);
                }
            }
            if zs.len() > 1 {
                let gz0 = m::min_of(list.iter().map(|q| q.z0));
                w.upsert_pset(
                    id,
                    "CadCraft_Stepped",
                    &[
                        row("Steps", "IFCINTEGER", P::Num((zs.len() as f64) - 1.0)),
                        row("Lowest", "IFCLENGTHMEASURE", P::Num(round6(w.u(gz0)))),
                    ],
                )?;
            }
            parts.push(id);
            continue;
        }
        let Some(g) = list.first() else { continue };
        let a = m::area(&g.outer).abs() - g.inner.as_ref().map_or(0.0, |i| m::area(i).abs());
        let h = g.z1 - g.z0;
        if *role == "slab" {
            let rows: Vec<QRow> = vec![
                ("Width".into(), "length", h),
                ("Perimeter".into(), "length", m::perim(&g.outer)),
                ("GrossArea".into(), "area", a),
                ("NetArea".into(), "area", a),
                ("GrossVolume".into(), "volume", a * h),
                ("NetVolume".into(), "volume", a * h),
            ];
            w.upsert_qto(id, "Qto_SlabBaseQuantities", &rows)?;
        } else {
            let len = (m::perim(&g.outer) + g.inner.as_ref().map_or(0.0, |i| m::perim(i))) / 2.0;
            let width = a / len;
            let qn = if *role == "stem" { "Qto_WallBaseQuantities" } else { "Qto_FootingBaseQuantities" };
            let rows: Vec<QRow> = vec![
                ("Length".into(), "length", len),
                ("Width".into(), "length", width),
                ("Height".into(), "length", h),
                ("GrossVolume".into(), "volume", a * h),
                ("NetVolume".into(), "volume", a * h),
            ];
            w.upsert_qto(id, qn, &rows)?;
        }
        if *role == "stem" {
            w.upsert_pset(
                id,
                "Pset_WallCommon",
                &[row("IsExternal", "IFCBOOLEAN", P::Bool(true)), row("LoadBearing", "IFCBOOLEAN", P::Bool(true))],
            )?;
        }
        parts.push(id);
    }
    match agg_rel(w, f.id)? {
        Some(rel) => {
            let mut x = w.touch(rel)?;
            x.set("RelatedObjects", JV::L(parts.iter().map(|&q| JV::H(q)).collect()));
            w.write(&mut x)?;
        }
        None if !parts.is_empty() => {
            let g = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(f.id), JV::L(parts.iter().map(|&q| JV::H(q)).collect())])?;
        }
        None => {}
    }
    let sd = label_point(&poly);
    let rows: Vec<Row> = vec![
        row("SeedX", "IFCLENGTHMEASURE", P::Num(round6(w.u(sd[0])))),
        row("SeedY", "IFCLENGTHMEASURE", P::Num(round6(w.u(sd[1])))),
        row("ShapeKey", "IFCIDENTIFIER", P::s(&key)),
        row("Stories", "IFCINTEGER", P::Num(stories)),
        row("FootingWidthUsed", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(p.foot_w())))),
        row("FootingDepthUsed", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(p.foot_d())))),
    ];
    w.upsert_pset(f.id, FOUND_PSET, &rows)?;
    Ok(true)
}

/// `syncFoundations(w, storey, ctx)`: foundations made again where their outline moved — on that level and on the
/// levels below it (the stories above size their footings). Gone outlines: left alone.
pub fn sync_foundations(w: &mut W<'_>, storey: Id, ctx: &ResCtx<'_>) -> R<usize> {
    let mut n = 0usize;
    let levels = levels_or(w, ctx.levels)?;
    let here = levels.iter().find(|l| l.id == storey).cloned();
    for f in read_foundations(w)? {
        let lv = levels.iter().find(|l| Some(l.id) == f.storey);
        if let (Some(h), Some(lv)) = (&here, lv)
            && lv.elevation > h.elevation + 1.0
        {
            continue;
        }
        let given = match (f.storey == Some(storey), ctx.l) {
            (true, Some(l)) => Some(Outlines { walls: l.walls.clone(), outlines: l.loops.outlines.clone() }),
            _ => None,
        };
        if write_foundation(w, &f, given.as_ref())? {
            n = n.saturating_add(1);
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footing_sizes_follow_the_table() {
        assert_eq!(footing_size("stemslab", 1.0, false), (12.0 * IN, 8.0 * IN));
        assert_eq!(footing_size("crawl", 3.0, true), (31.0 * IN, 10.0 * IN));
        assert_eq!(footing_size("basement", 9.0, false), (27.0 * IN, 10.0 * IN));
    }

    #[test]
    fn every_kind_makes_its_parts() {
        let o = vec![[0.0, 0.0], [8000.0, 0.0], [8000.0, 6000.0], [0.0, 6000.0]];
        for k in FOUNDATION_KINDS {
            let p = foundation_params(k, &[None; 9], 1.0, false);
            assert!(complete(k, &foundation_geometry(&o, &p, None)), "{k}");
        }
        // stepped to a sloping grade: several footing pieces, at whole 8" steps
        let p = foundation_params("crawl", &[None; 9], 1.0, false);
        let g = |x: f64, _y: f64| Some(-x / 4.0);
        let parts = foundation_geometry(&o, &p, Some(&g));
        let feet: Vec<&Part> = parts.iter().filter(|q| q.role == "footing").collect();
        assert!(feet.len() > 4);
        assert!(feet.iter().all(|q| ((q.z0 / STEP).round() * STEP - q.z0).abs() < 1e-6));
    }

    #[test]
    fn a_degenerate_outline_makes_nothing() {
        let p = foundation_params("slab", &[None; 9], 1.0, false);
        // a tiny outline: the slab is too small to make (its mitred edge ring is whatever the JS makes too)
        assert!(!foundation_geometry(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], &p, None).iter().any(|q| q.role == "slab"));
        assert!(foundation_geometry(&[], &p, None).is_empty());
    }
}
