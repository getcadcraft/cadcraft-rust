//! REGION / 3DSOLID / BODY (the ACIS entities, 20.4.41) and the ACIS data the R2013+ data storage section keeps for
//! them (port of lib/dwg/solid-read.js). Written from the ODA "Open Design Specification for .dwg files" v5.4.1: 20.4.41
//! (ACIS data, wireframe, silhouettes, history), 20.4.1 (the R2013+ "has data storage" flag), ch. 24 (data storage,
//! via acds.rs). Facts found by experiment are marked "Spec gap:".
//!
//! Record fields: acis { format 'sat'|'sab', version, data (SAT text | SAB base64) } (absent when the entity is empty),
//! wireframe? { point?, isolines, isolinesPresent, wires [] }, silhouettes? [], acisExtra? (a second ACIS block, very
//! rare), hasRevisionGuid + revisionGuid (R2013+), history (R2007+ handle, read.rs names it).

use crate::acis::{acis_value, find_acis_end, sat_decrypt, sat_version};
use crate::bits::{BitReader, Res, err};
use crate::messages::msg;
use crate::objects::{Ctx, Decoder, Record};
use crate::read::{Build, Records};
use crate::util::{b64, handle_hex as hx, to_i32, u32_from_i32, usize_from_i32, usize_to_f64};
use crate::value::{Obj, V};

pub fn decoder(name: &str) -> Option<Decoder> {
    match name {
        "REGION" | "3DSOLID" | "BODY" => Some(decode_acis_entity),
        _ => None,
    }
}

fn left(r: &BitReader) -> f64 {
    usize_to_f64(r.end) - usize_to_f64(r.pos)
}

/// V8's RangeError for `new Array(n)` with a negative n.
fn bad_array_len() -> crate::bits::DwgError {
    crate::bits::DwgError("Invalid array length".into())
}

struct AcisBlock {
    unknown: u32,
    dwg_version: u32,
    acis: Obj,
}

/// One ACIS data block (20.4.41): B empty, B unknown, BS version, then version 1 SAT blocks or a version 2 file.
/// `with_empty_bit` false: the block after the wireframe's "ACIS Empty bit" (that bit was its empty bit).
fn acis_block(r: &mut BitReader, with_empty_bit: bool) -> Res<Option<AcisBlock>> {
    if with_empty_bit && r.b()? != 0 {
        return Ok(None); // empty
    }
    let unknown = r.b()?;
    let version = r.bs()?;
    if version == 1 {
        // BL block size, that many RC; until a 0 size
        let mut all: Vec<u8> = Vec::new();
        for _guard in 0..1_000_000 {
            let n = r.bl()?;
            if n == 0 {
                break;
            }
            if f64::from(n) * 8.0 > left(r) {
                return err(msg("ccdwg.err.acis_block", &[("n", &n.to_string())]));
            }
            if n < 0 {
                return err(format!("Invalid typed array length: {n}"));
            }
            all.extend(r.bytes(usize_from_i32(n))?);
        }
        let text = sat_decrypt(&all);
        let mut acis = Obj::with_capacity(3);
        acis.set("format", "sat");
        acis.set_opt("version", sat_version(&text).map(V::Num));
        acis.set("data", text);
        return Ok(Some(AcisBlock { unknown, dwg_version: 1, acis }));
    }
    if version == 2 {
        // no length: read on to the end mark
        let n_left = r.end.saturating_sub(r.pos) >> 3;
        let save = r.pos;
        let all = r.bytes(n_left)?;
        let Some(n) = find_acis_end(&all) else {
            return err(msg("ccdwg.err.acis_no_end", &[]));
        };
        r.pos = save.saturating_add(n.saturating_mul(8));
        let b = all.get(..n).unwrap_or(&[]);
        return Ok(Some(AcisBlock { unknown, dwg_version: 2, acis: acis_value(b) }));
    }
    err(msg("ccdwg.err.acis_version", &[("version", &version.to_string())]))
}

/// 20.4.41 a wire of the wireframe or of a silhouette.
fn wire(r: &mut BitReader) -> Res<V> {
    let mut w = Obj::with_capacity(6);
    w.set("type", r.rc()?);
    w.set("selectionMarker", r.bl()?);
    w.set("color", r.bs()?);
    w.set("acisIndex", r.bl()?);
    let n = r.bl()?;
    if f64::from(n) * 6.0 > left(r) {
        return err(msg("ccdwg.err.wire_large", &[("n", &n.to_string())]));
    }
    if n < 0 {
        return Err(bad_array_len());
    }
    let n = usize_from_i32(n);
    let mut pts = Vec::with_capacity(n);
    for _ in 0..n {
        pts.push(r.bd3()?);
    }
    w.set("points", pts);
    if r.b()? != 0 {
        let x_axis = r.bd3()?;
        let y_axis = r.bd3()?;
        let z_axis = r.bd3()?;
        let translation = r.bd3()?;
        let scale = r.bd()?;
        let rot = r.bool()?;
        let refl = r.bool()?;
        let shear = r.bool()?;
        w.set(
            "transform",
            crate::obj! { "xAxis" => x_axis, "yAxis" => y_axis, "zAxis" => z_axis, "translation" => translation, "scale" => scale,
            "hasRotation" => rot, "hasReflection" => refl, "hasShear" => shear },
        );
    }
    Ok(V::Obj(w))
}
fn wires(r: &mut BitReader) -> Res<Vec<V>> {
    let n = r.bl()?;
    if f64::from(n) * 10.0 > left(r) {
        return err(msg("ccdwg.err.wires_large", &[("n", &n.to_string())]));
    }
    if n < 0 {
        return Err(bad_array_len());
    }
    let n = usize_from_i32(n);
    let mut a = Vec::with_capacity(n);
    for _ in 0..n {
        a.push(wire(r)?);
    }
    Ok(a)
}

fn wf_set(rec: &mut Record, k: &'static str, v: impl Into<V>) {
    if let Some(wf) = rec.f.obj_mut("wireframe") {
        wf.set(k, v);
    }
}

/// REGION / 3DSOLID / BODY.
pub fn decode_acis_entity(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    // Spec gap: R2013+ with the common "has data storage" flag (20.4.1) set: the ACIS data lives in AcDb:AcDsPrototype_1b
    // and the entity data starts directly with the wireframe part — no empty bit, unknown bit or version (AutoCAD's
    // example_2013/2018.dwg: their entity data equals what follows the SAB in example_2010.dwg).
    if ver >= 27 && rec.ds.is_some_and(|d| d != 0) {
        rec.f.set("_acisInDataStorage", true);
    } else {
        match acis_block(r, true)? {
            Some(a) => {
                rec.f.set("acis", a.acis);
                rec.f.set("_acisUnknownBit", a.unknown);
                rec.f.set("_acisDwgVersion", a.dwg_version);
            }
            None => rec.f.set("acisEmpty", true),
        }
    }
    // Spec gap: 20.4.41 lists "Num silhouettes", the second "ACIS Empty bit" and (R2007+) "Unknown BL" after the wireframe
    // for every entity. In files they are: wireframe present B; only when present: point, isolines, wires, silhouettes and
    // the second empty bit; then (R2007+) the BL. (AutoCAD's solids have wireframes: 155 bits after the SAB in
    // example_2010.dwg = wireframe … empty bit + BL 0 + the string-stream flag; the ODA File Converter's solids without one
    // have just "0" + BL 0 + the flag.)
    if r.b()? != 0 {
        rec.f.set("wireframe", Obj::new());
        if r.b()? != 0 {
            let p = r.bd3()?;
            wf_set(rec, "point", p);
        }
        let iso = r.bl()?;
        wf_set(rec, "isolines", iso);
        let present = r.bool()?;
        wf_set(rec, "isolinesPresent", present);
        // Spec gap: "Num Wires" and the wires are there only when "IsoLines present" is set (the ODA File Converter's
        // R2000/R2004 saves of example_2010.dwg: isolines present 0, then at once the silhouette count).
        if present {
            let w = wires(r)?;
            wf_set(rec, "wires", w);
        }
        let ns = r.bl()?;
        if f64::from(ns) * 10.0 > left(r) {
            return err(msg("ccdwg.err.silhouettes_large", &[("n", &ns.to_string())]));
        }
        rec.f.set("silhouettes", V::Arr(Vec::new()));
        // Spec gap: the viewport id is a BLL from R2010 (a BL before: the ODA File Converter's R2004/R2007 and R2010 saves
        // of the same client drawing), and a silhouette's "Num Wires" and wires follow a "wires present" bit, as the
        // wireframe's follow "IsoLines present".
        for _ in 0..ns.max(0) {
            let vp = if ver >= 24 { r.bll()? } else { f64::from(r.bl()?) };
            let target = r.bd3()?;
            let direction = r.bd3()?;
            let up = r.bd3()?;
            let persp = r.bool()?;
            let mut sil =
                crate::obj! { "viewportId" => vp, "target" => target, "direction" => direction, "up" => up, "perspective" => persp };
            let wp = r.bool()?;
            sil.set("wiresPresent", wp);
            if wp {
                sil.set("wires", wires(r)?);
            }
            if let Some(V::Arr(a)) = rec.f.get_mut("silhouettes") {
                a.push(V::Obj(sil));
            }
        }
        // "ACIS Empty bit — normally 1. If 0, then acis data follows in the same format as described above". Spec gap: the
        // block has no empty bit of its own (this bit is it): B unknown, BS version, data — the ODA File Converter writes
        // the solid's ACIS data a second time here when it saves an AutoCAD solid with a wireframe down to R2000/R2004.
        if r.b()? == 0
            && let Some(a) = acis_block(r, false)?
        {
            rec.f.set("acisExtra", a.acis);
        }
    }
    if ver >= 21 {
        let v = r.bl()?;
        rec.f.set("_unknownBL", v);
    }
    // Spec gap: R2013+ add the revision GUID (DXF 290 flag + 2 GUID of AcDbModelerGeometry): B flag, BL, BS, BS, 8 RC, then
    // a BL (0 in every file seen) — found by locating the GUIDs the ODA File Converter's DXF shows in the entity bits.
    if ver >= 27 {
        let flag = r.bool()?;
        rec.f.set("hasRevisionGuid", flag);
        let d1 = u32_from_i32(r.bl()?);
        let d2 = u32_from_i32(r.bs()?) & 0xFFFF;
        let d3 = u32_from_i32(r.bs()?) & 0xFFFF;
        let d4 = r.bytes(8)?;
        rec.f.set("revisionGuid", guid_string(d1, d2, d3, &d4));
        let v = r.bl()?;
        rec.f.set("_unknownBL2", v);
    }
    if ver >= 21 && rec.typ == "3DSOLID" {
        let v = h.h(rec.handle)?;
        rec.h.set("history", v); // DXF 350: AcDb3dSolid only
    }
    Ok(())
}

pub fn guid_string(d1: u32, d2: u32, d3: u32, d4: &[u8]) -> String {
    let b: String = d4.iter().map(|x| format!("{x:02x}")).collect();
    let (b1, b2) = if b.len() >= 4 { b.split_at(4) } else { (b.as_str(), "") };
    format!("{{{d1:08x}-{d2:04x}-{d3:04x}-{b1}-{b2}}}")
}

/// JS ToInt32 of an xdata value (`v & 0xFF`, `v >>> 0`): numbers wrap, numeric strings parse, the rest are 0.
fn js_int(v: &V) -> i32 {
    match v {
        V::Num(n) => to_i32(*n),
        V::Bool(b) => i32::from(*b),
        V::Str(s) => {
            let t = s.trim();
            if t.is_empty() { 0 } else { t.parse::<f64>().map_or(0, to_i32) }
        }
        _ => 0,
    }
}

/// ACAD_OBJECT_CHANGE_GUID extended data (R2004–R2010 keep the revision GUID there): [1070 flag][1071][1070][1070][1070 × 8].
fn guid_from_xdata(items: &[(i32, V)]) -> Option<(bool, String)> {
    if items.len() < 12 {
        return None;
    }
    let v: Vec<&V> = items.iter().map(|x| &x.1).collect();
    let g = |i: usize| v.get(i).map_or(0, |x| js_int(x));
    let d4: Vec<u8> = (0..8).map(|i| crate::util::u8_of(u32_from_i32(g(4usize.saturating_add(i))) & 0xFF)).collect();
    let flag = v.first().is_some_and(|x| x.truthy());
    Some((flag, guid_string(u32_from_i32(g(1)), u32_from_i32(g(2)) & 0xFFFF, u32_from_i32(g(3)) & 0xFFFF, &d4)))
}

/// R2013+: the SAB of every ACIS entity stored in AcDb:AcDsPrototype_1b (ch. 24), attached to its record by handle.
/// Records of other kinds (thumbnails …) are left alone.
pub fn attach_data_storage(records: &mut Records, sec: Option<&[u8]>, notes: &mut Vec<String>) {
    let in_ds = |r: &Record| r.f.truthy("_acisInDataStorage");
    let Some(sec) = sec else {
        let n = records.iter().filter(|r| in_ds(r)).count();
        if n > 0 {
            notes.push(msg("ccdwg.read.acis_no_storage", &[("n", &n.to_string())]));
        }
        return;
    };
    let ds = crate::acds::read_acds(sec);
    notes.extend(ds.notes.iter().cloned());
    let mut missing = 0usize;
    for r in &mut records.list {
        if !in_ds(r) {
            continue;
        }
        match ds.records.get(&crate::util::to_i64(r.handle)) {
            None => {
                missing = missing.saturating_add(1);
                r.f.set("acisEmpty", true);
            }
            Some(b) => r.f.set("acis", acis_value(b)),
        }
    }
    if missing > 0 {
        notes.push(msg("ccdwg.read.acis_no_record", &[("n", &missing.to_string())]));
    }
}

/// read.rs: model fields of an ACIS entity record (handles named, the R2004–R2010 xdata GUID surfaced).
pub fn solid_fields(rec: &Record, f: &mut Obj, b: &Build) {
    if let Some(hh) = rec.hn("history")
        && hh != 0.0
        && !hh.is_nan()
    {
        f.set("history", hx(hh));
    }
    if !rec.f.has("revisionGuid")
        && let Some(eed) = &rec.eed
        && let Some(x) = eed.iter().find(|e| b.app_name(e.app) == "ACAD_OBJECT_CHANGE_GUID")
        && let Some((flag, guid)) = guid_from_xdata(&x.items)
    {
        f.set("hasRevisionGuid", flag);
        f.set("revisionGuid", guid);
    }
}

/// The rest of the entity data as `{ ver, n, data }` (surface_read.rs: embedded data of unknown encoding).
pub(crate) fn raw_rest(r: &mut BitReader, ver: u32) -> Obj {
    let n = r.end.saturating_sub(r.pos);
    let data = b64(&crate::util::slice_bits(r.buf, r.pos, r.pos.saturating_add(n)));
    r.pos = r.end;
    crate::obj! { "ver" => ver, "n" => n, "data" => data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_text() {
        assert_eq!(guid_string(0xABC, 0x12, 0xFFFF, &[1, 2, 3, 4, 5, 6, 7, 0xFF]), "{00000abc-0012-ffff-0102-0304050607ff}");
        let items: Vec<(i32, V)> = (0..12).map(|i| (1070, V::Num(f64::from(i)))).collect();
        let (flag, g) = guid_from_xdata(&items).unwrap_or_default();
        assert!(!flag);
        assert_eq!(g, "{00000001-0002-0003-0405-060708090a0b}");
    }
}
