//! A test rig in the frozen bin/test-modelcraft.mjs's words: a model, a camera, the Tools with the stub host, and
//! clickAt / moveTo / value helpers that click through the camera at world points.
#![allow(dead_code)]

use cc_m3d::Model;
use cc_m3d::def::{Def, face_area, face_normal};
use cc_m3d::edges::{EdgeOpts, add_edge};
use cc_m3d::ops::{Sel, add_polygon, connected, push_pull, rect_pts};
use cc_m3d::vec::{P3, dot};
use cc_m3d_tools::camera::Camera;
use cc_m3d_tools::tools::{Ev, Host, ToolArg, Tools};

pub const R: P3 = [1.0, 0.0, 0.0];
pub const G: P3 = [0.0, 1.0, 0.0];
pub const Z: P3 = [0.0, 0.0, 1.0];
pub const O: P3 = [0.0, 0.0, 0.0];

pub fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
pub fn c6(a: f64, b: f64) -> bool {
    close(a, b, 1e-6)
}
pub fn dist(a: P3, b: P3) -> f64 {
    cc_m3d::vec::dist(a, b)
}
pub fn faces_facing(d: &Def, n: P3) -> Vec<f64> {
    d.faces.iter().filter(|(_, f)| dot(face_normal(d, f), n) > 1.0 - 1e-9).map(|(id, _)| id).collect()
}
pub fn area(d: &Def, fid: f64) -> f64 {
    face_area(d, d.faces.get(fid).unwrap())
}
pub fn volume(d: &Def) -> f64 {
    cc_m3d::solids::solid_info(d).volume.unwrap_or(f64::NAN)
}
pub fn closed(d: &Def) -> bool {
    cc_m3d::solids::solid_info(d).closed
}

pub struct Rig {
    pub t: Tools,
    pub time: f64,
}

impl Rig {
    pub fn new() -> Rig {
        let mut m = Model::new();
        m.units = "m".into();
        let mut cam = Camera::new();
        cam.set_size(1000.0, 800.0);
        let host = Host::new(m, cam);
        Rig { t: Tools::new(host).unwrap(), time: 0.0 }
    }
    /// A new, empty model (another tab).
    pub fn new_model(&mut self) {
        let mut m = Model::new();
        m.units = "m".into();
        self.t.host.units = "m".into();
        self.t.host.set_model(m);
        self.t.host.path = Vec::new();
        self.t.host.sel = Vec::new();
    }
    pub fn model(&self) -> &Model {
        &self.t.host.model
    }
    pub fn m(&mut self) -> &mut Model {
        &mut self.t.host.model
    }
    pub fn root(&self) -> &Def {
        self.t.host.model.root().unwrap()
    }
    pub fn cam(&mut self, eye: P3, target: P3, clip: f64) {
        let c = &mut self.t.host.cam;
        c.eye = eye;
        c.target = target;
        c.clip(clip);
    }
    pub fn at(&self, p: P3) -> (f64, f64) {
        let s = self.t.host.cam.project(p).unwrap();
        (s[0], s[1])
    }
    pub fn set(&mut self, name: &str) {
        self.t.set(name, ToolArg::None).unwrap();
        self.t.flush().unwrap();
    }
    pub fn set_arg(&mut self, name: &str, arg: ToolArg) {
        self.t.set(name, arg).unwrap();
        self.t.flush().unwrap();
    }
    pub fn ev(&self, x: f64, y: f64) -> Ev {
        Ev::at(x, y)
    }
    /// `T.click(x, y, {time})`.
    pub fn click_xy(&mut self, x: f64, y: f64, mods: Ev) {
        self.time += 1000.0;
        let ev = Ev { x, y, time: Some(self.time), ..mods };
        self.t.click(&ev).unwrap();
        self.t.flush().unwrap();
    }
    /// clickAt(p): a click at world point p (no move first, as T.click moves itself).
    pub fn click(&mut self, p: P3) {
        let (x, y) = self.at(p);
        self.click_xy(x, y, Ev::default());
    }
    pub fn click_mods(&mut self, p: P3, mods: Ev) {
        let (x, y) = self.at(p);
        self.click_xy(x, y, mods);
    }
    /// moveTo(p), then clickAt(p).
    pub fn move_click(&mut self, p: P3) {
        self.move_to(p);
        self.click(p);
    }
    pub fn move_to(&mut self, p: P3) {
        let (x, y) = self.at(p);
        self.move_xy(x, y);
    }
    pub fn move_xy(&mut self, x: f64, y: f64) {
        self.t.mv(&Ev::at(x, y)).unwrap();
    }
    pub fn down_xy(&mut self, x: f64, y: f64, time: f64) {
        self.t.down(&Ev { x, y, time: Some(time), ..Ev::default() }).unwrap();
    }
    pub fn up_xy(&mut self, x: f64, y: f64) {
        self.t.up(&Ev::at(x, y)).unwrap();
    }
    pub fn value(&mut self, s: &str) -> bool {
        let r = self.t.value(s).unwrap();
        self.t.flush().unwrap();
        r
    }
    pub fn key(&mut self, k: &str) -> bool {
        self.t.key(k).unwrap()
    }
    pub fn escape(&mut self) -> bool {
        self.t.escape().unwrap()
    }
    pub fn msgs(&self) -> &[String] {
        &self.t.host.msgs
    }
    pub fn last_msg(&self) -> String {
        self.t.host.msgs.last().cloned().unwrap_or_default()
    }
    /// One edit of the root, as model.change('…', (m) => …).
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Model) -> cc_m3d::Res<R>) -> R {
        self.t.host.model.change("edit", f).unwrap()
    }
    /// A w × d box at p, h tall, in the root.
    pub fn box_at(&mut self, p: P3, w: f64, d: f64, h: f64) {
        self.edit(|m| {
            let r = m.root_mut().unwrap();
            add_polygon(r, &rect_pts(p, R, G, w, d), Some(Z))?;
            let f = r.faces.keys().last().unwrap();
            push_pull(r, f, h);
            Ok(())
        });
    }
    /// A box made into a group → the instance id.
    pub fn group_box(&mut self, p: P3, w: f64, d: f64, h: f64) -> f64 {
        self.edit(|m| {
            let r = m.root_mut().unwrap();
            add_polygon(r, &rect_pts(p, R, G, w, d), Some(Z))?;
            let f = r.faces.keys().last().unwrap();
            push_pull(r, f, h);
            let sel = connected(r, &[Sel::new("f", f)]);
            Ok(m.make_group("root", &sel, "group", None)?.unwrap())
        })
    }
}

pub fn line(d: &mut Def, p: P3, q: P3) -> Vec<f64> {
    add_edge(d, p, q, EdgeOpts::default())
}
pub fn line_nf(d: &mut Def, p: P3, q: P3) -> Vec<f64> {
    add_edge(d, p, q, EdgeOpts { no_faces: true, hint: None })
}
pub fn connected0(d: &Def) -> Vec<Sel> {
    connected(d, &[Sel::new("f", d.faces.first_key().unwrap())])
}
pub fn curves(d: &Def, kind: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in d.edges.values() {
        if let Some(c) = &e.curve
            && c.starts_with(&format!("{kind}:"))
            && !out.contains(c)
        {
            out.push(c.clone());
        }
    }
    out
}
