//! ifc-elements.js, the reading half (0.4 B1): why an element cannot be deleted or copied (`whyNot`), the
//! relationships an element set leans on (`relations`: parts, openings, fills, containment, property sets, type,
//! material) and what an element takes with it (`expand`). The writing half (remove / paste through IfcPlacer and
//! the editor's journal) needs ifc-edit.js and is not ported here.

use cc_io_ifc::model::{Id, IfcFile};
use serde_json::{Map, Value as J};

use crate::api::{AV, Prim, R, get_line, type_err};
use crate::js::{OMap, jn};

/// `whyNot(id)`: '' = it can be deleted or copied.
pub fn why_not(f: &IfcFile<'_>, id: Id) -> &'static str {
    let Ok(Some(line)) = get_line(f, id) else { return "not-element" };
    if !line.get("GlobalId").is_some_and(|v| v.truthy()) {
        return "not-element";
    }
    let cls = line.class().to_uppercase();
    if cls == "IFCPROJECT" {
        return "spatial";
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
    if SPATIAL.contains(&cls.as_str()) {
        return "spatial";
    }
    if !line.has("ObjectPlacement") {
        return "not-element";
    }
    ""
}

/// A Map key / value as the JS holds it: `x?.value` of an attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum RV {
    Undef,
    Null,
    Num(f64),
    Str(String),
    Bool(bool),
    /// an object or array value: never equal to anything else
    Obj(u64),
}

impl RV {
    fn key(&self) -> String {
        match self {
            RV::Undef => "u".into(),
            RV::Null => "l".into(),
            RV::Num(n) => format!("n{}", if *n == 0.0 { 0 } else { n.to_bits() }),
            RV::Str(s) => format!("s{s}"),
            RV::Bool(b) => format!("b{b}"),
            RV::Obj(i) => format!("o{i}"),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            RV::Undef | RV::Null => false,
            RV::Num(n) => *n != 0.0 && !n.is_nan(),
            RV::Str(s) => !s.is_empty(),
            RV::Bool(b) => *b,
            RV::Obj(_) => true,
        }
    }
    pub fn json(&self) -> J {
        match self {
            RV::Undef => J::String("$undefined".into()),
            RV::Null => J::Null,
            RV::Num(n) => jn(*n),
            RV::Str(s) => J::String(s.clone()),
            RV::Bool(b) => J::Bool(*b),
            RV::Obj(_) => J::String("$object".into()),
        }
    }
}

struct Ctr(u64);

impl Ctr {
    /// `v?.value`
    fn of(&mut self, v: Option<&AV>) -> RV {
        match v {
            None | Some(AV::Null) => RV::Undef,
            Some(AV::Derived) => RV::Null,
            Some(AV::Handle(n)) => RV::Num(*n),
            Some(AV::List(_)) => RV::Undef,
            Some(AV::Val(p)) => match p {
                Prim::Undef => RV::Undef,
                Prim::Null => RV::Null,
                Prim::Num(n) => RV::Num(*n),
                Prim::Str(s) => RV::Str(s.clone()),
                Prim::Bool(b) => RV::Bool(*b),
                Prim::Arr(_) | Prim::Obj => {
                    self.0 = self.0.saturating_add(1);
                    RV::Obj(self.0)
                }
            },
        }
    }
    /// `for (const h of v || [])` → each `h.value` (TypeError on a null member or a value that is not a list)
    fn members(&mut self, v: Option<AV>) -> R<Vec<RV>> {
        match v {
            None | Some(AV::Null) => Ok(Vec::new()),
            Some(AV::List(l)) => {
                let mut out = Vec::with_capacity(l.len());
                for h in &l {
                    if matches!(h, AV::Null) {
                        return Err(type_err("Cannot read properties of null (reading 'value')"));
                    }
                    out.push(self.of(Some(h)));
                }
                Ok(out)
            }
            Some(_) => Err(type_err("object is not iterable")),
        }
    }
}

/// A Map keyed by RV (insertion order).
#[derive(Clone, Debug)]
pub struct RMap<V> {
    pub m: OMap<String, (RV, V)>,
}

impl<V> Default for RMap<V> {
    fn default() -> Self {
        RMap { m: OMap::new() }
    }
}

impl<V: Clone> RMap<V> {
    fn set(&mut self, k: RV, v: V) {
        self.m.set(k.key(), (k, v));
    }
    pub fn get(&self, k: &RV) -> Option<&V> {
        self.m.get(&k.key()).map(|x| &x.1)
    }
    pub fn has(&self, k: &RV) -> bool {
        self.m.has(&k.key())
    }
}

impl RMap<Vec<RV>> {
    fn push(&mut self, k: RV, v: RV) {
        let key = k.key();
        if let Some(e) = self.m.entry(key, || (k, Vec::new())) {
            e.1.push(v);
        }
    }
}

/// `relations()`.
#[derive(Clone, Debug, Default)]
pub struct Relations {
    pub parts: RMap<Vec<RV>>,
    pub part_of: RMap<RV>,
    pub voids: RMap<Vec<RV>>,
    pub void_host: RMap<RV>,
    pub fills: RMap<Vec<RV>>,
    pub filled_by: RMap<RV>,
    pub container: RMap<RV>,
    pub psets: RMap<Vec<RV>>,
    pub ty: RMap<RV>,
    pub material: RMap<RV>,
    pub rels: Vec<Id>,
}

/// The ids of an entity type and its subtypes (ifc-util.js `ids(api, mid, name, true)`).
fn ids(f: &IfcFile<'_>, up: &str) -> Vec<Id> {
    f.of_type(up, true)
}

pub fn relations(f: &IfcFile<'_>) -> R<Relations> {
    let mut r = Relations::default();
    let mut c = Ctr(0);
    for rid in ids(f, "IFCRELATIONSHIP") {
        let line = get_line(f, rid)?.ok_or_else(|| type_err("Cannot read properties of null (reading 'constructor')"))?;
        r.rels.push(rid);
        let t = line.class().to_uppercase();
        match t.as_str() {
            "IFCRELAGGREGATES" | "IFCRELNESTS" => {
                let ro = c.of(line.get("RelatingObject").as_ref());
                for h in c.members(line.get("RelatedObjects"))? {
                    r.parts.push(ro.clone(), h.clone());
                    r.part_of.set(h, ro.clone());
                }
            }
            "IFCRELVOIDSELEMENT" => {
                let host = c.of(line.get("RelatingBuildingElement").as_ref());
                let op = c.of(line.get("RelatedOpeningElement").as_ref());
                r.voids.push(host.clone(), op.clone());
                r.void_host.set(op, host);
            }
            "IFCRELFILLSELEMENT" => {
                let op = c.of(line.get("RelatingOpeningElement").as_ref());
                let el = c.of(line.get("RelatedBuildingElement").as_ref());
                r.fills.push(op.clone(), el.clone());
                r.filled_by.set(el, op);
            }
            "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                let s = c.of(line.get("RelatingStructure").as_ref());
                for h in c.members(line.get("RelatedElements"))? {
                    r.container.set(h, s.clone());
                }
            }
            "IFCRELDEFINESBYPROPERTIES" => {
                let p = c.of(line.get("RelatingPropertyDefinition").as_ref());
                for h in c.members(line.get("RelatedObjects"))? {
                    r.psets.push(h, p.clone());
                }
            }
            "IFCRELDEFINESBYTYPE" => {
                let ty = c.of(line.get("RelatingType").as_ref());
                for h in c.members(line.get("RelatedObjects"))? {
                    r.ty.set(h, ty.clone());
                }
            }
            "IFCRELASSOCIATESMATERIAL" => {
                let m = c.of(line.get("RelatingMaterial").as_ref());
                for h in c.members(line.get("RelatedObjects"))? {
                    r.material.set(h, m.clone());
                }
            }
            _ => {}
        }
    }
    Ok(r)
}

/// `expand(list, R, { withFilledOpenings })`: the elements plus their parts and openings (any depth).
pub fn expand(list: &[Id], r: &Relations, with_filled: bool) -> Vec<RV> {
    let mut out: OMap<String, RV> = OMap::new();
    let mut todo: Vec<RV> = Vec::new();
    for &id in list {
        let v = RV::Num(id);
        out.set(v.key(), v.clone());
        todo.push(v);
    }
    let mut guard = 0usize;
    while let Some(x) = todo.pop() {
        guard = guard.saturating_add(1);
        if guard > 50_000_000 {
            break;
        }
        let parts = r.parts.get(&x).cloned().unwrap_or_default();
        let voids = r.voids.get(&x).cloned().unwrap_or_default();
        for y in parts.into_iter().chain(voids) {
            if y.truthy() && !out.has(&y.key()) {
                out.set(y.key(), y.clone());
                todo.push(y);
            }
        }
    }
    if with_filled {
        let snapshot: Vec<RV> = out.vals.clone();
        for x in snapshot {
            let Some(op) = r.filled_by.get(&x) else { continue };
            if op.truthy() && !out.has(&op.key()) {
                let fills = r.fills.get(op).cloned().unwrap_or_default();
                if fills.iter().all(|f| out.has(&f.key())) {
                    out.set(op.key(), op.clone());
                }
            }
        }
    }
    out.vals
}

fn pairs_json<V>(m: &RMap<V>, val: impl Fn(&V) -> J) -> J {
    J::Array(m.m.vals.iter().map(|(k, v)| J::Array(vec![k.json(), val(v)])).collect())
}

impl Relations {
    pub fn json(&self) -> J {
        let list = |v: &Vec<RV>| J::Array(v.iter().map(RV::json).collect());
        let one = |v: &RV| v.json();
        let mut o = Map::new();
        o.insert("parts".into(), pairs_json(&self.parts, list));
        o.insert("partOf".into(), pairs_json(&self.part_of, one));
        o.insert("voids".into(), pairs_json(&self.voids, list));
        o.insert("voidHost".into(), pairs_json(&self.void_host, one));
        o.insert("fills".into(), pairs_json(&self.fills, list));
        o.insert("filledBy".into(), pairs_json(&self.filled_by, one));
        o.insert("container".into(), pairs_json(&self.container, one));
        o.insert("psets".into(), pairs_json(&self.psets, list));
        o.insert("type".into(), pairs_json(&self.ty, one));
        o.insert("material".into(), pairs_json(&self.material, one));
        o.insert("rels".into(), J::Array(self.rels.iter().map(|x| jn(*x)).collect()));
        J::Object(o)
    }
}
