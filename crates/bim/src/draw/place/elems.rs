//! ifc-elements.js, the writing half (0.4 B1): delete any element with its parts and openings (`remove`), what Copy
//! and Cut keep in memory for Paste (`snapshot`) and new copies of a snapshot (`paste`), each one journal so the
//! editor undoes it as one step. Built on IfcPlacer as the JS is (its journal, `relateType`, `contain`, the
//! placement set with the next free item number).
//!
//! Refused: the project, sites, buildings, storeys and spaces (spatial structure), and anything not in the model.
//! Cyclic documents exist: every walk over parts and placements is bounded by a seen-set.

use std::collections::{HashMap, HashSet};

use cc_io_ifc::model::{Id, id_key};

use super::{D2R, PSET_PLACEMENT, Placer};
use crate::api::{JsErr, R};
use crate::draw::m;
use crate::draw::writer::{Journal, undo_journal};
use crate::util::{inv4, mul4};
use crate::wapi::{JV, Line, P};

/// The relationships an element set leans on (`relations()`).
#[derive(Clone, Debug, Default)]
pub struct Rels {
    pub parts: HashMap<u64, Vec<Id>>,
    pub part_of: HashMap<u64, Option<Id>>,
    pub voids: HashMap<u64, Vec<Id>>,
    pub void_host: HashMap<u64, Option<Id>>,
    pub fills: HashMap<u64, Vec<Id>>,
    pub filled_by: HashMap<u64, Option<Id>>,
    pub container: HashMap<u64, Option<Id>>,
    pub psets: HashMap<u64, Vec<Option<Id>>>,
    pub ty: HashMap<u64, Option<Id>>,
    pub material: HashMap<u64, Option<Id>>,
    pub rels: Vec<Id>,
}

/// The CadCraft_Placement values of a placed library item.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementSet {
    pub id: Id,
    pub item_number: String,
    pub quantity: P,
    pub notes: String,
}

/// One element of a snapshot.
#[derive(Clone, Debug)]
pub struct SnapEl {
    pub id: Id,
    pub line: Line,
    pub pl_id: Option<Id>,
    pub rel_to: Option<Id>,
    pub world: Option<[f64; 16]>,
    pub parent_world: Option<[f64; 16]>,
    pub psets: Vec<Id>,
    pub ty: Option<Id>,
    pub material: Option<Id>,
    pub container: Option<Id>,
    pub parts: Vec<Id>,
    pub voids: Vec<Id>,
    pub fills: Option<Id>,
    pub top: bool,
    pub placement_pset: Option<PlacementSet>,
}

/// What Paste needs (`snapshot()`), plus the frame IfcEditor adds.
#[derive(Clone, Debug)]
pub struct Snap {
    pub unit_mm: f64,
    pub elements: Vec<SnapEl>,
    pub top: Vec<Id>,
    pub storey: Option<Id>,
    pub frame: Option<super::Frame>,
}

/// Paste's options: target storey, move (model mm), turn (degrees about Z) about `about` (model mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PasteOpts {
    pub storey: Option<Id>,
    pub dx: f64,
    pub dy: f64,
    pub rot: f64,
    pub about: Option<[f64; 2]>,
    pub keep_numbers: bool,
}

/// What a remove did.
#[derive(Clone, Debug, Default)]
pub struct Removed {
    pub removed: Vec<Id>,
    pub refused: Vec<(Id, &'static str)>,
    pub journal: Option<Journal>,
}

/// What a paste made: the new top-level elements and every new element.
#[derive(Clone, Debug, Default)]
pub struct Pasted {
    pub ids: Vec<Id>,
    pub all: Vec<Id>,
    pub journal: Journal,
}

const SPATIAL: [&str; 12] = [
    "IFCSITE",
    "IFCBUILDING",
    "IFCBUILDINGSTOREY",
    "IFCSPACE",
    "IFCFACILITY",
    "IFCFACILITYPART",
    "IFCBRIDGE",
    "IFCROAD",
    "IFCRAILWAY",
    "IFCMARINEFACILITY",
    "IFCSPATIALZONE",
    "IFCEXTERNALSPATIALELEMENT",
];

/// An insertion-ordered set of ids (the JS `Set`).
#[derive(Clone, Debug, Default)]
struct OSet {
    v: Vec<Id>,
    s: HashSet<u64>,
}
impl OSet {
    fn add(&mut self, id: Id) -> bool {
        if self.s.insert(id_key(id)) {
            self.v.push(id);
            true
        } else {
            false
        }
    }
    fn has(&self, id: Id) -> bool {
        self.s.contains(&id_key(id))
    }
}

/// ifc-place.js / ifc-elements.js axis rounding.
fn rd(v: f64) -> f64 {
    m::round(v * 1e9) / 1e9
}
fn rl(v: f64) -> f64 {
    m::round(v * 1e6) / 1e6
}

impl Placer<'_> {
    /// `whyNot(id)`: '' = it can be deleted or copied.
    pub fn why_not(&self, id: Id) -> &'static str {
        let line = match self.w.line(id) {
            Ok(Some(l)) => l,
            _ => return "not-element",
        };
        if !line.get("GlobalId").is_some_and(JV::truthy) {
            return "not-element";
        }
        let cls = line.class.to_ascii_uppercase();
        if cls == "IFCPROJECT" || SPATIAL.contains(&cls.as_str()) {
            return "spatial";
        }
        if line.get("ObjectPlacement").is_none() {
            return "not-element";
        }
        ""
    }

    /// `relations()`
    pub fn relations(&self) -> R<Rels> {
        let mut r = Rels::default();
        for rid in self.w.api.ids("IFCRELATIONSHIP", true) {
            let l = self.w.must(rid)?;
            r.rels.push(rid);
            match l.up.as_ref() {
                "IFCRELAGGREGATES" | "IFCRELNESTS" => {
                    let ro = l.a("RelatingObject").href();
                    for h in l.a("RelatedObjects").hrefs() {
                        if let Some(o) = ro {
                            r.parts.entry(id_key(o)).or_default().push(h);
                        }
                        r.part_of.insert(id_key(h), ro);
                    }
                }
                "IFCRELVOIDSELEMENT" => {
                    let host = l.a("RelatingBuildingElement").href();
                    let op = l.a("RelatedOpeningElement").href();
                    if let (Some(hh), Some(o)) = (host, op) {
                        r.voids.entry(id_key(hh)).or_default().push(o);
                    }
                    if let Some(o) = op {
                        r.void_host.insert(id_key(o), host);
                    }
                }
                "IFCRELFILLSELEMENT" => {
                    let op = l.a("RelatingOpeningElement").href();
                    let el = l.a("RelatedBuildingElement").href();
                    if let (Some(o), Some(e)) = (op, el) {
                        r.fills.entry(id_key(o)).or_default().push(e);
                    }
                    if let Some(e) = el {
                        r.filled_by.insert(id_key(e), op);
                    }
                }
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    let s = l.a("RelatingStructure").href();
                    for h in l.a("RelatedElements").hrefs() {
                        r.container.insert(id_key(h), s);
                    }
                }
                "IFCRELDEFINESBYPROPERTIES" => {
                    let p = l.a("RelatingPropertyDefinition").href();
                    for h in l.a("RelatedObjects").hrefs() {
                        r.psets.entry(id_key(h)).or_default().push(p);
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    let t = l.a("RelatingType").href();
                    for h in l.a("RelatedObjects").hrefs() {
                        r.ty.insert(id_key(h), t);
                    }
                }
                "IFCRELASSOCIATESMATERIAL" => {
                    let mt = l.a("RelatingMaterial").href();
                    for h in l.a("RelatedObjects").hrefs() {
                        r.material.insert(id_key(h), mt);
                    }
                }
                _ => {}
            }
        }
        Ok(r)
    }

    /// `expand(list, R, { withFilledOpenings })`: the elements plus their parts and openings (any depth).
    fn expand(list: &[Id], r: &Rels, with_filled: bool) -> OSet {
        let mut out = OSet::default();
        let mut todo = Vec::new();
        for &id in list {
            out.add(id);
            todo.push(id);
        }
        while let Some(x) = todo.pop() {
            let parts = r.parts.get(&id_key(x)).map(Vec::as_slice).unwrap_or(&[]);
            let voids = r.voids.get(&id_key(x)).map(Vec::as_slice).unwrap_or(&[]);
            for &y in parts.iter().chain(voids) {
                if y != 0.0 && out.add(y) {
                    todo.push(y);
                }
            }
        }
        if with_filled {
            for x in out.v.clone() {
                let Some(Some(op)) = r.filled_by.get(&id_key(x)).copied() else { continue };
                if op != 0.0 && !out.has(op) && r.fills.get(&id_key(op)).map(Vec::as_slice).unwrap_or(&[]).iter().all(|f| out.has(*f)) {
                    out.add(op);
                }
            }
        }
        out
    }

    /// `remove(list)`: delete elements with their parts and openings; one journal.
    pub fn remove(&mut self, list: &[Id]) -> R<Removed> {
        let mut refused = Vec::new();
        let mut ok = Vec::new();
        let mut seen = HashSet::new();
        for &id in list {
            if !seen.insert(id_key(id)) {
                continue;
            }
            let why = self.why_not(id);
            if why.is_empty() {
                ok.push(id);
            } else {
                refused.push((id, why));
            }
        }
        if ok.is_empty() {
            return Ok(Removed { removed: Vec::new(), refused, journal: None });
        }
        let r = self.relations()?;
        let targets = Self::expand(&ok, &r, true);
        self.w.begin();
        let res = (|| -> R<()> {
            for &rid in &r.rels {
                let mut line = self.w.must(rid)?;
                let (mut dead, mut changed) = (false, false);
                for (_, v) in &mut line.attrs {
                    if let Some(h) = v.href() {
                        if targets.has(h) {
                            dead = true;
                        }
                    } else if let JV::L(l) = v
                        && l.iter().any(|h| h.href().is_some_and(|x| targets.has(x)))
                    {
                        let keep: Vec<JV> = l.iter().filter(|h| !h.href().is_some_and(|x| targets.has(x))).cloned().collect();
                        if keep.is_empty() {
                            dead = true;
                        } else {
                            *l = keep;
                            changed = true;
                        }
                    }
                    if dead {
                        break;
                    }
                }
                if dead {
                    self.w.touch(rid)?;
                    self.w.api.delete(rid);
                } else if changed {
                    self.w.touch(rid)?;
                    self.w.api.retype(&mut line);
                    self.w.write(&mut line)?;
                }
            }
            for &id in &targets.v {
                self.w.touch(id)?;
                self.w.api.delete(id);
            }
            Ok(())
        })();
        match res {
            Ok(()) => Ok(Removed { removed: targets.v, refused, journal: self.w.journal.take() }),
            Err(e) => {
                if let Some(j) = self.w.journal.take() {
                    undo_journal(self.w.api, &j)?;
                }
                Err(e)
            }
        }
    }

    /// `placementPsetOf(id, R)`
    fn placement_pset_of(&self, id: Id, r: &Rels) -> R<Option<PlacementSet>> {
        for ps in r.psets.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]).iter().flatten() {
            let l = self.w.must(*ps)?;
            if l.a("Name").val().as_str().as_deref() != Some(PSET_PLACEMENT) {
                continue;
            }
            let mut props: Vec<(String, P)> = Vec::new();
            for h in l.a("HasProperties").hrefs() {
                if let Some(p) = self.w.line(h)?
                    && p.get("Name").is_some_and(JV::truthy)
                {
                    let k = p.a("Name").val().string();
                    let v = p.a("NominalValue").val();
                    if let Some(e) = props.iter_mut().find(|(n, _)| *n == k) {
                        e.1 = v;
                    } else {
                        props.push((k, v));
                    }
                }
            }
            let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            let item_number = match get("ItemNumber") {
                Some(v) if !matches!(v, P::Undef | P::Null) => v.string(),
                _ => String::new(),
            };
            let quantity = match get("Quantity") {
                Some(v) if !matches!(v, P::Undef | P::Null) => v,
                _ => P::Num(1.0),
            };
            let notes = match get("Notes") {
                Some(v) if v.truthy() => v.string(),
                _ => String::new(),
            };
            return Ok(Some(PlacementSet { id: *ps, item_number, quantity, notes }));
        }
        Ok(None)
    }

    /// `snapshot(list)`: each element's line, placement and parent, and relations, read now.
    pub fn snapshot(&mut self, list: &[Id]) -> R<Snap> {
        let mut ok = Vec::new();
        let mut seen = HashSet::new();
        for &id in list {
            if seen.insert(id_key(id)) && self.why_not(id).is_empty() {
                ok.push(id);
            }
        }
        let okset: HashSet<u64> = ok.iter().map(|x| id_key(*x)).collect();
        let r = self.relations()?;
        let all = Self::expand(&ok, &r, false);
        let mut els = Vec::new();
        for &id in &all.v {
            let line = self.w.must(id)?;
            let pl_id = line.a("ObjectPlacement").href().filter(|v| *v != 0.0);
            let mut rel_to = None;
            if let Some(p) = pl_id {
                let pl = self.w.must(p)?;
                rel_to = pl.a("PlacementRelTo").href().filter(|v| *v != 0.0);
            }
            let world = match pl_id {
                Some(p) => Some(self.w.api.world(p)?),
                None => None,
            };
            let parent_world = match rel_to {
                Some(p) => Some(self.w.api.world(p)?),
                None => None,
            };
            let k = id_key(id);
            let in_all = |x: &Option<Id>| x.is_some_and(|v| all.has(v));
            els.push(SnapEl {
                id,
                pl_id,
                rel_to,
                world,
                parent_world,
                psets: r.psets.get(&k).map(|v| v.iter().flatten().copied().filter(|x| *x != 0.0).collect()).unwrap_or_default(),
                ty: r.ty.get(&k).copied().flatten().filter(|x| *x != 0.0),
                material: r.material.get(&k).copied().flatten().filter(|x| *x != 0.0),
                container: r.container.get(&k).copied().flatten().filter(|x| *x != 0.0),
                parts: r.parts.get(&k).map(|v| v.iter().copied().filter(|x| all.has(*x)).collect()).unwrap_or_default(),
                voids: r.voids.get(&k).map(|v| v.iter().copied().filter(|x| all.has(*x)).collect()).unwrap_or_default(),
                fills: r.filled_by.get(&k).copied().flatten().filter(|x| *x != 0.0 && all.has(*x)),
                top: okset.contains(&k)
                    && !(r.part_of.contains_key(&k) && in_all(r.part_of.get(&k).unwrap_or(&None)))
                    && !(r.void_host.contains_key(&k) && in_all(r.void_host.get(&k).unwrap_or(&None))),
                placement_pset: self.placement_pset_of(id, &r)?,
                line,
            });
        }
        // parents before children, so a part placed relative to a copied element finds its new placement
        let by_pl: HashMap<u64, usize> = els.iter().enumerate().filter_map(|(i, e)| e.pl_id.map(|p| (id_key(p), i))).collect();
        let depth = |start: usize| -> usize {
            let mut seen = HashSet::new();
            let mut d = 0usize;
            let mut cur = start;
            while let Some(p) = els.get(cur).and_then(|e| e.rel_to).and_then(|r| by_pl.get(&id_key(r)).copied()) {
                if !seen.insert(p) {
                    break;
                }
                d = d.saturating_add(1);
                cur = p;
            }
            d
        };
        let depths: Vec<usize> = (0..els.len()).map(depth).collect();
        let mut order: Vec<usize> = (0..els.len()).collect();
        order.sort_by_key(|&i| depths.get(i).copied().unwrap_or(0));
        let mut sorted = Vec::with_capacity(els.len());
        for i in order {
            if let Some(e) = els.get(i) {
                sorted.push(e.clone());
            }
        }
        let els = sorted;
        let storey_of = |this: &Self, id: Id| -> R<Option<Id>> {
            let mut c = r.container.get(&id_key(id)).copied().flatten();
            let mut guard = 0u32;
            while let Some(cid) = c.filter(|x| *x != 0.0) {
                if guard >= 20 {
                    break;
                }
                guard = guard.saturating_add(1);
                let l =
                    this.w.line(cid)?.ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading 'constructor')".into()))?;
                if l.class.to_ascii_lowercase().contains("buildingstorey") {
                    return Ok(Some(cid));
                }
                c = r.part_of.get(&id_key(cid)).copied().flatten();
            }
            Ok(None)
        };
        let mut storey = None;
        for e in els.iter().filter(|e| e.top) {
            let mut s = storey_of(self, e.id)?;
            if s.is_none()
                && let Some(Some(p)) = r.part_of.get(&id_key(e.id))
            {
                s = storey_of(self, *p)?;
            }
            if s.is_some() {
                storey = s;
                break;
            }
        }
        let top = els.iter().filter(|e| e.top).map(|e| e.id).collect();
        Ok(Snap { unit_mm: self.w.unit_mm, elements: els, top, storey, frame: None })
    }

    /// `axis(m)`: IfcAxis2Placement3D of a column-major matrix (location, Z axis, X axis).
    fn axis_of(&mut self, mm: &[f64; 16]) -> R<Id> {
        let n = |x: f64, y: f64, z: f64| {
            let l = m::hypot3(x, y, z);
            let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
            [rd(x / l), rd(y / l), rd(z / l)]
        };
        let loc = self.w.point3(rl(mm[12]), rl(mm[13]), rl(mm[14]))?;
        let z = n(mm[8], mm[9], mm[10]);
        let zd = self.w.dir3(z[0], z[1], z[2])?;
        let x = n(mm[0], mm[1], mm[2]);
        let xd = self.w.dir3(x[0], x[1], x[2])?;
        self.w.make("IFCAXIS2PLACEMENT3D", vec![JV::H(loc), JV::H(zd), JV::H(xd)])
    }

    /// `relatePset(id, ps)`: added to a relation that already holds the set, else a new relation.
    fn relate_pset(&mut self, id: Id, ps: Id) -> R<Id> {
        for rid in self.w.ids("IFCRELDEFINESBYPROPERTIES") {
            let r = self.w.must(rid)?;
            if r.a("RelatingPropertyDefinition").href() == Some(ps) {
                self.append_to(rid, id, "RelatedObjects")?;
                return Ok(rid);
            }
        }
        self.w.relate(id, ps)
    }
    /// `relateMaterial(id, mat)`
    fn relate_material(&mut self, id: Id, mat: Id) -> R<Id> {
        for rid in self.w.ids("IFCRELASSOCIATESMATERIAL") {
            let r = self.w.must(rid)?;
            if r.a("RelatingMaterial").href() == Some(mat) {
                self.append_to(rid, id, "RelatedObjects")?;
                return Ok(rid);
            }
        }
        let g = self.w.guid();
        let oh = self.w.owner_history();
        self.w.make("IFCRELASSOCIATESMATERIAL", vec![g, oh, JV::Null, JV::Null, JV::L(vec![JV::H(id)]), JV::H(mat)])
    }
    fn append_to(&mut self, rid: Id, id: Id, list: &str) -> R<()> {
        let mut l = self.w.touch(rid)?;
        let mut objs = match l.a(list) {
            JV::L(v) => v.clone(),
            _ => return Err(JsErr(format!("TypeError: l.{list} is not iterable"))),
        };
        objs.push(JV::H(id));
        l.set(list, JV::L(objs));
        self.w.write(&mut l)
    }

    /// `paste(snap, opts)`: new copies (new GlobalIds, the same shape, sets, type and material; a new placement).
    pub fn paste(&mut self, snap: &Snap, o: &PasteOpts) -> R<Pasted> {
        let um = self.w.unit_mm;
        let u = |mm: f64| mm / um;
        let src_storey = snap.storey;
        let dst_storey = o.storey.filter(|s| *s != 0.0).or(src_storey);
        // `dstStorey && dstStorey !== srcStorey` (a snapshot with no storey moves to the target's)
        let changes_storey = dst_storey.is_some() && dst_storey != src_storey;
        let elev = |this: &mut Self, st: Id| -> R<f64> {
            let l = this.w.must(st)?;
            let p =
                l.a("ObjectPlacement").href().ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading 'value')".into()))?;
            Ok(this.w.api.world(p)?[14])
        };
        let dz = match (dst_storey, src_storey) {
            (Some(d), Some(s)) if d != s => elev(self, d)? - elev(self, s)?,
            _ => 0.0,
        };
        let about = match o.about {
            Some(a) => [u(a[0]), u(a[1])],
            None => snap.elements.iter().find(|e| e.top).and_then(|e| e.world).map_or([0.0, 0.0], |w| [w[12], w[13]]),
        };
        let r = o.rot * D2R;
        let (c, s) = (m::cos(r), m::sin(r));
        let (ax, ay) = (about[0], about[1]);
        let (tx, ty) = (ax + u(o.dx), ay + u(o.dy));
        let mm: [f64; 16] = [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, tx - (c * ax - s * ay), ty - (s * ax + c * ay), dz, 1.0];
        let dst_pl = match dst_storey {
            Some(d) if changes_storey => self.w.must(d)?.a("ObjectPlacement").href().filter(|v| *v != 0.0),
            _ => None,
        };
        let dst_pl_world = match dst_pl {
            Some(p) => Some(self.w.api.world(p)?),
            None => None,
        };
        self.w.begin();
        let res = (|| -> R<(Vec<Id>, Vec<Id>)> {
            let mut new_pl: HashMap<u64, Id> = HashMap::new();
            let mut new_id: HashMap<u64, Id> = HashMap::new();
            let mut all = Vec::new();
            for e in &snap.elements {
                let mut pl = None;
                if let Some(pid) = e.pl_id {
                    let old = self.w.must(pid)?;
                    let made = if let Some(np) = e.rel_to.and_then(|r| new_pl.get(&id_key(r)).copied()) {
                        self.w.make("IFCLOCALPLACEMENT", vec![JV::H(np), old.a("RelativePlacement").clone()])?
                    } else {
                        let ew = e.world.ok_or_else(|| JsErr("TypeError: e.world is null".into()))?;
                        let world = mul4(&mm, &ew);
                        let (parent, pw) = if dst_pl.is_some() && e.top { (dst_pl, dst_pl_world) } else { (e.rel_to, e.parent_world) };
                        let local = match pw {
                            Some(p) => {
                                let ip = inv4(&p).ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading '0')".into()))?;
                                mul4(&ip, &world)
                            }
                            None => world,
                        };
                        let ax = self.axis_of(&local)?;
                        self.w.make("IFCLOCALPLACEMENT", vec![parent.map_or(JV::Null, JV::H), JV::H(ax)])?
                    };
                    new_pl.insert(id_key(pid), made);
                    pl = Some(made);
                }
                let mut line = e.line.clone();
                self.w.api.retype(&mut line);
                line.id = -1.0;
                let g = self.w.guid();
                line.set("GlobalId", g);
                if let Some(p) = pl {
                    line.set("ObjectPlacement", JV::H(p));
                }
                self.w.write(&mut line)?;
                if !(line.id > 0.0) {
                    return Err(JsErr(format!("Could not write the copy of #{}.", crate::js::num_str(e.id))));
                }
                self.w.adopt(line.id)?;
                new_id.insert(id_key(e.id), line.id);
                all.push(line.id);
            }
            let mut next: Option<f64> = None;
            let nid = |m: &HashMap<u64, Id>, x: Id| m.get(&id_key(x)).copied().map_or(JV::Null, JV::H);
            for e in &snap.elements {
                let Some(id) = new_id.get(&id_key(e.id)).copied() else { continue };
                if let Some(t) = e.ty {
                    self.w.relate_type(id, t)?;
                }
                if let Some(mt) = e.material {
                    self.relate_material(id, mt)?;
                }
                for &ps in &e.psets {
                    match &e.placement_pset {
                        Some(pp) if pp.id == ps => {
                            // a paste after a Cut keeps the number (the original is gone); a copy gets the next free one
                            let mut no = if o.keep_numbers { pp.item_number.clone() } else { String::new() };
                            if no.is_empty() {
                                let n = match next {
                                    Some(n) => n,
                                    None => {
                                        let k = crate::js::str_num(&super::next_item_number(self.w.api)?);
                                        if k != 0.0 && !k.is_nan() { k } else { 1.0 }
                                    }
                                };
                                no = crate::js::num_str(n);
                                next = Some(n + 1.0);
                            }
                            let q = pp.quantity.number();
                            let set = self.placement_pset(&no, q, &pp.notes)?;
                            self.w.relate(id, set)?;
                        }
                        _ => {
                            self.relate_pset(id, ps)?;
                        }
                    }
                }
                if !e.parts.is_empty() {
                    let g = self.w.guid();
                    let oh = self.w.owner_history();
                    let parts = e.parts.iter().map(|p| nid(&new_id, *p)).collect();
                    self.w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(id), JV::L(parts)])?;
                }
                for &op in &e.voids {
                    let g = self.w.guid();
                    let oh = self.w.owner_history();
                    self.w.make("IFCRELVOIDSELEMENT", vec![g, oh, JV::Null, JV::Null, JV::H(id), nid(&new_id, op)])?;
                }
                if let Some(f) = e.fills {
                    let g = self.w.guid();
                    let oh = self.w.owner_history();
                    self.w.make("IFCRELFILLSELEMENT", vec![g, oh, JV::Null, JV::Null, nid(&new_id, f), JV::H(id)])?;
                }
                if e.top {
                    let into = if changes_storey { dst_storey } else { e.container.or(dst_storey) };
                    if let Some(st) = into {
                        self.w.contain(id, st)?;
                    }
                }
            }
            let ids = snap.elements.iter().filter(|e| e.top).filter_map(|e| new_id.get(&id_key(e.id)).copied()).collect();
            Ok((ids, all))
        })();
        match res {
            Ok((ids, all)) => Ok(Pasted { ids, all, journal: self.w.journal.take().unwrap_or_default() }),
            Err(e) => {
                if let Some(j) = self.w.journal.take() {
                    undo_journal(self.w.api, &j)?;
                }
                Err(e)
            }
        }
    }
}
