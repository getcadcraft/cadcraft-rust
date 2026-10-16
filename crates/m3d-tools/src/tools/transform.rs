//! Move / copy, Rotate and Scale (with the 0.4 M16 grips), and their Blender-style starts (grab, turn, resize).

use std::cell::RefCell;
use std::rc::Rc;

use cc_m3d::Error;
use cc_m3d::def::edge_between;
use cc_m3d::file::str_to_number;
use cc_m3d::omap::IdSet;
use cc_m3d::ops::{Sel, copy_sel, face_with_edges, transform_sel};
use cc_m3d::vec::{EPS, M4, P3, add, cross, dist, dot, len, mmul, mul, norm, plane_from, ray_plane, rotation, scaling, sub, translation};
use serde_json::Value as Json;

use super::{
    AxesO, Base, ClickTool, Ev, Res, Ret, SnapOpts, ToolArg, ToolImpl, Tools, click_tool_events, ev_of, marker, p3_of, plane_axes,
    same_path,
};
use crate::collide::Collider;
use crate::i18n::{n as nstr, t as tr, t0};
use crate::infer::{Snap, along_line};
use crate::js::{fixed_num, hypot2, jmax, jmin, round, to_fixed, truthy};
use crate::overlay::{Line, Overlay};
use crate::scene::{ObjRef, Scene};
use crate::units::Value;

/// The selection's geometry (not guides, dimensions or section planes).
fn geo_sel(t: &Tools) -> Vec<Sel> {
    t.host.sel.iter().filter(|s| s.t != "g" && s.t != "d" && s.t != "s").cloned().collect()
}

/// `moving()`: which faces, edges and placements travel with the selection.
#[derive(Clone, Debug)]
pub struct Moving {
    path: Vec<f64>,
    eids: IdSet,
    fids: IdSet,
    iids: IdSet,
}
impl Moving {
    pub fn of(t: &mut Tools) -> Moving {
        let sel = geo_sel(t);
        let c = t.ctx();
        let (mut eids, mut fids, mut iids) = (IdSet::new(), IdSet::new(), IdSet::new());
        for s in &sel {
            match s.t.as_str() {
                "e" => eids.add(s.id),
                "f" => fids.add(s.id),
                _ => iids.add(s.id),
            }
        }
        if let Some(def) = t.host.model.def_mut(&c.def) {
            for fid in fids.to_vec() {
                let Some(f) = def.faces.get(fid).cloned() else { continue };
                for l in &f.loops {
                    let n = l.len();
                    for (i, v) in l.iter().enumerate() {
                        let w = l.get((i + 1) % n).copied().unwrap_or(f64::NAN);
                        if let Some(e) = edge_between(def, *v, w) {
                            eids.add(e);
                        }
                    }
                }
            }
        }
        Moving { path: t.host.path.clone(), eids, fids, iids }
    }
    pub fn test(&self, o: ObjRef<'_>, k: char) -> bool {
        let p = &self.path;
        if o.path.len() > p.len() && same_path(o.path.get(..p.len()).unwrap_or(&[]), p) {
            return o.path.get(p.len()).is_some_and(|x| self.iids.has(*x));
        }
        if !same_path(o.path, p) {
            return false;
        }
        if k == 'e' { self.eids.has(o.id) } else { self.fids.has(o.id) }
    }
}

/// Pick something to work on when nothing is selected (a face comes with its edges). False when nothing was hit.
fn pick_if_none(t: &mut Tools, ev: &Ev, msg: &str) -> bool {
    if !geo_sel(t).is_empty() {
        return true;
    }
    let Some(s) = t.pick7(ev).filter(|s| s.t != "g") else {
        t.host.print(t0(msg));
        return false;
    };
    let c = t.ctx();
    let sel = if s.t == "f" { t.host.model.def_mut(&c.def).map(|d| face_with_edges(d, s.id)).unwrap_or_default() } else { vec![s] };
    t.host.set_sel(sel);
    true
}

/// `done()`: a tool started from a key goes back to Select when it ends.
fn done(b: &mut Base, t: &mut Tools) -> Res<()> {
    if b.modal {
        b.modal = false;
        t.set("select", ToolArg::None)?;
    }
    Ok(())
}

fn snap_or(t: &mut Tools, cur: &Option<Snap>, ev: &Ev) -> Snap {
    match cur {
        Some(c) => c.clone(),
        None => t.snap(ev, SnapOpts::default()),
    }
}

// ---- move / copy ----

#[derive(Debug)]
pub struct MoveTool {
    pub b: Base,
    p0: Option<P3>,
    copy: bool,
    pub delta: P3,
    pub stopped: bool,
    sc0: Option<Rc<Scene>>,
    skip: Option<Moving>,
    coll: Option<Rc<RefCell<Collider>>>,
    result: Option<Vec<Sel>>,
}
impl MoveTool {
    pub fn new(name: &str) -> MoveTool {
        MoveTool {
            b: Base::new(name),
            p0: None,
            copy: false,
            delta: [0.0; 3],
            stopped: false,
            sc0: None,
            skip: None,
            coll: None,
            result: None,
        }
    }
    fn reset_(&mut self) {
        self.p0 = None;
        self.b.busy = false;
        self.copy = false;
        self.delta = [0.0; 3];
    }
    pub fn apply(&mut self, t: &mut Tools, delta0: P3) -> Res<()> {
        let mut delta = delta0;
        self.stopped = false;
        if let Some(c) = &self.coll {
            let (d, hit) = c.borrow_mut().clamp(delta);
            delta = d;
            self.stopped = hit;
        }
        self.delta = delta;
        let sel = geo_sel(t);
        let copy = self.copy;
        let ld = t.local_dir(delta);
        let path = t.host.path.clone();
        let scope = t.scope();
        self.result = t.host.model.preview(scope.as_deref(), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            let s = if copy { copy_sel(def, &sel) } else { sel.clone() };
            transform_sel(def, &s, &translation(ld));
            Ok(s)
        })?;
        t.host.changed();
        let txt = t.host.fmt(len(delta));
        t.host.vcb(t0("m3d.vcb.distance"), txt);
        Ok(())
    }
    pub fn begin(&mut self, t: &mut Tools, p0: P3, copy: bool) -> Res<()> {
        self.p0 = Some(p0);
        let sc = t.host.scene();
        self.sc0 = Some(sc.clone());
        let mv = Moving::of(t);
        self.copy = copy;
        self.coll = if t.collide { Some(Rc::new(RefCell::new(super::edit::move_collider(t, sc, &mv)))) } else { None };
        self.skip = Some(mv);
        t.host.model.begin();
        self.b.busy = true;
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
    pub fn finish(&mut self, t: &mut Tools) -> Res<()> {
        if len(self.delta) <= EPS {
            t.host.model.abort()?;
            self.reset_();
            t.host.changed();
            return done(&mut self.b, t);
        }
        let (copy, delta) = (self.copy, self.delta);
        let src = geo_sel(t);
        t.host.model.commit(&if copy { t0("m3d.undo.copy") } else { t0("m3d.undo.move") })?;
        if copy {
            t.last_copy = Some(super::LastCopy { delta, src });
            t.host.set_sel(self.result.clone().unwrap_or_default());
        } else {
            t.last_copy = None;
        }
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        let h = if copy { t0("m3d.tool.copied_type_3x_for_more_copies") } else { self.hint(t) };
        t.host.tip(h);
        done(&mut self.b, t)
    }
}
impl ToolImpl for MoveTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn lockable(&self) -> bool {
        true
    }
    fn hint(&self, _t: &Tools) -> String {
        if self.p0.is_some() {
            t0(if self.copy { "m3d.tool.move_where_copy" } else { "m3d.tool.move_where" })
        } else {
            t0("m3d.tool.click_a_point_to_move_from")
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.distance")
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.reset_();
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        if self.p0.is_some() {
            t.host.model.abort()?;
        }
        self.reset_();
        t.host.changed();
        Ok(())
    }
    fn key(&mut self, t: &mut Tools, k: &str) -> Res<bool> {
        if (k == "Control" || k == "Alt") && self.p0.is_some() {
            self.copy = !self.copy;
            let h = self.hint(t);
            t.host.tip(h);
            if let Some(ev) = self.b.last_ev {
                self.hover(t, &ev)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        if let Some(p0) = self.p0 {
            let delta = match v {
                Value::Len { v: vv, .. } => {
                    let d = norm(self.delta);
                    if !truthy(len(d)) {
                        return Ok(Ret::False);
                    }
                    mul(d, *vv)
                }
                Value::Rel(p) => cc_m3d::vec::apply_dir(&t.ctx_m(), *p),
                Value::Abs(p) => sub(t.world(*p), p0),
                _ => return Ok(Ret::False),
            };
            self.apply(t, delta)?;
            if self.stopped {
                let msg = tr("m3d.tool.stopped_after", &[("d", &t.host.fmt(len(self.delta)))]);
                t.host.print(msg);
            }
            self.finish(t)?;
            return Ok(Ret::True);
        }
        let (is_array, n) = match v {
            Value::Array(n) => (true, *n),
            Value::Divide(n) => (false, *n),
            _ => return Ok(Ret::False),
        };
        let Some(lc) = t.last_copy.clone() else { return Ok(Ret::False) };
        if n < 1.0 || n > 500.0 {
            return Ok(Ret::False);
        }
        t.host.model.undo()?;
        let label = if is_array { format!("Copy ×{}", nstr(n)) } else { format!("Copy /{}", nstr(n)) };
        let path = t.host.path.clone();
        let mut steps = Vec::new();
        let mut k = 1.0;
        while k <= n {
            steps.push(t.local_dir(mul(lc.delta, if is_array { k } else { k / n })));
            k += 1.0;
        }
        let made = t.host.model.change(&label, |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            let mut out = Vec::new();
            for ld in &steps {
                let s = copy_sel(def, &lc.src);
                transform_sel(def, &s, &translation(*ld));
                out.extend(s);
            }
            Ok(out)
        })?;
        t.host.set_sel(made);
        t.host.changed();
        Ok(Ret::True)
    }
    fn call(&mut self, t: &mut Tools, name: &str, args: &[Json]) -> Res<Option<Json>> {
        match name {
            "begin" => {
                let p = p3_of(args.first()).ok_or(Error::Type)?;
                self.begin(t, p, args.get(1).is_some_and(|v| cc_m3d::json::truthy(Some(v))))?;
            }
            "apply" => {
                let d = p3_of(args.first()).ok_or(Error::Type)?;
                self.apply(t, d)?;
            }
            "finish" => self.finish(t)?,
            "start" => {
                let ev = ev_of(args.first().unwrap_or(&Json::Null));
                let r = t.snap(&ev, SnapOpts::default());
                self.begin(t, r.p, args.get(1).is_some_and(|v| cc_m3d::json::truthy(Some(v))))?;
                self.b.modal = true;
                self.b.last_ev = Some(ev);
            }
            _ => return Ok(None),
        }
        Ok(Some(Json::Null))
    }
    fn state(&self) -> Json {
        use crate::outj::{opt_p3, p3};
        serde_json::json!({"p0": opt_p3(self.p0.as_ref()), "copy": self.copy, "delta": p3(&self.delta), "stopped": self.stopped})
    }
}
impl ClickTool for MoveTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(p0) = self.p0 else {
            let r = t.snap(ev, SnapOpts::default());
            let mut o = Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() };
            if t.host.sel.is_empty()
                && let Some(s) = t.pick7(ev).filter(|s| s.t != "g")
            {
                o.hover = Some(Some(s));
            }
            self.b.cur = Some(r);
            t.host.overlay(Some(o));
            return Ok(());
        };
        let mv = self.skip.clone();
        let skip = move |o: ObjRef<'_>, k: char| mv.as_ref().is_some_and(|m| m.test(o, k));
        let r = t.snap(ev, SnapOpts { from: Some(p0), scene: self.sc0.clone(), skip: Some(&skip), ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        self.apply(t, sub(r.p, p0))?;
        let tip = if self.stopped { t0("m3d.tool.stopped_where_it_touches_2") } else { r.tip.clone() };
        t.host.overlay(Some(Overlay {
            lines: Some(vec![Line::new(p0, r.p, &r.kind)]),
            marker: Some(marker(&r)),
            tip: Some(tip),
            ..Overlay::default()
        }));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.p0.is_none() {
            if !pick_if_none(t, ev, "m3d.tool.select_something_to_move_first_or") {
                return Ok(());
            }
            let r = snap_or(t, &self.b.cur, ev);
            return self.begin(t, r.p, ev.ctrl || ev.alt);
        }
        self.finish(t)
    }
}

// ---- rotate ----

fn axis_kind(n: P3, ax: &AxesO) -> &'static str {
    if dot(n, ax.x).abs() > 0.999 {
        "axis-x"
    } else if dot(n, ax.y).abs() > 0.999 {
        "axis-y"
    } else if dot(n, ax.z).abs() > 0.999 {
        "axis-z"
    } else {
        "axis"
    }
}

/// The lock's direction when it is an axis lock.
fn axis_lock(t: &Tools) -> Option<P3> {
    t.lock.as_ref().filter(|l| l.kind.starts_with("axis")).map(|l| l.dir)
}

#[derive(Debug)]
pub struct RotateTool {
    pub b: Base,
    pub stage: u8,
    angle: f64,
    c: P3,
    n: P3,
    reff: P3,
    sc0: Option<Rc<Scene>>,
    skip: Option<Moving>,
}
impl RotateTool {
    pub fn new(name: &str) -> RotateTool {
        RotateTool {
            b: Base::new(name),
            stage: 0,
            angle: 0.0,
            c: [f64::NAN; 3],
            n: [f64::NAN; 3],
            reff: [f64::NAN; 3],
            sc0: None,
            skip: None,
        }
    }
    fn reset_(&mut self) {
        self.stage = 0;
        self.b.busy = false;
        self.angle = 0.0;
    }
    fn axis(&self, t: &Tools, r: Option<&Snap>) -> P3 {
        if let Some(d) = axis_lock(t) {
            return d;
        }
        match r.and_then(|r| r.face.as_ref().filter(|_| r.kind == "face" || r.kind == "center")) {
            Some(f) => f.n,
            None => t.axes().z,
        }
    }
    fn on_plane(&self, t: &mut Tools, ev: &Ev) -> (Snap, P3) {
        let ray = t.host.cam.ray(ev.x, ev.y);
        let mv = self.skip.clone();
        let skip = move |o: ObjRef<'_>, k: char| mv.as_ref().is_some_and(|m| m.test(o, k));
        let r = t.snap(ev, SnapOpts { scene: self.sc0.clone(), skip: Some(&skip), ..SnapOpts::default() });
        let mut q = r.p;
        if !["end", "mid", "edge", "guidept", "guide"].contains(&r.kind.as_str())
            && let Some((_, g)) = ray_plane(ray.o, ray.d, &plane_from(self.n, self.c))
        {
            q = g;
        }
        let q = sub(q, mul(self.n, dot(sub(q, self.c), self.n)));
        (r, q)
    }
    fn apply(&mut self, t: &mut Tools, a: f64) -> Res<()> {
        self.angle = a;
        let sel = geo_sel(t);
        let m4 = rotation(t.local_dir(self.n), a, t.local(self.c));
        let path = t.host.path.clone();
        let scope = t.scope();
        t.host.model.preview(scope.as_deref(), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            transform_sel(def, &sel, &m4);
            Ok(())
        })?;
        t.host.changed();
        t.host.vcb(t0("m3d.vcb.angle"), to_fixed(a * 180.0 / std::f64::consts::PI, 1));
        Ok(())
    }
    fn start_at(&mut self, t: &mut Tools, c: P3, ev: &Ev) -> Res<()> {
        self.c = c;
        self.n = norm(axis_lock(t).unwrap_or_else(|| t.axes().z));
        self.sc0 = Some(t.host.scene());
        self.skip = Some(Moving::of(t));
        let (_, q) = self.on_plane(t, ev);
        self.reff = if dist(q, c) > EPS { norm(sub(q, c)) } else { plane_axes(self.n, &t.axes()).0 };
        self.stage = 2;
        self.b.busy = true;
        self.b.modal = true;
        t.host.model.begin();
        t.host.tip(t0("m3d.tool.move_to_turn_click_to_finish"));
        self.b.last_ev = Some(*ev);
        Ok(())
    }
    fn finish(&mut self, t: &mut Tools) -> Res<()> {
        if self.angle.abs() < 1e-9 {
            t.host.model.abort()?;
        } else {
            t.host.model.commit(&t0("m3d.undo.rotate"))?;
        }
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        done(&mut self.b, t)
    }
}
impl ToolImpl for RotateTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn lockable(&self) -> bool {
        true
    }
    fn hint(&self, _t: &Tools) -> String {
        match self.stage {
            0 => t0("m3d.tool.click_the_centre_of_rotation_on"),
            1 => t0("m3d.tool.click_a_point_to_start_the"),
            _ => t0("m3d.tool.click_to_set_the_angle_or"),
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.angle")
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
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
        if self.b.modal
            && self.stage == 2
            && let Some(ev) = self.b.last_ev
        {
            t.host.model.abort()?;
            let c = self.c;
            self.start_at(t, c, &ev)?;
            return self.hover(t, &ev);
        }
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
        if self.stage == 2 {
            t.host.model.abort()?;
        }
        self.reset_();
        t.host.changed();
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        if self.stage != 2 {
            return Ok(Ret::False);
        }
        let deg = match v {
            Value::Num(x) => *x,
            Value::Len { num: Some(n), .. } => *n,
            _ => return Ok(Ret::False),
        };
        self.apply(t, deg * std::f64::consts::PI / 180.0)?;
        self.finish(t)?;
        Ok(Ret::True)
    }
    fn call(&mut self, t: &mut Tools, name: &str, args: &[Json]) -> Res<Option<Json>> {
        if name != "startAt" {
            return Ok(None);
        }
        let c = p3_of(args.first()).ok_or(Error::Type)?;
        let ev = ev_of(args.get(1).unwrap_or(&Json::Null));
        self.start_at(t, c, &ev)?;
        Ok(Some(Json::Null))
    }
    fn state(&self) -> Json {
        serde_json::json!({"stage": self.stage, "angle": crate::outj::num(self.angle)})
    }
}
impl ClickTool for RotateTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        if self.stage == 0 {
            let r = t.snap(ev, SnapOpts::default());
            let n = self.axis(t, Some(&r));
            let ax = t.axes();
            let (u, _) = plane_axes(n, &ax);
            let sz = t.host.cam.pixel_size(r.p) * 40.0;
            t.host.overlay(Some(Overlay {
                poly: Some(cc_m3d::ops::ngon_pts(r.p, n, u, sz, 36.0)),
                poly_kind: Some(axis_kind(n, &ax).into()),
                marker: Some(marker(&r)),
                tip: Some(r.tip.clone()),
                ..Overlay::default()
            }));
            self.b.cur = Some(r);
            return Ok(());
        }
        let (r, q) = self.on_plane(t, ev);
        let sz = t.host.cam.pixel_size(self.c) * 40.0;
        let ax = t.axes();
        let ring = cc_m3d::ops::ngon_pts(self.c, self.n, plane_axes(self.n, &ax).0, sz, 36.0);
        let pk = axis_kind(self.n, &ax).to_string();
        if self.stage == 1 {
            t.host.overlay(Some(Overlay {
                poly: Some(ring),
                poly_kind: Some(pk),
                lines: Some(vec![Line::dashed(self.c, q, "axis")]),
                marker: Some(marker(&r)),
                ..Overlay::default()
            }));
            self.b.cur = Some(Snap::bare(q));
            return Ok(());
        }
        let v = norm(sub(q, self.c));
        let pi = std::f64::consts::PI;
        let mut a = libm::atan2(dot(cross(self.reff, v), self.n), dot(self.reff, v));
        let step = pi / 12.0;
        let snapped = round(a / step) * step;
        if (a - snapped).abs() < 2.5 * pi / 180.0 {
            a = snapped;
        }
        self.apply(t, a)?;
        t.host.overlay(Some(Overlay {
            poly: Some(ring),
            poly_kind: Some(pk),
            lines: Some(vec![Line::dashed(self.c, add(self.c, mul(self.reff, sz * 1.4)), "axis"), Line::new(self.c, q, "axis")]),
            tip: Some(format!("{}°", to_fixed(a * 180.0 / pi, 1))),
            ..Overlay::default()
        }));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.stage == 0 {
            if !pick_if_none(t, ev, "m3d.tool.select_something_to_rotate_first_or") {
                return Ok(());
            }
            let r = snap_or(t, &self.b.cur, ev);
            self.c = r.p;
            self.n = norm(self.axis(t, Some(&r)));
            self.sc0 = Some(t.host.scene());
            self.skip = Some(Moving::of(t));
            self.stage = 1;
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        if self.stage == 1 {
            let (_, q) = self.on_plane(t, ev);
            if dist(q, self.c) <= EPS {
                return Ok(());
            }
            self.reff = norm(sub(q, self.c));
            self.stage = 2;
            t.host.model.begin();
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        self.finish(t)
    }
}

// ---- scale (uniform about a point, or per axis from a grip) ----

/// A grip on the selection's box (open group's own axes): where, the opposite point, the centre, which axes it pulls.
#[derive(Clone, Debug)]
pub struct Grip {
    pub p: P3,
    pub anchor: P3,
    pub center: P3,
    pub mask: [bool; 3],
}

#[derive(Debug)]
pub struct ScaleTool {
    pub b: Base,
    pub stage: u8,
    f: f64,
    grip: Option<usize>,
    pub grips: Vec<Grip>,
    grip_anchor: P3,
    fs: [f64; 3],
    base: P3,
    reff: P3,
    sc0: Option<Rc<Scene>>,
    skip: Option<Moving>,
}
impl ScaleTool {
    pub fn new(t: &mut Tools, name: &str) -> ScaleTool {
        let mut s = ScaleTool {
            b: Base::new(name),
            stage: 0,
            f: 1.0,
            grip: None,
            grips: Vec::new(),
            grip_anchor: [f64::NAN; 3],
            fs: [1.0; 3],
            base: [f64::NAN; 3],
            reff: [f64::NAN; 3],
            sc0: None,
            skip: None,
        };
        s.reset_(t);
        s
    }
    fn reset_(&mut self, t: &Tools) {
        self.stage = 0;
        self.b.busy = false;
        self.f = 1.0;
        self.grip = None;
        self.grips = compute_grips(t);
    }
    fn grip_at(&self, t: &Tools, ev: &Ev) -> Option<usize> {
        let vp = t.host.cam.view_proj();
        let mut best = None;
        let mut bd = if t.touch { 20.0 } else { 9.0 };
        for (i, g) in self.grips.iter().enumerate() {
            if let Some(s) = t.host.cam.project_vp(t.world(g.p), &vp) {
                let d = hypot2(s[0] - ev.x, s[1] - ev.y);
                if d < bd {
                    bd = d;
                    best = Some(i);
                }
            }
        }
        best
    }
    fn apply_axes(&mut self, t: &mut Tools, fs: [f64; 3]) -> Res<()> {
        let sel = geo_sel(t);
        let a = self.grip_anchor;
        let s: M4 = [fs[0], 0.0, 0.0, 0.0, 0.0, fs[1], 0.0, 0.0, 0.0, 0.0, fs[2], 0.0, 0.0, 0.0, 0.0, 1.0];
        let m4 = mmul(&translation(a), &mmul(&s, &translation(mul(a, -1.0))));
        self.fs = fs;
        let path = t.host.path.clone();
        let scope = t.scope();
        t.host.model.preview(scope.as_deref(), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            transform_sel(def, &sel, &m4);
            Ok(())
        })?;
        t.host.changed();
        let txt = fs.iter().map(|x| nstr(fixed_num(*x, 4))).collect::<Vec<_>>().join(", ");
        t.host.vcb(t0("m3d.vcb.scale"), txt);
        Ok(())
    }
    fn apply(&mut self, t: &mut Tools, f: f64) -> Res<()> {
        self.f = f;
        let sel = geo_sel(t);
        let m4 = scaling(f, t.local(self.base));
        let path = t.host.path.clone();
        let scope = t.scope();
        t.host.model.preview(scope.as_deref(), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            transform_sel(def, &sel, &m4);
            Ok(())
        })?;
        t.host.changed();
        t.host.vcb(t0("m3d.vcb.scale"), nstr(fixed_num(f, 4)));
        Ok(())
    }
    fn start_at(&mut self, t: &mut Tools, c: P3, ev: &Ev) -> Res<()> {
        self.base = c;
        let sc = t.host.scene();
        self.sc0 = Some(sc.clone());
        let mv = Moving::of(t);
        self.skip = Some(mv.clone());
        let skip = move |o: ObjRef<'_>, k: char| mv.test(o, k);
        let r = t.snap(ev, SnapOpts { from: Some(c), scene: Some(sc), skip: Some(&skip), ..SnapOpts::default() });
        let mut q = r.p;
        if dist(q, c) <= EPS {
            let cam = &t.host.cam;
            let ray = cam.ray(ev.x, ev.y);
            let g = ray_plane(ray.o, ray.d, &plane_from(cam.axes().z, c));
            q = match g {
                Some((_, gp)) if dist(gp, c) > EPS => gp,
                _ => add(c, mul(t.axes().x, cam.pixel_size(c) * 60.0)),
            };
        }
        self.reff = q;
        self.stage = 2;
        self.b.busy = true;
        self.b.modal = true;
        t.host.model.begin();
        t.host.tip(t0("m3d.tool.move_away_from_the_centre_to"));
        self.b.last_ev = Some(*ev);
        Ok(())
    }
    fn finish_axes(&mut self, t: &mut Tools) -> Res<()> {
        if self.fs.iter().all(|x| (x - 1.0).abs() < 1e-9) {
            t.host.model.abort()?;
        } else {
            t.host.model.commit(&t0("m3d.undo.scale"))?;
        }
        self.reset_(t);
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(())
    }
    fn finish(&mut self, t: &mut Tools) -> Res<()> {
        if (self.f - 1.0).abs() < 1e-9 {
            t.host.model.abort()?;
        } else {
            t.host.model.commit(&t0("m3d.undo.scale"))?;
        }
        self.reset_(t);
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        done(&mut self.b, t)
    }
}

/// The 26 (or fewer, for a flat or thin selection) grips on the selection's box.
fn compute_grips(t: &Tools) -> Vec<Grip> {
    let sel: Vec<&Sel> = t.host.sel.iter().filter(|s| s.t == "e" || s.t == "f" || s.t == "i").collect();
    if sel.is_empty() {
        return Vec::new();
    }
    let c = t.ctx();
    let m = &t.host.model;
    let Some(def) = m.def(&c.def) else { return Vec::new() };
    let mut bb: Option<(P3, P3)> = None;
    let mut put = |p: P3| {
        let b = bb.get_or_insert((p, p));
        b.0 = [jmin(b.0[0], p[0]), jmin(b.0[1], p[1]), jmin(b.0[2], p[2])];
        b.1 = [jmax(b.1[0], p[0]), jmax(b.1[1], p[1]), jmax(b.1[2], p[2])];
    };
    for s in sel {
        match s.t.as_str() {
            "e" => {
                if let Some(e) = def.edges.get(s.id) {
                    put(def.v(e.a));
                    put(def.v(e.b));
                }
            }
            "f" => {
                if let Some(f) = def.faces.get(s.id) {
                    for l in &f.loops {
                        for v in l {
                            put(def.v(*v));
                        }
                    }
                }
            }
            _ => {
                if let Some(inst) = def.insts.get(s.id)
                    && m.def(&inst.def).is_some()
                    && let Some(ib) = m.bbox(&inst.def, &inst.m)
                {
                    put(ib.min);
                    put(ib.max);
                }
            }
        }
    }
    let Some((lo, hi)) = bb else { return Vec::new() };
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let live = ext.map(|x| x > EPS);
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for i in [0.0, 0.5, 1.0] {
        for j in [0.0, 0.5, 1.0] {
            for k in [0.0, 0.5, 1.0] {
                let f = [i, j, k];
                if f.iter().all(|x| *x == 0.5) {
                    continue;
                }
                let mask = [f[0] != 0.5 && live[0], f[1] != 0.5 && live[1], f[2] != 0.5 && live[2]];
                if !mask.iter().any(|x| *x) {
                    continue;
                }
                let p = [lo[0] + ext[0] * f[0], lo[1] + ext[1] * f[1], lo[2] + ext[2] * f[2]];
                let key = p.iter().map(|x| to_fixed(*x, 9)).collect::<Vec<_>>().join(",");
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                out.push(Grip {
                    p,
                    anchor: [lo[0] + ext[0] * (1.0 - f[0]), lo[1] + ext[1] * (1.0 - f[1]), lo[2] + ext[2] * (1.0 - f[2])],
                    center: [lo[0] + ext[0] / 2.0, lo[1] + ext[1] / 2.0, lo[2] + ext[2] / 2.0],
                    mask,
                });
            }
        }
    }
    out
}

impl ToolImpl for ScaleTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn lockable(&self) -> bool {
        true
    }
    fn hint(&self, _t: &Tools) -> String {
        if self.stage == 3 {
            return t0("m3d.tool.move_to_resize_click_to_finish");
        }
        if self.stage == 0 && !self.grips.is_empty() {
            return t0("m3d.tool.drag_or_click_a_grip_on");
        }
        match self.stage {
            0 => t0("m3d.tool.click_the_point_that_stays_put"),
            1 => t0("m3d.tool.click_a_reference_point_its_distance"),
            _ => t0("m3d.tool.scale_set"),
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.scale")
    }
    click_tool_events!();
    fn reset(&mut self, t: &mut Tools) -> bool {
        self.reset_(t);
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        if self.stage == 2 || self.stage == 3 {
            t.host.model.abort()?;
        }
        self.reset_(t);
        t.host.changed();
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, text: &str) -> Res<Ret> {
        if self.stage == 3 {
            let nums: Vec<f64> =
                text.split([',', ';']).map(|x| if crate::js::trim(x).is_empty() { f64::NAN } else { str_to_number(x) }).collect();
            let mask = self.grip.and_then(|i| self.grips.get(i)).map(|g| g.mask).ok_or(Error::Type)?;
            let fs = if let [a, b, c] = nums.as_slice()
                && nums.iter().all(|x| *x > 0.0)
            {
                [*a, *b, *c]
            } else if let [n0] = nums.as_slice()
                && *n0 > 0.0
            {
                mask.map(|on| if on { *n0 } else { 1.0 })
            } else {
                t.host.print(t0("m3d.tool.type_a_factor_above_0_2"));
                return Ok(Ret::False);
            };
            self.apply_axes(t, fs)?;
            self.finish_axes(t)?;
            return Ok(Ret::True);
        }
        if self.stage != 2 {
            return Ok(Ret::False);
        }
        let f = match v {
            Value::Num(x) => Some(*x),
            Value::Len { num: Some(n), .. } => Some(*n),
            _ => None,
        };
        let Some(f) = f.filter(|f| *f > 0.0) else {
            t.host.print(t0("m3d.tool.type_a_factor_above_0_such"));
            return Ok(Ret::False);
        };
        self.apply(t, f)?;
        self.finish(t)?;
        Ok(Ret::True)
    }
    fn call(&mut self, t: &mut Tools, name: &str, args: &[Json]) -> Res<Option<Json>> {
        if name != "startAt" {
            return Ok(None);
        }
        let c = p3_of(args.first()).ok_or(Error::Type)?;
        let ev = ev_of(args.get(1).unwrap_or(&Json::Null));
        self.start_at(t, c, &ev)?;
        Ok(Some(Json::Null))
    }
    fn state(&self) -> Json {
        use crate::outj::{num, p3};
        serde_json::json!({
            "stage": self.stage, "f": num(self.f), "grip": self.grip.map_or(Json::Null, Json::from),
            "grips": self.grips.iter().map(|g| serde_json::json!({"p": p3(&g.p), "anchor": p3(&g.anchor), "center": p3(&g.center), "mask": g.mask})).collect::<Vec<_>>(),
        })
    }
}
impl ClickTool for ScaleTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        if self.stage == 0 && !self.grips.is_empty() {
            let hot = self.grip_at(t, ev);
            let gw: Vec<P3> = self.grips.iter().map(|g| t.world(g.p)).collect();
            if let Some(h) = hot {
                let even = self.grips.get(h).is_some_and(|g| g.mask.iter().filter(|x| **x).count() == 3);
                t.host.overlay(Some(Overlay {
                    grips: Some(gw),
                    hot_grip: Some(h as f64),
                    tip: Some(if even { t0("m3d.tool.scale_evenly") } else { "Stretch".into() }),
                    ..Overlay::default()
                }));
                return Ok(());
            }
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { grips: Some(gw), marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        }
        if self.stage == 3 {
            let g = self.grip.and_then(|i| self.grips.get(i)).cloned().ok_or(Error::Type)?;
            let aw = t.world(self.grip_anchor);
            let gw = t.world(g.p);
            let u = sub(gw, aw);
            let q = along_line(&t.host.cam, ev.x, ev.y, aw, norm(u));
            let Some((_, qs)) = q else { return Ok(()) };
            if !(len(u) > EPS) {
                return Ok(());
            }
            let f = jmax(0.001, qs / len(u));
            self.apply_axes(t, g.mask.map(|on| if on { f } else { 1.0 }))?;
            t.host.overlay(Some(Overlay {
                lines: Some(vec![Line::dashed(aw, add(aw, mul(u, f)), "axis")]),
                grips: Some(vec![add(aw, mul(u, f))]),
                hot_grip: Some(0.0),
                tip: Some(format!("{}×", nstr(fixed_num(f, 3)))),
                ..Overlay::default()
            }));
            return Ok(());
        }
        let r = if self.stage > 0 {
            let mv = self.skip.clone();
            let skip = move |o: ObjRef<'_>, k: char| mv.as_ref().is_some_and(|m| m.test(o, k));
            t.snap(ev, SnapOpts { from: Some(self.base), scene: self.sc0.clone(), skip: Some(&skip), ..SnapOpts::default() })
        } else {
            t.snap(ev, SnapOpts::default())
        };
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        if self.stage < 2 {
            let lines = if self.stage > 0 { vec![Line::dashed(self.base, r.p, &r.kind)] } else { Vec::new() };
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), lines: Some(lines), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        }
        let u = sub(self.reff, self.base);
        let f = jmax(0.001, dot(sub(r.p, self.base), u) / dot(u, u));
        self.apply(t, f)?;
        t.host.overlay(Some(Overlay {
            lines: Some(vec![Line::new(self.base, add(self.base, mul(u, f)), "axis")]),
            marker: Some(marker(&r)),
            tip: Some(format!("{}×", to_fixed(f, 3))),
            ..Overlay::default()
        }));
        self.b.cur = Some(r);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.stage == 0
            && !self.grips.is_empty()
            && let Some(i) = self.grip_at(t, ev)
        {
            self.grip = Some(i);
            let g = self.grips.get(i).cloned().ok_or(Error::Type)?;
            self.grip_anchor = if ev.ctrl { g.center } else { g.anchor };
            self.stage = 3;
            self.b.busy = true;
            self.fs = [1.0; 3];
            t.host.model.begin();
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        if self.stage == 3 {
            self.hover(t, ev)?;
            return self.finish_axes(t);
        }
        if self.stage == 0 {
            if !pick_if_none(t, ev, "m3d.tool.select_something_to_scale_first_or") {
                return Ok(());
            }
            let r = snap_or(t, &self.b.cur, ev);
            self.base = r.p;
            self.sc0 = Some(t.host.scene());
            self.skip = Some(Moving::of(t));
            self.stage = 1;
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        if self.stage == 1 {
            let r = snap_or(t, &self.b.cur, ev);
            if dist(r.p, self.base) <= EPS {
                return Ok(());
            }
            self.reff = r.p;
            self.stage = 2;
            t.host.model.begin();
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        self.finish(t)
    }
}

#[allow(dead_code)]
fn _same(a: &[f64], b: &[f64]) -> bool {
    same_path(a, b)
}
