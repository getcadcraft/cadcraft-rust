//! The drawing tools: Line, Rectangle, Circle / Polygon, Arc (two ends and a bulge) and Arc from its centre.

use cc_m3d::edges::{EdgeOpts, add_edge};
use cc_m3d::ops::{add_polygon, arc_pts, ngon_pts, rect_pts, tag_curve};
use cc_m3d::vec::{EPS, P3, Plane, add, apply_dir, cross, dist, dot, len, mul, norm, plane_from, ray_plane, sub};

use super::{Base, ClickTool, Ev, Res, Ret, SnapOpts, ToolImpl, Tools, click_tool_events, marker, plane_axes};
use crate::i18n::{n as nstr, t as tr, t0};
use crate::infer::Snap;
use crate::js::{is_integer, or, round, sign, truthy};
use crate::overlay::{Line, Overlay};
use crate::units::{Value, parse_float};

fn kind_is(r: &Snap, ks: &[&str]) -> bool {
    ks.contains(&r.kind.as_str())
}

/// Edges in the open context along `pts`, one undo step, tagged as one arc.
fn draw_arc(t: &mut Tools, pts: &[P3], label: &str) -> Res<()> {
    let c = t.ctx();
    let loc: Vec<P3> = pts.iter().map(|p| t.local(*p)).collect();
    let hint = t.local_dir(t.toward_eye(pts.first().copied().unwrap_or([f64::NAN; 3])));
    t.host.model.change(label, |m| {
        let d = m.def_mut(&c.def).ok_or(cc_m3d::Error::Type)?;
        for w in loc.windows(2) {
            if let [a, b] = w {
                add_edge(d, *a, *b, EdgeOpts { no_faces: false, hint: Some(hint) });
            }
        }
        tag_curve(d, &loc, "arc", false);
        Ok(())
    })?;
    t.host.changed();
    Ok(())
}

/// The plane an arc from a to b is drawn on: the face it starts on, else level (or upright facing us when a and b
/// differ in height).
fn arc_plane(t: &Tools, a: P3, b: P3, face: Option<&Plane>) -> Plane {
    if let Some(f) = face {
        return *f;
    }
    let ax = t.axes();
    let ab = sub(b, a);
    if dot(ab, ax.z).abs() <= 1e-9 * crate::js::jmax(1.0, len(ab)) {
        let z = t.host.cam.axes().z;
        if dot(z, ax.z).abs() > 0.08 {
            return plane_from(ax.z, a);
        }
        let n = norm(cross(ab, ax.z));
        return if truthy(len(n)) { plane_from(n, a) } else { plane_from(ax.z, a) };
    }
    let n = norm(cross(ab, ax.z));
    plane_from(if truthy(len(n)) { n } else { ax.x }, a)
}

// ---- line ----

#[derive(Clone, Debug)]
pub struct LineTool {
    pub b: Base,
    start: Option<P3>,
    plane: Option<Plane>,
}
impl LineTool {
    pub fn new(name: &str) -> LineTool {
        LineTool { b: Base::new(name), start: None, plane: None }
    }
    fn to(&mut self, t: &mut Tools, p: P3) -> Res<()> {
        let Some(start) = self.start else { return Err(cc_m3d::Error::Type) };
        if dist(p, start) <= EPS {
            return Ok(());
        }
        let c = t.ctx();
        let before = t.host.model.def(&c.def).map_or(0, |d| d.faces.len());
        let (a, b) = (t.local(start), t.local(p));
        let hint = t.local_dir(t.toward_eye(p));
        let path = t.host.path.clone();
        t.host.model.change(&t0("m3d.undo.line"), |m| {
            let id = super::ctx_of(m, &path).def;
            let d = m.def_mut(&id).ok_or(cc_m3d::Error::Type)?;
            add_edge(d, a, b, EdgeOpts { no_faces: false, hint: Some(hint) });
            Ok(())
        })?;
        let after = t.ctx();
        let closed = t.host.model.def(&after.def).map_or(0, |d| d.faces.len()) > before;
        self.start = if closed { None } else { Some(p) };
        self.b.busy = !closed;
        t.host.changed();
        let h = self.hint(t);
        t.host.tip(h);
        if let Some(ev) = self.b.last_ev {
            self.hover(t, &ev)?;
        }
        Ok(())
    }
}
impl ToolImpl for LineTool {
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
        if self.start.is_some() { t0("m3d.tool.click_the_end_of_the_line") } else { t0("m3d.tool.click_the_start_of_the_line") }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.length")
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.start = None;
        self.plane = None;
        self.b.busy = false;
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Some(start) = self.start else { return Ok(Ret::False) };
        match v {
            Value::Abs(p) => {
                let w = t.world(*p);
                self.to(t, w)?;
                Ok(Ret::Undef)
            }
            Value::Rel(p) => {
                let q = add(start, apply_dir(&t.ctx_m(), *p));
                self.to(t, q)?;
                Ok(Ret::Undef)
            }
            Value::Len { v, .. } => {
                let d = self.b.cur.as_ref().map_or([0.0; 3], |c| norm(sub(c.p, start)));
                if !truthy(len(d)) {
                    return Ok(Ret::False);
                }
                self.to(t, add(start, mul(d, *v)))?;
                Ok(Ret::Undef)
            }
            _ => Ok(Ret::False),
        }
    }
    fn state(&self) -> serde_json::Value {
        serde_json::json!({"start": crate::outj::opt_p3(self.start.as_ref())})
    }
}
impl ClickTool for LineTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let r = t.snap(ev, SnapOpts { from: self.start, plane: self.plane, ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        let mut o = Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() };
        if let Some(s) = self.start {
            o.lines = Some(vec![Line::new(s, r.p, &r.kind)]);
            let txt = t.host.fmt(dist(s, r.p));
            t.host.vcb(t0("m3d.vcb.length"), txt);
        }
        self.b.cur = Some(r);
        t.host.overlay(Some(o));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let r = match &self.b.cur {
            Some(c) => c.clone(),
            None => t.snap(ev, SnapOpts::default()),
        };
        if self.start.is_none() {
            self.start = Some(r.p);
            self.plane = r.face.as_ref().map(|f| f.plane);
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.to(t, r.p)
    }
}

// ---- rectangle ----

#[derive(Clone, Debug)]
pub struct RectTool {
    pub b: Base,
    p0: Option<P3>,
    wh: Option<(f64, f64)>,
    n: P3,
    plane: Option<Plane>,
    u: P3,
    v: P3,
    on_face: bool,
}
impl RectTool {
    pub fn new(name: &str) -> RectTool {
        RectTool {
            b: Base::new(name),
            p0: None,
            wh: None,
            n: [f64::NAN; 3],
            plane: None,
            u: [f64::NAN; 3],
            v: [f64::NAN; 3],
            on_face: false,
        }
    }
    fn make(&mut self, t: &mut Tools, w: f64, h: f64) -> Res<()> {
        if w.abs() <= EPS || h.abs() <= EPS {
            return Ok(());
        }
        let p0 = self.p0.unwrap_or([f64::NAN; 3]);
        let pts: Vec<P3> = rect_pts(p0, self.u, self.v, w, h).into_iter().map(|p| t.local(p)).collect();
        let hint = t.local_dir(if self.on_face || dot(self.n, t.axes().z).abs() > 0.999 { self.n } else { t.toward_eye(p0) });
        let path = t.host.path.clone();
        t.host.model.change(&t0("m3d.undo.rectangle"), |m| {
            let id = super::ctx_of(m, &path).def;
            let d = m.def_mut(&id).ok_or(cc_m3d::Error::Type)?;
            add_polygon(d, &pts, Some(hint)).map(|_| ())
        })?;
        self.reset(t);
        t.host.changed();
        t.host.overlay(None);
        let hn = self.hint(t);
        t.host.tip(hn);
        Ok(())
    }
}
impl ToolImpl for RectTool {
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
        if self.p0.is_some() { t0("m3d.tool.click_the_opposite_corner_or_type") } else { t0("m3d.tool.click_the_first_corner_on_the") }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        t0("m3d.vcb.dimensions")
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.p0 = None;
        self.b.busy = false;
        self.wh = None;
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let Value::List(vs) = v else { return Ok(Ret::False) };
        if self.p0.is_none() || vs.len() != 2 {
            return Ok(Ret::False);
        }
        let (cw, ch) = self.wh.unwrap_or((1.0, 1.0));
        let (a, b) = (vs.first().copied().unwrap_or(f64::NAN), vs.get(1).copied().unwrap_or(f64::NAN));
        self.make(t, sign(or(cw, 1.0)) * a.abs(), sign(or(ch, 1.0)) * b.abs())?;
        Ok(Ret::Undef)
    }
    fn state(&self) -> serde_json::Value {
        serde_json::json!({"p0": crate::outj::opt_p3(self.p0.as_ref())})
    }
}
impl ClickTool for RectTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(p0) = self.p0 else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        let r = t.snap(ev, SnapOpts { from: Some(p0), plane: self.plane, ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        let mut q = sub(r.p, mul(self.n, dot(sub(r.p, p0), self.n)));
        if kind_is(&r, &["face", "none", "ground"]) {
            let ray = t.host.cam.ray(ev.x, ev.y);
            if let Some(pl) = &self.plane
                && let Some((gt, gp)) = ray_plane(ray.o, ray.d, pl)
                && gt > 0.0
            {
                q = gp;
            }
        }
        let w = dot(sub(q, p0), self.u);
        let h = dot(sub(q, p0), self.v);
        self.wh = Some((w, h));
        let pts = rect_pts(p0, self.u, self.v, w, h);
        let sq = (w.abs() - h.abs()).abs() < 1e-9 * crate::js::jmax(1.0, w.abs());
        let tip = if r.tip.is_empty() { if sq { t0("m3d.tip.square") } else { String::new() } } else { r.tip.clone() };
        t.host.overlay(Some(Overlay { poly: Some(pts), marker: Some(marker(&r)), tip: Some(tip), ..Overlay::default() }));
        let txt = format!("{}, {}", t.host.fmt(w.abs()), t.host.fmt(h.abs()));
        t.host.vcb(t0("m3d.vcb.dimensions"), txt);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.p0.is_none() {
            let r = match &self.b.cur {
                Some(c) => c.clone(),
                None => t.snap(ev, SnapOpts::default()),
            };
            let ax = t.axes();
            self.n = r.face.as_ref().map_or(ax.z, |f| f.n);
            self.plane = Some(plane_from(self.n, r.p));
            let (u, v) = plane_axes(self.n, &ax);
            self.u = u;
            self.v = v;
            self.p0 = Some(r.p);
            self.on_face = r.face.is_some();
            self.b.busy = true;
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        if let Some((w, h)) = self.wh {
            self.make(t, w, h)?;
        }
        Ok(())
    }
}

// ---- circle / polygon ----

#[derive(Clone, Debug)]
pub struct CircleTool {
    pub b: Base,
    sides: f64,
    c0: Option<P3>,
    rad: f64,
    n: P3,
    plane: Option<Plane>,
    u: P3,
    on_face: bool,
}
impl CircleTool {
    pub fn new(name: &str) -> CircleTool {
        CircleTool {
            b: Base::new(name),
            sides: if name == "circle" { 24.0 } else { 6.0 },
            c0: None,
            rad: 0.0,
            n: [f64::NAN; 3],
            plane: None,
            u: [f64::NAN; 3],
            on_face: false,
        }
    }
    fn make(&mut self, t: &mut Tools, rad: f64) -> Res<()> {
        if rad <= EPS {
            return Ok(());
        }
        let c0 = self.c0.unwrap_or([f64::NAN; 3]);
        let pts: Vec<P3> = ngon_pts(c0, self.n, self.u, rad, self.sides).into_iter().map(|p| t.local(p)).collect();
        let hint = t.local_dir(if self.on_face || dot(self.n, t.axes().z).abs() > 0.999 { self.n } else { t.toward_eye(c0) });
        let path = t.host.path.clone();
        let name = self.b.name.clone();
        let label = if name == "circle" { t0("m3d.undo.circle") } else { t0("m3d.undo.polygon") };
        t.host.model.change(&label, |m| {
            let id = super::ctx_of(m, &path).def;
            let d = m.def_mut(&id).ok_or(cc_m3d::Error::Type)?;
            add_polygon(d, &pts, Some(hint))?;
            let id2 = super::ctx_of(m, &path).def;
            let d2 = m.def_mut(&id2).ok_or(cc_m3d::Error::Type)?;
            tag_curve(d2, &pts, &name, true);
            Ok(())
        })?;
        self.reset(t);
        t.host.changed();
        t.host.overlay(None);
        t.host.vcb(t0("m3d.vcb.sides"), nstr(self.sides));
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for CircleTool {
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
        if self.c0.is_some() {
            return t0("m3d.tool.click_to_set_the_radius_or");
        }
        let eg = if self.b.name == "circle" { "24" } else { "6" };
        tr("m3d.tool.circle_center", &[("eg", eg), ("n", &nstr(self.sides))])
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        if self.c0.is_some() { t0("m3d.vcb.radius") } else { t0("m3d.vcb.sides") }
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.c0 = None;
        self.b.busy = false;
        self.rad = 0.0;
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        let as_sides = match v {
            Value::Sides(n) => Some(*n),
            Value::Len { num: Some(n), .. } if self.c0.is_none() && is_integer(*n) => Some(*n),
            _ => None,
        };
        if let Some(n) = as_sides {
            if !(3.0..=999.0).contains(&n) {
                t.host.print("Use 3 to 999 sides.".into());
                return Ok(Ret::False);
            }
            self.sides = n;
            let l = self.vcb_label(t);
            t.host.vcb(l, if self.c0.is_some() { String::new() } else { nstr(n) });
            let h = self.hint(t);
            t.host.tip(h);
            if let Some(ev) = self.b.last_ev {
                self.hover(t, &ev)?;
            }
            return Ok(Ret::True);
        }
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        if self.c0.is_none() {
            return Ok(Ret::False);
        }
        self.make(t, vv.abs())?;
        Ok(Ret::Undef)
    }
    fn state(&self) -> serde_json::Value {
        serde_json::json!({"c0": crate::outj::opt_p3(self.c0.as_ref()), "sides": crate::outj::num(self.sides), "rad": crate::outj::num(self.rad)})
    }
}
impl ClickTool for CircleTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(c0) = self.c0 else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        let r = t.snap(ev, SnapOpts { from: Some(c0), plane: self.plane, ..SnapOpts::default() });
        self.b.last_dir = r.dir;
        self.b.last_kind = Some(r.kind.clone());
        let mut q = sub(r.p, mul(self.n, dot(sub(r.p, c0), self.n)));
        if kind_is(&r, &["face", "none", "ground"]) {
            let ray = t.host.cam.ray(ev.x, ev.y);
            if let Some(pl) = &self.plane
                && let Some((gt, gp)) = ray_plane(ray.o, ray.d, pl)
                && gt > 0.0
            {
                q = gp;
            }
        }
        self.rad = dist(q, c0);
        if self.rad > EPS {
            self.u = norm(sub(q, c0));
        }
        let pts = ngon_pts(c0, self.n, self.u, self.rad, self.sides);
        t.host.overlay(Some(Overlay {
            poly: Some(pts),
            lines: Some(vec![Line::dashed(c0, q, &r.kind)]),
            marker: Some(marker(&r)),
            tip: Some(r.tip.clone()),
            ..Overlay::default()
        }));
        let txt = t.host.fmt(self.rad);
        t.host.vcb(t0("m3d.vcb.radius"), txt);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.c0.is_none() {
            let r = match &self.b.cur {
                Some(c) => c.clone(),
                None => t.snap(ev, SnapOpts::default()),
            };
            let ax = t.axes();
            self.n = r.face.as_ref().map_or(ax.z, |f| f.n);
            self.plane = Some(plane_from(self.n, r.p));
            self.u = plane_axes(self.n, &ax).0;
            self.c0 = Some(r.p);
            self.on_face = r.face.is_some();
            self.b.busy = true;
            t.host.vcb(t0("m3d.vcb.radius"), String::new());
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        self.hover(t, ev)?;
        let rad = self.rad;
        self.make(t, rad)
    }
}

// ---- arcs (0.4 M3) ----

const ARC_SEGS: f64 = 12.0;

#[derive(Clone, Debug)]
pub struct ArcTool {
    pub b: Base,
    segs: f64,
    p0: Option<P3>,
    p1: Option<P3>,
    face: Option<Plane>,
    plane: Option<Plane>,
    h0: f64,
    side: P3,
}
impl ArcTool {
    pub fn new(name: &str) -> ArcTool {
        ArcTool { b: Base::new(name), segs: ARC_SEGS, p0: None, p1: None, face: None, plane: None, h0: 0.0, side: [f64::NAN; 3] }
    }
    /// Arc points from p0 to p1 bulging by h (signed: + toward side w).
    fn pts(&self, h: f64) -> Option<Vec<P3>> {
        let (p0, p1) = (self.p0?, self.p1?);
        let c = dist(p0, p1);
        let m = mul(add(p0, p1), 0.5);
        if h.abs() <= EPS {
            return None;
        }
        let w = self.side;
        let hc = c / 2.0;
        let rr = (h * h + hc * hc) / (2.0 * h.abs());
        let ctr = add(m, mul(w, sign(h) * (h.abs() - rr)));
        let sweep = 4.0 * libm::atan(2.0 * h.abs() / c);
        let u0 = norm(sub(p0, ctr));
        let n = norm(cross(u0, norm(sub(add(m, mul(w, h)), ctr))));
        let nn = if truthy(len(n)) { n } else { self.plane.map_or([f64::NAN; 3], |p| p.n) };
        Some(arc_pts(ctr, nn, u0, rr, sweep, self.segs))
    }
    fn set_end(&mut self, t: &mut Tools, p: P3) {
        let Some(p0) = self.p0 else { return };
        if dist(p, p0) <= EPS {
            return;
        }
        self.p1 = Some(p);
        let pl = arc_plane(t, p0, p, self.face.as_ref());
        self.plane = Some(pl);
        self.side = norm(cross(pl.n, sub(p, p0)));
        t.host.vcb(t0("m3d.vcb.bulge"), String::new());
        let h = self.hint(t);
        t.host.tip(h);
    }
    fn make(&mut self, t: &mut Tools, h: f64) -> Res<()> {
        let Some(pts) = self.pts(h) else { return Ok(()) };
        draw_arc(t, &pts, &t0("m3d.undo.arc"))?;
        self.reset(t);
        t.host.overlay(None);
        let l = self.vcb_label(t);
        t.host.vcb(l, String::new());
        let hn = self.hint(t);
        t.host.tip(hn);
        Ok(())
    }
}
impl ToolImpl for ArcTool {
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
            t0("m3d.tool.click_the_start_of_the_arc")
        } else if self.p1.is_none() {
            t0("m3d.tool.click_the_end_of_the_arc")
        } else {
            t0("m3d.tool.move_to_bulge_the_arc_click")
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        if self.p0.is_none() {
            t0("m3d.vcb.segments")
        } else if self.p1.is_none() {
            t0("m3d.vcb.length")
        } else {
            t0("m3d.vcb.bulge")
        }
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.p0 = None;
        self.p1 = None;
        self.face = None;
        self.plane = None;
        self.h0 = 0.0;
        self.b.busy = false;
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, _text: &str) -> Res<Ret> {
        if let Value::Sides(n) = v {
            if !(2.0..=999.0).contains(n) {
                t.host.print(t0("m3d.tool.use_2_to_999_segments"));
                return Ok(Ret::False);
            }
            self.segs = *n;
            t.host.print(tr("m3d.tool.arc_segs", &[("n", &nstr(*n))]));
            if let Some(ev) = self.b.last_ev {
                self.hover(t, &ev)?;
            }
            return Ok(Ret::True);
        }
        let Value::Len { v: vv, .. } = v else { return Ok(Ret::False) };
        let vv = *vv;
        if let (Some(p0), None) = (self.p0, self.p1) {
            let d = self.b.cur.as_ref().map(|c| norm(sub(c.p, p0)));
            let Some(d) = d.filter(|d| truthy(len(*d))) else { return Ok(Ret::False) };
            if !(vv > 0.0) {
                return Ok(Ret::False);
            }
            self.set_end(t, add(p0, mul(d, vv)));
            return Ok(Ret::True);
        }
        if self.p1.is_some() {
            if !(vv > 0.0) {
                return Ok(Ret::False);
            }
            self.make(t, sign(or(self.h0, 1.0)) * vv)?;
            return Ok(Ret::True);
        }
        Ok(Ret::False)
    }
    fn state(&self) -> serde_json::Value {
        use crate::outj::{num, opt_p3};
        serde_json::json!({"p0": opt_p3(self.p0.as_ref()), "p1": opt_p3(self.p1.as_ref()), "segs": num(self.segs), "h0": num(self.h0)})
    }
}
impl ClickTool for ArcTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(p0) = self.p0 else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        let Some(p1) = self.p1 else {
            let r = t.snap(ev, SnapOpts { from: Some(p0), plane: self.face, ..SnapOpts::default() });
            self.b.last_dir = r.dir;
            self.b.last_kind = Some(r.kind.clone());
            t.host.overlay(Some(Overlay {
                lines: Some(vec![Line::new(p0, r.p, &r.kind)]),
                marker: Some(marker(&r)),
                tip: Some(r.tip.clone()),
                ..Overlay::default()
            }));
            let txt = t.host.fmt(dist(p0, r.p));
            t.host.vcb(t0("m3d.vcb.length"), txt);
            self.b.cur = Some(r);
            return Ok(());
        };
        let ray = t.host.cam.ray(ev.x, ev.y);
        let Some(pl) = self.plane else { return Err(cc_m3d::Error::Type) };
        let Some((_, gp)) = ray_plane(ray.o, ray.d, &pl) else { return Ok(()) };
        let m = mul(add(p0, p1), 0.5);
        let c = dist(p0, p1);
        let mut h = dot(sub(gp, m), self.side);
        let mut tip = String::new();
        if (h.abs() - c / 2.0).abs() < c * 0.03 {
            h = sign(h) * c / 2.0;
            tip = t0("m3d.tool.half_circle");
        }
        self.h0 = h;
        let lines = match self.pts(h) {
            Some(pts) => pts.windows(2).filter_map(|w| if let [a, b] = w { Some(Line::new(*a, *b, "arc")) } else { None }).collect(),
            None => vec![Line::new(p0, p1, "arc")],
        };
        t.host.overlay(Some(Overlay { lines: Some(lines), tip: Some(tip), ..Overlay::default() }));
        let txt = t.host.fmt(h.abs());
        t.host.vcb(t0("m3d.vcb.bulge"), txt);
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(p0) = self.p0 else {
            let r = match &self.b.cur {
                Some(c) => c.clone(),
                None => t.snap(ev, SnapOpts::default()),
            };
            self.p0 = Some(r.p);
            self.face = r.face.as_ref().map(|f| f.plane);
            self.b.busy = true;
            t.host.vcb(t0("m3d.vcb.length"), String::new());
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        };
        if self.p1.is_none() {
            let r = match &self.b.cur {
                Some(c) => c.clone(),
                None => t.snap(ev, SnapOpts { from: Some(p0), ..SnapOpts::default() }),
            };
            let p = match &self.face {
                Some(f) => sub(r.p, mul(f.n, dot(r.p, f.n) - f.d)),
                None => r.p,
            };
            self.set_end(t, p);
            return Ok(());
        }
        self.hover(t, ev)?;
        let h0 = self.h0;
        self.make(t, h0)
    }
}

// ---- arc from its centre ----

#[derive(Clone, Debug)]
pub struct ArcCenterTool {
    pub b: Base,
    segs: f64,
    c0: Option<P3>,
    u: Option<P3>,
    r: f64,
    n: Option<P3>,
    sweep: f64,
}
impl ArcCenterTool {
    pub fn new(name: &str) -> ArcCenterTool {
        ArcCenterTool { b: Base::new(name), segs: ARC_SEGS, c0: None, u: None, r: 0.0, n: None, sweep: 0.0 }
    }
    fn nn(&self) -> P3 {
        self.n.unwrap_or([f64::NAN; 3])
    }
    fn set_start(&mut self, t: &mut Tools, q: P3) {
        let c0 = self.c0.unwrap_or([f64::NAN; 3]);
        let d = dist(q, c0);
        if d <= EPS {
            return;
        }
        self.r = d;
        self.u = Some(norm(sub(q, c0)));
        t.host.vcb(t0("m3d.vcb.angle"), String::new());
        let h = self.hint(t);
        t.host.tip(h);
    }
    fn make(&mut self, t: &mut Tools, sweep: f64) -> Res<()> {
        if !(sweep > 1e-9) {
            return Ok(());
        }
        let pts = arc_pts(
            self.c0.unwrap_or([f64::NAN; 3]),
            self.nn(),
            self.u.unwrap_or([f64::NAN; 3]),
            self.r,
            crate::js::jmin(sweep, 2.0 * std::f64::consts::PI),
            self.segs,
        );
        draw_arc(t, &pts, &t0("m3d.undo.arc"))?;
        self.reset(t);
        t.host.overlay(None);
        let l = self.vcb_label(t);
        t.host.vcb(l, String::new());
        let h = self.hint(t);
        t.host.tip(h);
        Ok(())
    }
}
impl ToolImpl for ArcCenterTool {
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
        if self.c0.is_none() {
            t0("m3d.tool.click_the_center_of_the_arc")
        } else if self.u.is_none() {
            t0("m3d.tool.click_where_the_arc_starts_or")
        } else {
            t0("m3d.tool.click_where_the_arc_ends_or")
        }
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        if self.c0.is_none() {
            t0("m3d.vcb.segments")
        } else if self.u.is_none() {
            t0("m3d.vcb.radius")
        } else {
            t0("m3d.vcb.angle")
        }
    }
    click_tool_events!();
    fn reset(&mut self, _t: &mut Tools) -> bool {
        self.c0 = None;
        self.u = None;
        self.r = 0.0;
        self.n = None;
        self.sweep = 0.0;
        self.b.busy = false;
        true
    }
    fn cancel(&mut self, t: &mut Tools) -> Res<()> {
        self.reset(t);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, text: &str) -> Res<Ret> {
        if let Value::Sides(n) = v {
            if !(2.0..=999.0).contains(n) {
                t.host.print(t0("m3d.tool.use_2_to_999_segments"));
                return Ok(Ret::False);
            }
            self.segs = *n;
            if let Some(ev) = self.b.last_ev {
                self.hover(t, &ev)?;
            }
            return Ok(Ret::True);
        }
        if let (Some(c0), None, Value::Len { v: vv, .. }) = (self.c0, self.u, v) {
            let d = self.b.cur.as_ref().map(|c| sub(c.p, c0));
            let u = match d.filter(|d| len(*d) > EPS) {
                Some(d) => norm(d),
                None => plane_axes(self.nn(), &t.axes()).0,
            };
            if !(*vv > 0.0) {
                return Ok(Ret::False);
            }
            self.set_start(t, add(c0, mul(u, *vv)));
            return Ok(Ret::True);
        }
        if self.u.is_some() {
            let deg = match v {
                Value::Num(x) => Some(*x),
                Value::Len { num, .. } => Some(num.unwrap_or_else(|| parse_float(text))),
                _ => None,
            };
            if let Some(deg) = deg {
                if !deg.is_finite() || deg == 0.0 {
                    return Ok(Ret::False);
                }
                self.make(t, crate::js::jmin(deg.abs(), 360.0) * std::f64::consts::PI / 180.0)?;
                return Ok(Ret::True);
            }
        }
        Ok(Ret::False)
    }
    fn state(&self) -> serde_json::Value {
        use crate::outj::{num, opt_p3};
        serde_json::json!({"c0": opt_p3(self.c0.as_ref()), "u": opt_p3(self.u.as_ref()), "r": num(self.r), "segs": num(self.segs), "sweep": num(self.sweep)})
    }
}
impl ClickTool for ArcCenterTool {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        self.b.last_ev = Some(*ev);
        let Some(c0) = self.c0 else {
            let r = t.snap(ev, SnapOpts::default());
            t.host.overlay(Some(Overlay { marker: Some(marker(&r)), tip: Some(r.tip.clone()), ..Overlay::default() }));
            self.b.cur = Some(r);
            return Ok(());
        };
        let n = self.nn();
        let pl = plane_from(n, c0);
        let Some(u) = self.u else {
            let r = t.snap(ev, SnapOpts { from: Some(c0), plane: Some(pl), ..SnapOpts::default() });
            let q = sub(r.p, mul(n, dot(sub(r.p, c0), n)));
            self.b.last_dir = r.dir;
            self.b.last_kind = Some(r.kind.clone());
            t.host.overlay(Some(Overlay {
                lines: Some(vec![Line::dashed(c0, q, &r.kind)]),
                marker: Some(marker(&r)),
                tip: Some(r.tip.clone()),
                ..Overlay::default()
            }));
            let txt = t.host.fmt(dist(c0, q));
            t.host.vcb(t0("m3d.vcb.radius"), txt);
            let mut c = r;
            c.p = q;
            self.b.cur = Some(c);
            return Ok(());
        };
        let ray = t.host.cam.ray(ev.x, ev.y);
        let Some((_, gp)) = ray_plane(ray.o, ray.d, &pl) else { return Ok(()) };
        let w = norm(sub(gp, c0));
        let pi = std::f64::consts::PI;
        let mut a = libm::atan2(dot(cross(u, w), n), dot(u, w));
        if a <= 0.0 {
            a += 2.0 * pi;
        }
        let step = pi / 12.0;
        if (a - round(a / step) * step).abs() < 0.03 {
            a = round(a / step) * step;
        }
        self.sweep = a;
        let pts = arc_pts(c0, n, u, self.r, a, self.segs);
        let mut lines = vec![Line::dashed(c0, add(c0, mul(u, self.r)), "arc")];
        lines.extend(pts.windows(2).filter_map(|w| if let [p, q] = w { Some(Line::new(*p, *q, "arc")) } else { None }));
        let tip = format!("{}°", nstr(round((a * 180.0) / pi * 10.0) / 10.0));
        t.host.overlay(Some(Overlay { lines: Some(lines), tip: Some(tip), ..Overlay::default() }));
        t.host.vcb(t0("m3d.vcb.angle"), nstr(round((a * 180.0) / pi * 100.0) / 100.0));
        Ok(())
    }
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if self.c0.is_none() {
            let r = match &self.b.cur {
                Some(c) => c.clone(),
                None => t.snap(ev, SnapOpts::default()),
            };
            self.n = Some(r.face.as_ref().map_or(t.axes().z, |f| f.n));
            self.c0 = Some(r.p);
            self.b.busy = true;
            t.host.vcb(t0("m3d.vcb.radius"), String::new());
            let h = self.hint(t);
            t.host.tip(h);
            return Ok(());
        }
        if self.u.is_none() {
            if let Some(q) = self.b.cur.as_ref().map(|c| c.p) {
                self.set_start(t, q);
            }
            return Ok(());
        }
        self.hover(t, ev)?;
        let s = self.sweep;
        self.make(t, s)
    }
}
