//! Dimensions (model.js `Model` § dimensions): linear dimensions between two points, attached to the vertices they
//! were placed on (giving one a new length stretches the geometry: setDimLength), radius / diameter dimensions on
//! round curves (0.4 M5) and angle dimensions.

use crate::def::find_vert;
use crate::error::{Error, Res, msg};
use crate::model::{Dim, End, Model, RadC, Radial};
use crate::ops::{connected_verts, curve_info, set_curve_radius, stretch};
use crate::vec::*;

/// What a dimension end moves: an anchor (path, vertex).
pub type Anchor = (Vec<f64>, f64);

impl Model {
    /// The vertex at world point p as (path, v), preferring one in the open context `prefer`.
    pub fn anchor_at(&self, p: P3, prefer: &[f64]) -> Option<Anchor> {
        let mut best: Option<(f64, Vec<f64>, f64)> = None;
        self.walk("root", &IDENT, &mut |def, m, path, _| {
            for (v, q) in def.verts.iter() {
                if dist(apply(m, *q), p) > EPS * 100.0 {
                    continue;
                }
                let score = (if path == prefer { 0.0 } else { 1000.0 }) + path.len() as f64;
                if best.as_ref().is_none_or(|b| score < b.0) {
                    best = Some((score, path.to_vec(), v));
                }
            }
            true
        });
        best.map(|(_, path, v)| (path, v))
    }
    fn end(&self, p: P3, prefer: &[f64]) -> End {
        End { p, at: self.anchor_at(p, prefer) }
    }
    pub fn add_dim(&mut self, a: P3, b: P3, off: P3, prefer: &[f64]) -> f64 {
        let id = self.new_guide_id();
        let ea = self.end(a, prefer);
        let eb = self.end(b, prefer);
        self.dims.set(id, Dim::Lin { a: ea, b: eb, off, mv: "b".into() });
        id
    }
    /// Where a dimension end is now: on its vertex (re-attached by position if the vertex was merged or regrouped),
    /// else its last place.
    pub fn end_at(&self, e: &mut End) -> P3 {
        for _ in 0..3 {
            let Some((path, v)) = e.at.clone() else { return e.p };
            let ctx = self.context(&path);
            let mut q = ctx.as_ref().and_then(|c| self.def(&c.def)).and_then(|d| d.verts.get(v)).copied();
            if q.is_none()
                && let Some(c) = &ctx
                && let Some(d) = self.def(&c.def)
                && let Some(nv) = find_vert(d, apply(&c.inv, e.p), EPS * 100.0)
            {
                e.at = Some((path.clone(), nv));
                q = d.verts.get(nv).copied();
            }
            if let (Some(q), Some(c)) = (q, &ctx) {
                return apply(&c.m, q);
            }
            match self.anchor_at(e.p, &[]) {
                Some(a) => e.at = Some(a),
                None => {
                    e.at = None;
                    return e.p;
                }
            }
        }
        e.p
    }
    pub fn dim_ends(&self, dm: &mut Dim) -> Vec<P3> {
        match dm {
            Dim::Ang { o, a, b, .. } => vec![self.end_at(o), self.end_at(a), self.end_at(b)],
            Dim::Lin { a, b, .. } => vec![self.end_at(a), self.end_at(b)],
            Dim::Rad { .. } => Vec::new(),
        }
    }
    /// A radius ('rad') or diameter ('dia') dimension on round curve cid of the context `path`.
    pub fn add_radial_dim(&mut self, path: &[f64], cid: &str, off: P3, kind: &str) -> Res<f64> {
        let mut c = RadC { p: None, r: f64::NAN, n: None, at: Some((path.to_vec(), cid.to_string())) };
        let g = self.radial_at(&mut c);
        if !g.is_some_and(|g| g.live) {
            return Err(msg("m3d.model.dim_not_round"));
        }
        let id = self.new_guide_id();
        self.dims.set(id, Dim::Rad { kind: if kind == "dia" { "dia".into() } else { "rad".into() }, c, off });
        Ok(id)
    }
    /// The angle at corner o between the lines to a and b.
    pub fn add_angle_dim(&mut self, o: P3, a: P3, b: P3, off: P3, prefer: &[f64]) -> f64 {
        let id = self.new_guide_id();
        let eo = self.end(o, prefer);
        let ea = self.end(a, prefer);
        let eb = self.end(b, prefer);
        self.dims.set(id, Dim::Ang { o: eo, a: ea, b: eb, off });
        id
    }
    /// Where a radial dimension's curve is now; its last known place when the curve is gone (live false); None if never.
    pub fn radial_at(&self, c: &mut RadC) -> Option<Radial> {
        let at = |m: &M4, d: &crate::def::Def, ci: &crate::ops::CurveInfo| -> Radial {
            let cc = apply(m, ci.center.unwrap_or([f64::NAN; 3]));
            let v0 = ci.verts.first().map_or([f64::NAN; 3], |&v| d.v(v));
            let r = dist(cc, apply(m, v0));
            Radial { c: cc, r, n: Some(norm(apply_dir(m, ci.normal.unwrap_or([f64::NAN; 3])))), live: true, flat: ci.flat, arc: !ci.closed }
        };
        for _ in 0..3 {
            let Some((path, curve)) = c.at.clone() else { break };
            let ctx = self.context(&path);
            let d = ctx.as_ref().and_then(|x| self.def(&x.def));
            if let (Some(ctx), Some(d)) = (&ctx, d)
                && let Some(ci) = curve_info(d, &curve).filter(|ci| ci.round)
            {
                let g = at(&ctx.m, d, &ci);
                c.p = Some(g.c);
                c.r = g.r;
                c.n = g.n;
                return Some(g);
            }
            if let Some(cp) = c.p {
                let mut found: Option<(Vec<f64>, String)> = None;
                self.walk("root", &IDENT, &mut |def, m, path, _| {
                    if found.is_some() {
                        return true;
                    }
                    let mut seen: Vec<&str> = Vec::new();
                    for e in def.edges.values() {
                        let Some(cid) = e.curve.as_deref().filter(|s| !s.is_empty()) else { continue };
                        if seen.contains(&cid) {
                            continue;
                        }
                        seen.push(cid);
                        let Some(ci2) = curve_info(def, cid).filter(|ci| ci.round) else { continue };
                        let g = at(m, def, &ci2);
                        if dist(g.c, cp) <= EPS * 100.0 && (g.r - c.r).abs() <= EPS * 100.0 {
                            found = Some((path.to_vec(), cid.to_string()));
                            return true;
                        }
                    }
                    true
                });
                if let Some(f) = found {
                    c.at = Some(f);
                    continue;
                }
            }
            c.at = None;
            break;
        }
        c.p.map(|p| Radial { c: p, r: c.r, n: c.n, live: false, flat: false, arc: false })
    }
    /// Give a dimension a new length by moving the geometry at its moving end.
    pub fn set_dim_length(&mut self, id: f64, l: f64) -> Res<f64> {
        let Some(mut dm) = self.dims.get(id).cloned() else { return Err(msg("m3d.model.the_dimension_is_gone")) };
        let r = self.set_dim_length_of(&mut dm, l);
        // the dimension's ends were brought up to date on the way, also when it failed
        self.dims.set(id, dm);
        r
    }
    fn set_dim_length_of(&mut self, dm: &mut Dim, l: f64) -> Res<f64> {
        match dm {
            Dim::Ang { .. } => Err(msg("m3d.model.dim_angle_only")),
            Dim::Rad { kind, c, .. } => {
                let g = self.radial_at(c);
                let Some(g) = g.filter(|g| g.live) else { return Err(msg("m3d.model.dim_curve_gone")) };
                let want = if kind == "dia" { l / 2.0 } else { l };
                if !(want > EPS) {
                    return Err(msg("m3d.model.a_length_must_be_above_zero"));
                }
                let Some((path, curve)) = c.at.clone() else { return Err(Error::Type) };
                let Some(ctx) = self.context(&path) else { return Err(Error::Type) };
                let Some(def) = self.def_mut(&ctx.def) else { return Err(Error::Type) };
                let Some(ci) = curve_info(def, &curve) else { return Err(Error::Type) };
                set_curve_radius(def, &curve, ci.radius * want / g.r)?;
                self.radial_at(c);
                Ok(want - g.r)
            }
            Dim::Lin { a, b, mv, .. } => {
                let pa = self.end_at(a);
                let pb = self.end_at(b);
                let ea = a.at.clone();
                let eb = b.at.clone();
                let r = self.resize_between(ea.as_ref(), eb.as_ref(), pa, pb, l, mv);
                let r = r?;
                a.p = self.end_at(a);
                b.p = self.end_at(b);
                Ok(r)
            }
        }
    }
    /// Make the distance between ends A and B (anchors or None, now at world pa, pb) L, moving 'b', 'a' or 'both'.
    pub fn resize_between(&mut self, a: Option<&Anchor>, b: Option<&Anchor>, pa: P3, pb: P3, l: f64, mv: &str) -> Res<f64> {
        if !(l > EPS) {
            return Err(msg("m3d.model.a_length_must_be_above_zero"));
        }
        let cur = dist(pa, pb);
        if cur <= EPS {
            return Err(msg("m3d.model.the_two_ends_are_at_the"));
        }
        let d = norm(sub(pb, pa));
        let delta = l - cur;
        if delta.abs() <= EPS {
            return Ok(0.0);
        }
        if mv == "both" {
            self.move_end(b, a, d, delta / 2.0)?;
            self.move_end(a, b, mul(d, -1.0), delta / 2.0)?;
        } else if mv == "a" {
            self.move_end(a, b, mul(d, -1.0), delta)?;
        } else {
            self.move_end(b, a, d, delta)?;
        }
        Ok(delta)
    }
    /// Move end E (an anchor) by delta along world direction d, away from the fixed end F.
    pub fn move_end(&mut self, e: Option<&Anchor>, f: Option<&Anchor>, d: P3, delta: f64) -> Res<()> {
        let Some((epath, ev)) = e else { return Err(msg("m3d.model.the_end_that_moves_is_not")) };
        let on = self.context(epath).and_then(|c| self.def(&c.def).map(|dd| dd.verts.has(*ev))).unwrap_or(false);
        if !on {
            return Err(msg("m3d.model.the_end_that_moves_is_not"));
        }
        let mut k = 0usize;
        if let Some((fpath, _)) = f {
            while k < epath.len() && k < fpath.len() && epath.get(k) == fpath.get(k) {
                k += 1;
            }
        }
        let p: Vec<f64> = if f.is_some() { epath.get(..k).unwrap_or(&[]).to_vec() } else { epath.clone() };
        let Some(ctx) = self.context(&p) else { return Err(Error::Type) };
        let ld = apply_dir(&ctx.inv, d);
        let s = len(ld);
        let dl = norm(ld);
        if epath.len() > p.len() {
            // the moving end is inside a group or component the fixed end is not in: the whole group moves
            let Some(&iid) = epath.get(p.len()) else { return Err(Error::Type) };
            let Some(inst) = self.def_mut(&ctx.def).and_then(|dd| dd.insts.get_mut(iid)) else { return Err(Error::Type) };
            inst.m = mmul(&translation(mul(dl, delta * s)), &inst.m);
            return Ok(());
        }
        // the same definition: stretch the moving end's piece at its end; when the fixed end is on another piece (or
        // inside a group), the piece moves whole
        let Some(def) = self.def(&ctx.def) else { return Err(Error::Type) };
        let piece = connected_verts(def, *ev);
        let fixed_here = f.is_some_and(|(fpath, fv)| fpath.len() == p.len() && piece.has(*fv));
        let cut = if fixed_here { dot(def.v(*ev), dl) } else { f64::NEG_INFINITY };
        let boxes = self.inst_boxes(&ctx.def);
        let Some(def) = self.def_mut(&ctx.def) else { return Err(Error::Type) };
        stretch(def, dl, cut, delta * s, Some(&piece), &|id| boxes.iter().find(|(k, _)| k == id).and_then(|(_, b)| *b))?;
        Ok(())
    }
    /// Each placed definition's own box (what stretch's boxOf asks for), for the placements of def.
    fn inst_boxes(&self, def: &str) -> Vec<(String, Option<BBox>)> {
        let mut out: Vec<(String, Option<BBox>)> = Vec::new();
        if let Some(d) = self.def(def) {
            for i in d.insts.values() {
                if !out.iter().any(|(k, _)| *k == i.def) {
                    out.push((i.def.clone(), self.bbox(&i.def, &IDENT)));
                }
            }
        }
        out
    }
    /// An edge of the open context `path` to length L; the end further along red, green, then blue moves.
    pub fn set_edge_length(&mut self, path: &[f64], eid: f64, l: f64) -> Res<f64> {
        let ctx = self.context(path);
        let e = ctx.as_ref().and_then(|c| self.def(&c.def)).and_then(|d| d.edges.get(eid)).cloned();
        let (Some(ctx), Some(e)) = (ctx, e) else { return Err(msg("m3d.model.the_edge_is_gone")) };
        let Some(d) = self.def(&ctx.def) else { return Err(Error::Type) };
        let pa = apply(&ctx.m, d.v(e.a));
        let pb = apply(&ctx.m, d.v(e.b));
        let dv = sub(pb, pa);
        let i = [0usize, 1, 2].into_iter().fold(0usize, |m, j| if at3(&dv, j).abs() > at3(&dv, m).abs() + EPS { j } else { m });
        let (aa, bb, qa, qb) = if at3(&dv, i) >= 0.0 { (e.a, e.b, pa, pb) } else { (e.b, e.a, pb, pa) };
        let a = (path.to_vec(), aa);
        let b = (path.to_vec(), bb);
        self.resize_between(Some(&a), Some(&b), qa, qb, l, "b")
    }
    /// A group's or component's size along its own red (0), green (1) or blue (2) axis: its far side moves.
    pub fn set_def_size(&mut self, def_id: &str, axis: usize, size: f64) -> Res<()> {
        let bb = if self.def(def_id).is_some() { self.bbox(def_id, &IDENT) } else { None };
        let Some(bb) = bb else { return Err(msg("m3d.model.it_is_empty")) };
        if !(size > EPS) {
            return Err(msg("m3d.model.a_size_must_be_above_zero"));
        }
        let (Some(&hi), Some(&lo)) = (bb.max.get(axis), bb.min.get(axis)) else { return Err(Error::Type) };
        let cur = hi - lo;
        if cur <= EPS {
            return Err(msg("m3d.model.it_is_flat_that_way_push"));
        }
        let mut d = [0.0, 0.0, 0.0];
        if let Some(x) = d.get_mut(axis) {
            *x = 1.0;
        }
        let boxes = self.inst_boxes(def_id);
        let Some(def) = self.def_mut(def_id) else { return Err(Error::Type) };
        stretch(def, d, hi, size - cur, None, &|id| boxes.iter().find(|(k, _)| k == id).and_then(|(_, b)| *b))?;
        Ok(())
    }
}
