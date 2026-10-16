//! The SURFACE family: SURFACE, PLANESURFACE, EXTRUDEDSURFACE, LOFTEDSURFACE, REVOLVEDSURFACE, SWEPTSURFACE and
//! NURBSURFACE (port of lib/dwg/surface-write.js + surface-layout.js). The ACIS part goes as for 3DSOLID
//! (w_solid.rs), then AcDbSurface (BS U isolines, BS V isolines) and the class's fields.
//! Spec gap (the whole class layout, found by experiment; see surface-layout.js): NURBSURFACE (R2013+) is saved down
//! to R2000–R2010 as a plain SURFACE with isolines 6/6, no revision GUID and an ACAD_XREC_ROUNDTRIP extension
//! dictionary entry; saved up again it becomes a NURBSURFACE with a new GUID.

use std::collections::{HashMap, HashSet};

use crate::obj;
use crate::util::unb64;
use crate::value::{Obj, V};
use crate::w_bits::{BitWriter, append_bits};
use crate::w_js::{arr, field, hex_key, js_string, nn, num, truthy};
use crate::w_objects::{Cx, Enc, Rec, begin, entity_common, finish};
use crate::w_solid::{Acis, with_guid_xdata, write_acis_part};
use crate::w_write::{Conv, W};

const SWEEP_OPTIONS: [(&str, &str); 17] = [
    ("draftAngle", "BD"),
    ("startDraftDistance", "BD"),
    ("endDraftDistance", "BD"),
    ("twistAngle", "BD"),
    ("scaleFactor", "BD"),
    ("alignAngle", "BD"),
    ("solid", "B"),
    ("alignment", "BS"),
    ("miterOption", "BS"),
    ("alignStart", "B"),
    ("bank", "B"),
    ("basePointSet", "B"),
    ("twistRefVector", "BD3"),
    ("sweepTransformComputed", "B"),
    ("pathTransformComputed", "B"),
    ("sweepEntityTransform", "M"),
    ("pathEntityTransform", "M"),
];
/// SURFACE_LAYOUTS[t]
pub fn layout(t: &str) -> Vec<(&'static str, &'static str)> {
    match t {
        "EXTRUDEDSURFACE" => {
            let mut v = SWEEP_OPTIONS.to_vec();
            v.extend([("sweepVector", "BD3"), ("matrix", "M"), ("profileEntity", "ENT")]);
            v
        }
        "SWEPTSURFACE" => {
            let mut v = SWEEP_OPTIONS.to_vec();
            v.extend([("sweepEntityMatrix", "M"), ("pathEntityMatrix", "M"), ("sweepEntity", "ENT"), ("pathEntity", "ENT")]);
            v
        }
        "REVOLVEDSURFACE" => vec![
            ("draftAngle", "BD"),
            ("startDraftDistance", "BD"),
            ("endDraftDistance", "BD"),
            ("twistAngle", "BD"),
            ("solid", "B"),
            ("closeToAxis", "B"),
            ("axisPoint", "BD3"),
            ("axisVector", "BD3"),
            ("revolveAngle", "BD"),
            ("startAngle", "BD"),
            ("matrix", "M"),
            ("revolveEntity", "ENT"),
        ],
        "LOFTEDSURFACE" => vec![
            ("matrix", "M"),
            ("crossSections", "ENT"),
            ("guides", "ENT"),
            ("pathEntity", "ENTB"),
            ("startDraftAngle", "BD"),
            ("endDraftAngle", "BD"),
            ("startDraftMagnitude", "BD"),
            ("endDraftMagnitude", "BD"),
            ("arcLengthParameterization", "B"),
            ("noTwist", "B"),
            ("alignDirection", "B"),
            ("simpleSurfaces", "B"),
            ("solid", "B"),
            ("closedSurfaces", "B"),
            ("ruledSurface", "B"),
            ("virtualGuide", "B"),
            ("normal", "BS"),
        ],
        "NURBSURFACE" => {
            vec![("nurbs170", "BS"), ("nurbs290", "B"), ("nurbs10", "BD3"), ("nurbs11", "BD3"), ("nurbs12", "BD3"), ("nurbs13", "BD3")]
        }
        _ => Vec::new(),
    }
}
fn is_surface(t: &str) -> bool {
    matches!(t, "SURFACE" | "PLANESURFACE" | "EXTRUDEDSURFACE" | "LOFTEDSURFACE" | "REVOLVEDSURFACE" | "SWEPTSURFACE" | "NURBSURFACE")
}
/// Defaults of a field the model does not give.
fn default_of(name: &str) -> Option<V> {
    Some(match name {
        "scaleFactor" => V::Num(1.0),
        "startDraftAngle" => V::Num(std::f64::consts::PI / 2.0),
        "noTwist" | "alignDirection" | "simpleSurfaces" => V::Bool(true),
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct SurfRec {
    pub acis: Option<Acis>,
    pub f: Obj,
}
#[derive(Default)]
pub struct SfState {
    downs: Vec<f64>,
    ups: Vec<(f64, Option<V>)>,
    down_n: u32,
    up_n: u32,
    embedded: u32,
}

fn write_field(w: &mut BitWriter, op: &str, v: Option<&V>, name: &str) {
    let d = default_of(name);
    let dn = num(d.as_ref(), 0.0);
    match op {
        "BD" => w.bd(num(v, dn)),
        "BS" => w.bs(num(v, dn)),
        "BL" => w.bl(num(v, 0.0)),
        "B" => w.b(truthy(nn(v).or(d.as_ref()))),
        "BD3" => w.p3n(num(field(v, "x"), 0.0), num(field(v, "y"), 0.0), num(field(v, "z"), 0.0)),
        "M" => {
            let m = v.and_then(V::as_arr).filter(|m| m.len() == 16 && m.iter().all(|x| matches!(x, V::Num(n) if n.is_finite())));
            match m {
                Some(m) => {
                    for x in m {
                        w.bd(num(Some(x), 0.0));
                    }
                }
                None => {
                    for (i, _) in (0..16).enumerate() {
                        w.bd(if i % 5 == 0 { 1.0 } else { 0.0 });
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn encode_surface(s: &SurfRec, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let mut w = BitWriter::obj_writer(cx.version, false);
    let t = r.t().to_string();
    let ty = cx.classes.class_num(&t);
    begin(&mut w, ty, r);
    entity_common(&mut w, r);
    write_acis_part(&mut w, r, s.acis.as_ref());
    w.bs(num(r.g("uIsolines"), 6.0));
    w.bs(num(r.g("vIsolines"), 6.0));
    for (name, op) in layout(&t) {
        if op != "ENT" && op != "ENTB" {
            write_field(&mut w, op, s.f.get(name), name);
            continue;
        }
        let ent = s.f.get(name);
        let raw = field(ent, "raw");
        if truthy(ent) && truthy(field(ent, "value")) && truthy(raw) && num(field(raw, "ver"), f64::NAN) == f64::from(cx.ver) {
            // unknown encoding (surface-read.js): the value and the rest of the entity's data as they came
            if op == "ENT" {
                w.bl(crate::w_js::to_number(field(ent, "value")));
            } else {
                w.b(true);
            }
            let bits = unb64(&js_string(field(raw, "data")));
            let n = crate::w_bits::js_bit_count(field(raw, "n"), bits.len().saturating_mul(8));
            append_bits(&mut w, &bits, n);
            break;
        }
        if op == "ENT" {
            w.bl(0.0);
        } else {
            w.b(false);
        }
    }
    finish(&mut w)
}

impl W<'_> {
    pub fn surf_convert(&mut self, e: &Obj, ty: &str, owner: f64) -> Option<Conv> {
        if !is_surface(ty) {
            return None;
        }
        let ver = self.ver;
        let mut t = ty.to_string();
        let down = t == "NURBSURFACE" && ver < 27;
        let up = t == "SURFACE" && ver >= 27 && matches!(e.get("savedDownFrom"), Some(V::Str(s)) if s == "NURBSURFACE");
        if down {
            t = "SURFACE".into();
        }
        if up {
            t = "NURBSURFACE".into();
        }
        let xd = field(e.get("xdata"), "ACAD_OBJECT_CHANGE_GUID").filter(|x| x.truthy());
        let rg = e.get("revisionGuid");
        let src = if down {
            let mut x = e.obj("xdata").cloned().unwrap_or_default();
            x.remove("ACAD_OBJECT_CHANGE_GUID");
            let mut s = e.clone();
            s.set("xdata", x);
            s.set("hasRevisionGuid", false);
            s.remove("revisionGuid");
            s
        } else if ver < 18
            && xd.is_none()
            && truthy(e.get("hasRevisionGuid"))
            && truthy(rg)
            && !{
                let g = js_string(rg);
                g.strip_prefix('{').unwrap_or(&g).starts_with("00000000-")
            }
        {
            with_guid_xdata(e, rg)
        } else {
            e.clone()
        };
        let Some(Conv::Recs(mut rs)) = self.sol_convert(&src, "BODY", owner) else { return None };
        let mut f = Obj::new();
        for (name, op) in layout(&t) {
            f.set_opt(name, e.get(name).cloned());
            let en = e.get(name);
            if (op == "ENT" || op == "ENTB") && truthy(en) && truthy(field(en, "value")) {
                let raw = field(en, "raw");
                if !(truthy(raw) && num(field(raw, "ver"), f64::NAN) == f64::from(ver)) {
                    self.surf.embedded = self.surf.embedded.saturating_add(1);
                }
            }
        }
        let r = rs.first_mut()?;
        let acis = match &r.enc {
            Enc::Acis(a) => a.acis.clone(),
            _ => None,
        };
        r.o.set("t", t.as_str());
        r.enc = Enc::Surface(Box::new(SurfRec { acis, f }));
        r.o.set_opt("uIsolines", if down { Some(V::Num(6.0)) } else { e.get("uIsolines").cloned() });
        r.o.set_opt("vIsolines", if down { Some(V::Num(6.0)) } else { e.get("vIsolines").cloned() });
        if up {
            let g = js_string(r.g("revisionGuid"));
            if !truthy(r.g("hasRevisionGuid")) || is_zero_guid(&g) {
                let ng = self.guids.guid_lower();
                r.o.set("revisionGuid", ng);
            }
            r.o.set("hasRevisionGuid", true);
            self.surf.up_n = self.surf.up_n.saturating_add(1);
            self.surf.ups.push((r.h(), e.get("extDict").cloned()));
        }
        if down {
            self.surf.down_n = self.surf.down_n.saturating_add(1);
            self.surf.downs.push(r.h());
        }
        Some(Conv::Recs(rs))
    }

    /// The NURBSURFACE round-trip entries.
    pub fn surf_finish(&mut self) {
        let roundtrip = || {
            V::Arr(vec![
                V::Arr(vec![V::Num(102.0), V::from("AcadNurbSurfaceData")]),
                V::Arr(vec![V::Num(90.0), V::Num(0.0)]),
                V::Arr(vec![V::Num(310.0), V::from("")]),
                V::Arr(vec![V::Num(102.0), V::from("AcadModlrDecomposedType")]),
                V::Arr(vec![V::Num(90.0), V::Num(1.0)]),
            ])
        };
        if self.surf.downs.is_empty() && self.surf.ups.is_empty() {
            return self.surf_notes();
        }
        // byH: the records as they are now (the last record of a handle wins); surface records by handle
        let mut by_h: HashMap<u64, usize> = HashMap::new();
        let mut surf_at: HashMap<u64, usize> = HashMap::new();
        for (i, r) in self.recs.iter().enumerate() {
            by_h.insert(r.h().to_bits(), i);
            if matches!(r.enc, Enc::Surface(_)) {
                surf_at.entry(r.h().to_bits()).or_insert(i);
            }
        }
        for rh in self.surf.downs.clone() {
            let Some(&ri) = surf_at.get(&rh.to_bits()) else { continue };
            let xdic = self.recs.get(ri).map(|r| num(r.g("xdic"), 0.0)).unwrap_or(0.0);
            let mut di = if xdic != 0.0 { by_h.get(&xdic.to_bits()).copied() } else { None };
            let xh = self.next();
            if di.is_none_or(|i| self.recs.get(i).is_none_or(|d| d.t() != "DICTIONARY")) {
                let dh = self.next();
                self.recs.push(Rec::new(obj! { "t" => "DICTIONARY", "h" => dh, "owner" => rh, "reactors" => vec![V::Num(rh)], "items" => Vec::<V>::new(), "hardOwner" => true, "cloning" => 1 }));
                di = Some(self.recs.len().saturating_sub(1));
                if let Some(r) = self.recs.get_mut(ri) {
                    r.o.set("xdic", dh);
                }
            }
            let Some(di) = di else { continue };
            let dh = self.recs.get(di).map_or(0.0, Rec::h);
            if let Some(d) = self.recs.get_mut(di) {
                let mut items: Vec<V> = arr(d.g("items"))
                    .iter()
                    .filter(|x| js_string(x.as_arr().and_then(|a| a.first())) != "ACAD_XREC_ROUNDTRIP")
                    .cloned()
                    .collect();
                items.push(V::Arr(vec![V::from("ACAD_XREC_ROUNDTRIP"), V::Num(xh)]));
                crate::w_nongraphic::sort_items(&mut items);
                d.o.set("items", items);
            }
            self.recs.push(Rec::new(
                obj! { "t" => "XRECORD", "h" => xh, "owner" => dh, "reactors" => vec![V::Num(dh)], "data" => roundtrip(), "cloning" => 1 },
            ));
        }
        // ups: the source's round-trip entry is stale once the surface is a NURBSURFACE again (the converter drops it).
        // The JS splices records as it goes; here the removals are collected and applied once (the same result).
        let model = self.model;
        let mut remove: HashSet<u64> = HashSet::new();
        let mut removed_dicts: HashSet<usize> = HashSet::new();
        let mut items_of: HashMap<usize, Vec<V>> = HashMap::new();
        let mut drop_xdic: Vec<usize> = Vec::new();
        let mut obj_by_h: HashMap<Option<String>, &V> = HashMap::new();
        for o in arr(model.get("objects")) {
            obj_by_h.entry(hex_key(field(Some(o), "handle"))).or_insert(o);
        }
        for (rh, ext) in self.surf.ups.clone() {
            let sd = obj_by_h.get(&hex_key(ext.as_ref())).copied();
            let xs = sd.and_then(|s| field(field(Some(s), "entries"), "ACAD_XREC_ROUNDTRIP")).filter(|x| x.truthy());
            let xh = xs.map_or(0.0, |x| self.src(Some(x)));
            let Some(&ri) = surf_at.get(&rh.to_bits()) else { continue };
            let xdic = self.recs.get(ri).map(|r| num(r.g("xdic"), 0.0)).unwrap_or(0.0);
            let di = if xdic != 0.0 { by_h.get(&xdic.to_bits()).copied() } else { None };
            if xh != 0.0 && by_h.contains_key(&xh.to_bits()) {
                remove.insert(xh.to_bits());
            }
            let Some(di) = di else { continue };
            if self.recs.get(di).is_none_or(|d| d.g("items").is_none()) {
                continue;
            }
            let cur = items_of.remove(&di).unwrap_or_else(|| arr(self.recs.get(di).and_then(|d| d.g("items"))).to_vec());
            let items: Vec<V> = cur
                .into_iter()
                .filter(|x| {
                    let a = x.as_arr().map(Vec::as_slice).unwrap_or(&[]);
                    js_string(a.first()) != "ACAD_XREC_ROUNDTRIP" && !(xh != 0.0 && num(a.get(1), f64::NAN) == xh)
                })
                .collect();
            if items.is_empty() {
                removed_dicts.insert(di);
                drop_xdic.push(ri);
            }
            items_of.insert(di, items);
        }
        for (di, items) in items_of {
            if let Some(d) = self.recs.get_mut(di) {
                d.o.set("items", items);
            }
        }
        for ri in drop_xdic {
            if let Some(r) = self.recs.get_mut(ri) {
                r.o.remove("xdic");
            }
        }
        if !remove.is_empty() || !removed_dicts.is_empty() {
            let mut seen: HashSet<u64> = HashSet::new();
            let mut i = 0usize;
            self.recs.retain(|r| {
                let k = r.h().to_bits();
                let drop = removed_dicts.contains(&i) || (remove.contains(&k) && seen.insert(k));
                i = i.saturating_add(1);
                !drop
            });
        }
        self.surf_notes();
    }
    fn surf_notes(&mut self) {
        let version = self.version;
        if self.surf.down_n > 0 {
            let n = self.surf.down_n;
            self.notes.push(crate::w_msg::msg("ccdwg.write.nurbs_down", &[("n", &n.to_string()), ("version", version)]));
        }
        if self.surf.up_n > 0 {
            let n = self.surf.up_n;
            self.notes.push(crate::w_msg::msg("ccdwg.write.nurbs_up", &[("n", &n.to_string())]));
        }
        if self.surf.embedded > 0 {
            let n = self.surf.embedded;
            self.notes.push(crate::w_msg::msg("ccdwg.write.embedded", &[("n", &n.to_string())]));
        }
    }
}
/// `/^\{?0{8}-0{4}-0{4}-0{4}-0{12}\}?$/`
fn is_zero_guid(s: &str) -> bool {
    let t = s.strip_prefix('{').unwrap_or(s);
    let t = t.strip_suffix('}').unwrap_or(t);
    t == "00000000-0000-0000-0000-000000000000"
}
