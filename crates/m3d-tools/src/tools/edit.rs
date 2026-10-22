//! The editing tools: Follow me, Offset, Paint, Push/pull and Eraser (tools.js), with Push/pull's and Move's colliders.

use std::rc::Rc;

use cc_m3d::def::{face_normal, key as ekey};
use cc_m3d::edges::{EdgeOpts, add_edge, remove_loose_verts};
use cc_m3d::offset::{offset_chain_pts, offset_face, offset_face_pts};
use cc_m3d::omap::{IdSet, OMap};
use cc_m3d::ops::{Sel, chain_through, connected, erase_edges, face_with_edges, follow_me, push_pull, tag_curve};
use cc_m3d::paint::{paint_faces, paint_instance};
use cc_m3d::vec::{EPS, P3, Plane, add, dot, len, mul, newell, norm, ray_plane, ray_tri, sub};
use cc_m3d::vec::{frame, to2};
use cc_m3d::{Error, js::num_str};
use serde_json::Value as Json;

use super::{Base, ClickTool, Ev, Micro, Res, Ret, SnapOpts, ToolImpl, Tools, err_message, same_path};
use crate::collide::Collider;
use crate::i18n::{n as nstr, t as tr, t0};
use crate::infer::{along_line, tip as tips};
use crate::js::{hypot2, jmax, jmin, sign, truthy};
use crate::overlay::{Line, Overlay};
use crate::scene::{ObjRef, SFace, Scene};
use crate::units::Value;

fn ids_of(sel: &[Sel], kind: &str) -> Vec<f64> {
    sel.iter().filter(|s| s.t == kind).map(|s| s.id).collect()
}

// ---- follow me (0.4 M10) ----

#[derive(Clone, Debug)]
pub struct FollowTool {
    pub b: Base,
    path: Vec<f64>,
    face: Option<f64>,
}
impl FollowTool {
    pub fn new(t: &mut Tools, name: &str) -> FollowTool {
        let c = t.ctx();
        let path: Vec<f64> = match t.host.model.def(&c.def) {
            Some(def) => t.host.sel.iter().filter(|x| x.t == "e" && def.edges.has(x.id)).map(|x| x.id).collect(),
            None => Vec::new(),
        };
        let faces = ids_of(&t.host.sel, "f");
        if !path.is_empty()
            && let [f] = faces.as_slice()
        {
            t.micro.push(Micro::FollowRun(*f, path.clone()));
        }
        FollowTool { b: Base::new(name), path, face: None }
    }
    fn run(&mut self, t: &mut Tools, fid: f64, path_eids: &[f64]) -> Res<()> {
        let c = t.ctx();
        let r = t.host.model.change(&t0("m3d.tool.follow_me"), |m| {
            let d = m.def_mut(&c.def).ok_or(Error::Type)?;
            follow_me(d, fid, path_eids)
        });
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                t.host.print(tr("m3d.tool.follow_failed", &[("error", &err_message(&e))]));
                self.face = None;
                self.b.busy = false;
                return Ok(());
            }
        };
        let key = if r.closed { "m3d.tool.follow_ring" } else { "m3d.tool.follow_done" };
        t.host.print(tr(key, &[("n", &nstr(path_eids.len() as f64)), ("faces", &nstr(r.faces.len() as f64))]));
        t.host.set_sel(Vec::new());
        self.path = Vec::new();
        self.face = None;
        self.b.busy = false;
        t.host.changed();
        t.host.overlay(None);
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for FollowTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        if !self.path.is_empty() {
            t0("m3d.tool.click_the_face_to_sweep_along")
        } else if self.face.is_some() {
            t0("m3d.tool.now_click_an_edge_of_the")
        } else {
            t0("m3d.tool.click_the_face_to_sweep_then")
        }
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let s = t.pick_at(ev);
        let want = if !self.path.is_empty() || self.face.is_none() { "f" } else { "e" };
        t.host.overlay(s.filter(|s| s.t == want).map(|s| Overlay { hover: Some(Some(s)), ..Overlay::default() }));
        Ok(())
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let s = t.pick_at(ev);
        let c = t.ctx();
        let Some(face) = self.face.filter(|_| self.path.is_empty()) else {
            let Some(s) = s.filter(|s| s.t == "f") else {
                t.host.print(t0("m3d.tool.click_a_face_inside_the_open"));
                return Ok(());
            };
            if !self.path.is_empty() {
                let p = self.path.clone();
                return self.run(t, s.id, &p);
            }
            self.face = Some(s.id);
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        };
        let Some(s) = s.filter(|s| s.t == "e") else {
            t.host.print(t0("m3d.tool.click_an_edge_of_the_path"));
            return Ok(());
        };
        let def = t.host.model.def(&c.def).ok_or(Error::Type)?;
        let mut own = std::collections::HashSet::new();
        if let Some(f) = def.faces.get(face) {
            for l in &f.loops {
                for (i, v) in l.iter().enumerate() {
                    let w = l.get((i + 1) % l.len()).copied().unwrap_or(f64::NAN);
                    own.insert(ekey(*v, w));
                }
            }
        }
        let e = def.edges.get(s.id).ok_or(Error::Type)?;
        if own.contains(&ekey(e.a, e.b)) {
            t.host.print(t0("m3d.tool.that_edge_is_part_of_the"));
            return Ok(());
        }
        let chain = chain_through(def, s.id);
        self.run(t, face, &chain)
    }
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.face = None;
        self.b.busy = false;
        Ok(())
    }
    fn call(&mut self, t: &mut Tools, name: &str, args: &[Json]) -> Res<Option<Json>> {
        if name != "run" {
            return Ok(None);
        }
        let fid = args.first().and_then(Json::as_f64).unwrap_or(f64::NAN);
        let path: Vec<f64> =
            args.get(1).and_then(Json::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
        self.run(t, fid, &path)?;
        Ok(Some(Json::Null))
    }
    fn state(&self) -> Json {
        serde_json::json!({"path": crate::outj::nums(&self.path), "face": crate::outj::opt_num(self.face)})
    }
}

// ---- offset (0.4 M4) ----

/// Distance from 2D point p to segment ab.
fn seg_d2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let l = dx * dx + dy * dy;
    let tt = if truthy(l) { jmax(0.0, jmin(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l)) } else { 0.0 };
    hypot2(p[0] - a[0] - dx * tt, p[1] - a[1] - dy * tt)
}
fn in_loop2(p: [f64; 2], l: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = l.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (l.get(i), l.get(j))
            && (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
        j = i;
    }
    c
}

#[derive(Clone, Debug)]
pub struct OffsetTool {
    pub b: Base,
    fid: Option<f64>,
    chain: Option<Vec<f64>>,
    d: f64,
    n: P3,
    fr: Option<cc_m3d::vec::Frame>,
    loops2: Vec<Vec<[f64; 2]>>,
    h0: f64,
    base: Option<(Vec<P3>, bool)>,
}
impl OffsetTool {
    pub fn new(t: &mut Tools, name: &str) -> Res<OffsetTool> {
        let mut o = OffsetTool {
            b: Base::new(name),
            fid: None,
            chain: None,
            d: 0.0,
            n: [f64::NAN; 3],
            fr: None,
            loops2: Vec::new(),
            h0: f64::NAN,
            base: None,
        };
        o.reset_(t);
        o.start_from_sel(t)?;
        Ok(o)
    }
    fn reset_(&mut self, _t: &mut Tools) {
        self.fid = None;
        self.chain = None;
        self.d = 0.0;
        self.b.busy = false;
    }
    fn active(&self) -> bool {
        self.fid.is_some() || self.chain.is_some()
    }
    fn start_from_sel(&mut self, t: &mut Tools) -> Res<()> {
        let c = t.ctx();
        let faces = ids_of(&t.host.sel, "f");
        let edges = ids_of(&t.host.sel, "e");
        if let [f] = faces.as_slice() {
            self.start_face(t, *f);
        } else if faces.is_empty() && !edges.is_empty() && t.host.model.def(&c.def).is_some_and(|d| edges.iter().all(|e| d.edges.has(*e))) {
            self.start_chain(t, &edges)?;
        }
        Ok(())
    }
    fn start_face(&mut self, t: &mut Tools, fid: f64) {
        let c = t.ctx();
        let Some(def) = t.host.model.def(&c.def) else { return };
        let Some(f) = def.faces.get(fid) else { return };
        self.fid = Some(fid);
        self.chain = None;
        self.b.busy = true;
        self.n = face_normal(def, f);
        let fr = frame(self.n);
        self.loops2 = f.loops.iter().map(|l| l.iter().map(|v| to2(&fr, def.v(*v))).collect()).collect();
        self.fr = Some(fr);
        self.h0 = dot(f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |v| def.v(*v)), self.n);
        let h = self.hint(t);
        t.host.tip(h);
    }
    fn start_chain(&mut self, t: &mut Tools, eids: &[f64]) -> Res<()> {
        let c = t.ctx();
        let def = t.host.model.def(&c.def).ok_or(Error::Type)?;
        let mut vs = IdSet::new();
        for id in eids {
            let e = def.edges.get(*id).ok_or(Error::Type)?;
            vs.add(e.a);
            vs.add(e.b);
        }
        let pts: Vec<P3> = vs.iter().map(|v| def.v(v)).collect();
        let mut n = newell(&pts);
        if len(n) < 1e-9 {
            n = t.local_dir(t.axes().z);
        }
        self.n = norm(n);
        let fr = frame(self.n);
        self.fr = Some(fr);
        self.h0 = dot(pts.first().copied().ok_or(Error::Type)?, self.n);
        let def = t.host.model.def(&c.def).ok_or(Error::Type)?;
        match offset_chain_pts(def, eids, 0.0, self.n) {
            Ok(r) => self.base = Some((r.pts, r.closed)),
            Err(e) => {
                t.host.print(err_message(&e));
                return Ok(());
            }
        }
        self.chain = Some(eids.to_vec());
        self.fid = None;
        self.b.busy = true;
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
    /// the pointer on the plane, in 2D of the outline's frame
    fn where_(&self, t: &Tools, ev: &Ev) -> Option<[f64; 2]> {
        let ray = t.host.cam.ray(ev.x, ev.y);
        let o = t.local(ray.o);
        let d = t.local_dir(ray.d);
        let (_, gp) = ray_plane(o, d, &Plane { n: self.n, d: self.h0 })?;
        self.fr.as_ref().map(|fr| to2(fr, gp))
    }
    fn preview(&self, t: &Tools, d: f64) -> Res<Vec<Line>> {
        let c = t.ctx();
        let def = t.host.model.def(&c.def).ok_or(Error::Type)?;
        let loops: Vec<(Vec<P3>, bool)> = if let Some(fid) = self.fid {
            offset_face_pts(def, fid, d)?.into_iter().map(|p| (p, true)).collect()
        } else {
            let r = offset_chain_pts(def, self.chain.as_deref().unwrap_or(&[]), d, self.n)?;
            vec![(r.pts, r.closed)]
        };
        let mut lines = Vec::new();
        for (pp, closed) in &loops {
            let n = pp.len();
            let mut i = 0usize;
            while i + 1 < n + usize::from(*closed) {
                let a = pp.get(i).copied().unwrap_or([f64::NAN; 3]);
                let b = pp.get((i + 1) % n).copied().unwrap_or([f64::NAN; 3]);
                lines.push(Line::new(t.world(a), t.world(b), "edge"));
                i += 1;
            }
        }
        Ok(lines)
    }
    fn make(&mut self, t: &mut Tools, d: f64) -> Res<()> {
        let c = t.ctx();
        let r = if let Some(fid) = self.fid {
            t.host.model.change(&t0("m3d.undo.offset"), |m| {
                let def = m.def_mut(&c.def).ok_or(Error::Type)?;
                offset_face(def, fid, d).map(|_| ())
            })
        } else {
            let chain = self.chain.clone().unwrap_or_default();
            let n = self.n;
            t.host.model.change(&t0("m3d.undo.offset"), |m| {
                let def = m.def_mut(&c.def).ok_or(Error::Type)?;
                let r = offset_chain_pts(def, &chain, d, n)?;
                let pp = r.pts;
                let mut kinds: Vec<String> = Vec::new();
                for id in &chain {
                    if let Some(cv) = def.edges.get(*id).and_then(|e| e.curve.clone()).filter(|c| !c.is_empty())
                        && !kinds.contains(&cv)
                    {
                        kinds.push(cv);
                    }
                }
                let k = pp.len();
                let mut i = 0usize;
                while i + 1 < k + usize::from(r.closed) {
                    let a = pp.get(i).copied().unwrap_or([f64::NAN; 3]);
                    let b = pp.get((i + 1) % k).copied().unwrap_or([f64::NAN; 3]);
                    add_edge(def, a, b, EdgeOpts { no_faces: false, hint: Some(n) });
                    i += 1;
                }
                if kinds.len() == 1
                    && chain.iter().all(|id| def.edges.get(*id).is_some_and(|e| e.curve.as_deref().is_some_and(|c| !c.is_empty())))
                {
                    let kind = kinds.first().map(|s| s.split(':').next().unwrap_or("").to_string()).unwrap_or_default();
                    tag_curve(def, &pp, &kind, r.closed);
                }
                Ok(())
            })
        };
        if let Err(e) = r {
            t.host.print(tr("m3d.tool.offset_failed", &[("error", &err_message(&e))]));
            return Ok(());
        }
        t.last_offset = Some(d);
        let msg = format!(
            "Offset {}{}",
            t.host.fmt(d.abs()),
            if self.fid.is_some() { if d > 0.0 { " inward." } else { " outward." } } else { "." }
        );
        t.host.print(msg);
        t.host.set_sel(Vec::new());
        self.reset_(t);
        t.host.changed();
        t.host.overlay(None);
        t.host.vcb(t0("m3d.vcb.distance"), String::new());
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for OffsetTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        if self.active() {
            "Move into or out of the outline, click, or type the distance (into a face = inward).".into()
        } else {
            t0("m3d.tool.click_a_face_to_offset_its")
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.distance")
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if !self.active() {
            let s = t.pick_at(ev);
            let Some(s) = s.filter(|s| s.t == "f") else {
                t.host.print(t0("m3d.tool.click_a_face_inside_the_open_2"));
                return Ok(());
            };
            self.start_face(t, s.id);
            if ev.count >= 2.0
                && let Some(lo) = t.last_offset.filter(|x| truthy(*x))
            {
                return self.make(t, lo);
            }
            return Ok(());
        }
        if ev.count >= 2.0
            && self.fid.is_some()
            && let Some(lo) = t.last_offset.filter(|x| truthy(*x))
        {
            return self.make(t, lo);
        }
        self.hover(t, ev)?;
        if self.d.abs() > EPS {
            let d = self.d;
            self.make(t, d)?;
        }
        Ok(())
    }
    fn up(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        Ok(())
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.hover(t, ev)
    }
    fn relock(&mut self, t: &mut Tools) -> Res<()> {
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(())
    }
    fn reset(&mut self, t: &mut Tools) -> bool {
        self.reset_(t);
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset_(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        if !self.active() || !truthy(*vv) {
            return Ok(Ret::False);
        }
        let sg = if *vv < 0.0 { -1.0 } else { 1.0 };
        self.make(t, sg * vv.abs() * if self.d < 0.0 { -1.0 } else { 1.0 })?;
        Ok(Ret::True)
    }
    fn call(&mut self, t: &mut Tools, name: &str, _args: &[Json]) -> Res<Option<Json>> {
        if name != "finish" {
            return Ok(None);
        }
        if !self.active() || !(self.d.abs() > EPS) {
            return Ok(Some(Json::Bool(false)));
        }
        let d = self.d;
        self.make(t, d)?;
        Ok(Some(Json::Bool(true)))
    }
    fn state(&self) -> Json {
        use crate::outj::{num, opt_num};
        serde_json::json!({"fid": opt_num(self.fid), "chain": self.chain.as_ref().map_or(Json::Null, |c| crate::outj::nums(c)), "d": num(self.d)})
    }
}
impl ClickTool for OffsetTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        if !self.active() {
            let s = t.pick_at(ev);
            t.host.overlay(s.filter(|s| s.t == "f").map(|s| Overlay { hover: Some(Some(s)), ..Overlay::default() }));
            return Ok(());
        }
        let Some(p) = self.where_(t, ev) else { return Ok(()) };
        let d = if self.fid.is_some() {
            let mut m = f64::INFINITY;
            for l in &self.loops2 {
                let n = l.len();
                for i in 0..n {
                    if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) {
                        m = jmin(m, seg_d2(p, *a, *b));
                    }
                }
            }
            let inside = self.loops2.first().is_some_and(|l| in_loop2(p, l)) && !self.loops2.iter().skip(1).any(|l| in_loop2(p, l));
            if inside { m } else { -m }
        } else {
            let (bp, closed) = self.base.clone().unwrap_or_default();
            let fr = self.fr.ok_or(Error::Type)?;
            let pp: Vec<[f64; 2]> = bp.iter().map(|q| to2(&fr, *q)).collect();
            let (mut m, mut side) = (f64::INFINITY, 1.0);
            let n = pp.len();
            let mut i = 0usize;
            while i + 1 < n + usize::from(closed) {
                if let (Some(a), Some(b)) = (pp.get(i), pp.get((i + 1) % n)) {
                    let dd = seg_d2(p, *a, *b);
                    if dd < m {
                        m = dd;
                        side = crate::js::or(sign((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])), 1.0);
                    }
                }
                i += 1;
            }
            side * m
        };
        self.d = d;
        match self.preview(t, d) {
            Ok(lines) => {
                let suffix = if d >= 0.0 {
                    if self.fid.is_some() { " inward" } else { "" }
                } else if self.fid.is_some() {
                    " outward"
                } else {
                    ""
                };
                let tip = format!("{}{}", t.host.fmt(d.abs()), suffix);
                t.host.overlay(Some(Overlay { lines: Some(lines), tip: Some(tip), ..Overlay::default() }));
            }
            Err(e) => t.host.overlay(Some(Overlay { tip: Some(err_message(&e)), ..Overlay::default() })),
        }
        let txt = t.host.fmt(d.abs());
        t.host.vcb(t0("m3d.vcb.distance"), txt);
        Ok(())
    }
    fn pick_point(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        Ok(())
    }
}

// ---- paint (0.4 M8) ----

#[derive(Clone, Debug)]
pub struct PaintTool {
    pub b: Base,
}
impl PaintTool {
    pub fn new(name: &str) -> PaintTool {
        PaintTool { b: Base::new(name) }
    }
}
impl ToolImpl for PaintTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        t0("m3d.tool.click_a_face_group_or_component")
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let s = t.pick_at(ev);
        t.host.overlay(s.filter(|s| s.t == "f" || s.t == "i").map(|s| Overlay { hover: Some(Some(s)), ..Overlay::default() }));
        Ok(())
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let s = t.pick_at(ev);
        let Some(s) = s.filter(|s| s.t == "f" || s.t == "i") else {
            t.host.print(t0("m3d.tool.click_a_face_a_group_or"));
            return Ok(());
        };
        let c = t.ctx();
        let col = t.paint_color.clone();
        let def = t.host.model.def(&c.def).ok_or(Error::Type)?;
        let col_of = |x: &Option<String>| x.clone().filter(|c| !c.is_empty());
        if ev.alt {
            let picked = if s.t == "f" {
                def.faces
                    .get(s.id)
                    .and_then(|f| col_of(&f.color))
                    .or_else(|| t.host.model.path_color(&t.host.path).filter(|c| !c.is_empty()))
            } else {
                def.insts.get(s.id).and_then(|i| col_of(&i.color))
            };
            t.paint_color = picked.clone();
            t.host.paint_picked = Some(t.paint_color.clone());
            let msg = match &picked {
                Some(p) => tr("m3d.tool.picked_up", &[("color", p)]),
                None => t0("m3d.tool.picked_up_the_default_color"),
            };
            t.host.print(msg);
            return Ok(());
        }
        let in_sel = t.host.sel.iter().any(|x| x.t == s.t && x.id == s.id);
        let (mut fids, mut iids) = (Vec::new(), Vec::new());
        if in_sel {
            fids = ids_of(&t.host.sel, "f");
            iids = ids_of(&t.host.sel, "i");
        } else if s.t == "i" {
            iids = vec![s.id];
        } else {
            let col0 = def.faces.get(s.id).and_then(|f| col_of(&f.color));
            let same = |fc: &Option<String>| col_of(fc) == col0;
            if ev.shift {
                fids = def.faces.iter().filter(|(_, f)| same(&f.color)).map(|(id, _)| id).collect();
            } else if ev.ctrl {
                fids = connected(def, std::slice::from_ref(&s))
                    .into_iter()
                    .filter(|x| x.t == "f" && def.faces.get(x.id).is_some_and(|f| same(&f.color)))
                    .map(|x| x.id)
                    .collect();
            } else {
                fids = vec![s.id];
            }
        }
        let (fl, il) = (fids.clone(), iids.clone());
        let colr = col.clone();
        t.host.model.change(&t0("m3d.undo.paint"), |m| {
            let d = m.def_mut(&c.def).ok_or(Error::Type)?;
            paint_faces(d, &fl, colr.as_deref())?;
            for id in &il {
                paint_instance(d, *id, colr.as_deref())?;
            }
            Ok(())
        })?;
        t.host.changed();
        let what = if !fids.is_empty() && !iids.is_empty() {
            tr("m3d.tool.paint_both", &[("faces", &nstr(fids.len() as f64)), ("groups", &nstr(iids.len() as f64))])
        } else if !fids.is_empty() {
            tr("m3d.tool.paint_faces", &[("n", &nstr(fids.len() as f64))])
        } else {
            tr("m3d.tool.paint_groups", &[("n", &nstr(iids.len() as f64))])
        };
        let msg = match col.filter(|c| !c.is_empty()) {
            Some(c) => tr("m3d.tool.painted", &[("what", &what), ("color", &c)]),
            None => tr("m3d.tool.painted_default", &[("what", &what)]),
        };
        t.host.print(msg);
        Ok(())
    }
}

// ---- push/pull ----

#[derive(Debug)]
struct Amend {
    step: Option<u64>,
    fid: f64,
    k: f64,
    sign: f64,
    coll: Option<Rc<std::cell::RefCell<Collider>>>,
    n: P3,
    path: Vec<f64>,
}

#[derive(Debug)]
pub struct PushTool {
    pub b: Base,
    fid: Option<f64>,
    d: f64,
    last: Option<f64>,
    f0: Option<Rc<SFace>>,
    n: P3,
    p0: P3,
    k: f64,
    sc0: Option<Rc<Scene>>,
    coll: Option<Rc<std::cell::RefCell<Collider>>>,
    stopped: bool,
    amend: Option<Amend>,
}
impl PushTool {
    /// The face being pushed or pulled (`tool.fid`).
    pub fn fid(&self) -> Option<f64> {
        self.fid
    }
    pub fn new(name: &str) -> PushTool {
        PushTool {
            b: Base::new(name),
            fid: None,
            d: 0.0,
            last: None,
            f0: None,
            n: [f64::NAN; 3],
            p0: [f64::NAN; 3],
            k: 1.0,
            sc0: None,
            coll: None,
            stopped: false,
            amend: None,
        }
    }
    fn reset_(&mut self) {
        self.fid = None;
        self.b.busy = false;
        self.d = 0.0;
    }
    /// Does the pushed face, where it was, lie in front of point p under the cursor?
    fn hides(&self, t: &Tools, ev: &Ev, p: P3) -> bool {
        let ray = t.host.cam.ray(ev.x, ev.y);
        let tp = dot(sub(p, ray.o), ray.d) / dot(ray.d, ray.d);
        self.f0.as_ref().is_some_and(|f| {
            f.tris.iter().any(|[a, b, c]| ray_tri(ray.o, ray.d, *a, *b, *c).is_some_and(|s| s < tp - 1e-9 * jmax(1.0, tp)))
        })
    }
    fn apply(&mut self, t: &mut Tools, d0: f64) -> Res<()> {
        let k = self.k;
        let mut d = d0;
        self.stopped = false;
        if let Some(c) = &self.coll
            && d.abs() > EPS
        {
            let (delta, hit) = c.borrow_mut().clamp(mul(self.n, d));
            if hit {
                d = sign(d) * len(delta);
                self.stopped = true;
            }
        }
        self.d = d;
        let path = t.host.path.clone();
        let fid = self.fid.unwrap_or(f64::NAN);
        let scope = t.scope();
        let r = t.host.model.preview(scope.as_deref(), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            Ok(push_pull(def, fid, d * k))
        })?;
        if let Some(r) = r
            && r.ok
        {
            self.d = r.distance / k;
        }
        t.host.changed();
        let txt = t.host.fmt(self.d.abs());
        t.host.vcb(t0("m3d.vcb.distance"), txt);
        Ok(())
    }
    fn finish(&mut self, t: &mut Tools) -> Res<()> {
        if self.d.abs() <= EPS {
            t.host.model.abort()?;
            self.reset_();
            t.host.changed();
            return Ok(());
        }
        t.host.model.commit(&t0("m3d.undo.push_pull"))?;
        self.last = Some(self.d);
        self.amend = Some(Amend {
            step: t.host.model.last_step(),
            fid: self.fid.unwrap_or(f64::NAN),
            k: self.k,
            sign: crate::js::or(sign(self.d), 1.0),
            coll: self.coll.clone(),
            n: self.n,
            path: t.host.path.clone(),
        });
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
    fn repeat(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(s) = t.pick7(ev).filter(|s| s.t == "f") else { return Ok(()) };
        if self.b.busy {
            t.host.model.abort()?;
            self.reset_();
        }
        let sc = t.host.scene();
        let f = sc.face(&t.host.path, s.id).ok_or(Error::Type)?;
        let k = crate::js::or(len(t.local_dir(f.n)), 1.0);
        let path = t.host.path.clone();
        let last = self.last.unwrap_or(f64::NAN);
        t.host.model.change(&t0("m3d.undo.push_pull"), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            push_pull(def, s.id, last * k);
            Ok(())
        })?;
        t.host.changed();
        Ok(())
    }
    /// Change the push just finished to distance v.
    fn amend_last(&mut self, t: &mut Tools, v: f64) -> Res<bool> {
        let Some(a) = &self.amend else { return Ok(false) };
        if !(v.abs() > EPS) || !same_path(&a.path, &t.host.path) {
            return Ok(false);
        }
        let mut d = a.sign * v;
        let mut stopped = false;
        if let Some(c) = &a.coll {
            let (delta, hit) = c.borrow_mut().clamp(mul(a.n, d));
            if hit {
                d = sign(d) * len(delta);
                stopped = true;
            }
        }
        let (fid, k, step) = (a.fid, a.k, a.step);
        let path = t.host.path.clone();
        let mut res: Option<cc_m3d::ops::PushResult> = None;
        let ok = t.host.model.amend_last(step, |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            let r = push_pull(def, fid, d * k);
            let good = r.ok;
            res = Some(r);
            if !good {
                return Err(Error::Msg("push".into()));
            }
            Ok(())
        })?;
        if !ok {
            return Ok(false);
        }
        let got = res.map_or(f64::NAN, |r| r.distance) / k;
        self.last = Some(got);
        t.host.changed();
        let msg = if stopped { tr("m3d.tool.stopped_at", &[("d", &t.host.fmt(got.abs()))]) } else { String::new() };
        t.host.print(msg);
        Ok(true)
    }
}
impl ToolImpl for PushTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        if self.fid.is_some() { t0("m3d.tool.move_to_push_or_pull_click") } else { t0("m3d.tool.click_a_face_to_push_or") }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.distance")
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if ev.count == 2.0 && self.last.is_some() && self.fid.is_some() && self.d.abs() <= EPS {
            t.host.model.abort()?;
            self.reset_();
            return self.repeat(t, ev);
        }
        if ev.count == 2.0 && self.last.is_some() && self.fid.is_none() {
            return self.repeat(t, ev);
        }
        self.pick_point(t, ev)
    }
    fn up(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if ev.dragged && self.b.busy {
            self.pick_point(t, ev)?;
        }
        Ok(())
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.hover(t, ev)
    }
    fn relock(&mut self, t: &mut Tools) -> Res<()> {
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(())
    }
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.reset_();
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        if self.fid.is_some() {
            t.host.model.abort()?;
        }
        self.reset_();
        t.host.changed();
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        if self.fid.is_some() {
            let d = sign(crate::js::or(self.d, 1.0)) * vv;
            self.apply(t, d)?;
            if self.stopped {
                let msg = tr("m3d.tool.stopped_at", &[("d", &t.host.fmt(self.d.abs()))]);
                t.host.print(msg);
            }
            self.finish(t)?;
            return Ok(Ret::True);
        }
        Ok(Ret::of(self.amend_last(t, *vv)?))
    }
    fn state(&self) -> Json {
        use crate::outj::{num, opt_num};
        serde_json::json!({"fid": opt_num(self.fid), "d": num(self.d), "last": opt_num(self.last), "stopped": self.stopped})
    }
}
impl ClickTool for PushTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(fid) = self.fid else {
            let sc = t.host.scene();
            let s = crate::infer::pick(&sc, &t.host.cam, ev.x, ev.y, &t.host.path, 7.0);
            let f = s.filter(|s| s.t == "f").and_then(|s| t.host.scene().face(&t.host.path, s.id));
            t.host.overlay(f.map(|f| Overlay {
                faces: Some(f.tris.clone()),
                tip: Some(t0("m3d.tool.push_or_pull_this_face")),
                ..Overlay::default()
            }));
            return Ok(());
        };
        let path = t.host.path.clone();
        let skip = move |o: ObjRef<'_>, k: char| same_path(o.path, &path) && k == 'f' && o.id == fid;
        let r = t.snap(ev, SnapOpts { scene: self.sc0.clone(), skip: Some(&skip), ..SnapOpts::default() });
        let at: Option<(P3, String)> = if ["end", "mid", "guidept", "edge", "guide"].contains(&r.kind.as_str()) {
            Some((r.p, r.kind.clone()))
        } else if (r.kind == "face" || r.kind == "center")
            && r.face.as_ref().is_some_and(|f| dot(f.n, self.n).abs() > 1.0 - 1e-6)
            && !self.hides(t, ev, r.p)
        {
            Some((r.p, "face".into()))
        } else {
            None
        };
        let d = match &at {
            Some((p, _)) => dot(sub(*p, self.p0), self.n),
            None => along_line(&t.host.cam, ev.x, ev.y, self.p0, self.n).map_or(0.0, |a| a.1),
        };
        self.apply(t, d)?;
        let tip = if at.as_ref().is_some_and(|a| a.1 == "face") && !self.stopped {
            tips("face")
        } else {
            format!("{}{}", t.host.fmt(self.d.abs()), if self.stopped { t0("m3d.tool.stopped_where_it_touches") } else { String::new() })
        };
        t.host.overlay(Some(Overlay {
            marker: Some(at.map(|(p, kind)| crate::overlay::Marker { p, kind })),
            tip: Some(tip),
            lines: Some(vec![Line::dashed(self.p0, add(self.p0, mul(self.n, self.d)), "axis")]),
            ..Overlay::default()
        }));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.fid.is_none() {
            if ev.count == 2.0 && self.last.is_some() {
                return self.repeat(t, ev);
            }
            let sc = t.host.scene();
            let s = crate::infer::pick(&sc, &t.host.cam, ev.x, ev.y, &t.host.path, 7.0);
            let Some(s) = s.filter(|s| s.t == "f") else {
                t.host.print(t0("m3d.tool.push_pull_works_on_a_face"));
                return Ok(());
            };
            let f = sc.face(&t.host.path, s.id).ok_or(Error::Type)?;
            let ray = t.host.cam.ray(ev.x, ev.y);
            let g = ray_plane(ray.o, ray.d, &f.plane);
            self.fid = Some(s.id);
            self.n = f.n;
            self.p0 = match g {
                Some((_, p)) => p,
                None => f.tris.first().map_or([f64::NAN; 3], |t| t[0]),
            };
            self.f0 = Some(f);
            let ln = len(t.local_dir(self.n));
            self.k = if 1.0 / jmax(ln, 1e-9) == 0.0 { 1.0 } else { ln };
            self.sc0 = Some(sc.clone());
            self.coll = if t.collide { Some(Rc::new(std::cell::RefCell::new(push_collider(t, sc, s.id)))) } else { None };
            t.host.model.begin();
            self.b.busy = true;
            self.d = 0.0;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.finish(t)
    }
}

/// Push/pull's collider: the face travels; its own solid is neither in the way nor moving.
fn push_collider(t: &mut Tools, sc: Rc<Scene>, fid: f64) -> Collider {
    let pp = t.host.path.clone();
    let c = t.ctx();
    let (own, face_e) = match t.host.model.def_mut(&c.def) {
        Some(def) => {
            let own = connected(def, &[Sel::new("f", fid)]);
            let fe = face_with_edges(def, fid);
            (own, fe)
        }
        None => (Vec::new(), Vec::new()),
    };
    let own_f = IdSet::from_iter_ids(own.iter().filter(|s| s.t == "f").map(|s| s.id));
    let own_e = IdSet::from_iter_ids(own.iter().filter(|s| s.t == "e").map(|s| s.id));
    let face_e = IdSet::from_iter_ids(face_e.iter().filter(|s| s.t == "e").map(|s| s.id));
    let p1 = pp.clone();
    let moving = move |o: ObjRef<'_>, k: char| same_path(o.path, &p1) && if k == 'f' { o.id == fid } else { face_e.has(o.id) };
    let ignore = move |o: ObjRef<'_>, k: char| same_path(o.path, &pp) && if k == 'f' { own_f.has(o.id) } else { own_e.has(o.id) };
    Collider::new(sc, &moving, &ignore)
}

/// Move's collider: the selection travels; loose geometry stuck to it stretches, so it is left out.
pub(crate) fn move_collider(t: &mut Tools, sc: Rc<Scene>, moving: &super::transform::Moving) -> Collider {
    let pp = t.host.path.clone();
    let c = t.ctx();
    let loose: Vec<Sel> = t.host.sel.iter().filter(|s| s.t == "e" || s.t == "f").cloned().collect();
    let stuck = if loose.is_empty() { Vec::new() } else { t.host.model.def(&c.def).map(|d| connected(d, &loose)).unwrap_or_default() };
    let s_f = IdSet::from_iter_ids(stuck.iter().filter(|s| s.t == "f").map(|s| s.id));
    let s_e = IdSet::from_iter_ids(stuck.iter().filter(|s| s.t == "e").map(|s| s.id));
    let ignore = move |o: ObjRef<'_>, k: char| same_path(o.path, &pp) && if k == 'f' { s_f.has(o.id) } else { s_e.has(o.id) };
    let mv = |o: ObjRef<'_>, k: char| moving.test(o, k);
    Collider::new(sc, &mv, &ignore)
}

// ---- eraser ----

#[derive(Clone, Debug)]
pub struct EraseTool {
    pub b: Base,
    items: OMap<String, Sel>,
}
impl EraseTool {
    pub fn new(name: &str) -> EraseTool {
        EraseTool { b: Base::new(name), items: OMap::new() }
    }
    fn add(&mut self, t: &mut Tools, ev: &Ev) {
        let s = t.pick_at(ev);
        if let Some(s) = s.as_ref().filter(|s| s.t != "f") {
            self.items.set(format!("{}{}", s.t, num_str(s.id)), s.clone());
        }
        t.host.overlay(Some(Overlay {
            hover: Some(s.filter(|s| s.t != "f")),
            many: Some(self.items.values().cloned().collect()),
            ..Overlay::default()
        }));
    }
}
impl ToolImpl for EraseTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        t0("m3d.tool.click_an_edge_a_group_a")
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.busy = true;
        self.items = OMap::new();
        self.add(t, ev);
        Ok(())
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.b.busy {
            self.add(t, ev);
            return Ok(());
        }
        let s = t.pick_at(ev);
        t.host.overlay(s.filter(|s| s.t != "f").map(|s| Overlay { hover: Some(Some(s)), ..Overlay::default() }));
        Ok(())
    }
    fn up(&mut self, t: &mut Tools, _ev: &Ev) -> Res<()> {
        if !self.b.busy {
            return Ok(());
        }
        self.b.busy = false;
        let items: Vec<Sel> = self.items.values().cloned().collect();
        self.items = OMap::new();
        t.host.overlay(None);
        if items.is_empty() {
            return Ok(());
        }
        let path = t.host.path.clone();
        t.host.model.change(&t0("m3d.undo.erase"), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            {
                let def = m.def_mut(&id).ok_or(Error::Type)?;
                erase_edges(def, &ids_of(&items, "e"));
                for s in items.iter().filter(|s| s.t == "i") {
                    def.insts.delete(s.id);
                }
            }
            for s in items.iter().filter(|s| s.t == "g") {
                m.guides.delete(s.id);
            }
            for s in items.iter().filter(|s| s.t == "d") {
                m.dims.delete(s.id);
            }
            for s in items.iter().filter(|s| s.t == "s") {
                m.delete_section(s.id);
            }
            if let Some(def) = m.def_mut(&id) {
                remove_loose_verts(def);
            }
            m.purge();
            Ok(())
        })?;
        t.host.set_sel(Vec::new());
        t.host.changed();
        Ok(())
    }
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.b.busy = false;
        self.items = OMap::new();
        Ok(())
    }
    fn state(&self) -> Json {
        Json::Array(self.items.values().map(|s| serde_json::json!({"t": s.t, "id": crate::outj::num(s.id)})).collect())
    }
}

#[allow(dead_code)]
fn _unused(_: &Plane) {}
