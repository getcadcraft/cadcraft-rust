//! The SURFACE family: SURFACE, PLANESURFACE, EXTRUDEDSURFACE, LOFTEDSURFACE, REVOLVEDSURFACE, SWEPTSURFACE and
//! NURBSURFACE (port of lib/dwg/surface-read.js and surface-layout.js). Written from the ODA "Open Design Specification
//! for .dwg files" v5.4.1: 20.4.41 (the ACIS data, wireframe and silhouettes every ACIS entity starts with —
//! solid_read.rs reads it), 20.4.1 (the R2013+ "has data storage" flag), ch. 24 (data storage, acds.rs).
//!
//! Spec gap: the specification documents none of these classes (it has no section on any SURFACE entity, nor on
//! AcDbSurface's own data). Everything after the 20.4.41 part was found by experiment: fixtures with a distinctive value
//! in every field (made by ezdxf), saved by the ODA File Converter as DWG in all six versions; each double located bit by
//! bit in the entity data, each flag by fixtures that set one flag at a time, and every field checked against the
//! converter's DXF of the same DWG. The layout is the same in every version from R2000 to R2018 (the R2007+ string
//! stream holds no strings for these classes).
//!
//! Layout (all versions): the 20.4.41 ACIS part (as REGION/BODY; no history handle) — then AcDbSurface: BS U isolines
//! (DXF 71), BS V isolines (72) — then the class's fields, [`layout`]. Codes: BD, BS, B, BL, BD3 (3BD), M (a 4×4 matrix
//! as 16 BD, in DXF order), ENT (an embedded entity: BL class id, then its data when the id is not 0), ENTB (the same
//! announced by a B).
//!
//! Spec gap: an embedded profile / path / revolve / sweep entity (the DXF's "90 class id, 90 size, 310 data"; the loft's
//! cross sections, guides and path) was 0 / absent in every file the converter wrote; the converter drops a surface whose
//! embedded data it cannot parse (found by giving it made-up DXF data) and refuses one that announces data without it
//! (found by writing such files), so the encoding is unknown: a non-zero value ends the decoding and the rest of the
//! entity's data is kept as it is (`{ value, raw: { ver, n, data } }`, value = the class id / count, or 1 for the loft's
//! path flag).

use crate::bits::{BitReader, Res};
use crate::objects::{Ctx, Decoder, Record};
use crate::read::Build;
use crate::value::{Obj, V};

#[derive(Clone, Copy)]
enum Op {
    Bd,
    Bs,
    B,
    Bd3,
    M,
    Ent,
    Entb,
}
use Op::{B, Bd, Bd3, Bs, Ent, Entb, M};

pub const SURFACE_TYPES: [&str; 7] =
    ["SURFACE", "PLANESURFACE", "EXTRUDEDSURFACE", "LOFTEDSURFACE", "REVOLVEDSURFACE", "SWEPTSURFACE", "NURBSURFACE"];

/// AcDbSweepOptions as EXTRUDEDSURFACE and SWEPTSURFACE store them (DXF codes in the comments).
const SWEEP_OPTIONS: [(&str, Op); 17] = [
    ("draftAngle", Bd),            // 42
    ("startDraftDistance", Bd),    // 43
    ("endDraftDistance", Bd),      // 44
    ("twistAngle", Bd),            // 45
    ("scaleFactor", Bd),           // 48
    ("alignAngle", Bd),            // 49
    ("solid", B),                  // 290
    ("alignment", Bs),             // 70 sweep alignment option
    ("miterOption", Bs),           // 71
    ("alignStart", B),             // 292
    ("bank", B),                   // 293
    ("basePointSet", B),           // 294
    ("twistRefVector", Bd3),       // 11 reference vector for controlling twist
    ("sweepTransformComputed", B), // 295
    ("pathTransformComputed", B),  // 296
    ("sweepEntityTransform", M),   // 46
    ("pathEntityTransform", M),    // 47
];

const EXTRUDED: [(&str, Op); 3] = [
    ("sweepVector", Bd3),   // 10
    ("matrix", M),          // 40 transform of the extruded entity
    ("profileEntity", Ent), // 90 (+ size, 310 data)
];
const SWEPT: [(&str, Op); 4] = [
    ("sweepEntityMatrix", M), // 40 transform of the sweep entity
    ("pathEntityMatrix", M),  // 41 transform of the path entity
    ("sweepEntity", Ent),     // 90
    ("pathEntity", Ent),      // 91
];
const REVOLVED: [(&str, Op); 12] = [
    ("draftAngle", Bd),         // 43
    ("startDraftDistance", Bd), // 44
    ("endDraftDistance", Bd),   // 45
    ("twistAngle", Bd),         // 46
    ("solid", B),               // 290
    ("closeToAxis", B),         // 291
    ("axisPoint", Bd3),         // 10
    ("axisVector", Bd3),        // 11
    ("revolveAngle", Bd),       // 40
    ("startAngle", Bd),         // 41
    ("matrix", M),              // 42 transform of the revolved entity
    ("revolveEntity", Ent),     // 90
];
// Spec gap: after the matrix come a BL, a BL and a B that were 0 in every file the converter wrote, and any other value
// there makes the converter fail ("Object improperly read") — so they announce embedded data, presumably the cross
// section and guide curve entities and the path entity of the loft (found by writing such files). The last field, a BS,
// is DXF 70 (writing 1 there, the converter's DXF shows 70 1). The flags are in DXF order except "solid" (295), which
// comes before "closed" (294).
const LOFTED: [(&str, Op); 17] = [
    ("matrix", M),                    // 40 transform of the loft entity
    ("crossSections", Ent),           // (not in DXF)
    ("guides", Ent),                  // (not in DXF)
    ("pathEntity", Entb),             // (not in DXF)
    ("startDraftAngle", Bd),          // 41
    ("endDraftAngle", Bd),            // 42
    ("startDraftMagnitude", Bd),      // 43
    ("endDraftMagnitude", Bd),        // 44
    ("arcLengthParameterization", B), // 290
    ("noTwist", B),                   // 291
    ("alignDirection", B),            // 292
    ("simpleSurfaces", B),            // 293
    ("solid", B),                     // 295
    ("closedSurfaces", B),            // 294
    ("ruledSurface", B),              // 296
    ("virtualGuide", B),              // 297
    ("normal", Bs),                   // 70 plane normal lofting type
];
// R2013+ only (the converter saves it down to R2000–R2010 as a plain SURFACE). The fields' meanings are not given by the
// spec nor by the converter's DXF (AcDbNurbSurface 170, 290, 10–13): they are named by their DXF codes.
const NURBS: [(&str, Op); 6] = [
    ("nurbs170", Bs), // 170
    ("nurbs290", B),  // 290
    ("nurbs10", Bd3), // 10
    ("nurbs11", Bd3), // 11
    ("nurbs12", Bd3), // 12
    ("nurbs13", Bd3), // 13
];

/// The class's fields after the AcDbSurface part, in order (SURFACE_LAYOUTS of surface-layout.js).
fn layout(t: &str) -> Vec<(&'static str, Op)> {
    match t {
        "EXTRUDEDSURFACE" => SWEEP_OPTIONS.iter().chain(EXTRUDED.iter()).copied().collect(),
        "SWEPTSURFACE" => SWEEP_OPTIONS.iter().chain(SWEPT.iter()).copied().collect(),
        "REVOLVEDSURFACE" => REVOLVED.to_vec(),
        "LOFTEDSURFACE" => LOFTED.to_vec(),
        "NURBSURFACE" => NURBS.to_vec(),
        _ => Vec::new(), // SURFACE, PLANESURFACE (AcDbPlaneSurface adds nothing)
    }
}

pub fn decoder(name: &str) -> Option<Decoder> {
    if SURFACE_TYPES.contains(&name) { Some(decode_surface) } else { None }
}

fn read_field(r: &mut BitReader, op: Op) -> Res<V> {
    Ok(match op {
        Bd => V::Num(r.bd()?),
        Bs => V::from(r.bs()?),
        B => V::Bool(r.bool()?),
        Bd3 => r.bd3()?,
        M => {
            let mut m = Vec::with_capacity(16);
            for _ in 0..16 {
                m.push(V::Num(r.bd()?));
            }
            V::Arr(m)
        }
        // handled by decode_surface
        Ent | Entb => V::Null,
    })
}

/// Any SURFACE class: the ACIS part, AcDbSurface, the class's fields.
fn decode_surface(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    crate::solid_read::decode_acis_entity(r, h, rec, ctx)?;
    let u = r.bs()?;
    rec.f.set("uIsolines", u);
    let v = r.bs()?;
    rec.f.set("vIsolines", v);
    for (name, op) in layout(&rec.typ) {
        let value = match op {
            Ent => r.bl()?,
            Entb => crate::util::i32_from_u32(r.b()?),
            _ => {
                let x = read_field(r, op)?;
                rec.f.set(name, x);
                continue;
            }
        };
        if value == 0 {
            continue;
        }
        // embedded data of unknown encoding (see the header): keep the rest of the entity's data as it is and stop
        let raw = crate::solid_read::raw_rest(r, ctx.ver);
        rec.f.set(name, crate::obj! { "value" => value, "raw" => raw });
        break;
    }
    Ok(())
}

/// JS `===` between an XRECORD value and a code / string.
fn is_num(v: Option<&V>, n: f64) -> bool {
    matches!(v, Some(V::Num(x)) if *x == n)
}

/// read.rs: model fields of a surface — the revision GUID R2004–R2010 keep in ACAD_OBJECT_CHANGE_GUID xdata (as for
/// solids), and `savedDownFrom: 'NURBSURFACE'` for a SURFACE that a save-down made of a NURBS surface. Spec gap: not in
/// the spec; the ODA File Converter marks it with XRECORD ACAD_XREC_ROUNDTRIP of the surface's extension dictionary,
/// [102 "AcadNurbSurfaceData"][90 0][310][102 "AcadModlrDecomposedType"][90 1], and makes it a NURBSURFACE again when it
/// saves the drawing as R2013+ (found by converting the fixture down and up again).
pub fn surface_fields(rec: &Record, f: &mut Obj, b: &Build) -> Res<()> {
    crate::solid_read::solid_fields(rec, f, b);
    let xdic = rec.hn("xdic").unwrap_or(0.0);
    if rec.typ != "SURFACE" || xdic == 0.0 || xdic.is_nan() {
        return Ok(());
    }
    let Some(dict) = b.get(xdic) else { return Ok(()) };
    if dict.typ != "DICTIONARY" {
        return Ok(());
    }
    let Some(names) = dict.f.arr("names") else { return Ok(()) };
    let Some(k) = names.iter().position(|n| n.as_str() == Some("ACAD_XREC_ROUNDTRIP")) else { return Ok(()) };
    let items = dict.hlist("items").unwrap_or_default();
    let Some(xr) = items.get(k).and_then(|&x| b.get(x)) else { return Ok(()) };
    if xr.typ != "XRECORD" {
        return Ok(());
    }
    let Some(data) = xr.f.arr("data") else { return Ok(()) };
    let pair = |d: &V| -> Option<(Option<V>, Option<V>)> {
        match d {
            V::Arr(p) => Some((p.first().cloned(), p.get(1).cloned())),
            _ => None,
        }
    };
    let Some(i) = data.iter().position(|d| {
        pair(d).is_some_and(|(c, v)| is_num(c.as_ref(), 102.0) && matches!(v, Some(V::Str(ref s)) if s == "AcadModlrDecomposedType"))
    }) else {
        return Ok(());
    };
    let next = data.get(i.saturating_add(1)).and_then(pair);
    let v = match next {
        Some((c, v)) if is_num(c.as_ref(), 90.0) => v,
        _ => None,
    };
    // v === undefined: the pair's value slot missing as well
    match v {
        Some(V::Num(1.0)) => f.set("savedDownFrom", "NURBSURFACE"),
        Some(x) => f.set("decomposedType", x),
        None => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        assert_eq!(layout("EXTRUDEDSURFACE").len(), 20);
        assert_eq!(layout("SWEPTSURFACE").len(), 21);
        assert!(layout("PLANESURFACE").is_empty());
        assert!(decoder("NURBSURFACE").is_some());
        assert!(decoder("3DSOLID").is_none());
    }
}
