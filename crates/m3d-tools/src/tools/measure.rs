//! Measuring tools: Tape measure (guide lines and points), Protractor (a guide line at an angle) and Dimension
//! (linear, radius / diameter, angle; typing a length right after resizes the model through it).

use std::rc::Rc;

use cc_m3d::Error;
use cc_m3d::jsmath::{fd_cos, fd_sin};
use cc_m3d::model::{Dim, Guide, RadC, Radial};
use cc_m3d::vec::{EPS, P3, add, cross, dist, dot, len, mul, norm, plane_from, ray_plane, sub};

use super::{Base, ClickTool, Ev, Res, Ret, SnapOpts, ToolImpl, Tools, click_tool_events, err_message, marker, plane_axes};
use crate::i18n::{n as nstr, t as tr, t0};
use crate::infer::{Snap, angle_layout, format_deg};
use crate::js::{jmax, round, truthy};
use crate::overlay::{DimOv, Line, Overlay};
use crate::scene::SEdge;
use crate::units::{Value, parse_float};

fn snap_or(t: &mut Tools, cur: &Option<Snap>, ev: &Ev) -> Snap {
    match cur {
        Some(c) => c.clone(),
        None => t.snap(ev, SnapOpts::default()),
    }
}

// ---- tape measure ----

#[derive(Clone, Debug)]
pub struct TapeTool {
    pub b: Base,
    p0: Option<P3>,
    /// "line" (from an edge or guide line) or "point"
    mode: Option<&'static str>,
    q: Option<P3>,
    dir: P3,
    w: P3,
}
impl TapeTool {
    pub fn new(name: &str) -> TapeTool {
        TapeTool { b: Base::new(name), p0: None, mode: None, q: None, dir: [f64::NAN; 3], w: [0.0; 3] }
    }
    fn reset_(&mut self) {
        self.p0 = None;
        self.b.busy = false;
        self.mode = None;
        self.q = None;
    }
    fn make(&mut self, t: &mut Tools) -> Res<()> {
        let p0 = self.p0.unwrap_or([f64::NAN; 3]);
        if self.mode == Some("line") {
            if len(self.w) <= EPS {
                return Ok(());
            }
            let (p, dir) = (add(p0, self.w), self.dir);
            t.host.model.change(&t0("m3d.tool.guide_line"), |mm| Ok(mm.add_guide_line(p, dir)))?;
            let msg = tr("m3d.tool.guide_from_edge", &[("d", &t.host.fmt(len(self.w)))]);
            t.host.print(msg);
        } else {
            let Some(q) = self.q.filter(|q| dist(*q, p0) > EPS) else { return Ok(()) };
            t.host.model.change(&t0("m3d.tool.guide_point"), |mm| Ok(mm.add_guide_point(q)))?;
            let msg = format!("Measured {}.", t.host.fmt(dist(p0, q)));
            t.host.print(msg);
        }
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for TapeTool {
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
        if self.p0.is_none() {
            t0("m3d.tool.click_an_edge_to_start_a")
        } else if self.mode == Some("line") {
            t0("m3d.tool.move_off_the_edge_and_click")
        } else {
            t0("m3d.tool.click_to_measure_and_drop_a")
        }
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
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let (Some(p0), Value::Len { v: vv, .. }) = (self.p0, v) else { return Ok(Ret::False) };
        if self.mode == Some("line") {
            let d = norm(self.w);
            if !truthy(len(d)) {
                t.host.print(t0("m3d.tool.move_off_the_edge_first_to"));
                return Ok(Ret::False);
            }
            self.w = mul(d, *vv);
        } else {
            let d = self.q.map_or([0.0; 3], |q| norm(sub(q, p0)));
            if !truthy(len(d)) {
                return Ok(Ret::False);
            }
            self.q = Some(add(p0, mul(d, *vv)));
        }
        self.make(t)?;
        Ok(Ret::True)
    }
    fn state(&self) -> serde_json::Value {
        use crate::outj::{opt_p3, p3};
        serde_json::json!({"p0": opt_p3(self.p0.as_ref()), "mode": self.mode, "q": opt_p3(self.q.as_ref()), "w": p3(&self.w)})
    }
}
impl ClickTool for TapeTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(p0) = self.p0 else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        if self.mode == Some("line") {
            let r = t.snap(ev, SnapOpts::default());
            let w0 = sub(r.p, p0);
            self.w = sub(w0, mul(self.dir, dot(w0, self.dir)));
            let l = t.host.cam.pixel_size(p0) * 4000.0;
            let base = add(p0, self.w);
            let txt = t.host.fmt(len(self.w));
            t.host.overlay(Some(Overlay {
                lines: Some(vec![
                    Line::dashed(add(base, mul(self.dir, -l)), add(base, mul(self.dir, l)), "guide"),
                    Line::dashed(p0, base, "axis"),
                ]),
                marker: Some(marker(&r)),
                tip: Some(txt.clone()),
                ..Overlay::default()
            }));
            t.host.vcb(t0("m3d.vcb.offset"), txt);
            return Ok(());
        }
        let r = t.snap(ev, SnapOpts { from: Some(p0), ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        self.q = Some(r.p);
        let txt = t.host.fmt(dist(p0, r.p));
        t.host.overlay(Some(Overlay {
            lines: Some(vec![Line::dashed(p0, r.p, &r.kind)]),
            points: Some(vec![r.p]),
            marker: Some(marker(&r)),
            tip: Some(txt.clone()),
            ..Overlay::default()
        }));
        t.host.vcb(t0("m3d.vcb.length"), txt);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.p0.is_none() {
            let r = snap_or(t, &self.b.cur, ev);
            self.p0 = Some(r.p);
            if r.kind == "edge"
                && let Some(e) = &r.edge
            {
                self.mode = Some("line");
                self.dir = norm(sub(e.b, e.a));
            } else if r.kind == "guide" {
                self.mode = Some("line");
                let g = r.guide.and_then(|id| t.host.model.guides.get(id));
                self.dir = match g {
                    Some(Guide::Line { d, .. }) => *d,
                    Some(Guide::Point { .. }) | None => return Err(Error::Type),
                };
            } else {
                self.mode = Some("point");
            }
            self.w = [0.0; 3];
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        self.make(t)
    }
}

// ---- protractor (0.4 M14) ----

#[derive(Clone, Debug)]
pub struct ProtractorTool {
    pub b: Base,
    c: Option<P3>,
    n0: Option<P3>,
    u: Option<P3>,
    a: f64,
}
impl ProtractorTool {
    pub fn new(name: &str) -> ProtractorTool {
        ProtractorTool { b: Base::new(name), c: None, n0: None, u: None, a: 0.0 }
    }
    fn reset_(&mut self) {
        self.c = None;
        self.n0 = None;
        self.u = None;
        self.a = 0.0;
        self.b.busy = false;
    }
    fn n(&self, t: &Tools) -> P3 {
        match &t.lock {
            Some(l) if l.kind.starts_with("axis") => norm(l.dir),
            _ => self.n0.unwrap_or([f64::NAN; 3]),
        }
    }
    fn dial(&self, t: &Tools, lines: Vec<Line>, mk: Option<Option<crate::overlay::Marker>>, tip: Option<String>) -> Overlay {
        let c = self.c.unwrap_or([f64::NAN; 3]);
        let rr = t.host.cam.pixel_size(c) * 60.0;
        let n = self.n(t);
        let u = self.u.unwrap_or_else(|| plane_axes(n, &t.axes()).0);
        let v = cross(n, u);
        let pts: Vec<P3> = (0..=36)
            .map(|i| {
                let tt = (f64::from(i) / 36.0) * std::f64::consts::PI * 2.0;
                add(c, add(mul(u, rr * fd_cos(tt)), mul(v, rr * fd_sin(tt))))
            })
            .collect();
        let mut all: Vec<Line> =
            pts.windows(2).filter_map(|w| if let [p, q] = w { Some(Line::dashed(*p, *q, "axis")) } else { None }).collect();
        all.extend(lines);
        Overlay { lines: Some(all), marker: mk, tip, ..Overlay::default() }
    }
    fn dir_at(&self, t: &Tools, a: f64) -> P3 {
        let n = self.n(t);
        let u = self.u.unwrap_or([f64::NAN; 3]);
        let v = cross(n, u);
        norm(add(mul(u, fd_cos(a)), mul(v, fd_sin(a))))
    }
    fn make(&mut self, t: &mut Tools, a: f64) -> Res<()> {
        let d = self.dir_at(t, a);
        let c = self.c.unwrap_or([f64::NAN; 3]);
        t.host.model.change(&t0("m3d.tool.guide_line"), |mm| Ok(mm.add_guide_line(c, d)))?;
        t.host.print(tr("m3d.tool.guide_at_angle", &[("a", &nstr(round((a * 180.0) / std::f64::consts::PI * 100.0) / 100.0))]));
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for ProtractorTool {
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
        if self.c.is_none() {
            t0("m3d.tool.click_the_center_of_the_protractor")
        } else if self.u.is_none() {
            t0("m3d.tool.click_a_point_for_0_arrow")
        } else {
            t0("m3d.tool.move_to_the_angle_15_steps")
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.angle")
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
    fn value(&mut self, t: &mut Tools, v: &Value, text: &str) -> Res<Ret> {
        if self.u.is_none() {
            return Ok(Ret::False);
        }
        let deg = match v {
            Value::Num(x) => *x,
            Value::Len { num, .. } => num.unwrap_or_else(|| parse_float(text)),
            _ => f64::NAN,
        };
        if !deg.is_finite() {
            return Ok(Ret::False);
        }
        let a = ((if self.a < 0.0 { -1.0 } else { 1.0 }) * deg.abs() * std::f64::consts::PI) / 180.0;
        self.make(t, a)?;
        Ok(Ret::True)
    }
    fn state(&self) -> serde_json::Value {
        use crate::outj::{num, opt_p3};
        serde_json::json!({"c": opt_p3(self.c.as_ref()), "u": opt_p3(self.u.as_ref()), "n0": opt_p3(self.n0.as_ref()), "a": num(self.a)})
    }
}
impl ClickTool for ProtractorTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(c) = self.c else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        let n = self.n(t);
        let pl = plane_from(n, c);
        let Some(u) = self.u else {
            // the arrow keys pick the protractor's plane here, not a direction: snap without the lock
            let r = if t.lock.is_some() {
                t.snap(ev, SnapOpts { plane: Some(pl), ..SnapOpts::default() })
            } else {
                t.snap(ev, SnapOpts { from: Some(c), plane: Some(pl), ..SnapOpts::default() })
            };
            let q = sub(r.p, mul(n, dot(sub(r.p, c), n)));
            let o = self.dial(t, vec![Line::new(c, q, "axis")], Some(marker(&r)), Some(r.tip.clone()));
            let mut cur = r;
            cur.p = q;
            self.b.cur = Some(cur);
            t.host.overlay(Some(o));
            return Ok(());
        };
        let ray = t.host.cam.ray(ev.x, ev.y);
        let Some((_, gp)) = ray_plane(ray.o, ray.d, &pl) else { return Ok(()) };
        let w = sub(gp, c);
        if len(w) <= EPS {
            return Ok(());
        }
        let pi = std::f64::consts::PI;
        let mut a = libm::atan2(dot(cross(u, norm(w)), n), dot(u, norm(w)));
        let step = pi / 12.0;
        if (a - round(a / step) * step).abs() < 0.035 {
            a = round(a / step) * step;
        }
        self.a = a;
        let d = self.dir_at(t, a);
        let l = t.host.cam.pixel_size(c) * 4000.0;
        let deg = round((a * 180.0) / pi * 100.0) / 100.0;
        let o = self.dial(
            t,
            vec![Line::dashed(add(c, mul(d, -l)), add(c, mul(d, l)), "guide"), Line::new(c, add(c, mul(u, len(w))), "axis")],
            None,
            Some(format!("{}°", nstr(deg))),
        );
        t.host.overlay(Some(o));
        t.host.vcb(t0("m3d.vcb.angle"), nstr(deg));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(c) = self.c else {
            let r = snap_or(t, &self.b.cur, ev);
            self.c = Some(r.p);
            let on = ["face", "center", "end", "mid", "edge"].contains(&r.kind.as_str());
            self.n0 = Some(match r.face.as_ref().filter(|_| on) {
                Some(f) => f.n,
                None => t.axes().z,
            });
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            t.host.vcb(t0("m3d.vcb.angle"), String::new());
            return Ok(());
        };
        if self.u.is_none() {
            let Some(q) = self.b.cur.as_ref().map(|r| r.p).filter(|q| dist(*q, c) > EPS) else { return Ok(()) };
            self.n0 = Some(self.n(t));
            t.lock = None;
            self.u = Some(norm(sub(q, c)));
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        let a = self.a;
        self.make(t, a)
    }
}

// ---- dimension ----

#[derive(Clone, Debug)]
struct RoundAt {
    path: Vec<f64>,
    cid: String,
    kind: &'static str,
    g: Radial,
}
#[derive(Clone, Copy, Debug)]
struct Angle {
    o: P3,
    a: P3,
    b: P3,
}

#[derive(Clone, Debug)]
pub struct DimTool {
    pub b: Base,
    stage: u8,
    pa: Option<P3>,
    pb: Option<P3>,
    off: P3,
    edge1: Option<Rc<SEdge>>,
    rad: Option<RoundAt>,
    ang: Option<Angle>,
    last: Option<f64>,
}
impl DimTool {
    pub fn new(name: &str) -> DimTool {
        DimTool { b: Base::new(name), stage: 0, pa: None, pb: None, off: [0.0; 3], edge1: None, rad: None, ang: None, last: None }
    }
    fn reset_(&mut self) {
        self.stage = 0;
        self.b.busy = false;
        self.pa = None;
        self.pb = None;
        self.off = [0.0; 3];
        self.edge1 = None;
        self.rad = None;
        self.ang = None;
    }
    /// The round curve (arc, circle, polygon) an edge snap is on.
    fn round_at(&self, t: &Tools, r: &Snap) -> Option<RoundAt> {
        if r.kind != "edge" {
            return None;
        }
        let e = r.edge.as_ref()?;
        let ctx = t.host.model.context(&e.path)?;
        let curve = t.host.model.def(&ctx.def)?.edges.get(e.id)?.curve.clone().filter(|c| !c.is_empty())?;
        if !(curve.starts_with("arc:") || curve.starts_with("circle:") || curve.starts_with("polygon:")) {
            return None;
        }
        let mut c = RadC { p: None, r: f64::NAN, n: None, at: Some((e.path.to_vec(), curve.clone())) };
        let g = t.host.model.radial_at(&mut c)?;
        if !g.live {
            return None;
        }
        Some(RoundAt { path: e.path.to_vec(), kind: if curve.starts_with("arc:") { "rad" } else { "dia" }, cid: curve, g })
    }
    /// The angle between the first edge and edge snap r when they meet at a corner and are not in line.
    fn angle_with(&self, r: &Snap) -> Option<Angle> {
        let e1 = self.edge1.as_ref()?;
        if r.kind != "edge" {
            return None;
        }
        let e2 = r.edge.as_ref()?;
        let same = |p: P3, q: P3| dist(p, q) <= EPS * 100.0;
        if (same(e2.a, e1.a) && same(e2.b, e1.b)) || (same(e2.a, e1.b) && same(e2.b, e1.a)) {
            return None;
        }
        for (o, a) in [(e1.a, e1.b), (e1.b, e1.a)] {
            for (o2, b) in [(e2.a, e2.b), (e2.b, e2.a)] {
                if !same(o, o2) {
                    continue;
                }
                if len(cross(norm(sub(a, o)), norm(sub(b, o)))) < 1e-6 {
                    return None;
                }
                return Some(Angle { o, a, b });
            }
        }
        None
    }
    /// Where the pointer meets the plane through p square to n (or, side-on, the plane square to the view).
    fn on_plane(t: &Tools, ev: &Ev, n0: P3, p: P3) -> Option<P3> {
        let ray = t.host.cam.ray(ev.x, ev.y);
        let n = if dot(ray.d, n0).abs() < 0.05 { t.host.cam.axes().z } else { n0 };
        ray_plane(ray.o, ray.d, &plane_from(n, p)).map(|g| g.1)
    }
    fn next(&mut self, t: &mut Tools, stage: u8) {
        self.stage = stage;
        self.b.busy = true;
        let h = self.hint(t);
        t.host.tip(h);
        let l = self.vcb_label(t);
        t.host.vcb(l, String::new());
    }
    fn done(&mut self, t: &mut Tools, id: f64) {
        self.last = Some(id);
        self.reset_();
        t.host.overlay(None);
        t.host.changed();
        t.host.vcb(t0("m3d.vcb.length"), String::new());
        let h = self.hint(t);
        t.host.tip(h);
    }
    fn make(&mut self, t: &mut Tools) -> Res<()> {
        let (pa, pb, off) = (self.pa.unwrap_or([f64::NAN; 3]), self.pb.unwrap_or([f64::NAN; 3]), self.off);
        let path = t.host.path.clone();
        let id = t.host.model.change(&t0("m3d.undo.dimension"), |m| Ok(m.add_dim(pa, pb, off, &path)))?;
        let anchored = matches!(t.host.model.dims.get(id), Some(Dim::Lin { a, b, .. }) if a.at.is_some() && b.at.is_some());
        let key = if anchored { "m3d.tool.dim_made" } else { "m3d.tool.dim_made_free" };
        let msg = tr(key, &[("len", &t.host.fmt(dist(pa, pb)))]);
        t.host.print(msg);
        self.done(t, id);
        Ok(())
    }
    fn make_radial(&mut self, t: &mut Tools) -> Res<()> {
        let Some(rd) = self.rad.clone() else { return Err(Error::Type) };
        let off = self.off;
        let dia = rd.kind == "dia";
        let id = t.host.model.change(&t0(if dia { "m3d.undo.dim_dia" } else { "m3d.undo.dim_rad" }), |m| {
            m.add_radial_dim(&rd.path, &rd.cid, off, rd.kind)
        })?;
        let key = match (rd.g.flat, dia) {
            (true, true) => "m3d.tool.dim_made_dia",
            (true, false) => "m3d.tool.dim_made_rad",
            (false, true) => "m3d.tool.dim_made_dia_fixed",
            (false, false) => "m3d.tool.dim_made_rad_fixed",
        };
        let msg = tr(key, &[("len", &t.host.fmt(if dia { 2.0 * rd.g.r } else { rd.g.r }))]);
        t.host.print(msg);
        self.done(t, id);
        Ok(())
    }
    fn make_angle(&mut self, t: &mut Tools) -> Res<()> {
        let Some(an) = self.ang else { return Err(Error::Type) };
        let off = self.off;
        let path = t.host.path.clone();
        let id = t.host.model.change(&t0("m3d.undo.dim_angle"), |m| Ok(m.add_angle_dim(an.o, an.a, an.b, off, &path)))?;
        let mut dm = t.host.model.dims.get(id).cloned().ok_or(Error::Type)?;
        let ends = t.host.model.dim_ends(&mut dm);
        t.host.model.dims.set(id, dm);
        let g = |i: usize| ends.get(i).copied().unwrap_or([f64::NAN; 3]);
        let vp = t.host.cam.view_proj();
        let l = angle_layout(g(0), g(1), g(2), off, &t.host.cam, &vp);
        t.host.print(tr("m3d.tool.dim_made_angle", &[("deg", &format_deg((l.l * 180.0) / std::f64::consts::PI))]));
        self.done(t, id);
        Ok(())
    }
}
impl ToolImpl for DimTool {
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
        if self.stage == 2 && self.edge1.is_some() {
            return t0("m3d.tool.dim_place_or_angle");
        }
        if self.stage == 3 {
            return t0(if self.rad.as_ref().is_some_and(|r| r.kind == "dia") {
                "m3d.tool.dim_place_dia"
            } else {
                "m3d.tool.dim_place_rad"
            });
        }
        if self.stage == 4 {
            return t0("m3d.tool.dim_place_angle");
        }
        match self.stage {
            0 => t0(if self.last.is_some() { "m3d.tool.dim_start_resize" } else { "m3d.tool.dim_start" }),
            1 => t0("m3d.tool.click_the_second_corner"),
            _ => t0("m3d.tool.move_to_place_the_dimension_line"),
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        if self.stage == 3 {
            return t0(if self.rad.as_ref().is_some_and(|r| r.kind == "dia") { "m3d.vcb.diameter" } else { "m3d.vcb.radius" });
        }
        if self.stage == 4 {
            return t0("m3d.vcb.radius");
        }
        if self.stage == 2 { t0("m3d.vcb.offset") } else { t0("m3d.vcb.length") }
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
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        let vv = *vv;
        if self.stage == 2 {
            let d = norm(self.off);
            if !truthy(len(d)) {
                t.host.print(t0("m3d.tool.move_the_pointer_first_to_show"));
                return Ok(Ret::False);
            }
            self.off = mul(d, vv.abs());
            self.make(t)?;
            return Ok(Ret::True);
        }
        if self.stage == 4 {
            if !(vv > 0.0) || !truthy(len(self.off)) {
                t.host.print(t0("m3d.tool.move_the_pointer_first_to_show"));
                return Ok(Ret::False);
            }
            self.off = mul(norm(self.off), vv);
            self.make_angle(t)?;
            return Ok(Ret::True);
        }
        if self.stage == 3 {
            // place it where it is, then give the curve the typed radius (or diameter)
            if !(vv > 0.0) {
                t.host.print(t0("m3d.tool.type_a_length_above_zero"));
                return Ok(Ret::False);
            }
            self.make_radial(t)?;
        }
        if (self.stage == 0 || self.stage == 3)
            && let Some(id) = self.last.filter(|id| t.host.model.dims.has(*id))
        {
            if !(vv > 0.0) {
                t.host.print(t0("m3d.tool.type_a_length_above_zero"));
                return Ok(Ret::False);
            }
            let kind = match t.host.model.dims.get(id) {
                Some(Dim::Ang { .. }) => "ang",
                Some(Dim::Rad { kind, .. }) if kind == "dia" => "dia",
                Some(Dim::Rad { .. }) => "rad",
                _ => "len",
            };
            if kind == "ang" {
                t.host.print(t0("m3d.model.dim_angle_only"));
                return Ok(Ret::True);
            }
            if let Err(e) = t.host.model.change(&t0("m3d.tool.dimension_length"), |m| m.set_dim_length(id, vv).map(|_| ())) {
                t.host.print(err_message(&e));
                return Ok(Ret::True);
            }
            let key = match kind {
                "rad" => "m3d.tool.rad_set",
                "dia" => "m3d.tool.dia_set",
                _ => "m3d.tool.dim_set",
            };
            let msg = tr(key, &[("len", &t.host.fmt(vv))]);
            t.host.print(msg);
            t.host.changed();
            return Ok(Ret::True);
        }
        Ok(Ret::False)
    }
    fn state(&self) -> serde_json::Value {
        use crate::outj::{opt_num, opt_p3, p3};
        serde_json::json!({"stage": self.stage, "pa": opt_p3(self.pa.as_ref()), "pb": opt_p3(self.pb.as_ref()), "off": p3(&self.off), "last": opt_num(self.last)})
    }
}
impl ClickTool for DimTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        if self.stage == 3 {
            let rd = self.rad.clone().ok_or(Error::Type)?;
            let g = &rd.g;
            let gn = g.n.unwrap_or([f64::NAN; 3]);
            if let Some(q) = Self::on_plane(t, ev, gn, g.c) {
                let w = sub(q, g.c);
                self.off = sub(w, mul(gn, dot(w, gn)));
            }
            t.host.overlay(Some(Overlay {
                dims: Some(vec![DimOv::Rad { kind: rd.kind.into(), p: g.c, r: g.r, n: g.n, off: self.off }]),
                ..Overlay::default()
            }));
            let l = self.vcb_label(t);
            let txt = t.host.fmt(if rd.kind == "dia" { 2.0 * g.r } else { g.r });
            t.host.vcb(l, txt);
            return Ok(());
        }
        if self.stage == 4 {
            let an = self.ang.ok_or(Error::Type)?;
            let n = norm(cross(sub(an.a, an.o), sub(an.b, an.o)));
            if let Some(q) = Self::on_plane(t, ev, n, an.o) {
                let w = sub(q, an.o);
                self.off = sub(w, mul(n, dot(w, n)));
            }
            t.host
                .overlay(Some(Overlay { dims: Some(vec![DimOv::Ang { o: an.o, a: an.a, b: an.b, off: self.off }]), ..Overlay::default() }));
            let l = self.vcb_label(t);
            let txt = t.host.fmt(len(self.off));
            t.host.vcb(l, txt);
            return Ok(());
        }
        if self.stage < 2 {
            let r = if self.stage > 0 {
                t.snap(ev, SnapOpts { from: self.pa, ..SnapOpts::default() })
            } else {
                t.snap(ev, SnapOpts::default())
            };
            self.b.last_dir = r.dir;
            self.b.last_kind = Some(r.kind.clone());
            let mut o = Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() };
            if self.stage == 0
                && let Some(rd) = self.round_at(t, &r)
            {
                o.tip = Some(t0(if rd.kind == "dia" { "m3d.tool.dim_tip_dia" } else { "m3d.tool.dim_tip_rad" }));
            }
            if self.stage == 1 {
                let pa = self.pa.unwrap_or([f64::NAN; 3]);
                o.lines = Some(vec![Line::dashed(pa, r.p, &r.kind)]);
                let txt = t.host.fmt(dist(pa, r.p));
                t.host.vcb(t0("m3d.vcb.length"), txt);
            }
            self.b.cur = Some(r);
            t.host.overlay(Some(o));
            return Ok(());
        }
        // stage 2, from an edge: over a second edge that meets it, the angle between them
        if self.edge1.is_some() {
            let r = t.snap(ev, SnapOpts::default());
            let an = self.angle_with(&r);
            let rp = r.p;
            self.b.cur = Some(r);
            if let Some(an) = an {
                let w = sub(rp, an.o);
                let u = norm(sub(an.a, an.o));
                let v = norm(sub(an.b, an.o));
                let off = mul(norm(add(u, v)), jmax(len(w), EPS * 1000.0));
                t.host.overlay(Some(Overlay {
                    dims: Some(vec![DimOv::Ang { o: an.o, a: an.a, b: an.b, off }]),
                    tip: Some(t0("m3d.tool.dim_tip_angle")),
                    ..Overlay::default()
                }));
                return Ok(());
            }
        }
        let (pa, pb) = (self.pa.unwrap_or([f64::NAN; 3]), self.pb.unwrap_or([f64::NAN; 3]));
        let ab = norm(sub(pb, pa));
        let z = t.host.cam.axes().z;
        let ax = t.axes();
        let fits = |a: P3, min: f64| dot(a, ab).abs() < 1e-6 && dot(a, z).abs() > min;
        let mut n: Option<P3> = if fits(ax.z, 0.25) { Some(ax.z) } else { None };
        if n.is_none() {
            for a in [ax.x, ax.y] {
                if fits(a, 0.3) && n.is_none_or(|nn| dot(a, z).abs() > dot(nn, z).abs()) {
                    n = Some(a);
                }
            }
        }
        let n = match n {
            Some(n) => n,
            None => {
                let mut w = cross(z, ab);
                if len(w) < 1e-6 {
                    w = cross(ax.z, ab);
                }
                if len(w) < 1e-6 {
                    w = cross(ax.x, ab);
                }
                norm(cross(ab, w))
            }
        };
        let ray = t.host.cam.ray(ev.x, ev.y);
        if let Some((_, gp)) = ray_plane(ray.o, ray.d, &plane_from(n, pa)) {
            let q = sub(gp, pa);
            self.off = sub(q, mul(ab, dot(q, ab)));
        }
        let tip = t.host.fmt(dist(pa, pb));
        t.host.overlay(Some(Overlay {
            dims: Some(vec![DimOv::Lin { a: pa, b: pb, off: self.off }]),
            tip: Some(tip),
            ..Overlay::default()
        }));
        let txt = t.host.fmt(len(self.off));
        t.host.vcb(t0("m3d.vcb.offset"), txt);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.stage == 3 {
            self.hover(t, ev)?;
            return self.make_radial(t);
        }
        if self.stage == 4 {
            self.hover(t, ev)?;
            return self.make_angle(t);
        }
        let r = snap_or(t, &self.b.cur, ev);
        if self.stage == 0 {
            if let Some(rd) = self.round_at(t, &r) {
                self.rad = Some(rd);
                self.next(t, 3);
                return self.hover(t, ev);
            }
            match (&r.edge, r.kind == "edge") {
                (Some(e), true) => {
                    self.pa = Some(e.a);
                    self.pb = Some(e.b);
                    self.edge1 = Some(e.clone());
                    self.next(t, 2);
                }
                _ => {
                    self.pa = Some(r.p);
                    self.next(t, 1);
                }
            }
            return Ok(());
        }
        if self.stage == 1 {
            if dist(r.p, self.pa.unwrap_or([f64::NAN; 3])) <= EPS {
                return Ok(());
            }
            self.pb = Some(r.p);
            self.next(t, 2);
            return Ok(());
        }
        if self.edge1.is_some() {
            let s2 = t.snap(ev, SnapOpts::default());
            if let Some(an) = self.angle_with(&s2) {
                self.ang = Some(an);
                self.next(t, 4);
                return self.hover(t, ev);
            }
        }
        self.hover(t, ev)?;
        self.make(t)
    }
}
