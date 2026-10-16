//! REGION / 3DSOLID / BODY with their ACIS data, and LIGHT (port of lib/dwg/solid-write.js; ODA spec 20.4.41,
//! 20.4.1/20.4.2, ch. 24 data storage, ch. 10). The ACIS data is written as the model holds it: SAT as version 1
//! (4096-byte blocks, 0x9F − c substitution) and SAB as version 2 inline up to R2010; from R2013 both go to the data
//! storage section (the entity's data-storage flag set; SAT as its text).
//! Spec gap (24.2.2.3 speaks of SAB only): SAT text inline in an R2013+ entity is dropped by the ODA File Converter; the
//! SAT text as the data storage record is read.

use crate::obj;
use crate::util::{unb64, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::BitWriter;
use crate::w_js::{arr, field, int32, js_string, js_trim, nn, num, parse_int16, str_or, to_number, truthy};
use crate::w_objects::{Cx, Enc, Rec, SOFT_OWN, begin, entity_common, finish, nnum};
use crate::w_write::{Conv, W, aci_from_rgb};

pub const GUID0: &str = "{00000000-0000-0000-0000-000000000000}";

/// The model's ACIS data (acis.js normAcis): SAT text (as latin-1 bytes of its char codes) or SAB bytes.
#[derive(Clone, Debug)]
pub enum Acis {
    Sat(Vec<u8>),
    Sab(Vec<u8>),
}
#[derive(Clone, Debug)]
pub struct AcisRec {
    pub acis: Option<Acis>,
}
#[derive(Default)]
pub struct SState {
    acds: Vec<(f64, Vec<u8>)>,
    recs: Vec<(f64, Option<V>)>,
    empty: u32,
    sat_in_r2013: u32,
    sab_in_r2000: u32,
}

fn latin1(s: &str) -> Vec<u8> {
    s.encode_utf16().map(|u| crate::util::u8_of(u32::from(u))).collect()
}
/// acis.js normAcis
pub fn norm_acis(a: Option<&V>) -> Option<Acis> {
    let a = a.filter(|x| x.truthy())?;
    if let V::Str(s) = a {
        return if js_trim(s).is_empty() { None } else { Some(Acis::Sat(latin1(s))) };
    }
    if matches!(field(Some(a), "format"), Some(V::Str(f)) if f == "sab") {
        let d = field(Some(a), "data");
        let bytes = match d {
            Some(V::Bytes(b)) => b.clone(),
            Some(x) if x.truthy() => unb64(&js_string(Some(x))),
            _ => Vec::new(),
        };
        return if bytes.is_empty() { None } else { Some(Acis::Sab(bytes)) };
    }
    let text = str_or(field(Some(a), "data"), "");
    if js_trim(&text).is_empty() { None } else { Some(Acis::Sat(latin1(&text))) }
}

/// '{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}' → (d1, d2, d3, d4) (zeros when malformed)
pub fn parse_guid(s: Option<&V>) -> (f64, f64, f64, [u8; 8]) {
    let s = if truthy(s) { js_string(s) } else { String::new() };
    let t = s.strip_prefix('{').unwrap_or(&s);
    let t = t.strip_suffix('}').unwrap_or(t);
    let parts: Vec<&str> = t.split('-').collect();
    let lens = [8usize, 4, 4, 4, 12];
    let ok = parts.len() == 5 && parts.iter().zip(lens).all(|(p, l)| p.len() == l && p.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut d4 = [0u8; 8];
    if !ok {
        return (0.0, 0.0, 0.0, d4);
    }
    let p = |i: usize| parts.get(i).copied().unwrap_or("");
    let tail = format!("{}{}", p(3), p(4));
    for (i, x) in d4.iter_mut().enumerate() {
        *x = crate::util::u8_of(crate::util::to_u32(parse_int16(
            tail.get(i.saturating_mul(2)..i.saturating_mul(2).saturating_add(2)).unwrap_or("0"),
        )));
    }
    (f64::from(crate::util::to_u32(parse_int16(p(0)))), parse_int16(p(1)), parse_int16(p(2)), d4)
}
fn guid_items(g: &(f64, f64, f64, [u8; 8])) -> V {
    let sx = |v: f64| f64::from(crate::util::to_i32(v) << 16 >> 16);
    let mut items = vec![
        V::Arr(vec![V::Num(1070.0), V::Num(1.0)]),
        V::Arr(vec![V::Num(1071.0), V::Num(f64::from(crate::util::to_i32(g.0)))]),
        V::Arr(vec![V::Num(1070.0), V::Num(sx(g.1))]),
        V::Arr(vec![V::Num(1070.0), V::Num(sx(g.2))]),
    ];
    for b in g.3 {
        items.push(V::Arr(vec![V::Num(1070.0), V::Num(f64::from(b))]));
    }
    V::Arr(items)
}
/// `{...e, xdata: {...(e.xdata || {}), ACAD_OBJECT_CHANGE_GUID: items}}`
pub fn with_guid_xdata(e: &Obj, rg: Option<&V>) -> Obj {
    let g = parse_guid(rg);
    let mut x = e.obj("xdata").cloned().unwrap_or_default();
    x.set("ACAD_OBJECT_CHANGE_GUID", guid_items(&g));
    let mut s = e.clone();
    s.set("xdata", x);
    s
}

/// One inline ACIS block (20.4.41): B empty, B unknown, BS version, data.
pub fn write_acis_block(w: &mut BitWriter, a: Option<&Acis>) {
    let Some(a) = a else {
        w.b(true);
        return;
    };
    w.b(false);
    match a {
        Acis::Sat(text) => {
            // version 1: the text in blocks of at most 4096 bytes, then a 0 size
            w.b(true);
            w.bs(1.0);
            let b: Vec<u8> = text.iter().map(|&c| if (0x21..=0x7E).contains(&c) { 0x9Fu8.wrapping_sub(c) } else { c }).collect();
            for part in b.chunks(4096) {
                w.bl(usize_to_f64(part.len()));
                for &x in part {
                    w.rc8(x);
                }
            }
            w.bl(0.0);
        }
        Acis::Sab(bytes) => {
            w.b(false);
            w.bs(2.0);
            for &x in bytes {
                w.rc8(x);
            }
        }
    }
}
fn write_wires(w: &mut BitWriter, list: &[V]) {
    w.bl(usize_to_f64(list.len()));
    for x in list {
        let x = Some(x);
        w.rc(f64::from(int32(field(x, "type"))));
        w.bl(f64::from(int32(field(x, "selectionMarker"))));
        w.bs(f64::from(int32(field(x, "color"))));
        w.bl(f64::from(int32(field(x, "acisIndex"))));
        let pts = arr(field(x, "points"));
        w.bl(usize_to_f64(pts.len()));
        for p in pts {
            w.p3(Some(p));
        }
        let t = field(x, "transform").filter(|t| t.truthy());
        w.b(t.is_some());
        if let Some(t) = t {
            let t = Some(t);
            w.p3(field(t, "xAxis"));
            w.p3(field(t, "yAxis"));
            w.p3(field(t, "zAxis"));
            w.p3(field(t, "translation"));
            w.bd(num(field(t, "scale"), 1.0));
            w.b(truthy(field(t, "hasRotation")));
            w.b(truthy(field(t, "hasReflection")));
            w.b(truthy(field(t, "hasShear")));
        }
    }
}
/// Wireframe present B, then point, isolines, wires, silhouettes and the "ACIS Empty bit" (1).
pub fn write_wireframe(w: &mut BitWriter, r: &Rec) {
    let Some(wf) = r.g("wireframe").filter(|x| x.truthy()) else {
        w.b(false);
        return;
    };
    let wf = Some(wf);
    w.b(true);
    let p = field(wf, "point").filter(|x| x.truthy());
    w.b(p.is_some());
    if p.is_some() {
        w.p3(p);
    }
    w.bl(nnum(field(wf, "isolines"), 4.0));
    let wires = arr(field(wf, "wires"));
    let present = match nn(field(wf, "isolinesPresent")) {
        Some(x) => x.truthy(),
        None => !wires.is_empty(),
    };
    w.b(present);
    if present {
        write_wires(w, wires);
    }
    let sils = arr(r.g("silhouettes"));
    w.bl(usize_to_f64(sils.len()));
    for s in sils {
        let s = Some(s);
        let vid = field(s, "viewportId");
        let vn = if truthy(vid) { to_number(vid) } else { 0.0 };
        if w.r2010 {
            w.bll(vn);
        } else {
            w.bl(vn);
        }
        w.p3(field(s, "target"));
        w.p3(field(s, "direction"));
        w.p3(field(s, "up"));
        w.b(truthy(field(s, "perspective")));
        let sw = arr(field(s, "wires"));
        let sp = match nn(field(s, "wiresPresent")) {
            Some(x) => x.truthy(),
            None => !sw.is_empty(),
        };
        w.b(sp);
        if sp {
            write_wires(w, sw);
        }
    }
    w.b(true);
}
/// The ACIS part after the common entity data (REGION/3DSOLID/BODY and the SURFACE family).
pub fn write_acis_part(w: &mut BitWriter, r: &Rec, acis: Option<&Acis>) {
    if !(w.r2013 && truthy(r.g("ds"))) {
        write_acis_block(w, acis);
    }
    write_wireframe(w, r);
    if w.r2007 {
        w.bl(0.0);
    }
    if w.r2013 {
        let g = parse_guid(r.g("revisionGuid"));
        w.b(truthy(r.g("hasRevisionGuid")));
        w.bl(g.0);
        w.bs(g.1);
        w.bs(g.2);
        for x in g.3 {
            w.rc8(x);
        }
        w.bl(0.0);
    }
}

pub fn encode_acis_entity(a: &AcisRec, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = match r.t() {
        "REGION" => 37.0,
        "3DSOLID" => 38.0,
        _ => 39.0,
    };
    begin(&mut w, ty, r);
    entity_common(&mut w, r);
    write_acis_part(&mut w, r, a.acis.as_ref());
    if w.r2007 && r.t() == "3DSOLID" {
        w.hv(SOFT_OWN, r.g("history"));
    }
    finish(&mut w)
}

pub fn encode_light(r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = cx.classes.class_num("LIGHT");
    begin(&mut w, ty, r);
    entity_common(&mut w, r);
    let g = |k: &str| r.g(k);
    w.bl(nnum(g("lightVersion"), 1.0));
    w.tv(&str_or(g("name"), ""));
    w.bs(nnum(g("lightType"), 2.0));
    w.b(truthy(g("status")));
    let lc = g("lightColor").filter(|x| x.truthy()).cloned().unwrap_or_else(|| V::Obj(obj! { "index" => 7 }));
    w.cmc(Some(&lc));
    w.b(truthy(g("plotGlyph")));
    w.bd(num(g("intensity"), 1.0));
    w.p3(g("position"));
    w.p3(g("target"));
    w.bs(f64::from(int32(g("attenuationType"))));
    w.b(truthy(g("useAttenuationLimits")));
    w.bd(num(g("attenuationStart"), 0.0));
    w.bd(num(g("attenuationEnd"), 10.0));
    w.bd(num(g("hotspot"), std::f64::consts::PI / 4.0));
    w.bd(num(g("falloff"), std::f64::consts::PI / 4.0 + 0.087_266_462_6));
    w.b(truthy(g("castShadows")));
    w.bs(f64::from(int32(g("shadowType"))));
    w.bs(nnum(g("shadowMapSize"), 256.0));
    w.rc(nnum(g("shadowMapSoftness"), 1.0));
    finish(&mut w)
}

fn p3s(p: Option<&V>) -> V {
    crate::value::p3(num(field(p, "x"), 0.0), num(field(p, "y"), 0.0), num(field(p, "z"), 0.0))
}

impl W<'_> {
    /// S.convert(e, type, owner, c): the common part is always taken from `e` (surface-write.js passes its own)
    pub fn sol_convert(&mut self, e: &Obj, ty: &str, owner: f64) -> Option<Conv> {
        let ver = self.ver;
        if ty == "LIGHT" {
            let mut lc = e.get("lightColor").filter(|x| x.truthy()).cloned().unwrap_or_else(|| V::Obj(obj! { "index" => 7 }));
            if ver < 18 && field(Some(&lc), "index").is_none() && field(Some(&lc), "rgb").is_some() {
                lc = V::Obj(obj! { "index" => aci_from_rgb(to_number(field(Some(&lc), "rgb"))) });
            }
            let mut r = self.common(e, "LIGHT", owner);
            r.enc = Enc::Light;
            let o = &mut r.o;
            o.set_opt("lightVersion", e.get("lightVersion").cloned());
            o.set("name", str_or(e.get("name"), ""));
            o.set("lightType", nn(e.get("lightType")).cloned().unwrap_or(V::Num(2.0)));
            o.set("status", !matches!(e.get("status"), Some(V::Bool(false))));
            o.set("lightColor", lc);
            o.set("plotGlyph", truthy(e.get("plotGlyph")));
            o.set_opt("intensity", e.get("intensity").cloned());
            o.set("position", p3s(e.get("position")));
            o.set("target", p3s(e.get("target")));
            o.set_opt("attenuationType", e.get("attenuationType").cloned());
            o.set("useAttenuationLimits", truthy(e.get("useAttenuationLimits")));
            for k in ["attenuationStart", "attenuationEnd", "hotspot", "falloff"] {
                o.set_opt(k, e.get(k).cloned());
            }
            o.set("castShadows", truthy(e.get("castShadows")));
            for k in ["shadowType", "shadowMapSize", "shadowMapSoftness"] {
                o.set_opt(k, e.get(k).cloned());
            }
            return Some(Conv::Recs(vec![r]));
        }
        if !matches!(ty, "REGION" | "3DSOLID" | "BODY") {
            return None;
        }
        let a = norm_acis(e.get("acis"));
        if a.is_none() {
            self.sol.empty = self.sol.empty.saturating_add(1);
        }
        let xd = field(e.get("xdata"), "ACAD_OBJECT_CHANGE_GUID").filter(|x| x.truthy()).cloned();
        let rg = e.get("revisionGuid");
        let mut r = if ver >= 27 && xd.is_some() {
            let mut x = e.obj("xdata").cloned().unwrap_or_default();
            x.remove("ACAD_OBJECT_CHANGE_GUID");
            let mut s = e.clone();
            s.set("xdata", x);
            self.common(&s, ty, owner)
        } else if (18..27).contains(&ver) && xd.is_none() && truthy(e.get("hasRevisionGuid")) && truthy(rg) && js_string(rg) != GUID0 {
            let s = with_guid_xdata(e, rg);
            self.common(&s, ty, owner)
        } else {
            self.common(e, ty, owner)
        };
        let mut has_guid = truthy(e.get("hasRevisionGuid"));
        let mut guid = rg.cloned();
        if guid.is_none()
            && let Some(xd) = &xd
        {
            let v: Vec<V> = arr(Some(xd)).iter().map(|x| x.as_arr().and_then(|a| a.get(1)).cloned().unwrap_or(V::Null)).collect();
            if v.len() >= 12 {
                has_guid = v.first().is_some_and(V::truthy);
                let n = |i: usize| to_number(v.get(i));
                let hx = |x: u32, w: usize| format!("{x:0w$x}");
                let b = |i: usize| hx(crate::util::to_u32(n(i)) & 0xFF, 2);
                let s = format!(
                    "{{{}-{}-{}-{}{}-{}}}",
                    hx(crate::util::to_u32(n(1)), 8),
                    hx(crate::util::to_u32(n(2)) & 0xFFFF, 4),
                    hx(crate::util::to_u32(n(3)) & 0xFFFF, 4),
                    b(4),
                    b(5),
                    (6..12).map(b).collect::<String>()
                );
                guid = Some(V::Str(s));
            }
        }
        r.o.set("hasRevisionGuid", has_guid && guid.as_ref().is_some_and(V::truthy));
        r.o.set("revisionGuid", guid.filter(V::truthy).unwrap_or_else(|| V::from(GUID0)));
        r.o.set_opt("wireframe", e.get("wireframe").cloned());
        r.o.set_opt("silhouettes", e.get("silhouettes").cloned());
        let hist = if ty == "3DSOLID" { e.get("history").cloned() } else { None };
        if let Some(a) = &a
            && ver >= 27
        {
            r.o.set("ds", true);
            match a {
                Acis::Sab(b) => self.sol.acds.push((r.h(), b.clone())),
                Acis::Sat(t) => {
                    self.sol.acds.push((r.h(), t.clone()));
                    self.sol.sat_in_r2013 = self.sol.sat_in_r2013.saturating_add(1);
                }
            }
        }
        if let Some(Acis::Sab(_)) = &a
            && ver < 18
        {
            self.sol.sab_in_r2000 = self.sol.sab_in_r2000.saturating_add(1);
        }
        self.sol.recs.push((r.h(), hist));
        r.enc = Enc::Acis(Box::new(AcisRec { acis: a }));
        Some(Conv::Recs(vec![r]))
    }

    /// S.finish: history handles of copied objects, notes.
    pub fn sol_finish(&mut self) {
        let hs: Vec<(f64, f64)> =
            self.sol.recs.iter().filter(|(_, s)| s.as_ref().is_some_and(V::truthy)).map(|(h, s)| (*h, self.src(s.as_ref()))).collect();
        if !hs.is_empty() {
            let mut at: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
            for (i, r) in self.recs.iter().enumerate() {
                if matches!(r.enc, Enc::Acis(_) | Enc::Surface(_)) {
                    at.entry(r.h().to_bits()).or_insert(i);
                }
            }
            for (h, hist) in hs {
                if let Some(r) = at.get(&h.to_bits()).and_then(|&i| self.recs.get_mut(i)) {
                    r.o.set("history", hist);
                }
            }
        }
        let n = |k: u32, key: &str, notes: &mut Vec<String>| {
            if k > 0 {
                notes.push(crate::w_msg::msg(key, &[("n", &k.to_string())]));
            }
        };
        n(self.sol.empty, "ccdwg.write.acis_empty", &mut self.notes);
        n(self.sol.sat_in_r2013, "ccdwg.write.acis_sat", &mut self.notes);
        n(self.sol.sab_in_r2000, "ccdwg.write.acis_sab", &mut self.notes);
    }
    /// S.dataStorage(): R2013+ AcDb:AcDsPrototype_1b
    pub fn sol_data_storage(&self) -> Option<Vec<u8>> {
        if self.ver >= 27 { crate::w_acds::write_acds(&self.sol.acds) } else { None }
    }
}
