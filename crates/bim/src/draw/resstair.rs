//! res-stair.js (0.5 part 4, RC.3), the half that follows the model: a stair's layout (straight, L, U, winders; risers
//! and treads from the floor-to-floor height), its flights, landings, handrails (res-railing.js `railingParts` /
//! `railingItems`) and the stairwell cut in the floor above, made again when the levels or the floor above change
//! (`syncStairs`).

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::levels::Level;
use crate::draw::m::{self, V2, hypot, hypot3};
use crate::draw::res::{agg_rel, is_true, levels_or, num, num0, parts_of, s_or};
use crate::draw::rooms::plan_loops_snap;
use crate::draw::slabs::{K, shape_key};
use crate::draw::tr::{t, t0};
use crate::draw::writer::{W, inv4j, round6, row};
use crate::util::{apply4, mul4};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;
pub const STAIR_PSET: &str = "CadCraft_Stair";
pub const WELL_PSET: &str = "CadCraft_StairWell";
pub const STAIR_KINDS: [&str; 4] = ["straight", "l", "u", "winder"];
const RISER_MAX: f64 = 7.75 * IN;
const TREAD_MIN: f64 = 10.0 * IN;
const WIDTH_MIN: f64 = 36.0 * IN;
const HEADROOM: f64 = 80.0 * IN;
const RAIL_MIN: f64 = 34.0 * IN;
const RAIL_MAX: f64 = 38.0 * IN;

pub(crate) fn ifc_type(kind: &str) -> &'static str {
    match kind {
        "l" => "QUARTER_TURN_STAIR",
        "u" => "HALF_TURN_STAIR",
        "winder" => "QUARTER_WINDING_STAIR",
        _ => "STRAIGHT_RUN_STAIR",
    }
}

/// A stair as read (`readStairs`), which is also the layout's spec.
#[derive(Clone, Debug)]
pub struct Stair {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub top_level: String,
    pub kind: String,
    pub turn: String,
    pub start: V2,
    pub angle: f64,
    pub width: f64,
    pub fixed_height: f64,
    pub riser_max: f64,
    pub tread: f64,
    pub split: f64,
    pub gap: f64,
    pub railings: String,
    pub rail_height: f64,
    pub well: bool,
    pub key: String,
    pub parts: Vec<Id>,
}

/// A tread or landing: its outline, its top.
#[derive(Clone, Debug)]
pub struct Tread {
    pub poly: Vec<V2>,
    pub z: f64,
}
#[derive(Clone, Debug)]
pub enum Piece {
    Flight(Vec<Tread>),
    Winders(Vec<Tread>),
    Landing(Tread),
}

/// `stairLayout(spec)` (Err = its error key).
#[derive(Clone, Debug)]
pub struct Layout {
    pub n: f64,
    pub riser: f64,
    pub tread: f64,
    pub treads: f64,
    pub pieces: Vec<Piece>,
    pub sides: Vec<Vec<[f64; 3]>>,
    pub checks: [(&'static str, bool); 4],
    /// the walk line (plan), bottom to top
    pub walk: Vec<V2>,
}

impl Layout {
    /// `outline`: every tread and landing joined (`planLoops(polys, { snap: 0.5 }).outlines`).
    pub fn outline(&self) -> Vec<Vec<V2>> {
        let mut polys = Vec::new();
        for pc in &self.pieces {
            match pc {
                Piece::Landing(t) => polys.push(t.poly.clone()),
                Piece::Flight(ts) | Piece::Winders(ts) => polys.extend(ts.iter().map(|t| t.poly.clone())),
            }
        }
        crate::draw::rooms::plan_loops_snap(&polys, 0.5).outlines.into_iter().map(|o| o.poly).collect()
    }
}

fn ccw(poly: Vec<V2>) -> Vec<V2> {
    if m::area(&poly) < 0.0 {
        let mut p = poly;
        p.reverse();
        p
    } else {
        poly
    }
}

pub fn stair_layout(s: &Stair, height: f64) -> Result<Layout, &'static str> {
    let h = height;
    let w = s.width;
    let d = s.tread;
    let n = m::max(2.0, (h / s.riser_max - 1e-9).ceil());
    // a hostile file (no riser height, a height of kilometres): refused rather than counted out step by step
    if !(n <= 1000.0) || !s.width.is_finite() || !s.tread.is_finite() {
        return Err("res.err.stairsize");
    }
    let r = h / n;
    let tt = n - 1.0;
    let ang = if s.angle != 0.0 && !s.angle.is_nan() { s.angle } else { 0.0 };
    let a0 = ang * std::f64::consts::PI / 180.0;
    let u = [m::cos(a0), m::sin(a0)];
    let nl = [-u[1], u[0]];
    let t_side = if s.turn == "right" { -1.0 } else { 1.0 };
    let p = |si: f64, ti: f64| -> V2 { [s.start[0] + u[0] * si + nl[0] * ti, s.start[1] + u[1] * si + nl[1] * ti] };
    let rect = |s0: f64, s1: f64, t0: f64, t1: f64| vec![p(s0, t0), p(s1, t0), p(s1, t1), p(s0, t1)];
    let p3 = |si: f64, ti: f64, z: f64| -> [f64; 3] {
        let q = p(si, ti);
        [q[0], q[1], z]
    };
    let mut pieces: Vec<Piece> = Vec::new();
    let mut sides: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut walk: Vec<V2> = Vec::new();
    let checks = [
        ("riser", r <= s.riser_max + 1e-6 && r <= RISER_MAX + 1e-6),
        ("tread", d >= TREAD_MIN - 1e-6),
        ("width", w >= WIDTH_MIN - 1e-6),
        ("rail", s.railings == "none" || (s.rail_height >= RAIL_MIN - 1e-6 && s.rail_height <= RAIL_MAX + 1e-6)),
    ];
    let ins = 2.0 * IN;
    let rail_l = s.railings == "both" || s.railings == "left";
    let rail_r = s.railings == "both" || s.railings == "right";
    let nosing = |sa: f64, sb: f64, t: f64, za: f64, zb: f64| vec![p3(sa, t, za), p3(sb, t, zb)];
    if s.kind == "straight" {
        let mut treads = Vec::new();
        let mut i = 0.0;
        while i < tt {
            treads.push(Tread { poly: ccw(rect(i * d, (i + 1.0) * d, -w / 2.0, w / 2.0)), z: (i + 1.0) * r });
            i += 1.0;
        }
        pieces.push(Piece::Flight(treads));
        if rail_l {
            sides.push(nosing(0.0, (tt - 1.0) * d + d, w / 2.0 - ins, r, tt * r + r));
        }
        if rail_r {
            sides.push(nosing(0.0, (tt - 1.0) * d + d, -w / 2.0 + ins, r, tt * r + r));
        }
        walk.push(p(-0.0, 0.0));
        walk.push(p(tt * d, 0.0));
    } else {
        let turn_treads = if s.kind == "winder" { 3.0 } else { 1.0 };
        let before = m::max(1.0, m::min(tt - turn_treads - 1.0, m::round((tt - turn_treads) * s.split)));
        let after = tt - before - turn_treads;
        if after < 1.0 {
            return Err("res.err.stairshort");
        }
        let s0 = before * d;
        let mut f1 = Vec::new();
        let mut i = 0.0;
        while i < before {
            f1.push(Tread { poly: ccw(rect(i * d, (i + 1.0) * d, -w / 2.0, w / 2.0)), z: (i + 1.0) * r });
            i += 1.0;
        }
        pieces.push(Piece::Flight(f1));
        let t_in = t_side * w / 2.0;
        let t_out = -t_side * w / 2.0;
        if s.kind == "u" {
            let gap = s.gap;
            let wide = 2.0 * w + gap;
            let t0 = m::min(t_out, t_out + t_side * wide);
            let t1 = m::max(t_out, t_out + t_side * wide);
            let z_l = (before + 1.0) * r;
            pieces.push(Piece::Landing(Tread { poly: ccw(rect(s0, s0 + w, t0, t1)), z: z_l }));
            let c2 = t_side * (w + gap);
            let mut f2 = Vec::new();
            let mut j = 0.0;
            while j < after {
                f2.push(Tread { poly: ccw(rect(s0 - (j + 1.0) * d, s0 - j * d, c2 - w / 2.0, c2 + w / 2.0)), z: z_l + (j + 1.0) * r });
                j += 1.0;
            }
            pieces.push(Piece::Flight(f2));
            let out1 = t_out - m::sign(t_out) * ins;
            let out2 = c2 + t_side * (w / 2.0 - ins);
            let in1 = t_in - m::sign(t_in) * ins;
            let in2 = c2 - t_side * (w / 2.0 - ins);
            let z1 = before * r + r;
            let (outer, inner) = if s.turn == "right" { (rail_l, rail_r) } else { (rail_r, rail_l) };
            if outer {
                sides.push(vec![
                    p3(0.0, out1, r),
                    p3(s0, out1, z1),
                    p3(s0 + w - ins, out1, z_l),
                    p3(s0 + w - ins, out2, z_l),
                    p3(s0, out2, z_l + r),
                    p3(s0 - after * d, out2, z_l + (after + 1.0) * r),
                ]);
            }
            if inner {
                sides.push(nosing(0.0, s0, in1, r, z1));
                sides.push(vec![p3(s0, in2, z_l + r), p3(s0 - after * d, in2, z_l + (after + 1.0) * r)]);
            }
            walk.extend([p(0.0, 0.0), p(s0 + w / 2.0, 0.0), p(s0 + w / 2.0, c2), p(s0 - after * d, c2)]);
        } else {
            let z_t = |k: f64| (before + 1.0 + k) * r;
            if s.kind == "l" {
                pieces.push(Piece::Landing(Tread { poly: ccw(rect(s0, s0 + w, -w / 2.0, w / 2.0)), z: z_t(0.0) }));
            } else {
                let c = p(s0, t_in);
                let path = [p(s0, t_out), p(s0 + w, t_out), p(s0 + w, t_in)];
                let at_len = |len: f64| -> V2 {
                    if len <= w {
                        [path[0][0] + (path[1][0] - path[0][0]) * len / w, path[0][1] + (path[1][1] - path[0][1]) * len / w]
                    } else {
                        [path[1][0] + (path[2][0] - path[1][0]) * (len - w) / w, path[1][1] + (path[2][1] - path[1][1]) * (len - w) / w]
                    }
                };
                let mut wt = Vec::new();
                for k in 0..3u8 {
                    let kf = f64::from(k);
                    let l0 = 2.0 * w * kf / 3.0;
                    let l1 = 2.0 * w * (kf + 1.0) / 3.0;
                    let mut poly = vec![c, at_len(l0)];
                    if l0 < w && l1 > w {
                        poly.push(path[1]);
                    }
                    poly.push(at_len(l1));
                    wt.push(Tread { poly: ccw(poly), z: z_t(kf) });
                }
                pieces.push(Piece::Winders(wt));
            }
            let mut f2 = Vec::new();
            let base = before + turn_treads;
            let mut j = 0.0;
            while j < after {
                let t0 = t_in + t_side * j * d;
                let t1 = t_in + t_side * (j + 1.0) * d;
                f2.push(Tread { poly: ccw(rect(s0, s0 + w, m::min(t0, t1), m::max(t0, t1))), z: (base + 1.0 + j) * r });
                j += 1.0;
            }
            pieces.push(Piece::Flight(f2));
            let z_end = (base + after) * r + r;
            let z_land = z_t(0.0);
            let (outer, inner) = if s.turn == "right" { (rail_l, rail_r) } else { (rail_r, rail_l) };
            let s_out = s0 + w - ins;
            let t_o = t_out - m::sign(t_out) * ins;
            if outer {
                let z_mid = if s.kind == "l" { z_land } else { z_t(1.0) };
                sides.push(vec![
                    p3(0.0, t_o, r),
                    p3(s0, t_o, before * r + r),
                    p3(s_out, t_o, z_mid),
                    p3(s_out, t_in, if s.kind == "l" { z_land } else { z_t(2.0) }),
                    p3(s_out, t_in + t_side * after * d, z_end),
                ]);
            }
            if inner {
                let t_i = t_in - m::sign(t_in) * ins;
                let s_i = s0 + ins;
                sides.push(vec![
                    p3(0.0, t_i, r),
                    p3(s0, t_i, before * r + r),
                    p3(s_i, t_in, (base + 1.0) * r),
                    p3(s_i, t_in + t_side * after * d, z_end),
                ]);
            }
            walk.extend([p(0.0, 0.0), p(s0 + w / 2.0, 0.0), p(s0 + w / 2.0, t_in + t_side * after * d)]);
        }
    }
    Ok(Layout { n, riser: r, tread: d, treads: tt, pieces, sides, checks, walk })
}

// ---------------------------------------------------------------------------------------------------------- railings
const RAIL_BALUSTER: f64 = 1.5 * IN;
const RAIL_GAP: f64 = 4.0 * IN - 0.5;
const RAIL_POST: f64 = 3.5 * IN;
const RAIL_BAR: f64 = 2.0 * IN;

/// `railingParts(path3, { height })`
pub struct RailParts {
    pub rails: Vec<([f64; 3], [f64; 3])>,
    pub posts: Vec<[f64; 3]>,
    pub balusters: Vec<[f64; 3]>,
    pub length: f64,
    pub height: f64,
}

pub fn railing_parts(path3: &[[f64; 3]], height: f64) -> RailParts {
    let mut out = RailParts { rails: Vec::new(), posts: Vec::new(), balusters: Vec::new(), length: 0.0, height };
    let mut pts: Vec<[f64; 3]> = Vec::new();
    for (i, p) in path3.iter().enumerate() {
        let keep = match i.checked_sub(1).and_then(|k| path3.get(k)) {
            None => true,
            Some(q) => hypot(p[0] - q[0], p[1] - q[1]) > 1.0,
        };
        if keep {
            pts.push(*p);
        }
    }
    if pts.len() < 2 {
        return out;
    }
    out.posts.extend(pts.iter().copied());
    for pair in pts.windows(2) {
        let [a, b] = *pair else { continue };
        let l = hypot(b[0] - a[0], b[1] - a[1]);
        out.length += hypot(l, b[2] - a[2]);
        out.rails.push(([a[0], a[1], a[2] + height], [b[0], b[1], b[2] + height]));
        let clear = l - RAIL_POST;
        let n = m::max(0.0, ((clear - RAIL_GAP) / (RAIL_GAP + RAIL_BALUSTER)).ceil());
        let mut k = 1.0;
        while k <= n && k <= 100_000.0 {
            let s = RAIL_POST / 2.0 + (clear - n * RAIL_BALUSTER) / (n + 1.0) * k + RAIL_BALUSTER * (k - 0.5);
            let t = s / l;
            out.balusters.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
            k += 1.0;
        }
    }
    out
}

/// `railingItems(w, parts)` (plan mm = the storey's axes)
pub fn railing_items(w: &mut W<'_>, parts: &RailParts) -> R<Vec<Id>> {
    let mut items = Vec::new();
    let sq = |s: f64| [[-s / 2.0, -s / 2.0], [s / 2.0, -s / 2.0], [s / 2.0, s / 2.0], [-s / 2.0, s / 2.0]];
    let rail_top = parts.height;
    let up = |w: &mut W<'_>, p: [f64; 3], s: f64, h: f64| -> R<Id> {
        let pr = w.profile(&sq(s), None)?;
        let fr = w.axis_frame(p, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])?;
        w.extrude_at(pr, fr, h, [0.0, 0.0, 1.0])
    };
    for p in &parts.posts {
        items.push(up(w, *p, RAIL_POST, rail_top + RAIL_BAR / 2.0)?);
    }
    for p in &parts.balusters {
        items.push(up(w, *p, RAIL_BALUSTER, rail_top - RAIL_BAR / 2.0)?);
    }
    for (a, b) in &parts.rails {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = hypot3(d[0], d[1], d[2]);
        if l < 1.0 {
            continue;
        }
        let hz = hypot(d[0], d[1]);
        let hz = if hz != 0.0 && !hz.is_nan() { hz } else { 1.0 };
        let x = [d[0] / hz, d[1] / hz, 0.0];
        let z = [-x[1], x[0], 0.0];
        let fr = w.axis_frame([a[0] - z[0] * RAIL_BAR / 2.0, a[1] - z[1] * RAIL_BAR / 2.0, a[2] - RAIL_BAR / 2.0], x, z)?;
        let dl = [d[0] / l, d[1] / l, d[2] / l];
        let ex = [dl[0] * z[0] + dl[1] * z[1], dl[2], dl[0] * x[0] + dl[1] * x[1]];
        let pr = w.profile(&[[0.0, 0.0], [RAIL_BAR, 0.0], [RAIL_BAR, RAIL_BAR], [0.0, RAIL_BAR]], None)?;
        items.push(w.extrude_at(pr, fr, l, ex)?);
    }
    Ok(items)
}

// ----------------------------------------------------------------------------------------------------------- reading
pub fn read_stairs(w: &mut W<'_>) -> R<Vec<Stair>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCSTAIR") {
        let Some(ps) = w.read_pset(id, STAIR_PSET)? else { continue };
        let l = w.must(id)?;
        let nm = |k: &str| num0(&ps, k) * um;
        let kind = s_or(&ps, "Kind");
        let rl = s_or(&ps, "Railings");
        let is_str = |k: &str, v: &str| matches!(ps.get(k), Some(P::Str(s)) if s.lossy() == v);
        out.push(Stair {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&id_key(id)).copied(),
            top_level: s_or(&ps, "TopLevel"),
            kind: if STAIR_KINDS.iter().any(|k| is_str("Kind", k)) { kind } else { "straight".into() },
            turn: if is_str("Turn", "right") { "right".into() } else { "left".into() },
            start: [nm("StartX"), nm("StartY")],
            angle: num0(&ps, "Angle"),
            width: nm("Width"),
            fixed_height: nm("Height"),
            riser_max: nm("RiserMax"),
            tread: nm("Tread"),
            split: match ps.get("Split") {
                None | Some(P::Undef | P::Null) => 0.5,
                Some(v) => v.number(),
            },
            gap: nm("Gap"),
            railings: if ["both", "left", "right", "none"].iter().any(|k| is_str("Railings", k)) { rl } else { "both".into() },
            rail_height: nm("RailHeight"),
            well: is_true(ps.get("Well")),
            key: s_or(&ps, "ShapeKey"),
            parts: parts_of(w, id)?,
        });
    }
    Ok(out)
}

/// `stairHeight(w, st, levels)` → (floor to floor, the level it arrives at)
pub fn stair_height(st: &Stair, levels: &[Level]) -> (f64, Option<Level>) {
    let i = levels.iter().position(|l| Some(l.id) == st.storey);
    let top = if !st.top_level.is_empty() {
        levels.iter().find(|l| l.guid.as_deref() == Some(st.top_level.as_str()))
    } else {
        i.and_then(|i| levels.get(i.saturating_add(1)))
    };
    if let (Some(i), Some(top)) = (i, top)
        && let Some(base) = levels.get(i)
        && top.elevation > base.elevation + 100.0
    {
        return (top.elevation - base.elevation, Some(top.clone()));
    }
    let lh = i.and_then(|i| levels.get(i)).map(|l| l.height).filter(|h| *h != 0.0 && !h.is_nan()).unwrap_or(2743.0);
    (if st.fixed_height > 0.0 { st.fixed_height } else { lh }, None)
}

pub(crate) fn wells_of(w: &mut W<'_>, guid: &str) -> R<Vec<Id>> {
    let mut out = Vec::new();
    for id in w.ids("IFCOPENINGELEMENT") {
        if let Some(ps) = w.read_pset(id, WELL_PSET)?
            && matches!(ps.get("Stair"), Some(P::Str(s)) if s.lossy() == guid)
        {
            out.push(id);
        }
    }
    Ok(out)
}

struct FloorAbove {
    id: Id,
    thickness: f64,
    offset: f64,
}
fn floors_of(w: &mut W<'_>, storey: Id) -> R<Vec<FloorAbove>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCSLAB") {
        if so.get(&id_key(id)).copied() != Some(storey) {
            continue;
        }
        let Some(ps) = w.read_pset(id, "CadCraft_Floor")? else { continue };
        out.push(FloorAbove { id, thickness: num(&ps, "Thickness") * um, offset: num0(&ps, "Offset") * um });
    }
    Ok(out)
}

// ----------------------------------------------------------------------------------------------------------- writing
/// `writeStair(w, st, { levels })` → true when it was made again.
pub(crate) fn write_stair(w: &mut W<'_>, st: &Stair, levels: &[Level]) -> R<bool> {
    let Some(storey) = st.storey else { return Ok(false) };
    let (height, top) = stair_height(st, levels);
    let floors = match &top {
        Some(t) => floors_of(w, t.id)?,
        None => Vec::new(),
    };
    let Ok(l) = stair_layout(st, height) else { return Ok(false) };
    let key = shape_key(vec![
        K::S(st.kind.clone()),
        K::S(st.turn.clone()),
        K::A(vec![K::N(st.start[0]), K::N(st.start[1])]),
        K::N(st.angle),
        K::N(st.width),
        K::N(height),
        K::N(st.riser_max),
        K::N(st.tread),
        K::N(st.split),
        K::N(st.gap),
        K::S(st.railings.clone()),
        K::N(st.rail_height),
        K::B(st.well),
        K::A(floors.iter().map(|f| K::A(vec![K::N(f.id), K::N(f.thickness), K::N(f.offset)])).collect()),
    ]);
    if st.key == key && !st.parts.is_empty() {
        return Ok(false);
    }
    if !st.parts.is_empty() {
        w.drop_product(&st.parts)?;
    }
    let old = wells_of(w, &st.guid)?;
    if !old.is_empty() {
        w.drop_product(&old)?;
    }
    let st_lp = w.must(storey)?.a("ObjectPlacement").href();
    let wood = w.surface_style(&t0("res.style.stair"), [0.72, 0.58, 0.42], 0.0)?;
    let mut parts: Vec<Id> = Vec::new();
    let mut fi = 0u32;
    for pc in &l.pieces {
        if let Piece::Landing(t) = pc {
            let nm = w.l(&t0("res.st.landing"));
            let lp = w.place_at(st_lp, 0.0, 0.0, 0.0, 0.0)?;
            let g = w.guid();
            let oh = w.owner_history();
            let id = w.make("IFCSLAB", vec![g, oh, nm.clone(), JV::Null, nm, JV::H(lp), JV::Null, JV::Null, JV::s("LANDING")])?;
            let pr = w.profile(&t.poly, None)?;
            let th = m::max(l.riser, 150.0);
            let it = w.extrude(pr, t.z - th, th)?;
            w.style_item(it, wood)?;
            let rep = w.rep("body", "Body", "SweptSolid", &[it])?;
            w.set_shape(id, &[rep])?;
            parts.push(id);
            continue;
        }
        let (winder, treads) = match pc {
            Piece::Winders(t) => (true, t),
            Piece::Flight(t) => (false, t),
            Piece::Landing(_) => continue,
        };
        fi = fi.saturating_add(1);
        let nm = w.l(&t(if winder { "res.st.winders" } else { "res.st.flight" }, &[("n", &fi.to_string())]));
        let nt = treads.len() as f64;
        let mut extra = vec![JV::Num(nt + 1.0), JV::Num(nt), JV::Num(round6(w.u(l.riser))), JV::Num(round6(w.u(l.tread)))];
        if w.v4 {
            extra.push(JV::s(if winder { "WINDER" } else { "STRAIGHT" }));
        }
        let lp = w.place_at(st_lp, 0.0, 0.0, 0.0, 0.0)?;
        let g = w.guid();
        let oh = w.owner_history();
        let mut args = vec![g, oh, nm.clone(), JV::Null, nm, JV::H(lp), JV::Null, JV::Null];
        args.extend(extra);
        let id = w.make("IFCSTAIRFLIGHT", args)?;
        let mut items = Vec::new();
        for t in treads {
            let pr = w.profile(&t.poly, None)?;
            let it = w.extrude(pr, t.z - l.riser, l.riser)?;
            w.style_item(it, wood)?;
            items.push(it);
        }
        let rep = w.rep("body", "Body", "SweptSolid", &items)?;
        w.set_shape(id, &[rep])?;
        parts.push(id);
    }
    // handrails
    let steel = w.surface_style(&t0("res.style.rail"), [0.95, 0.95, 0.93], 0.0)?;
    let (mut rail_len, mut balusters) = (0.0, 0usize);
    for side in &l.sides {
        let rp = railing_parts(side, st.rail_height);
        rail_len += rp.length;
        balusters = balusters.saturating_add(rp.balusters.len());
        let items = railing_items(w, &rp)?;
        if items.is_empty() {
            continue;
        }
        for &it in &items {
            w.style_item(it, steel)?;
        }
        let nm = w.l(&t0("res.rail.handrail"));
        let lp = w.place_at(st_lp, 0.0, 0.0, 0.0, 0.0)?;
        let g = w.guid();
        let oh = w.owner_history();
        let pt = if w.v4 { "HANDRAIL" } else { "NOTDEFINED" };
        let id = w.make("IFCRAILING", vec![g, oh, nm.clone(), JV::Null, nm, JV::H(lp), JV::Null, JV::Null, JV::s(pt)])?;
        let rep = w.rep("body", "Body", "SweptSolid", &items)?;
        w.set_shape(id, &[rep])?;
        parts.push(id);
    }
    match agg_rel(w, st.id)? {
        Some(rel) => {
            let mut x = w.touch(rel)?;
            x.set("RelatedObjects", JV::L(parts.iter().map(|&q| JV::H(q)).collect()));
            w.write(&mut x)?;
        }
        None if !parts.is_empty() => {
            let g = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(st.id), JV::L(parts.iter().map(|&q| JV::H(q)).collect())])?;
        }
        None => {}
    }
    // the stairwell: where the floor above leaves less than the headroom over the steps
    if st.well
        && let Some(top) = &top
        && !floors.is_empty()
    {
        let mt = mul4(&inv4j(&w.storey_world(top.id)?)?, &w.storey_world(storey)?);
        let mut polys: Vec<Vec<V2>> = Vec::new();
        for f in &floors {
            let under = height + f.offset - f.thickness;
            for pc in &l.pieces {
                let list: Vec<&Tread> = match pc {
                    Piece::Landing(t) => vec![t],
                    Piece::Flight(ts) | Piece::Winders(ts) => ts.iter().collect(),
                };
                for t in list {
                    if t.z + HEADROOM > under {
                        polys.push(t.poly.clone());
                    }
                }
            }
        }
        let loops = if polys.is_empty() { Vec::new() } else { plan_loops_snap(&polys, 0.5).outlines };
        for f in &floors {
            for lp in &loops {
                let fl = w.must(f.id)?;
                let pts: Vec<V2> = lp
                    .poly
                    .iter()
                    .map(|q| {
                        let r = apply4(&mt, q[0], q[1], 0.0);
                        [r[0], r[1]]
                    })
                    .collect();
                let ax = w.axis3(0.0, 0.0, 0.0, 0.0)?;
                let opl = w.make("IFCLOCALPLACEMENT", vec![fl.a("ObjectPlacement").clone(), JV::H(ax)])?;
                let g = w.guid();
                let nm = w.l(&t0("res.st.well"));
                let oh = w.owner_history();
                let mut args = vec![g, oh, nm, JV::Null, JV::Null, JV::H(opl), JV::Null, JV::Null];
                if w.v4 {
                    args.push(JV::s("OPENING"));
                }
                let oid = w.make("IFCOPENINGELEMENT", args)?;
                let pr = w.profile(&pts, None)?;
                let ex = w.extrude(pr, f.offset - f.thickness - 10.0, f.thickness + 20.0)?;
                let rep = w.rep("body", "Body", "SweptSolid", &[ex])?;
                w.set_shape(oid, &[rep])?;
                let g = w.guid();
                let oh = w.owner_history();
                w.make("IFCRELVOIDSELEMENT", vec![g, oh, JV::Null, JV::Null, JV::H(f.id), JV::H(oid)])?;
                w.upsert_pset(oid, WELL_PSET, &[row("Stair", "IFCIDENTIFIER", P::s(&st.guid))])?;
            }
        }
    }
    w.upsert_pset(
        st.id,
        "Pset_StairCommon",
        &[
            row("NumberOfRiser", "IFCCOUNTMEASURE", P::Num(l.n)),
            row("NumberOfTreads", "IFCCOUNTMEASURE", P::Num(l.treads)),
            row("RiserHeight", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(l.riser)))),
            row("TreadLength", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(l.tread)))),
            row("RequiredHeadroom", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(HEADROOM)))),
        ],
    )?;
    let failed: Vec<&str> = l.checks.iter().filter(|(_, ok)| !ok).map(|(k, _)| *k).collect();
    w.upsert_pset(
        st.id,
        STAIR_PSET,
        &[
            row("ShapeKey", "IFCIDENTIFIER", P::s(&key)),
            row("Risers", "IFCINTEGER", P::Num(l.n)),
            row("RiserUsed", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(l.riser)))),
            row("Checks", "IFCLABEL", P::s(&if failed.is_empty() { "ok".to_string() } else { failed.join(",") })),
            row("RailLength", "IFCLENGTHMEASURE", P::Num(round6(w.u(rail_len)))),
            row("Balusters", "IFCINTEGER", P::Num(balusters as f64)),
        ],
    )?;
    if w.v4 {
        let mut x = w.touch(st.id)?;
        x.set("PredefinedType", JV::s(ifc_type(&st.kind)));
        w.write(&mut x)?;
    }
    Ok(true)
}

/// `syncStairs(w, storey, ctx)`: stairs starting on this level or arriving at it, made again when the floor-to-floor
/// height or the floor above changed.
pub fn sync_stairs(w: &mut W<'_>, storey: Id, levels: Option<&[Level]>) -> R<usize> {
    let mut n = 0usize;
    let levels = levels_or(w, levels)?;
    for st in read_stairs(w)? {
        let i = levels.iter().position(|l| Some(l.id) == st.storey);
        let top = if !st.top_level.is_empty() {
            levels.iter().find(|l| l.guid.as_deref() == Some(st.top_level.as_str()))
        } else {
            match i {
                Some(i) => levels.get(i.saturating_add(1)),
                None => levels.first(),
            }
        };
        if st.storey != Some(storey) && top.map(|t| t.id) != Some(storey) {
            continue;
        }
        if write_stair(w, &st, &levels)? {
            n = n.saturating_add(1);
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stair(kind: &str) -> Stair {
        Stair {
            id: 1.0,
            guid: String::new(),
            storey: None,
            top_level: String::new(),
            kind: kind.into(),
            turn: "left".into(),
            start: [0.0, 0.0],
            angle: 0.0,
            width: 36.0 * IN,
            fixed_height: 0.0,
            riser_max: 7.75 * IN,
            tread: 10.0 * IN,
            split: 0.5,
            gap: 50.0,
            railings: "both".into(),
            rail_height: 36.0 * IN,
            well: true,
            key: String::new(),
            parts: Vec::new(),
        }
    }

    #[test]
    fn risers_from_the_height() {
        let l = stair_layout(&stair("straight"), 3000.0).ok();
        let l = l.as_ref();
        assert_eq!(l.map(|l| l.n), Some(16.0));
        assert_eq!(l.map(|l| l.sides.len()), Some(2));
        for k in STAIR_KINDS {
            assert!(stair_layout(&stair(k), 3000.0).is_ok(), "{k}");
        }
        assert_eq!(stair_layout(&stair("winder"), 600.0).err(), Some("res.err.stairshort"));
    }

    #[test]
    fn railing_balusters_keep_the_gap() {
        let r = railing_parts(&[[0.0, 0.0, 0.0], [3000.0, 0.0, 0.0]], 900.0);
        assert_eq!(r.posts.len(), 2);
        let xs: Vec<f64> = r.balusters.iter().map(|b| b[0]).collect();
        assert!(xs.windows(2).all(|w| w[1] - w[0] - RAIL_BALUSTER < RAIL_GAP + 1e-6));
        assert!(railing_parts(&[[0.0, 0.0, 0.0]], 900.0).rails.is_empty());
    }
}
