//! Scripted sessions: a camera, a model and the Tools driven by a list of steps (pointer events in screen pixels or at
//! world points, keys, typed values, tool choices, tool methods, model ops, camera moves and function-level queries),
//! as `forks/tools/tdiff.mjs` drives the frozen JS. After each step a record: the step's return value or error, the
//! tool's state (name, busy, hint, its own fields, last snap), the host's tip / value box / new messages / selection /
//! open context, the overlay, the undo and redo labels, the model's version and `toJSON()`. Everything here is hostile
//! input: malformed steps are errors in the record, never panics; the number of steps is capped.
//!
//! Script: `{units?, init?, cam?: {w, h, eye, target, fov?, persp?, clip?}, steps: [...]}`. A step is one of
//! `{set, arg?}` · `{move|down|up|click: [x, y] | {at: [x, y, z]}, shift?, ctrl?, alt?, time?}` · `{key}` · `{keyUp}` ·
//! `{value}` · `{escape}` · `{call, args?}` · `{grab|turn|resize: pos, copy?}` · `{sel: [{t, id}]}` ·
//! `{model: {op, path?, args}, toSel?}` · `{newModel, units?}` · `{open: path}` · `{close}` · `{flags: {...}}` ·
//! `{cam: {...}}` · `{camOp, args}` · `{infer: pos, opts?}` · `{pick: pos, path?, radius?}` · `{pickWindow: rect,
//! crossing?}` · `{pickAt: pos}` · `{snap: pos}` · `{collide: {movingRoot, delta}}` · `{sun: {...}}` ·
//! `{parse: text, unit?}` · `{project: [x, y, z]}` · `{ray: [x, y]}` · `{dims: true}` · `{section: id}` ·
//! `{app: name, args}` (terrain, stretch, starter).

use serde_json::{Map, Value, json};

use cc_m3d::model::Model;
use cc_m3d::ops::Sel;
use cc_m3d::script::{Regs, exec};
use cc_m3d::vec::P3;
use cc_m3d::{Error, Res};

use crate::camera::{Box3, Camera};
use crate::collide::Collider;
use crate::infer::{Axes3, InferOpts, Rect, dim_layout, dim_text, infer, pick, pick_dim, pick_section, pick_window, section_layout};
use crate::outj::{num, nums, p3, pts};
use crate::scene::ObjRef;
use crate::tools::{Ev, Host, PickArg, ToolArg, Tools};
use crate::units::{format_length, parse_length, parse_value};

/// Most steps a script may have.
pub const MAX_STEPS: usize = 200_000;

fn f(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}
fn pt(v: Option<&Value>) -> Option<P3> {
    let a = v?.as_array()?;
    Some([f(a.first()).unwrap_or(f64::NAN), f(a.get(1)).unwrap_or(f64::NAN), f(a.get(2)).unwrap_or(f64::NAN)])
}
fn ids(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default()
}
fn truthy(v: Option<&Value>) -> bool {
    cc_m3d::json::truthy(v)
}
fn sel_of(v: Option<&Value>) -> Vec<Sel> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| Some(Sel::new(s.get("t")?.as_str()?, s.get("id").and_then(Value::as_f64).unwrap_or(f64::NAN))))
                .collect()
        })
        .unwrap_or_default()
}
fn sel_json(s: &[Sel]) -> Value {
    Value::Array(s.iter().map(|x| json!({"t": x.t, "id": num(x.id)})).collect())
}

/// A session: the tools (with their host) and the model-op registers.
pub struct Session {
    pub tools: Tools,
    regs: Regs,
    seen_msgs: usize,
    /// the model version (and model) of the last record that carried the model's JSON
    last_json: Option<(f64, u64)>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("tools", &self.tools).finish_non_exhaustive()
    }
}

/// Apply a camera description ({w, h, eye, target, fov, persp, clip}).
pub fn set_cam(cam: &mut Camera, c: &Value) {
    if let (Some(w), Some(h)) = (f(c.get("w")), f(c.get("h"))) {
        cam.set_size(w, h);
    }
    if let Some(e) = pt(c.get("eye")) {
        cam.eye = e;
    }
    if let Some(t) = pt(c.get("target")) {
        cam.target = t;
    }
    if let Some(v) = f(c.get("fov")) {
        cam.fov = v;
    }
    if let Some(p) = c.get("persp").and_then(Value::as_bool) {
        cam.persp = p;
    }
    if let Some(r) = c.get("clip") {
        match r.as_f64() {
            Some(r) => cam.clip(r),
            None => cam.clip_default(),
        }
    }
}

impl Session {
    pub fn new(script: &Value) -> Res<Session> {
        let mut m = Model::new();
        m.uid = "u0".into();
        if let Some(init) = script.get("init").filter(|v| !v.is_null()) {
            m.load(init, false)?;
            m.uid = "u0".into();
        }
        if let Some(u) = script.get("units").and_then(Value::as_str) {
            m.units = u.to_string();
        }
        let mut cam = Camera::new();
        if let Some(c) = script.get("cam") {
            set_cam(&mut cam, c);
        }
        let mut host = Host::new(m, cam);
        host.units = host.model.units.clone();
        let tools = Tools::new(host)?;
        Ok(Session { tools, regs: Regs::default(), seen_msgs: 0, last_json: None })
    }

    /// A screen position: [x, y], or {at: [x, y, z]} projected through the camera (null behind the eye).
    fn pos(&self, v: Option<&Value>) -> Res<(f64, f64)> {
        let v = v.ok_or(Error::Type)?;
        if let Some(a) = v.get("at") {
            let p = pt(Some(a)).ok_or(Error::Type)?;
            let s = self.tools.host.cam.project(p).ok_or(Error::Type)?;
            let dx = f(v.get("dx")).unwrap_or(0.0);
            let dy = f(v.get("dy")).unwrap_or(0.0);
            return Ok((s[0] + dx, s[1] + dy));
        }
        let a = v.as_array().ok_or(Error::Type)?;
        Ok((f(a.first()).unwrap_or(f64::NAN), f(a.get(1)).unwrap_or(f64::NAN)))
    }
    fn ev(&self, st: &Value, key: &str) -> Res<Ev> {
        let (x, y) = self.pos(st.get(key))?;
        Ok(Ev {
            x,
            y,
            shift: truthy(st.get("shift")),
            ctrl: truthy(st.get("ctrl")),
            alt: truthy(st.get("alt")),
            time: f(st.get("time")),
            ..Ev::default()
        })
    }

    /// Run one step; Ok(return value as JSON, None = undefined).
    #[allow(clippy::too_many_lines)]
    pub fn step(&mut self, st: &Value, index: usize) -> Res<Option<Value>> {
        let t = &mut self.tools;
        let g = |k: &str| st.get(k);
        if let Some(name) = g("set").and_then(Value::as_str) {
            let arg = tool_arg(name, g("arg"), t)?;
            self.tools.set(name, arg)?;
            self.tools.flush()?;
            return Ok(None);
        }
        for k in ["move", "down", "up", "click"] {
            if st.get(k).is_some() {
                let ev = self.ev(st, k)?;
                let t = &mut self.tools;
                match k {
                    "move" => t.mv(&ev)?,
                    "down" => t.down(&ev)?,
                    "up" => t.up(&ev)?,
                    _ => t.click(&ev)?,
                }
                t.flush()?;
                return Ok(None);
            }
        }
        if let Some(k) = g("key").and_then(Value::as_str) {
            let r = t.key(k)?;
            t.flush()?;
            return Ok(Some(Value::Bool(r)));
        }
        if let Some(k) = g("keyUp").and_then(Value::as_str) {
            t.key_up(k)?;
            return Ok(None);
        }
        if let Some(v) = g("value").and_then(Value::as_str) {
            let r = t.value(v)?;
            t.flush()?;
            return Ok(Some(Value::Bool(r)));
        }
        if g("escape").is_some() {
            return Ok(Some(Value::Bool(t.escape()?)));
        }
        if let Some(name) = g("call").and_then(Value::as_str) {
            let args = g("args").and_then(Value::as_array).cloned().unwrap_or_default();
            let r = t.call(name, &args)?;
            t.flush()?;
            return match r {
                Some(Value::Null) => Ok(None),
                Some(v) => Ok(Some(v)),
                None => Err(Error::Type),
            };
        }
        for k in ["grab", "turn", "resize"] {
            if st.get(k).is_some() {
                let ev = self.ev(st, k)?;
                let t = &mut self.tools;
                let r = match k {
                    "grab" => t.grab(&ev, truthy(st.get("copy")))?,
                    "turn" => t.turn(&ev)?,
                    _ => t.resize(&ev)?,
                };
                return Ok(Some(Value::Bool(r)));
            }
        }
        if let Some(s) = g("sel") {
            t.host.sel = sel_of(Some(s));
            return Ok(None);
        }
        if let Some(op) = g("model") {
            let r = exec(&mut t.host.model, &mut self.regs, op, index as f64, true)?;
            let v: Option<Value> = match r {
                Some(s) => Some(serde_json::from_str(&s).map_err(|_| Error::Type)?),
                None => None,
            };
            if truthy(g("toSel")) {
                self.tools.host.sel = sel_of(v.as_ref());
            }
            return Ok(v);
        }
        if g("newModel").is_some() {
            let mut m = Model::new();
            m.uid = format!("u{index}");
            if let Some(u) = g("units").and_then(Value::as_str) {
                m.units = u.to_string();
            }
            t.host.units = m.units.clone();
            t.host.set_model(m);
            t.host.path = Vec::new();
            t.host.sel = Vec::new();
            return Ok(None);
        }
        if let Some(p) = g("open") {
            t.host.open_context(ids(Some(p)));
            return Ok(None);
        }
        if g("close").is_some() {
            t.host.close_context();
            return Ok(None);
        }
        if let Some(fl) = g("flags") {
            if let Some(b) = fl.get("snapping").and_then(Value::as_bool) {
                t.snapping = b;
            }
            if let Some(b) = fl.get("collide").and_then(Value::as_bool) {
                t.collide = b;
            }
            if let Some(b) = fl.get("touch").and_then(Value::as_bool) {
                t.touch = b;
            }
            if let Some(p) = fl.get("paint") {
                t.paint_color = p.as_str().map(String::from);
            }
            if fl.get("unlock").is_some() {
                t.lock = None;
            }
            return Ok(None);
        }
        if let Some(c) = g("cam") {
            set_cam(&mut t.host.cam, c);
            return Ok(Some(t.host.cam.to_json()));
        }
        if let Some(op) = g("camOp").and_then(Value::as_str) {
            let a = g("args").and_then(Value::as_array).cloned().unwrap_or_default();
            let n = |i: usize| f(a.get(i));
            let cam = &mut t.host.cam;
            match op {
                "orbit" => cam.orbit(n(0).unwrap_or(f64::NAN), n(1).unwrap_or(f64::NAN)),
                "pan" => cam.pan(n(0).unwrap_or(f64::NAN), n(1).unwrap_or(f64::NAN)),
                "zoomAt" => cam.zoom_at(n(0).unwrap_or(f64::NAN), n(1), n(2), pt(a.get(3))),
                "fit" | "standard" => {
                    let bb =
                        a.get(if op == "fit" { 0 } else { 1 }).and_then(|b| Some(Box3 { min: pt(b.get("min"))?, max: pt(b.get("max"))? }));
                    if op == "fit" {
                        cam.fit(bb.as_ref(), pt(a.get(1)));
                    } else {
                        cam.standard(a.first().and_then(Value::as_str).unwrap_or(""), bb.as_ref());
                    }
                }
                "viewCube" => {
                    let d = pt(a.first()).ok_or(Error::Type)?;
                    crate::viewcube_camera::apply_view(cam, d, pt(a.get(1)));
                }
                _ => return Err(Error::Msg(format!("unknown camOp {op}"))),
            }
            return Ok(Some(t.host.cam.to_json()));
        }
        if g("infer").is_some() {
            let (x, y) = self.pos(g("infer"))?;
            let t = &mut self.tools;
            let o = g("opts").cloned().unwrap_or(Value::Null);
            let sc = t.host.scene();
            let ax = o.get("axes").and_then(|a| Some(Axes3 { x: pt(a.get("x"))?, y: pt(a.get("y"))?, z: pt(a.get("z"))? }));
            let opts = InferOpts {
                radius: f(o.get("radius")),
                axes: ax,
                snap: o.get("snap").and_then(Value::as_bool) != Some(false),
                from: pt(o.get("from")),
                lock_dir: pt(o.get("lockDir")),
                lock_kind: o.get("lockKind").and_then(Value::as_str).map(String::from),
                ref_edge: o.get("ref").and_then(|r| Some((pt(r.get("a"))?, pt(r.get("b"))?))),
                plane: o.get("plane").and_then(|p| Some(cc_m3d::vec::Plane { n: pt(p.get("n"))?, d: f(p.get("d"))? })),
                origin: pt(o.get("origin")),
                ground: o.get("ground").and_then(|p| Some(cc_m3d::vec::Plane { n: pt(p.get("n"))?, d: f(p.get("d"))? })),
                ..InferOpts::default()
            };
            let r = infer(&sc, &t.host.cam, x, y, &opts);
            return Ok(Some(r.to_json()));
        }
        if g("pick").is_some() {
            let (x, y) = self.pos(g("pick"))?;
            let t = &mut self.tools;
            let sc = t.host.scene();
            let path = g("path").map_or_else(Vec::new, |p| ids(Some(p)));
            let r = pick(&sc, &t.host.cam, x, y, &path, f(g("radius")).unwrap_or(7.0));
            return Ok(Some(r.map_or(Value::Null, |s| sel_json(&[s]).get(0).cloned().unwrap_or(Value::Null))));
        }
        if let Some(r) = g("pickWindow") {
            let rect = Rect {
                x0: f(r.get("x0")).unwrap_or(f64::NAN),
                y0: f(r.get("y0")).unwrap_or(f64::NAN),
                x1: f(r.get("x1")).unwrap_or(f64::NAN),
                y1: f(r.get("y1")).unwrap_or(f64::NAN),
            };
            let sc = t.host.scene();
            let path = g("path").map_or_else(Vec::new, |p| ids(Some(p)));
            let r = pick_window(&sc, &t.host.cam, rect, truthy(g("crossing")), &path);
            return Ok(Some(sel_json(&r)));
        }
        if g("pickAt").is_some() {
            let (x, y) = self.pos(g("pickAt"))?;
            let r = self.tools.pick_at(&Ev::at(x, y));
            return Ok(Some(r.map_or(Value::Null, |s| sel_json(&[s]).get(0).cloned().unwrap_or(Value::Null))));
        }
        if g("snap").is_some() {
            let (x, y) = self.pos(g("snap"))?;
            let r = self.tools.snap(&Ev::at(x, y), crate::tools::SnapOpts::default());
            return Ok(Some(r.to_json()));
        }
        if let Some(c) = g("collide") {
            let root = f(c.get("movingRoot")).unwrap_or(f64::NAN);
            let sc = t.host.scene();
            let moving = move |o: ObjRef<'_>, _k: char| o.path.first() == Some(&root);
            let ignore = |_o: ObjRef<'_>, _k: char| false;
            let mut col = Collider::new(sc, &moving, &ignore);
            let (d, hit) = col.clamp(pt(c.get("delta")).ok_or(Error::Type)?);
            return Ok(Some(json!({"delta": p3(&d), "hit": hit, "empty": col.empty})));
        }
        if let Some(s) = g("sun") {
            let d = crate::sun::When::default();
            let w = crate::sun::When {
                month: f(s.get("month")).unwrap_or(d.month),
                day: f(s.get("day")).unwrap_or(d.day),
                hour: f(s.get("hour")).unwrap_or(d.hour),
                lat: f(s.get("lat")).unwrap_or(d.lat),
                lon: f(s.get("lon")).unwrap_or(d.lon),
                tz: f(s.get("tz")).unwrap_or(d.tz),
                year: f(s.get("year")).unwrap_or(d.year),
            };
            let r = crate::sun::sun_position(&w);
            return Ok(Some(
                json!({"altitude": num(r.altitude), "azimuth": num(r.azimuth), "declination": num(r.declination), "eqtime": num(r.eqtime), "dir": p3(&r.dir), "up": r.up}),
            ));
        }
        if let Some(text) = g("parse").and_then(Value::as_str) {
            let unit = g("unit").and_then(Value::as_str).unwrap_or("m");
            return Ok(Some(json!({
                "value": parse_value(text, unit).map_or(Value::Null, |v| v.to_json()),
                "length": parse_length(text, unit).map_or(Value::Null, num),
            })));
        }
        if let Some(p) = g("project") {
            let p = pt(Some(p)).ok_or(Error::Type)?;
            return Ok(Some(t.host.cam.project(p).map_or(Value::Null, |s| nums(&s))));
        }
        if let Some(r) = g("ray") {
            let (x, y) = (f(r.get(0)).unwrap_or(f64::NAN), f(r.get(1)).unwrap_or(f64::NAN));
            let ray = t.host.cam.ray(x, y);
            return Ok(Some(json!({"o": p3(&ray.o), "d": p3(&ray.d), "pixel": num(t.host.cam.pixel_size0())})));
        }
        if g("dims").is_some() {
            // every dimension as drawn, and what a click at its text picks
            let cam = t.host.cam.clone();
            let vp = cam.view_proj();
            let units = t.host.units.clone();
            let fmt = move |m: f64| format_length(m, &units);
            let mut out = Vec::new();
            for id in t.host.model.dims.key_vec() {
                let Some(mut dm) = t.host.model.dims.get(id).cloned() else { continue };
                let l = dim_layout(&t.host.model, &mut dm, &cam, &vp);
                t.host.model.dims.set(id, dm);
                let picked = match &l.m {
                    Some(m) => pick_dim(
                        &mut t.host.model,
                        &cam,
                        m.first().copied().unwrap_or(f64::NAN),
                        m.get(1).copied().unwrap_or(f64::NAN),
                        &fmt,
                        6.0,
                    )
                    .map(|s| num(s.id)),
                    None => None,
                };
                out.push(json!({"id": num(id), "kind": l.kind, "L": num(l.l), "M": l.m.as_ref().map_or(Value::Null, |m| nums(m)), "text": dim_text(&l, &fmt), "pick": picked}));
            }
            return Ok(Some(Value::Array(out)));
        }
        if let Some(id) = f(g("section")) {
            let s = t.host.model.sections.get(id).cloned().ok_or(Error::Type)?;
            let l = section_layout(&t.host.model, s.p, s.n);
            let c = l.corners;
            let mid = |a: P3, b: P3| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
            let q = t.host.cam.project(mid(c[0], c[1]));
            let picked = q.and_then(|q| pick_section(&t.host.model, &t.host.cam, q[0], q[1], 6.0)).map(|s| num(s.id));
            return Ok(Some(
                json!({"corners": pts(&c), "arrows": l.arrows.iter().map(|(a, b)| json!([p3(a), p3(b)])).collect::<Vec<_>>(), "pick": picked}),
            ));
        }
        if let Some(name) = g("app").and_then(Value::as_str) {
            return app(t, name, g("args").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()));
        }
        Err(Error::Msg("unknown step".into()))
    }

    /// The record after a step.
    pub fn record(&mut self, r: &Result<Option<Value>, String>) -> String {
        let t = &mut self.tools;
        let mut o = Map::new();
        match r {
            Ok(Some(v)) => {
                o.insert("ret".into(), v.clone());
            }
            Ok(None) => {}
            Err(e) => {
                o.insert("err".into(), Value::String(e.clone()));
            }
        }
        o.insert("name".into(), Value::String(t.name.clone()));
        o.insert("busy".into(), Value::Bool(t.busy()));
        o.insert("hint".into(), Value::String(t.hint()));
        o.insert("tip".into(), Value::String(t.host.tip.clone()));
        o.insert("vcb".into(), json!([t.host.vcb.0, t.host.vcb.1]));
        o.insert("cur".into(), t.cur().map_or(Value::Null, crate::infer::Snap::to_json));
        o.insert(
            "lock".into(),
            t.lock.as_ref().map_or(Value::Null, |l| {
                if l.shift { json!({"kind": l.kind, "dir": p3(&l.dir), "shift": true}) } else { json!({"kind": l.kind, "dir": p3(&l.dir)}) }
            }),
        );
        o.insert("sel".into(), sel_json(&t.host.sel));
        o.insert("path".into(), nums(&t.host.path));
        let new: Vec<Value> = t.host.msgs.iter().skip(self.seen_msgs).map(|m| Value::String(m.clone())).collect();
        self.seen_msgs = t.host.msgs.len();
        o.insert("msgs".into(), Value::Array(new));
        o.insert("overlay".into(), t.host.overlay.as_ref().map_or(Value::Null, crate::overlay::Overlay::to_json));
        o.insert("tool".into(), t.tool_state());
        o.insert("paint".into(), t.paint_color.clone().map_or(Value::Null, Value::String));
        o.insert("snapping".into(), Value::Bool(t.snapping));
        let m = &t.host.model;
        o.insert("undo".into(), Value::Array(m.undo_stack.iter().map(|s| Value::String(s.label.clone())).collect()));
        o.insert("redo".into(), Value::Array(m.redo_stack.iter().map(|s| Value::String(s.label.clone())).collect()));
        o.insert("v".into(), num(m.version()));
        o.insert("pv".into(), Value::Bool(m.previewing()));
        o.insert("cam".into(), json!({"eye": p3(&t.host.cam.eye), "target": p3(&t.host.cam.target)}));
        // the model's JSON when its version (or the model) changed since the last record that carried it
        let key = (t.host.model.version(), t.host.model_gen());
        if self.last_json == Some(key) {
            return format!("{{\"json\":null,\"r\":{}}}", Value::Object(o));
        }
        self.last_json = Some(key);
        let mut out = cc_m3d::json::Out::new(true);
        t.host.model.to_json_out(&mut out, false);
        format!("{{\"json\":{},\"r\":{}}}", out.s, Value::Object(o))
    }
}

/// The argument of `{set, arg}`.
fn tool_arg(name: &str, a: Option<&Value>, t: &mut Tools) -> Res<ToolArg> {
    let Some(a) = a.filter(|a| !a.is_null()) else { return Ok(ToolArg::None) };
    Ok(match name {
        "curve" => match a.get("edit").and_then(Value::as_str) {
            Some(c) => ToolArg::CurveEdit(c.to_string()),
            None => ToolArg::None,
        },
        "place" => match a {
            Value::String(s) => ToolArg::Place { def_id: s.clone(), align: false, hint: None },
            _ => ToolArg::Place {
                def_id: a.get("defId").and_then(Value::as_str).unwrap_or("").to_string(),
                align: truthy(a.get("align")),
                hint: a.get("hint").and_then(Value::as_str).map(String::from),
            },
        },
        "light" => ToolArg::Light(a.clone()),
        "paste" => {
            // {path, sel}: the clipboard as model.copyClip makes it now
            let path = ids(a.get("path"));
            let sel = sel_of(a.get("sel"));
            let clip = t.host.model.copy_clip(&path, &sel)?.unwrap_or(Value::Null);
            ToolArg::Paste(clip)
        }
        "pick" => ToolArg::Pick(PickArg {
            hint: a.get("hint").and_then(Value::as_str).map(String::from),
            need: a.get("need").and_then(Value::as_u64).map_or(0, |n| usize::try_from(n).unwrap_or(0)),
            picked: Vec::new(),
        }),
        _ => ToolArg::None,
    })
}

/// The app modules' functions (terrain, stretch, starter), edits run inside `model.change(label, …)`.
fn app(t: &mut Tools, name: &str, a: &[Value]) -> Res<Option<Value>> {
    let s = |i: usize| a.get(i).and_then(Value::as_str).unwrap_or("").to_string();
    let label = name.to_string();
    let m = &mut t.host.model;
    let r = match name {
        "delaunay" => {
            let p: Vec<[f64; 2]> = a
                .first()
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|q| [f(q.get(0)).unwrap_or(f64::NAN), f(q.get(1)).unwrap_or(f64::NAN)]).collect())
                .unwrap_or_default();
            let tris = crate::terrain::delaunay(&p);
            json!(tris)
        }
        "terrainFromContours" => {
            let path = ids(a.first());
            let eids = ids(a.get(1));
            let nm = a.get(2).and_then(Value::as_str).unwrap_or("Terrain").to_string();
            let id = m.change(&label, |mm| {
                let d = mm.context(&path).ok_or(Error::Type)?.def;
                Ok(crate::terrain::terrain_from_contours(mm, &d, &eids, &nm))
            })?;
            id.map_or(Value::Null, Value::String)
        }
        "terrainGrid" => {
            let o = a.first().cloned().unwrap_or(Value::Null);
            let w = f(o.get("width")).unwrap_or(20.0);
            let d = f(o.get("depth")).unwrap_or(20.0);
            let sp = f(o.get("spacing")).unwrap_or(1.0);
            let org = pt(o.get("origin")).unwrap_or([0.0; 3]);
            let nm = o.get("name").and_then(Value::as_str).unwrap_or("Terrain").to_string();
            Value::String(m.change(&label, |mm| Ok(crate::terrain::terrain_grid(mm, w, d, sp, org, &nm)))?)
        }
        "stretchDef" => {
            let src = s(0);
            let size = pt(a.get(1)).ok_or(Error::Type)?;
            let nm = s(2);
            Value::String(m.change(&label, |mm| crate::dynamic::stretch_def(mm, &src, size, &nm))?)
        }
        "buildStarter" => {
            let id = s(0);
            let nm = a.get(1).and_then(Value::as_str).map(String::from);
            let r = m.change(&label, |mm| crate::starter::build_starter(mm, &id, nm.as_deref()))?;
            r.map_or(Value::Null, Value::String)
        }
        "partsOf" => {
            if m.def(&s(0)).is_none() {
                return Err(Error::Type);
            }
            let parts = crate::dynamic::parts_of(m, &s(0));
            Value::Array(parts.iter().map(|p| json!({"iid": num(p.iid), "min": p3(&p.bx.min), "max": p3(&p.bx.max)})).collect())
        }
        _ => return Err(Error::Msg(format!("unknown app {name}"))),
    };
    Ok(Some(r))
}

/// Run a script and write one record per step (a JSON array); `records` false runs the steps only (the benchmark).
pub fn run(script: &Value, records: bool) -> Res<String> {
    let mut s = Session::new(script)?;
    let empty = Vec::new();
    let steps = script.get("steps").and_then(Value::as_array).unwrap_or(&empty);
    let mut out = String::from("[");
    for (i, st) in steps.iter().take(MAX_STEPS).enumerate() {
        let r = s.step(st, i).map_err(|e| e.to_string());
        if records {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&s.record(&r));
        }
    }
    out.push(']');
    Ok(out)
}
