//! The other tools: Curve (smooth curves, with point editing), Sculpt (terrain), drawing Axes, Section planes, the
//! camera tools (Orbit, Pan, Zoom), Pick (groups for the app), Place (a component), Light (a lamp) and Paste.

use cc_m3d::Error;
use cc_m3d::def::face_normal;
use cc_m3d::model::place;
use cc_m3d::ops::{SPLINE_SEGS, add_spline, edit_spline, ngon_pts, smooth_pts};
use cc_m3d::vec::{EPS, IDENT, M4, P3, add, apply, apply_dir, cross, dist, dot, len, mul, norm, plane_from, ray_plane, sub, translation};
use serde_json::Value as Json;

use super::{Base, ClickTool, Ev, Res, Ret, SnapOpts, ToolArg, ToolImpl, Tools, click_tool_events, err_message, marker, plane_axes};
use crate::i18n::{n as nstr, t as tr, t0};
use crate::infer::Snap;
use crate::js::{hypot2, truthy};
use crate::overlay::{Line, Overlay};
use crate::scene::ObjRef;
use crate::units::Value;

fn snap_or(t: &mut Tools, cur: &Option<Snap>, ev: &Ev) -> Snap {
    match cur {
        Some(c) => c.clone(),
        None => t.snap(ev, SnapOpts::default()),
    }
}

fn polyline(pts: &[P3], kind: &str) -> Vec<Line> {
    pts.windows(2).filter_map(|w| if let [a, b] = w { Some(Line::new(*a, *b, kind)) } else { None }).collect()
}

const BOX_EDGES: [(u8, u8); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
fn corner(min: P3, max: P3, i: u8) -> P3 {
    [if i & 1 != 0 { max[0] } else { min[0] }, if i & 2 != 0 { max[1] } else { min[1] }, if i & 4 != 0 { max[2] } else { min[2] }]
}

// ---- smooth curves (0.4 M27) ----

#[derive(Clone, Debug)]
pub struct CurveTool {
    pub b: Base,
    pub pts: Vec<P3>,
    edit: Option<String>,
    grab: Option<usize>,
}
impl CurveTool {
    pub fn new(t: &mut Tools, name: &str, arg: ToolArg) -> Res<CurveTool> {
        let mut c = CurveTool { b: Base::new(name), pts: Vec::new(), edit: None, grab: None };
        if let ToolArg::CurveEdit(cid) = arg {
            c.start_edit(t, &cid);
        }
        Ok(c)
    }
    fn reset_(&mut self) {
        self.pts = Vec::new();
        self.b.busy = false;
        self.edit = None;
        self.grab = None;
    }
    fn start_edit(&mut self, t: &mut Tools, cid: &str) {
        let c = t.ctx();
        if !t.host.model.def(&c.def).is_some_and(|d| d.splines.has(cid)) {
            t.host.print(t0("m3d.tool.this_curve_can_no_longer_be"));
            return;
        }
        self.edit = Some(cid.to_string());
        self.b.busy = true;
        let h = self.hint(t);
        t.host.tip(h);
        self.show_grips(t, -1.0, Overlay::default());
    }
    fn grips_w(&self, t: &Tools) -> Vec<P3> {
        let c = t.ctx();
        let Some(cid) = &self.edit else { return Vec::new() };
        match t.host.model.def(&c.def).and_then(|d| d.splines.get(cid.as_str())) {
            Some(sp) => sp.pts.iter().map(|p| apply(&c.m, *p)).collect(),
            None => Vec::new(),
        }
    }
    fn show_grips(&self, t: &mut Tools, hot: f64, extra: Overlay) {
        let g = self.grips_w(t);
        t.host.overlay(Some(Overlay { grips: Some(g), hot_grip: Some(hot), ..extra }));
    }
    fn grip_at(&self, t: &Tools, ev: &Ev) -> f64 {
        let g = self.grips_w(t);
        let vp = t.host.cam.view_proj();
        let mut best = -1.0;
        let mut bd = if t.touch { 20.0 } else { 10.0 };
        for (i, p) in g.iter().enumerate() {
            if let Some(s) = t.host.cam.project_vp(*p, &vp) {
                let d = hypot2(s[0] - ev.x, s[1] - ev.y);
                if d < bd {
                    bd = d;
                    best = i as f64;
                }
            }
        }
        best
    }
    fn add(&mut self, t: &mut Tools, p: P3) -> Res<()> {
        if self.pts.last().is_some_and(|l| dist(*l, p) <= EPS) {
            return Ok(());
        }
        self.pts.push(p);
        self.b.busy = true;
        let h = self.hint(t);
        t.host.tip(h);
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(())
    }
    fn place_grip(&mut self, t: &mut Tools, p: P3) -> Res<()> {
        let c = t.ctx();
        let (Some(cid), Some(g)) = (self.edit.clone(), self.grab) else { return Err(Error::Type) };
        let sp = t.host.model.def(&c.def).and_then(|d| d.splines.get(cid.as_str())).cloned().ok_or(Error::Type)?;
        let mut pts = sp.pts.clone();
        let lp = t.local(p);
        // pts[this.grab] = …: a grip past the end grows the array (sparse in the JS); grips come from these points
        match pts.get_mut(g) {
            Some(slot) => *slot = lp,
            None => return Err(Error::Type),
        }
        let r = t.host.model.change(&t0("m3d.tool.edit_curve"), |m| {
            let d = m.def_mut(&c.def).ok_or(Error::Type)?;
            edit_spline(d, &cid, &pts, None, None).map(|_| ())
        });
        if let Err(e) = r {
            t.host.print(tr("m3d.tool.curve_edit_failed", &[("error", &err_message(&e))]));
            return Ok(());
        }
        self.grab = None;
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        self.show_grips(t, -1.0, Overlay::default());
        Ok(())
    }
    /// Enter or a double-click: draw it (closed when the first point was clicked again).
    pub fn finish(&mut self, t: &mut Tools, closed: bool) -> Res<bool> {
        if self.edit.is_some() {
            return Ok(false);
        }
        if self.pts.len() < 2 {
            t.host.print(t0("m3d.tool.click_two_points_or_more_for"));
            return Ok(false);
        }
        let c = t.ctx();
        let pts: Vec<P3> = self.pts.iter().map(|p| t.local(*p)).collect();
        let hint = t.local_dir(t.toward_eye(self.pts.first().copied().unwrap_or([f64::NAN; 3])));
        t.host.model.change(&t0("m3d.undo.curve"), |m| {
            let d = m.def_mut(&c.def).ok_or(Error::Type)?;
            add_spline(d, &pts, closed, SPLINE_SEGS, Some(hint)).map(|_| ())
        })?;
        t.host.print(tr(if closed { "m3d.tool.curve_done_closed" } else { "m3d.tool.curve_done" }, &[("n", &nstr(pts.len() as f64))]));
        self.pts = Vec::new();
        self.b.busy = false;
        t.host.changed();
        t.host.overlay(None);
        let h = self.hint(t);
        t.host.tip(h);
        t.host.vcb(t0("m3d.vcb.length"), String::new());
        Ok(true)
    }
    /// Backspace: the last point back.
    pub fn back(&mut self, t: &mut Tools) -> Res<bool> {
        if self.edit.is_some() || self.pts.is_empty() {
            return Ok(false);
        }
        self.pts.pop();
        self.b.busy = !self.pts.is_empty();
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(true)
    }
}
impl ToolImpl for CurveTool {
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
        if self.edit.is_some() {
            return t0(if self.grab.is_some() {
                "m3d.tool.click_where_this_point_goes_or"
            } else {
                "m3d.tool.click_a_square_to_move_that"
            });
        }
        t0(if self.pts.is_empty() { "m3d.tool.click_where_the_curve_starts" } else { "m3d.tool.click_the_next_point_enter_or" })
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.length")
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.reset_();
        true
    }
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.reset_();
        Ok(())
    }
    fn escape_step(&mut self, t: &mut Tools) -> Res<bool> {
        if self.edit.is_none() || self.grab.is_none() {
            return Ok(false);
        }
        self.grab = None;
        self.show_grips(t, -1.0, Overlay::default());
        let h = self.hint(t);
        t.host.tip(h);
        Ok(true)
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        if self.edit.is_some() {
            let (Some(g), Value::Rel(p)) = (self.grab, v) else { return Ok(Ret::False) };
            let base = self.grips_w(t).get(g).copied().unwrap_or([f64::NAN; 3]);
            let q = add(base, apply_dir(&t.ctx_m(), *p));
            self.place_grip(t, q)?;
            return Ok(Ret::True);
        }
        let Some(last) = self.pts.last().copied() else {
            if let Value::Abs(p) = v {
                let w = t.world(*p);
                self.add(t, w)?;
                return Ok(Ret::True);
            }
            return Ok(Ret::False);
        };
        match v {
            Value::Abs(p) => {
                let w = t.world(*p);
                self.add(t, w)?;
                Ok(Ret::True)
            }
            Value::Rel(p) => {
                let q = add(last, apply_dir(&t.ctx_m(), *p));
                self.add(t, q)?;
                Ok(Ret::True)
            }
            Value::Len { v: vv, .. } => {
                let d = self.b.cur.as_ref().map_or([0.0; 3], |c| norm(sub(c.p, last)));
                if !truthy(len(d)) {
                    return Ok(Ret::False);
                }
                self.add(t, add(last, mul(d, *vv)))?;
                Ok(Ret::True)
            }
            _ => Ok(Ret::False),
        }
    }
    fn call(&mut self, t: &mut Tools, name: &str, args: &[Json]) -> Res<Option<Json>> {
        match name {
            "finish" => {
                let closed = args.first().is_some_and(|v| cc_m3d::json::truthy(Some(v)));
                Ok(Some(Json::Bool(self.finish(t, closed)?)))
            }
            "back" => Ok(Some(Json::Bool(self.back(t)?))),
            _ => Ok(None),
        }
    }
    fn state(&self) -> Json {
        serde_json::json!({"pts": crate::outj::pts(&self.pts), "edit": self.edit, "grab": self.grab})
    }
}
impl ClickTool for CurveTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        if let Some(cid) = self.edit.clone() {
            let Some(g) = self.grab else {
                let hot = self.grip_at(t, ev);
                self.show_grips(t, hot, Overlay::default());
                return Ok(());
            };
            let from = self.grips_w(t).get(g).copied();
            let c = t.ctx();
            let own: Vec<f64> = t
                .host
                .model
                .def(&c.def)
                .map(|d| d.edges.iter().filter(|(_, e)| e.curve.as_deref() == Some(cid.as_str())).map(|(id, _)| id).collect())
                .unwrap_or_default();
            let skip = move |o: ObjRef<'_>, k: char| k == 'e' && own.contains(&o.id) && o.path.is_empty();
            let r = t.snap(ev, SnapOpts { from, skip: Some(&skip), ..SnapOpts::default() });
            self.b.last_dir = r.dir;
            self.b.last_kind = Some(r.kind.clone());
            let mut gw = self.grips_w(t);
            if let Some(slot) = gw.get_mut(g) {
                *slot = r.p;
            }
            let sp = t.host.model.def(&c.def).and_then(|d| d.splines.get(cid.as_str())).cloned().ok_or(Error::Type)?;
            let s = smooth_pts(&gw, sp.closed, sp.segs);
            let mut lines = polyline(&s, "arc");
            if sp.closed
                && let (Some(a), Some(b)) = (s.last(), s.first())
            {
                lines.push(Line::new(*a, *b, "arc"));
            }
            let extra = Overlay { lines: Some(lines), marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() };
            self.b.cur = Some(r);
            self.show_grips(t, g as f64, extra);
            return Ok(());
        }
        let last = self.pts.last().copied();
        let r = t.snap(ev, SnapOpts { from: last, ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        let mut o = Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() };
        if let Some(l) = last {
            let mut all = self.pts.clone();
            all.push(r.p);
            let s = smooth_pts(&all, false, SPLINE_SEGS);
            o.lines = Some(polyline(&s, "arc"));
            o.points = Some(self.pts.clone());
            let txt = t.host.fmt(dist(l, r.p));
            t.host.vcb(t0("m3d.vcb.length"), txt);
        }
        self.b.cur = Some(r);
        t.host.overlay(Some(o));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.edit.is_some() {
            if self.grab.is_none() {
                let i = self.grip_at(t, ev);
                if i >= 0.0 {
                    self.grab = Some(i as usize);
                    let h = self.hint(t);
                    t.host.tip(h);
                }
                return Ok(());
            }
            let r = snap_or(t, &self.b.cur, ev);
            return self.place_grip(t, r.p);
        }
        let r = snap_or(t, &self.b.cur, ev);
        if ev.count >= 2.0 && self.pts.len() >= 2 {
            self.finish(t, false)?;
            return Ok(());
        }
        if self.pts.len() >= 3
            && let Some(s0) = self.pts.first().and_then(|p| t.host.cam.project(*p))
            && hypot2(s0[0] - ev.x, s0[1] - ev.y) < 8.0
        {
            self.finish(t, true)?;
            return Ok(());
        }
        self.add(t, r.p)
    }
}

// ---- sculpt terrain (0.4 M24) ----

#[derive(Clone, Debug)]
struct Brush {
    v: f64,
    w: f64,
    z: f64,
}
#[derive(Clone, Debug)]
struct Drag {
    y: f64,
    w: P3,
    pts: Vec<Brush>,
    dz: f64,
}

#[derive(Clone, Debug)]
pub struct SculptTool {
    pub b: Base,
    radius: f64,
    pending: Option<Drag>,
    drag: Option<Drag>,
}
impl SculptTool {
    pub fn new(name: &str) -> SculptTool {
        SculptTool { b: Base::new(name), radius: 2.0, pending: None, drag: None }
    }
    /// The open context's face under the pointer and the point there: (local, world).
    fn at(&self, t: &mut Tools, ev: &Ev) -> Option<(P3, P3)> {
        let s = t.pick_at(ev)?;
        if s.t != "f" {
            return None;
        }
        let ray = t.host.cam.ray(ev.x, ev.y);
        let c = t.ctx();
        let def = t.host.model.def(&c.def)?;
        let f = def.faces.get(s.id)?;
        let p0 = apply(&c.m, f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |v| def.v(*v)));
        let n = norm(apply_dir(&c.m, face_normal(def, f)));
        let (_, g) = ray_plane(ray.o, ray.d, &plane_from(n, p0))?;
        Some((apply(&c.inv, g), g))
    }
    /// The vertices the brush moves around local point p, with their weights and heights now.
    fn brush(&self, t: &Tools, p: P3) -> Vec<Brush> {
        let c = t.ctx();
        let Some(def) = t.host.model.def(&c.def) else { return Vec::new() };
        let rr = self.radius * crate::js::or(len(t.local_dir([1.0, 0.0, 0.0])), 1.0);
        let is_tri = |f: &cc_m3d::def::Face| f.loops.len() == 1 && f.loops.first().is_some_and(|l| l.len() == 3);
        let mut tri = cc_m3d::omap::IdSet::new();
        let mut not_tri = cc_m3d::omap::IdSet::new();
        for f in def.faces.values() {
            if is_tri(f) {
                for v in f.loops.iter().flatten() {
                    tri.add(*v);
                }
            }
        }
        for f in def.faces.values() {
            if !is_tri(f) {
                for v in f.loops.iter().flatten() {
                    not_tri.add(*v);
                }
            }
        }
        let mut out = Vec::new();
        for (v, q) in def.verts.iter() {
            let d = hypot2(q[0] - p[0], q[1] - p[1]);
            if d >= rr || not_tri.has(v) || !tri.has(v) {
                continue;
            }
            out.push(Brush { v, w: 0.5 * (1.0 + cc_m3d::jsmath::fd_cos((std::f64::consts::PI * d) / rr)), z: q[2] });
        }
        out
    }
    fn ring(&self, t: &Tools, w: P3) -> Vec<P3> {
        let ax = t.axes();
        let (u, _) = plane_axes(ax.z, &ax);
        ngon_pts(w, ax.z, u, self.radius, 36.0)
    }
    /// Put the brush's vertices at height z + w·dz (dz None: back where they were), in place, as the JS does.
    fn set_heights(t: &mut Tools, pts: &[Brush], dz: Option<f64>) {
        let c = t.ctx();
        if let Some(def) = t.host.model.def_mut(&c.def) {
            for b in pts {
                if let Some(q) = def.verts.get(b.v).copied() {
                    def.verts.set(b.v, [q[0], q[1], dz.map_or(b.z, |d| b.z + b.w * d)]);
                }
            }
        }
        t.host.model.bump_version();
    }
    fn apply(&mut self, t: &mut Tools, d: &Drag, dz: f64) -> Res<()> {
        let path = t.host.path.clone();
        let pts = d.pts.clone();
        t.host.model.change(&t0("m3d.undo.sculpt"), |m| {
            let id = m.context(&path).ok_or(Error::Type)?.def;
            let def = m.def_mut(&id).ok_or(Error::Type)?;
            for b in &pts {
                if let Some(q) = def.verts.get(b.v).copied() {
                    def.verts.set(b.v, [q[0], q[1], b.z + b.w * dz]);
                }
            }
            Ok(())
        })?;
        let key = if dz >= 0.0 { "m3d.tool.sculpt_raised" } else { "m3d.tool.sculpt_lowered" };
        let msg = tr(key, &[("h", &t.host.fmt(dz.abs())), ("r", &t.host.fmt(self.radius))]);
        t.host.print(msg);
        self.pending = None;
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        let l = self.vcb_label(t);
        t.host.vcb(l, String::new());
        Ok(())
    }
}
impl ToolImpl for SculptTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, t: &Tools) -> String {
        if self.pending.is_some() {
            t0("m3d.tool.sculpt_height")
        } else {
            crate::i18n::t("m3d.tool.sculpt_hint", &[("r", &t.host.fmt(self.radius))])
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        if self.pending.is_some() { t0("m3d.vcb.height") } else { t0("m3d.vcb.radius") }
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if let Some(mut d) = self.drag.take() {
            let dz = (d.y - ev.y) * t.host.cam.pixel_size(d.w) * crate::js::or(len(t.local_dir([0.0, 0.0, 1.0])), 1.0);
            d.dz = dz;
            Self::set_heights(t, &d.pts, Some(dz));
            t.host.vcb(t0("m3d.vcb.height"), t.host.fmt(dz));
            let ring = self.ring(t, d.w);
            t.host.overlay(Some(Overlay { poly: Some(ring), poly_kind: Some("axis-z".into()), ..Overlay::default() }));
            self.drag = Some(d);
            return Ok(());
        }
        let a = self.at(t, ev);
        let o = a.map(|(_, w)| Overlay { poly: Some(self.ring(t, w)), poly_kind: Some("axis-z".into()), ..Overlay::default() });
        t.host.overlay(o);
        if a.is_none() && self.pending.is_none() {
            let tip = if t.pick_at(ev).is_some_and(|s| s.t == "i") { t0("m3d.tool.sculpt_open") } else { self.hint(t) };
            t.host.tip(tip);
        }
        Ok(())
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some((p, w)) = self.at(t, ev) else {
            let key = if t.pick_at(ev).is_some_and(|s| s.t == "i") { "m3d.tool.sculpt_open" } else { "m3d.tool.sculpt_none" };
            t.host.print(t0(key));
            return Ok(());
        };
        let pts = self.brush(t, p);
        if pts.is_empty() {
            t.host.print(t0("m3d.tool.sculpt_none"));
            return Ok(());
        }
        self.drag = Some(Drag { y: ev.y, w, pts, dz: 0.0 });
        self.b.busy = true;
        Ok(())
    }
    fn up(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let d = self.drag.take();
        self.b.busy = false;
        let Some(d) = d else { return Ok(()) };
        Self::set_heights(t, &d.pts, None);
        if !ev.dragged || d.dz.abs() < 1e-9 {
            self.pending = Some(d);
            let h = self.hint(t);
            t.host.tip(h);
            let l = self.vcb_label(t);
            t.host.vcb(l, String::new());
            return Ok(());
        }
        let dz = d.dz;
        self.apply(t, &d, dz)
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        if let Some(p) = self.pending.clone() {
            let dz = vv * crate::js::or(len(t.local_dir([0.0, 0.0, 1.0])), 1.0);
            self.apply(t, &p, dz)?;
            return Ok(Ret::True);
        }
        if !(*vv > 0.0) {
            t.host.print(t0("m3d.tool.type_a_length_above_zero"));
            return Ok(Ret::False);
        }
        self.radius = *vv;
        let msg = tr("m3d.tool.sculpt_radius", &[("r", &t.host.fmt(*vv))]);
        t.host.print(msg);
        let h = self.hint(t);
        t.host.tip(h);
        Ok(Ret::True)
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        if let Some(d) = self.drag.take() {
            Self::set_heights(t, &d.pts, None);
        }
        self.pending = None;
        self.b.busy = false;
        Ok(())
    }
    fn state(&self) -> Json {
        serde_json::json!({"radius": crate::outj::num(self.radius), "pending": self.pending.is_some(), "drag": self.drag.as_ref().map(|d| crate::outj::num(d.dz))})
    }
}

// ---- drawing axes (0.4 M20) ----

#[derive(Clone, Debug)]
pub struct AxesTool {
    pub b: Base,
    pts: Vec<P3>,
}
impl AxesTool {
    pub fn new(name: &str) -> AxesTool {
        AxesTool { b: Base::new(name), pts: Vec::new() }
    }
}
impl ToolImpl for AxesTool {
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
        match self.pts.len() {
            0 => t0("m3d.tool.axes_origin"),
            1 => t0("m3d.tool.axes_red"),
            2 => t0("m3d.tool.axes_green"),
            _ => String::new(),
        }
    }
    click_tool_events!();
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.pts = Vec::new();
        self.b.busy = false;
        Ok(())
    }
    fn state(&self) -> Json {
        crate::outj::pts(&self.pts)
    }
}
impl ClickTool for AxesTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let r = t.snap(ev, SnapOpts { from: self.pts.first().copied(), ..SnapOpts::default() });
        let mut lines = Vec::new();
        if let Some(p0) = self.pts.first().copied() {
            let b = if self.pts.len() == 1 { r.p } else { self.pts.get(1).copied().unwrap_or(r.p) };
            lines.push(Line::new(p0, b, "axis-x"));
            if let (2, Some(p1)) = (self.pts.len(), self.pts.get(1)) {
                let x = norm(sub(*p1, p0));
                let q = sub(r.p, p0);
                lines.push(Line::new(p0, add(p0, sub(q, mul(x, dot(q, x)))), "axis-y"));
            }
        }
        t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), lines: Some(lines), ..Overlay::default() }));
        self.b.cur = Some(r);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = snap_or(t, &self.b.cur, ev);
        if self.pts.first().is_some_and(|p0| dist(r.p, *p0) <= EPS) {
            return Ok(());
        }
        self.pts.push(r.p);
        self.b.busy = true;
        if self.pts.len() < 3 {
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        let (o, px, py) = match self.pts.as_slice() {
            [o, px, py, ..] => (*o, *px, *py),
            _ => return Err(Error::Type),
        };
        self.pts = Vec::new();
        self.b.busy = false;
        if let Err(e) = t.host.model.change(&t0("m3d.undo.axes"), |m| m.set_axes(o, sub(px, o), sub(py, o))) {
            t.host.print(err_message(&e));
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        t.host.print(t0("m3d.tool.axes_done"));
        t.host.overlay(None);
        t.host.changed();
        t.set("select", ToolArg::None)
    }
}

// ---- section plane (0.4 M7) ----

#[derive(Clone, Debug)]
pub struct SectionTool {
    pub b: Base,
}
impl SectionTool {
    pub fn new(name: &str) -> SectionTool {
        SectionTool { b: Base::new(name) }
    }
    /// The plane under the pointer: a face's plane, else square to the axis looked along most; arrows toward you.
    fn plane(&self, t: &mut Tools, ev: &Ev) -> (P3, P3, Snap) {
        let r = t.snap(ev, SnapOpts::default());
        let n = if let Some(l) = &t.lock {
            l.dir
        } else if let Some(f) = &r.face {
            f.n
        } else {
            let z = t.host.cam.axes().z;
            let mut b = [1.0, 0.0, 0.0];
            for a in [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                if dot(a, z).abs() > dot(b, z).abs() {
                    b = a;
                }
            }
            b
        };
        let mut n = norm(n);
        if dot(n, sub(t.host.cam.eye, r.p)) < 0.0 {
            n = mul(n, -1.0);
        }
        (r.p, n, r)
    }
}
impl ToolImpl for SectionTool {
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
        t0("m3d.tool.section_hint")
    }
    click_tool_events!();
}
impl ClickTool for SectionTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let (p, n, r) = self.plane(t, ev);
        let tip = if r.face.is_some() { t0("m3d.tool.section_on_face") } else { r.tip.clone() };
        t.host.overlay(Some(Overlay { section: Some((p, n)), marker: Some(marker(&r)), tip: Some(tip), ..Overlay::default() }));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let (p, n, _) = self.plane(t, ev);
        let id = t.host.model.change(&t0("m3d.undo.section"), |m| Ok(m.add_section(p, n, "")))?;
        let name = t.host.model.sections.get(id).map(|s| s.name.clone()).unwrap_or_default();
        t.host.print(tr("m3d.tool.section_placed", &[("name", &name)]));
        t.host.overlay(None);
        t.set("select", ToolArg::None)?;
        t.host.set_sel(vec![cc_m3d::ops::Sel::new("s", id)]);
        t.host.changed();
        Ok(())
    }
}

// ---- camera tools ----

#[derive(Clone, Debug)]
pub struct CamTool {
    pub b: Base,
    last: Option<Ev>,
}
impl CamTool {
    pub fn new(name: &str) -> CamTool {
        CamTool { b: Base::new(name), last: None }
    }
}
impl ToolImpl for CamTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        match self.b.name.as_str() {
            "orbit" => t0("m3d.tool.drag_to_orbit_middle_drag_orbits"),
            "pan" => t0("m3d.tool.drag_to_pan_shift_middle_drag"),
            _ => t0("m3d.tool.drag_up_and_down_to_zoom"),
        }
    }
    fn down(&mut self, _t: &mut Tools, ev: &Ev) -> Res<()> {
        self.last = Some(*ev);
        Ok(())
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(last) = self.last else { return Ok(()) };
        let dx = ev.x - last.x;
        let dy = ev.y - last.y;
        self.last = Some(*ev);
        let cam = &mut t.host.cam;
        match self.b.name.as_str() {
            "orbit" => cam.orbit(dx, dy),
            "pan" => cam.pan(dx, dy),
            _ => cam.zoom_at(libm::exp(dy * 0.01), ev.from.map(|f| f.0), ev.from.map(|f| f.1), None),
        }
        t.host.changed();
        Ok(())
    }
    fn up(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        self.last = None;
        Ok(())
    }
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.last = None;
        Ok(())
    }
}

// ---- pick groups for the app (0.4 M9) ----

/// What the app asks the Pick tool for: a hint, and how many placements it wants (`pick(s)` → true at that count).
#[derive(Clone, Debug, Default)]
pub struct PickArg {
    pub hint: Option<String>,
    pub need: usize,
    pub picked: Vec<cc_m3d::ops::Sel>,
}

#[derive(Clone, Debug)]
pub struct PickTool {
    pub b: Base,
    pub arg: PickArg,
}
impl PickTool {
    pub fn new(name: &str, arg: ToolArg) -> PickTool {
        let arg = if let ToolArg::Pick(a) = arg { a } else { PickArg::default() };
        PickTool { b: Base::new(name), arg }
    }
    fn at(t: &mut Tools, ev: &Ev) -> Option<cc_m3d::ops::Sel> {
        t.pick_at(ev).filter(|s| s.t == "i")
    }
}
impl ToolImpl for PickTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        self.arg.hint.clone().unwrap_or_default()
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let s = Self::at(t, ev);
        t.host.overlay(Some(Overlay { hover: Some(s), many: Some(self.arg.picked.clone()), ..Overlay::default() }));
        Ok(())
    }
    fn down(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(s) = Self::at(t, ev) else {
            t.host.print(t0("m3d.tool.pick_a_group"));
            return Ok(());
        };
        if self.arg.need > 0 {
            self.arg.picked.push(s);
            if self.arg.picked.len() >= self.arg.need {
                return t.set("select", ToolArg::None);
            }
        }
        let h = self.hint(t);
        t.host.tip(h);
        self.mv(t, ev)
    }
    fn state(&self) -> Json {
        Json::Array(self.arg.picked.iter().map(|s| serde_json::json!({"t": s.t, "id": crate::outj::num(s.id)})).collect())
    }
}

// ---- place a component ----

#[derive(Clone, Debug)]
pub struct PlaceTool {
    pub b: Base,
    def_id: String,
    align: bool,
    hint_text: Option<String>,
}
impl PlaceTool {
    pub fn new(name: &str, arg: ToolArg) -> PlaceTool {
        let (def_id, align, hint_text) = match arg {
            ToolArg::Place { def_id, align, hint } => (def_id, align, hint),
            _ => (String::new(), false, None),
        };
        PlaceTool { b: Base::new(name), def_id, align, hint_text }
    }
    /// The world frame the copy gets at snap r.
    fn frame_at(&self, t: &Tools, r: &Snap) -> [P3; 3] {
        let a = t.axes();
        let Some(f) = r.face.as_ref().filter(|_| self.align) else { return [a.x, a.y, a.z] };
        let mut z = norm(f.n);
        if dot(z, sub(t.host.cam.eye, r.p)) < 0.0 {
            z = mul(z, -1.0);
        }
        let mut x = sub(a.x, mul(z, dot(a.x, z)));
        if len(x) < 1e-6 || dot(z, a.z).abs() < 0.9 {
            x = cross(a.z, z);
            if len(x) < 1e-6 {
                x = a.x;
            }
        }
        let x = norm(x);
        [x, cross(z, x), z]
    }
}
impl ToolImpl for PlaceTool {
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
        self.hint_text.clone().unwrap_or_else(|| t0("m3d.tool.click_where_the_component_goes_its"))
    }
    click_tool_events!();
}
impl ClickTool for PlaceTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = t.snap(ev, SnapOpts::default());
        let bb = t.host.model.def(&self.def_id).and_then(|_| t.host.model.def_bbox(&self.def_id));
        let mut lines = Vec::new();
        if let Some(bb) = bb {
            let cm = t.ctx_m();
            let o = r.p;
            let [fx, fy, fz] = self.frame_at(t, &r);
            let w = |i: u8| {
                let c = corner(bb.min, bb.max, i);
                if self.align { add(o, add(add(mul(fx, c[0]), mul(fy, c[1])), mul(fz, c[2]))) } else { add(o, apply_dir(&cm, c)) }
            };
            for (a, b) in BOX_EDGES {
                lines.push(Line::dashed(w(a), w(b), "axis"));
            }
        }
        t.host.overlay(Some(Overlay { lines: Some(lines), marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
        self.b.cur = Some(r);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = snap_or(t, &self.b.cur, ev);
        let p = t.local(r.p);
        let mut m: M4 = translation(p);
        if self.align {
            let fr = self.frame_at(t, &r).map(|v| norm(t.local_dir(v)));
            let [fx, fy, fz] = fr;
            m = [fx[0], fx[1], fx[2], 0.0, fy[0], fy[1], fy[2], 0.0, fz[0], fz[1], fz[2], 0.0, p[0], p[1], p[2], 1.0];
        }
        let path = t.host.path.clone();
        let def_id = self.def_id.clone();
        let iid = t.host.model.change(&t0("m3d.tool.place_component"), |mm| {
            let id = mm.context(&path).ok_or(Error::Type)?.def;
            let d = mm.def_mut(&id).ok_or(Error::Type)?;
            Ok(place(d, &def_id, &m))
        })?;
        t.host.set_sel(vec![cc_m3d::ops::Sel::new("i", iid)]);
        t.host.changed();
        t.set("select", ToolArg::None)
    }
}

// ---- light (0.5 RC.11 (d)) ----

#[derive(Clone, Debug)]
pub struct LightTool {
    pub b: Base,
    o: Json,
    at: Option<(P3, Option<P3>)>,
}
impl LightTool {
    pub fn new(name: &str, arg: ToolArg) -> LightTool {
        let o = match arg {
            ToolArg::Light(j) if j.is_object() => j,
            _ => serde_json::json!({"kind": "point"}),
        };
        LightTool { b: Base::new(name), o, at: None }
    }
    fn kind(&self) -> String {
        match self.o.get("kind") {
            Some(Json::String(s)) => s.clone(),
            Some(Json::Null) | None => "undefined".into(),
            Some(v) => v.to_string(),
        }
    }
    /// The lamp's point for a snap: off the face toward the eye.
    fn spot(&self, t: &Tools, r: &Snap) -> (P3, Option<P3>) {
        let mut n = r.face.as_ref().map(|f| norm(f.n));
        if let Some(nn) = n
            && dot(nn, sub(t.host.cam.eye, r.p)) < 0.0
        {
            n = Some(mul(nn, -1.0));
        }
        let k = self.kind();
        let off = if k == "point" {
            0.25
        } else if k == "spot" {
            0.05
        } else {
            0.005
        };
        (n.map_or(r.p, |n| add(r.p, mul(n, off))), n)
    }
}
impl ToolImpl for LightTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        if self.at.is_some() {
            t0("m3d.tool.light_aim")
        } else {
            tr("m3d.tool.light_where", &[("kind", &t0(&format!("m3d.light.{}", self.kind())))])
        }
    }
    click_tool_events!();
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.at = None;
        self.b.busy = false;
        Ok(())
    }
}
impl ClickTool for LightTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let r = t.snap(ev, SnapOpts::default());
        let lines = match &self.at {
            Some((p, _)) => vec![Line::dashed(*p, r.p, "axis")],
            None => Vec::new(),
        };
        let tip = if self.at.is_some() { t0("m3d.tool.light_aim") } else { r.tip.clone() };
        t.host.overlay(Some(Overlay { lines: Some(lines), marker: Some(marker(&r)), tip: Some(tip), ..Overlay::default() }));
        self.b.cur = Some(r);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = snap_or(t, &self.b.cur, ev);
        if self.kind() == "spot" && self.at.is_none() {
            self.at = Some(self.spot(t, &r));
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        let mut l = self.o.as_object().cloned().unwrap_or_default();
        if let Some((p, _)) = self.at {
            let d = sub(r.p, p);
            l.insert("kind".into(), Json::String("spot".into()));
            l.insert("p".into(), crate::outj::p3(&p));
            l.insert("dir".into(), crate::outj::p3(&if len(d) > 1e-6 { norm(d) } else { [0.0, 0.0, -1.0] }));
        } else {
            let (p, n) = self.spot(t, &r);
            l.insert("p".into(), crate::outj::p3(&p));
            let dir = if self.kind() == "area" { n.unwrap_or([0.0, 0.0, -1.0]) } else { [0.0, 0.0, -1.0] };
            l.insert("dir".into(), crate::outj::p3(&dir));
        }
        let lj = Json::Object(l);
        let id = t.host.model.change(&t0("m3d.undo.light"), |m| m.add_light(&lj))?;
        self.at = None;
        self.b.busy = false;
        t.host.changed();
        t.host.light_placed = Some(id);
        t.set("select", ToolArg::None)
    }
}

// ---- paste ----

#[derive(Clone, Debug)]
pub struct PasteTool {
    pub b: Base,
    clip: Json,
}
impl PasteTool {
    pub fn new(name: &str, arg: ToolArg) -> PasteTool {
        let clip = if let ToolArg::Paste(c) = arg { c } else { Json::Null };
        PasteTool { b: Base::new(name), clip }
    }
    fn placement(&self, p: P3) -> Res<M4> {
        let a = self.clip.get("m").and_then(Json::as_array).ok_or(Error::Type)?;
        let mut m = IDENT;
        for (i, s) in m.iter_mut().enumerate() {
            // m.slice(): a shorter array leaves undefined (NaN in arithmetic)
            *s = a.get(i).and_then(Json::as_f64).unwrap_or(f64::NAN);
        }
        m[12] = p[0];
        m[13] = p[1];
        m[14] = p[2];
        Ok(m)
    }
}
impl ToolImpl for PasteTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        t0("m3d.tool.click_where_the_pasted_copy_goes")
    }
    click_tool_events!();
}
impl ClickTool for PasteTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let r = t.snap(ev, SnapOpts::default());
        self.b.cur = Some(r.clone());
        let mut lines = Vec::new();
        // `this.clip.box`: no clipboard is a TypeError in the JS
        if !self.clip.is_object() {
            return Err(Error::Type);
        }
        let bb = self.clip.get("box").filter(|b| b.is_object());
        if let Some(bb) = bb {
            let m = self.placement(r.p)?;
            let mn = super::p3_of(bb.get("min")).unwrap_or([f64::NAN; 3]);
            let mx = super::p3_of(bb.get("max")).unwrap_or([f64::NAN; 3]);
            for (a, b) in BOX_EDGES {
                lines.push(Line::dashed(apply(&m, corner(mn, mx, a)), apply(&m, corner(mn, mx, b)), "axis"));
            }
        }
        t.host.overlay(Some(Overlay { lines: Some(lines), marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
        self.b.cur = Some(r);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = snap_or(t, &self.b.cur, ev);
        let m = self.placement(r.p)?;
        let path = t.host.path.clone();
        let mj = Json::Array(m.iter().map(|x| cc_m3d::file::jn(*x)).collect());
        let clip = self.clip.clone();
        let made = match t.host.model.change(&t0("m3d.undo.paste"), |mm| mm.paste_clip(&path, &clip, &mj)) {
            Ok(m) => m,
            Err(e) => {
                t.host.print(tr("m3d.tool.paste_failed", &[("error", &err_message(&e))]));
                return t.set("select", ToolArg::None);
            }
        };
        t.host.set_sel(made);
        t.host.changed();
        t.host.print("Pasted.".into());
        t.set("select", ToolArg::None)
    }
}

#[allow(dead_code)]
fn _unused(_: Ret) {}
