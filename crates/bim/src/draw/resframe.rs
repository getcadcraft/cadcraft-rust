//! res-framing.js (0.5 part 4, RC.9), the half that follows the model: a framed wall's plates, studs, kings, jacks,
//! headers, cripples, sills, corner and tee studs and blocking; a floor's joists, rims, trimmers and headers round its
//! openings; a roof's rafters, hips, valleys, ridge boards, ceiling joists and collar ties (or trusses) — labelled
//! defaults, not engineering — read back from the framing assembly and made again when the host changed
//! (`syncFraming`); pieces kept by hand stay and get flagged.

use std::collections::HashMap;

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::levels::{Level, read_levels};
use crate::draw::m::{self, V2, at, cross, dot, hypot, hypot3, nx, sub};
use crate::draw::openings::{Opening, read_openings};
use crate::draw::res::{agg_rel, inset_poly, is_false, is_true, num, parts_of, s_or};
use crate::draw::roof::{EdgeKind, RoofPlan, edge_kinds, height_at, roof_defaults, roof_geometry};
use crate::draw::rooms::loop_at;
use crate::draw::skeleton::face_z;
use crate::draw::slabs::{Derived, Floor, K, kpts, read_derived, shape_key_scaled, storey_loops};
use crate::draw::tr::t0;
use crate::draw::walls::{Wall, read_walls, wall_type};
use crate::draw::writer::{W, inv4j, round_d, row};
use crate::util::{apply4, mul4};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;
const FT: f64 = 304.8;
pub const FRAME_PSET: &str = "CadCraft_Framing";
pub const LUMBER_PSET: &str = "CadCraft_Lumber";
const SIZES: [&str; 5] = ["2x4", "2x6", "2x8", "2x10", "2x12"];
const DEFAULT_SPACING: f64 = 16.0 * IN;

/// LUMBER: (thickness, depth, nominal)
fn lumber(size: &str) -> Option<(f64, f64, [f64; 2])> {
    Some(match size {
        "2x4" => (1.5 * IN, 3.5 * IN, [2.0, 4.0]),
        "2x6" => (1.5 * IN, 5.5 * IN, [2.0, 6.0]),
        "2x8" => (1.5 * IN, 7.25 * IN, [2.0, 8.0]),
        "2x10" => (1.5 * IN, 9.25 * IN, [2.0, 10.0]),
        "2x12" => (1.5 * IN, 11.25 * IN, [2.0, 12.0]),
        _ => return None,
    })
}
fn depth_of(size: &str) -> f64 {
    lumber(size).map_or(f64::NAN, |l| l.1)
}
fn thick_of(size: &str) -> f64 {
    lumber(size).map_or(f64::NAN, |l| l.0)
}

type Table = [(&'static str, f64); 4];
fn joist_table(sp: f64) -> Table {
    if (sp - 609.6).abs() < 50.0 {
        [("2x6", 8.5 * FT), ("2x8", 11.25 * FT), ("2x10", 14.3 * FT), ("2x12", 17.4 * FT)]
    } else {
        [("2x6", 9.75 * FT), ("2x8", 12.8 * FT), ("2x10", 16.4 * FT), ("2x12", 19.9 * FT)]
    }
}
fn rafter_table(sp: f64) -> Table {
    if (sp - 609.6).abs() < 50.0 {
        [("2x6", 11.0 * FT), ("2x8", 14.0 * FT), ("2x10", 17.0 * FT), ("2x12", 20.5 * FT)]
    } else {
        [("2x6", 13.5 * FT), ("2x8", 17.0 * FT), ("2x10", 21.0 * FT), ("2x12", 25.0 * FT)]
    }
}
fn ceiling_table(sp: f64) -> Table {
    if (sp - 609.6).abs() < 50.0 {
        [("2x4", 8.5 * FT), ("2x6", 13.5 * FT), ("2x8", 17.75 * FT), ("2x10", 22.75 * FT)]
    } else {
        [("2x4", 10.0 * FT), ("2x6", 15.5 * FT), ("2x8", 20.5 * FT), ("2x10", 26.0 * FT)]
    }
}
/// `sizeFor(table, span)` → (size, over)
fn size_for(t: &Table, span: f64) -> (&'static str, bool) {
    for (size, max) in t {
        if span <= max + 1e-6 {
            return (size, false);
        }
    }
    (t[3].0, true)
}
/// `headerFor(width, bearing)` → (size, plies, over)
fn header_for(width: f64, bearing: bool) -> (&'static str, f64, bool) {
    if !bearing {
        return (if width > 8.0 * FT { "2x12" } else { "2x4" }, 2.0, width > 8.0 * FT);
    }
    for (max, size) in [(3.0 * FT, "2x6"), (4.0 * FT, "2x8"), (6.0 * FT, "2x10"), (8.0 * FT, "2x12")] {
        if width <= max + 1e-6 {
            return (size, 2.0, false);
        }
    }
    ("2x12", 3.0, true)
}
/// `boardFeet(size, length)`
fn board_feet(size: &str, length: f64) -> f64 {
    lumber(size).map_or(0.0, |l| l.2[0] * l.2[1] * (length / FT) / 12.0)
}

/// A piece of framing.
#[derive(Clone, Debug)]
pub enum Shape {
    /// wall axes: x along, y across, z up
    Box {
        x0: f64,
        x1: f64,
        y0: f64,
        y1: f64,
        z0: f64,
        z1: f64,
    },
    /// a member along a 3D line (plan axes)
    Line {
        a: [f64; 3],
        b: [f64; 3],
        depth: f64,
        flat: bool,
    },
    Truss {
        p: V2,
        acr: V2,
        t0: f64,
        t1: f64,
        plate: f64,
        top: Vec<V2>,
    },
}
#[derive(Clone, Debug)]
pub struct Piece {
    pub role: &'static str,
    pub size: &'static str,
    pub shape: Shape,
    pub length: f64,
    pub width: f64,
    pub plies: f64,
}

/// What a frame* function gives back.
#[derive(Clone, Debug, Default)]
pub struct Framed {
    pub pieces: Vec<Piece>,
    pub size: &'static str,
    pub notes: Vec<&'static str>,
    pub error: Option<&'static str>,
}

// ------------------------------------------------------------------------------------------------------------ walls
struct Band {
    y0: f64,
    y1: f64,
    depth: f64,
}
/// `studBand(wall)`
fn stud_band(type_key: &str, ext_side: &str) -> Option<Band> {
    let ty = wall_type(type_key)?;
    let t = ty.layers.iter().fold(0.0, |s, l| s + l.t);
    let mut cum = 0.0;
    let mut band = None;
    let sgn = if ext_side == "right" { -1.0 } else { 1.0 };
    for l in &ty.layers {
        if l.mat == "studs" {
            let y0 = sgn * (t / 2.0 - cum - l.t);
            let y1 = sgn * (t / 2.0 - cum);
            band = Some(Band { y0: m::min(y0, y1), y1: m::max(y0, y1), depth: l.t });
        }
        cum += l.t;
    }
    band
}
fn stud_size(depth: f64) -> Option<&'static str> {
    SIZES.iter().copied().find(|s| (depth_of(s) - depth).abs() < 3.0)
}
fn line_hit(p: V2, d: V2, q: V2, e: V2) -> Option<f64> {
    let c = cross(d, e);
    if c.abs() < 1e-9 {
        return None;
    }
    Some(cross(sub(q, p), e) / c)
}

pub struct WallCtx<'a> {
    pub walls: Vec<&'a Wall>,
    pub openings: Vec<&'a Opening>,
    pub spacing: f64,
    pub corners: &'a str,
    pub blocking: bool,
    pub bearing: bool,
}

struct WallB<'p> {
    pieces: &'p mut Vec<Piece>,
    vert: Vec<(f64, f64)>,
    size: &'static str,
    t: f64,
    sd: f64,
    yb0: f64,
    yd: f64,
    flat_y: f64,
    zs0: f64,
    zs1: f64,
}
impl WallB<'_> {
    #[allow(clippy::too_many_arguments)]
    fn bx(&mut self, role: &'static str, sz: &'static str, x0: f64, x1: f64, z0: f64, z1: f64, y0: f64, y1: f64, plies: f64) {
        if x1 - x0 > 1.0 && z1 - z0 > 1.0 {
            self.pieces.push(Piece {
                role,
                size: sz,
                shape: Shape::Box { x0, x1, y0, y1, z0, z1 },
                length: m::max(x1 - x0, z1 - z0),
                width: 0.0,
                plies,
            });
        }
    }
    fn b(&mut self, role: &'static str, x0: f64, x1: f64, z0: f64, z1: f64) {
        let (y0, y1, sz) = (self.yb0, self.yb0 + self.yd, self.size);
        self.bx(role, sz, x0, x1, z0, z1, y0, y1, 1.0);
    }
    fn stud(&mut self, role: &'static str, x: f64, z0: f64, z1: f64, flat: bool) {
        if flat {
            let (fy, t, sz, sd) = (self.flat_y, self.t, self.size, self.sd);
            self.bx(role, sz, x - sd / 2.0, x + sd / 2.0, z0, z1, fy, fy + t, 1.0);
            self.vert.push((x - sd / 2.0, x + sd / 2.0));
            return;
        }
        let t = self.t;
        self.b(role, x - t / 2.0, x + t / 2.0, z0, z1);
        self.vert.push((x - t / 2.0, x + t / 2.0));
    }
    fn stud_std(&mut self, role: &'static str, x: f64) {
        let (z0, z1) = (self.zs0, self.zs1);
        self.stud(role, x, z0, z1, false);
    }
    fn taken(&self, x: f64) -> bool {
        let w = self.t;
        self.vert.iter().any(|(a, b)| x + w / 2.0 > a - 1.0 && x - w / 2.0 < b + 1.0)
    }
}

/// `frameWall(wall, ctx)`
pub fn frame_wall(wall: &Wall, ctx: &WallCtx<'_>) -> Framed {
    let Some(band) = stud_band(&wall.type_key, &wall.ext_side) else {
        return Framed { error: Some("res.err.nostuds"), ..Framed::default() };
    };
    let size = stud_size(band.depth).unwrap_or("2x4");
    let (t, sd) = (thick_of(size), depth_of(size));
    let u = [(wall.b[0] - wall.a[0]) / wall.len, (wall.b[1] - wall.a[1]) / wall.len];
    let n = [-u[1], u[0]];
    let yc = (band.y0 + band.y1) / 2.0;
    let (yb0, yd) = (band.y0, band.y1 - band.y0);
    let to_plan = |x: f64, y: f64| -> V2 { [wall.a[0] + u[0] * x + n[0] * y, wall.a[1] + u[1] * x + n[1] * y] };
    let to_local = |p: V2| -> V2 { [dot(sub(p, wall.a), u), dot(sub(p, wall.a), n)] };
    let h = wall.height;
    let spacing = if ctx.spacing != 0.0 && !ctx.spacing.is_nan() { ctx.spacing } else { DEFAULT_SPACING };
    let flat_y = if wall.ext_side == "right" { band.y1 - t } else { band.y0 };
    let mut pieces = Vec::new();
    let (mut x0, mut x1) = (0.0, wall.len);
    let ends = [("a", wall.a), ("b", wall.b)];
    let mut corner_at: Vec<(&str, f64)> = Vec::new();
    let mut tees: Vec<(f64, f64)> = Vec::new();
    for o in &ctx.walls {
        if o.guid == wall.guid {
            continue;
        }
        let Some(ob) = stud_band(&o.type_key, &o.ext_side) else { continue };
        let ou = [(o.b[0] - o.a[0]) / o.len, (o.b[1] - o.a[1]) / o.len];
        let on = [-ou[1], ou[0]];
        let o_edge = |y: f64| -> V2 { [o.a[0] + on[0] * y, o.a[1] + on[1] * y] };
        for (e_at, e_p) in ends {
            let shared = [o.a, o.b].iter().any(|q| hypot(q[0] - e_p[0], q[1] - e_p[1]) < m::max(wall.t, o.t) / 2.0 + 1.0);
            if shared {
                if cross(u, ou).abs() < 0.05 {
                    continue;
                }
                let through = wall.guid < o.guid;
                let mid = to_plan(wall.len / 2.0, 0.0);
                let (e0, e1) = (o_edge(ob.y0), o_edge(ob.y1));
                let far0 = dot(sub(mid, e0), on).abs() > dot(sub(mid, e1), on).abs();
                let (outer, inner) = if far0 { (e0, e1) } else { (e1, e0) };
                let (Some(t_out), Some(t_in)) = (line_hit(to_plan(0.0, yc), u, outer, ou), line_hit(to_plan(0.0, yc), u, inner, ou)) else {
                    continue;
                };
                if e_at == "a" {
                    x0 = if through { m::min(t_out, t_in) } else { m::max(t_out, t_in) };
                } else {
                    x1 = if through { m::max(t_out, t_in) } else { m::min(t_out, t_in) };
                }
                if through {
                    corner_at.push((e_at, ob.depth));
                }
                continue;
            }
            // this end on the other wall's middle: a T — stop against its studs
            let s = dot(sub(e_p, o.a), ou);
            let d = dot(sub(e_p, o.a), on).abs();
            if s > 1.0 && s < o.len - 1.0 && d < o.t / 2.0 + 1.0 {
                let mid = to_plan(wall.len / 2.0, 0.0);
                let (e0, e1) = (o_edge(ob.y0), o_edge(ob.y1));
                let near0 = dot(sub(mid, e0), on).abs() < dot(sub(mid, e1), on).abs();
                let face = if near0 { e0 } else { e1 };
                if let Some(tf) = line_hit(to_plan(0.0, yc), u, face, ou) {
                    if e_at == "a" {
                        x0 = tf;
                    } else {
                        x1 = tf;
                    }
                }
            }
        }
        // the other wall's end on this wall's middle: backer studs here
        for q in [o.a, o.b] {
            let [s, d] = to_local(q);
            if s > t && s < wall.len - t && d.abs() < wall.t / 2.0 + 1.0 && cross(u, ou).abs() > 0.05 {
                tees.push((s, ob.depth));
            }
        }
    }
    if x1 - x0 < 3.0 * t {
        return Framed { error: Some("res.err.shortwall"), ..Framed::default() };
    }
    let mut notes: Vec<&'static str> = Vec::new();
    {
        let mut b = WallB { pieces: &mut pieces, vert: Vec::new(), size, t, sd, yb0, yd, flat_y, zs0: t, zs1: h - 2.0 * t };
        // plates (the bottom one cut at doors)
        let mut doors: Vec<(f64, f64)> = ctx.openings.iter().filter(|o| o.door).map(|o| (o.offset, o.offset + o.width)).collect();
        doors.sort_by(|p, q| crate::js::sign_ord(p.0 - q.0));
        let mut px = x0;
        for (d0, d1) in doors {
            b.b("plate.bottom", px, m::max(px, d0), 0.0, t);
            px = m::max(px, d1);
        }
        b.b("plate.bottom", px, x1, 0.0, t);
        b.b("plate.top", x0, x1, h - 2.0 * t, h - t);
        b.b("plate.cap", x0, x1, h - t, h);
        let (zs0, zs1) = (t, h - 2.0 * t);
        let mut blocked: Vec<(f64, f64)> = Vec::new();
        let lay = |a: f64, bb: f64| -> Vec<f64> {
            let mut out = Vec::new();
            let mut k = ((a - x0) / spacing).ceil();
            let mut guard = 0u32;
            while x0 + k * spacing < bb && guard < 100_000 {
                out.push(x0 + k * spacing);
                k += 1.0;
                guard = guard.saturating_add(1);
            }
            out
        };
        let mut ops: Vec<&&Opening> = ctx.openings.iter().collect();
        ops.sort_by(|p, q| crate::js::sign_ord(p.offset - q.offset));
        for o in ops {
            let (ro0, ro1, head) = (o.offset, o.offset + o.width, o.sill + o.height);
            let (hsize, plies, over) = header_for(o.width, ctx.bearing);
            let jacks = if o.width > 6.0 * FT { 2.0 } else { 1.0 };
            let (hx0, hx1) = (ro0 - jacks * t, ro1 + jacks * t);
            let h_depth = depth_of(hsize);
            let mut j = 0.0;
            while j < jacks {
                b.stud("jack", ro0 - t * (j + 0.5), zs0, head, false);
                b.stud("jack", ro1 + t * (j + 0.5), zs0, head, false);
                j += 1.0;
            }
            b.stud_std("king", hx0 - t / 2.0);
            b.stud_std("king", hx1 + t / 2.0);
            let h_top = m::min(head + h_depth, zs1);
            b.bx("header", hsize, hx0, hx1, head, h_top, yb0, yb0 + yd, plies);
            if over {
                notes.push("header");
            }
            if head + h_depth > zs1 + 1.0 {
                notes.push("headerroom");
            }
            if zs1 - h_top > t {
                for x in lay(hx0 + t / 2.0, hx1 - t / 2.0) {
                    b.stud("cripple", x, h_top, zs1, false);
                }
            }
            if !o.door && o.sill > 2.0 * t {
                b.b("sill", ro0, ro1, o.sill - t, o.sill);
                let mut xs = vec![ro0 + t / 2.0];
                xs.extend(lay(ro0 + t, ro1 - t));
                xs.push(ro1 - t / 2.0);
                for x in xs {
                    b.stud("cripple", x, zs0, o.sill - t, false);
                }
            }
            blocked.push((hx0 - t, hx1 + t));
        }
        // ends, corners and tees
        b.stud_std("stud.end", x0 + t / 2.0);
        b.stud_std("stud.end", x1 - t / 2.0);
        for (c_at, c_depth) in &corner_at {
            let sgn = if *c_at == "a" { 1.0 } else { -1.0 };
            let xe = if *c_at == "a" { x0 } else { x1 };
            if ctx.corners == "california" {
                b.stud("stud.corner", xe + sgn * (c_depth + sd / 2.0), zs0, zs1, true);
            } else {
                b.stud_std("stud.corner", xe + sgn * (t * 1.5));
                b.stud_std("stud.corner", xe + sgn * (c_depth + t * 0.5));
            }
        }
        for (tx, td) in &tees {
            b.stud_std("stud.tee", tx - td / 2.0 - t / 2.0);
            b.stud_std("stud.tee", tx + td / 2.0 + t / 2.0);
        }
        // the layout: on centre from the wall's start
        let mut k = 1.0;
        let mut guard = 0u32;
        while x0 + k * spacing < x1 - t && guard < 100_000 {
            guard = guard.saturating_add(1);
            let x = x0 + k * spacing;
            k += 1.0;
            if blocked.iter().any(|(a, bb)| x > *a && x < *bb) || b.taken(x) {
                continue;
            }
            b.stud_std("stud", x);
        }
        // blocking: one row between the studs at mid-height (not across openings)
        if ctx.blocking {
            let zb = m::min(48.0 * IN, (zs0 + zs1) / 2.0);
            let mut xs = b.vert.clone();
            xs.sort_by(|p, q| crate::js::sign_ord(p.0 - q.0));
            for pair in xs.windows(2) {
                let [p0, p1] = pair else { continue };
                let (a, bb) = (p0.1, p1.0);
                if bb - a < 2.0 || blocked.iter().any(|(p, q)| (a + bb) / 2.0 > *p && (a + bb) / 2.0 < *q) {
                    continue;
                }
                b.b("blocking", a, bb, zb - t / 2.0, zb + t / 2.0);
            }
        }
    }
    Framed { pieces, size, notes, error: None }
}

// ------------------------------------------------------------------------------------------------------------ floors
/// `clipLine(p, d, poly)` → [t0, t1] intervals inside it.
fn clip_line(p: V2, d: V2, poly: &[V2]) -> Vec<(f64, f64)> {
    let mut ts = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (at(poly, i), at(poly, nx(i, n)));
        let e = sub(b, a);
        let c = cross(d, e);
        if c.abs() < 1e-12 {
            continue;
        }
        let t = cross(sub(a, p), e) / c;
        let s = cross(sub(a, p), d) / c;
        if (-1e-9..1.0 - 1e-9).contains(&s) {
            ts.push(t);
        }
    }
    ts.sort_by(|x, y| crate::js::sign_ord(x - y));
    let mut out = Vec::new();
    let mut i = 0usize;
    while i.saturating_add(1) < ts.len() {
        let (a, b) = (ts.get(i).copied().unwrap_or(0.0), ts.get(i.saturating_add(1)).copied().unwrap_or(0.0));
        if b - a > 1.0 {
            out.push((a, b));
        }
        i = i.saturating_add(2);
    }
    out
}

struct OpBox {
    s0: f64,
    s1: f64,
    c0: f64,
    c1: f64,
}

/// `frameFloor(poly, { spacing, top, openings })` (+ the span it found)
fn frame_floor(poly: &[V2], spacing: f64, top0: f64, openings: &[Vec<V2>]) -> (Framed, f64) {
    let sp = if spacing != 0.0 && !spacing.is_nan() { spacing } else { DEFAULT_SPACING };
    let mut pieces = Vec::new();
    let mut notes = Vec::new();
    let n = poly.len();
    let mut best: Option<(f64, V2)> = None;
    for i in 0..n {
        let (a, b) = (at(poly, i), at(poly, nx(i, n)));
        let l = hypot(b[0] - a[0], b[1] - a[1]);
        if best.is_none_or(|(bl, _)| l > bl) {
            best = Some((l, [(b[0] - a[0]) / l, (b[1] - a[1]) / l]));
        }
    }
    let u = best.map_or([f64::NAN, f64::NAN], |b| b.1);
    let v = [-u[1], u[0]];
    let ext = |d: V2| {
        let s: Vec<f64> = poly.iter().map(|p| dot(*p, d)).collect();
        m::max_of(s.iter().copied()) - m::min_of(s.iter().copied())
    };
    let ax = if ext(u) <= ext(v) { u } else { v };
    let across = [-ax[1], ax[0]];
    let inner = inset_poly(poly, 1.5 * IN);
    let cs: Vec<f64> = inner.iter().map(|p| dot(*p, across)).collect();
    let (c0, c1) = (m::min_of(cs.iter().copied()), m::max_of(cs.iter().copied()));
    let mut lines = Vec::new();
    let mut c = c0 + sp;
    let mut guard = 0u32;
    while c < c1 - 1.0 && guard < 1_000_000 {
        lines.push(c);
        c += sp;
        guard = guard.saturating_add(1);
    }
    let ops: Vec<OpBox> = openings
        .iter()
        .map(|op| {
            let ss: Vec<f64> = op.iter().map(|p| dot(*p, ax)).collect();
            let cc: Vec<f64> = op.iter().map(|p| dot(*p, across)).collect();
            OpBox {
                s0: m::min_of(ss.iter().copied()),
                s1: m::max_of(ss.iter().copied()),
                c0: m::min_of(cc.iter().copied()),
                c1: m::max_of(cc.iter().copied()),
            }
        })
        .collect();
    let segs_at = |c: f64| clip_line([across[0] * c, across[1] * c], ax, &inner);
    let mut span: f64 = 0.0;
    for &c in &lines {
        for (t0, t1) in segs_at(c) {
            span = m::max(span, t1 - t0);
        }
    }
    let (size, over) = size_for(&joist_table(sp), span);
    if over {
        notes.push("joist");
    }
    let (lt, ld) = (thick_of(size), depth_of(size));
    let top = top0 - 0.75 * IN;
    let pp = |c: f64, s: f64| -> V2 { [across[0] * c + ax[0] * s, across[1] * c + ax[1] * s] };
    let line = |a: V2, b: V2| Shape::Line { a: [a[0], a[1], top - ld / 2.0], b: [b[0], b[1], top - ld / 2.0], depth: ld, flat: true };
    let joist = |pieces: &mut Vec<Piece>, role: &'static str, c: f64, s0: f64, s1: f64| {
        if s1 - s0 > 1.0 {
            pieces.push(Piece { role, size, shape: line(pp(c, s0), pp(c, s1)), length: s1 - s0, width: lt, plies: 1.0 });
        }
    };
    for &c in &lines {
        for (t0, t1) in segs_at(c) {
            // cut round openings (tail joists), the trimmer lines skip them
            let mut hit: Vec<&OpBox> = ops.iter().filter(|o| c > o.c0 - 3.0 * IN && c < o.c1 + 3.0 * IN).collect();
            hit.sort_by(|p, q| crate::js::sign_ord(p.s0 - q.s0));
            let mut s = t0;
            for o in hit {
                joist(&mut pieces, "joist", c, s, m::max(s, o.s0 - 3.0 * IN));
                s = m::max(s, o.s1 + 3.0 * IN);
            }
            joist(&mut pieces, "joist", c, s, t1);
        }
    }
    for o in &ops {
        for c in [o.c0 - 0.75 * IN, o.c0 - 2.25 * IN, o.c1 + 0.75 * IN, o.c1 + 2.25 * IN] {
            for (t0, t1) in segs_at(c) {
                joist(&mut pieces, "trimmer", c, t0, t1);
            }
        }
        for s in [o.s0 - 0.75 * IN, o.s0 - 2.25 * IN, o.s1 + 0.75 * IN, o.s1 + 2.25 * IN] {
            let (a, b) = (pp(o.c0 - 3.0 * IN, s), pp(o.c1 + 3.0 * IN, s));
            pieces.push(Piece {
                role: "header.floor",
                size,
                shape: line(a, b),
                length: hypot(b[0] - a[0], b[1] - a[1]),
                width: lt,
                plies: 1.0,
            });
        }
    }
    // rims: along each edge, 3/4" in
    let rim = inset_poly(poly, 0.75 * IN);
    let rn = rim.len();
    for i in 0..rn {
        let (a, b) = (at(&rim, i), at(&rim, nx(i, rn)));
        pieces.push(Piece { role: "rim", size, shape: line(a, b), length: hypot(b[0] - a[0], b[1] - a[1]), width: lt, plies: 1.0 });
    }
    (Framed { pieces, size, notes, error: None }, span)
}

/// `frameFloorLike(poly, sp, z, table)` → the ceiling joists, over
fn frame_floor_like(poly: &[V2], sp: f64, z: f64) -> (Vec<Piece>, bool) {
    let (r, _) = frame_floor(poly, sp, z + 0.75 * IN, &[]);
    let mut span: f64 = 0.0;
    for p in &r.pieces {
        if p.role == "joist" {
            span = m::max(span, p.length);
        }
    }
    let (size, over) = size_for(&ceiling_table(sp), span);
    let d = depth_of(size);
    let pieces = r
        .pieces
        .into_iter()
        .filter(|p| p.role == "joist")
        .map(|p| {
            let shape = match p.shape {
                Shape::Line { a, b, flat, .. } => {
                    Shape::Line { a: [a[0], a[1], z + d / 2.0], b: [b[0], b[1], z + d / 2.0], depth: d, flat }
                }
                s => s,
            };
            Piece { role: "ceilingjoist", size, shape, ..p }
        })
        .collect();
    (pieces, over)
}

// ------------------------------------------------------------------------------------------------------------ roofs
/// `frameRoof({ outline, kinds, plate, thickness, spacing, mode, fascia })`
fn frame_roof(outline: &[V2], kinds: &[EdgeKind], plate: f64, thickness: f64, spacing: f64, mode: &str, fascia: f64) -> Framed {
    let sp = if spacing != 0.0 && !spacing.is_nan() { spacing } else { DEFAULT_SPACING };
    let mut pieces = Vec::new();
    let mut notes = Vec::new();
    let plan: RoofPlan = match roof_geometry(outline, kinds, thickness, plate, fascia, roof_defaults().fascia_t, false) {
        Ok(p) => p,
        Err(e) => return Framed { error: Some(e), ..Framed::default() },
    };
    if mode == "truss" {
        let mut ridges: Vec<_> = plan.lines.iter().filter(|l| l.kind == "ridge").collect();
        ridges.sort_by(|p, q| crate::js::sign_ord(q.len - p.len));
        let dir = match ridges.first() {
            Some(rl) => {
                let d = [rl.b[0] - rl.a[0], rl.b[1] - rl.a[1]];
                let l = hypot(d[0], d[1]);
                [d[0] / l, d[1] / l]
            }
            None => {
                let n = outline.len();
                let mut best: Option<(f64, V2)> = None;
                for i in 0..n {
                    let (a, b) = (at(outline, i), at(outline, nx(i, n)));
                    let l = hypot(b[0] - a[0], b[1] - a[1]);
                    if best.is_none_or(|(bl, _)| l > bl) {
                        best = Some((l, [(b[0] - a[0]) / l, (b[1] - a[1]) / l]));
                    }
                }
                best.map_or([f64::NAN, f64::NAN], |b| b.1)
            }
        };
        let acr = [-dir[1], dir[0]];
        let ss: Vec<f64> = outline.iter().map(|p| dot(*p, dir)).collect();
        let (s0, s1) = (m::min_of(ss.iter().copied()), m::max_of(ss.iter().copied()));
        let mut s = s0 + 0.75 * IN;
        let mut guard = 0u32;
        while s <= s1 - 0.75 * IN + 1e-6 && guard < 100_000 {
            guard = guard.saturating_add(1);
            let p = [dir[0] * s, dir[1] * s];
            for (t0, t1) in clip_line(p, acr, outline) {
                let mut pts: Vec<V2> = Vec::new();
                for k in 0..=40u32 {
                    let tt = t0 + (t1 - t0) * f64::from(k) / 40.0;
                    let q = [p[0] + acr[0] * tt, p[1] + acr[1] * tt];
                    if let Some(z) = height_at(&plan, q)
                        && z > plate + 10.0
                    {
                        pts.push([tt, z]);
                    }
                }
                if pts.len() < 3 {
                    continue;
                }
                pieces.push(Piece {
                    role: "truss",
                    size: "2x4",
                    shape: Shape::Truss { p, acr, t0, t1, plate, top: pts },
                    length: t1 - t0,
                    width: 1.5 * IN,
                    plies: 1.0,
                });
            }
            if s + sp > s1 - 0.75 * IN && s < s1 - 0.75 * IN - 1.0 {
                s = s1 - 0.75 * IN - sp;
            }
            s += sp;
        }
        return Framed { pieces, size: "2x4", notes, error: None };
    }
    // stick framing: rafters square to each face's eave, at spacing along it, clipped by the face
    let mut run: f64 = 0.0;
    for f in &plan.faces {
        for p in &f.f.poly {
            run = m::max(run, dot(sub(*p, [f.f.n[0] * f.f.c, f.f.n[1] * f.f.c]), f.f.n));
        }
    }
    let (size, over) = size_for(&rafter_table(sp), run);
    if over {
        notes.push("rafter");
    }
    let (rt, rd) = (thick_of(size), depth_of(size));
    let up = SIZES
        .iter()
        .position(|s| *s == size)
        .map_or("2x12", |i| SIZES.get(i.saturating_add(1).min(SIZES.len().saturating_sub(1))).copied().unwrap_or("2x12"));
    for rf in &plan.faces {
        let f = &rf.f;
        let o = [f.n[0] * f.c, f.n[1] * f.c];
        let us: Vec<f64> = f.poly.iter().map(|q| dot(sub(*q, o), f.u)).collect();
        let (u0, u1) = (m::min_of(us.iter().copied()), m::max_of(us.iter().copied()));
        let mut uu = u0 + sp / 2.0;
        let mut guard = 0u32;
        while uu < u1 && guard < 100_000 {
            guard = guard.saturating_add(1);
            let base = [o[0] + f.u[0] * uu, o[1] + f.u[1] * uu];
            for (t0, t1) in clip_line(base, f.n, &f.poly) {
                let a = [base[0] + f.n[0] * t0, base[1] + f.n[1] * t0];
                let b = [base[0] + f.n[0] * t1, base[1] + f.n[1] * t1];
                let (za, zb) = (face_z(f, a), face_z(f, b));
                pieces.push(Piece {
                    role: "rafter",
                    size,
                    shape: Shape::Line { a: [a[0], a[1], za], b: [b[0], b[1], zb], depth: rd, flat: false },
                    length: hypot(t1 - t0, zb - za),
                    width: rt,
                    plies: 1.0,
                });
            }
            uu += sp;
        }
    }
    for l in &plan.lines {
        let role = match l.kind {
            "ridge" => "ridge",
            "hip" => "hip",
            _ => "valley",
        };
        pieces.push(Piece {
            role,
            size: up,
            shape: Shape::Line { a: l.a, b: l.b, depth: depth_of(up), flat: false },
            length: l.len,
            width: rt,
            plies: 1.0,
        });
    }
    // ceiling joists at the plate across the short way, and collar ties under each ridge every 4'
    let (cj, cj_over) = frame_floor_like(outline, sp, plate);
    pieces.extend(cj);
    if cj_over {
        notes.push("ceiling");
    }
    for l in plan.lines.iter().filter(|x| x.kind == "ridge") {
        let d = [l.b[0] - l.a[0], l.b[1] - l.a[1]];
        let len = hypot(d[0], d[1]);
        if len < 1.0 {
            continue;
        }
        let u = [d[0] / len, d[1] / len];
        let acr = [-u[1], u[0]];
        let zr = l.a[2];
        let zc = plate + (zr - plate) * 2.0 / 3.0;
        let mut s = 2.0 * FT;
        while s < len - 1.0 && s < 1e7 {
            let mm = [l.a[0] + u[0] * s, l.a[1] + u[1] * s];
            let reach = |sgn: f64| {
                let (mut lo, mut hi) = (0.0, 20000.0);
                for _ in 0..40 {
                    let mid = (lo + hi) / 2.0;
                    let q = [mm[0] + acr[0] * sgn * mid, mm[1] + acr[1] * sgn * mid];
                    match height_at(&plan, q) {
                        Some(z) if z > zc => lo = mid,
                        _ => hi = mid,
                    }
                }
                lo
            };
            let (r1, r2) = (reach(1.0), reach(-1.0));
            s += 4.0 * FT;
            if r1 + r2 < 300.0 {
                continue;
            }
            let a = [mm[0] - acr[0] * r2, mm[1] - acr[1] * r2];
            let b = [mm[0] + acr[0] * r1, mm[1] + acr[1] * r1];
            pieces.push(Piece {
                role: "collar",
                size: "2x6",
                shape: Shape::Line { a: [a[0], a[1], zc], b: [b[0], b[1], zc], depth: depth_of("2x6"), flat: true },
                length: r1 + r2,
                width: rt,
                plies: 1.0,
            });
        }
    }
    Framed { pieces, size, notes, error: None }
}

// ------------------------------------------------------------------------------------------------------------ IFC
fn cls_of(role: &str) -> (&'static str, &'static str) {
    match role {
        "stud" | "stud.end" | "stud.corner" | "stud.tee" | "king" | "jack" | "cripple" => ("IFCMEMBER", "STUD"),
        "plate.bottom" | "plate.top" | "plate.cap" | "sill" => ("IFCMEMBER", "PLATE"),
        "header" => ("IFCBEAM", "LINTEL"),
        "joist" | "rim" | "trimmer" | "header.floor" | "ceilingjoist" => ("IFCBEAM", "JOIST"),
        "rafter" | "hip" | "valley" => ("IFCMEMBER", "RAFTER"),
        "ridge" => ("IFCBEAM", "USERDEFINED"),
        "collar" => ("IFCMEMBER", "COLLAR"),
        _ => ("IFCMEMBER", "USERDEFINED"),
    }
}

/// How a piece's axes map into the assembly's storey.
enum Map {
    Wall { a: V2, u: V2, n: V2, z: f64 },
    Plan([f64; 16]),
}
impl Map {
    fn p(&self, x: f64, y: f64, z: f64) -> [f64; 3] {
        match self {
            Map::Wall { a, u, n, z: wz } => [a[0] + u[0] * x + n[0] * y, a[1] + u[1] * x + n[1] * y, wz + z],
            Map::Plan(mm) => apply4(mm, x, y, z),
        }
    }
    fn d(&self, x: f64, y: f64, z: f64) -> [f64; 3] {
        match self {
            Map::Wall { u, n, .. } => [u[0] * x + n[0] * y, u[1] * x + n[1] * y, z],
            Map::Plan(mm) => {
                let a = apply4(mm, x, y, z);
                let o = apply4(mm, 0.0, 0.0, 0.0);
                [a[0] - o[0], a[1] - o[1], a[2] - o[2]]
            }
        }
    }
}

fn simplify(p: &[V2]) -> Vec<V2> {
    let n = p.len();
    (0..n)
        .filter(|&i| {
            if i < 2 || i == n.saturating_sub(1) {
                return true;
            }
            let (q, a, b) = (at(p, i), at(p, i.saturating_sub(1)), at(p, i.saturating_add(1)));
            ((b[1] - q[1]) * (q[0] - a[0]) - (q[1] - a[1]) * (b[0] - q[0])).abs() > 1e-3
        })
        .map(|i| at(p, i))
        .collect()
}

/// `pieceItem(w, pc, P, D)`
fn piece_item(w: &mut W<'_>, pc: &Piece, mp: &Map) -> R<Id> {
    match &pc.shape {
        Shape::Box { x0, x1, y0, y1, z0, z1 } => {
            let pr = w.profile(&[[0.0, 0.0], [x1 - x0, 0.0], [x1 - x0, y1 - y0], [0.0, y1 - y0]], None)?;
            let fr = w.axis_frame(mp.p(*x0, *y0, *z0), mp.d(0.0, 0.0, 1.0), mp.d(1.0, 0.0, 0.0))?;
            w.extrude_at(pr, fr, z1 - z0, [0.0, 0.0, 1.0])
        }
        Shape::Truss { p, acr, t0, t1, plate, top } => {
            let o = [p[0] + acr[0] * t0, p[1] + acr[1] * t0];
            let mut prof = vec![[0.0, 0.0], [t1 - t0, 0.0]];
            for q in top.iter().rev() {
                prof.push([q[0] - t0, q[1] - plate]);
            }
            let x = mp.d(acr[0], acr[1], 0.0);
            let zd = mp.d(-acr[1], acr[0], 0.0);
            let oo = mp.p(o[0] - zd[0] * pc.width / 2.0, o[1] - zd[1] * pc.width / 2.0, *plate);
            let pr = w.profile(&simplify(&prof), None)?;
            let fr = w.axis_frame(oo, zd, x)?;
            w.extrude_at(pr, fr, pc.width, [0.0, 0.0, 1.0])
        }
        Shape::Line { a, b, depth, flat } => {
            let aa = mp.p(a[0], a[1], a[2]);
            let bb = mp.p(b[0], b[1], b[2]);
            let d = [bb[0] - aa[0], bb[1] - aa[1], bb[2] - aa[2]];
            let l = hypot3(d[0], d[1], d[2]);
            let hz = hypot(d[0], d[1]);
            let hz = if hz != 0.0 && !hz.is_nan() { hz } else { 1.0 };
            let x = [d[0] / hz, d[1] / hz, 0.0];
            let z = [-x[1], x[0], 0.0];
            let slope = d[2].abs() / hz;
            let vd = depth * (1.0 + slope * slope).sqrt();
            let zb = if *flat { aa[2] - vd / 2.0 } else { aa[2] };
            let fr = w.axis_frame([aa[0] - z[0] * pc.width / 2.0, aa[1] - z[1] * pc.width / 2.0, zb], x, z)?;
            let dl = [d[0] / l, d[1] / l, d[2] / l];
            let ex = [dl[0] * z[0] + dl[1] * z[1], dl[2], dl[0] * x[0] + dl[1] * x[1]];
            let pr = w.profile(&[[0.0, 0.0], [pc.width, 0.0], [pc.width, vd], [0.0, vd]], None)?;
            w.extrude_at(pr, fr, l, ex)
        }
    }
}

/// A framing read back (`readFraming`).
#[derive(Clone, Debug)]
pub struct Framing {
    pub id: Id,
    pub storey: Option<Id>,
    pub host: String,
    pub spacing: f64,
    pub corners: String,
    pub blocking: bool,
    pub roof_mode: String,
    pub key: String,
    pub parts: Vec<Id>,
}

pub fn read_framing(w: &mut W<'_>) -> R<Vec<Framing>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCELEMENTASSEMBLY") {
        let Some(ps) = w.read_pset(id, FRAME_PSET)? else { continue };
        let sp = num(&ps, "Spacing") * um;
        let is_str = |k: &str, v: &str| matches!(ps.get(k), Some(P::Str(s)) if s.lossy() == v);
        out.push(Framing {
            id,
            storey: so.get(&id_key(id)).copied(),
            host: s_or(&ps, "Host"),
            spacing: if sp != 0.0 && !sp.is_nan() { sp } else { DEFAULT_SPACING },
            corners: if is_str("Corners", "california") { "california".into() } else { "three-stud".into() },
            blocking: !is_false(ps.get("Blocking")),
            roof_mode: if is_str("RoofMode", "truss") { "truss".into() } else { "stick".into() },
            key: s_or(&ps, "ShapeKey"),
            parts: parts_of(w, id)?,
        });
    }
    Ok(out)
}

#[derive(Default)]
struct Cache {
    walls: Option<Vec<Wall>>,
    derived: Option<Derived>,
    levels: Option<Vec<Level>>,
    openings: Option<Vec<Opening>>,
}

enum Host {
    Wall(usize),
    Floor(usize),
    Roof(usize),
}
/// `hostOf(w, guid, cache)` → (the host, its storey)
fn host_of(w: &mut W<'_>, guid: &str, cache: &mut Cache) -> R<Option<(Host, Option<Id>)>> {
    if cache.walls.is_none() {
        cache.walls = Some(read_walls(w, None)?);
    }
    if let Some(i) = cache.walls.as_ref().and_then(|ws| ws.iter().position(|x| x.guid == guid)) {
        let st = cache.walls.as_ref().and_then(|ws| ws.get(i)).and_then(|x| x.storey);
        return Ok(Some((Host::Wall(i), st)));
    }
    if cache.derived.is_none() {
        cache.derived = Some(read_derived(w)?);
    }
    let Some(d) = cache.derived.as_ref() else { return Ok(None) };
    if let Some(i) = d.floors.iter().position(|x| x.guid == guid) {
        return Ok(Some((Host::Floor(i), d.floors.get(i).and_then(|f| f.storey))));
    }
    if let Some(i) = d.roofs.iter().position(|x| x.guid == guid) {
        let walls_lv = d.roofs.get(i).map(|r| r.walls.clone()).unwrap_or_default();
        if cache.levels.is_none() {
            cache.levels = Some(read_levels(w)?);
        }
        let st = cache.levels.as_ref().and_then(|ls| ls.iter().find(|l| l.guid.as_deref() == Some(walls_lv.as_str()))).map(|l| l.id);
        return Ok(Some((Host::Roof(i), st)));
    }
    Ok(None)
}

fn opts_key(fr: &Framing) -> K {
    K::O(vec![("spacing", K::N(fr.spacing)), ("corners", K::S(fr.corners.clone())), ("blocking", K::B(fr.blocking))])
}
fn p2k(p: V2) -> K {
    K::A(vec![K::N(p[0]), K::N(p[1])])
}

/// `floorOpenings(w, slabId)`: openings cut in a floor slab, their outlines in the floor's level plan.
fn floor_openings(w: &mut W<'_>, slab: Id) -> R<Vec<Vec<Vec<f64>>>> {
    let mut out = Vec::new();
    let hi = w.host_index()?;
    let um = w.unit_mm;
    for &oid in hi.voids.get(&id_key(slab)).map(Vec::as_slice).unwrap_or(&[]) {
        let get = |w: &W<'_>, v: Option<f64>| -> Option<crate::wapi::Line> { v.and_then(|id| w.line(id).ok().flatten()) };
        let Some(op) = get(w, Some(oid)) else { continue };
        let body = get(w, op.a("Representation").href());
        let rep = body.as_ref().and_then(|b| match b.a("Representations") {
            JV::L(l) => l.first().and_then(JV::href),
            _ => None,
        });
        let rep = get(w, rep);
        let ex = rep.as_ref().and_then(|r| match r.a("Items") {
            JV::L(l) => l.first().and_then(JV::href),
            _ => None,
        });
        let ex = get(w, ex);
        let prof = get(w, ex.as_ref().and_then(|e| e.a("SweptArea").href()));
        let pl = get(w, prof.as_ref().and_then(|p| p.a("OuterCurve").href()));
        let mut pts: Vec<Vec<f64>> = Vec::new();
        let mut bad = false;
        if let Some(pl) = &pl {
            for h in pl.a("Points").hrefs() {
                let Some(pt) = get(w, Some(h)) else {
                    bad = true;
                    break;
                };
                match pt.a("Coordinates") {
                    JV::L(cs) => pts.push(cs.iter().map(|c| c.val().number() * um).collect()),
                    _ => {
                        bad = true;
                        break;
                    }
                }
            }
        }
        if bad || pts.len() < 3 {
            continue;
        }
        let (f, l) = (pts.first().cloned().unwrap_or_default(), pts.last().cloned().unwrap_or_default());
        let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        if hypot(g(&f, 0) - g(&l, 0), g(&f, 1) - g(&l, 1)) < 1.0 {
            pts.pop();
        }
        out.push(pts);
    }
    Ok(out)
}

struct Made {
    framed: Framed,
    key: String,
    map: MapKind,
}
enum MapKind {
    Wall(V2, V2, V2, f64),
    Plan(Option<Id>),
}

/// `piecesFor(w, fr, h, cache)`
fn pieces_for(w: &mut W<'_>, fr: &Framing, host: &Host, storey: Option<Id>, cache: &mut Cache) -> R<Made> {
    let ok = |framed: Framed, key: String, map: MapKind| Ok(Made { framed, key, map });
    let err =
        |e: &'static str| Ok(Made { framed: Framed { error: Some(e), ..Framed::default() }, key: String::new(), map: MapKind::Plan(None) });
    match host {
        Host::Wall(i) => {
            if cache.openings.is_none() {
                let ws = cache.walls.clone().unwrap_or_default();
                cache.openings = Some(read_openings(w, None, &ws)?);
            }
            let walls_all = cache.walls.as_deref().unwrap_or(&[]);
            let Some(hw) = walls_all.get(*i) else { return err("model.err.gone") };
            let walls: Vec<&Wall> = walls_all.iter().filter(|x| x.storey == storey).collect();
            let ops: Vec<&Opening> = cache.openings.as_deref().unwrap_or(&[]).iter().filter(|o| o.wall_guid == hw.guid).collect();
            let bearing = wall_type(&hw.type_key).is_none_or(|t| t.exterior);
            let ctx = WallCtx {
                walls: walls.clone(),
                openings: ops.clone(),
                spacing: fr.spacing,
                corners: &fr.corners,
                blocking: fr.blocking,
                bearing,
            };
            let r = frame_wall(hw, &ctx);
            let key = shape_key_scaled(
                vec![
                    K::S("wall".into()),
                    p2k(hw.a),
                    p2k(hw.b),
                    K::N(hw.t),
                    K::N(hw.height),
                    K::S(hw.type_key.clone()),
                    K::S(hw.ext_side.clone()),
                    K::N(hw.z),
                    K::A(
                        ops.iter()
                            .map(|o| K::A(vec![K::S(o.kind().into()), K::N(o.offset), K::N(o.width), K::N(o.sill), K::N(o.height)]))
                            .collect(),
                    ),
                    K::A(
                        walls
                            .iter()
                            .filter(|x| x.guid != hw.guid)
                            .map(|x| {
                                K::A(vec![
                                    K::S(x.guid.clone()),
                                    p2k(x.a),
                                    p2k(x.b),
                                    K::N(x.t),
                                    K::S(x.type_key.clone()),
                                    K::S(x.ext_side.clone()),
                                ])
                            })
                            .collect(),
                    ),
                    opts_key(fr),
                ],
                10.0,
            );
            let u = [(hw.b[0] - hw.a[0]) / hw.len, (hw.b[1] - hw.a[1]) / hw.len];
            ok(r, key, MapKind::Wall(hw.a, u, [-u[1], u[0]], hw.z))
        }
        Host::Floor(i) => {
            let Some(fl): Option<Floor> = cache.derived.as_ref().and_then(|d| d.floors.get(*i)).cloned() else {
                return err("model.err.gone");
            };
            let Some(st) = storey else { return err("model.err.noroom") };
            let l = storey_loops(w, st)?;
            let lp = if fl.mode == "building" {
                loop_at(&l.loops.outlines, fl.seed).map(|o| o.poly.clone())
            } else {
                loop_at(&l.loops.rooms, fl.seed).map(|o| o.poly.clone())
            };
            let Some(poly) = lp else { return err("model.err.noroom") };
            let openings = floor_openings(w, fl.id)?;
            let o2: Vec<Vec<V2>> = openings
                .iter()
                .map(|op| op.iter().map(|p| [p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN)]).collect())
                .collect();
            let (r, _) = frame_floor(&poly, fr.spacing, fl.offset, &o2);
            let key = shape_key_scaled(
                vec![
                    K::S("floor".into()),
                    kpts(&poly),
                    K::N(fl.offset),
                    K::A(openings.iter().map(|op| K::A(op.iter().map(|p| K::A(p.iter().map(|v| K::N(*v)).collect())).collect())).collect()),
                    opts_key(fr),
                ],
                10.0,
            );
            ok(r, key, MapKind::Plan(storey))
        }
        Host::Roof(i) => {
            let Some(rf) = cache.derived.as_ref().and_then(|d| d.roofs.get(*i)).cloned() else { return err("model.err.gone") };
            let Some(st) = storey else { return err("model.err.nobuilding2") };
            let l = storey_loops(w, st)?;
            let Some(o) = loop_at(&l.loops.outlines, rf.seed) else { return err("model.err.nobuilding2") };
            let poly = o.poly.clone();
            let top = l.walls.iter().fold(0.0, |mx, x| m::max(mx, x.base_offset + x.height));
            let plate = if rf.plate > 0.0 { rf.plate } else { top };
            let kinds = edge_kinds(&poly, &l.walls, &rf.set());
            let r = frame_roof(&poly, &kinds, plate, rf.thickness, fr.spacing, &fr.roof_mode, rf.fascia);
            let kk = K::A(
                kinds
                    .iter()
                    .map(|k| {
                        K::O(vec![
                            ("kind", K::S(k.kind.clone())),
                            ("pitch", k.pitch.map_or(K::Null, K::N)),
                            ("overhang", K::N(k.overhang)),
                            ("wall", k.wall.clone().map_or(K::Null, K::S)),
                        ])
                    })
                    .collect(),
            );
            let key = shape_key_scaled(
                vec![
                    K::S("roof".into()),
                    kpts(&poly),
                    kk,
                    K::N(plate),
                    K::N(rf.thickness),
                    K::N(rf.fascia),
                    K::S(fr.roof_mode.clone()),
                    opts_key(fr),
                ],
                10.0,
            );
            ok(r, key, MapKind::Plan(storey))
        }
    }
}

/// `writeFraming(w, fr, cache)` → Some(true) written, Some(false) the same / an error, None the host is gone.
fn write_framing(w: &mut W<'_>, fr: &Framing, cache: &mut Cache) -> R<bool> {
    let Some((host, storey)) = host_of(w, &fr.host, cache)? else { return Ok(false) };
    if storey.is_none() {
        return Ok(false);
    }
    let made = pieces_for(w, fr, &host, storey, cache)?;
    if made.framed.error.is_some() {
        return Ok(false);
    }
    if fr.key == made.key && !fr.parts.is_empty() {
        return Ok(false);
    }
    // generated pieces go; pieces kept by hand stay, flagged
    let (mut kept, mut gone) = (Vec::new(), Vec::new());
    for &pid in &fr.parts {
        match w.read_pset(pid, LUMBER_PSET)? {
            Some(ps) if !is_true(ps.get("Generated")) => kept.push(pid),
            _ => gone.push(pid),
        }
    }
    if !gone.is_empty() {
        w.drop_product(&gone)?;
    }
    for &pid in &kept {
        w.upsert_pset(pid, LUMBER_PSET, &[row("Check", "IFCLABEL", P::s(&t0("res.fr.check")))])?;
    }
    let Some(fst) = fr.storey else { return Ok(false) };
    let st_lp = w.must(fst)?.a("ObjectPlacement").href();
    let mp = match made.map {
        MapKind::Wall(a, u, n, z) => Map::Wall { a, u, n, z },
        MapKind::Plan(hs) => {
            let a = inv4j(&w.storey_world(fst)?)?;
            let b = match hs {
                Some(h) => w.storey_world(h)?,
                None => [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            };
            Map::Plan(mul4(&a, &b))
        }
    };
    let wood = w.surface_style(&t0("res.style.framing"), [0.86, 0.72, 0.5], 0.0)?;
    let mut parts = kept.clone();
    let mut bf = 0.0;
    let mut names: HashMap<&str, String> = HashMap::new();
    for pc in &made.framed.pieces {
        let (cls, ptype) = cls_of(pc.role);
        let name = names.entry(pc.role).or_insert_with(|| t0(&format!("res.fr.role.{}", pc.role))).clone();
        let nm = w.l(&name);
        let g = w.guid();
        let oh = w.owner_history();
        let lp = w.place_at(st_lp, 0.0, 0.0, 0.0, 0.0)?;
        let ot = if ptype == "USERDEFINED" { nm.clone() } else { JV::Null };
        let mut args = vec![g, oh, nm, JV::Null, ot, JV::H(lp), JV::Null, JV::Null];
        if w.v4 {
            args.push(JV::s(ptype));
        }
        let id = w.make(cls, args)?;
        let it = piece_item(w, pc, &mp)?;
        w.style_item(it, wood)?;
        let rep = w.rep("body", "Body", "SweptSolid", &[it])?;
        w.set_shape(id, &[rep])?;
        let b = board_feet(pc.size, pc.length) * pc.plies;
        bf += b;
        let size = if pc.plies > 1.0 { format!("{} × {}", pc.size, crate::js::num_str(pc.plies)) } else { pc.size.to_string() };
        w.upsert_pset(
            id,
            LUMBER_PSET,
            &[
                row("Role", "IFCLABEL", P::s(pc.role)),
                row("Size", "IFCLABEL", P::s(&size)),
                row("Length", "IFCPOSITIVELENGTHMEASURE", P::Num(round_d(w.u(pc.length), 3))),
                row("BoardFeet", "IFCREAL", P::Num(round_d(b, 3))),
                row("Generated", "IFCBOOLEAN", P::Bool(true)),
            ],
        )?;
        parts.push(id);
    }
    match agg_rel(w, fr.id)? {
        Some(rel) => {
            if !parts.is_empty() {
                let mut x = w.touch(rel)?;
                x.set("RelatedObjects", JV::L(parts.iter().map(|&q| JV::H(q)).collect()));
                w.write(&mut x)?;
            }
        }
        None if !parts.is_empty() => {
            let g = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(fr.id), JV::L(parts.iter().map(|&q| JV::H(q)).collect())])?;
        }
        None => {}
    }
    let notes = if made.framed.notes.is_empty() { "ok".to_string() } else { made.framed.notes.join(",") };
    w.upsert_pset(fr.id, FRAME_PSET, &[row("ShapeKey", "IFCIDENTIFIER", P::s(&made.key)), row("Notes", "IFCLABEL", P::s(&notes))])?;
    w.upsert_pset(
        fr.id,
        "CadCraft_FramingQuantities",
        &[
            row("Pieces", "IFCINTEGER", P::Num(made.framed.pieces.len() as f64)),
            row("BoardFeet", "IFCREAL", P::Num(round_d(bf, 2))),
            row("Size", "IFCLABEL", P::s(made.framed.size)),
        ],
    )?;
    Ok(true)
}

/// `syncFraming(w, storey)`: framings of the level's walls and floors, and of roofs over its walls, made again where
/// they changed; framings whose host is gone go.
pub fn sync_framing(w: &mut W<'_>, storey: Id) -> R<usize> {
    let all = read_framing(w)?;
    if all.is_empty() {
        return Ok(0);
    }
    let mut n = 0usize;
    let mut cache = Cache::default();
    let mut gone: Vec<Id> = Vec::new();
    let mut ngone = 0usize;
    for fr in &all {
        let Some((_, hs)) = host_of(w, &fr.host, &mut cache)? else {
            gone.push(fr.id);
            gone.extend(fr.parts.iter().copied());
            ngone = ngone.saturating_add(1);
            continue;
        };
        if hs != Some(storey) && fr.storey != Some(storey) {
            continue;
        }
        if write_framing(w, fr, &mut cache)? {
            n = n.saturating_add(1);
        }
    }
    if !gone.is_empty() {
        w.drop_product(&gone)?;
        n = n.saturating_add(ngone);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_spans_follow_the_tables() {
        assert_eq!(header_for(3.0 * FT, true), ("2x6", 2.0, false));
        assert_eq!(header_for(9.0 * FT, true), ("2x12", 3.0, true));
        assert_eq!(size_for(&joist_table(406.4), 12.0 * FT), ("2x8", false));
        assert_eq!(size_for(&joist_table(609.6), 30.0 * FT), ("2x12", true));
        assert!((board_feet("2x4", 8.0 * FT) - 16.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_floor_gets_joists_and_rims_and_hostile_shapes_do_not_panic() {
        let o = vec![[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0]];
        let (f, span) = frame_floor(&o, 406.4, 0.0, &[vec![[1000.0, 1000.0], [2000.0, 1000.0], [2000.0, 2000.0], [1000.0, 2000.0]]]);
        assert!(span > 3000.0);
        assert_eq!(f.pieces.iter().filter(|p| p.role == "rim").count(), 4);
        assert!(f.pieces.iter().filter(|p| p.role == "joist").count() > 10);
        let _ = frame_floor(&[], 406.4, 0.0, &[]);
        let _ = frame_floor(&[[f64::NAN, 0.0], [1.0, f64::INFINITY], [0.0, 1.0]], 0.0, f64::NAN, &[]);
    }
}
