//! ifc-props.js — every element's properties in one index (0.4 B15), Color by property, and colour schemes (0.5 WL.5).
//! Find (ifc-find.js), checks (ifc-checks.js), quantities (ifc-qto.js) and compare (ifc-compare.js) read the same index.
//!
//! Built-in fields: cls, type, storey, material, name, longname, objtype, tag, predefined, guid; then
//! 'p:<set>.<name>' from IfcRelDefinesByProperties (property and quantity sets) and the type's own sets, the instance
//! value winning (the first value put stays). Values: numbers (to 6 decimals), 'Yes' / 'No', text.

use std::collections::{HashMap, HashSet};

use cc_io_ifc::model::{Id, IfcFile, id_key};
use serde_json::{Map, Value as J};

use crate::api::{AV, Line, R, get_line, plain, type_err};
use crate::js::{OMap, PKey, PV, jn, locale_cmp, str_num, to_fixed};
use crate::model::Model;

pub const CATEGORICAL_LIGHT: [&str; 7] = ["#B3122E", "#1E48C8", "#8A6200", "#9A1F86", "#17703A", "#5B3CC4", "#A84A00"];
pub const CATEGORICAL_DARK: [&str; 7] = ["#EF5467", "#688CF0", "#B78B00", "#D65AC4", "#25AD5C", "#987DE5", "#D47713"];
pub const SEQUENTIAL_LIGHT: [&str; 6] = ["#83A9FB", "#658EEC", "#4873DD", "#2C57CE", "#143AB9", "#051B9E"];
pub const SEQUENTIAL_DARK: [&str; 6] = ["#2C57CE", "#4873DD", "#658EEC", "#83A9FB", "#A3C3FF", "#C3DEFF"];
pub const OTHER_LIGHT: &str = "#8A929C";
pub const OTHER_DARK: &str = "#7D8792";
pub const NONE_LIGHT: &str = "#D5DAE0";
pub const NONE_DARK: &str = "#3A434E";

const BUILTIN: [&str; 10] = ["cls", "type", "storey", "material", "name", "longname", "objtype", "tag", "predefined", "guid"];

/// One field of the index.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: String,
    /// '' for a built-in; a property's name (None = undefined: a property without a name)
    pub label: Option<PV>,
    pub group: Option<PV>,
    pub number: bool,
    pub count: usize,
}

impl Field {
    pub fn kind(&self) -> &'static str {
        if self.number { "number" } else { "text" }
    }
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("id".into(), J::String(self.id.clone()));
        if let Some(l) = &self.label {
            o.insert("label".into(), l.json());
        }
        if let Some(g) = &self.group {
            o.insert("group".into(), g.json());
        }
        o.insert("kind".into(), J::String(self.kind().into()));
        o.insert("count".into(), J::from(self.count));
        J::Object(o)
    }
}

/// The values of one field: expressID → value, in the order they were put.
pub type Vals = OMap<u64, PV>;

/// An id back from its map key.
pub fn idf(k: u64) -> f64 {
    f64::from_bits(k)
}

/// The property index.
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub fields: Vec<Field>,
    pub key_base: f64,
    pub ids: Vec<Id>,
    pub vals: HashMap<String, Vals>,
    /// what `values()` hands out for a field the index does not have
    pub empty: Vals,
}

impl Index {
    pub fn keys(&self) -> Vec<f64> {
        self.ids.iter().map(|id| self.key_base + id).collect()
    }
    /// `ix.values(fid)` (an empty map for a field it does not have).
    pub fn values(&self, fid: &str) -> &Vals {
        self.vals.get(fid).unwrap_or(&self.empty)
    }
    /// `ix.value(fid, id)`
    pub fn value(&self, fid: &str, id: Id) -> Option<&PV> {
        self.vals.get(fid).and_then(|m| m.get(&id_key(id)))
    }
    pub fn has_field(&self, fid: &str) -> bool {
        self.fields.iter().any(|f| f.id == fid)
    }
    pub fn field(&self, fid: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == fid)
    }

    /// A synthetic index (the tests' `{ keyBase, ids, values: () => Map }`, plus fields): JSON
    /// `{ keyBase, ids: [n], fields?: [{ id, label, group, kind }], values: { fid: [[id, v], …] } }`.
    pub fn from_json(v: &J) -> Index {
        let key_base = v.get("keyBase").and_then(J::as_f64).unwrap_or(0.0);
        let ids: Vec<Id> = v.get("ids").and_then(J::as_array).map(|a| a.iter().filter_map(J::as_f64).collect()).unwrap_or_default();
        let mut vals: HashMap<String, Vals> = HashMap::new();
        if let Some(o) = v.get("values").and_then(J::as_object) {
            for (fid, pairs) in o {
                let mut m = Vals::new();
                for p in pairs.as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    let (Some(id), Some(x)) = (p.get(0).and_then(J::as_f64), p.get(1)) else { continue };
                    let pv = match x {
                        J::Number(n) => PV::Num(n.as_f64().unwrap_or(f64::NAN)),
                        J::String(s) => PV::s(s),
                        other => PV::s(&cc_cad::js::to_js_string(other)),
                    };
                    m.set(id_key(id), pv);
                }
                vals.insert(fid.clone(), m);
            }
        }
        let fields = v
            .get("fields")
            .and_then(J::as_array)
            .map(|a| {
                a.iter()
                    .map(|f| {
                        let id = f.get("id").and_then(J::as_str).unwrap_or("").to_string();
                        let count = vals.get(&id).map_or(0, OMap::len);
                        Field {
                            label: f.get("label").and_then(J::as_str).map(PV::s),
                            group: f.get("group").and_then(J::as_str).map(PV::s),
                            number: f.get("kind").and_then(J::as_str) == Some("number"),
                            count,
                            id,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Index { fields, key_base, ids, vals, empty: Vals::new() }
    }
}

/// `kindOf(m)`: number when every value is a number.
fn all_numbers(m: &Vals) -> bool {
    m.vals.iter().all(PV::is_num)
}

/// JS truthiness of an index value.
fn truthy(v: &PV) -> bool {
    match v {
        PV::Num(n) => *n != 0.0 && !n.is_nan(),
        PV::Str(s) => !s.is_empty(),
    }
}

struct SetRows {
    name: PV,
    rows: Vec<(Option<PV>, Option<PV>)>,
}

struct Builder<'f, 'a> {
    f: &'f IfcFile<'a>,
    lines: HashMap<u64, Option<Line<'f, 'a>>>,
    vals: HashMap<String, Vals>,
    meta: HashMap<String, (Option<PV>, Option<PV>)>,
    order: Vec<String>,
}

impl<'f, 'a> Builder<'f, 'a> {
    /// `get(id)`: the line, null when there is none or it does not parse.
    fn get(&mut self, id: Option<f64>) -> Option<Line<'f, 'a>> {
        let id = id?;
        let k = id_key(id);
        if let Some(l) = self.lines.get(&k) {
            return *l;
        }
        let l = get_line(self.f, id).ok().flatten();
        self.lines.insert(k, l);
        l
    }
    fn put(&mut self, fid: &str, label: Option<PV>, group: Option<PV>, id: Id, v: Option<PV>) {
        let Some(v) = v else { return };
        let m = match self.vals.get_mut(fid) {
            Some(m) => m,
            None => {
                self.vals.insert(fid.to_string(), Vals::new());
                self.meta.insert(fid.to_string(), (label, group));
                self.order.push(fid.to_string());
                match self.vals.get_mut(fid) {
                    Some(m) => m,
                    None => return,
                }
            }
        };
        let k = id_key(id);
        if !m.has(&k) {
            m.set(k, v);
        }
    }
}

/// `x || y` on optional attributes: the first that is there and truthy.
fn or_attr(l: &Line<'_, '_>, names: &[&str]) -> Option<AV> {
    let mut last = None;
    for n in names {
        let v = l.get(n);
        if v.as_ref().is_some_and(AV::truthy) {
            return v;
        }
        last = v;
    }
    let _ = last;
    None
}

/// `x ?? y ?? …`: the first that is neither null nor undefined.
fn coalesce(l: &Line<'_, '_>, names: &[&str]) -> Option<AV> {
    names.iter().filter_map(|n| l.get(n)).find(|v| !matches!(v, AV::Null))
}

/// `(list || []).filter(isH)` → the handles' ids; TypeError when the value is not an array.
fn handles(v: Option<AV>) -> R<Vec<f64>> {
    match v {
        None | Some(AV::Null) => Ok(Vec::new()),
        Some(AV::List(l)) => Ok(l.iter().filter_map(|h| if let AV::Handle(n) = h { Some(*n) } else { None }).collect()),
        Some(_) => Err(type_err("x.filter is not a function")),
    }
}

/// `for (const h of v || [])` → each member's `h.value` (as a number when it is one).
fn members(v: Option<AV>) -> R<Vec<Option<f64>>> {
    match v {
        None | Some(AV::Null) => Ok(Vec::new()),
        Some(AV::List(l)) => l.iter().map(AV::member_num).collect(),
        Some(_) => Err(type_err("object is not iterable")),
    }
}

/// `propertyIndex(W, api, mid, model, { spaces })`.
pub fn property_index(f: &IfcFile<'_>, model: &Model, spaces: bool) -> R<Index> {
    let kb = model.key_base;
    let ids: Vec<Id> = model.class_ids.iter().copied().filter(|&id| (model.class(id) == Some("IfcSpace")) == spaces).collect();
    let want: HashSet<u64> = ids.iter().map(|&id| id_key(id)).collect();
    let mut b = Builder { f, lines: HashMap::new(), vals: HashMap::new(), meta: HashMap::new(), order: Vec::new() };
    let mut storey_name: HashMap<u64, PV> = HashMap::new();
    for s in &model.storeys {
        storey_name.insert(id_key(s.id), s.name.clone());
    }
    for &id in &ids {
        let l = b.get(Some(id));
        b.put("cls", None, None, id, model.class(id).map(PV::s));
        b.put("storey", None, None, id, model.storey(id).and_then(|s| storey_name.get(&id_key(s)).cloned()));
        let Some(l) = l else { continue };
        b.put("name", None, None, id, plain(l.get("Name").as_ref()));
        b.put("longname", None, None, id, plain(l.get("LongName").as_ref()));
        b.put("objtype", None, None, id, plain(l.get("ObjectType").as_ref()));
        b.put("tag", None, None, id, plain(l.get("Tag").as_ref()));
        b.put("predefined", None, None, id, plain(l.get("PredefinedType").as_ref()));
        b.put("guid", None, None, id, plain(l.get("GlobalId").as_ref()));
    }

    let mut cache: HashMap<u64, Option<std::rc::Rc<SetRows>>> = HashMap::new();
    let mut rows_of = |b: &mut Builder<'_, '_>, ps: f64| -> R<Option<std::rc::Rc<SetRows>>> {
        let k = id_key(ps);
        if let Some(c) = cache.get(&k) {
            return Ok(c.clone());
        }
        let r = set_rows(b, ps)?.map(std::rc::Rc::new);
        cache.insert(k, r.clone());
        Ok(r)
    };
    let put_set = |b: &mut Builder<'_, '_>, id: f64, set: &Option<std::rc::Rc<SetRows>>| {
        let Some(set) = set else { return };
        for (n, v) in &set.rows {
            let fid = format!("p:{}.{}", set.name.text(), n.as_ref().map_or_else(|| "undefined".to_string(), PV::text));
            b.put(&fid, n.clone(), Some(set.name.clone()), id, v.clone());
        }
    };

    for rid in f.of_type("IFCRELDEFINESBYPROPERTIES", true) {
        let Some(r) = b.get(Some(rid)) else { continue };
        let ps = r.get("RelatingPropertyDefinition").and_then(|v| v.value_num());
        let Some(ps) = ps.filter(|p| *p != 0.0 && !p.is_nan()) else { continue };
        for h in members(r.get("RelatedObjects"))? {
            if let Some(h) = h
                && want.contains(&id_key(h))
            {
                let set = rows_of(&mut b, ps)?;
                put_set(&mut b, h, &set);
            }
        }
    }
    for rid in f.of_type("IFCRELDEFINESBYTYPE", true) {
        let Some(r) = b.get(Some(rid)) else { continue };
        let rt = r.get("RelatingType").and_then(|v| v.value_num()).filter(|p| *p != 0.0 && !p.is_nan());
        let Some(ty) = (if rt.is_some() { b.get(rt) } else { None }) else { continue };
        let mut tsets = Vec::new();
        for h in handles(or_attr(&ty, &["HasPropertySets"]))? {
            tsets.push(rows_of(&mut b, h)?);
        }
        let tname = plain(ty.get("Name").as_ref());
        for h in members(r.get("RelatedObjects"))? {
            let Some(h) = h.filter(|h| want.contains(&id_key(*h))) else { continue };
            b.put("type", None, None, h, tname.clone());
            for s in &tsets {
                put_set(&mut b, h, s);
            }
        }
    }
    for rid in f.of_type("IFCRELASSOCIATESMATERIAL", true) {
        let Some(r) = b.get(Some(rid)) else { continue };
        let Some(rm) = r.get("RelatingMaterial").filter(AV::truthy) else { continue };
        let n = mat_name(&mut b, rm.value_num(), 0)?;
        for h in members(r.get("RelatedObjects"))? {
            if let Some(h) = h
                && want.contains(&id_key(h))
            {
                b.put("material", None, None, h, n.clone());
            }
        }
    }

    let mut fields = Vec::new();
    for id in BUILTIN {
        if let Some(m) = b.vals.get(id) {
            fields.push(Field { id: id.into(), label: Some(PV::s("")), group: Some(PV::s("")), number: all_numbers(m), count: m.len() });
        }
    }
    let mut rest: Vec<String> = b.order.iter().filter(|k| k.starts_with("p:")).cloned().collect();
    rest.sort_by(|a, c| locale_cmp(a, c));
    for id in rest {
        let (label, group) = b.meta.get(&id).cloned().unwrap_or((None, None));
        let (number, count) = b.vals.get(&id).map_or((true, 0), |m| (all_numbers(m), m.len()));
        fields.push(Field { id, label, group, number, count });
    }
    Ok(Index { fields, key_base: kb, ids, vals: b.vals, empty: Vals::new() })
}

/// `setRows(psId)`: a set's single values and quantities; None = null.
fn set_rows(b: &mut Builder<'_, '_>, ps_id: f64) -> R<Option<SetRows>> {
    let Some(ps) = b.get(Some(ps_id)) else { return Ok(None) };
    let name = plain(ps.get("Name").as_ref()).filter(truthy).unwrap_or_else(|| PV::s("Properties"));
    let mut rows = Vec::new();
    let list = or_attr(&ps, &["HasProperties", "Quantities"]);
    let items: Vec<AV> = match list {
        None => Vec::new(),
        Some(AV::List(l)) => l,
        Some(_) => return Err(type_err("object is not iterable")),
    };
    for h in items {
        let p = if let AV::Handle(id) = h { b.get(Some(id)) } else { None };
        let Some(p) = p else { continue };
        if !p.get("Name").is_some_and(|v| v.truthy()) {
            continue;
        }
        let v = match p.get("NominalValue") {
            Some(nv) => plain(Some(&nv)),
            None => plain(
                coalesce(
                    &p,
                    &[
                        "LengthValue",
                        "AreaValue",
                        "VolumeValue",
                        "CountValue",
                        "WeightValue",
                        "TimeValue",
                        "EnumerationValues",
                        "ListValues",
                    ],
                )
                .as_ref(),
            ),
        };
        rows.push((plain(p.get("Name").as_ref()), v));
    }
    Ok(Some(SetRows { name, rows }))
}

/// `matName(id, depth)`: a material, a layer set (usage), a list, a constituent set — names joined.
fn mat_name(b: &mut Builder<'_, '_>, id: Option<f64>, depth: u32) -> R<Option<PV>> {
    let Some(m) = b.get(id) else { return Ok(None) };
    if depth > 4 {
        return Ok(None);
    }
    let t = m.class().to_string();
    if t == "IfcMaterial" {
        return Ok(plain(m.get("Name").as_ref()));
    }
    if t == "IfcMaterialLayerSetUsage" {
        return match m.get("ForLayerSet").filter(AV::truthy) {
            Some(v) => mat_name(b, v.value_num(), depth.saturating_add(1)),
            None => Ok(None),
        };
    }
    if t == "IfcMaterialProfileSetUsage" {
        return match m.get("ForProfileSet").filter(AV::truthy) {
            Some(v) => mat_name(b, v.value_num(), depth.saturating_add(1)),
            None => Ok(None),
        };
    }
    let list = or_attr(&m, &["MaterialLayers", "Materials", "MaterialConstituents", "MaterialProfiles"]);
    let mut parts: Vec<PV> = Vec::new();
    for h in handles(list)? {
        let x = b.get(Some(h));
        let mat = x.and_then(|x| x.get("Material")).filter(AV::truthy);
        let n = match mat {
            Some(mv) => mat_name(b, mv.value_num(), depth.saturating_add(1))?,
            None => mat_name(b, Some(h), depth.saturating_add(1))?,
        };
        if let Some(n) = n.filter(truthy) {
            parts.push(n);
        }
    }
    if !parts.is_empty() {
        let mut seen: HashSet<PKey> = HashSet::new();
        let uniq: Vec<String> = parts.iter().filter(|p| seen.insert(p.key())).map(PV::text).collect();
        return Ok(Some(PV::s(&uniq.join(", "))));
    }
    Ok(plain(coalesce(&m, &["LayerSetName", "Name"]).as_ref()))
}

// ---- Color by ----

/// `other` on a legend row: a count of folded values, or `true` (a range scheme's outliers).
#[derive(Clone, Debug, PartialEq)]
pub enum Other {
    Count(usize),
    True,
}

/// One legend row (the fields a row has in the JS; absent ones stay None).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Row {
    pub label: String,
    pub value: Option<PV>,
    pub keys: Vec<f64>,
    pub color: String,
    pub count: usize,
    pub other: Option<Other>,
    pub none: bool,
    pub outside: bool,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

impl Row {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("label".into(), J::String(self.label.clone()));
        if let Some(v) = &self.value {
            o.insert("value".into(), v.json());
        }
        o.insert("keys".into(), J::Array(self.keys.iter().map(|k| jn(*k)).collect()));
        o.insert("color".into(), J::String(self.color.clone()));
        o.insert("count".into(), J::from(self.count));
        match &self.other {
            Some(Other::Count(n)) => {
                o.insert("other".into(), J::from(*n));
            }
            Some(Other::True) => {
                o.insert("other".into(), J::Bool(true));
            }
            None => {}
        }
        if self.none {
            o.insert("none".into(), J::Bool(true));
        }
        if self.outside {
            o.insert("outside".into(), J::Bool(true));
        }
        if let Some(x) = self.lo {
            o.insert("lo".into(), jn(x));
        }
        if let Some(x) = self.hi {
            o.insert("hi".into(), jn(x));
        }
        J::Object(o)
    }
}

/// A colouring: `{ kind, legend, colorOf }` (+ applyScheme's missing / unmatched / scheme).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Coloring {
    pub range: bool,
    pub legend: Vec<Row>,
    pub missing: Option<bool>,
    pub unmatched: Option<usize>,
}

impl Coloring {
    /// `colorOf`: key → colour, in Map order (a later row overrides the colour, the first place stays).
    pub fn color_of(&self) -> OMap<u64, (f64, String)> {
        let mut m = OMap::new();
        for l in &self.legend {
            for &k in &l.keys {
                m.set(id_key(k), (k, l.color.clone()));
            }
        }
        m
    }
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("kind".into(), J::String(if self.range { "range" } else { "category" }.into()));
        o.insert("legend".into(), J::Array(self.legend.iter().map(Row::json).collect()));
        let co = self.color_of();
        o.insert("colorOf".into(), J::Array(co.vals.iter().map(|(k, c)| J::Array(vec![jn(*k), J::String(c.clone())])).collect()));
        if let Some(m) = self.missing {
            o.insert("missing".into(), J::Bool(m));
        }
        if let Some(u) = self.unmatched {
            o.insert("unmatched".into(), J::from(u));
            o.insert("scheme".into(), J::Bool(true));
        }
        J::Object(o)
    }
}

fn fmt_n(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        to_fixed(v, 0)
    } else if a >= 10.0 {
        let s = to_fixed(v, 1);
        match s.strip_suffix(".0") {
            Some(t) => t.to_string(),
            None => s,
        }
    } else {
        // .replace(/\.?0+$/, '')
        let s = to_fixed(v, 3);
        let t = s.trim_end_matches('0');
        if t.len() == s.len() { s } else { t.strip_suffix('.').unwrap_or(t).to_string() }
    }
}

/// `fmtRange(a, b)`
pub fn fmt_range(a: f64, b: f64) -> String {
    format!("{} – {}", fmt_n(a), fmt_n(b))
}

/// `colorBy(ix, fid, { theme })`.
pub fn color_by(ix: &Index, fid: &str, dark: bool) -> Coloring {
    let vals = ix.values(fid);
    let cat: &[&str] = if dark { &CATEGORICAL_DARK } else { &CATEGORICAL_LIGHT };
    let seq: &[&str] = if dark { &SEQUENTIAL_DARK } else { &SEQUENTIAL_LIGHT };
    // group key → (the key as a value, keys)
    let mut groups: OMap<PKey, (PV, Vec<f64>)> = OMap::new();
    for (k, v) in vals.iter() {
        let g = if v.is_num() { v.clone() } else { PV::s(&v.text()) };
        if let Some(e) = groups.entry(g.key(), || (g.clone(), Vec::new())) {
            e.1.push(ix.key_base + idf(*k));
        }
    }
    let none: Vec<f64> = ix.ids.iter().filter(|id| !vals.has(&id_key(**id))).map(|id| ix.key_base + id).collect();
    let numeric = !vals.is_empty() && groups.vals.iter().all(|(g, _)| g.is_num());
    let mut legend = Vec::new();
    let mut range = false;
    if numeric && groups.len() > cat.len() {
        range = true;
        let nums: Vec<f64> = groups.vals.iter().filter_map(|(g, _)| g.as_num()).collect();
        let lo = nums.iter().copied().fold(f64::INFINITY, crate::js::min);
        let hi = nums.iter().copied().fold(f64::NEG_INFINITY, crate::js::max);
        let n = seq.len();
        let nf = 6.0;
        let st = (hi - lo) / nf;
        let step = if st == 0.0 || st.is_nan() { 1.0 } else { st };
        let mut bins: Vec<Row> = seq
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let fi = f64::from(u32::try_from(i).unwrap_or(0));
                let blo = lo + fi * step;
                let bhi = if i == n.saturating_sub(1) { hi } else { lo + (fi + 1.0) * step };
                Row { label: fmt_range(blo, bhi), color: (*c).into(), lo: Some(blo), hi: Some(bhi), ..Row::default() }
            })
            .collect();
        for (g, keys) in groups.vals.iter() {
            let v = g.as_num().unwrap_or(f64::NAN);
            let at = crate::js::min(nf - 1.0, ((v - lo) / step).floor());
            // NaN or out of range: the JS reads `bins[NaN].keys` and throws; never with finite values
            if let Some(b) = usize_of(at).and_then(|i| bins.get_mut(i)) {
                b.keys.extend_from_slice(keys);
            }
        }
        for mut b in bins {
            b.count = b.keys.len();
            legend.push(b);
        }
    } else {
        let mut sorted: Vec<&(PV, Vec<f64>)> = groups.vals.iter().collect();
        sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| locale_cmp(&a.0.text(), &b.0.text())));
        for (i, (v, keys)) in sorted.iter().take(cat.len()).enumerate() {
            legend.push(Row {
                label: v.text(),
                value: Some(v.clone()),
                keys: keys.clone(),
                color: cat.get(i).copied().unwrap_or("").into(),
                count: keys.len(),
                ..Row::default()
            });
        }
        let more: Vec<&&(PV, Vec<f64>)> = sorted.iter().skip(cat.len()).collect();
        if !more.is_empty() {
            let keys: Vec<f64> = more.iter().flat_map(|(_, k)| k.iter().copied()).collect();
            legend.push(Row {
                label: String::new(),
                other: Some(Other::Count(more.len())),
                count: keys.len(),
                keys,
                color: if dark { OTHER_DARK } else { OTHER_LIGHT }.into(),
                ..Row::default()
            });
        }
    }
    if !none.is_empty() {
        legend.push(Row {
            label: String::new(),
            none: true,
            count: none.len(),
            keys: none,
            color: if dark { NONE_DARK } else { NONE_LIGHT }.into(),
            ..Row::default()
        });
    }
    Coloring { range, legend, missing: None, unmatched: None }
}

/// A whole non-negative number as an index.
fn usize_of(x: f64) -> Option<usize> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 4_294_967_296.0 {
        // checked above: a whole number in u32 range
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = x as u32;
        usize::try_from(v).ok()
    } else {
        None
    }
}

// ---- Schemes ----

pub const SCHEME_FORMAT: &str = "bimcraft-color-scheme";
pub const SCHEME_MAX: usize = 500;

/// A scheme as readScheme returns it (or as a caller changed it).
#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub field: J,
    pub field_id: String,
    pub range: bool,
    pub other: String,
    pub none: String,
    /// category: (value, colour)
    pub values: Vec<(PV, String)>,
    /// range: (from, to, colour)
    pub ranges: Vec<(f64, f64, String)>,
}

impl Scheme {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("format".into(), J::String(SCHEME_FORMAT.into()));
        o.insert("version".into(), J::from(1));
        o.insert("field".into(), self.field.clone());
        o.insert("kind".into(), J::String(if self.range { "range" } else { "category" }.into()));
        o.insert("other".into(), J::String(self.other.clone()));
        o.insert("none".into(), J::String(self.none.clone()));
        if self.range {
            o.insert(
                "ranges".into(),
                J::Array(
                    self.ranges
                        .iter()
                        .map(|(a, b, c)| {
                            let mut r = Map::new();
                            r.insert("from".into(), jn(*a));
                            r.insert("to".into(), jn(*b));
                            r.insert("color".into(), J::String(c.clone()));
                            J::Object(r)
                        })
                        .collect(),
                ),
            );
        } else {
            o.insert(
                "values".into(),
                J::Array(
                    self.values
                        .iter()
                        .map(|(v, c)| {
                            let mut r = Map::new();
                            r.insert("value".into(), v.json());
                            r.insert("color".into(), J::String(c.clone()));
                            J::Object(r)
                        })
                        .collect(),
                ),
            );
        }
        J::Object(o)
    }

    /// A scheme object as the page hands it to applyScheme (already read): lenient, well-formed input.
    pub fn from_json(o: &J) -> Scheme {
        let field = o.get("field").cloned().unwrap_or(J::Null);
        let field_id = field.get("id").and_then(J::as_str).unwrap_or("").to_string();
        let range = o.get("kind").and_then(J::as_str) == Some("range");
        let s = |k: &str| o.get(k).and_then(J::as_str).unwrap_or("").to_string();
        let values = o
            .get("values")
            .and_then(J::as_array)
            .map(|a| {
                a.iter()
                    .map(|x| {
                        let v = match x.get("value") {
                            Some(J::Number(n)) => PV::Num(n.as_f64().unwrap_or(f64::NAN)),
                            Some(J::String(t)) => PV::s(t),
                            Some(other) => PV::s(&cc_cad::js::to_js_string(other)),
                            None => PV::s("undefined"),
                        };
                        (v, x.get("color").and_then(J::as_str).unwrap_or("").to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ranges = o
            .get("ranges")
            .and_then(J::as_array)
            .map(|a| {
                a.iter()
                    .map(|x| {
                        (
                            x.get("from").and_then(J::as_f64).unwrap_or(f64::NAN),
                            x.get("to").and_then(J::as_f64).unwrap_or(f64::NAN),
                            x.get("color").and_then(J::as_str).unwrap_or("").to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Scheme { field, field_id, range, other: s("other"), none: s("none"), values, ranges }
    }
}

/// `schemeOf(c, field)` → the saved scheme JSON. `field` is an index field as JSON (`{ id, label, group, kind }`).
pub fn scheme_of(c: &Coloring, field: &J) -> J {
    let mut fo = Map::new();
    let s_or = |k: &str, d: &str| -> J {
        match field.get(k) {
            Some(v) if cc_cad::js::truthy(Some(v)) => v.clone(),
            _ => J::String(d.into()),
        }
    };
    if let Some(id) = field.get("id") {
        fo.insert("id".into(), id.clone());
    }
    fo.insert("label".into(), s_or("label", ""));
    fo.insert("group".into(), s_or("group", ""));
    fo.insert("kind".into(), s_or("kind", "text"));
    let mut o = Map::new();
    o.insert("format".into(), J::String(SCHEME_FORMAT.into()));
    o.insert("version".into(), J::from(1));
    o.insert("field".into(), J::Object(fo));
    o.insert("kind".into(), J::String(if c.range { "range" } else { "category" }.into()));
    let rows: Vec<&Row> = c.legend.iter().filter(|l| !l.none && !l.other.as_ref().is_some_and(|x| *x != Other::Count(0))).collect();
    if c.range {
        o.insert(
            "ranges".into(),
            J::Array(
                rows.iter()
                    .map(|l| {
                        let mut r = Map::new();
                        if let Some(x) = l.lo {
                            r.insert("from".into(), jn(x));
                        }
                        if let Some(x) = l.hi {
                            r.insert("to".into(), jn(x));
                        }
                        r.insert("color".into(), J::String(l.color.clone()));
                        J::Object(r)
                    })
                    .collect(),
            ),
        );
    } else {
        o.insert(
            "values".into(),
            J::Array(
                rows.iter()
                    .map(|l| {
                        let mut r = Map::new();
                        r.insert("value".into(), l.value.as_ref().map_or_else(|| J::String(l.label.clone()), PV::json));
                        r.insert("color".into(), J::String(l.color.clone()));
                        J::Object(r)
                    })
                    .collect(),
            ),
        );
    }
    let other = c.legend.iter().find(|l| l.other.as_ref().is_some_and(|x| *x != Other::Count(0))).map(|l| l.color.clone());
    let none = c.legend.iter().find(|l| l.none).map(|l| l.color.clone());
    o.insert("other".into(), J::String(other.filter(|s| !s.is_empty()).unwrap_or_else(|| OTHER_LIGHT.into())));
    o.insert("none".into(), J::String(none.filter(|s| !s.is_empty()).unwrap_or_else(|| NONE_LIGHT.into())));
    J::Object(o)
}

/// `/^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : d`
fn hex_color(v: Option<&J>, d: Option<&str>) -> Option<String> {
    if let Some(J::String(s)) = v {
        let b = s.as_bytes();
        if b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit) {
            return Some(s.to_ascii_lowercase());
        }
    }
    d.map(str::to_string)
}

/// `readScheme(json)` → Ok(scheme) | Err(error code).
pub fn read_scheme(text: Option<&str>, parsed: Option<&J>) -> Result<Scheme, &'static str> {
    let owned;
    let o = match (text, parsed) {
        (Some(t), _) => {
            owned = serde_json::from_str::<J>(t).map_err(|_| "json")?;
            &owned
        }
        (None, Some(p)) => p,
        (None, None) => return Err("format"),
    };
    let obj = match o {
        J::Object(m) => m,
        // an array is an object without .format
        _ => return Err("format"),
    };
    if obj.get("format").and_then(J::as_str) != Some(SCHEME_FORMAT) {
        return Err("format");
    }
    if obj.get("version").and_then(J::as_f64) != Some(1.0) {
        return Err("version");
    }
    let f = obj.get("field");
    let fid = match f.and_then(|f| f.get("id")) {
        Some(J::String(s)) if !s.is_empty() && crate::js::len16(s) <= 300 => s.clone(),
        _ => return Err("field"),
    };
    let f = f.unwrap_or(&J::Null);
    let st = |v: Option<&J>| -> String {
        match v {
            Some(J::String(s)) => crate::js::slice16(s, 300),
            _ => String::new(),
        }
    };
    let mut field = Map::new();
    field.insert("id".into(), J::String(fid.clone()));
    field.insert("label".into(), J::String(st(f.get("label"))));
    field.insert("group".into(), J::String(st(f.get("group"))));
    field.insert("kind".into(), J::String(if f.get("kind").and_then(J::as_str) == Some("number") { "number" } else { "text" }.into()));
    let range = obj.get("kind").and_then(J::as_str) == Some("range");
    let other = hex_color(obj.get("other"), Some(OTHER_LIGHT)).unwrap_or_default();
    let none = hex_color(obj.get("none"), Some(NONE_LIGHT)).unwrap_or_default();
    let mut s = Scheme { field: J::Object(field), field_id: fid, range, other, none, values: Vec::new(), ranges: Vec::new() };
    if range {
        let r = obj.get("ranges").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]);
        if r.is_empty() || r.len() > SCHEME_MAX {
            return Err("rows");
        }
        for x in r {
            let from = cc_cad::js::to_number_opt(x.get("from"));
            let to = cc_cad::js::to_number_opt(x.get("to"));
            let c = hex_color(x.get("color"), None);
            let Some(c) = c.filter(|_| from.is_finite() && to.is_finite() && to >= from) else { return Err("rows") };
            s.ranges.push((from, to, c));
        }
        s.ranges.sort_by(|a, b| crate::js::sign_ord(a.0 - b.0));
    } else {
        let v = obj.get("values").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]);
        if v.is_empty() || v.len() > SCHEME_MAX {
            return Err("rows");
        }
        let mut seen: HashSet<String> = HashSet::new();
        for x in v {
            let val = match x.get("value") {
                Some(J::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => Some(PV::Num(n.as_f64().unwrap_or(0.0))),
                Some(J::String(t)) => Some(PV::s(&crate::js::slice16(t, 500))),
                _ => None,
            };
            let c = hex_color(x.get("color"), None);
            let (Some(val), Some(c)) = (val, c) else { return Err("rows") };
            let k = match &val {
                PV::Num(n) => format!("number:{}", crate::js::num_str(*n)),
                PV::Str(t) => format!("string:{t}"),
            };
            if !seen.insert(k) {
                continue;
            }
            s.values.push((val, c));
        }
    }
    Ok(s)
}

/// `applyScheme(ix, scheme)`.
pub fn apply_scheme(ix: &Index, scheme: &Scheme) -> Coloring {
    let fid = &scheme.field_id;
    let missing = !ix.has_field(fid);
    let empty = Vals::new();
    let vals = if missing { &empty } else { ix.values(fid) };
    let none: Vec<f64> = ix.ids.iter().filter(|id| !vals.has(&id_key(**id))).map(|id| ix.key_base + id).collect();
    let mut legend = Vec::new();
    let mut unmatched = 0usize;
    if scheme.range {
        let rr = &scheme.ranges;
        let mut bins: Vec<Row> = rr
            .iter()
            .map(|(a, b, c)| Row { label: fmt_range(*a, *b), lo: Some(*a), hi: Some(*b), color: c.clone(), ..Row::default() })
            .collect();
        let mut rest = Vec::new();
        let last = rr.len().saturating_sub(1);
        for (k, v) in vals.iter() {
            let n = match v {
                PV::Num(n) => *n,
                PV::Str(s) => str_num(s),
            };
            let i = if n.is_finite() {
                rr.iter().enumerate().position(|(j, (from, to, _))| n >= *from && (n < *to || (j == last && n <= *to)))
            } else {
                None
            };
            let key = ix.key_base + idf(*k);
            match i.and_then(|i| bins.get_mut(i)) {
                Some(b) => b.keys.push(key),
                None => rest.push(key),
            }
        }
        for mut b in bins {
            b.count = b.keys.len();
            legend.push(b);
        }
        if !rest.is_empty() {
            unmatched = rest.len();
            legend.push(Row {
                label: String::new(),
                other: Some(Other::True),
                outside: true,
                count: rest.len(),
                keys: rest,
                color: scheme.other.clone(),
                ..Row::default()
            });
        }
    } else {
        let mut rows: Vec<Row> = scheme
            .values
            .iter()
            .map(|(v, c)| Row { label: v.text(), value: Some(v.clone()), color: c.clone(), ..Row::default() })
            .collect();
        let mut at: HashMap<String, usize> = HashMap::new();
        for (i, (v, _)) in scheme.values.iter().enumerate() {
            let k = match v {
                PV::Num(n) => format!("n:{}", crate::js::num_str(*n)),
                PV::Str(s) => format!("s:{s}"),
            };
            at.insert(k, i);
        }
        let mut rest: OMap<String, Vec<f64>> = OMap::new();
        for (k, v) in vals.iter() {
            let i = match v {
                PV::Num(n) => {
                    let t = crate::js::num_str(*n);
                    at.get(&format!("n:{t}")).or_else(|| at.get(&format!("s:{t}"))).copied()
                }
                PV::Str(s) => at.get(&format!("s:{s}")).copied(),
            };
            let key = ix.key_base + idf(*k);
            match i.and_then(|i| rows.get_mut(i)) {
                Some(r) => r.keys.push(key),
                None => {
                    if let Some(e) = rest.entry(v.text(), Vec::new) {
                        e.push(key);
                    }
                }
            }
        }
        for mut r in rows {
            r.count = r.keys.len();
            legend.push(r);
        }
        if !rest.is_empty() {
            let keys: Vec<f64> = rest.vals.iter().flatten().copied().collect();
            unmatched = rest.len();
            legend.push(Row {
                label: String::new(),
                other: Some(Other::Count(rest.len())),
                outside: true,
                count: keys.len(),
                keys,
                color: scheme.other.clone(),
                ..Row::default()
            });
        }
    }
    if !none.is_empty() {
        legend.push(Row { label: String::new(), none: true, count: none.len(), keys: none, color: scheme.none.clone(), ..Row::default() });
    }
    Coloring { range: scheme.range, legend, missing: Some(missing), unmatched: Some(unmatched) }
}

/// The JSON of an index (fields, keys, every value) for the parity harness.
pub fn index_json(ix: &Index) -> J {
    let mut o = Map::new();
    o.insert("fields".into(), J::Array(ix.fields.iter().map(Field::json).collect()));
    o.insert("keyBase".into(), jn(ix.key_base));
    o.insert("ids".into(), J::Array(ix.ids.iter().map(|x| jn(*x)).collect()));
    let mut vals = Map::new();
    for fld in &ix.fields {
        let m = ix.values(&fld.id);
        vals.insert(fld.id.clone(), J::Array(m.iter().map(|(k, v)| J::Array(vec![jn(idf(*k)), v.json()])).collect()));
    }
    o.insert("values".into(), J::Object(vals));
    J::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_labels() {
        assert_eq!(fmt_range(0.0, 15.833333333333334), "0 – 15.8");
        assert_eq!(fmt_range(79.16666666666667, 95.0), "79.2 – 95");
        assert_eq!(fmt_n(1.5), "1.5");
        assert_eq!(fmt_n(2.0), "2");
        assert_eq!(fmt_n(-0.0001), "-0");
        assert_eq!(fmt_n(1234.5), "1235");
    }

    #[test]
    fn synthetic_range() {
        let mut vals = HashMap::new();
        let mut m = Vals::new();
        for i in 0..20u32 {
            m.set(id_key(f64::from(i + 1)), PV::Num(f64::from(i * 5)));
        }
        vals.insert("x".to_string(), m);
        let ix = Index { fields: vec![], key_base: 0.0, ids: (1..=20u32).map(f64::from).collect(), vals, empty: Vals::new() };
        let c = color_by(&ix, "x", false);
        assert!(c.range);
        assert_eq!(c.legend.len(), 6);
        assert_eq!(c.legend[0].label, "0 – 15.8");
        assert!(c.legend[5].keys.contains(&20.0));
    }

    #[test]
    fn schemes_refuse() {
        assert_eq!(read_scheme(Some("{"), None).unwrap_err(), "json");
        assert_eq!(read_scheme(Some(r#"{"format":"x"}"#), None).unwrap_err(), "format");
        let ok = read_scheme(Some(r##"{"format":"bimcraft-color-scheme","version":1,"field":{"id":"cls"},"values":[{"value":"A","color":"#AABBCC"},{"value":"A","color":"#000000"},{"value":1,"color":"#111111"},{"value":"1","color":"#222222"}],"other":"nope"}"##), None).unwrap();
        assert_eq!(ok.values.len(), 3);
        assert_eq!(ok.values[0].1, "#aabbcc");
        assert_eq!(ok.other, OTHER_LIGHT);
    }
}
