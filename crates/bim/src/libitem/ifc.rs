//! libitem-ifc.js — IFC types → library items: every IfcTypeProduct with representation maps, plus every occurrence
//! of a furnishing / proxy / distribution class with no type (grouped by ObjectType). Body: one occurrence's mesh
//! taken back into its own placement, in mm; plan: the type's FootPrint / Plan curves, else the mesh's convex hull;
//! params from the property sets (every property also kept as "Pset.Name" in source_attrs).

use std::collections::HashMap;
use std::sync::Arc;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use super::{LIMIT_TEXT, LIMIT_TRIANGLES, Read, convex_hull, n, pts_json, r4, s, suggest_category, validate_item};
use crate::js::{len16, num_str, slice16};
use crate::util::{apply4, inv4};
use crate::wapi::{Api, JV, Line, P};

const LOOSE: [&str; 10] = [
    "IFCFURNISHINGELEMENT",
    "IFCFURNITURE",
    "IFCBUILDINGELEMENTPROXY",
    "IFCFLOWTERMINAL",
    "IFCSANITARYTERMINAL",
    "IFCELECTRICAPPLIANCE",
    "IFCSYSTEMFURNITUREELEMENT",
    "IFCDISTRIBUTIONELEMENT",
    "IFCENERGYCONVERSIONDEVICE",
    "IFCFLOWCONTROLLER",
];

/// libitem-ifc.js `clip`: control characters as spaces, trimmed, at most n UTF-16 units.
fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().map(|c| if (c as u32) < 0x20 || c as u32 == 0x7f { ' ' } else { c }).collect();
    let t = crate::js::trim(&t);
    if len16(t) > n { slice16(t, n) } else { t.to_string() }
}
/// `val(v)` as text (`String(val(v) || '')` style: null → '')
fn vtext(v: &JV) -> String {
    let p = v.val();
    if p.truthy() { p.string() } else { String::new() }
}

/// One candidate (a type, or a group of occurrences with no type).
#[derive(Clone, Debug)]
pub struct Cand {
    pub key: String,
    pub type_id: Option<f64>,
    pub cls: String,
    pub name: String,
    pub occ: Vec<f64>,
}

/// What the Files… list shows of a candidate.
#[derive(Clone, Debug)]
pub struct Listed {
    pub key: String,
    pub name: String,
    pub has_body: bool,
    pub ifc_class: String,
    pub occurrences: usize,
}

fn line(api: &Api, id: f64) -> Option<Line> {
    api.line(id).ok().flatten()
}
fn hrefs(l: &Line, k: &str) -> Vec<f64> {
    l.a(k).hrefs()
}

/// `itemsFromIfc(api, mid, { list, keys })` on a file's bytes → (items, notes, candidates)
pub fn items_from_ifc(bytes: Arc<[u8]>, list: bool, keys: Option<&[String]>) -> Result<(Read, Vec<Listed>), String> {
    let mut api = Api::open(bytes.clone(), 0).map_err(|e| e.0)?;
    let mut notes: Vec<String> = Vec::new();
    let mm = api.length_unit_mm().map_err(|e| e.0)?;
    let mut type_of: HashMap<u64, f64> = HashMap::new();
    let mut occ_of_type: Vec<(f64, Vec<f64>)> = Vec::new();
    for rid in api.ids("IFCRELDEFINESBYTYPE", false) {
        let Some(r) = line(&api, rid) else { continue };
        let t = r.a("RelatingType").href().unwrap_or(f64::NAN);
        for h in hrefs(&r, "RelatedObjects") {
            type_of.insert(id_key(h), t);
            match occ_of_type.iter_mut().find(|(k, _)| k.to_bits() == t.to_bits()) {
                Some(e) => e.1.push(h),
                None => occ_of_type.push((t, vec![h])),
            }
        }
    }
    let mut cands: Vec<Cand> = Vec::new();
    let names: Vec<String> = api.f.ix.names.iter().map(|n| n.to_string()).collect();
    for name in names {
        if name.is_empty() || !name.ends_with("TYPE") || name.starts_with("IFCRELDEFINES") {
            continue;
        }
        let ids = api.ids(&name, false);
        if ids.is_empty() {
            continue;
        }
        for id in ids {
            let Some(l) = line(&api, id) else { continue };
            if hrefs(&l, "RepresentationMaps").is_empty() {
                continue;
            }
            let occ = occ_of_type.iter().find(|(k, _)| k.to_bits() == id.to_bits()).map(|(_, v)| v.clone()).unwrap_or_default();
            let nm = {
                let a = vtext(l.a("Name"));
                if !a.is_empty() {
                    a
                } else {
                    let b = vtext(l.a("ElementType"));
                    if b.is_empty() { name.clone() } else { b }
                }
            };
            cands.push(Cand { key: format!("type-{}", num_str(id)), type_id: Some(id), cls: l.class.to_string(), name: nm, occ });
        }
    }
    let mut groups: Vec<(String, String, String, Vec<f64>)> = Vec::new();
    for nn in LOOSE {
        for id in api.ids(nn, false) {
            if type_of.contains_key(&id_key(id)) {
                continue;
            }
            let Some(l) = line(&api, id) else { continue };
            if !l.a("Representation").truthy() {
                continue;
            }
            let label = {
                let ot = vtext(l.a("ObjectType"));
                if ot.is_empty() {
                    let nm = vtext(l.a("Name"));
                    let nm = if nm.is_empty() { nn.to_string() } else { nm };
                    // .replace(/:\d+$/, '')
                    match nm.rfind(':') {
                        Some(i)
                            if i.saturating_add(1) < nm.len()
                                && nm.get(i.saturating_add(1)..).is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit())) =>
                        {
                            nm.get(..i).unwrap_or("").to_string()
                        }
                        _ => nm,
                    }
                } else {
                    ot
                }
            };
            let cls = if l.class.is_empty() { nn.to_string() } else { l.class.to_string() };
            let k = format!("{cls}|{label}");
            match groups.iter_mut().find(|g| g.0 == k) {
                Some(g) => g.3.push(id),
                None => groups.push((k, cls, label, vec![id])),
            }
        }
    }
    for (g, (_, cls, name, occ)) in groups.into_iter().enumerate() {
        cands.push(Cand { key: format!("group-{}", g.saturating_add(1)), type_id: None, cls, name, occ: occ.clone() });
        if let Some(c) = cands.last_mut() {
            c.occ = occ;
        }
    }
    let listed: Vec<Listed> = cands
        .iter()
        .map(|c| Listed {
            key: c.key.clone(),
            name: clip(&c.name, 200),
            has_body: !c.occ.is_empty(),
            ifc_class: c.cls.clone(),
            occurrences: c.occ.len(),
        })
        .collect();
    if list {
        for c in &listed {
            if !c.has_body {
                notes.push(format!("{}: the type is not placed anywhere in this file, so its geometry cannot be read.", c.name));
            }
        }
        return Ok((Read { items: Vec::new(), notes }, listed));
    }
    // GetFlatMesh per occurrence: the geometry engine on its own copy of the file
    let gfile = cc_io_ifc::model::IfcFile::open(&bytes).ok();
    let ops = cc_io_ifc::geo::bool_ops::ManifoldOps;
    let mut geo = gfile.as_ref().and_then(|f| cc_io_ifc::geo::geometry::Geometry::new(f, &ops).ok());
    let mut items = Vec::new();
    for c in &cands {
        if let Some(k) = keys
            && !k.contains(&c.key)
        {
            continue;
        }
        match item_from(&mut api, geo.as_mut(), c, mm, &mut notes) {
            Ok(Some(mut it)) => {
                it.insert("_key".into(), s(&c.key));
                items.push(J::Object(it));
            }
            Ok(None) => {}
            Err(e) => notes.push(format!("{}: could not be read ({e}).", c.name)),
        }
    }
    Ok((Read { items, notes }, listed))
}

/// `productTriangles(api, mid, id, mm)`: a product's triangles in file units (the file's own coordinates).
pub(crate) fn product_triangles(geo: Option<&mut cc_io_ifc::geo::geometry::Geometry<'_>>, id: f64, unit_mm: f64) -> (Vec<f64>, Vec<f64>) {
    let mut pos = Vec::new();
    let mut idx = Vec::new();
    let Some(geo) = geo else { return (pos, idx) };
    let Ok(pieces) = geo.pieces(id) else { return (pos, idx) };
    let unit = geo.g.length_unit;
    let to_unit = 1000.0 / unit_mm;
    for pc in pieces {
        let Some(mesh) = geo.mesh(pc.geom) else { continue };
        let mw = cc_io_ifc::geo::math::mul(&cc_io_ifc::geo::math::scaling(unit, unit, unit), &pc.matrix);
        let base = pos.len() as f64 / 3.0;
        for c in mesh.pos.chunks(3) {
            if let [x, y, z] = c {
                // GetVertexArray hands out float32 vertices
                #[allow(clippy::cast_possible_truncation)]
                let f = |v: f64| f64::from(v as f32);
                let q = apply4(&mw, f(*x), f(*y), f(*z));
                pos.extend([q[0] * to_unit, q[1] * to_unit, q[2] * to_unit]);
            }
        }
        idx.extend(mesh.idx.iter().map(|k| base + f64::from(*k)));
    }
    (pos, idx)
}

/// `occurrenceClass(c)`
fn occurrence_class(cls: &str) -> String {
    let nn = cls.strip_suffix("Type").unwrap_or(cls);
    let ifc_name = |x: &str| x.strip_prefix("Ifc").is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_alphabetic()));
    if cls == "IfcFurnitureType" || nn == "IfcFurniture" {
        return "IfcFurniture".into();
    }
    if nn.contains("Furnishing") {
        return "IfcFurnishingElement".into();
    }
    for p in ["IfcSanitaryTerminal", "IfcElectricAppliance", "IfcFlowTerminal"] {
        if nn.starts_with(p) {
            return p.into();
        }
    }
    if ifc_name(nn) && nn.contains("Proxy") {
        "IfcBuildingElementProxy".into()
    } else if ifc_name(nn) {
        nn.to_string()
    } else {
        "IfcBuildingElementProxy".into()
    }
}

/// The type's FootPrint / Plan curves in mm (`footprint`).
fn footprint(api: &Api, type_id: f64, mm: f64) -> Vec<J> {
    let mut out = Vec::new();
    let Some(tl) = line(api, type_id) else { return out };
    for rm in hrefs(&tl, "RepresentationMaps") {
        let Some(rm) = line(api, rm) else { continue };
        let Some(rep) = rm.a("MappedRepresentation").href().and_then(|r| line(api, r)) else { continue };
        let ident = vtext(rep.a("RepresentationIdentifier")).to_lowercase();
        if ident != "footprint" && ident != "plan" {
            continue;
        }
        let mut curves: Vec<Line> = Vec::new();
        for it in hrefs(&rep, "Items") {
            let Some(item) = line(api, it) else { continue };
            if item.get("Elements").is_some_and(JV::truthy) {
                curves.extend(hrefs(&item, "Elements").into_iter().filter_map(|e| line(api, e)));
            } else {
                curves.push(item);
            }
        }
        for cv in curves.iter().take(10_000) {
            let num = |v: &JV| v.val().number();
            let pts: Option<Vec<Vec<f64>>> = match cv.a("Points") {
                JV::L(list) => Some(
                    list.iter()
                        .map(|p| match p.href().and_then(|h| line(api, h)) {
                            Some(pl) => match pl.a("Coordinates") {
                                JV::L(cs) => cs.iter().map(num).collect(),
                                _ => Vec::new(),
                            },
                            None => Vec::new(),
                        })
                        .collect(),
                ),
                JV::H(h) if !cv.get("Segments").is_some_and(JV::truthy) => line(api, *h).and_then(|pl| match pl.a("CoordList") {
                    JV::L(cl) => Some(
                        cl.iter()
                            .map(|c| match c {
                                JV::L(xs) => xs.iter().map(num).collect(),
                                _ => Vec::new(),
                            })
                            .collect(),
                    ),
                    _ => None,
                }),
                _ => None,
            };
            let Some(pts) = pts.filter(|p| p.len() >= 2) else { continue };
            let p: Vec<[f64; 2]> = pts
                .iter()
                .map(|q| [r4(q.first().copied().unwrap_or(f64::NAN) * mm), r4(q.get(1).copied().unwrap_or(f64::NAN) * mm)])
                .collect();
            let closed =
                p.len() > 2 && p.first().map(|a| a[0]) == p.last().map(|a| a[0]) && p.first().map(|a| a[1]) == p.last().map(|a| a[1]);
            let pp = if closed { p.get(..p.len().saturating_sub(1)).unwrap_or(&[]) } else { p.as_slice() };
            out.push(serde_json::json!({ "type": "poly", "layer": "EQUIP", "color": null, "pts": pts_json(pp), "closed": closed }));
        }
    }
    out
}

fn suggest_furniture(name: &str) -> &'static str {
    let t = name.to_lowercase();
    let any = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
    if any(&["chair", "seat", "sofa", "bench", "stool"]) {
        "furniture/seating"
    } else if any(&["desk"]) {
        "furniture/desks"
    } else if any(&["bed"]) {
        "furniture/beds"
    } else if any(&["table"]) {
        "furniture/tables"
    } else if any(&["cabinet", "shelf", "storage", "wardrobe", "dresser", "bookcase"]) {
        "furniture/storage"
    } else {
        "furniture/other"
    }
}

/// A property's value and its IFC type name.
struct Prop {
    v: P,
    ty: String,
}

fn item_from(
    api: &mut Api,
    geo: Option<&mut cc_io_ifc::geo::geometry::Geometry<'_>>,
    c: &Cand,
    mm: f64,
    notes: &mut Vec<String>,
) -> Result<Option<Map<String, J>>, String> {
    let name = {
        let x = clip(&c.name, LIMIT_TEXT);
        if x.is_empty() { "IFC item".to_string() } else { x }
    };
    let mut it = Map::new();
    it.insert("format".into(), s(super::FORMAT));
    it.insert("format_version".into(), n(1.0));
    it.insert("name".into(), s(&name));
    it.insert("kind".into(), s("equipment"));
    it.insert("category".into(), s("other/other"));
    let desc =
        format!("IFC {} {}.", if c.type_id.is_some() { "type" } else { "element group" }, c.cls.strip_prefix("Ifc").unwrap_or(&c.cls));
    it.insert("description".into(), s(&clip(&desc, LIMIT_TEXT)));
    it.insert("maker".into(), s(""));
    it.insert("model".into(), s(""));
    it.insert("ifc".into(), serde_json::json!({ "class": occurrence_class(&c.cls), "predefined": "NOTDEFINED" }));
    let mut plan_entities: Vec<J> = Vec::new();
    let mut body = J::Null;
    let mut params = Map::new();
    if let Some(&occ) = c.occ.first() {
        let ol = line(api, occ);
        let wm = match ol.as_ref().and_then(|l| l.a("ObjectPlacement").href()) {
            Some(p) => Some(api.world(p).map_err(|e| e.0)?),
            None => None,
        };
        let wi = match wm {
            Some(w) => Some(inv4(&w).ok_or_else(|| "Cannot read properties of null (reading '0')".to_string())?),
            None => None,
        };
        let (pos, idx) = product_triangles(geo, occ, mm);
        let nt = idx.len() as f64 / 3.0;
        if nt > LIMIT_TRIANGLES as f64 {
            notes.push(format!("{}: {} triangles, over the limit; no body.", c.name, num_str(nt)));
        } else if !idx.is_empty() {
            let mut out: Vec<f64> = Vec::with_capacity(pos.len());
            for q in pos.chunks(3) {
                if let [x, y, z] = q {
                    let p = match &wi {
                        Some(wi) => apply4(wi, *x, *y, *z),
                        None => [*x, *y, *z],
                    };
                    out.extend([r4(p[0] * mm), r4(p[1] * mm), r4(p[2] * mm)]);
                }
            }
            let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for q in out.chunks(3) {
                for (k, v) in q.iter().enumerate() {
                    if let Some(lo) = b.get_mut(k) {
                        *lo = crate::draw::m::min(*lo, *v);
                    }
                    if let Some(hi) = b.get_mut(k.saturating_add(3)) {
                        *hi = crate::draw::m::max(*hi, *v);
                    }
                }
            }
            body = serde_json::json!({ "type": "mesh", "positions": out.iter().map(|x| n(*x)).collect::<Vec<_>>(), "indices": idx.iter().map(|x| n(*x)).collect::<Vec<_>>() });
            params.insert("width".into(), n(r4(b[3] - b[0])));
            params.insert("depth".into(), n(r4(b[4] - b[1])));
            params.insert("height".into(), n(r4(b[5] - b[2])));
            let fp = c.type_id.map(|t| footprint(api, t, mm)).unwrap_or_default();
            if !fp.is_empty() {
                plan_entities = fp;
            } else {
                let xy: Vec<[f64; 2]> =
                    out.chunks(3).map(|q| [q.first().copied().unwrap_or(0.0), q.get(1).copied().unwrap_or(0.0)]).collect();
                let hull = convex_hull(&xy);
                if hull.len() >= 3 {
                    plan_entities.push(
                        serde_json::json!({ "type": "poly", "layer": "EQUIP", "color": null, "pts": pts_json(&hull), "closed": true }),
                    );
                }
            }
        }
    } else {
        notes.push(format!("{}: the type is not placed anywhere in this file, so its geometry cannot be read; no body.", c.name));
    }
    it.insert("plan".into(), serde_json::json!({ "layers": [{ "name": "EQUIP", "color": 7 }], "entities": plan_entities }));
    it.insert("body".into(), body);
    it.insert("clearances".into(), J::Array(Vec::new()));
    // properties: the type's sets, else the first occurrence's
    let mut props: Vec<(String, Prop)> = Vec::new();
    let take = |api: &Api, pset: f64, props: &mut Vec<(String, Prop)>| {
        let Some(ps) = line(api, pset) else { return };
        let set_name = {
            let x = vtext(ps.a("Name"));
            if x.is_empty() { "Properties".to_string() } else { x }
        };
        for h in hrefs(&ps, "HasProperties") {
            let Some(p) = line(api, h) else { continue };
            let Some(nv) = p.get("NominalValue").filter(|v| !matches!(v, JV::Null)) else { continue };
            let v = nv.val();
            if matches!(v, P::Null | P::Undef) || v.as_str().is_some_and(|x| x.is_empty()) {
                continue;
            }
            let key = format!("{set_name}.{}", p.a("Name").val().string());
            let pr = Prop { v, ty: nv.tname().unwrap_or("").to_string() };
            match props.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = pr,
                None => props.push((key, pr)),
            }
        }
    };
    if let Some(t) = c.type_id {
        if let Some(tl) = line(api, t) {
            for s0 in hrefs(&tl, "HasPropertySets") {
                take(api, s0, &mut props);
            }
        }
    } else if let Some(&occ) = c.occ.first() {
        for rid in api.ids("IFCRELDEFINESBYPROPERTIES", false) {
            let Some(r) = line(api, rid) else { continue };
            if hrefs(&r, "RelatedObjects").iter().any(|h| h.to_bits() == occ.to_bits())
                && let Some(d) = r.a("RelatingPropertyDefinition").href()
            {
                take(api, d, &mut props);
            }
        }
    }
    let mut sa = Map::new();
    for (k, p) in &props {
        let sv = match &p.v {
            P::Bool(b) => (if *b { "Yes" } else { "No" }).to_string(),
            v => v.string(),
        };
        if len16(&sv) <= LIMIT_TEXT {
            sa.insert(slice16(k, 255), s(&sv));
        }
    }
    let get = |names: &[&str]| -> Option<&Prop> {
        for nn in names {
            let suf = format!(".{}", nn.to_lowercase());
            if let Some((_, p)) = props.iter().find(|(k, _)| k.to_lowercase().ends_with(&suf)) {
                return Some(p);
            }
        }
        None
    };
    let sp = |p: Option<&Prop>| p.map_or(String::new(), |p| clip(&p.v.string(), LIMIT_TEXT));
    it.insert("maker".into(), s(&sp(get(&["Manufacturer", "Maker"]))));
    it.insert("model".into(), s(&sp(get(&["ModelReference", "ModelLabel", "Model"]))));
    for (k, names) in [
        ("width", &["Width", "NominalWidth", "NominalLength"][..]),
        ("depth", &["Depth", "NominalDepth"][..]),
        ("height", &["Height", "NominalHeight"][..]),
    ] {
        if let Some(p) = get(names)
            && let P::Num(v) = p.v
            && v > 0.0
            && !params.get(k).is_some_and(|x| x.as_f64().is_some_and(|x| x != 0.0))
        {
            let lengthy = p.ty.contains("LENGTH") || p.ty.is_empty();
            params.insert(k.into(), n(r4(if lengthy { v * mm } else { v })));
        }
    }
    let mut elec = Map::new();
    for (k, names) in [
        ("volts", &["RatedVoltage", "Volts"][..]),
        ("amps", &["RatedCurrent", "Amps"][..]),
        ("kw", &["RatedPower", "Kw"][..]),
        ("phase", &["NumberOfPoles", "Phase"][..]),
        ("hz", &["NominalFrequencyRange", "Hz"][..]),
    ] {
        if let Some(p) = get(names) {
            elec.insert(k.into(), s(&sp(Some(p))));
        }
    }
    if !elec.is_empty() {
        params.insert("elec".into(), J::Object(elec));
    }
    if let Some(p) = get(&["Remarks", "Notes"]) {
        params.insert("remarks".into(), s(&sp(Some(p))));
    }
    it.insert("params".into(), J::Object(params));
    it.insert("source_attrs".into(), J::Object(sa));
    let up = c.cls.to_uppercase();
    if up.contains("FURNITURE") || up.contains("FURNISHING") || up.contains("SYSTEMFURNITURE") {
        it.insert("kind".into(), s("furniture"));
        it.insert("category".into(), s(suggest_furniture(&name)));
    } else if up.contains("SANITARY") || up.contains("FLOWTERMINAL") {
        it.insert("kind".into(), s("fixture"));
        it.insert("category".into(), s("plumbing/fixtures"));
    } else if let Some(sg) = suggest_category(&name) {
        it.insert("category".into(), s(sg));
    }
    let cat = it.get("category").and_then(J::as_str).unwrap_or("").to_string();
    notes.push(format!("{name}: category {cat} set from the IFC class and name; check it."));
    let (ok, errors) = validate_item(&J::Object(it.clone()), true);
    if !ok {
        notes.push(format!("{name}: {}", errors.iter().take(3).cloned().collect::<Vec<_>>().join("; ")));
        return Ok(None);
    }
    Ok(Some(it))
}
