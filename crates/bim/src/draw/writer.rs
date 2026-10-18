//! model-write.js `ModelWriter` and the parts of ifc-place.js `IfcPlacer` it stands on: journaled writing (every line
//! made and the first state of every line changed or deleted is kept, `undoJournal` puts them back), GlobalId lookup,
//! storey frames in millimetres, placements, extrusions, shape representations, property sets and quantities,
//! materials, colours, containment, deleting what the tools made. Lengths in mm in, file units out.

use std::collections::{HashMap, HashSet};

use cc_io_ifc::model::{Id, id_key};

use crate::api::{JsErr, R};
use crate::util::{M4, inv4, mul4};
use crate::wapi::{Api, JV, Line, P};

pub const D2R: f64 = std::f64::consts::PI / 180.0;
const I4: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// A building-tool error: a catalog key (`model.err.*`) the page shows, or a message.
pub fn key_err(key: &str) -> JsErr {
    JsErr(key.to_string())
}

/// `round(v, d)` of model-write.js (−0 → 0).
pub fn round_d(v: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    let r = crate::js::round(v * p) / p;
    if r == 0.0 { 0.0 } else { r }
}
/// `round(v)` (6 decimals)
pub fn round6(v: f64) -> f64 {
    round_d(v, 6)
}

/// `inv4(m)` where the JS goes on with the result (a singular matrix throws there).
pub fn inv4j(m: &M4) -> R<M4> {
    inv4(m).ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading '0')".into()))
}

/// A world matrix in file units → mm (translation scaled).
pub fn scale_t(m: &M4, unit_mm: f64) -> M4 {
    let mut o = *m;
    o[12] *= unit_mm;
    o[13] *= unit_mm;
    o[14] *= unit_mm;
    o
}

/// One change's journal: what it made, and the first state of what it changed or deleted.
#[derive(Clone, Debug, Default)]
pub struct Journal {
    pub created: Vec<Id>,
    created_set: HashSet<u64>,
    /// (id, line before) in the order first touched
    pub before: Vec<(Id, Option<Line>)>,
    before_set: HashSet<u64>,
}

impl Journal {
    fn add_created(&mut self, id: Id) {
        self.created.push(id);
        self.created_set.insert(id_key(id));
    }
    fn has(&self, id: Id) -> bool {
        self.before_set.contains(&id_key(id)) || self.created_set.contains(&id_key(id))
    }
    /// model-ui.js `mergeJournals(a, b)`: the second's first states of lines the first already changed are dropped.
    pub fn merge(a: Journal, b: Journal) -> Journal {
        let mut out = a;
        for (id, line) in b.before {
            if !out.before_set.contains(&id_key(id)) && !out.created_set.contains(&id_key(id)) {
                out.before_set.insert(id_key(id));
                out.before.push((id, line));
            }
        }
        for id in b.created {
            out.add_created(id);
        }
        out
    }
}

/// The representation contexts the tools draw in (IfcPlacer `contexts()`).
#[derive(Clone, Debug)]
struct Ctx {
    body: Id,
    foot: Id,
    model: Id,
    subs: HashMap<String, Id>,
    extra: HashMap<String, Id>,
}

/// Is `v` a handle (ifc-util `isH`)?
fn is_h(v: &JV) -> Option<f64> {
    v.href()
}

/// Lines dropTree never deletes (model-write.js KEEP).
fn keep_class(up: &str) -> bool {
    let Some(rest) = up.strip_prefix("IFC") else { return false };
    matches!(
        rest,
        "GEOMETRICREPRESENTATIONCONTEXT"
            | "GEOMETRICREPRESENTATIONSUBCONTEXT"
            | "OWNERHISTORY"
            | "LOCALPLACEMENT"
            | "REPRESENTATIONMAP"
            | "PRESENTATIONLAYERASSIGNMENT"
            | "PRESENTATIONSTYLEASSIGNMENT"
            | "COLOURRGB"
    ) || rest.starts_with("MATERIAL")
        || rest.starts_with("SURFACESTYLE")
}

/// Classes the tools look up by GlobalId.
const GUID_CLASSES: [&str; 17] = [
    "IFCBUILDINGSTOREY",
    "IFCBUILDING",
    "IFCWALL",
    "IFCWALLSTANDARDCASE",
    "IFCDOOR",
    "IFCWINDOW",
    "IFCOPENINGELEMENT",
    "IFCSLAB",
    "IFCCOVERING",
    "IFCROOF",
    "IFCSPACE",
    "IFCWALLTYPE",
    "IFCDOORTYPE",
    "IFCWINDOWTYPE",
    "IFCSLABTYPE",
    "IFCCOVERINGTYPE",
    "IFCROOFTYPE",
];

/// A pset row value: [name, IFC type, value] (null rows are skipped where the JS skips them).
pub type Row = (String, String, P);
/// A quantity row: [name, 'length' | 'area' | 'volume' | 'count', value in mm / mm² / mm³ / n].
pub type QRow = (String, &'static str, f64);

pub fn row(n: &str, ty: &str, v: P) -> Row {
    (n.to_string(), ty.to_string(), v)
}

/// The relations of a model: openings and what fills them (ModelWriter `hostIndex()`).
#[derive(Clone, Debug, Default)]
pub struct HostIndex {
    pub voids: HashMap<u64, Vec<Id>>,
    pub void_host: HashMap<u64, Id>,
    pub fills: HashMap<u64, Vec<Id>>,
    pub filled_by: HashMap<u64, Id>,
}

/// ModelWriter on the editor's model.
pub struct W<'a> {
    pub api: &'a mut Api,
    pub v4: bool,
    pub x3: bool,
    pub unit_mm: f64,
    pub journal: Option<Journal>,
    last: f64,
    oh: Option<JV>,
    ctx: Option<Ctx>,
    guids: Option<HashMap<String, Id>>,
    defs: Option<HashMap<u64, Vec<(Id, Id)>>>,
    styled: Option<HashMap<u64, Vec<Id>>>,
    styles: Option<HashMap<String, Id>>,
    mats: Option<HashMap<String, Id>>,
}

impl<'a> W<'a> {
    pub fn new(api: &'a mut Api) -> R<W<'a>> {
        let schema = api.schema_name();
        let v4 = !schema.to_ascii_uppercase().starts_with("IFC2X3");
        let x3 = schema.to_ascii_uppercase().starts_with("IFC4X3");
        let unit_mm = api.length_unit_mm()?;
        Ok(W {
            api,
            v4,
            x3,
            unit_mm,
            journal: None,
            last: f64::NAN,
            oh: None,
            ctx: None,
            guids: None,
            defs: None,
            styled: None,
            styles: None,
            mats: None,
        })
    }

    // ---- low-level writing ----
    pub fn begin(&mut self) {
        self.journal = Some(Journal::default());
    }
    /// `make(name, ...args)`: a new line, journaled → its id.
    pub fn make(&mut self, name: &str, args: Vec<JV>) -> R<Id> {
        let mut e = self.api.create(name, args)?;
        self.api.write(&mut e)?;
        if !(e.id > 0.0) || e.id == self.last {
            return Err(JsErr(crate::draw::tr::t("place.err.write", &[("what", name)])));
        }
        self.last = e.id;
        self.journal.as_mut().ok_or_else(|| JsErr("TypeError: no journal".into()))?.add_created(e.id);
        Ok(e.id)
    }
    /// `touch(id)`: its first state journaled → the line now.
    pub fn touch(&mut self, id: Id) -> R<Line> {
        let j = self.journal.as_ref().ok_or_else(|| JsErr("TypeError: no journal".into()))?;
        if !j.has(id) {
            let before = self.api.line(id)?;
            if let Some(j) = self.journal.as_mut() {
                j.before_set.insert(id_key(id));
                j.before.push((id, before));
            }
        }
        self.api.must(id)
    }
    pub fn write(&mut self, line: &mut Line) -> R<()> {
        self.api.write(line).map(|_| ())
    }
    pub fn line(&self, id: Id) -> R<Option<Line>> {
        self.api.line(id)
    }
    pub fn must(&self, id: Id) -> R<Line> {
        self.api.must(id)
    }
    pub fn h(id: Id) -> JV {
        JV::H(id)
    }
    pub fn t(&self, name: &str, v: P) -> JV {
        self.api.create_type(name, v)
    }
    /// `L(s)`: '' / null → null, else an IfcLabel of its first 255 UTF-16 units.
    pub fn l(&self, s: &str) -> JV {
        if s.is_empty() { JV::Null } else { self.t("IFCLABEL", P::s(&crate::js::slice16(s, 255))) }
    }
    /// `L(s)` of an optional string.
    pub fn lo(&self, s: Option<&str>) -> JV {
        s.map_or(JV::Null, |s| self.l(s))
    }
    pub fn x(&self, s: &str) -> JV {
        if s.is_empty() { JV::Null } else { self.t("IFCTEXT", P::s(s)) }
    }
    pub fn guid(&mut self) -> JV {
        self.api.new_guid()
    }
    /// mm → file units
    pub fn u(&self, mm: f64) -> f64 {
        mm / self.unit_mm
    }
    pub fn ids(&self, up: &str) -> Vec<Id> {
        self.api.ids(up, false)
    }

    pub fn owner_history(&mut self) -> JV {
        if let Some(o) = &self.oh {
            return o.clone();
        }
        let o = self.ids("IFCOWNERHISTORY").first().map_or(JV::Null, |&id| JV::H(id));
        self.oh = Some(o.clone());
        o
    }

    /// element → the storey it is contained in (IfcPlacer `storeyOf()`).
    pub fn storey_of(&self) -> R<HashMap<u64, Id>> {
        let mut m = HashMap::new();
        for rid in self.ids("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
            let r = self.must(rid)?;
            let Some(st) = r.a("RelatingStructure").href() else {
                for h in r.a("RelatedElements").hrefs() {
                    m.remove(&id_key(h));
                }
                continue;
            };
            for h in r.a("RelatedElements").hrefs() {
                m.insert(id_key(h), st);
            }
        }
        Ok(m)
    }

    // ---- contexts and points ----
    fn contexts(&mut self) -> R<Ctx> {
        if let Some(c) = &self.ctx {
            return Ok(c.clone());
        }
        let mains = self.ids("IFCGEOMETRICREPRESENTATIONCONTEXT");
        let mut model = None;
        for &id in &mains {
            let ct = self.must(id)?.a("ContextType").val();
            if ct.truthy() && ct.string().eq_ignore_ascii_case("model") {
                model = Some(id);
                break;
            }
        }
        let model = model.or_else(|| mains.first().copied()).ok_or_else(|| JsErr(crate::draw::tr::t0("place.err.no_context")))?;
        let mut subs = HashMap::new();
        for id in self.ids("IFCGEOMETRICREPRESENTATIONSUBCONTEXT") {
            let c = self.must(id)?;
            let k = c.a("ContextIdentifier").val();
            let k = if k.truthy() { k.string().to_lowercase() } else { String::new() };
            if !k.is_empty() && !subs.contains_key(&k) {
                subs.insert(k, id);
            }
        }
        let mut c = Ctx { body: 0.0, foot: 0.0, model, subs, extra: HashMap::new() };
        c.body = self.sub(&c, "Body", "MODEL_VIEW")?;
        c.foot = self.sub(&c, "FootPrint", "MODEL_VIEW")?;
        self.ctx = Some(c.clone());
        Ok(c)
    }
    fn sub(&mut self, c: &Ctx, name: &str, view: &str) -> R<Id> {
        if let Some(&id) = c.subs.get(&name.to_lowercase()) {
            return Ok(id);
        }
        // as ifc-place.js writes it (its six values in the first six places)
        let args = vec![self.l(name), self.l("Model"), JV::H(c.model), JV::Null, JV::e(view), JV::Null];
        self.make("IFCGEOMETRICREPRESENTATIONSUBCONTEXT", args)
    }
    /// `ctxOf(key)`: 'body' | 'foot' | 'axis' | another sub-context made on first use.
    pub fn ctx_of(&mut self, key: &str) -> R<Id> {
        let c = self.contexts()?;
        match key {
            "body" => Ok(c.body),
            "foot" => Ok(c.foot),
            _ => {
                if let Some(&id) = c.extra.get(key) {
                    return Ok(id);
                }
                let id = if key == "axis" { self.sub(&c, "Axis", "GRAPH_VIEW")? } else { self.sub(&c, key, "MODEL_VIEW")? };
                if let Some(cc) = self.ctx.as_mut() {
                    cc.extra.insert(key.to_string(), id);
                }
                Ok(id)
            }
        }
    }
    pub fn point3(&mut self, x: f64, y: f64, z: f64) -> R<Id> {
        self.make("IFCCARTESIANPOINT", vec![JV::L(vec![JV::Num(x), JV::Num(y), JV::Num(z)])])
    }
    pub fn point2(&mut self, x: f64, y: f64) -> R<Id> {
        self.make("IFCCARTESIANPOINT", vec![JV::L(vec![JV::Num(x), JV::Num(y)])])
    }
    pub fn dir3(&mut self, x: f64, y: f64, z: f64) -> R<Id> {
        self.make("IFCDIRECTION", vec![JV::L(vec![JV::Num(x), JV::Num(y), JV::Num(z)])])
    }

    // ---- psets ----
    pub fn prop_lines(&mut self, rows: &[Row]) -> R<Vec<JV>> {
        let mut out = Vec::new();
        for (name, ty, v) in rows {
            if matches!(v, P::Null | P::Undef) {
                continue;
            }
            let n = self.t("IFCIDENTIFIER", P::s(&crate::js::slice16(name, 255)));
            let val = self.t(ty, v.clone());
            out.push(JV::H(self.make("IFCPROPERTYSINGLEVALUE", vec![n, JV::Null, val, JV::Null])?));
        }
        Ok(out)
    }
    pub fn pset(&mut self, name: &str, rows: &[Row]) -> R<Id> {
        let g = self.guid();
        let oh = self.owner_history();
        let n = self.l(name);
        let props = self.prop_lines(rows)?;
        self.make("IFCPROPERTYSET", vec![g, oh, n, JV::Null, JV::L(props)])
    }
    pub fn relate(&mut self, obj: Id, pset: Id) -> R<Id> {
        let g = self.guid();
        let oh = self.owner_history();
        self.make("IFCRELDEFINESBYPROPERTIES", vec![g, oh, JV::Null, JV::Null, JV::L(vec![JV::H(obj)]), JV::H(pset)])
    }
    pub fn relate_type(&mut self, id: Id, type_id: Id) -> R<Id> {
        for rid in self.ids("IFCRELDEFINESBYTYPE") {
            let r = self.must(rid)?;
            if r.a("RelatingType").href() == Some(type_id) {
                let mut l = self.touch(rid)?;
                let mut objs = match l.a("RelatedObjects") {
                    JV::L(v) => v.clone(),
                    _ => return Err(JsErr("TypeError: l.RelatedObjects is not iterable".into())),
                };
                objs.push(JV::H(id));
                l.set("RelatedObjects", JV::L(objs));
                self.write(&mut l)?;
                return Ok(rid);
            }
        }
        let g = self.guid();
        let oh = self.owner_history();
        self.make("IFCRELDEFINESBYTYPE", vec![g, oh, JV::Null, JV::Null, JV::L(vec![JV::H(id)]), JV::H(type_id)])
    }
    pub fn contain(&mut self, id: Id, storey: Id) -> R<Id> {
        for rid in self.ids("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
            let r = self.must(rid)?;
            if r.a("RelatingStructure").href() == Some(storey) {
                let mut l = self.touch(rid)?;
                let mut objs = match l.a("RelatedElements") {
                    JV::L(v) => v.clone(),
                    _ => return Err(JsErr("TypeError: l.RelatedElements is not iterable".into())),
                };
                objs.push(JV::H(id));
                l.set("RelatedElements", JV::L(objs));
                self.write(&mut l)?;
                return Ok(rid);
            }
        }
        let g = self.guid();
        let oh = self.owner_history();
        self.make("IFCRELCONTAINEDINSPATIALSTRUCTURE", vec![g, oh, JV::Null, JV::Null, JV::L(vec![JV::H(id)]), JV::H(storey)])
    }

    // ---- ModelWriter: finding things ----
    /// `run(fn)`: a journaled change; an error undoes what it wrote.
    pub fn run<T>(&mut self, f: impl FnOnce(&mut W<'a>) -> R<T>) -> R<(T, Journal)> {
        self.begin();
        match f(self) {
            Ok(r) => {
                let j = self.journal.take().unwrap_or_default();
                Ok((r, j))
            }
            Err(e) => {
                if let Some(j) = self.journal.take() {
                    undo_journal(self.api, &j)?;
                }
                self.guids = None;
                self.defs = None;
                Err(e)
            }
        }
    }
    pub fn by_guid(&mut self, guid: &str) -> R<Option<Id>> {
        if guid.is_empty() {
            return Ok(None);
        }
        if self.guids.is_none() {
            let mut m = HashMap::new();
            for c in GUID_CLASSES {
                for id in self.ids(c) {
                    if let Some(g) = self.guid_of(id)?.filter(|g| !g.is_empty()) {
                        m.insert(g, id);
                    }
                }
            }
            self.guids = Some(m);
        }
        Ok(self.guids.as_ref().and_then(|m| m.get(guid).copied()))
    }
    pub fn guid_of(&self, id: Id) -> R<Option<String>> {
        Ok(self.line(id)?.and_then(|l| l.a("GlobalId").val_str()))
    }
    pub fn note_guid(&mut self, guid: &str, id: Id) {
        if let Some(m) = self.guids.as_mut() {
            m.insert(guid.to_string(), id);
        }
    }
    /// `gid(fixed)`: the caller's GlobalId, else a fresh one.
    pub fn gid(&mut self, fixed: Option<&str>) -> String {
        match fixed.filter(|g| !g.is_empty()) {
            Some(g) => g.to_string(),
            None => self.guid().val_str().unwrap_or_default(),
        }
    }
    pub fn gid_jv(&self, g: &str) -> JV {
        // a GlobalId passed as text (the tools' fixed ids) is written as a bare string
        JV::s(g)
    }

    /// The storey's world matrix in mm (the file's own coordinates).
    pub fn storey_world(&mut self, storey: Id) -> R<M4> {
        let st = self.line(storey)?;
        let m = match st.as_ref().and_then(|s| s.get("ObjectPlacement")).filter(|v| v.truthy()) {
            Some(p) => {
                let pid = p.href().ok_or_else(|| JsErr("TypeError: placement".into()))?;
                self.api.world(pid)?
            }
            None => I4,
        };
        Ok(scale_t(&m, self.unit_mm))
    }
    pub fn rel_world_z(&mut self, lp: Id) -> R<f64> {
        Ok(self.api.world(lp)?[14] * self.unit_mm)
    }
    /// Where `child` sits in `parent`'s axes (mm).
    pub fn rel_matrix(&mut self, parent: Id, child: Id) -> R<M4> {
        let p = scale_t(&self.api.world(parent)?, self.unit_mm);
        let c = scale_t(&self.api.world(child)?, self.unit_mm);
        Ok(mul4(&inv4j(&p)?, &c))
    }

    // ---- placements ----
    pub fn axis3(&mut self, x: f64, y: f64, z: f64, ang_deg: f64) -> R<Id> {
        let a = ang_deg * D2R;
        let loc = self.point3(round6(self.u(x)), round6(self.u(y)), round6(self.u(z)))?;
        let zd = self.dir3(0.0, 0.0, 1.0)?;
        let xd = self.dir3(round_d(crate::draw::m::cos(a), 12), round_d(crate::draw::m::sin(a), 12), 0.0)?;
        self.make("IFCAXIS2PLACEMENT3D", vec![JV::H(loc), JV::H(zd), JV::H(xd)])
    }
    pub fn place_at(&mut self, rel_to: Option<Id>, x: f64, y: f64, z: f64, ang_deg: f64) -> R<Id> {
        let ax = self.axis3(x, y, z, ang_deg)?;
        self.make("IFCLOCALPLACEMENT", vec![rel_to.map_or(JV::Null, JV::H), JV::H(ax)])
    }
    pub fn move_placement(&mut self, lp: Id, x: f64, y: f64, z: f64, ang_deg: f64) -> R<()> {
        let mut line = self.touch(lp)?;
        let old = line.a("RelativePlacement").href();
        let ax = self.axis3(x, y, z, ang_deg)?;
        line.set("RelativePlacement", JV::H(ax));
        self.write(&mut line)?;
        if let Some(o) = old {
            self.drop_tree(o)?;
        }
        Ok(())
    }

    // ---- geometry (mm in) ----
    pub fn point2mm(&mut self, x: f64, y: f64) -> R<Id> {
        self.point2(round6(self.u(x)), round6(self.u(y)))
    }
    pub fn polyline2(&mut self, points: &[[f64; 2]], close: bool) -> R<Id> {
        let mut pts = Vec::with_capacity(points.len().saturating_add(1));
        for p in points {
            pts.push(JV::H(self.point2mm(p[0], p[1])?));
        }
        if close && let Some(first) = pts.first().cloned() {
            pts.push(first);
        }
        self.make("IFCPOLYLINE", vec![JV::L(pts)])
    }
    pub fn profile(&mut self, points: &[[f64; 2]], name: Option<&str>) -> R<Id> {
        let n = self.lo(name);
        let pl = self.polyline2(points, true)?;
        self.make("IFCARBITRARYCLOSEDPROFILEDEF", vec![JV::s("AREA"), n, JV::H(pl)])
    }
    pub fn extrude(&mut self, profile: Id, z0: f64, depth: f64) -> R<Id> {
        let p = self.point3(0.0, 0.0, round6(self.u(z0)))?;
        let pos = self.make("IFCAXIS2PLACEMENT3D", vec![JV::H(p), JV::Null, JV::Null])?;
        let d = self.dir3(0.0, 0.0, 1.0)?;
        self.make("IFCEXTRUDEDAREASOLID", vec![JV::H(profile), JV::H(pos), JV::H(d), JV::Num(round6(self.u(depth)))])
    }
    pub fn profile_voids(&mut self, outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>], name: Option<&str>) -> R<Id> {
        if holes.is_empty() {
            return self.profile(outer, name);
        }
        let n = self.lo(name);
        let o = self.polyline2(outer, true)?;
        let mut hs = Vec::new();
        for h in holes {
            hs.push(JV::H(self.polyline2(h, true)?));
        }
        self.make("IFCARBITRARYPROFILEDEFWITHVOIDS", vec![JV::s("AREA"), n, JV::H(o), JV::L(hs)])
    }
    pub fn axis_frame(&mut self, o: [f64; 3], z: [f64; 3], xd: [f64; 3]) -> R<Id> {
        let r = |v: f64| round_d(v, 12);
        let p = self.point3(round6(self.u(o[0])), round6(self.u(o[1])), round6(self.u(o[2])))?;
        let zd = self.dir3(r(z[0]), r(z[1]), r(z[2]))?;
        let xx = self.dir3(r(xd[0]), r(xd[1]), r(xd[2]))?;
        self.make("IFCAXIS2PLACEMENT3D", vec![JV::H(p), JV::H(zd), JV::H(xx)])
    }
    pub fn extrude_at(&mut self, profile: Id, frame: Id, depth: f64, dir: [f64; 3]) -> R<Id> {
        let d = self.dir3(round_d(dir[0], 12), round_d(dir[1], 12), round_d(dir[2], 12))?;
        self.make("IFCEXTRUDEDAREASOLID", vec![JV::H(profile), JV::H(frame), JV::H(d), JV::Num(round6(self.u(depth)))])
    }
    pub fn boxx(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> R<Id> {
        let p = self.profile(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], None)?;
        self.extrude(p, z0, z1 - z0)
    }
    pub fn rep(&mut self, ctx_key: &str, ident: &str, ty: &str, items: &[Id]) -> R<Id> {
        let c = self.ctx_of(ctx_key)?;
        let i = self.l(ident);
        let t = self.l(ty);
        self.make("IFCSHAPEREPRESENTATION", vec![JV::H(c), i, t, JV::L(items.iter().map(|&x| JV::H(x)).collect())])
    }
    pub fn set_shape(&mut self, pid: Id, reps: &[Id]) -> R<()> {
        let mut line = self.touch(pid)?;
        let old = line.a("Representation").href();
        let pds = self.make("IFCPRODUCTDEFINITIONSHAPE", vec![JV::Null, JV::Null, JV::L(reps.iter().map(|&x| JV::H(x)).collect())])?;
        line.set("Representation", JV::H(pds));
        self.write(&mut line)?;
        if let Some(o) = old {
            self.drop_tree(o)?;
        }
        Ok(())
    }
    /// Delete a line and every line under it only geometry uses (journaled).
    pub fn drop_tree(&mut self, id: Id) -> R<()> {
        let mut seen = HashSet::new();
        self.drop_tree_in(id, &mut seen, 0)
    }
    fn drop_tree_in(&mut self, id: Id, seen: &mut HashSet<u64>, depth: u32) -> R<()> {
        if !seen.insert(id_key(id)) || depth > 4096 {
            return Ok(());
        }
        let line = match self.api.line(id) {
            Ok(Some(l)) => l,
            _ => return Ok(()),
        };
        if keep_class(&line.class.to_ascii_uppercase()) {
            return Ok(());
        }
        let mut kids = Vec::new();
        fn walk(v: &JV, kids: &mut Vec<Id>) {
            match v {
                JV::L(l) => l.iter().for_each(|x| walk(x, kids)),
                other => {
                    if let Some(h) = is_h(other) {
                        kids.push(h);
                    }
                }
            }
        }
        for (_, v) in &line.attrs {
            walk(v, &mut kids);
        }
        self.touch(id)?;
        self.api.delete(id);
        for st in self.styled_of(id)? {
            if seen.insert(id_key(st)) {
                self.touch(st)?;
                self.api.delete(st);
            }
        }
        for k in kids {
            self.drop_tree_in(k, seen, depth.saturating_add(1))?;
        }
        Ok(())
    }
    fn styled_of(&mut self, id: Id) -> R<Vec<Id>> {
        if self.styled.is_none() {
            let mut m: HashMap<u64, Vec<Id>> = HashMap::new();
            for sid in self.ids("IFCSTYLEDITEM") {
                if let Some(it) = self.line(sid)?.and_then(|l| l.a("Item").href()) {
                    m.entry(id_key(it)).or_default().push(sid);
                }
            }
            self.styled = Some(m);
        }
        Ok(self.styled.as_ref().and_then(|m| m.get(&id_key(id)).cloned()).unwrap_or_default())
    }

    // ---- colours ----
    pub fn surface_style(&mut self, name: &str, rgb: [f64; 3], transparency: f64) -> R<Id> {
        if self.styles.is_none() {
            let mut m = HashMap::new();
            for id in self.ids("IFCSURFACESTYLE") {
                let n = self.must(id)?.a("Name").val();
                if n.truthy() {
                    m.entry(n.string()).or_insert(id);
                }
            }
            self.styles = Some(m);
        }
        let mut id = self.styles.as_ref().and_then(|m| m.get(name).copied());
        if let Some(x) = id
            && self.line(x)?.is_none()
        {
            id = None;
        }
        if let Some(x) = id {
            return Ok(x);
        }
        let col = self.make("IFCCOLOURRGB", vec![JV::Null, JV::Num(rgb[0]), JV::Num(rgb[1]), JV::Num(rgb[2])])?;
        let shade = if self.v4 {
            self.make("IFCSURFACESTYLESHADING", vec![JV::H(col), JV::Num(transparency)])?
        } else {
            self.make("IFCSURFACESTYLESHADING", vec![JV::H(col)])?
        };
        let n = self.l(name);
        let id = self.make("IFCSURFACESTYLE", vec![n, JV::s("BOTH"), JV::L(vec![JV::H(shade)])])?;
        if let Some(m) = self.styles.as_mut() {
            m.insert(name.to_string(), id);
        }
        Ok(id)
    }
    pub fn style_item(&mut self, item: Id, style: Id) -> R<Id> {
        let styles = if self.v4 {
            vec![JV::H(style)]
        } else {
            vec![JV::H(self.make("IFCPRESENTATIONSTYLEASSIGNMENT", vec![JV::L(vec![JV::H(style)])])?)]
        };
        let sid = self.make("IFCSTYLEDITEM", vec![JV::H(item), JV::L(styles), JV::Null])?;
        if let Some(m) = self.styled.as_mut() {
            m.entry(id_key(item)).or_default().push(sid);
        }
        Ok(sid)
    }

    // ---- property sets and quantities ----
    /// object id → [(def, rel)] (built once per writer).
    pub fn defs(&mut self, id: Id) -> R<Vec<(Id, Id)>> {
        if self.defs.is_none() {
            let mut m: HashMap<u64, Vec<(Id, Id)>> = HashMap::new();
            for rid in self.ids("IFCRELDEFINESBYPROPERTIES") {
                let r = self.must(rid)?;
                let Some(d) = r.a("RelatingPropertyDefinition").href() else { continue };
                for h in r.a("RelatedObjects").hrefs() {
                    m.entry(id_key(h)).or_default().push((d, rid));
                }
            }
            self.defs = Some(m);
        }
        Ok(self.defs.as_ref().and_then(|m| m.get(&id_key(id)).cloned()).unwrap_or_default())
    }
    pub fn def_named(&mut self, id: Id, name: &str) -> R<Option<(Id, Id, Line)>> {
        for (d, rel) in self.defs(id)? {
            if let Some(l) = self.line(d)?
                && l.a("Name").val_str().as_deref() == Some(name)
            {
                return Ok(Some((d, rel, l)));
            }
        }
        Ok(None)
    }
    /// `readPset(id, name)` → { prop: value } (None = no such set).
    pub fn read_pset(&mut self, id: Id, name: &str) -> R<Option<HashMap<String, P>>> {
        let Some((_, _, l)) = self.def_named(id, name)? else { return Ok(None) };
        let mut out = HashMap::new();
        for h in l.a("HasProperties").hrefs() {
            if let Some(p) = self.line(h)?
                && p.get("Name").is_some_and(JV::truthy)
            {
                out.insert(p.a("Name").val().string(), p.a("NominalValue").val());
            }
        }
        Ok(Some(out))
    }
    /// Set these values in the element's own set `name`, making the set or the property.
    pub fn upsert_pset(&mut self, id: Id, name: &str, rows: &[Row]) -> R<Id> {
        let Some((def, _, line)) = self.def_named(id, name)? else {
            let ps = self.pset(name, rows)?;
            self.relate_def(id, ps)?;
            return Ok(ps);
        };
        let mut have: HashMap<String, Id> = HashMap::new();
        for h in line.a("HasProperties").hrefs() {
            if let Some(p) = self.line(h)?
                && p.get("Name").is_some_and(JV::truthy)
            {
                have.insert(p.a("Name").val().string(), h);
            }
        }
        let mut add = Vec::new();
        for (n, ty, v) in rows {
            if matches!(v, P::Null | P::Undef) {
                continue;
            }
            if let Some(&pid) = have.get(n) {
                let p = self.must(pid)?;
                let cur = p.a("NominalValue");
                let same_type = cur.tname().map(str::to_ascii_uppercase).unwrap_or_default() == *ty;
                if strict_eq(&cur.val(), v) && same_type {
                    continue;
                }
                let mut q = self.touch(pid)?;
                q.set("NominalValue", self.t(ty, v.clone()));
                self.write(&mut q)?;
            } else {
                add.push((n.clone(), ty.clone(), v.clone()));
            }
        }
        if !add.is_empty() {
            let mut ps = self.touch(def)?;
            let mut list = match ps.a("HasProperties") {
                JV::L(v) => v.clone(),
                _ => Vec::new(),
            };
            list.extend(self.prop_lines(&add)?);
            ps.set("HasProperties", JV::L(list));
            self.write(&mut ps)?;
        }
        Ok(def)
    }
    fn qty_conv(&self, kind: &str, v: f64) -> f64 {
        match kind {
            "length" => round6(self.u(v)),
            "area" => round6(v / 1e6),
            "volume" => round6(v / 1e9),
            _ => v,
        }
    }
    fn qty_line(&mut self, r: &QRow) -> R<JV> {
        let cls = match r.1 {
            "length" => "IFCQUANTITYLENGTH",
            "area" => "IFCQUANTITYAREA",
            "volume" => "IFCQUANTITYVOLUME",
            _ => "IFCQUANTITYCOUNT",
        };
        let n = self.t("IFCLABEL", P::s(&r.0));
        let mut args = vec![n, JV::Null, JV::Null, JV::Num(self.qty_conv(r.1, r.2))];
        if self.v4 {
            args.push(JV::Null);
        }
        Ok(JV::H(self.make(cls, args)?))
    }
    /// Quantities in the element's own set `name`.
    pub fn upsert_qto(&mut self, id: Id, name: &str, rows: &[QRow]) -> R<Id> {
        let Some((def, _, line)) = self.def_named(id, name)? else {
            let g = self.guid();
            let oh = self.owner_history();
            let n = self.l(name);
            let b = self.l("BaseQuantities");
            let mut qs = Vec::new();
            for r in rows {
                qs.push(self.qty_line(r)?);
            }
            let q = self.make("IFCELEMENTQUANTITY", vec![g, oh, n, JV::Null, b, JV::L(qs)])?;
            self.relate_def(id, q)?;
            return Ok(q);
        };
        let mut have: HashMap<String, Id> = HashMap::new();
        for h in line.a("Quantities").hrefs() {
            if let Some(p) = self.line(h)?
                && p.get("Name").is_some_and(JV::truthy)
            {
                have.insert(p.a("Name").val().string(), h);
            }
        }
        let mut add = Vec::new();
        for r in rows {
            let Some(&qid) = have.get(&r.0) else {
                add.push(r.clone());
                continue;
            };
            let attr = match r.1 {
                "length" => "LengthValue",
                "area" => "AreaValue",
                "volume" => "VolumeValue",
                _ => "CountValue",
            };
            let mut q = self.touch(qid)?;
            q.set(attr, JV::Num(self.qty_conv(r.1, r.2)));
            self.write(&mut q)?;
        }
        if !add.is_empty() {
            let mut s = self.touch(def)?;
            let mut list = match s.a("Quantities") {
                JV::L(v) => v.clone(),
                _ => Vec::new(),
            };
            for r in &add {
                list.push(self.qty_line(r)?);
            }
            s.set("Quantities", JV::L(list));
            self.write(&mut s)?;
        }
        Ok(def)
    }
    pub fn relate_def(&mut self, id: Id, def: Id) -> R<Id> {
        let rel = self.relate(id, def)?;
        if let Some(m) = self.defs.as_mut() {
            m.entry(id_key(id)).or_default().push((def, rel));
        }
        Ok(rel)
    }

    /// An IfcMaterial by name (made once per file).
    pub fn material(&mut self, name: &str) -> R<Id> {
        if self.mats.is_none() {
            let mut m = HashMap::new();
            for id in self.ids("IFCMATERIAL") {
                let n = self.must(id)?.a("Name").val();
                if n.truthy() {
                    m.entry(n.string()).or_insert(id);
                }
            }
            self.mats = Some(m);
        }
        let mut id = self.mats.as_ref().and_then(|m| m.get(name).copied());
        if let Some(x) = id
            && self.line(x)?.is_none()
        {
            id = None;
        }
        if let Some(x) = id {
            return Ok(x);
        }
        let n = self.l(name);
        let args = if self.v4 { vec![n, JV::Null, JV::Null] } else { vec![n] };
        let id = self.make("IFCMATERIAL", args)?;
        if let Some(m) = self.mats.as_mut() {
            m.insert(name.to_string(), id);
        }
        Ok(id)
    }
    pub fn associate_material(&mut self, id: Id, mat: Id) -> R<Id> {
        let g = self.guid();
        let oh = self.owner_history();
        self.make("IFCRELASSOCIATESMATERIAL", vec![g, oh, JV::Null, JV::Null, JV::L(vec![JV::H(id)]), JV::H(mat)])
    }
    /// (relation, material definition) of the first IfcRelAssociatesMaterial naming `id`.
    pub fn material_rel(&self, id: Id) -> R<Option<(Id, Option<Id>)>> {
        for rid in self.ids("IFCRELASSOCIATESMATERIAL") {
            let r = self.must(rid)?;
            if r.a("RelatedObjects").hrefs().contains(&id) {
                return Ok(Some((rid, r.a("RelatingMaterial").href())));
            }
        }
        Ok(None)
    }
    pub fn host_index(&self) -> R<HostIndex> {
        let mut h = HostIndex::default();
        for rid in self.ids("IFCRELVOIDSELEMENT") {
            let r = self.must(rid)?;
            if let (Some(e), Some(o)) = (r.a("RelatingBuildingElement").href(), r.a("RelatedOpeningElement").href()) {
                h.voids.entry(id_key(e)).or_default().push(o);
                h.void_host.insert(id_key(o), e);
            }
        }
        for rid in self.ids("IFCRELFILLSELEMENT") {
            let r = self.must(rid)?;
            if let (Some(o), Some(f)) = (r.a("RelatingOpeningElement").href(), r.a("RelatedBuildingElement").href()) {
                h.fills.entry(id_key(o)).or_default().push(f);
                h.filled_by.insert(id_key(f), o);
            }
        }
        Ok(h)
    }

    /// Delete elements the tools made: relationships lose them (an emptied one goes), then their own sets, shapes and
    /// placements, and the lines themselves.
    pub fn drop_product(&mut self, list: &[Id]) -> R<()> {
        let mut gone_order = Vec::new();
        let mut gone = HashSet::new();
        for &id in list {
            if gone.insert(id_key(id)) {
                gone_order.push(id);
            }
        }
        let mut own = Vec::new();
        for &id in &gone_order {
            for (d, _) in self.defs(id)? {
                own.push(d);
            }
        }
        for rid in self.api.ids("IFCRELATIONSHIP", true) {
            let mut line = self.must(rid)?;
            let mut dead = false;
            let mut changed = false;
            for (_, v) in &mut line.attrs {
                if let Some(h) = is_h(v) {
                    if gone.contains(&id_key(h)) {
                        dead = true;
                        break;
                    }
                } else if let JV::L(l) = v
                    && l.iter().any(|h| is_h(h).is_some_and(|x| gone.contains(&id_key(x))))
                {
                    let keep: Vec<JV> = l.iter().filter(|h| !is_h(h).is_some_and(|x| gone.contains(&id_key(x)))).cloned().collect();
                    if keep.is_empty() {
                        dead = true;
                        break;
                    }
                    *l = keep;
                    changed = true;
                }
            }
            if dead {
                let l = self.touch(rid)?;
                let m = l.a("RelatingMaterial").href();
                self.api.delete(rid);
                if let Some(m) = m
                    && let Some(ml) = self.line(m)?
                    && ml.class.to_ascii_uppercase().ends_with("USAGE")
                {
                    self.touch(m)?;
                    self.api.delete(m);
                }
            } else if changed {
                self.touch(rid)?;
                self.write(&mut line)?;
            }
        }
        for def in own {
            self.drop_tree(def)?;
        }
        for &id in &gone_order {
            let Some(l) = self.line(id)? else { continue };
            let shape = l.a("Representation").href();
            let lp = l.a("ObjectPlacement").href();
            self.touch(id)?;
            self.api.delete(id);
            if let Some(s) = shape {
                self.drop_tree(s)?;
            }
            if let Some(lp) = lp {
                let pl = self.line(lp)?;
                self.touch(lp)?;
                self.api.delete(lp);
                if let Some(rp) = pl.and_then(|p| p.a("RelativePlacement").href()) {
                    self.drop_tree(rp)?;
                }
            }
        }
        self.defs = None;
        self.guids = None;
        Ok(())
    }
}

/// `a === b` on plain values.
pub fn strict_eq(a: &P, b: &P) -> bool {
    match (a, b) {
        (P::Num(x), P::Num(y)) => x == y,
        (P::Str(x), P::Str(y)) => x == y,
        (P::Bool(x), P::Bool(y)) => x == y,
        (P::Null, P::Null) | (P::Undef, P::Undef) => true,
        _ => false,
    }
}

/// `undoJournal(api, mid, journal)`: delete what a change made, write back what it changed; relationships that name a
/// deleted line made later lose it (pruneRefs).
pub fn undo_journal(api: &mut Api, j: &Journal) -> R<()> {
    let mut gone: HashSet<u64> = HashSet::new();
    for &id in j.created.iter().rev() {
        api.delete(id);
        gone.insert(id_key(id));
    }
    for (_, line) in &j.before {
        let Some(line) = line else { return Err(JsErr("TypeError: Cannot read properties of null (reading 'expressID')".into())) };
        let mut l = line.clone();
        api.retype(&mut l);
        api.write(&mut l)?;
    }
    for (id, _) in &j.before {
        gone.remove(&id_key(*id));
    }
    if !gone.is_empty() {
        prune_refs(api, &gone)?;
    }
    Ok(())
}

/// `pruneRefs`: relationships naming any of `gone` lose those references; an emptied one is deleted.
pub fn prune_refs(api: &mut Api, gone: &HashSet<u64>) -> R<()> {
    for rid in api.ids("IFCRELATIONSHIP", true) {
        let mut line = api.must(rid)?;
        let mut dead = false;
        let mut changed = false;
        for (_, v) in &mut line.attrs {
            if let Some(h) = is_h(v)
                && gone.contains(&id_key(h))
            {
                dead = true;
                break;
            }
            if let JV::L(l) = v
                && l.iter().any(|h| is_h(h).is_some_and(|x| gone.contains(&id_key(x))))
            {
                let keep: Vec<JV> = l.iter().filter(|h| !is_h(h).is_some_and(|x| gone.contains(&id_key(x)))).cloned().collect();
                if keep.is_empty() {
                    dead = true;
                    break;
                }
                *l = keep;
                changed = true;
            }
        }
        if dead {
            api.delete(rid);
        } else if changed {
            api.retype(&mut line);
            api.write(&mut line)?;
        }
    }
    Ok(())
}
