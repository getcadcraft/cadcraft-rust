//! BimCraft's IFC questions for assistants — 0.5 tools/cli/headless/ifc.mjs on cc-io-ifc (the Rust cc-ifc) and
//! cc-bim's API-shaped lines: properties only, no geometry, so a large model answers quickly.

use std::collections::{HashMap, HashSet};

use cc_bim::api::{AV, Line, Prim, get_line};
use cc_io_ifc::model::{Id, IfcFile, id_key};
use cc_io_ifc::step::StepIndex;
use cc_io_ifc::value::Value as SV;
use serde_json::{Map, Value, json};

/// IFC string escapes (\X2\…\X0\, \X\hh, \S\c) → text (ifc.mjs `decodeIfcString`).
pub fn decode_ifc_string(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    // \X2\<hex>\X0\ first (case-insensitive), as UTF-16 code units in groups of four
    let mut out16: Vec<u16> = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let starts = |at: usize, pat: &str| -> bool {
        let p: Vec<char> = pat.chars().collect();
        p.iter().enumerate().all(|(k, c)| cs.get(at.saturating_add(k)).is_some_and(|x| x.eq_ignore_ascii_case(c)))
    };
    let mut first: Vec<char> = Vec::new();
    while i < cs.len() {
        if starts(i, "\\X2\\") {
            let body_start = i.saturating_add(4);
            let mut j = body_start;
            while cs.get(j).is_some_and(char::is_ascii_hexdigit) {
                j = j.saturating_add(1);
            }
            if j > body_start && starts(j, "\\X0\\") {
                let hex: String = cs.get(body_start..j).unwrap_or(&[]).iter().collect();
                // `h.match(/.{4}/g)`: whole groups of four; a short tail is dropped
                for g4 in hex.as_bytes().chunks(4).filter(|c| c.len() == 4) {
                    if let Ok(g) = std::str::from_utf8(g4)
                        && let Ok(u) = u16::from_str_radix(g, 16)
                    {
                        out16.push(u);
                    }
                }
                first.extend(char::decode_utf16(out16.drain(..)).map(|r| r.unwrap_or('\u{fffd}')));
                i = j.saturating_add(4);
                continue;
            }
        }
        if let Some(c) = cs.get(i) {
            first.push(*c);
        }
        i = i.saturating_add(1);
    }
    // then \X\hh, then \S\c
    let mut second: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < first.len() {
        let at = |k: usize| first.get(i.saturating_add(k)).copied();
        if at(0) == Some('\\')
            && at(1).is_some_and(|c| c.eq_ignore_ascii_case(&'X'))
            && at(2) == Some('\\')
            && let (Some(a), Some(b)) = (at(3), at(4))
            && a.is_ascii_hexdigit()
            && b.is_ascii_hexdigit()
        {
            let v = u32::from_str_radix(&format!("{a}{b}"), 16).unwrap_or(0);
            second.push(char::from_u32(v).unwrap_or('\u{fffd}'));
            i = i.saturating_add(5);
            continue;
        }
        if at(0) == Some('\\')
            && at(1) == Some('S')
            && at(2) == Some('\\')
            && let Some(c) = at(3)
        {
            let v = u32::from(c).saturating_add(128);
            second.push(char::from_u32(v).unwrap_or('\u{fffd}'));
            i = i.saturating_add(4);
            continue;
        }
        if let Some(c) = at(0) {
            second.push(c);
        }
        i = i.saturating_add(1);
    }
    second.into_iter().collect()
}

/// A number as JSON (integers stay integers).
fn jnum(n: f64) -> Value {
    cc_cad::num(n)
}

/// ifc.mjs `val(v)` of an API value: None = undefined.
fn val(v: &AV) -> Option<Value> {
    match v {
        AV::Null | AV::Derived => Some(Value::Null),
        AV::List(l) => Some(Value::Array(l.iter().map(|x| val(x).unwrap_or(Value::Null)).collect())),
        AV::Handle(n) => Some(json!({"ref": jnum(*n)})),
        AV::Val(p) => prim(p),
    }
}
fn prim(p: &Prim) -> Option<Value> {
    match p {
        Prim::Undef => None,
        Prim::Null => Some(Value::Null),
        Prim::Bool(b) => Some(Value::Bool(*b)),
        Prim::Num(n) => Some(jnum(*n)),
        Prim::Str(s) => Some(Value::String(decode_ifc_string(s))),
        Prim::Arr(a) => Some(Value::Array(a.iter().map(|x| prim(x).unwrap_or(Value::Null)).collect())),
        Prim::Obj => Some(Value::Null),
    }
}
fn val_opt(v: Option<&AV>) -> Option<Value> {
    v.and_then(val)
}
/// `String(v)` of what val gave (for property matching).
fn js_str(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(x) => cc_cad::js::to_js_string(x),
    }
}

/// An open IFC model: the file's bytes and index, and what ifc.mjs `index()` works out once.
pub struct IfcDoc {
    bytes: Vec<u8>,
    ix: StepIndex,
    pub name: String,
    elements: Vec<Id>,
    storey_ids: Vec<Id>,
    storeys: HashSet<u64>,
    container: HashMap<u64, Id>,
    parent: HashMap<u64, Id>,
    type_of: HashMap<u64, Id>,
}

impl std::fmt::Debug for IfcDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IfcDoc").field("name", &self.name).field("elements", &self.elements.len()).finish()
    }
}

fn refs(v: Option<&SV>) -> Vec<Id> {
    match v {
        Some(SV::List(l)) => l.iter().filter_map(SV::as_ref_id).collect(),
        _ => Vec::new(),
    }
}
fn one_ref(v: Option<&SV>) -> Option<Id> {
    v.and_then(SV::as_ref_id)
}

impl IfcDoc {
    /// ifc.mjs `IfcSession.open` + `index()`.
    pub fn open(bytes: Vec<u8>, name: &str) -> Result<IfcDoc, String> {
        let bad = || format!("\"{name}\" could not be read as IFC (ISO 10303-21).");
        let ix = cc_io_ifc::step::scan_step(&bytes).map_err(|_| bad())?;
        let mut d = IfcDoc {
            bytes: Vec::new(),
            ix,
            name: name.to_string(),
            elements: Vec::new(),
            storey_ids: Vec::new(),
            storeys: HashSet::new(),
            container: HashMap::new(),
            parent: HashMap::new(),
            type_of: HashMap::new(),
        };
        d.bytes = bytes;
        {
            let f = IfcFile::new(&d.bytes, d.ix.clone());
            let attr = |id: Id, k: &str| f.attr(id, k).ok().flatten();
            let mut ids: Vec<Id> = f.of_type("IFCELEMENT", true);
            ids.extend(f.of_type("IFCSPACE", true));
            if !f.schema.has("IFCELEMENT") {
                ids.extend(f.of_type("IFCPRODUCT", true));
            }
            let mut seen = HashSet::new();
            ids.retain(|id| seen.insert(id_key(*id)));
            ids.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            d.elements = ids;
            for r in f.of_type("IFCRELCONTAINEDINSPATIALSTRUCTURE", true) {
                let st = one_ref(attr(r, "RelatingStructure"));
                for x in refs(attr(r, "RelatedElements")) {
                    if let Some(s) = st {
                        d.container.insert(id_key(x), s);
                    }
                }
            }
            for r in f.of_type("IFCRELAGGREGATES", true) {
                let p = one_ref(attr(r, "RelatingObject"));
                for x in refs(attr(r, "RelatedObjects")) {
                    if let Some(p) = p {
                        d.parent.insert(id_key(x), p);
                    }
                }
            }
            for r in f.of_type("IFCRELNESTS", true) {
                let p = one_ref(attr(r, "RelatingObject"));
                for x in refs(attr(r, "RelatedObjects")) {
                    if let Some(p) = p {
                        d.parent.entry(id_key(x)).or_insert(p);
                    }
                }
            }
            d.storey_ids = f.of_type("IFCBUILDINGSTOREY", true);
            d.storeys = d.storey_ids.iter().map(|i| id_key(*i)).collect();
            for r in f.of_type("IFCRELDEFINESBYTYPE", true) {
                let t = one_ref(attr(r, "RelatingType"));
                for x in refs(attr(r, "RelatedObjects")) {
                    if let Some(t) = t {
                        d.type_of.insert(id_key(x), t);
                    }
                }
            }
        }
        Ok(d)
    }

    fn file(&self) -> IfcFile<'_> {
        IfcFile::new(&self.bytes, self.ix.clone())
    }

    fn storey_of(&self, id: Id) -> Option<Id> {
        let mut cur = Some(id);
        for _ in 0..32 {
            let c = cur?;
            if self.storeys.contains(&id_key(c)) {
                return Some(c);
            }
            cur = self.container.get(&id_key(c)).or_else(|| self.parent.get(&id_key(c))).copied();
        }
        None
    }

    fn pretty(f: &IfcFile<'_>, id: Id) -> Value {
        match f.ty(id) {
            Some(up) if !up.is_empty() => Value::String(f.schema.pretty(up).unwrap_or(up).to_string()),
            _ => Value::Null,
        }
    }

    /// ifc.mjs `attrText(id, attr)`.
    fn attr_text(f: &IfcFile<'_>, id: Id, k: &str) -> Value {
        match f.attr(id, k).ok().flatten() {
            None | Some(SV::Null) => Value::Null,
            Some(SV::Str(s)) => Value::String(decode_ifc_string(&s.lossy())),
            Some(SV::Typed(t)) => {
                let inner = match &t.1 {
                    SV::Str(s) => s.lossy().into_owned(),
                    SV::Num(n) => cc_cad::js::num_to_string(*n),
                    SV::Enum(e) => e.to_string(),
                    SV::Null => "null".into(),
                    _ => "[object Object]".into(),
                };
                Value::String(decode_ifc_string(&inner))
            }
            Some(SV::Enum(e)) => Value::String(e.to_string()),
            Some(SV::Num(n)) => jnum(*n),
            _ => Value::Null,
        }
    }

    fn row(&self, f: &IfcFile<'_>, id: Id) -> Value {
        let st = self.storey_of(id);
        let ty = self.type_of.get(&id_key(id)).copied();
        json!({
            "id": jnum(id), "guid": Self::attr_text(f, id, "GlobalId"), "class": Self::pretty(f, id), "name": Self::attr_text(f, id, "Name"),
            "objectType": Self::attr_text(f, id, "ObjectType"), "storey": st.map_or(Value::Null, |s| Self::attr_text(f, s, "Name")),
            "type": ty.map_or(Value::Null, |t| json!({"id": jnum(t), "class": Self::pretty(f, t), "name": Self::attr_text(f, t, "Name")})),
        })
    }

    /// ifc.mjs `summary()`.
    pub fn summary(&self) -> Value {
        let f = self.file();
        let mut classes: Vec<(String, u64)> = Vec::new();
        let mut per_storey: HashMap<Option<u64>, u64> = HashMap::new();
        for id in &self.elements {
            let c = cc_cad::js::to_js_string(&Self::pretty(&f, *id));
            match classes.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n = n.saturating_add(1),
                None => classes.push((c, 1)),
            }
            let s = self.storey_of(*id).map(id_key);
            let n = per_storey.entry(s).or_insert(0);
            *n = n.saturating_add(1);
        }
        classes.sort_by_key(|c| std::cmp::Reverse(c.1));
        let proj = f.of_type("IFCPROJECT", true).first().copied();
        let elev = |id: Id| match Self::attr_text(&f, id, "Elevation") {
            v @ Value::Number(_) => v,
            _ => Value::Null,
        };
        let mut storeys: Vec<Value> = self
            .storey_ids
            .iter()
            .map(|id| json!({"id": jnum(*id), "name": Self::attr_text(&f, *id, "Name"), "elevation": elev(*id), "elements": per_storey.get(&Some(id_key(*id))).copied().unwrap_or(0)}))
            .collect();
        storeys.sort_by(|a, b| {
            let e = |v: &Value| v["elevation"].as_f64().unwrap_or(0.0);
            e(a).partial_cmp(&e(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let app = f.of_type("IFCAPPLICATION", true).first().copied().map(|a| {
            let parts: Vec<String> = [Self::attr_text(&f, a, "ApplicationFullName"), Self::attr_text(&f, a, "Version")]
                .iter()
                .filter(|v| cc_cad::js::truthy(Some(v)))
                .map(cc_cad::js::to_js_string)
                .collect();
            if parts.is_empty() { Value::Null } else { Value::String(parts.join(" ")) }
        });
        let mut cls = Map::new();
        for (k, n) in classes {
            cls.insert(k, json!(n));
        }
        json!({
            "file": self.name, "schema": f.schema_name,
            "project": proj.map_or(Value::Null, |p| json!({"id": jnum(p), "name": Self::attr_text(&f, p, "Name"), "longName": Self::attr_text(&f, p, "LongName")})),
            "application": app.unwrap_or(Value::Null), "storeys": storeys, "classes": Value::Object(cls), "elements": self.elements.len(),
            "notInAStorey": per_storey.get(&None).copied().unwrap_or(0),
        })
    }

    /// A line flattened one level for the property code: a reference becomes the line it points at.
    fn line<'f, 'a>(f: &'f IfcFile<'a>, v: &AV) -> Option<Line<'f, 'a>> {
        match v {
            AV::Handle(n) => get_line(f, *n).ok().flatten(),
            _ => None,
        }
    }

    /// The first attribute of a quantity whose name ends in "Value" (not Formula), in schema order.
    fn quantity_value(q: &Line<'_, '_>, skip_formula: bool) -> Option<Option<Value>> {
        q.attrs().into_iter().find(|(k, _)| k.ends_with("Value") && (!skip_formula || k != "Formula")).map(|(_, v)| val(&v))
    }

    /// ifc.mjs `psets(id)`: {set: {name: value}} over the element's property sets and quantities.
    fn psets(&self, f: &IfcFile<'_>, id: Id) -> Map<String, Value> {
        let mut out = Map::new();
        let Ok(rels) = f.inverse("IFCRELDEFINESBYPROPERTIES", "RelatedObjects") else { return out };
        for r in rels.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]) {
            let def = f.attr(*r, "RelatingPropertyDefinition").ok().flatten();
            let ids: Vec<Id> = match def {
                Some(SV::List(l)) => l.iter().filter_map(SV::as_ref_id).filter(|x| *x != 0.0).collect(),
                Some(SV::Ref(n)) if *n != 0.0 => vec![*n],
                _ => Vec::new(),
            };
            for p in ids {
                let Some(ps) = get_line(f, p).ok().flatten() else { continue };
                let set_name = match ps.get("Name").as_ref().and_then(val).filter(|v| cc_cad::js::truthy(Some(v))) {
                    Some(v) => cc_cad::js::to_js_string(&v),
                    None => format!("#{}", cc_cad::js::num_to_string(p)),
                };
                let vals = out.entry(set_name).or_insert_with(|| Value::Object(Map::new()));
                let Some(vals) = vals.as_object_mut() else { continue };
                for item in ps.get("HasProperties").map(|v| if let AV::List(l) = v { l } else { Vec::new() }).unwrap_or_default() {
                    let Some(pl) = Self::line(f, &item) else { continue };
                    let Some(n) = val_opt(pl.get("Name").as_ref()).filter(|v| cc_cad::js::truthy(Some(v))) else { continue };
                    let n = cc_cad::js::to_js_string(&n);
                    let v = if pl.has("NominalValue") {
                        val_opt(pl.get("NominalValue").as_ref())
                    } else if pl.get("EnumerationValues").is_some_and(|v| v.truthy()) {
                        val_opt(pl.get("EnumerationValues").as_ref())
                    } else if pl.get("ListValues").is_some_and(|v| v.truthy()) {
                        val_opt(pl.get("ListValues").as_ref())
                    } else if pl.has("UpperBoundValue") || pl.has("LowerBoundValue") {
                        let mut b = Map::new();
                        if let Some(l) = val_opt(pl.get("LowerBoundValue").as_ref()) {
                            b.insert("lower".into(), l);
                        }
                        if let Some(u) = val_opt(pl.get("UpperBoundValue").as_ref()) {
                            b.insert("upper".into(), u);
                        }
                        Some(Value::Object(b))
                    } else {
                        Some(Value::Null)
                    };
                    match v {
                        Some(v) => {
                            vals.insert(n, v);
                        }
                        None => {
                            vals.remove(&n);
                        }
                    }
                }
                for item in ps.get("Quantities").map(|v| if let AV::List(l) = v { l } else { Vec::new() }).unwrap_or_default() {
                    let Some(ql) = Self::line(f, &item) else { continue };
                    let Some(n) = val_opt(ql.get("Name").as_ref()).filter(|v| cc_cad::js::truthy(Some(v))) else { continue };
                    let v = match Self::quantity_value(&ql, true) {
                        Some(v) => v,
                        None => Some(Value::Null),
                    };
                    if let Some(v) = v {
                        vals.insert(cc_cad::js::to_js_string(&n), v);
                    }
                }
            }
        }
        out
    }

    /// ifc.mjs `typeSets(ty)`: a type's own HasPropertySets.
    fn type_sets(&self, f: &IfcFile<'_>, ty: Id) -> Map<String, Value> {
        let mut out = Map::new();
        let Some(line) = get_line(f, ty).ok().flatten() else { return out };
        for item in line.get("HasPropertySets").map(|v| if let AV::List(l) = v { l } else { Vec::new() }).unwrap_or_default() {
            let Some(ps) = Self::line(f, &item) else { continue };
            let Some(name) = ps.get("Name").filter(AV::truthy) else { continue };
            let key = js_str(val(&name).as_ref());
            let mut vals = Map::new();
            for p in ps.get("HasProperties").map(|v| if let AV::List(l) = v { l } else { Vec::new() }).unwrap_or_default() {
                let Some(pl) = Self::line(f, &p) else { continue };
                let Some(pn) = pl.get("Name").filter(AV::truthy) else { continue };
                let v = if pl.has("NominalValue") { val_opt(pl.get("NominalValue").as_ref()) } else { Some(Value::Null) };
                let k = js_str(val(&pn).as_ref());
                match v {
                    Some(v) => {
                        vals.insert(k, v);
                    }
                    None => {
                        vals.remove(&k);
                    }
                }
            }
            for q in ps.get("Quantities").map(|v| if let AV::List(l) = v { l } else { Vec::new() }).unwrap_or_default() {
                let Some(ql) = Self::line(f, &q) else { continue };
                let Some(qn) = ql.get("Name").filter(AV::truthy) else { continue };
                let v = Self::quantity_value(&ql, false).unwrap_or(Some(Value::Null));
                if let Some(v) = v {
                    vals.insert(js_str(val(&qn).as_ref()), v);
                }
            }
            out.insert(key, Value::Object(vals));
        }
        out
    }

    /// ifc.mjs `properties(id)`.
    pub fn properties(&self, id: u64) -> Result<Value, String> {
        let f = self.file();
        #[allow(clippy::cast_precision_loss)]
        let id = id as f64;
        let Some(line) = get_line(&f, id).ok().flatten() else {
            return Err(format!("There is no line #{} in this model.", cc_cad::js::num_to_string(id)));
        };
        let mut attributes = Map::new();
        for (k, v) in line.attrs() {
            if let Some(x) = val(&v) {
                attributes.insert(k, x);
            }
        }
        let own = self.psets(&f, id);
        let ty = self.type_of.get(&id_key(id)).copied();
        let ts = ty.map(|t| self.type_sets(&f, t)).unwrap_or_default();
        let mut o = self.row(&f, id).as_object().cloned().unwrap_or_default();
        o.insert("attributes".into(), Value::Object(attributes));
        o.insert("propertySets".into(), Value::Object(own));
        o.insert("typePropertySets".into(), Value::Object(ts));
        Ok(Value::Object(o))
    }

    /// ifc.mjs `query({cls, storey, name, property, offset, limit})`.
    pub fn query(&self, a: &Value) -> Result<Value, String> {
        let f = self.file();
        let s = |k: &str| a.get(k).and_then(Value::as_str).filter(|x| !x.is_empty());
        let want = s("class").map(str::to_uppercase);
        let mut want_set: Option<HashSet<u64>> = None;
        if let Some(w) = &want {
            if !f.schema.has(w) && !self.elements.iter().any(|id| f.ty(*id) == Some(w.as_str())) {
                return Err(format!("No class {} in {}.", s("class").unwrap_or(""), f.schema_name));
            }
            want_set = Some(f.of_type(w, true).into_iter().map(id_key).collect());
        }
        let nm = s("name").map(str::to_lowercase);
        let st = s("storey").map(str::to_lowercase);
        let text = |v: Value| if cc_cad::js::truthy(Some(&v)) { cc_cad::js::to_js_string(&v) } else { String::new() };
        let mut ids: Vec<Id> = self
            .elements
            .iter()
            .copied()
            .filter(|id| {
                if want_set.as_ref().is_some_and(|w| !w.contains(&id_key(*id))) {
                    return false;
                }
                if nm.as_ref().is_some_and(|n| !text(Self::attr_text(&f, *id, "Name")).to_lowercase().contains(n.as_str())) {
                    return false;
                }
                if let Some(stn) = &st {
                    match self.storey_of(*id) {
                        Some(sid) if text(Self::attr_text(&f, sid, "Name")).to_lowercase() == *stn => {}
                        _ => return false,
                    }
                }
                true
            })
            .collect();
        if let Some(p) = a.get("property").filter(|p| p.get("name").and_then(Value::as_str).is_some_and(|n| !n.is_empty())) {
            let pn = p["name"].as_str().unwrap_or("").to_lowercase();
            let ps = p.get("set").and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_lowercase);
            let equals = p.get("equals");
            let contains = p.get("contains");
            let matches = |v: Option<&Value>| -> bool {
                if let Some(e) = equals {
                    let es = cc_cad::js::to_js_string(e).to_lowercase();
                    return js_str(v).to_lowercase() == es
                        || matches!(v, Some(Value::Number(n)) if n.as_f64() == Some(cc_cad::js::to_number(e)));
                }
                if let Some(c) = contains {
                    return js_str(v).to_lowercase().contains(&cc_cad::js::to_js_string(c).to_lowercase());
                }
                v.is_some()
            };
            ids.retain(|id| {
                let mut sets = self.psets(&f, *id);
                if let Some(t) = self.type_of.get(&id_key(*id)) {
                    for (k, v) in self.type_sets(&f, *t) {
                        let mut merged = v.as_object().cloned().unwrap_or_default();
                        if let Some(own) = sets.get(&k).and_then(Value::as_object) {
                            for (a, b) in own {
                                merged.insert(a.clone(), b.clone());
                            }
                        }
                        sets.insert(k, Value::Object(merged));
                    }
                }
                sets.iter().any(|(sn, vals)| {
                    if ps.as_ref().is_some_and(|x| sn.to_lowercase() != *x) {
                        return false;
                    }
                    vals.as_object().is_some_and(|m| m.iter().any(|(k, v)| k.to_lowercase() == pn && matches(Some(v))))
                })
            });
        }
        let offset = usize::try_from(a.get("offset").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(usize::MAX);
        let limit = usize::try_from(a.get("limit").and_then(Value::as_u64).unwrap_or(100)).unwrap_or(usize::MAX);
        let rows: Vec<Value> = ids.iter().skip(offset).take(limit).map(|id| self.row(&f, *id)).collect();
        Ok(json!({"total": ids.len(), "offset": a.get("offset").cloned().unwrap_or(json!(0)), "elements": rows}))
    }
}
