//! The rest of ifc-place.js `IfcPlacer` for copy / paste / swap: `info()` (the storeys by elevation), `psetsOf`,
//! `relTypeOf`, `placements()` (every placed library item with its library and equipment data), `placeLike` (another
//! placement of an existing one: copy, paste, duplicate), `swap` (or update: another item in the same place, the
//! occurrence written again as another class under the same express ID when the item maps to one) and
//! `elementsFrame` (the plan frame an elements clip is measured in).

use std::collections::HashMap;

use cc_io_ifc::model::{Id, id_key};
use serde_json::Value as J;

use super::{D2R, PSET_EQUIPMENT, PSET_LIBRARY, PSET_PLACEMENT, PSET_SOURCE, PlaceOpts, Placer, classes_for, get, label, or_null, text};
use crate::api::{JsErr, R};
use crate::draw::m;
use crate::draw::tr::{t, t0};
use crate::draw::writer::{Journal, undo_journal};
use crate::wapi::{JV, Line, P};

/// One property of a set (`propsOf`): its line and value.
#[derive(Clone, Debug)]
pub struct PropRow {
    pub id: Id,
    pub value: P,
}

/// One set on an object (`psetsOf(id)[name]`).
#[derive(Clone, Debug)]
pub struct PsetOn {
    pub id: Id,
    pub rel: Id,
    /// name → property, in the object's key order (a later property of the same name replaces the value in place)
    pub props: Vec<(String, PropRow)>,
}

impl PsetOn {
    pub fn get(&self, name: &str) -> Option<&PropRow> {
        self.props.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// A storey (`info().storeys`).
#[derive(Clone, Debug, PartialEq)]
pub struct StoreyInfo {
    pub id: Id,
    pub name: String,
    /// model mm
    pub elevation: f64,
}

/// A placed library item (`placements()` row).
#[derive(Clone, Debug)]
pub struct PlacementRow {
    pub id: Id,
    pub name: String,
    pub cls: String,
    pub storey: Option<Id>,
    pub type_id: Option<Id>,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub item_number: String,
    pub quantity: P,
    pub notes: String,
    pub library: String,
    pub uuid: String,
    /// `ItemVersion?.value ?? null`
    pub version: P,
    pub sha256: String,
    pub license: String,
    pub attribution: String,
    pub source_file: String,
    /// CadCraft_Equipment as name → value (object key order)
    pub equipment: Vec<(String, P)>,
}

impl PlacementRow {
    pub fn equip(&self, k: &str) -> Option<&P> {
        self.equipment.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }
}

/// What a swap did (`swap()` result).
#[derive(Clone, Debug)]
pub struct Swapped {
    pub id: Id,
    pub type_id: Id,
    pub type_created: bool,
    /// (from, to): readable class names, e.g. IfcElectricAppliance → IfcFurniture
    pub class_changed: Option<(String, String)>,
    pub class_differs: bool,
    pub journal: Journal,
}

/// The pset index: the objects in first-seen order, and object → [(pset, relation)].
type PsetIndex = (Vec<Id>, HashMap<u64, Vec<(Id, Id)>>);

/// The ordered-object insert of the JS (`out[k] = v`: a known key keeps its place).
fn put<V>(out: &mut Vec<(String, V)>, k: String, v: V) {
    if let Some(e) = out.iter_mut().find(|(x, _)| *x == k) {
        e.1 = v;
    } else {
        out.push((k, v));
    }
}

/// `val(v)` the way `x?.value || ''` reads it.
fn s_or_empty(v: Option<&PropRow>) -> String {
    match v.map(|r| &r.value) {
        Some(p) if p.truthy() => p.string(),
        _ => String::new(),
    }
}

/// ifc-place.js `norm(a)`.
fn norm(a: f64) -> f64 {
    let x = a % 360.0;
    if x < 0.0 { x + 360.0 } else { x }
}

impl Placer<'_> {
    /// `info()`: the storeys, by elevation (model mm).
    pub fn info(&mut self) -> R<Vec<StoreyInfo>> {
        let mut out = Vec::new();
        for id in self.w.ids("IFCBUILDINGSTOREY") {
            let s = self.w.must(id)?;
            let name = {
                let n = s.a("Name").val();
                let ln = s.a("LongName").val();
                if n.truthy() {
                    n.string()
                } else if ln.truthy() {
                    ln.string()
                } else {
                    t("place.storey.unnamed", &[("id", &crate::js::num_str(id))])
                }
            };
            let elevation = match s.a("ObjectPlacement").href() {
                Some(pl) if s.a("ObjectPlacement").truthy() => self.w.api.world(pl)?[14] * self.w.unit_mm,
                _ => {
                    let e = s.a("Elevation").val().number();
                    (if e != 0.0 && !e.is_nan() { e } else { 0.0 }) * self.w.unit_mm
                }
            };
            out.push(StoreyInfo { id, name, elevation });
        }
        out.sort_by(|a, b| crate::js::num_cmp(a.elevation - b.elevation, 0.0));
        Ok(out)
    }

    /// `propsOf(ps)`
    fn props_of(&self, ps: Id) -> R<Vec<(String, PropRow)>> {
        let l = self.w.must(ps)?;
        let mut out = Vec::new();
        for h in l.a("HasProperties").hrefs() {
            if let Some(p) = self.w.line(h)?
                && p.get("Name").is_some_and(JV::truthy)
            {
                put(&mut out, p.a("Name").val().string(), PropRow { id: h, value: p.a("NominalValue").val() });
            }
        }
        Ok(out)
    }

    /// The pset index (`_psetIndex`): object → [(pset, rel)] in relation order, and the objects in first-seen order.
    fn pset_index(&self) -> R<PsetIndex> {
        let mut order = Vec::new();
        let mut index: HashMap<u64, Vec<(Id, Id)>> = HashMap::new();
        for rid in self.w.ids("IFCRELDEFINESBYPROPERTIES") {
            let r = self.w.must(rid)?;
            let Some(ps) = r.a("RelatingPropertyDefinition").href().filter(|p| *p != 0.0) else { continue };
            for h in r.a("RelatedObjects").hrefs() {
                index
                    .entry(id_key(h))
                    .or_insert_with(|| {
                        order.push(h);
                        Vec::new()
                    })
                    .push((ps, rid));
            }
        }
        Ok((order, index))
    }

    fn psets_from(&self, list: &[(Id, Id)]) -> R<Vec<(String, PsetOn)>> {
        let mut out = Vec::new();
        for &(ps, rel) in list {
            let name = self.w.must(ps)?.a("Name").text_or_empty();
            let props = self.props_of(ps)?;
            put(&mut out, name, PsetOn { id: ps, rel, props });
        }
        Ok(out)
    }

    /// `psetsOf(id)`: name → { id, rel, props }.
    pub fn psets_of(&self, id: Id) -> R<Vec<(String, PsetOn)>> {
        let (_, index) = self.pset_index()?;
        self.psets_from(index.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]))
    }

    /// `relTypeOf()`: object → (relation, type).
    pub fn rel_type_of(&self) -> R<HashMap<u64, (Id, Option<Id>)>> {
        let mut m = HashMap::new();
        for rid in self.w.ids("IFCRELDEFINESBYTYPE") {
            let r = self.w.must(rid)?;
            let ty = r.a("RelatingType").href();
            for h in r.a("RelatedObjects").hrefs() {
                m.insert(id_key(h), (rid, ty));
            }
        }
        Ok(m)
    }

    /// `placements()`: every placed library item, by item number (natural order).
    pub fn placements(&mut self) -> R<Vec<PlacementRow>> {
        let rt = self.rel_type_of()?;
        let so = self.w.storey_of()?;
        let (order, index) = self.pset_index()?;
        let mut out = Vec::new();
        for id in order {
            let ps = self.psets_from(index.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]))?;
            let find = |n: &str| ps.iter().find(|(k, _)| k == n).map(|(_, v)| v);
            let Some(pl) = find(PSET_PLACEMENT) else { continue };
            let lib = find(PSET_LIBRARY);
            let lv = |k: &str| lib.and_then(|l| l.get(k));
            let eq = find(PSET_EQUIPMENT).map(|e| e.props.iter().map(|(k, v)| (k.clone(), v.value.clone())).collect()).unwrap_or_default();
            let line = self.w.must(id)?;
            let (mut x, mut y, mut rot) = (0.0, 0.0, 0.0);
            if line.a("ObjectPlacement").truthy()
                && let Some(lp) = line.a("ObjectPlacement").href()
            {
                let wm = self.w.api.world(lp)?;
                x = wm[12] * self.w.unit_mm;
                y = wm[13] * self.w.unit_mm;
                rot = m::round(m::atan2(wm[1], wm[0]) / D2R * 1000.0) / 1000.0;
            }
            let item_number = match pl.get("ItemNumber").map(|r| &r.value) {
                Some(v) if !matches!(v, P::Undef | P::Null) => v.string(),
                _ => String::new(),
            };
            let quantity = match pl.get("Quantity").map(|r| r.value.clone()) {
                Some(v) if !matches!(v, P::Undef | P::Null) => v,
                _ => P::Num(1.0),
            };
            let version = match lv("ItemVersion").map(|r| r.value.clone()) {
                Some(v) if !matches!(v, P::Undef) => v,
                _ => P::Null,
            };
            out.push(PlacementRow {
                id,
                name: line.a("Name").text_or_empty(),
                cls: line.class.to_string(),
                storey: so.get(&id_key(id)).copied(),
                type_id: rt.get(&id_key(id)).and_then(|r| r.1),
                x,
                y,
                rot,
                item_number,
                quantity,
                notes: s_or_empty(pl.get("Notes")),
                library: s_or_empty(lv("Library")),
                uuid: s_or_empty(lv("ItemUuid")),
                version,
                sha256: s_or_empty(lv("ItemSha256")),
                license: s_or_empty(lv("License")),
                attribution: s_or_empty(lv("Attribution")),
                source_file: s_or_empty(lv("SourceFile")),
                equipment: eq,
            });
        }
        out.sort_by(|a, b| crate::js::locale_cmp(&a.item_number, &b.item_number));
        Ok(out)
    }

    /// `storeyLine(id)`: the storey's line, or "Pick a storey first."
    fn storey_line(&self, id: Id) -> R<Line> {
        match self.w.line(id)? {
            Some(st) if st.class.to_ascii_lowercase().contains("buildingstorey") => Ok(st),
            _ => Err(JsErr(t0("place.err.pick_storey"))),
        }
    }

    /// `retype(v)`: a fresh typed value from one read back (null stays null).
    fn retype_v(&self, v: &JV) -> JV {
        match v {
            JV::T(tv) => match tv.name.as_deref() {
                Some(n) if !n.is_empty() => self.w.t(n, tv.v.clone()),
                _ => v.clone(),
            },
            other => other.clone(),
        }
    }

    /// `placeLike(srcId, opts)`: another placement of placement `src` (same type, class, name and library data).
    pub fn place_like(&mut self, src: Id, o: &PlaceOpts) -> R<(Id, Id, Journal)> {
        let ps = self.psets_of(src)?;
        let find = |n: &str| ps.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        if find(PSET_PLACEMENT).is_none() {
            return Err(JsErr(t0("place.err.not_placed")));
        }
        let type_id = self.rel_type_of()?.get(&id_key(src)).and_then(|r| r.1).filter(|v| *v != 0.0);
        let Some(type_id) = type_id else { return Err(JsErr(t0("place.err.no_type"))) };
        let src_line = self.w.must(src)?;
        self.w.begin();
        match self.place_like_in(&src_line, type_id, o, &find(PSET_LIBRARY), &find(PSET_EQUIPMENT), &find(PSET_SOURCE)) {
            Ok(id) => Ok((id, type_id, self.w.journal.take().unwrap_or_default())),
            Err(e) => {
                // nothing half-made stays behind
                if let Some(j) = self.w.journal.take() {
                    undo_journal(self.w.api, &j)?;
                }
                Err(e)
            }
        }
    }

    fn place_like_in(
        &mut self,
        src: &Line,
        type_id: Id,
        o: &PlaceOpts,
        lib: &Option<PsetOn>,
        eq: &Option<PsetOn>,
        source: &Option<PsetOn>,
    ) -> R<Id> {
        let st = self.storey_line(o.storey)?;
        let lp = self.local_placement(st.a("ObjectPlacement"), o)?;
        let shape = self.shape_for(type_id)?;
        let occ = src.up.to_string();
        let g = self.w.guid();
        let oh = self.w.owner_history();
        let mut args = vec![
            g,
            oh,
            src.a("Name").clone(),
            src.a("Description").clone(),
            src.a("ObjectType").clone(),
            JV::H(lp),
            shape.map_or(JV::Null, JV::H),
            src.a("Tag").clone(),
        ];
        for x in self.occ_extra(&occ) {
            args.push(if matches!(x, JV::Null) {
                src.a("CompositionType").clone()
            } else {
                match src.get("PredefinedType") {
                    Some(JV::Null) | None => x,
                    Some(v) => v.clone(),
                }
            });
        }
        let id = self.w.make(&occ, args)?;
        self.w.relate_type(id, type_id)?;
        self.w.contain(id, o.storey)?;
        let pl = self.placement_pset(&o.item_number, o.quantity, &o.notes)?;
        self.w.relate(id, pl)?;
        for set in [lib, eq, source].into_iter().flatten() {
            let from = self.w.must(set.id)?;
            let mut props = Vec::new();
            for h in from.a("HasProperties").hrefs() {
                let Some(p) = self.w.line(h)? else { continue };
                if !p.get("Name").is_some_and(JV::truthy) || p.get("NominalValue").is_none() {
                    continue;
                }
                let nv = self.retype_v(p.a("NominalValue"));
                let pid = self
                    .w
                    .make("IFCPROPERTYSINGLEVALUE", vec![p.a("Name").clone(), p.a("Description").clone(), nv, p.a("Unit").clone()])?;
                props.push(JV::H(pid));
            }
            let g = self.w.guid();
            let oh = self.w.owner_history();
            let psid = self.w.make("IFCPROPERTYSET", vec![g, oh, from.a("Name").clone(), from.a("Description").clone(), JV::L(props)])?;
            self.w.relate(id, psid)?;
        }
        Ok(id)
    }

    /// Take `id` out of a relation's list (`rest.length` kept, else the relation deleted; touched first).
    fn leave_rel(&mut self, rel: Id, id: Id, list: &str) -> R<()> {
        let mut l = self.w.touch(rel)?;
        let rest: Vec<JV> = match l.a(list) {
            JV::L(v) => v.iter().filter(|h| h.href() != Some(id)).cloned().collect(),
            _ => return Err(JsErr(format!("TypeError: {list}.filter is not a function"))),
        };
        if rest.is_empty() {
            self.w.api.delete(rel);
        } else {
            l.set(list, JV::L(rest));
            self.w.write(&mut l)?;
        }
        Ok(())
    }

    /// `swap(id, item, opts)` (also Update): new type and shape; placement, ItemNumber, Quantity and Notes stay.
    pub fn swap(&mut self, id: Id, item: &J, o: &PlaceOpts, keep_class: bool) -> R<Swapped> {
        self.w.begin();
        match self.swap_in(id, item, o, keep_class) {
            Ok(mut s) => {
                s.journal = self.w.journal.take().unwrap_or_default();
                Ok(s)
            }
            Err(e) => {
                // the editor records nothing that failed: what it wrote goes
                if let Some(j) = self.w.journal.take() {
                    undo_journal(self.w.api, &j)?;
                }
                Err(e)
            }
        }
    }

    fn swap_in(&mut self, id: Id, item: &J, o: &PlaceOpts, keep_class: bool) -> R<Swapped> {
        let (type_id, created) = self.type_for(item, o)?;
        let rt = self.rel_type_of()?.get(&id_key(id)).copied();
        if let Some((rel, ty)) = rt
            && ty != Some(type_id)
        {
            self.leave_rel(rel, id, "RelatedObjects")?;
        }
        if rt.is_none_or(|(_, ty)| ty != Some(type_id)) {
            self.w.relate_type(id, type_id)?;
        }
        let mut occ = self.w.touch(id)?;
        let shape = self.shape_for(type_id)?;
        occ.set("Representation", shape.map_or(JV::Null, JV::H));
        let name = item.as_object().and_then(|x| x.get("name"));
        occ.set("Name", label(&self.w, name));
        occ.set("ObjectType", label(&self.w, name));
        occ.set("Description", text(&self.w, or_null(get(item, "description"))));
        let (_, cls_occ, _) = classes_for(item, self.w.v4);
        let was = occ.class.to_ascii_uppercase();
        let mut class_changed = None;
        if !was.is_empty() && was != cls_occ && !keep_class {
            // raw: a typed IfcGloballyUniqueId is dropped
            let g = match occ.a("GlobalId") {
                JV::T(tv) => match &tv.v {
                    P::Str(s) => JV::Str(s.clone()),
                    other => JV::s(&other.string()),
                },
                other => other.clone(),
            };
            let tag = self.retype_v(occ.a("Tag"));
            let mut args = vec![
                g,
                occ.a("OwnerHistory").clone(),
                occ.a("Name").clone(),
                occ.a("Description").clone(),
                occ.a("ObjectType").clone(),
                occ.a("ObjectPlacement").clone(),
                occ.a("Representation").clone(),
                tag,
            ];
            args.extend(self.occ_extra(cls_occ));
            let mut e = self.w.api.create(cls_occ, args)?;
            e.id = id;
            self.w.api.write(&mut e)?;
            if self.w.must(id)?.up.to_ascii_uppercase() != cls_occ {
                return Err(JsErr(format!("Could not change the class of #{} to {cls_occ}.", crate::js::num_str(id))));
            }
            class_changed = Some((occ.class.to_string(), e.class.to_string()));
        } else {
            self.w.write(&mut occ)?;
        }
        // item psets: the relations now point at fresh sets; the placement set is left alone
        let ps = self.psets_of(id)?;
        for name in [PSET_LIBRARY, PSET_EQUIPMENT, PSET_SOURCE] {
            let Some(cur) = ps.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()) else { continue };
            self.leave_rel(cur.rel, id, "RelatedObjects")?;
        }
        self.add_item_psets(id, item, o)?;
        let class_differs = class_changed.is_none() && was != cls_occ;
        Ok(Swapped { id, type_id, type_created: created, class_changed, class_differs, journal: Journal::default() })
    }

    /// `elementsFrame(snap, { zBase })`: rot = the first top-level element's own X axis in plan (else the storey's,
    /// else 0); box = its meshes in those axes round the centre (x, y), z from `z_base` → (rot, x, y, box) | None.
    pub fn elements_frame(&mut self, els: &[FrameEl], storey: Option<Id>, z_base: f64, max_points: usize) -> R<Option<Frame>> {
        let turn = |w: &[f64; 16]| -> Option<f64> { if m::hypot(w[0], w[1]) > 1e-6 { Some(m::atan2(w[1], w[0]) / D2R) } else { None } };
        let mut rot = els.iter().find(|e| e.top).and_then(|e| e.world.as_ref()).and_then(turn);
        if rot.is_none()
            && let Some(st) = storey
            && let Some(sl) = self.w.line(st)?
            && sl.a("ObjectPlacement").truthy()
            && let Some(p) = sl.a("ObjectPlacement").href()
        {
            rot = turn(&self.w.api.world(p)?);
        }
        let rot = m::round(norm(rot.unwrap_or(0.0)) * 1e6) / 1e6;
        let um = self.w.unit_mm;
        let ops = cc_io_ifc::geo::bool_ops::ManifoldOps;
        let mut pts: Vec<f64> = Vec::new();
        {
            let Ok(mut geo) = cc_io_ifc::geo::geometry::Geometry::new(&self.w.api.f, &ops) else { return Ok(None) };
            for e in els {
                let c = e.class.to_ascii_lowercase();
                if c == "ifcopeningelement" || c == "ifcopeningstandardcase" || c == "ifcspace" {
                    continue;
                }
                let (pos, _) = crate::libitem::ifc::product_triangles(Some(&mut geo), e.id, um);
                if pts.len().saturating_add(pos.len()) > max_points.saturating_mul(3) {
                    return Ok(None);
                }
                pts.extend(pos.iter().map(|v| v * um));
            }
        }
        Ok(oriented_box(&pts, rot).map(|(x, y, mut b)| {
            b[2] -= z_base;
            b[5] -= z_base;
            Frame { rot, x, y, bx: b }
        }))
    }
}

/// What `elementsFrame` reads of a snapshot element.
#[derive(Clone, Debug)]
pub struct FrameEl {
    pub id: Id,
    pub class: String,
    pub top: bool,
    pub world: Option<[f64; 16]>,
}

/// `elementsFrame()` → { rot, x, y, box } (box: x0, y0, z0, x1, y1, z1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rot: f64,
    pub x: f64,
    pub y: f64,
    pub bx: [f64; 6],
}

/// `orientedBox(points, rotDeg)` → (x, y, [x0, y0, z0, x1, y1, z1]).
pub fn oriented_box(points: &[f64], rot_deg: f64) -> Option<(f64, f64, [f64; 6])> {
    if points.len() < 3 {
        return None;
    }
    let r = rot_deg * D2R;
    let (c, s) = (m::cos(r), m::sin(r));
    let (mut u0, mut u1, mut v0, mut v1, mut z0, mut z1) =
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for &[x, y, z] in points.as_chunks::<3>().0 {
        let u = x * c + y * s;
        let v = -x * s + y * c;
        if u < u0 {
            u0 = u;
        }
        if u > u1 {
            u1 = u;
        }
        if v < v0 {
            v0 = v;
        }
        if v > v1 {
            v1 = v;
        }
        if z < z0 {
            z0 = z;
        }
        if z > z1 {
            z1 = z;
        }
    }
    let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    Some((
        cu * c - cv * s,
        cu * s + cv * c,
        [u0 - cu, v0 - cv, z0, m::max(u1 - cu, u0 - cu + 1.0), m::max(v1 - cv, v0 - cv + 1.0), m::max(z1, z0)],
    ))
}
