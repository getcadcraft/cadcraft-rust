//! project.js — the CivilCraft project: what the user made (points, surface rules, alignments with design profiles,
//! corridors, pads, feature lines, parcels, a pipe network, GIS layers, a coordinate system) and everything computed from
//! it (surfaces, contours, profiles, the graded surface, volumes, sections, network checks), cached; JSON-snapshot undo
//! (200 steps) with the points, GIS features and TIN held outside the snapshots; the `.civilcraft.json` file (untrusted
//! on the way in: every field validated, control characters stripped, bad items dropped with notes).
//!
//! `state` is kept as the JSON value the JS keeps (a plain object edited by `change`), read into typed values where the
//! engine computes from it — the same reads the JS makes (`Number(x) || 0` and so on).

use crate::alignment::{self, Alignment};
use crate::corridor::{self, Assembly};
use crate::desckeys::{self, DescKey, LineCodes};
use crate::featureline::{self, FeatureItem};
use crate::geo::{P2, P3, clean_xy, clean_xyz};
use crate::gis_state::{self, Features, Gis};
use crate::grading::{self, Item, Pad};
use crate::i18n::t;
use crate::js::{J, Obj, arr, fin, num_to_string, slice16, strip_controls};
use crate::pipes;
use crate::points::{self, Pt};
use crate::profile::{self, Profile, StaZ};
use crate::sections::{self, SectionSet};
use crate::tin::{self, Breakline, Built, Surface, ZPt};
use crate::tr;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_UNDO: usize = 200;

/// `DISPLAY_DEFAULTS`.
pub fn display_defaults() -> Map<String, Value> {
    let v = json!({ "surface": "eg", "triangles": false, "contours": true, "interval": 1, "major": 5, "labels": true, "bands": false, "bandCount": 8,
        "slopes": false, "points": true, "pointLabels": false, "breaklines": true, "outline": true, "pipes": true, "profFeatures": true, "profWithin": 0,
        "gradeOverlay": false });
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// `emptyState()`.
pub fn empty_state() -> Value {
    json!({
        "name": t("project.untitled", &[]), "units": "ft",
        "surface": { "name": t("surface.eg", &[]), "groups": {}, "breaklines": [], "boundary": null, "maxEdge": 0, "tin": null, "descKeys": [], "lineCodes": desckeys::default_line_codes().j() },
        "display": Value::Object(display_defaults()),
        "alignments": [], "pads": [], "network": null, "seq": 1, "crs": null, "features": [], "parcels": [],
        "gis": { "layers": [], "maps": [] },
    })
}

// ---------------------------------------------------------------------------------------------------------------
// JSON.stringify (numbers as JS writes them) for the snapshots

/// `JSON.stringify(v)`.
pub fn js_stringify(v: &Value) -> String {
    let mut out = String::new();
    write_js(v, &mut out);
    out
}
fn write_js(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() { out.push_str(&num_to_string(x)) } else { out.push_str("null") }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_js(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_js(x, out);
            }
            out.push('}');
        }
    }
}

/// One undo / redo step.
#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub json: String,
    pub pts: Option<Vec<Pt>>,
    pub gis: Option<Features>,
    pub tin_set: bool,
    pub tin: Value,
}

/// The graded surface and its items (`graded()`).
#[derive(Debug, Default)]
pub struct GradedResult {
    pub surface: Option<Arc<Surface>>,
    pub notes: Vec<String>,
    pub errors: Vec<String>,
    pub report_items: Vec<(String, Vec<String>)>,
    pub regions: Vec<Option<Vec<P3>>>,
    /// per item: id, kind, name, problems, region, info (JSON spread into the item), corridor lines (typed)
    pub items: Vec<GradedItem>,
}
#[derive(Debug, Clone)]
pub struct GradedItem {
    pub id: Value,
    pub kind: &'static str,
    pub name: String,
    pub problems: Vec<String>,
    pub region: Option<Vec<P3>>,
    pub info: Value,
    pub lines: Option<Vec<(String, Vec<P3>)>>,
}
impl J for GradedItem {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), self.id.clone());
        m.insert("kind".into(), Value::String(self.kind.into()));
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("problems".into(), self.problems.j());
        m.insert("region".into(), self.region.j());
        if let Value::Object(info) = &self.info {
            for (k, v) in info {
                m.insert(k.clone(), v.clone());
            }
        }
        Value::Object(m)
    }
}
impl J for GradedResult {
    fn j(&self) -> Value {
        let items: Vec<Value> = self.report_items.iter().map(|(n, p)| json!({ "name": n, "problems": p })).collect();
        json!({
            "surface": self.surface.as_ref().map(|s| crate::script_ops::surface_view(s)),
            "report": { "notes": self.notes, "errors": self.errors, "items": items },
            "regions": self.regions.j(),
            "items": self.items.j(),
        })
    }
}

/// The project.
pub struct Project {
    pub state: Value,
    pub points: Vec<Pt>,
    pub points_file: String,
    pub points_version: u64,
    pub gis_features: Features,
    pub gis_version: u64,
    pub crs_grids: Vec<(String, Vec<u8>)>,
    pub gis_images: Vec<(String, String)>,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    pub version: u64,
    pub saved: i64,
    cache: HashMap<String, (String, Arc<dyn std::any::Any + Send + Sync>)>,
}

impl Default for Project {
    fn default() -> Self {
        Project::new()
    }
}

fn sget<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}
fn obj_mut<'a>(v: &'a mut Value, k: &str) -> Option<&'a mut Map<String, Value>> {
    v.get_mut(k).and_then(Value::as_object_mut)
}

impl Project {
    pub fn new() -> Project {
        Project {
            state: empty_state(),
            points: Vec::new(),
            points_file: String::new(),
            points_version: 0,
            gis_features: Vec::new(),
            gis_version: 0,
            crs_grids: Vec::new(),
            gis_images: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            version: 0,
            saved: 0,
            cache: HashMap::new(),
        }
    }
    pub fn dirty(&self) -> bool {
        i64::try_from(self.version).unwrap_or(i64::MAX) != self.saved
    }
    pub fn units(&self) -> &str {
        if self.state.get("units").and_then(Value::as_str) == Some("m") { "m" } else { "ft" }
    }

    // ---- edits ----

    /// `snapshot()`: the state without display and without the TIN.
    pub fn snapshot(&self) -> String {
        let mut s = self.state.clone();
        if let Some(m) = s.as_object_mut() {
            m.shift_remove("display");
            if let Some(Value::Object(surf)) = m.get_mut("surface") {
                surf.shift_remove("tin");
            }
        }
        js_stringify(&s)
    }
    fn restore(&mut self, json: &str) {
        let Ok(mut s) = serde_json::from_str::<Value>(json) else { return };
        let tin = sget(&self.state, &["surface", "tin"]).cloned().unwrap_or(Value::Null);
        let display = self.state.get("display").cloned().unwrap_or(Value::Null);
        if let Some(surf) = obj_mut(&mut s, "surface") {
            surf.insert("tin".into(), tin);
        }
        if let Some(m) = s.as_object_mut() {
            m.insert("display".into(), display);
        }
        self.state = s;
    }
    /// `setDisplay(k, v)` (not an undo step).
    pub fn set_display(&mut self, k: &str, v: Value) {
        let same = self.state.get("display").and_then(|d| d.get(k)).is_some_and(|x| js_eq(x, &v));
        if same {
            return;
        }
        if let Some(d) = obj_mut(&mut self.state, "display") {
            d.insert(k.to_string(), v);
        }
        self.touch();
    }
    fn push_undo(&mut self, step: Step) {
        self.undo_stack.push(step);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }
    /// `change(label, fn)`: one undo step when the snapshot changed.
    pub fn change(&mut self, label: &str, f: &mut dyn FnMut(&mut Value)) -> bool {
        let before = self.snapshot();
        f(&mut self.state);
        if self.snapshot() == before {
            return false;
        }
        self.push_undo(Step { label: label.to_string(), json: before, pts: None, gis: None, tin_set: false, tin: Value::Null });
        self.touch();
        true
    }
    /// `editPoints(label, fn)`: one undo step that replaces the points (None from fn = nothing happens).
    pub fn edit_points(&mut self, label: &str, f: &mut dyn FnMut(Vec<Pt>, &mut Value) -> Option<Vec<Pt>>) -> bool {
        let before = self.snapshot();
        let was = self.points.clone();
        let Some(next) = f(was.clone(), &mut self.state) else { return false };
        let keys = self.state_keys();
        let key_of = desckeys::grouper(&keys);
        let groups = points::point_groups(&next, Some(&key_of));
        let defaults: Vec<(String, String)> = groups.iter().map(|(k, _)| (k.clone(), self.default_use(k))).collect();
        if let Some(g) = self.state.get_mut("surface").and_then(|s| s.get_mut("groups")).and_then(Value::as_object_mut) {
            for (k, d) in defaults {
                if !g.contains_key(&k) {
                    g.insert(k, Value::String(d));
                }
            }
        }
        self.points = next;
        self.points_version = self.points_version.saturating_add(1);
        self.push_undo(Step { label: label.to_string(), json: before, pts: Some(was), gis: None, tin_set: false, tin: Value::Null });
        self.touch();
        true
    }
    /// `editGis(label, fn)`.
    pub fn edit_gis(&mut self, label: &str, f: &mut dyn FnMut(Features, &mut Value) -> Option<Features>) -> bool {
        let before = self.snapshot();
        let was = self.gis_features.clone();
        match f(was.clone(), &mut self.state) {
            None => {
                self.restore(&before);
                false
            }
            Some(next) => {
                self.gis_features = next;
                self.gis_version = self.gis_version.saturating_add(1);
                self.push_undo(Step {
                    label: label.to_string(),
                    json: before,
                    pts: None,
                    gis: Some(was),
                    tin_set: false,
                    tin: Value::Null,
                });
                self.touch();
                true
            }
        }
    }
    /// `setTin(label, tin, fn)`.
    pub fn set_tin(&mut self, label: &str, tin: Value, f: Option<&mut dyn FnMut(&mut Value)>) -> bool {
        let before = self.snapshot();
        let was = sget(&self.state, &["surface", "tin"]).cloned().unwrap_or(Value::Null);
        if let Some(f) = f {
            f(&mut self.state);
        }
        if let Some(surf) = obj_mut(&mut self.state, "surface") {
            surf.insert("tin".into(), tin);
        }
        self.push_undo(Step { label: label.to_string(), json: before, pts: None, gis: None, tin_set: true, tin: was });
        self.cache.clear();
        self.touch();
        true
    }
    fn step(&mut self, undo: bool) -> Option<String> {
        let u = if undo { self.undo_stack.pop()? } else { self.redo_stack.pop()? };
        let back = Step {
            label: u.label.clone(),
            json: self.snapshot(),
            pts: u.pts.as_ref().map(|_| self.points.clone()),
            gis: u.gis.as_ref().map(|_| self.gis_features.clone()),
            tin_set: u.tin_set,
            tin: if u.tin_set { sget(&self.state, &["surface", "tin"]).cloned().unwrap_or(Value::Null) } else { Value::Null },
        };
        if undo {
            self.redo_stack.push(back);
        } else {
            self.undo_stack.push(back);
        }
        self.restore(&u.json);
        if u.tin_set {
            if let Some(surf) = obj_mut(&mut self.state, "surface") {
                surf.insert("tin".into(), u.tin.clone());
            }
            self.cache.clear();
        }
        if let Some(p) = u.pts {
            self.points = p;
            self.points_version = self.points_version.saturating_add(1);
        }
        if let Some(g) = u.gis {
            self.gis_features = g;
            self.gis_version = self.gis_version.saturating_add(1);
        }
        self.touch();
        Some(u.label)
    }
    pub fn undo(&mut self) -> Option<String> {
        self.step(true)
    }
    pub fn redo(&mut self) -> Option<String> {
        self.step(false)
    }
    pub fn touch(&mut self) {
        self.version = self.version.saturating_add(1);
    }
    /// `nextId(prefix)`.
    pub fn next_id(&mut self, prefix: &str) -> String {
        let seq = self.state.get("seq").map_or(f64::NAN, crate::js::to_number);
        if let Some(m) = self.state.as_object_mut() {
            m.insert("seq".into(), crate::js::jnum(seq + 1.0));
        }
        format!("{prefix}{}", num_to_string(seq))
    }

    /// `setPoints(points, file)` (not undoable).
    pub fn set_points(&mut self, points: Vec<Pt>, file: &str) {
        self.points = points;
        self.points_file = file.to_string();
        self.points_version = self.points_version.saturating_add(1);
        let keys = self.state_keys();
        let key_of = desckeys::grouper(&keys);
        let present: std::collections::HashSet<String> = self.points.iter().map(&key_of).collect();
        let groups = points::point_groups(&self.points, Some(&key_of));
        let defaults: Vec<(String, String)> = groups.iter().map(|(k, _)| (k.clone(), self.default_use(k))).collect();
        if let Some(g) = self.state.get_mut("surface").and_then(|s| s.get_mut("groups")).and_then(Value::as_object_mut) {
            g.retain(|k, _| present.contains(k));
            for (k, d) in defaults {
                if !g.contains_key(&k) {
                    g.insert(k, Value::String(d));
                }
            }
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.touch();
    }
    /// `groups()` → [(key, count, use)].
    pub fn groups(&self) -> Vec<(String, usize, String)> {
        let keys = self.state_keys();
        let key_of = desckeys::grouper(&keys);
        points::point_groups(&self.points, Some(&key_of))
            .into_iter()
            .map(|(k, n)| {
                let u = sget(&self.state, &["surface", "groups"])
                    .and_then(|g| g.get(&k))
                    .filter(|v| crate::js::truthy(Some(v)))
                    .map_or("points".into(), crate::js::to_js_string);
                (k, n, u)
            })
            .collect()
    }
    /// The description keys in the state.
    pub fn state_keys(&self) -> Vec<DescKey> {
        desckeys::read_desc_keys(sget(&self.state, &["surface", "descKeys"]))
    }
    fn state_line_codes(&self) -> LineCodes {
        match sget(&self.state, &["surface", "lineCodes"]).filter(|v| crate::js::truthy(Some(v))) {
            Some(v) => desckeys::read_line_codes(v),
            None => desckeys::clean_line_codes(None),
        }
    }
    /// `setCoding(keys, lineCodes, label)`.
    pub fn set_coding(&mut self, keys: Option<&Value>, line_codes: Option<&Value>, label: &str) -> bool {
        let keys = desckeys::clean_desc_keys(keys);
        let lc = desckeys::clean_line_codes(line_codes);
        let pts = self.points.clone();
        self.change(label, &mut |s: &mut Value| {
            let key_of = desckeys::grouper(&keys);
            let mut now: Vec<String> = Vec::new();
            for p in &pts {
                let k = key_of(p);
                if !now.contains(&k) {
                    now.push(k);
                }
            }
            if let Some(surf) = obj_mut(s, "surface") {
                surf.insert("descKeys".into(), keys.j());
                surf.insert("lineCodes".into(), lc.j());
                if let Some(Value::Object(g)) = surf.get_mut("groups") {
                    g.retain(|k, _| now.contains(k));
                    for k in &now {
                        if !g.contains_key(k) {
                            let d = keys.iter().find(|x| (if x.group.is_empty() { &x.code } else { &x.group }) == k && !x.use_.is_empty());
                            g.insert(k.clone(), Value::String(d.map_or("points".into(), |d| d.use_.clone())));
                        }
                    }
                }
            }
        })
    }
    /// `defaultUse(group)`.
    pub fn default_use(&self, group: &str) -> String {
        self.state_keys()
            .into_iter()
            .find(|x| (if x.group.is_empty() { &x.code } else { &x.group }) == group && !x.use_.is_empty())
            .map_or("points".into(), |k| k.use_)
    }
    /// `figures(only)`.
    pub fn figures(&self, only: Option<&[String]>) -> Vec<desckeys::Figure> {
        let pts: Vec<Pt> = self.points.iter().filter(|p| p.z.is_some()).cloned().collect();
        desckeys::figures(&pts, &self.state_keys(), &self.state_line_codes(), only)
    }

    // ---- derived ----

    fn memo<T: std::any::Any + Send + Sync>(&mut self, key: &str, deps: String, f: impl FnOnce(&mut Self) -> T) -> Arc<T> {
        if let Some((d, v)) = self.cache.get(key)
            && *d == deps
            && let Ok(v) = Arc::clone(v).downcast::<T>()
        {
            return v;
        }
        let v = Arc::new(f(self));
        self.cache.insert(key.to_string(), (deps, v.clone() as Arc<dyn std::any::Any + Send + Sync>));
        v
    }
    pub fn features(&self) -> Vec<featureline::Feature> {
        arr(self.state.get("features")).iter().map(featureline::read_feature).collect()
    }
    fn surface_deps(&self) -> String {
        let mut s = self.state.get("surface").cloned().unwrap_or(Value::Null);
        let tin = s.as_object_mut().and_then(|m| m.shift_remove("tin")).unwrap_or(Value::Null);
        let tin_part = if crate::js::truthy(Some(&tin)) {
            format!("{}:{}", arr(tin.get("pts")).len(), arr(tin.get("faces")).len())
        } else {
            String::new()
        };
        let eg_f: Vec<Value> =
            arr(self.state.get("features")).iter().filter(|f| f.get("use").and_then(Value::as_str) == Some("eg")).cloned().collect();
        format!("{}|{}|{}|{}", self.points_version, tin_part, js_stringify(&s), js_stringify(&Value::Array(eg_f)))
    }

    /// `egBreaklines()`.
    pub fn eg_breaklines(&self) -> Vec<Breakline> {
        let mut out = Vec::new();
        let bl: Vec<String> = sget(&self.state, &["surface", "groups"])
            .and_then(Value::as_object)
            .map(|g| g.iter().filter(|(_, u)| u.as_str() == Some("breakline")).map(|(k, _)| k.clone()).collect())
            .unwrap_or_default();
        if !bl.is_empty() {
            for f in self.figures(Some(&bl)) {
                let mut pts: Vec<P3> = f.pts.iter().map(|p| P3 { x: p.x, y: p.y, z: p.z.unwrap_or(f64::NAN) }).collect();
                if f.closed
                    && let Some(p0) = pts.first().copied()
                {
                    pts.push(p0);
                }
                out.push(Breakline { name: f.code.clone(), pts, from_group: true });
            }
        }
        for b in arr(sget(&self.state, &["surface", "breaklines"])) {
            out.push(crate::script_ops::read_breakline(b));
        }
        for (b, _) in featureline::feature_breaklines(&self.features(), "eg") {
            out.push(b);
        }
        out
    }
    /// `egPoints()`.
    pub fn eg_points(&self) -> Vec<Pt> {
        let keys = self.state_keys();
        let key_of = desckeys::grouper(&keys);
        let g = sget(&self.state, &["surface", "groups"]);
        self.points
            .iter()
            .filter(|p| {
                p.z.is_some() && {
                    let u = g
                        .and_then(|g| g.get(key_of(p)))
                        .filter(|v| crate::js::truthy(Some(v)))
                        .map_or("points".into(), crate::js::to_js_string);
                    u == "points"
                }
            })
            .cloned()
            .collect()
    }
    fn zpts(p: &[Pt]) -> Vec<ZPt> {
        p.iter().map(|q| ZPt { x: q.x, y: q.y, z: q.z }).collect()
    }
    fn tin_of(v: &Value) -> (Vec<Option<P3>>, Vec<Vec<f64>>) {
        let pts = arr(v.get("pts")).iter().map(|p| if p.is_object() { Some(crate::geo::read_p3(p)) } else { None }).collect();
        let faces = arr(v.get("faces")).iter().map(|f| arr(Some(f)).iter().map(|x| crate::js::numf(Some(x))).collect()).collect();
        (pts, faces)
    }
    fn boundary(&self) -> Option<Vec<P2>> {
        sget(&self.state, &["surface", "boundary"]).filter(|b| b.is_array()).map(|b| crate::geo::read_p2s(Some(b)))
    }
    fn max_edge(&self) -> f64 {
        crate::js::number_or0(sget(&self.state, &["surface", "maxEdge"]))
    }
    fn has_tin(&self) -> bool {
        crate::js::truthy(sget(&self.state, &["surface", "tin"]))
    }
    /// `eg()`.
    pub fn eg(&mut self) -> Arc<Built> {
        let deps = self.surface_deps();
        self.memo("eg", deps, |p| {
            if let Some(tin) = sget(&p.state, &["surface", "tin"]).filter(|v| crate::js::truthy(Some(v))) {
                let (pts, faces) = Project::tin_of(tin);
                return tin::surface_from_faces(&pts, &faces);
            }
            let no_bl = arr(sget(&p.state, &["surface", "breaklines"])).is_empty();
            if p.points.is_empty() && no_bl && !p.features().iter().any(|f| f.use_ == "eg") {
                return Built { surface: None, report: tin::Report::default() };
            }
            let pts = Project::zpts(&p.eg_points());
            let bl = p.eg_breaklines();
            let bnd = p.boundary();
            tin::build_surface(&pts, &bl, bnd.as_deref(), p.max_edge())
        })
    }
    /// `contours()` of the existing ground.
    pub fn contours(&mut self) -> Arc<Vec<tin::Contour>> {
        let eg = self.eg();
        let Some(s) = eg.surface.as_ref() else { return Arc::new(Vec::new()) };
        self.contours_memo(s, "eg")
    }
    /// `contours(graded().surface, 'fg')`. Like 0.5, the memo key holds the grading inputs, not the surface itself.
    pub fn contours_fg(&mut self) -> Arc<Vec<tin::Contour>> {
        let g = self.graded();
        let Some(s) = g.surface.as_ref() else { return Arc::new(Vec::new()) };
        self.contours_memo(s, "fg")
    }
    fn contours_memo(&mut self, s: &Surface, tag: &str) -> Arc<Vec<tin::Contour>> {
        let interval = crate::js::number_or0(sget(&self.state, &["display", "interval"]));
        let pads = if tag == "eg" { String::new() } else { self.grade_deps() };
        let deps =
            format!("{}|{}|{}", self.surface_deps(), pads, num_to_string(crate::js::number(sget(&self.state, &["display", "interval"]))));
        let iv = if interval != 0.0 { interval } else { 1.0 };
        self.memo(&format!("contours:{tag}"), deps, |_| s.contours(iv, 0.0, 2000.0).unwrap_or_default())
    }
    /// `alignment(a)`.
    pub fn alignment(&mut self, a: &Value) -> Arc<Alignment> {
        let id = crate::js::string_of(a.get("id"));
        let deps =
            js_stringify(&Value::Array(vec![a.get("pis").cloned().unwrap_or(Value::Null), a.get("start").cloned().unwrap_or(Value::Null)]));
        let pis: Vec<alignment::Pi> = arr(a.get("pis")).iter().map(alignment::read_pi).collect();
        let start = crate::js::number_or0(a.get("start"));
        self.memo(&format!("al:{id}"), deps, |_| alignment::build_alignment(&pis, start))
    }
    /// `egProfile(a)`.
    pub fn eg_profile(&mut self, a: &Value) -> Arc<Vec<StaZ>> {
        let al = self.alignment(a);
        let eg = self.eg();
        let deps = format!(
            "{}{}",
            self.surface_deps(),
            js_stringify(&Value::Array(vec![a.get("pis").cloned().unwrap_or(Value::Null), a.get("start").cloned().unwrap_or(Value::Null)]))
        );
        let id = crate::js::string_of(a.get("id"));
        self.memo(&format!("eg:{id}"), deps, |_| profile::sample_surface(eg.surface.as_ref(), &al, f64::NAN))
    }
    /// `designProfile(a)`.
    pub fn design_profile(&mut self, a: &Value) -> Arc<Profile> {
        let id = crate::js::string_of(a.get("id"));
        let deps = js_stringify(a.get("pvis").unwrap_or(&Value::Null));
        let pv: Vec<profile::Pvi> = arr(a.get("pvis")).iter().map(profile::read_pvi).collect();
        self.memo(&format!("fg:{id}"), deps, |_| profile::build_profile(&pv))
    }
    fn grade_deps(&self) -> String {
        let pads = self.state.get("pads").cloned().unwrap_or(Value::Null);
        let als: Vec<Value> = arr(self.state.get("alignments"))
            .iter()
            .filter(|a| a.get("corridor").and_then(|c| c.get("on")).is_some_and(|v| crate::js::truthy(Some(v))))
            .cloned()
            .collect();
        let fs: Vec<Value> =
            arr(self.state.get("features")).iter().filter(|f| f.get("use").and_then(Value::as_str) == Some("design")).cloned().collect();
        js_stringify(&Value::Array(vec![pads, Value::Array(als), Value::Array(fs)]))
    }

    /// `graded()`.
    pub fn graded(&mut self) -> Arc<GradedResult> {
        let deps = format!("{}|{}", self.surface_deps(), self.grade_deps());
        if let Some((d, v)) = self.cache.get("graded")
            && *d == deps
            && let Ok(v) = Arc::clone(v).downcast::<GradedResult>()
        {
            return v;
        }
        let r = self.compute_graded();
        let r = Arc::new(r);
        self.cache.insert("graded".into(), (deps, r.clone() as Arc<dyn std::any::Any + Send + Sync>));
        r
    }
    fn compute_graded(&mut self) -> GradedResult {
        let eg = self.eg();
        let units = self.units().to_string();
        // items: corridors, closed design feature lines, pads
        struct Spec {
            id: Value,
            kind: &'static str,
            name: String,
            item: SpecItem,
        }
        enum SpecItem {
            Corridor(Arc<Alignment>, Arc<Profile>, Assembly),
            Pad(Pad),
            Outline(Vec<P3>),
        }
        let mut specs: Vec<Spec> = Vec::new();
        let als: Vec<Value> = arr(self.state.get("alignments")).to_vec();
        for a in &als {
            if !a.get("corridor").and_then(|c| c.get("on")).is_some_and(|v| crate::js::truthy(Some(v))) {
                continue;
            }
            let al = self.alignment(a);
            let prof = self.design_profile(a);
            let asm = corridor::read_assembly(a.get("corridor").unwrap_or(&Value::Null), &units);
            let name = tr!("project.corridor", name = crate::js::string_of(a.get("name")));
            specs.push(Spec {
                id: a.get("id").cloned().unwrap_or(Value::Null),
                kind: "corridor",
                name,
                item: SpecItem::Corridor(al, prof, asm),
            });
        }
        for (fv, f) in arr(self.state.get("features")).iter().zip(self.features()) {
            match featureline::feature_item(&f) {
                Some(FeatureItem::Pad(p)) => specs.push(Spec {
                    id: fv.get("id").cloned().unwrap_or(Value::Null),
                    kind: "feature",
                    name: f.name.clone(),
                    item: SpecItem::Pad(p),
                }),
                Some(FeatureItem::Outline(pts)) => specs.push(Spec {
                    id: fv.get("id").cloned().unwrap_or(Value::Null),
                    kind: "feature",
                    name: f.name.clone(),
                    item: SpecItem::Outline(pts),
                }),
                None => {}
            }
        }
        for p in arr(self.state.get("pads")) {
            let pts = crate::geo::read_pzs(p.get("pts"));
            if pts.len() < 3 {
                continue;
            }
            let opts = p.get("options");
            let o = |k: &str| opts.and_then(|o| o.get(k)).map_or(f64::NAN, |v| crate::js::numf(Some(v)));
            let pad = Pad {
                pts,
                z: crate::js::number(p.get("z")),
                cut: crate::js::number(p.get("cut")),
                fill: crate::js::number(p.get("fill")),
                name: crate::js::string_or_empty(p.get("name")),
                spacing: o("spacing"),
                fan_step: o("fanStep"),
                max_dist: o("maxDist"),
            };
            specs.push(Spec {
                id: p.get("id").cloned().unwrap_or(Value::Null),
                kind: "pad",
                name: crate::js::string_or_empty(p.get("name")),
                item: SpecItem::Pad(pad),
            });
        }
        let Some(eg_s) = eg.surface.as_ref() else { return GradedResult::default() };
        if specs.is_empty() {
            return GradedResult::default();
        }
        let has_tin = self.has_tin();
        let (base_pts, base_bl): (Vec<ZPt>, Vec<Breakline>) = if has_tin {
            (eg_s.to_faces().0.iter().map(|p| ZPt { x: p.x, y: p.y, z: Some(p.z) }).collect(), Vec::new())
        } else {
            (Project::zpts(&self.eg_points()), self.eg_breaklines())
        };
        let bnd = if has_tin { None } else { self.boundary() };
        let me = if has_tin { 0.0 } else { self.max_edge() };
        let build = |pts: &[ZPt], bls: &[Breakline]| tin::build_surface(pts, bls, bnd.as_deref(), me);
        // typed corridor infos are kept from the patch closures
        let lines_out: std::cell::RefCell<Vec<Option<Vec<(String, Vec<P3>)>>>> = std::cell::RefCell::new(Vec::new());
        let items: Vec<(String, Item)> = specs
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let item = match &s.item {
                    SpecItem::Pad(p) => Item::Pad(p.clone()),
                    SpecItem::Outline(pts) => {
                        let name = s.name.clone();
                        let pts = pts.clone();
                        Item::Patch(Box::new(move |_s: &Surface| featureline::outline_patch(&name, &pts)))
                    }
                    SpecItem::Corridor(al, prof, asm) => {
                        let lo = &lines_out;
                        Item::Patch(Box::new(move |surf: &Surface| {
                            let cp = corridor::corridor_patch(surf, Some(al), Some(prof), asm);
                            let lines: Vec<(String, Vec<P3>)> = cp
                                .info
                                .lines
                                .iter()
                                .map(|(n, v)| (n.clone(), v.iter().map(|q| P3 { x: q.x, y: q.y, z: q.z }).collect()))
                                .collect();
                            let mut l = lo.borrow_mut();
                            while l.len() <= k {
                                l.push(None);
                            }
                            if let Some(slot) = l.get_mut(k) {
                                *slot = Some(lines);
                            }
                            cp.patch
                        }))
                    }
                };
                (s.name.clone(), item)
            })
            .collect();
        let g = grading::graded_surface(eg_s, &base_pts, &base_bl, &items, &build);
        drop(items);
        let lines = lines_out.into_inner();
        let mut out = GradedResult {
            surface: g.surface.map(Arc::new),
            notes: g.notes,
            errors: g.errors,
            report_items: g.items_report.clone(),
            regions: g.regions.clone(),
            items: Vec::new(),
        };
        for (i, info) in g.infos.into_iter().enumerate() {
            let Some(s) = specs.get(i) else { continue };
            out.items.push(GradedItem {
                id: s.id.clone(),
                kind: s.kind,
                name: s.name.clone(),
                problems: g.items_report.get(i).map(|x| x.1.clone()).unwrap_or_default(),
                region: g.regions.get(i).cloned().flatten(),
                info,
                lines: lines.get(i).cloned().flatten(),
            });
        }
        out
    }

    /// `sections(a)` → None without sample lines (or a bad alignment).
    pub fn sections(&mut self, a: &Value) -> Option<Arc<SectionSet>> {
        let sampling = a.get("sampling").filter(|s| crate::js::truthy(Some(s)))?;
        let al = self.alignment(a);
        if !al.ok() {
            return None;
        }
        let id = crate::js::string_of(a.get("id"));
        let deps = format!(
            "{}|{}|{}",
            self.surface_deps(),
            self.grade_deps(),
            js_stringify(&Value::Array(vec![
                a.get("pis").cloned().unwrap_or(Value::Null),
                a.get("start").cloned().unwrap_or(Value::Null),
                sampling.clone()
            ]))
        );
        let smp = sections::read_sampling(sampling);
        if let Some((d, v)) = self.cache.get(&format!("sec:{id}"))
            && *d == deps
            && let Ok(v) = Arc::clone(v).downcast::<SectionSet>()
        {
            return Some(v);
        }
        let eg = self.eg();
        let g = self.graded();
        let cor = g.items.iter().find(|x| x.kind == "corridor" && x.id.as_str() == Some(id.as_str()));
        let regions: Vec<Vec<P3>> = g.regions.iter().flatten().cloned().collect();
        let design = g.surface.as_deref().or(eg.surface.as_ref());
        let is_ground = g.surface.is_none();
        let set = sections::build_sections(
            &al,
            eg.surface.as_ref(),
            design,
            is_ground,
            Some(&regions),
            cor.and_then(|c| c.lines.as_deref()),
            Some(&smp),
            f64::NAN,
            f64::NAN,
        );
        let set = Arc::new(set);
        self.cache.insert(format!("sec:{id}"), (deps, set.clone() as Arc<dyn std::any::Any + Send + Sync>));
        Some(set)
    }

    /// `volumes(cell)` → None when nothing is graded.
    pub fn volumes(&mut self, cell: f64) -> Option<Value> {
        let g = self.graded();
        let eg = self.eg();
        let (Some(gs), Some(es)) = (g.surface.as_ref(), eg.surface.as_ref()) else { return None };
        let regions: Vec<Vec<P3>> = g.regions.iter().flatten().cloned().collect();
        if regions.is_empty() {
            return None;
        }
        let total = grading::grid_volume(es, gs, cell, Some(&regions), None, 4e6);
        let mut items = Vec::new();
        for it in &g.items {
            let Some(r) = it.region.as_ref() else { continue };
            let v = grading::grid_volume(es, gs, total.cell, Some(std::slice::from_ref(r)), None, 4e6);
            let mut m = Map::new();
            m.insert("id".into(), it.id.clone());
            m.insert("name".into(), Value::String(it.name.clone()));
            if let Value::Object(vm) = v.j() {
                m.extend(vm);
            }
            items.push(Value::Object(m));
        }
        let mut tj = total.j();
        if let Some(m) = tj.as_object_mut() {
            m.insert("items".into(), Value::Array(items));
        }
        Some(tj)
    }

    /// `pipeSurface()`: the graded surface when there is one, else EG.
    pub fn network_check(&mut self) -> Option<pipes::Analysis> {
        let n = self.state.get("network").filter(|v| crate::js::truthy(Some(v)))?.clone();
        let net = pipes::read_network(&n);
        let g = self.graded();
        let eg = self.eg();
        let s = g.surface.as_deref().or(eg.surface.as_ref());
        Some(pipes::analyze_network(&net, s, self.units()))
    }

    // ---- point edits (point-edit.js editPoint) ----

    /// `editPoint(project, n, patch, { label })` → the check JSON + followed + message.
    pub fn edit_point(&mut self, n: &str, patch: &Map<String, Value>, label: Option<&str>) -> Value {
        let r = crate::point_edit::check_point_edit(&self.points, n, patch);
        if !r.ok {
            return r.j();
        }
        let (Some(old), Some(point), Some(index)) = (r.old.clone(), r.point.clone(), r.index) else { return r.j() };
        let moved = point.x != old.x || point.y != old.y || point.z != old.z;
        let mut followed = 0usize;
        let label = label.map(str::to_string).unwrap_or_else(|| tr!("pt.edit.undo", n = old.n.as_str()));
        let pnt = point.clone();
        self.edit_points(&label, &mut |mut pts: Vec<Pt>, s: &mut Value| {
            if let Some(slot) = pts.get_mut(index) {
                *slot = pnt.clone();
            }
            if moved && let Some(bls) = s.get_mut("surface").and_then(|x| x.get_mut("breaklines")).and_then(Value::as_array_mut) {
                for b in bls {
                    let Some(vs) = b.get_mut("pts").and_then(Value::as_array_mut) else { continue };
                    for v in vs {
                        let (vx, vy) = (v.get("x").and_then(Value::as_f64), v.get("y").and_then(Value::as_f64));
                        if vx != Some(old.x) || vy != Some(old.y) {
                            continue;
                        }
                        if let Some(m) = v.as_object_mut() {
                            m.insert("x".into(), crate::js::jnum(pnt.x));
                            m.insert("y".into(), crate::js::jnum(pnt.y));
                            if let Some(z) = pnt.z {
                                m.insert("z".into(), crate::js::jnum(z));
                            }
                        }
                        followed = followed.saturating_add(1);
                    }
                }
            }
            Some(pts)
        });
        let mut parts = vec![tr!("pt.edit.done", n = point.n.as_str())];
        if point.n != old.n {
            parts.push(tr!("pt.edit.renumbered", from = old.n.as_str(), to = point.n.as_str()));
        }
        if followed > 0 {
            parts.push(tr!("pt.edit.followed", n = followed));
        }
        if moved && point.z.is_none() && followed > 0 {
            parts.push(t("pt.edit.no.z", &[]));
        }
        if self.has_tin() {
            parts.push(t("pt.edit.tin", &[]));
        }
        let mut j = r.j();
        if let Some(m) = j.as_object_mut() {
            m.insert("followed".into(), json!(followed));
            m.insert("message".into(), Value::String(parts.join(" ")));
        }
        j
    }

    // ---- project file ----

    /// `toJSON()` (without `saved`, the clock).
    pub fn to_json(&self) -> Value {
        let pts: Vec<Value> = self
            .points
            .iter()
            .map(|p| {
                let mut row = vec![p.n.j(), p.x.j(), p.y.j(), p.z.j(), p.d.j()];
                if let Some(b) = &p.blk {
                    row.push(b.clone());
                }
                Value::Array(row)
            })
            .collect();
        let g = |k: &str| self.state.get(k).cloned().unwrap_or(Value::Null);
        let grid_name = sget(&self.state, &["crs", "grid"]).and_then(Value::as_str);
        let mut grids = Map::new();
        if let Some(n) = grid_name
            && let Some((_, b)) = self.crs_grids.iter().find(|(k, _)| k == n)
        {
            grids.insert(n.to_string(), Value::String(b64(b)));
        }
        let used: Vec<String> = arr(sget(&self.state, &["gis", "maps"]))
            .iter()
            .filter_map(|m| m.get("image").filter(|v| crate::js::truthy(Some(v))).map(crate::js::to_js_string))
            .collect();
        let mut images = Map::new();
        for (k, v) in &self.gis_images {
            if used.contains(k) {
                images.insert(k.clone(), Value::String(v.clone()));
            }
        }
        json!({
            "format": "civilcraft-project", "version": 1,
            "name": g("name"), "units": g("units"), "pointsFile": self.points_file,
            "points": pts,
            "surface": g("surface"), "display": g("display"),
            "alignments": g("alignments"), "pads": g("pads"), "network": g("network"), "seq": g("seq"),
            "features": if crate::js::truthy(self.state.get("features")) { g("features") } else { json!([]) },
            "parcels": if crate::js::truthy(self.state.get("parcels")) { g("parcels") } else { json!([]) },
            "crs": g("crs"),
            "gis": g("gis"), "gisFeatures": gis_state::gis_features_json(&self.gis_features),
            "crsGrids": Value::Object(grids),
            "gisImages": Value::Object(images),
        })
    }

    /// `Project.fromJSON(obj)` → (project, notes) or the error message the JS throws.
    pub fn from_json(obj: &Value) -> Result<(Project, Vec<String>), String> {
        let mut notes: Vec<String> = Vec::new();
        if obj.get("format").and_then(Value::as_str) != Some("civilcraft-project") {
            return Err(t("project.err.notcc", &[]));
        }
        if obj.get("version").and_then(Value::as_f64) != Some(1.0) {
            let v = slice16(&crate::js::string_of(obj.get("version")), 10);
            return Err(tr!("project.err.newer", version = v));
        }
        let mut p = Project::new();
        let units = if obj.get("units").and_then(Value::as_str) == Some("m") { "m" } else { "ft" };
        let st = |v: Option<&Value>, n: usize| slice16(&strip_controls(&crate::js::string_or_empty(v), " "), n);
        let seq = obj.get("seq").and_then(Value::as_f64).filter(|s| crate::js::is_integer(*s) && *s > 0.0).unwrap_or(1.0);
        {
            let s = p.state.as_object_mut();
            if let Some(s) = s {
                let name = st(obj.get("name"), 120);
                s.insert("name".into(), Value::String(if name.is_empty() { t("project.untitled", &[]) } else { name }));
                s.insert("units".into(), Value::String(units.into()));
                s.insert("seq".into(), crate::js::jnum(seq));
            }
        }
        // points
        let raw_pts = obj.get("points");
        let mut pts: Vec<Pt> = Vec::new();
        for r in arr(raw_pts).iter().take(2_000_000) {
            let Some(ra) = r.as_array() else { continue };
            let (Some(x), Some(y)) = (fin(ra.get(1)), fin(ra.get(2))) else { continue };
            let blk = clean_blk(ra.get(5));
            pts.push(Pt { n: st(ra.first(), 40), x, y, z: fin(ra.get(3)), d: st(ra.get(4), 200), blk });
        }
        if let Some(a) = raw_pts.and_then(Value::as_array)
            && pts.len() != a.len()
        {
            notes.push(tr!("project.bad.points", n = a.len().saturating_sub(pts.len())));
        }
        // surface
        let sf = obj.get("surface").filter(|v| crate::js::truthy(Some(v))).cloned().unwrap_or(json!({}));
        let mut surf = Map::new();
        let sname = st(sf.get("name"), 120);
        surf.insert("name".into(), Value::String(if sname.is_empty() { t("surface.eg", &[]) } else { sname }));
        let mut groups = Map::new();
        if let Some(g) = sf.get("groups").and_then(Value::as_object) {
            for (k, v) in g.iter().take(10000) {
                if let Some(u) = v.as_str().filter(|u| ["off", "points", "breakline"].contains(u)) {
                    groups.insert(slice16(&strip_controls(k, " "), 200), Value::String(u.into()));
                }
            }
        }
        surf.insert("groups".into(), Value::Object(groups));
        let mut bls = Vec::new();
        for b in arr(sf.get("breaklines")).iter().take(100_000) {
            let q: Vec<P3> = arr(b.get("pts")).iter().take(1_000_000).filter_map(clean_xyz).collect();
            if q.len() >= 2 {
                bls.push(json!({ "name": st(b.get("name"), 120), "pts": q.j() }));
            }
        }
        surf.insert("breaklines".into(), Value::Array(bls));
        let bnd: Vec<P2> = arr(sf.get("boundary")).iter().take(1_000_000).filter_map(clean_xy).collect();
        surf.insert("boundary".into(), if bnd.len() >= 3 { bnd.j() } else { Value::Null });
        surf.insert("maxEdge".into(), crate::js::jnum(fin(sf.get("maxEdge")).filter(|m| *m >= 0.0).unwrap_or(0.0)));
        let mut tin_v = Value::Null;
        if let Some(tn) = sf.get("tin")
            && let (Some(tp), Some(tf)) = (tn.get("pts").and_then(Value::as_array), tn.get("faces").and_then(Value::as_array))
        {
            let tp2: Vec<Value> = tp.iter().take(5_000_000).map(|q| clean_xyz(q).map_or(Value::Null, |p| p.j())).collect();
            let n = tp2.len();
            let tf2: Vec<Value> = tf
                .iter()
                .take(10_000_000)
                .filter(|f| {
                    f.as_array().is_some_and(|a| {
                        a.len() == 3 && a.iter().all(|i| i.as_f64().is_some_and(|x| crate::js::is_integer(x) && x >= 0.0 && x < n as f64))
                    })
                })
                .cloned()
                .collect();
            let mut t = Map::new();
            t.insert("pts".into(), Value::Array(tp2));
            t.insert("faces".into(), Value::Array(tf2));
            if tn.get("from").and_then(Value::as_str) == Some("grid") {
                t.insert("from".into(), Value::String("grid".into()));
            }
            tin_v = Value::Object(t);
        }
        surf.insert("tin".into(), tin_v);
        surf.insert("descKeys".into(), desckeys::clean_desc_keys(sf.get("descKeys")).j());
        surf.insert("lineCodes".into(), desckeys::clean_line_codes(sf.get("lineCodes")).j());
        if let Some(s) = p.state.as_object_mut() {
            s.insert("surface".into(), Value::Object(surf));
        }
        // display
        if let Some(d) = obj.get("display").filter(|d| d.is_object() || d.is_array()) {
            let defs = display_defaults();
            if let Some(disp) = obj_mut(&mut p.state, "display") {
                for (k, dv) in &defs {
                    let Some(v) = d.get(k) else { continue };
                    let same_type = matches!(
                        (v, dv),
                        (Value::String(_), Value::String(_)) | (Value::Bool(_), Value::Bool(_)) | (Value::Number(_), Value::Number(_))
                    );
                    if !same_type {
                        continue;
                    }
                    if let Value::Number(n) = v
                        && !n.as_f64().is_some_and(|x| x.is_finite() && x > 0.0)
                    {
                        continue;
                    }
                    if k == "surface" && !(v.as_str() == Some("eg") || v.as_str() == Some("fg")) {
                        continue;
                    }
                    disp.insert(k.clone(), v.clone());
                }
            }
        }
        // ids: kept when they look like ours, else new from seq
        let idf = |v: Option<&Value>, pre: &str, p: &mut Project| -> String {
            let s = crate::js::string_of(v);
            let ok = s.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && {
                let lead = s.chars().take_while(char::is_ascii_lowercase).count();
                let rest = s.get(lead..).unwrap_or("");
                !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
            };
            if ok { s } else { p.next_id(pre) }
        };
        // alignments
        let mut als = Vec::new();
        for a in arr(obj.get("alignments")).iter().take(1000) {
            let pis: Vec<Value> = arr(a.get("pis"))
                .iter()
                .take(10_000)
                .filter_map(|q| {
                    clean_xy(q).map(|xy| json!({ "x": xy.x, "y": xy.y, "r": fin(q.get("r")).filter(|r| *r > 0.0).unwrap_or(0.0) }))
                })
                .collect();
            if pis.len() < 2 {
                continue;
            }
            let pvis: Vec<Value> = arr(a.get("pvis"))
                .iter()
                .take(10_000)
                .filter_map(|v| {
                    let sta = fin(v.get("sta"))?;
                    let z = fin(v.get("z"))?;
                    Some(json!({ "sta": sta, "z": z, "L": fin(v.get("L")).filter(|l| *l > 0.0).unwrap_or(0.0) }))
                })
                .collect();
            let mut cor = Value::Null;
            if let Some(c) = a.get("corridor").filter(|c| c.is_object() || c.is_array()) {
                let d = corridor::default_assembly(units);
                let pos = |k: &str, dv: f64| fin(c.get(k)).filter(|v| *v >= 0.0).unwrap_or(dv);
                let nz = |v: f64, dv: f64| if v != 0.0 { v } else { dv };
                let mut m = Map::new();
                m.insert("on".into(), Value::Bool(c.get("on") == Some(&Value::Bool(true))));
                m.insert("lane".into(), crate::js::jnum(pos("lane", d.lane)));
                m.insert("laneSlope".into(), crate::js::jnum(pos("laneSlope", d.lane_slope)));
                m.insert("shoulder".into(), crate::js::jnum(pos("shoulder", d.shoulder)));
                m.insert("shoulderSlope".into(), crate::js::jnum(pos("shoulderSlope", d.shoulder_slope)));
                m.insert("cut".into(), crate::js::jnum(nz(pos("cut", d.cut), d.cut)));
                m.insert("fill".into(), crate::js::jnum(nz(pos("fill", d.fill), d.fill)));
                m.insert("step".into(), crate::js::jnum(nz(pos("step", d.step), d.step)));
                if let Some(sup) = crate::superelev::clean_super(c.get("super"), units) {
                    m.insert("super".into(), sup.j());
                }
                cor = Value::Object(m);
            }
            let min_r = fin(a.get("minR")).filter(|r| *r > 0.0 && *r < 1e7);
            let id = idf(a.get("id"), "a", &mut p);
            let name = st(a.get("name"), 120);
            let mut m = Map::new();
            m.insert("id".into(), Value::String(id));
            m.insert("name".into(), Value::String(if name.is_empty() { t("scene.al.default", &[]) } else { name }));
            m.insert("start".into(), crate::js::jnum(fin(a.get("start")).unwrap_or(0.0)));
            m.insert("pis".into(), Value::Array(pis));
            m.insert("pvis".into(), Value::Array(pvis));
            m.insert("corridor".into(), cor);
            m.insert("sampling".into(), sections::clean_sampling(a.get("sampling"), units).map_or(Value::Null, |s| s.j()));
            if let Some(r) = min_r {
                m.insert("minR".into(), crate::js::jnum(r));
            }
            als.push(Value::Object(m));
        }
        // pads
        let mut pads = Vec::new();
        for d in arr(obj.get("pads")).iter().take(1000) {
            let q: Vec<P2> = arr(d.get("pts")).iter().take(10_000).filter_map(clean_xy).collect();
            let Some(z) = fin(d.get("z")) else { continue };
            if q.len() < 3 {
                continue;
            }
            let id = idf(d.get("id"), "p", &mut p);
            let name = st(d.get("name"), 120);
            pads.push(json!({
                "id": id, "name": if name.is_empty() { t("scene.pad.default", &[]) } else { name }, "pts": q.j(), "z": z,
                "cut": fin(d.get("cut")).filter(|c| *c > 0.0).unwrap_or(2.0), "fill": fin(d.get("fill")).filter(|c| *c > 0.0).unwrap_or(2.0),
            }));
        }
        // features, parcels
        let mut feats = Vec::new();
        for f in arr(obj.get("features")).iter().take(1000) {
            if let Some(mut c) = featureline::clean_feature(f, units) {
                c.id = Some(idf(f.get("id"), "f", &mut p));
                feats.push(c.j());
            }
        }
        let mut parcels = Vec::new();
        for f in arr(obj.get("parcels")).iter().take(2000) {
            if let Some(mut c) = crate::parcels::clean_parcel(f) {
                c.id = Some(idf(f.get("id"), "r", &mut p));
                parcels.push(c.j());
            }
        }
        // network
        let mut network = Value::Null;
        if let Some(nw) = obj.get("network").filter(|n| n.is_object())
            && let (Some(ss), Some(ps)) = (nw.get("structures").and_then(Value::as_array), nw.get("pipes").and_then(Value::as_array))
        {
            let r = nw.get("rules").cloned().unwrap_or(json!({}));
            let pos = |v: Option<&Value>, d: f64| fin(v).filter(|x| *x >= 0.0).unwrap_or(d);
            let mut structures = Vec::new();
            for x in ss.iter().take(10_000) {
                let Some(xy) = clean_xy(x) else { continue };
                let id = idf(x.get("id"), "s", &mut p);
                let name = st(x.get("name"), 60);
                structures.push(json!({
                    "id": id, "name": if name.is_empty() { t("project.def.structure", &[]) } else { name }, "x": xy.x, "y": xy.y,
                    "rim": fin(x.get("rim")), "sumpDepth": pos(x.get("sumpDepth"), 0.0), "dia": pos(x.get("dia"), 48.0),
                }));
            }
            let ids: Vec<String> = structures.iter().filter_map(|x| x.get("id").and_then(Value::as_str).map(str::to_string)).collect();
            let has = |v: Option<&Value>| v.and_then(Value::as_str).is_some_and(|s| ids.iter().any(|i| i == s));
            let mut pipes_v = Vec::new();
            for x in ps.iter().take(10_000) {
                if !(x.is_object() || x.is_array()) || !has(x.get("from")) || !has(x.get("to")) || x.get("from") == x.get("to") {
                    continue;
                }
                let id = idf(x.get("id"), "q", &mut p);
                let name = st(x.get("name"), 60);
                let dia = pos(x.get("dia"), 12.0);
                pipes_v.push(json!({
                    "id": id, "name": if name.is_empty() { t("project.def.pipe", &[]) } else { name }, "from": x.get("from"), "to": x.get("to"),
                    "dia": if dia != 0.0 { dia } else { 12.0 }, "invUp": fin(x.get("invUp")), "invDn": fin(x.get("invDn")),
                }));
            }
            if pipes_v.len() != ps.len() {
                notes.push(tr!("project.bad.pipes", n = ps.len().saturating_sub(pipes_v.len())));
            }
            let nname = st(nw.get("name"), 60);
            let mut net = Map::new();
            net.insert("name".into(), Value::String(if nname.is_empty() { t("pipes.default", &[]) } else { nname }));
            net.insert(
                "type".into(),
                Value::String(if nw.get("type").and_then(Value::as_str) == Some("sanitary") { "sanitary".into() } else { "storm".into() }),
            );
            net.insert(
                "rules".into(),
                json!({ "minSlope": pos(r.get("minSlope"), 0.0), "minCover": pos(r.get("minCover"), 0.0), "minDrop": pos(r.get("minDrop"), 0.0) }),
            );
            net.insert("structures".into(), Value::Array(structures));
            net.insert("pipes".into(), Value::Array(pipes_v));
            let mut cs = Vec::new();
            for x in arr(nw.get("catchments")).iter().take(10_000) {
                let Some(mut c) = crate::hydro::clean_catchment(x) else { continue };
                c.id = Some(idf(x.get("id"), "k", &mut p));
                if c.to.as_ref().is_some_and(|to| !ids.contains(to)) {
                    c.to = None;
                }
                cs.push(c.j());
            }
            if !cs.is_empty() {
                net.insert("catchments".into(), Value::Array(cs));
            }
            if let Some(h) = nw.get("hydro").filter(|h| h.is_object() || h.is_array()) {
                net.insert("hydro".into(), crate::hydro::clean_hydro(Some(h), units).j());
            }
            network = Value::Object(net);
        }
        let crs = read_crs(obj.get("crs"), &mut notes);
        let (gis, gis_features) = gis_state::read_gis_state(obj.get("gis"), obj.get("gisFeatures"), &mut notes);
        let grids = read_crs_grids(obj.get("crsGrids"), &crs, &mut notes);
        let images = read_gis_images(obj.get("gisImages"), &gis, &mut notes);
        if let Some(s) = p.state.as_object_mut() {
            s.insert("alignments".into(), Value::Array(als));
            s.insert("pads".into(), Value::Array(pads));
            s.insert("features".into(), Value::Array(feats));
            s.insert("parcels".into(), Value::Array(parcels));
            s.insert("network".into(), network);
            s.insert("crs".into(), crs);
            s.insert("gis".into(), gis.j());
        }
        p.gis_features = gis_features;
        p.gis_version = p.gis_version.saturating_add(1);
        p.crs_grids = grids;
        p.gis_images = images;
        p.points = pts;
        p.points_file = st(obj.get("pointsFile"), 200);
        p.points_version = p.points_version.saturating_add(1);
        // seq past every id in use
        let mut all_ids: Vec<String> = Vec::new();
        let state = &p.state;
        for k in ["alignments", "pads"] {
            all_ids.extend(arr(state.get(k)).iter().map(|a| crate::js::string_of(a.get("id"))));
        }
        if let Some(n) = state.get("network").filter(|n| n.is_object()) {
            for k in ["structures", "pipes", "catchments"] {
                all_ids.extend(arr(n.get(k)).iter().map(|a| crate::js::string_of(a.get("id"))));
            }
        }
        all_ids.extend(arr(sget(state, &["gis", "layers"])).iter().map(|a| crate::js::string_of(a.get("id"))));
        for k in ["features", "parcels"] {
            all_ids.extend(arr(state.get(k)).iter().map(|a| crate::js::string_of(a.get("id"))));
        }
        let mut seq = state.get("seq").and_then(Value::as_f64).unwrap_or(1.0);
        for id in all_ids {
            let digits = id.trim_start_matches(|c: char| !c.is_ascii_digit());
            let n = crate::js::str_to_number(digits);
            if n >= seq {
                seq = n + 1.0;
            }
        }
        if let Some(s) = p.state.as_object_mut() {
            s.insert("seq".into(), crate::js::jnum(seq));
        }
        Ok((p, notes))
    }
}

/// `===` between two JSON values as display settings compare them.
fn js_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Object(_) | Value::Array(_), _) | (_, Value::Object(_) | Value::Array(_)) => false,
        _ => a == b,
    }
}

/// `cleanBlk(raw)` (block-points.js): a safe `blk`, or None.
pub fn clean_blk(raw: Option<&Value>) -> Option<Value> {
    let raw = raw.filter(|r| r.is_object() || r.is_array())?;
    let name = raw.get("name")?.as_str()?;
    let s = |v: Option<&Value>, n: usize| slice16(crate::js::trim(&strip_controls(&crate::js::string_or_empty(v), " ")), n);
    Some(
        json!({ "name": slice16(crate::js::trim(&strip_controls(name, " ")), 255), "layer": s(raw.get("layer"), 255), "handle": s(raw.get("handle"), 40) }),
    )
}

fn read_crs(c: Option<&Value>, notes: &mut Vec<String>) -> Value {
    let c = match c {
        None | Some(Value::Null) => return Value::Null,
        Some(c) => c,
    };
    let bad = |notes: &mut Vec<String>| {
        notes.push(t("project.bad.crs", &[]));
        Value::Null
    };
    let proj4 = c.get("proj4").and_then(Value::as_str);
    let unit = c.get("unit").and_then(Value::as_str);
    let ok = (c.is_object() || c.is_array())
        && proj4.is_some_and(|p| !crate::js::trim(p).is_empty() && p.encode_utf16().count() <= 20000)
        && unit.is_some_and(|u| ["m", "ft", "ftUS"].contains(&u));
    if !ok {
        return bad(notes);
    }
    let st = |v: Option<&Value>, n: usize| slice16(&strip_controls(&crate::js::string_or_empty(v), " "), n);
    let code = c.get("code").and_then(Value::as_f64).filter(|x| crate::js::is_integer(*x) && *x > 0.0 && *x < 1e6);
    let name = st(c.get("name"), 200);
    let mut out = Map::new();
    out.insert("code".into(), code.map_or(Value::Null, crate::js::jnum));
    out.insert("name".into(), Value::String(if name.is_empty() { t("project.def.crs", &[]) } else { name }));
    out.insert("proj4".into(), Value::String(strip_controls(proj4.unwrap_or(""), " ")));
    out.insert("unit".into(), Value::String(unit.unwrap_or("m").into()));
    out.insert("local".into(), Value::Null);
    if let Some(g) = c.get("grid").and_then(Value::as_str)
        && !g.is_empty()
        && g.len() <= 80
        && g.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        out.insert("grid".into(), Value::String(g.into()));
    }
    if let Some(l) = c.get("local").filter(|l| !l.is_null()) {
        let pt = |q: Option<&Value>| q.and_then(Value::as_array).is_some_and(|a| a.len() == 2 && a.iter().all(|x| fin(Some(x)).is_some()));
        let scale = fin(l.get("scale"));
        let ok = (l.is_object() || l.is_array())
            && scale.is_some_and(|s| s > 0.5 && s < 2.0)
            && fin(l.get("rotation")).is_some()
            && pt(l.get("from"))
            && pt(l.get("to"));
        if !ok {
            return bad(notes);
        }
        let two = |q: Option<&Value>| Value::Array(arr(q).iter().take(2).cloned().collect());
        out.insert(
            "local".into(),
            json!({ "scale": scale, "rotation": fin(l.get("rotation")), "from": two(l.get("from")), "to": two(l.get("to")) }),
        );
    }
    Value::Object(out)
}

const MAX_GRID: usize = 20 * 1024 * 1024;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// base64 (btoa) of bytes.
pub fn b64(u8s: &[u8]) -> String {
    let mut out = String::with_capacity(u8s.len().saturating_mul(4) / 3 + 4);
    for ch in u8s.chunks(3) {
        let b = [ch.first().copied().unwrap_or(0), ch.get(1).copied().unwrap_or(0), ch.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let idx = |s: u32| B64.get(((n >> s) & 63) as usize).copied().map_or('A', char::from);
        out.push(idx(18));
        out.push(idx(12));
        out.push(if ch.len() > 1 { idx(6) } else { '=' });
        out.push(if ch.len() > 2 { idx(0) } else { '=' });
    }
    out
}
/// atob → bytes (None on a bad character).
pub fn unb64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = B64.iter().position(|&x| x == c)?;
        acc = (acc << 6) | u32::try_from(v).ok()?;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

fn read_crs_grids(obj: Option<&Value>, crs: &Value, notes: &mut Vec<String>) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let Some(grid) = crs.get("grid").and_then(Value::as_str) else { return out };
    let Some(o) = obj.filter(|o| o.is_object()) else { return out };
    let v = o.get(grid);
    let Some(s) = v.and_then(Value::as_str) else {
        notes.push(tr!("grid.err.notinfile", name = grid));
        return out;
    };
    let okchars = !s.is_empty() && {
        let body = s.trim_end_matches('=');
        !body.is_empty() && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    };
    if s.len() > MAX_GRID.div_ceil(3).saturating_mul(4).saturating_add(4) || !okchars {
        notes.push(t("grid.err.badfile", &[]));
        return out;
    }
    if let Some(b) = unb64(s) {
        out.push((grid.to_string(), b));
    }
    out
}

fn read_gis_images(obj: Option<&Value>, gis: &Gis, notes: &mut Vec<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(o) = obj.and_then(Value::as_object) else { return out };
    let used: Vec<String> = gis.maps.iter().filter_map(|m| m.get("image").and_then(Value::as_str).map(str::to_string)).collect();
    let max_img: usize = 34_952_534 + 40; // ceil(25 MiB × 4/3) + 40
    let mut bad = 0usize;
    for (k, v) in o.iter().take(200) {
        let kok = k.starts_with("img") && k.len() > 3 && k.len() <= 12 && k.get(3..).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()));
        if !used.contains(k) || !kok {
            continue;
        }
        let good = v.as_str().is_some_and(|s| {
            s.len() <= max_img && {
                let rest = s.strip_prefix("data:image/png;base64,").or_else(|| s.strip_prefix("data:image/jpeg;base64,"));
                rest.is_some_and(|r| {
                    let body = r.trim_end_matches('=');
                    !body.is_empty() && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
                })
            }
        });
        if good {
            out.push((k.clone(), v.as_str().unwrap_or("").to_string()));
        } else {
            bad = bad.saturating_add(1);
        }
    }
    if bad > 0 {
        notes.push(tr!("project.bad.maps", n = bad));
    }
    out
}

/// The view of a project the harness compares.
pub fn view(p: &Project) -> Value {
    json!({
        "state": p.state.clone(),
        "points": p.points.j(),
        "pointsFile": p.points_file,
        "version": p.version,
        "pointsVersion": p.points_version,
        "gisVersion": p.gis_version,
        "undo": p.undo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
        "redo": p.redo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
        "dirty": p.dirty(),
        "gisFeatures": gis_state::gis_features_json(&p.gis_features),
    })
}

/// A `blk` field kept on a point (for read_pt callers).
pub fn obj_of(o: Obj) -> Value {
    o.done()
}
