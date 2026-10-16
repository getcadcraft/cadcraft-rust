//! The model (model.js `class Model`): definitions, guides, lamps, dimensions, section planes, storeys, drawing axes,
//! scenes and tags, with undo / redo / amend and live previews over JSON snapshots.
//!
//! Undo works exactly as the JS: a step holds a `Snap` (the model's JSON without its definitions + one JSON string
//! per definition). A definition keeps its JSON with the version it was made at (`Def::cache`, the JS's `defStrs`);
//! restoring keeps a definition whose kept JSON equals the snapshot's while that JSON is still known to be right.
//! That includes the JS's behaviour when an edit throws after changing a definition in place: the change is not
//! undone (the kept JSON still says the definition is as it was). Model::change → restore → (kept object).

use std::rc::Rc;

use serde_json::Value;

use crate::def::*;
use crate::edges::{cleanup, prune_splines, remove_loose_verts, vertex_at};
use crate::error::{Error, Res, msg, msg_with};
use crate::file::{Fail, def_from_json, def_of, def_value, jn};
use crate::i18n::t;
use crate::js;
use crate::json::{self, Out};
use crate::omap::{FxMap, IdMap, IdSet, OMap, kb};
use crate::ops::{CurveRemap, Sel};
use crate::paint::{image_id, is_color, is_image_paint};
use crate::part::{Part, PartRef, same};
use crate::vec::*;

pub const MAX_UNDO: usize = 200;
/// Most placements one walk visits. Spec gap: the JS walks every placement down to 32 levels, so a file whose
/// components hold two of the next one 33 times over is 2^32 placements (the page hangs); the port stops there.
pub const WALK_BUDGET: u64 = 2_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Guide {
    Line { p: P3, d: P3 },
    Point { p: P3 },
}

/// A lamp (0.5 RC.11 (d)), as cleanLight makes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub kind: String,
    pub p: P3,
    pub dir: P3,
    pub color: String,
    pub lumens: f64,
    pub angle: f64,
    pub size: [f64; 2],
    pub on: bool,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub p: P3,
    pub n: P3,
    pub name: String,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionView {
    pub planes: bool,
    pub cuts: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Storey {
    pub name: String,
    pub elevation: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: String,
    pub hidden: bool,
}
/// What a scene shows (0.4 M23).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneVis {
    pub insts: Vec<Vec<f64>>,
    pub tags: Vec<String>,
    pub section: Option<f64>,
    pub cuts: bool,
}
/// A saved camera with its style (cleanScene's shape).
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub vis: Option<SceneVis>,
    pub name: String,
    pub eye: P3,
    pub target: P3,
    pub fov: f64,
    pub persp: bool,
    pub height: f64,
    pub mode: String,
    pub edges: bool,
    pub shadows: bool,
    pub month: f64,
    pub day: f64,
    pub hour: f64,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
}

/// A dimension end: its last known world point, and the vertex it sits on (instance path from the root, vertex id).
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    pub p: P3,
    pub at: Option<(Vec<f64>, f64)>,
}
/// A radial dimension's curve: its last known world centre, radius and normal, and the curve (context path, id).
#[derive(Clone, Debug, PartialEq)]
pub struct RadC {
    pub p: Option<P3>,
    pub r: f64,
    pub n: Option<P3>,
    pub at: Option<(Vec<f64>, String)>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Dim {
    Lin { a: End, b: End, off: P3, mv: String },
    Ang { o: End, a: End, b: End, off: P3 },
    Rad { kind: String, c: RadC, off: P3 },
}

/// An imported image (0.5 RC.11 (b)), kept beside the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub name: String,
    pub mime: String,
    pub data: String,
    pub avg: Option<String>,
}

/// A copy of the model for undo: `head` the model's JSON with defs: 0, `parts` [id, JSON] per definition.
#[derive(Clone, Debug)]
pub struct Snap {
    pub head: Rc<str>,
    pub parts: Vec<(String, PartRef)>,
}
impl Snap {
    /// The whole model as one JSON string (the snapshot).
    pub fn s(&self) -> String {
        let mut defs = String::from("\"defs\":[");
        for (i, (_, p)) in self.parts.iter().enumerate() {
            if i > 0 {
                defs.push(',');
            }
            defs.push_str(&p.text());
        }
        defs.push(']');
        self.head.replacen("\"defs\":0", &defs, 1)
    }
}
/// A Snap or a snapshot string.
#[derive(Clone, Debug)]
pub enum SnapRef {
    Snap(Rc<Snap>),
    Str(Rc<str>),
}
impl SnapRef {
    pub fn s(&self) -> String {
        match self {
            SnapRef::Snap(s) => s.s(),
            SnapRef::Str(s) => s.to_string(),
        }
    }
}
fn same_str(a: &Rc<str>, b: &Rc<str>) -> bool {
    Rc::ptr_eq(a, b) || a == b
}
/// `sameSnap(x, y)`.
pub fn same_snap(x: &SnapRef, y: &SnapRef) -> bool {
    match (x, y) {
        (SnapRef::Snap(a), SnapRef::Snap(b)) => {
            same_str(&a.head, &b.head)
                && a.parts.len() == b.parts.len()
                && a.parts.iter().zip(b.parts.iter()).all(|(p, q)| p.0 == q.0 && same(&p.1, &q.1))
        }
        _ => x.s() == y.s(),
    }
}

/// An undo step: `b` before, `a` after.
#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub b: SnapRef,
    pub a: SnapRef,
    /// Which step this is (the JS compares step objects).
    pub serial: u64,
}

/// A live preview (`_pv`).
#[derive(Clone, Debug)]
struct Preview {
    full: Option<Rc<str>>,
    defs: OMap<String, Value>,
    strs: OMap<String, PartRef>,
    ids: Option<Vec<String>>,
    next_id: Option<f64>,
    mixed: bool,
}

/// The open context: a definition and its world matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Ctx {
    pub def: String,
    pub m: M4,
    pub inv: M4,
}

/// Where a radial dimension's curve is now (radialAt).
#[derive(Clone, Debug, PartialEq)]
pub struct Radial {
    pub c: P3,
    pub r: f64,
    pub n: Option<P3>,
    pub live: bool,
    pub flat: bool,
    pub arc: bool,
}

/// What captureVis gives.
pub type Vis = SceneVis;

/// A walk callback: cb(def, world matrix, path of instance ids, placement colour) → false: do not go inside.
pub type WalkFn<'a> = dyn FnMut(&Def, &M4, &[f64], Option<&str>) -> bool + 'a;

thread_local! {
    static UID_N: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
/// `newUid()`. Spec gap: the JS mixes the clock and Math.random into it; a session id needs only be unique, so this
/// counts (the parity harness sets the id it wants).
pub fn new_uid() -> String {
    let n = UID_N.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        n
    });
    format!("m-{}", js::radix36(n))
}

#[derive(Clone, Debug)]
pub struct Model {
    pub images: OMap<String, Image>,
    pub defs: OMap<String, Def>,
    pub guides: IdMap<Guide>,
    pub lights: IdMap<Light>,
    pub dims: IdMap<Dim>,
    pub sections: IdMap<Section>,
    pub active_section: Option<f64>,
    pub storeys: Vec<Storey>,
    pub axes_m: Option<M4>,
    pub section_view: SectionView,
    pub scenes: Vec<Scene>,
    pub tags: Vec<Tag>,
    pub units: String,
    pub next_id: f64,
    v: f64,
    unsafe_version: f64,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    pub dirty: bool,
    editing: i32,
    pv: Option<Preview>,
    pub uid: String,
    serial: u64,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        let mut m = Model {
            images: OMap::new(),
            defs: OMap::new(),
            guides: IdMap::new(),
            lights: IdMap::new(),
            dims: IdMap::new(),
            sections: IdMap::new(),
            active_section: None,
            storeys: Vec::new(),
            axes_m: None,
            section_view: SectionView { planes: true, cuts: true },
            scenes: Vec::new(),
            tags: Vec::new(),
            units: "ft-in".into(),
            next_id: 1.0,
            v: 0.0,
            unsafe_version: 0.0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            dirty: false,
            editing: 0,
            pv: None,
            uid: String::new(),
            serial: 0,
        };
        m.reset();
        m
    }

    pub fn reset(&mut self) {
        self.defs = OMap::new();
        self.defs.set("root".into(), Def::new("root", "root", "Model"));
        self.guides = IdMap::new();
        self.lights = IdMap::new();
        self.dims = IdMap::new();
        self.sections = IdMap::new();
        self.active_section = None;
        self.storeys = Vec::new();
        self.axes_m = None;
        self.section_view = SectionView { planes: true, cuts: true };
        self.scenes = Vec::new();
        self.tags = Vec::new();
        self.units = "ft-in".into();
        self.next_id = 1.0;
        self.set_version(0.0);
        self.undo_stack = Vec::new();
        self.redo_stack = Vec::new();
        self.dirty = false;
        self.editing = 0;
        self.pv = None;
        self.uid = new_uid();
    }

    // ---- images (0.5 RC.11 (b)) ----

    /// Keep an imported image; the same bytes give the same id.
    pub fn add_image(&mut self, name: &str, mime: &str, data: &str, avg: Option<&str>) -> Res<String> {
        let mime_ok = matches!(mime, "image/png" | "image/jpeg" | "image/webp");
        if !mime_ok || !is_b64(data) || data.len() as f64 > 24e6 {
            return Err(msg("m3d.model.bad_image"));
        }
        for (id, im) in self.images.iter() {
            if im.data == data {
                return Ok(id.clone());
            }
        }
        let mut h: u32 = 0;
        for &c in data.as_bytes().iter().step_by(97) {
            let m = i64::from(h.cast_signed().wrapping_mul(31));
            h = (m + i64::from(c)).rem_euclid(1 << 32).try_into().unwrap_or(0);
        }
        let mut id = format!("t{}", js::radix36(u64::from(h)));
        while self.images.has(&id) {
            id.push('x');
        }
        let avg = avg.filter(|a| is_color(a)).map(str::to_ascii_lowercase);
        self.images.set(id.clone(), Image { name: js::clean_text(name, 120), mime: mime.into(), data: data.into(), avg });
        Ok(id)
    }
    /// The images some face or placement still uses.
    pub fn images_json(&self) -> Vec<(String, Image)> {
        let mut used = std::collections::HashSet::new();
        for d in self.defs.values() {
            for f in d.faces.values() {
                if let Some(c) = f.color.as_deref().filter(|c| is_image_paint(c)) {
                    used.insert(image_id(c));
                }
            }
            for i in d.insts.values() {
                if let Some(c) = i.color.as_deref().filter(|c| is_image_paint(c)) {
                    used.insert(image_id(c));
                }
            }
        }
        self.images.iter().filter(|(id, _)| used.contains(*id)).map(|(id, im)| (id.clone(), im.clone())).collect()
    }
    /// Images from a file (untrusted): replaces the table; at most 256 images, 96 MB.
    pub fn set_images(&mut self, list: &Value) {
        self.images.clear();
        let mut total = 0.0;
        for im in list.as_array().map_or(&[][..], |a| a.get(..256).unwrap_or(a)) {
            let Some(id) = json::str(im.get("id"))
                .filter(|s| (1..=24).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
            else {
                continue;
            };
            let Some(mime) = json::str(im.get("mime")).filter(|m| matches!(*m, "image/png" | "image/jpeg" | "image/webp")) else {
                continue;
            };
            let Some(data) = json::str(im.get("data")).filter(|d| is_b64(d)) else { continue };
            total += data.len() as f64;
            if total > 96e6 {
                break;
            }
            let avg = json::str(im.get("avg")).filter(|a| is_color(a)).map(str::to_ascii_lowercase);
            self.images.set(id.into(), Image { name: json::clean(im.get("name"), 120), mime: mime.into(), data: data.into(), avg });
        }
    }

    // ---- definitions ----

    pub fn root(&self) -> Option<&Def> {
        self.defs.get("root")
    }
    pub fn root_mut(&mut self) -> Option<&mut Def> {
        self.defs.get_mut("root")
    }
    pub fn def(&self, id: &str) -> Option<&Def> {
        self.defs.get(id)
    }
    pub fn def_mut(&mut self, id: &str) -> Option<&mut Def> {
        self.defs.get_mut(id)
    }
    /// `newDef(kind, name)` → the new definition's id.
    pub fn new_def(&mut self, kind: &str, name: Option<&str>) -> String {
        let id = format!("d{}", js::num_str(self.next_id));
        self.next_id += 1.0;
        let nm = match name.filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => self.unique_name(if kind == "component" { "Component" } else { "Group" }),
        };
        self.defs.set(id.clone(), Def::new(&id, kind, &nm));
        id
    }
    pub fn unique_name(&self, base: &str) -> String {
        let names: std::collections::HashSet<&str> = self.defs.values().map(|d| d.name.as_str()).collect();
        let mut i = 1u64;
        loop {
            let n = format!("{base}#{i}");
            if !names.contains(n.as_str()) {
                return n;
            }
            i = i.saturating_add(1);
        }
    }
    pub fn new_guide_id(&mut self) -> f64 {
        let id = self.next_id;
        self.next_id += 1.0;
        id
    }

    /// Move placement iid of the group open at `from` into the group open at `to`, keeping where it is in the world.
    pub fn move_into(&mut self, from: &[f64], iid: f64, to: &[f64]) -> Res<f64> {
        let src = self.context(from);
        let dst = self.context(to);
        let inst = src.as_ref().and_then(|s| self.defs.get(&s.def)).and_then(|d| d.insts.get(iid)).cloned();
        let (Some(src), Some(dst), Some(inst)) = (src, dst, inst) else { return Err(msg("m3d.model.move_gone")) };
        // everything inside the moved definition, itself included
        let mut reach: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.reach_from(&inst.def, &mut reach);
        let mut d = "root".to_string();
        for &p in to {
            let Some(i) = self.defs.get(&d).and_then(|dd| dd.insts.get(p)) else { break };
            if reach.contains(&i.def) {
                return Err(msg("m3d.model.move_into_itself"));
            }
            d = i.def.clone();
        }
        if src.def == dst.def {
            return Ok(iid);
        }
        let m = mmul(&dst.inv, &mmul(&src.m, &inst.m));
        if let Some(sd) = self.defs.get_mut(&src.def) {
            sd.insts.delete(iid);
        }
        let Some(dd) = self.defs.get_mut(&dst.def) else { return Err(Error::Type) };
        let nidd = place(dd, &inst.def, &m);
        if let Some(ni) = dd.insts.get_mut(nidd) {
            ni.name = inst.name.clone();
            ni.color = inst.color.clone();
            ni.hidden = inst.hidden;
            ni.tag = inst.tag.clone();
            ni.rule = inst.rule.clone();
        }
        Ok(nidd)
    }
    /// Every definition reachable from id (64 levels at most), id included.
    fn reach_from(&self, id: &str, reach: &mut std::collections::HashSet<String>) {
        fn visit(me: &Model, id: &str, depth: u32, reach: &mut std::collections::HashSet<String>) {
            if reach.contains(id) || depth > 64 {
                return;
            }
            reach.insert(id.to_string());
            if let Some(d) = me.defs.get(id) {
                for i in d.insts.values() {
                    visit(me, &i.def, depth + 1, reach);
                }
            }
        }
        visit(self, id, 0, reach);
    }

    // ---- what scenes show (0.4 M23) ----

    /// What is hidden now.
    pub fn capture_vis(&self) -> Vis {
        let mut insts = Vec::new();
        // the same placement budget as walk (Spec gap there)
        fn visit(me: &Model, def: &Def, path: &[f64], depth: u32, out: &mut Vec<Vec<f64>>, budget: &mut u64) {
            if depth > 32 {
                return;
            }
            for (iid, inst) in def.insts.iter() {
                if *budget == 0 {
                    return;
                }
                *budget -= 1;
                let mut p = path.to_vec();
                p.push(iid);
                if inst.hidden {
                    out.push(p.clone());
                }
                if let Some(d) = me.defs.get(&inst.def) {
                    visit(me, d, &p, depth + 1, out, budget);
                }
            }
        }
        if let Some(r) = self.root() {
            visit(self, r, &[], 0, &mut insts, &mut { WALK_BUDGET });
        }
        Vis {
            insts,
            tags: self.tags.iter().filter(|g| g.hidden).map(|g| g.name.clone()).collect(),
            section: self.active_section,
            cuts: self.section_view.cuts,
        }
    }
    /// Show what a scene saved.
    pub fn apply_vis(&mut self, vis: &Vis) {
        for d in self.defs.values_mut() {
            for i in d.insts.values_mut() {
                i.hidden = false;
            }
        }
        for p in &vis.insts {
            let mut def = Some("root".to_string());
            let mut hit: Option<(String, f64)> = None;
            for &iid in p {
                let Some(dd) = def.as_deref().and_then(|d| self.defs.get(d)) else {
                    hit = None;
                    break;
                };
                let Some(inst) = dd.insts.get(iid) else {
                    hit = None;
                    break;
                };
                hit = Some((dd.id.clone(), iid));
                def = Some(inst.def.clone());
            }
            if let Some((d, iid)) = hit
                && let Some(i) = self.defs.get_mut(&d).and_then(|dd| dd.insts.get_mut(iid))
            {
                i.hidden = true;
            }
        }
        let off: std::collections::HashSet<String> = vis.tags.iter().map(|x| x.to_lowercase()).collect();
        for g in self.tags.iter_mut() {
            g.hidden = off.contains(&g.name.to_lowercase());
        }
        self.active_section = vis.section.filter(|s| self.sections.has(*s));
        self.section_view.cuts = vis.cuts;
        self.bump_version();
        self.dirty = true;
    }

    // ---- drawing axes (0.4 M20) ----

    pub fn axes_frame(&self, path: &[f64]) -> M4 {
        if !path.is_empty() {
            return self.context(path).or_else(|| self.context(&[])).map_or(IDENT, |c| c.m);
        }
        self.axes_m.unwrap_or(IDENT)
    }
    pub fn set_axes(&mut self, o: P3, x: P3, y: P3) -> Res<()> {
        let xn = norm(x);
        let y0 = sub(y, mul(xn, dot(y, xn)));
        if !js::truthy(len(xn)) || len(y0) < 1e-9 {
            return Err(msg("m3d.axes.bad"));
        }
        let yn = norm(y0);
        let z = cross(xn, yn);
        let m = [xn[0], xn[1], xn[2], 0.0, yn[0], yn[1], yn[2], 0.0, z[0], z[1], z[2], 0.0, o[0], o[1], o[2], 1.0];
        let same = m.iter().zip(IDENT.iter()).all(|(v, i)| (v - i).abs() < 1e-12);
        self.axes_m = if same { None } else { Some(m) };
        Ok(())
    }
    pub fn reset_axes(&mut self) {
        self.axes_m = None;
    }

    // ---- section planes (0.4 M7) ----

    pub fn add_section(&mut self, p: P3, n: P3, name: &str) -> f64 {
        let id = self.new_guide_id();
        let mut nm = js::clean_text(name, 80);
        if nm.is_empty() {
            let mut k = self.sections.len() as f64 + 1.0;
            let used: std::collections::HashSet<String> = self.sections.values().map(|s| s.name.clone()).collect();
            while used.contains(&t("m3d.section.name", &[("n", &js::num_str(k))])) {
                k += 1.0;
            }
            nm = t("m3d.section.name", &[("n", &js::num_str(k))]);
        }
        self.sections.set(id, Section { p, n: norm(n), name: nm });
        self.active_section = Some(id);
        self.section_view.cuts = true;
        id
    }
    /// The cut that shows now: (id, p, n) or None.
    pub fn active_cut(&self) -> Option<(f64, P3, P3)> {
        if !self.section_view.cuts {
            return None;
        }
        let id = self.active_section?;
        self.sections.get(id).map(|s| (id, s.p, s.n))
    }
    pub fn delete_section(&mut self, id: f64) {
        self.sections.delete(id);
        if self.active_section.is_some_and(|a| kb(a) == kb(id)) {
            self.active_section = None;
        }
    }

    // ---- versions ----

    pub fn version(&self) -> f64 {
        self.v
    }
    fn set_version(&mut self, v: f64) {
        self.v = v;
        self.unsafe_version = v;
    }
    /// `this.version++`.
    pub fn bump_version(&mut self) {
        let v = self.v + 1.0;
        self.set_version(v);
    }
    pub fn bump_safe(&mut self) {
        self.v += 1.0;
    }
    pub fn editing(&self) -> bool {
        self.editing > 0
    }

    // ---- undo ----

    fn new_step(&mut self, label: &str, b: SnapRef, a: SnapRef) -> Step {
        self.serial = self.serial.wrapping_add(1);
        Step { label: label.to_string(), b, a, serial: self.serial }
    }
    fn push_step(&mut self, step: Step) {
        self.undo_stack.push(step);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.dirty = true;
    }

    /// Run f(model) as one undo step labelled `label`. On an error the model goes back to its snapshot from before
    /// (as the JS does: see the module header) and the error is returned.
    pub fn change<R>(&mut self, label: &str, f: impl FnOnce(&mut Model) -> Res<R>) -> Res<R> {
        let before = self.snap();
        self.editing += 1;
        let r = f(self);
        self.editing -= 1;
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                self.restore(&before)?;
                return Err(e);
            }
        };
        self.bump_version();
        let after = self.snap();
        if !same_snap(&after, &before) {
            let step = self.new_step(label, before, after);
            self.push_step(step);
        }
        Ok(r)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    pub fn undo(&mut self) -> Res<Option<String>> {
        let Some(s) = self.undo_stack.pop() else { return Ok(None) };
        self.restore(&s.b)?;
        let label = s.label.clone();
        self.redo_stack.push(s);
        self.bump_safe();
        self.dirty = true;
        Ok(Some(label))
    }
    pub fn redo(&mut self) -> Res<Option<String>> {
        let Some(s) = self.redo_stack.pop() else { return Ok(None) };
        self.restore(&s.a)?;
        let label = s.label.clone();
        self.undo_stack.push(s);
        self.bump_safe();
        self.dirty = true;
        Ok(Some(label))
    }
    /// The newest undo step's serial (a tool keeps it to amend it).
    pub fn last_step(&self) -> Option<u64> {
        self.undo_stack.last().map(|s| s.serial)
    }
    /// Do the newest undo step again as f(model) from its own start. False — and nothing changed — when `step` is no
    /// longer the newest, a preview is open, or f fails.
    pub fn amend_last(&mut self, step: Option<u64>, f: impl FnOnce(&mut Model) -> Res<()>) -> Res<bool> {
        let Some(step) = step else { return Ok(false) };
        if self.last_step() != Some(step) || self.pv.is_some() {
            return Ok(false);
        }
        let Some(b) = self.undo_stack.last().map(|s| s.b.clone()) else { return Ok(false) };
        self.restore(&b)?;
        self.editing += 1;
        let r = f(self);
        self.editing -= 1;
        if r.is_err() {
            if let Some(a) = self.undo_stack.last().map(|s| s.a.clone()) {
                self.restore(&a)?;
            }
            self.bump_safe();
            return Ok(false);
        }
        self.bump_version();
        let a = self.snap();
        if let Some(s) = self.undo_stack.last_mut() {
            s.a = a;
        }
        self.redo_stack.clear();
        self.dirty = true;
        Ok(true)
    }

    // ---- live previews ----

    pub fn begin(&mut self) {
        self.pv = Some(Preview {
            full: None,
            defs: OMap::new(),
            strs: OMap::new(),
            ids: Some(self.defs.keys().cloned().collect()),
            next_id: Some(self.next_id),
            mixed: false,
        });
    }
    pub fn previewing(&self) -> bool {
        self.pv.is_some()
    }
    /// The preview's starting copy: a Snap while only scoped frames ran, else the whole as a string.
    fn base_snap(&mut self) -> Res<SnapRef> {
        let Some(pv) = self.pv.as_ref() else { return Err(Error::Type) };
        if let Some(f) = &pv.full {
            return Ok(SnapRef::Str(f.clone()));
        }
        let next_id = pv.next_id;
        let ids = pv.ids.clone().unwrap_or_default();
        let mut head = Out::new(false);
        self.to_json_out(&mut head, true);
        // j.nextId = pv.nextId
        let head = match next_id {
            Some(n) => set_next_id(&head.s, n),
            None => head.s,
        };
        let mut parts = Vec::with_capacity(ids.len());
        for id in ids {
            let has = self.pv.as_ref().is_some_and(|pv| pv.defs.has(&id));
            if !has {
                let Some(d) = self.defs.get(&id) else { return Err(Error::Type) };
                let s = self.def_string_of(d);
                parts.push((id, s));
                continue;
            }
            let Some(pv) = self.pv.as_mut() else { return Err(Error::Type) };
            let s = match pv.strs.get(&id) {
                Some(s) => s.clone(),
                None => {
                    let mut o = Out::new(false);
                    o.value(pv.defs.get(&id).unwrap_or(&Value::Null));
                    let s = Part::text_only(o.s.into());
                    pv.strs.set(id.clone(), s.clone());
                    s
                }
            };
            parts.push((id, s));
        }
        Ok(SnapRef::Snap(Rc::new(Snap { head: head.into(), parts })))
    }
    /// `get _base()`.
    pub fn base(&mut self) -> Res<Option<Rc<str>>> {
        let Some(pv) = self.pv.as_ref() else { return Ok(None) };
        if pv.full.is_none() {
            let s: Rc<str> = self.base_snap()?.s().into();
            if let Some(pv) = self.pv.as_mut() {
                pv.full = Some(s);
            }
        }
        Ok(self.pv.as_ref().and_then(|p| p.full.clone()))
    }
    /// `set _base(s)`.
    pub fn set_base(&mut self, s: Option<&str>) {
        self.pv = s.map(|s| Preview { full: Some(s.into()), defs: OMap::new(), strs: OMap::new(), ids: None, next_id: None, mixed: true });
    }
    /// Back to the preview's start: scoped (the definitions changed so far, plus `more`) or whole.
    fn revert_preview(&mut self, more: &[String]) -> Res<bool> {
        let Some(pv) = self.pv.as_ref() else { return Err(Error::Type) };
        if pv.mixed {
            let Some(full) = pv.full.clone() else { return Err(Error::Type) };
            self.restore(&SnapRef::Str(full))?;
            return Ok(false);
        }
        let ids = pv.ids.clone().unwrap_or_default();
        let idset: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let dead: Vec<String> = self.defs.keys().filter(|k| !idset.contains(k.as_str())).cloned().collect();
        for id in dead {
            self.defs.delete(&id);
        }
        for id in more {
            let skip = self.pv.as_ref().is_none_or(|pv| !idset.contains(id.as_str()) || pv.defs.has(id));
            if skip {
                continue;
            }
            let Some(d) = self.defs.get(id) else { return Err(Error::Type) };
            let v = def_value(d);
            let kept = d.cache.as_ref().filter(|c| c.0 >= self.unsafe_version).map(|c| c.1.clone());
            if let Some(pv) = self.pv.as_mut() {
                pv.defs.set(id.clone(), v);
                if let Some(s) = kept {
                    pv.strs.set(id.clone(), s);
                }
            }
        }
        let list: Vec<(String, Value)> =
            self.pv.as_ref().map(|pv| pv.defs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        for (id, j) in list {
            let d = def_of(&j, Fail::Plain)?;
            self.defs.set(id, d);
        }
        if let Some(n) = self.pv.as_ref().and_then(|p| p.next_id) {
            self.next_id = n;
        }
        Ok(true)
    }
    /// One preview frame: f(model) from the preview's start. `scope` lists the only definitions f changes.
    pub fn preview<R>(&mut self, scope: Option<&[String]>, f: impl FnOnce(&mut Model) -> Res<R>) -> Res<Option<R>> {
        let Some(mixed) = self.pv.as_ref().map(|p| p.mixed) else { return Err(Error::Type) };
        match scope {
            Some(sc) if !mixed => {
                self.revert_preview(sc)?;
            }
            _ => {
                let full = self.base()?;
                let Some(full) = full else { return Err(Error::Type) };
                self.restore(&SnapRef::Str(full))?;
                if let Some(pv) = self.pv.as_mut() {
                    pv.mixed = true;
                }
            }
        }
        self.editing += 1;
        let r = f(self);
        self.editing -= 1;
        let r = match r {
            Ok(r) => Some(r),
            Err(_) => {
                self.revert_preview(&[])?;
                None
            }
        };
        if self.pv.as_ref().is_some_and(|p| p.mixed) {
            self.bump_version();
        } else {
            self.bump_safe();
        }
        Ok(r)
    }
    pub fn commit(&mut self, label: &str) -> Res<()> {
        if self.pv.is_some() {
            let before = self.base_snap()?;
            let after = self.snap();
            if !same_snap(&after, &before) {
                let step = self.new_step(label, before, after);
                self.push_step(step);
            }
        }
        self.pv = None;
        self.bump_safe();
        Ok(())
    }
    pub fn abort(&mut self) -> Res<()> {
        let safe = if self.pv.is_some() { self.revert_preview(&[])? } else { true };
        self.pv = None;
        if safe {
            self.bump_safe();
        } else {
            self.bump_version();
        }
        Ok(())
    }

    // ---- snapshots ----

    /// A definition as JSON, kept with it until it may have been edited in place (`defString`).
    pub fn def_string(&mut self, id: &str) -> Option<PartRef> {
        let editing = self.editing > 0;
        let (v, uv) = (self.v, self.unsafe_version);
        let d = self.defs.get_mut(id)?;
        if editing {
            return Some(Part::of(d));
        }
        if let Some((cv, s)) = &d.cache
            && *cv >= uv
        {
            return Some(s.clone());
        }
        let s = Part::of(d);
        d.cache = Some((v, s.clone()));
        Some(s)
    }
    /// defString for a definition held by reference (no caching while editing; else the cache is read only).
    fn def_string_of(&self, d: &Def) -> PartRef {
        if self.editing == 0
            && let Some((cv, s)) = &d.cache
            && *cv >= self.unsafe_version
        {
            return s.clone();
        }
        Part::of(d)
    }
    /// Make definitions' JSON ahead of time while `more()`. → true when done.
    pub fn warm_snap(&mut self, mut more: impl FnMut() -> bool) -> bool {
        let ids: Vec<String> = self.defs.keys().cloned().collect();
        for id in ids {
            let fresh = self.defs.get(&id).is_some_and(|d| d.cache.as_ref().is_some_and(|c| c.0 >= self.unsafe_version));
            if fresh {
                continue;
            }
            if self.editing > 0 || self.pv.is_some() || !more() {
                return false;
            }
            self.def_string(&id);
        }
        true
    }
    /// A copy of the model for undo.
    pub fn snap(&mut self) -> SnapRef {
        let head = self.to_json_string(true);
        let ids: Vec<String> = self.defs.values().map(|d| d.id.clone()).collect();
        let keys: Vec<String> = self.defs.keys().cloned().collect();
        let mut parts = Vec::with_capacity(ids.len());
        for (id, k) in ids.into_iter().zip(keys) {
            let Some(s) = self.def_string(&k) else { continue };
            parts.push((id, s));
        }
        SnapRef::Snap(Rc::new(Snap { head: head.into(), parts }))
    }
    /// The whole model as one JSON string.
    pub fn snapshot(&mut self) -> String {
        self.snap().s()
    }
    /// Back to a copy (a Snap or a snapshot string).
    pub fn restore(&mut self, s: &SnapRef) -> Res<()> {
        let snap = match s {
            SnapRef::Str(st) => {
                let j: Value = serde_json::from_str(st).map_err(|_| Error::Type)?;
                return self.load_with(&j, true, None);
            }
            SnapRef::Snap(sn) => sn.clone(),
        };
        let mut list: Vec<(String, Option<Def>)> = Vec::with_capacity(snap.parts.len());
        let mut made: Vec<(String, PartRef)> = Vec::new();
        for (id, st) in &snap.parts {
            let keep = self.defs.get(id).and_then(|d| d.cache.as_ref()).is_some_and(|c| same(&c.1, st) && c.0 >= self.unsafe_version);
            if keep {
                list.push((id.clone(), None));
                continue;
            }
            let d = st.restore()?;
            list.push((id.clone(), Some(d)));
            made.push((id.clone(), st.clone()));
        }
        let head: Value = serde_json::from_str(&snap.head).map_err(|_| Error::Type)?;
        self.load_with(&head, true, Some(list))?;
        let v = self.v;
        for (id, st) in made {
            if let Some(d) = self.defs.get_mut(&id) {
                d.cache = Some((v, st));
            }
        }
        Ok(())
    }

    // ---- instances ----

    /// Every definition placed under def: cb(def, world matrix, path, colour); cb → false: do not go inside.
    pub fn walk(&self, def: &str, m: &M4, cb: &mut WalkFn<'_>) {
        let Some(d) = self.defs.get(def) else { return };
        self.walk_in(d, m, &mut Vec::new(), 0, None, None, cb, &mut { WALK_BUDGET });
    }
    /// walk() without hidden placements and hidden tags: what is on screen.
    pub fn walk_shown(&self, def: &str, m: &M4, cb: &mut WalkFn<'_>) {
        let Some(d) = self.defs.get(def) else { return };
        let off = self.hidden_tags();
        self.walk_in(d, m, &mut Vec::new(), 0, None, Some(&off), cb, &mut { WALK_BUDGET });
    }
    #[allow(clippy::too_many_arguments)]
    fn walk_in(
        &self,
        d: &Def,
        m: &M4,
        path: &mut Vec<f64>,
        depth: u32,
        color: Option<&str>,
        off: Option<&std::collections::HashSet<String>>,
        cb: &mut WalkFn<'_>,
        budget: &mut u64,
    ) {
        if depth > 32 || *budget == 0 {
            return;
        }
        *budget -= 1;
        if !cb(d, m, path, color) {
            return;
        }
        for (iid, inst) in d.insts.iter() {
            if let Some(off) = off
                && (inst.hidden || inst.tag.as_deref().is_some_and(|t| !t.is_empty() && off.contains(t)))
            {
                continue;
            }
            let Some(sub) = self.defs.get(&inst.def) else { continue };
            path.push(iid);
            let c = inst.color.as_deref().filter(|c| !c.is_empty()).or(color);
            self.walk_in(sub, &mmul(m, &inst.m), path, depth + 1, c, off, cb, budget);
            path.pop();
        }
    }

    // ---- tags (0.4 M6) ----

    pub fn hidden_tags(&self) -> std::collections::HashSet<String> {
        self.tags.iter().filter(|g| g.hidden).map(|g| g.name.clone()).collect()
    }
    /// The tag of that name (any case) → its index.
    pub fn tag(&self, name: &str) -> Option<usize> {
        let k = name.to_lowercase();
        self.tags.iter().position(|g| g.name.to_lowercase() == k)
    }
    /// A new tag (a free name from `name`, "Tag" by default). → its name.
    pub fn add_tag(&mut self, name: &str) -> String {
        let mut base = js::clean_text(name, 80);
        if base.is_empty() {
            base = t("m3d.tag.default", &[]);
        }
        let mut nm = base.clone();
        let mut k = 2u64;
        while self.tag(&nm).is_some() {
            nm = format!("{base} {k}");
            k = k.saturating_add(1);
        }
        self.tags.push(Tag { name: nm.clone(), hidden: false });
        nm
    }
    pub fn rename_tag(&mut self, old: &str, name: &str) -> Res<String> {
        let g = self.tag(old);
        let nm = js::clean_text(name, 80);
        let Some(g) = g else { return Err(msg("m3d.tag.gone")) };
        if nm.is_empty() {
            return Err(msg("m3d.tag.empty"));
        }
        if let Some(o) = self.tag(&nm)
            && o != g
        {
            return Err(msg_with("m3d.tag.taken", &[("name", &nm)]));
        }
        let gname = self.tags.get(g).map(|x| x.name.clone()).unwrap_or_default();
        for d in self.defs.values_mut() {
            for i in d.insts.values_mut() {
                if i.tag.as_deref() == Some(gname.as_str()) {
                    i.tag = Some(nm.clone());
                }
            }
        }
        if let Some(x) = self.tags.get_mut(g) {
            x.name = nm.clone();
        }
        Ok(nm)
    }
    /// Delete a tag: what had it becomes untagged. → how many placements had it.
    pub fn delete_tag(&mut self, name: &str) -> f64 {
        let Some(g) = self.tag(name) else { return 0.0 };
        let gname = self.tags.get(g).map(|x| x.name.clone()).unwrap_or_default();
        let mut n = 0.0;
        for d in self.defs.values_mut() {
            for i in d.insts.values_mut() {
                if i.tag.as_deref() == Some(gname.as_str()) {
                    i.tag = None;
                    n += 1.0;
                }
            }
        }
        self.tags.remove(g);
        n
    }
    pub fn set_tag_hidden(&mut self, name: &str, hidden: bool) {
        if let Some(g) = self.tag(name)
            && let Some(x) = self.tags.get_mut(g)
        {
            x.hidden = hidden;
        }
    }
    /// Give placements iids of def a tag (None / '': untagged).
    pub fn set_tag(&mut self, def: &str, iids: &[f64], name: Option<&str>) -> Res<()> {
        let name = name.filter(|n| !n.is_empty());
        let g = name.and_then(|n| self.tag(n));
        if name.is_some() && g.is_none() {
            return Err(msg("m3d.tag.gone"));
        }
        let gname = g.and_then(|g| self.tags.get(g)).map(|x| x.name.clone());
        let Some(d) = self.defs.get_mut(def) else { return Err(Error::Type) };
        for &iid in iids {
            if let Some(i) = d.insts.get_mut(iid) {
                i.tag = gname.clone();
            }
        }
        Ok(())
    }
    /// How many placements carry each tag: (name, n) ('' = untagged).
    pub fn tag_counts(&self) -> Vec<(String, f64)> {
        let mut out: OMap<String, f64> = OMap::new();
        out.set(String::new(), 0.0);
        for g in &self.tags {
            out.set(g.name.clone(), 0.0);
        }
        for d in self.defs.values() {
            for i in d.insts.values() {
                let k = i.tag.clone().unwrap_or_default();
                let n = out.get(&k).copied().unwrap_or(0.0);
                out.set(k, n + 1.0);
            }
        }
        out.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
    /// Add a tag for every name a placement carries that the list lacks.
    pub fn ensure_tags(&mut self) {
        let ids: Vec<String> = self.defs.keys().cloned().collect();
        for id in ids {
            let iids: Vec<f64> = self.defs.get(&id).map(|d| d.insts.key_vec()).unwrap_or_default();
            for iid in iids {
                let Some(tag) = self.defs.get(&id).and_then(|d| d.insts.get(iid)).and_then(|i| i.tag.clone()).filter(|t| !t.is_empty())
                else {
                    continue;
                };
                let g = self.tag(&tag);
                let newv = match g {
                    Some(g) => self.tags.get(g).map(|x| x.name.clone()),
                    None if self.tags.len() < 1000 => {
                        self.tags.push(Tag { name: tag.clone(), hidden: false });
                        Some(tag)
                    }
                    None => None,
                };
                if let Some(i) = self.defs.get_mut(&id).and_then(|d| d.insts.get_mut(iid)) {
                    i.tag = newv;
                }
            }
        }
    }
    /// The colour a placement path gives the faces inside it that have none of their own.
    pub fn path_color(&self, path: &[f64]) -> Option<String> {
        let mut def = Some("root".to_string());
        let mut color = None;
        for &iid in path {
            let Some(inst) = def.as_deref().and_then(|d| self.defs.get(d)).and_then(|d| d.insts.get(iid)) else { break };
            if let Some(c) = inst.color.as_deref().filter(|c| !c.is_empty()) {
                color = Some(c.to_string());
            }
            def = Some(inst.def.clone());
        }
        color
    }
    /// Instances of every definition: (def id, count) in order of first use.
    pub fn instance_counts(&self) -> OMap<String, f64> {
        let mut c: OMap<String, f64> = OMap::new();
        for d in self.defs.values() {
            for inst in d.insts.values() {
                let n = c.get(&inst.def).copied().unwrap_or(0.0);
                c.set(inst.def.clone(), n + 1.0);
            }
        }
        c
    }
    /// The definition and world matrix at the end of a path of instance ids (the editing context).
    pub fn context(&self, path: &[f64]) -> Option<Ctx> {
        let mut def = self.defs.get("root")?;
        let mut m = IDENT;
        for &iid in path {
            let inst = def.insts.get(iid)?;
            m = mmul(&m, &inst.m);
            def = self.defs.get(&inst.def)?;
        }
        Some(Ctx { def: def.id.clone(), m, inv: invert(&m) })
    }
    pub fn bbox(&self, def: &str, m: &M4) -> Option<BBox> {
        let mut bb = None;
        self.walk(def, m, &mut |d, wm, _, _| {
            for p in d.verts.values() {
                bb = Some(bbox_add(bb, apply(wm, *p)));
            }
            true
        });
        bb
    }
    pub fn def_bbox(&self, def: &str) -> Option<BBox> {
        self.bbox(def, &IDENT)
    }

    /// Make a group or component of a selection in `def`. → the new instance id.
    pub fn make_group(&mut self, def: &str, sel: &[Sel], kind: &str, name: Option<&str>) -> Res<Option<f64>> {
        let Some(d) = self.defs.get(def) else { return Err(Error::Type) };
        let fids: Vec<f64> = sel.iter().filter(|s| s.is("f")).map(|s| s.id).filter(|&id| d.faces.has(id)).collect();
        let mut eids = IdSet::from_iter_ids(sel.iter().filter(|s| s.is("e")).map(|s| s.id).filter(|&id| d.edges.has(id)));
        let iids: Vec<f64> = sel.iter().filter(|s| s.is("i")).map(|s| s.id).filter(|&id| d.insts.has(id)).collect();
        {
            let Some(dm) = self.defs.get_mut(def) else { return Err(Error::Type) };
            for &fid in &fids {
                let Some(f) = dm.faces.get(fid).cloned() else { continue };
                for (a, b) in loop_segs(&f) {
                    if let Some(e) = edge_between(dm, a, b) {
                        eids.add(e);
                    }
                }
            }
        }
        if fids.is_empty() && eids.is_empty() && iids.is_empty() {
            return Ok(None);
        }
        let Some(d) = self.defs.get(def) else { return Err(Error::Type) };
        let mut bb: Option<BBox> = None;
        for eid in eids.iter() {
            let Some(e) = d.edges.get(eid) else { continue };
            bb = Some(bbox_add(bb, d.v(e.a)));
            bb = Some(bbox_add(bb, d.v(e.b)));
        }
        for &iid in &iids {
            let Some(inst) = d.insts.get(iid) else { continue };
            if let Some(ib) = self.bbox(&inst.def, &inst.m) {
                bb = Some(bbox_add(bb, ib.min));
                bb = Some(bbox_add(bb, ib.max));
            }
        }
        let origin = bb.map_or([0.0, 0.0, 0.0], |b| b.min);
        let nd_id = self.new_def(kind, name);
        let Some(mut nd) = self.defs.get(&nd_id).cloned() else { return Err(Error::Type) };
        let Some(mut dm) = self.defs.get(def).cloned() else { return Err(Error::Type) };
        let mut vmap: FxMap<u64, f64> = FxMap::default();
        let cv = |nd: &mut Def, vmap: &mut FxMap<u64, f64>, dm: &Def, v: f64| -> f64 {
            if let Some(&x) = vmap.get(&kb(v)) {
                return x;
            }
            let id = v_add(nd, sub(dm.v(v), origin));
            vmap.insert(kb(v), id);
            id
        };
        for eid in eids.iter() {
            let Some(e) = dm.edges.get(eid).cloned() else { continue };
            let a = cv(&mut nd, &mut vmap, &dm, e.a);
            let b = cv(&mut nd, &mut vmap, &dm, e.b);
            e_add(&mut nd, a, b, e.soft, e.curve.clone(), e.hidden);
        }
        let mut cids: Vec<String> = Vec::new();
        for eid in eids.iter() {
            if let Some(c) = dm.edges.get(eid).and_then(|e| e.curve.clone())
                && dm.splines.has(&c)
                && !cids.contains(&c)
            {
                cids.push(c);
            }
        }
        for cid in cids {
            if let Some(sp) = dm.splines.get(&cid) {
                let sp = Spline { pts: sp.pts.iter().map(|p| sub(*p, origin)).collect(), ..sp.clone() };
                nd.splines.set(cid, sp);
            }
        }
        for &fid in &fids {
            let Some(f) = dm.faces.get(fid).cloned() else { continue };
            let loops: Vec<Vec<f64>> = f.loops.iter().map(|l| l.iter().map(|&v| cv(&mut nd, &mut vmap, &dm, v)).collect()).collect();
            f_add(&mut nd, loops, f.color.clone(), None, f.hidden);
        }
        let back = translation(neg(origin));
        let mut err = None;
        for &iid in &iids {
            let Some(inst) = dm.insts.get(iid).cloned() else {
                // a placement selected twice: the JS reads `undefined.m` the second time
                err = Some(Error::Type);
                break;
            };
            let id = nid(&mut nd);
            nd.insts.set(id, Inst { m: mmul(&back, &inst.m), ..inst });
            dm.insts.delete(iid);
        }
        if let Some(e) = err {
            self.defs.set(def.to_string(), dm);
            self.defs.set(nd_id, nd);
            return Err(e);
        }
        for &fid in &fids {
            dm.faces.delete(fid);
        }
        let still = edge_faces(&dm);
        for eid in eids.iter() {
            let Some(e) = dm.edges.get(eid) else { continue };
            if !still.has(&key(e.a, e.b)) {
                e_remove(&mut dm, eid);
            }
        }
        remove_loose_verts(&mut dm);
        prune_splines(&mut dm);
        let iid = nid(&mut dm);
        dm.insts.set(iid, Inst::new(nd_id.clone(), translation(origin)));
        self.put_def(def, dm);
        self.put_def(&nd_id, nd);
        Ok(Some(iid))
    }
    /// Put a definition back in its slot (its place in the table stays).
    fn put_def(&mut self, id: &str, d: Def) {
        if let Some(slot) = self.defs.get_mut(id) {
            *slot = d;
        }
    }

    /// Explode an instance into its parent: its geometry joins the parent's. → the new selection.
    pub fn explode(&mut self, def: &str, iid: f64) -> Res<Vec<Sel>> {
        let Some(inst) = self.defs.get(def).and_then(|d| d.insts.get(iid)).cloned() else { return Ok(Vec::new()) };
        let src = self.defs.get(&inst.def).cloned();
        let Some(d) = self.defs.get_mut(def) else { return Err(Error::Type) };
        d.insts.delete(iid);
        let mut out: Vec<(String, Option<f64>)> = Vec::new();
        if let Some(src) = src {
            let m = inst.m;
            let mirror = det3(&m) < 0.0;
            let mut vmap: FxMap<u64, f64> = FxMap::default();
            let cv = |d: &mut Def, vmap: &mut FxMap<u64, f64>, v: f64| -> f64 {
                if let Some(&x) = vmap.get(&kb(v)) {
                    return x;
                }
                let id = vertex_at(d, apply(&m, src.v(v)));
                vmap.insert(kb(v), id);
                id
            };
            let mut rc = CurveRemap::default();
            for e in src.edges.values() {
                let a = cv(d, &mut vmap, e.a);
                let b = cv(d, &mut vmap, e.b);
                let c = rc.map(d, e.curve.as_deref());
                let id = e_add(d, a, b, e.soft, c, e.hidden);
                out.push(("e".into(), id));
            }
            for (cid, sp) in src.splines.iter() {
                let nc = rc.map(d, Some(cid)).unwrap_or_default();
                d.splines.set(nc, Spline { pts: sp.pts.iter().map(|p| apply(&m, *p)).collect(), ..sp.clone() });
            }
            for f in src.faces.values() {
                let mut loops: Vec<Vec<f64>> = f.loops.iter().map(|l| l.iter().map(|&v| cv(d, &mut vmap, v)).collect()).collect();
                if mirror {
                    for l in loops.iter_mut() {
                        l.reverse();
                    }
                }
                let id = f_add(d, loops, f.color.clone(), None, f.hidden);
                out.push(("f".into(), Some(id)));
            }
            for child in src.insts.values() {
                let id = nid(d);
                d.insts.set(id, Inst { m: mmul(&m, &child.m), ..child.clone() });
                out.push(("i".into(), Some(id)));
            }
            cleanup(d);
        }
        self.purge();
        let Some(d) = self.defs.get(def) else { return Ok(Vec::new()) };
        Ok(out
            .into_iter()
            .filter_map(|(t, id)| {
                let id = id?;
                let keep = match t.as_str() {
                    "e" => d.edges.has(id),
                    "f" => d.faces.has(id),
                    _ => d.insts.has(id),
                };
                keep.then_some(Sel { t, id })
            })
            .collect())
    }

    /// Give one instance its own copy of its definition. → the new definition's id.
    pub fn make_unique(&mut self, def: &str, iid: f64) -> Res<Option<String>> {
        let Some(inst) = self.defs.get(def).and_then(|d| d.insts.get(iid)).cloned() else { return Ok(None) };
        let Some(src) = self.defs.get(&inst.def) else { return Err(Error::Type) };
        let (kind, sname) = (src.kind.clone(), src.name.clone());
        let j = def_value(src);
        let name = if kind == "component" { Some(self.unique_name(strip_hash_num(&sname))) } else { None };
        let nd_id = self.new_def(&kind, name.as_deref());
        if let Some(nd) = self.defs.get_mut(&nd_id) {
            def_from_json(nd, &j, Fail::Plain)?;
        }
        if let Some(i) = self.defs.get_mut(def).and_then(|d| d.insts.get_mut(iid)) {
            i.def = nd_id.clone();
        }
        Ok(Some(nd_id))
    }

    /// Delete definitions no instance uses any more.
    pub fn purge(&mut self) {
        loop {
            let used = self.instance_counts();
            let used_of = |id: &str| used.get(id).copied().is_some_and(js::truthy);
            let origin: std::collections::HashSet<String> = self
                .defs
                .values()
                .filter(|d| d.dyn_.is_some() && used_of(&d.id))
                .filter_map(|d| d.dyn_.as_ref().map(|y| y.src.clone()))
                .collect();
            let dead: Vec<String> =
                self.defs.keys().filter(|id| id.as_str() != "root" && !used_of(id) && !origin.contains(id.as_str())).cloned().collect();
            if dead.is_empty() {
                break;
            }
            for id in dead {
                self.defs.delete(&id);
            }
        }
    }

    // ---- clipboard ----

    /// The selection `sel` of the open context `path` as a clipboard (plain JSON). None when nothing to copy.
    pub fn copy_clip(&mut self, path: &[f64], sel: &[Sel]) -> Res<Option<Value>> {
        let mut scratch = Model::new();
        let snap = self.snapshot();
        let j: Value = serde_json::from_str(&snap).map_err(|_| Error::Type)?;
        scratch.load(&j, true)?;
        let Some(ctx) = scratch.context(path) else { return Ok(None) };
        let s2: Vec<Sel> = sel.iter().filter(|s| !s.is("g")).cloned().collect();
        let Some(iid) = scratch.make_group(&ctx.def, &s2, "group", None)? else { return Ok(None) };
        let Some(inst) = scratch.defs.get(&ctx.def).and_then(|d| d.insts.get(iid)).cloned() else { return Err(Error::Type) };
        let Some(top) = scratch.defs.get(&inst.def) else { return Err(Error::Type) };
        let mut need: Vec<String> = Vec::new();
        fn visit(sc: &Model, d: &Def, depth: u32, need: &mut Vec<String>) {
            if depth > 32 {
                return;
            }
            for i in d.insts.values() {
                if !need.contains(&i.def)
                    && let Some(sd) = sc.defs.get(&i.def)
                {
                    need.push(i.def.clone());
                    visit(sc, sd, depth + 1, need);
                }
            }
        }
        visit(&scratch, top, 0, &mut need);
        let mut o = serde_json::Map::new();
        o.insert("format".into(), Value::String("modelcraft-clip".into()));
        o.insert("version".into(), jn(1.0));
        o.insert("source".into(), Value::String(self.uid.clone()));
        o.insert("units".into(), Value::String(self.units.clone()));
        o.insert("m".into(), Value::Array(mmul(&ctx.m, &inst.m).iter().map(|&x| jn(x)).collect()));
        o.insert(
            "box".into(),
            match scratch.bbox(&inst.def, &IDENT) {
                Some(b) => {
                    let mut bo = serde_json::Map::new();
                    bo.insert("min".into(), Value::Array(b.min.iter().map(|&x| jn(x)).collect()));
                    bo.insert("max".into(), Value::Array(b.max.iter().map(|&x| jn(x)).collect()));
                    Value::Object(bo)
                }
                None => Value::Null,
            },
        );
        o.insert("top".into(), def_value(top));
        o.insert("defs".into(), Value::Array(need.iter().filter_map(|id| scratch.defs.get(id)).map(def_value).collect()));
        Ok(Some(Value::Object(o)))
    }

    /// Paste clipboard `clip` into the open context `path`, its origin at world matrix `m`. → the pasted selection.
    pub fn paste_clip(&mut self, path: &[f64], clip: &Value, m: &Value) -> Res<Vec<Sel>> {
        let fail = |k: &str| Err(msg(k));
        let ok = json::truthy(Some(clip))
            && json::str(clip.get("format")) == Some("modelcraft-clip")
            && json::num(clip.get("version")) == Some(1.0)
            && json::truthy(clip.get("top"));
        let Some(cdefs) = clip.get("defs").and_then(Value::as_array).filter(|_| ok) else {
            return fail("m3d.model.the_clipboard_does_not_hold_a");
        };
        let Some(mm) =
            m.as_array().filter(|a| a.len() == 16).and_then(|a| a.iter().map(|x| json::is_num(Some(x))).collect::<Option<Vec<f64>>>())
        else {
            return fail("m3d.model.the_place_is_wrong");
        };
        let mut mat = [0.0; 16];
        for (k, v) in mm.iter().enumerate() {
            if let Some(s) = mat.get_mut(k) {
                *s = *v;
            }
        }
        if cdefs.len() > 10000 {
            return fail("m3d.model.the_copy_is_too_big");
        }
        let Some(ctx) = self.context(path) else { return fail("m3d.model.the_open_group_is_gone") };
        let same = json::str(clip.get("source")) == Some(self.uid.as_str());
        let ids: Vec<Option<&str>> = cdefs.iter().map(|d| if json::truthy(Some(d)) { json::str(d.get("id")) } else { None }).collect();
        let mut map: OMap<String, String> = OMap::new();
        for dj in cdefs {
            let id = if json::truthy(Some(dj)) { json::str(dj.get("id")) } else { None };
            let Some(id) = id.filter(|s| crate::file::is_def_id(s)) else { return fail("m3d.model.a_definition_id_is_wrong") };
            if same && json::str(dj.get("kind")) == Some("component") && self.defs.get(id).is_some_and(|d| d.kind == "component") {
                map.set(id.to_string(), id.to_string());
            }
        }
        // a component may not end up inside itself: nothing reused may contain the definitions being edited
        let mut open: Vec<String> = Vec::new();
        {
            let mut d = "root".to_string();
            for &iid in path {
                let Some(nd) = self.defs.get(&d).and_then(|x| x.insts.get(iid)).map(|i| i.def.clone()) else { return Err(Error::Type) };
                d = nd;
                open.push(d.clone());
            }
        }
        let mut reach = std::collections::HashSet::new();
        let reused: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for x in map.values() {
                if !v.contains(x) {
                    v.push(x.clone());
                }
            }
            v
        };
        for id in &reused {
            self.reach_from(id, &mut reach);
        }
        if open.iter().any(|id| reach.contains(id)) {
            return fail("m3d.model.a_component_cannot_be_pasted_inside");
        }
        let names: std::collections::HashSet<String> = self.defs.values().map(|d| d.name.clone()).collect();
        for dj in cdefs {
            let id = json::str(dj.get("id")).unwrap_or("").to_string();
            if map.has(&id) {
                continue;
            }
            let kind = if json::str(dj.get("kind")) == Some("component") { "component" } else { "group" };
            let nm = json::clean(dj.get("name"), 120);
            let use_nm = (!nm.is_empty() && !names.contains(&nm)).then_some(nm);
            let nid_ = self.new_def(kind, use_nm.as_deref());
            map.set(id, nid_);
        }
        let remap = |j: &Value, map: &OMap<String, String>| -> Res<Value> {
            let mut o = j.as_object().cloned().unwrap_or_default();
            let mut insts = Vec::new();
            for i in j.get("insts").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
                let idef = if json::truthy(Some(i)) { json::str(i.get("def")) } else { None };
                let Some(idef) = idef.filter(|d| ids.contains(&Some(*d))) else {
                    return Err(msg("m3d.model.an_instance_points_at_a_missing"));
                };
                let mut io = i.as_object().cloned().unwrap_or_default();
                io.insert("def".into(), map.get(idef).map_or(Value::Null, |s| Value::String(s.clone())));
                insts.push(Value::Object(io));
            }
            o.insert("insts".into(), Value::Array(insts));
            Ok(Value::Object(o))
        };
        for dj in cdefs {
            let id = json::str(dj.get("id")).unwrap_or("");
            let target = map.get(id).cloned().unwrap_or_default();
            if target == id {
                continue;
            }
            let rj = remap(dj, &map)?;
            let Some(d) = self.defs.get_mut(&target) else { return Err(Error::Type) };
            def_from_json(d, &rj, Fail::Plain)?;
        }
        let tmp = self.new_def("group", Some("Paste"));
        let top = clip.get("top").cloned().unwrap_or(Value::Null);
        let rj = remap(&top, &map)?;
        if let Some(d) = self.defs.get_mut(&tmp) {
            def_from_json(d, &rj, Fail::Plain)?;
        }
        let pm = mmul(&ctx.inv, &mat);
        let Some(cd) = self.defs.get_mut(&ctx.def) else { return Err(Error::Type) };
        let iid = place(cd, &tmp, &pm);
        self.ensure_tags();
        self.explode(&ctx.def, iid)
    }

    // ---- lights (0.5 RC.11 (d)) ----

    pub fn add_light(&mut self, o: &Value) -> Res<f64> {
        let Some(l) = crate::file::clean_light(o) else { return Err(msg("m3d.model.bad_light")) };
        let id = self.new_guide_id();
        self.lights.set(id, l);
        Ok(id)
    }
    pub fn set_light(&mut self, id: f64, patch: &Value) -> Res<bool> {
        let Some(l) = self.lights.get(id) else { return Ok(false) };
        let mut o = crate::file::light_value(l);
        if let (Some(om), Some(pm)) = (o.as_object_mut(), patch.as_object()) {
            for (k, v) in pm {
                om.insert(k.clone(), v.clone());
            }
        }
        let Some(n) = crate::file::clean_light(&o) else { return Err(msg("m3d.model.bad_light")) };
        self.lights.set(id, n);
        Ok(true)
    }
    pub fn delete_light(&mut self, id: f64) -> bool {
        self.lights.delete(id).is_some()
    }

    // ---- guides ----

    pub fn add_guide_line(&mut self, p: P3, d: P3) -> f64 {
        let id = self.new_guide_id();
        self.guides.set(id, Guide::Line { p, d: norm(d) });
        id
    }
    pub fn add_guide_point(&mut self, p: P3) -> f64 {
        let id = self.new_guide_id();
        self.guides.set(id, Guide::Point { p });
        id
    }
}

/// `place(def, defId, m)`: a new instance of `def_id` in `def` at matrix m.
pub fn place(def: &mut Def, def_id: &str, m: &M4) -> f64 {
    let iid = nid(def);
    def.insts.set(iid, Inst::new(def_id.to_string(), *m));
    iid
}

/// `name.replace(/#\d+$/, '')`.
fn strip_hash_num(s: &str) -> &str {
    let t = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if t.len() < s.len()
        && let Some(r) = t.strip_suffix('#')
    {
        return r;
    }
    s
}

/// /^[A-Za-z0-9+/]+=*$/
fn is_b64(s: &str) -> bool {
    let body = s.trim_end_matches('=');
    !body.is_empty() && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// The head JSON with `nextId` set to n (`j.nextId = pv.nextId`): nextId is the fourth key, written by to_json_out.
fn set_next_id(head: &str, n: f64) -> String {
    let Some(a) = head.find(",\"nextId\":") else { return head.to_string() };
    let start = a + ",\"nextId\":".len();
    let Some(rest) = head.get(start..) else { return head.to_string() };
    let end = rest.find(',').map_or(head.len(), |e| start + e);
    let mut out = String::with_capacity(head.len());
    out.push_str(head.get(..start).unwrap_or(""));
    let mut o = Out::new(false);
    o.num(n);
    out.push_str(&o.s);
    out.push_str(head.get(end..).unwrap_or(""));
    out
}

/// How many things are hidden, in every definition.
pub fn hidden_count(model: &Model) -> f64 {
    let mut n = 0.0;
    for d in model.defs.values() {
        n += d.edges.values().filter(|o| o.hidden).count() as f64;
        n += d.faces.values().filter(|o| o.hidden).count() as f64;
        n += d.insts.values().filter(|o| o.hidden).count() as f64;
    }
    n
}
/// Show everything again, in every definition. → how many things were hidden.
pub fn unhide_all(model: &mut Model) -> f64 {
    let mut n = 0.0;
    for d in model.defs.values_mut() {
        for o in d.edges.values_mut() {
            if o.hidden {
                o.hidden = false;
                n += 1.0;
            }
        }
        for o in d.faces.values_mut() {
            if o.hidden {
                o.hidden = false;
                n += 1.0;
            }
        }
        for o in d.insts.values_mut() {
            if o.hidden {
                o.hidden = false;
                n += 1.0;
            }
        }
    }
    n
}
