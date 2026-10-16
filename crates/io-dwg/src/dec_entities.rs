//! Entity type decoders (port of lib/dwg/objects-read.js, 20.4.3–20.4.50: TEXT … MLINE). Field names are the JS
//! record's (`rec.<name>` → `rec.f`, `rec._h.<name>` → `rec.h`). DICTIONARY lives in dec_objects.rs; REGION /
//! 3DSOLID / BODY in solid_read.rs; the SURFACE family in surface_read.rs.

use crate::bits::{BitReader, Res};
use crate::objects::{Ctx, Decoder, Record, fits, hrefs};
use crate::util::usize_from_i32;
use crate::value::{Obj, V, p3};

pub fn decoder(name: &str) -> Option<Decoder> {
    Some(match name {
        "TEXT" => text,
        "ATTRIB" => attrib,
        "ATTDEF" => attdef,
        "BLOCK" => block,
        "ENDBLK" | "SEQEND" => nothing,
        "INSERT" => insert,
        "MINSERT" => minsert,
        "VERTEX_2D" => vertex_2d,
        "VERTEX_3D" | "VERTEX_MESH" | "VERTEX_PFACE" => vertex_3d,
        "VERTEX_PFACE_FACE" => vertex_pface_face,
        "POLYLINE_2D" => polyline_2d,
        "POLYLINE_3D" => polyline_3d,
        "POLYLINE_PFACE" => polyline_pface,
        "POLYLINE_MESH" => polyline_mesh,
        "ARC" => arc,
        "CIRCLE" => circle,
        "LINE" => line,
        "DIMENSION_ORDINATE" => dim_ordinate,
        "DIMENSION_LINEAR" => dim_linear,
        "DIMENSION_ALIGNED" => dim_aligned,
        "DIMENSION_ANG3PT" => dim_ang3pt,
        "DIMENSION_ANG2LN" => dim_ang2ln,
        "DIMENSION_RADIUS" => dim_radius,
        "DIMENSION_DIAMETER" => dim_diameter,
        "ARC_DIMENSION" => arc_dimension,
        "LARGE_RADIAL_DIMENSION" => large_radial_dimension,
        "POINT" => point,
        "3DFACE" => face3d,
        "SOLID" | "TRACE" => solid,
        "SHAPE" => shape,
        "VIEWPORT" => viewport,
        "ELLIPSE" => ellipse,
        "SPLINE" => spline,
        "RAY" | "XLINE" => ray,
        "MTEXT" => mtext,
        "LEADER" => leader,
        "TOLERANCE" => tolerance,
        "MLINE" => mline,
        _ => return None,
    })
}

// ---------------------------------------------------------------- small helpers

/// `o[k].push(v)` for an array set earlier (JS `rec.k = []; … rec.k.push(v)`).
fn push(o: &mut Obj, k: &str, v: V) {
    if let Some(V::Arr(a)) = o.get_mut(k) {
        a.push(v);
    }
}
/// `o[k][i] = v` for an array made with `new Array(n)` (unfilled slots stay null, as JSON prints holes).
fn put(o: &mut Obj, k: &str, i: usize, v: V) {
    if let Some(V::Arr(a)) = o.get_mut(k)
        && let Some(x) = a.get_mut(i)
    {
        *x = v;
    }
}
/// `rec._h.k = h.H(rec.handle)`.
fn href(h: &mut BitReader, rec: &mut Record, k: &'static str) -> Res<()> {
    let v = h.h(rec.handle)?;
    rec.h.set(k, v);
    Ok(())
}
/// A BD3 stored under `k`.
fn set_bd3(r: &mut BitReader, o: &mut Obj, k: &'static str) -> Res<()> {
    let v = r.bd3()?;
    o.set(k, v);
    Ok(())
}
/// A BD stored under `k`.
fn set_bd(r: &mut BitReader, o: &mut Obj, k: &'static str) -> Res<()> {
    let v = r.bd()?;
    o.set(k, v);
    Ok(())
}
/// A BS stored under `k`.
fn set_bs(r: &mut BitReader, o: &mut Obj, k: &'static str) -> Res<()> {
    let v = r.bs()?;
    o.set(k, v);
    Ok(())
}

// ---- 20.4.3 TEXT (and ATTRIB/ATTDEF text part)
fn text_data(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    if ctx.ver < 15 {
        let elev = r.bd()?;
        o.set("elevation", elev);
        let a = r.rd2_raw()?;
        let b = r.rd2_raw()?;
        o.set("insert", p3(a[0], a[1], elev));
        o.set("align", p3(b[0], b[1], elev));
        set_bd3(r, o, "extrusion")?;
        set_bd(r, o, "thickness")?;
        set_bd(r, o, "oblique")?;
        set_bd(r, o, "rotation")?;
        set_bd(r, o, "height")?;
        set_bd(r, o, "widthFactor")?;
        let v = r.tv()?;
        o.set("value", v);
        set_bs(r, o, "generation")?;
        set_bs(r, o, "hAlign")?;
        set_bs(r, o, "vAlign")?;
    } else {
        let f = r.rc()?;
        let elev = if f & 1 != 0 { 0.0 } else { r.rd()? };
        o.set("elevation", elev);
        let ins = r.rd2_raw()?;
        o.set("insert", p3(ins[0], ins[1], elev));
        if f & 2 != 0 {
            o.set("align", p3(ins[0], ins[1], elev));
        } else {
            let x = r.dd(ins[0])?;
            let y = r.dd(ins[1])?;
            o.set("align", p3(x, y, elev));
        }
        o.set("_alignGiven", f & 2 == 0);
        let e = r.be()?;
        o.set("extrusion", e);
        let t = r.bt()?;
        o.set("thickness", t);
        let ob = if f & 4 != 0 { 0.0 } else { r.rd()? };
        o.set("oblique", ob);
        let ro = if f & 8 != 0 { 0.0 } else { r.rd()? };
        o.set("rotation", ro);
        let he = r.rd()?;
        o.set("height", he);
        let wf = if f & 0x10 != 0 { 1.0 } else { r.rd()? };
        o.set("widthFactor", wf);
        let v = r.tv()?;
        o.set("value", v);
        let g = if f & 0x20 != 0 { 0 } else { r.bs()? };
        o.set("generation", g);
        let ha = if f & 0x40 != 0 { 0 } else { r.bs()? };
        o.set("hAlign", ha);
        let va = if f & 0x80 != 0 { 0 } else { r.bs()? };
        o.set("vAlign", va);
    }
    Ok(())
}
fn text(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    text_data(r, rec, ctx)?;
    href(h, rec, "style")
}
fn attrib_data(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    text_data(r, rec, ctx)?;
    if ctx.ver >= 24 {
        let v = r.rc()?;
        rec.f.set("_attVersion", v);
    }
    let mut typ = 1;
    if ctx.ver >= 32 {
        typ = r.rc()?;
    }
    rec.f.set("attType", typ);
    if typ > 1 {
        // R2018 multi-line attribute: an embedded MTEXT (from entmode on), then annotative data
        let m = embedded_mtext(r, ctx)?;
        rec.f.set("mtext", m);
        let ann_size = r.bs()?;
        if ann_size > 0 {
            r.bytes(usize_from_i32(ann_size))?;
            rec.f.set("_annApp", true);
            r.bs()?;
        }
        let tag = r.tv()?;
        rec.f.set("tag", tag);
        r.bs()?;
        let fl = r.rc()?;
        rec.f.set("flags", fl);
        let lp = r.bool()?;
        rec.f.set("lockPosition", lp);
    } else {
        let tag = r.tv()?;
        rec.f.set("tag", tag);
        set_bs(r, &mut rec.f, "fieldLength")?;
        let fl = r.rc()?;
        rec.f.set("flags", fl);
        if ctx.ver >= 21 {
            let lp = r.bool()?;
            rec.f.set("lockPosition", lp);
        }
    }
    Ok(())
}
fn attrib(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    attrib_data(r, rec, ctx)?;
    href(h, rec, "style")?;
    if rec.f.truthy("_annApp") {
        href(h, rec, "annApp")?;
    }
    Ok(())
}
fn attdef(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    attrib_data(r, rec, ctx)?;
    if ctx.ver >= 24 {
        let v = r.rc()?;
        rec.f.set("_attdefVersion", v);
    }
    let p = r.tv()?;
    rec.f.set("prompt", p);
    href(h, rec, "style")?;
    if rec.f.truthy("_annApp") {
        href(h, rec, "annApp")?;
    }
    Ok(())
}
/// R2018 multi-line ATTRIB/ATTDEF: "all fields of an embedded MTEXT object, starting from the entmode" (20.4.4).
/// Spec gap: that means the common entity data from the entmode on (entmode BB, reactors BL, no-xdictionary B,
/// R2013 data-store B, colour ENC, linetype scale BD, linetype/plot style/material flags BB, shadow RC, the three
/// visual-style bits, invisibility BS, lineweight RC) and then the MTEXT data; its handles are not separated
/// from the attribute's own — found by experiment with R2018 files from the ODA File Converter.
fn embedded_mtext(r: &mut BitReader, ctx: &Ctx) -> Res<Obj> {
    let ver = ctx.ver;
    let mut m = Obj::new();
    r.bb()?;
    r.bl()?;
    r.b()?;
    if ver >= 27 {
        r.b()?;
    }
    let c = r.enc()?.0;
    m.set("color", c);
    set_bd(r, &mut m, "ltscale")?;
    r.bb()?;
    r.bb()?;
    if ver >= 21 {
        r.bb()?;
        r.rc()?;
    }
    if ver >= 24 {
        r.b()?;
        r.b()?;
        r.b()?;
    }
    r.bs()?;
    r.rc()?;
    mtext_data(r, None, 0.0, &mut m, ctx)?;
    Ok(m)
}

// ---- 20.4.6/7/8 BLOCK, ENDBLK, SEQEND
fn block(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let n = r.tv()?;
    rec.f.set("name", n);
    Ok(())
}
fn nothing(_r: &mut BitReader, _h: &mut BitReader, _rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    Ok(())
}

// ---- 20.4.9/10 INSERT, MINSERT
fn insert_data(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx, minsert: bool) -> Res<()> {
    set_bd3(r, &mut rec.f, "insert")?;
    if ctx.ver < 15 {
        let s = r.bd3_raw()?;
        rec.f.set("scale", p3(s[0], s[1], s[2]));
    } else {
        let f = r.bb()?;
        let s = match f {
            3 => p3(1.0, 1.0, 1.0),
            1 => {
                let y = r.dd(1.0)?;
                p3(1.0, y, r.dd(1.0)?)
            }
            2 => {
                let x = r.rd()?;
                p3(x, x, x)
            }
            _ => {
                let x = r.rd()?;
                let y = r.dd(x)?;
                p3(x, y, r.dd(x)?)
            }
        };
        rec.f.set("scale", s);
    }
    set_bd(r, &mut rec.f, "rotation")?;
    set_bd3(r, &mut rec.f, "extrusion")?;
    let has = r.b()?;
    rec.f.set("_hasAttribs", has);
    let mut owned = 0;
    // Spec gap: 20.4.9 lists "Owned Object Count" for R2004+ unconditionally; it is present only when the
    // has-ATTRIBs bit is set (INSERTs without attributes end right after that bit — found by experiment).
    if ctx.ver >= 18 && has != 0 {
        owned = r.bl()?;
    }
    if minsert {
        set_bs(r, &mut rec.f, "cols")?;
        set_bs(r, &mut rec.f, "rows")?;
        set_bd(r, &mut rec.f, "colSpacing")?;
        set_bd(r, &mut rec.f, "rowSpacing")?;
    }
    href(h, rec, "block")?;
    if has != 0 {
        if ctx.ver < 18 {
            href(h, rec, "firstAttrib")?;
            href(h, rec, "lastAttrib")?;
        } else {
            let a = hrefs(h, rec.handle, f64::from(owned))?;
            rec.h.set("attribs", a);
        }
        href(h, rec, "seqend")?;
    }
    Ok(())
}
fn insert(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    insert_data(r, h, rec, ctx, false)
}
fn minsert(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    insert_data(r, h, rec, ctx, true)
}

// ---- 20.4.11–15 VERTEX
fn vertex_2d(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    let fl = r.rc()?;
    o.set("flags", fl);
    set_bd3(r, o, "point")?;
    let sw = r.bd()?;
    if sw < 0.0 {
        o.set("endWidth", -sw);
        o.set("startWidth", -sw);
    } else {
        o.set("startWidth", sw);
        set_bd(r, o, "endWidth")?;
    }
    set_bd(r, o, "bulge")?;
    if ctx.ver >= 24 {
        let id = r.bl()?;
        o.set("id", id);
    }
    set_bd(r, o, "tangent")
}
fn vertex_3d(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let fl = r.rc()?;
    rec.f.set("flags", fl);
    set_bd3(r, &mut rec.f, "point")
}
fn vertex_pface_face(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let a = r.bs()?;
    let b = r.bs()?;
    let c = r.bs()?;
    let d = r.bs()?;
    rec.f.set("indices", vec![V::from(a), V::from(b), V::from(c), V::from(d)]);
    Ok(())
}

// ---- 20.4.16/17/33/34 POLYLINE
fn poly_owned(h: &mut BitReader, rec: &mut Record, ctx: &Ctx, owned: i32) -> Res<()> {
    if ctx.ver < 18 {
        href(h, rec, "firstVertex")?;
        href(h, rec, "lastVertex")?;
    } else {
        let v = hrefs(h, rec.handle, f64::from(owned))?;
        rec.h.set("vertices", v);
    }
    href(h, rec, "seqend")
}
fn owned_count(r: &mut BitReader, ctx: &Ctx) -> Res<i32> {
    if ctx.ver >= 18 { r.bl() } else { Ok(0) }
}
fn polyline_2d(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    set_bs(r, o, "flags")?;
    set_bs(r, o, "curveType")?;
    set_bd(r, o, "startWidth")?;
    set_bd(r, o, "endWidth")?;
    let t = r.bt()?;
    o.set("thickness", t);
    set_bd(r, o, "elevation")?;
    let e = r.be()?;
    o.set("extrusion", e);
    let owned = owned_count(r, ctx)?;
    poly_owned(h, rec, ctx, owned)
}
fn polyline_3d(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let s = r.rc()?;
    rec.f.set("_splineFlags", s);
    let c = r.rc()?;
    rec.f.set("_closedFlags", c);
    let owned = owned_count(r, ctx)?;
    poly_owned(h, rec, ctx, owned)
}
fn polyline_pface(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    set_bs(r, &mut rec.f, "numVertices")?;
    set_bs(r, &mut rec.f, "numFaces")?;
    let owned = owned_count(r, ctx)?;
    poly_owned(h, rec, ctx, owned)
}
fn polyline_mesh(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    for k in ["flags", "curveType", "m", "n", "mDensity", "nDensity"] {
        set_bs(r, &mut rec.f, k)?;
    }
    let owned = owned_count(r, ctx)?;
    poly_owned(h, rec, ctx, owned)
}

// ---- 20.4.18/20/21 ARC CIRCLE LINE
fn circle_data(r: &mut BitReader, o: &mut Obj) -> Res<()> {
    set_bd3(r, o, "center")?;
    set_bd(r, o, "radius")?;
    let t = r.bt()?;
    o.set("thickness", t);
    let e = r.be()?;
    o.set("extrusion", e);
    Ok(())
}
fn arc(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    circle_data(r, &mut rec.f)?;
    set_bd(r, &mut rec.f, "startAngle")?;
    set_bd(r, &mut rec.f, "endAngle")
}
fn circle(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    circle_data(r, &mut rec.f)
}
fn line(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    if ctx.ver < 15 {
        set_bd3(r, o, "start")?;
        set_bd3(r, o, "end")?;
    } else {
        let zzero = r.b()?;
        let sx = r.rd()?;
        let ex = r.dd(sx)?;
        let sy = r.rd()?;
        let ey = r.dd(sy)?;
        let (mut sz, mut ez) = (0.0, 0.0);
        if zzero == 0 {
            sz = r.rd()?;
            ez = r.dd(sz)?;
        }
        o.set("start", p3(sx, sy, sz));
        o.set("end", p3(ex, ey, ez));
    }
    let t = r.bt()?;
    o.set("thickness", t);
    let e = r.be()?;
    o.set("extrusion", e);
    Ok(())
}

// ---- 20.4.22–30 DIMENSION
fn dim_common(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<f64> {
    let o = &mut rec.f;
    if ctx.ver >= 24 {
        let v = r.rc()?;
        o.set("_dimVersion", v);
    }
    set_bd3(r, o, "extrusion")?;
    let tm = r.rd2_raw()?;
    let elev = r.bd()?;
    o.set("elevation", elev);
    o.set("textMid", p3(tm[0], tm[1], elev));
    let f1 = r.rc()?;
    o.set("_flags1", f1);
    let t = r.tv()?;
    o.set("text", t);
    set_bd(r, o, "textRotation")?;
    set_bd(r, o, "horizDir")?;
    set_bd3(r, o, "insScale")?;
    set_bd(r, o, "insRotation")?;
    if ctx.ver >= 15 {
        set_bs(r, o, "attach")?;
        set_bs(r, o, "lineSpacingStyle")?;
        set_bd(r, o, "lineSpacing")?;
        set_bd(r, o, "measurement")?;
    }
    if ctx.ver >= 21 {
        for k in ["_dimUnknown", "flipArrow1", "flipArrow2"] {
            let v = r.b()?;
            o.set(k, v);
        }
    }
    let p12 = r.rd2_raw()?;
    o.set("insert", p3(p12[0], p12[1], elev));
    Ok(elev)
}
fn dim_handles(h: &mut BitReader, rec: &mut Record) -> Res<()> {
    href(h, rec, "dimstyle")?;
    href(h, rec, "block")
}
/// dimCommon, `dimType`, then the listed BD3 points.
fn dim_points(r: &mut BitReader, rec: &mut Record, ctx: &Ctx, dim_type: u32, pts: &[&'static str]) -> Res<f64> {
    let elev = dim_common(r, rec, ctx)?;
    rec.f.set("dimType", dim_type);
    for k in pts {
        set_bd3(r, &mut rec.f, k)?;
    }
    Ok(elev)
}
fn dim_ordinate(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 6, &["defPoint", "xline1", "xline2"])?;
    let f2 = r.rc()?;
    rec.f.set("_flags2", f2);
    dim_handles(h, rec)
}
fn dim_linear(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 0, &["xline1", "xline2", "defPoint"])?;
    set_bd(r, &mut rec.f, "obliqueAngle")?;
    set_bd(r, &mut rec.f, "angle")?;
    dim_handles(h, rec)
}
fn dim_aligned(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 1, &["xline1", "xline2", "defPoint"])?;
    set_bd(r, &mut rec.f, "obliqueAngle")?;
    dim_handles(h, rec)
}
fn dim_ang3pt(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 5, &["defPoint", "xline1", "xline2", "center"])?;
    dim_handles(h, rec)
}
fn dim_ang2ln(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let elev = dim_points(r, rec, ctx, 2, &[])?;
    let p16 = r.rd2_raw()?;
    rec.f.set("arcPoint", p3(p16[0], p16[1], elev));
    for k in ["xline1", "xline2", "center", "defPoint"] {
        set_bd3(r, &mut rec.f, k)?;
    }
    dim_handles(h, rec)
}
fn dim_radius(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 4, &["defPoint", "center"])?;
    set_bd(r, &mut rec.f, "leaderLen")?;
    dim_handles(h, rec)
}
fn dim_diameter(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 3, &["center", "defPoint"])?;
    set_bd(r, &mut rec.f, "leaderLen")?;
    dim_handles(h, rec)
}
fn arc_dimension(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 8, &["defPoint", "xline1", "xline2", "center"])?;
    let o = &mut rec.f;
    let p = r.bool()?;
    o.set("isPartial", p);
    set_bd(r, o, "arcStart")?;
    set_bd(r, o, "arcEnd")?;
    let l = r.bool()?;
    o.set("hasLeader", l);
    set_bd3(r, o, "leader1")?;
    set_bd3(r, o, "leader2")?;
    dim_handles(h, rec)
}
fn large_radial_dimension(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dim_points(r, rec, ctx, 9, &["defPoint", "chordPoint"])?;
    r.bd()?;
    set_bd3(r, &mut rec.f, "overrideCenter")?;
    set_bd3(r, &mut rec.f, "jogPoint")?;
    dim_handles(h, rec)
}

// ---- 20.4.31/32/35/36 POINT 3DFACE SOLID TRACE
fn point(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    set_bd3(r, o, "position")?;
    let t = r.bt()?;
    o.set("thickness", t);
    let e = r.be()?;
    o.set("extrusion", e);
    set_bd(r, o, "xAxisAngle")
}
fn face3d(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    if ctx.ver < 15 {
        let a = r.bd3()?;
        let b = r.bd3()?;
        let c = r.bd3()?;
        let d = r.bd3()?;
        rec.f.set("points", vec![a, b, c, d]);
        return set_bs(r, &mut rec.f, "invisibleEdges");
    }
    let no_flags = r.b()?;
    let zzero = r.b()?;
    let x = r.rd()?;
    let y = r.rd()?;
    let z = if zzero == 0 { r.rd()? } else { 0.0 };
    let p1 = [x, y, z];
    let p2 = dd3_raw(r, p1)?;
    let p3r = dd3_raw(r, p2)?;
    let p4 = dd3_raw(r, p3r)?;
    let pt = |p: [f64; 3]| p3(p[0], p[1], p[2]);
    rec.f.set("points", vec![pt(p1), pt(p2), pt(p3r), pt(p4)]);
    let inv = if no_flags != 0 { 0 } else { r.bs()? };
    rec.f.set("invisibleEdges", inv);
    Ok(())
}
/// DD3 with a default point, as raw numbers (the next point's default).
fn dd3_raw(r: &mut BitReader, d: [f64; 3]) -> Res<[f64; 3]> {
    let x = r.dd(d[0])?;
    let y = r.dd(d[1])?;
    Ok([x, y, r.dd(d[2])?])
}
fn solid(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    let t = r.bt()?;
    o.set("thickness", t);
    let elev = r.bd()?;
    o.set("elevation", elev);
    let mut pts = Vec::with_capacity(4);
    for _ in 0..4 {
        pts.push(r.rd2_raw()?);
    }
    o.set("points", pts.iter().map(|p| p3(p[0], p[1], elev)).collect::<Vec<V>>());
    let e = r.be()?;
    o.set("extrusion", e);
    Ok(())
}
fn shape(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    set_bd3(r, o, "insert")?;
    for k in ["scale", "rotation", "widthFactor", "oblique", "thickness"] {
        set_bd(r, o, k)?;
    }
    set_bs(r, o, "shapeNumber")?;
    set_bd3(r, o, "extrusion")?;
    href(h, rec, "style")
}

// ---- 20.4.38 VIEWPORT
fn viewport(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let o = &mut rec.f;
    set_bd3(r, o, "center")?;
    set_bd(r, o, "width")?;
    set_bd(r, o, "height")?;
    let mut frozen = 0;
    if ver >= 15 {
        set_bd3(r, o, "viewTarget")?;
        set_bd3(r, o, "viewDirection")?;
        for k in ["twist", "viewHeight", "lensLength", "frontClip", "backClip", "snapAngle"] {
            set_bd(r, o, k)?;
        }
        for k in ["viewCenter", "snapBase", "snapSpacing", "gridSpacing"] {
            let v = r.rd2()?;
            o.set(k, v);
        }
        set_bs(r, o, "circleZoom")?;
        if ver >= 21 {
            set_bs(r, o, "gridMajor")?;
        }
        frozen = r.bl()?;
        let st = r.bl()?;
        o.set("status", st);
        let ss = r.tv()?;
        o.set("styleSheet", ss);
        let rm = r.rc()?;
        o.set("renderMode", rm);
        let a = r.bool()?;
        o.set("ucsAtOrigin", a);
        let b = r.bool()?;
        o.set("ucsPerViewport", b);
        set_bd3(r, o, "ucsOrigin")?;
        set_bd3(r, o, "ucsXAxis")?;
        set_bd3(r, o, "ucsYAxis")?;
        set_bd(r, o, "ucsElevation")?;
        set_bs(r, o, "ucsOrthoType")?;
        if ver >= 18 {
            set_bs(r, o, "shadePlotMode")?;
        }
        if ver >= 21 {
            let u = r.bool()?;
            o.set("useDefaultLights", u);
            let t = r.rc()?;
            o.set("defaultLightingType", t);
            set_bd(r, o, "brightness")?;
            set_bd(r, o, "contrast")?;
            let c = r.cmc()?;
            o.set("ambientColor", c);
        }
    }
    if ver < 15 {
        href(h, rec, "vpentHeader")?;
    }
    if ver >= 15 {
        let fl = hrefs(h, rec.handle, f64::from(frozen))?;
        rec.h.set("frozenLayers", fl);
        href(h, rec, "clipBoundary")?;
        if ver == 15 {
            href(h, rec, "vpentHeader")?;
        }
        href(h, rec, "namedUcs")?;
        href(h, rec, "baseUcs")?;
    }
    if ver >= 21 {
        for k in ["background", "visualStyle", "shadePlot", "sun"] {
            href(h, rec, k)?;
        }
    }
    Ok(())
}

// ---- 20.4.39/40 ELLIPSE SPLINE
fn ellipse(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    for k in ["center", "majorAxis", "extrusion"] {
        set_bd3(r, o, k)?;
    }
    for k in ["ratio", "startParam", "endParam"] {
        set_bd(r, o, k)?;
    }
    Ok(())
}
fn spline(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    let mut scenario = r.bl()?;
    if ctx.ver >= 27 {
        let f1 = r.bl()?;
        o.set("splineFlags1", f1);
        let kp = r.bl()?;
        o.set("knotParam", kp);
        // 20.4.40: from R2013 the stored scenario is superseded — fit data (method "fit points", flag 1) means
        // scenario 2 unless the knot parameter is Custom (15). Spec gap: the stored value cannot be trusted
        // (AutoCAD R2013 fit splines store 1) — found by experiment.
        scenario = if f1 & 1 != 0 && kp != 15 { 2 } else { 1 };
    }
    let deg = r.bl()?;
    o.set("degree", deg);
    let (mut nknots, mut nctrl, mut nfit, mut weighted) = (0i32, 0i32, 0i32, false);
    o.set("flags", 0);
    if scenario & 2 != 0 {
        set_bd(r, o, "fitTolerance")?;
        set_bd3(r, o, "startTangent")?;
        set_bd3(r, o, "endTangent")?;
        nfit = r.bl()?;
    }
    if scenario & 1 != 0 {
        for k in ["rational", "closed", "periodic"] {
            let v = r.bool()?;
            o.set(k, v);
        }
        set_bd(r, o, "knotTolerance")?;
        set_bd(r, o, "controlTolerance")?;
        nknots = r.bl()?;
        nctrl = r.bl()?;
        weighted = r.bool()?;
    }
    o.set("scenario", scenario);
    fits(r, f64::from(nknots), 2.0)?;
    fits(r, f64::from(nctrl), 6.0)?;
    fits(r, f64::from(nfit), 6.0)?;
    let (nk, nc, nf) = (usize_from_i32(nknots), usize_from_i32(nctrl), usize_from_i32(nfit));
    o.set("knots", vec![V::Null; nk]);
    for i in 0..nk {
        let v = r.bd()?;
        put(o, "knots", i, V::Num(v));
    }
    o.set("control", vec![V::Null; nc]);
    if weighted {
        o.set("weights", vec![V::Null; nc]);
    }
    for i in 0..nc {
        let p = r.bd3()?;
        put(o, "control", i, p);
        if weighted {
            let w = r.bd()?;
            put(o, "weights", i, V::Num(w));
        }
    }
    o.set("fit", vec![V::Null; nf]);
    for i in 0..nf {
        let p = r.bd3()?;
        put(o, "fit", i, p);
    }
    Ok(())
}

// ---- 20.4.42/43 RAY XLINE
fn ray(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    set_bd3(r, &mut rec.f, "start")?;
    set_bd3(r, &mut rec.f, "direction")
}

// ---- 20.4.46 MTEXT
fn mtext(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let handle = rec.handle;
    mtext_data(r, Some((h, &mut rec.h)), handle, &mut rec.f, ctx)
}
/// MTEXT data into `o`; `hs` (the handle reader and where its references go) is `None` for the MTEXT embedded
/// in an R2018 multi-line attribute (no handles read).
pub fn mtext_data(r: &mut BitReader, mut hs: Option<(&mut BitReader, &mut Obj)>, handle: f64, o: &mut Obj, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    for k in ["insert", "extrusion", "xAxis"] {
        set_bd3(r, o, k)?;
    }
    set_bd(r, o, "width")?;
    if ver >= 21 {
        set_bd(r, o, "rectHeight")?;
    }
    set_bd(r, o, "height")?;
    set_bs(r, o, "attach")?;
    set_bs(r, o, "direction")?;
    set_bd(r, o, "_extH")?;
    set_bd(r, o, "_extW")?;
    let v = r.tv()?;
    o.set("value", v);
    if let Some((h, ho)) = hs.as_mut() {
        let s = h.h(handle)?;
        ho.set("style", s);
    }
    if ver >= 15 {
        set_bs(r, o, "lineSpacingStyle")?;
        set_bd(r, o, "lineSpacing")?;
        r.b()?;
    }
    if ver >= 18 {
        let bg = r.bl()?;
        o.set("backgroundFlags", bg);
        if bg & 1 != 0 || (ver >= 32 && bg & 0x10 != 0) {
            // Spec gap: 20.4.46 types the background scale BL; it is a BD (DXF 45, default 1.5) — found by experiment.
            set_bd(r, o, "backgroundScale")?;
            let c = r.cmc()?;
            o.set("backgroundColor", c);
            let t = r.bl()?;
            o.set("backgroundTransparency", t);
        }
    }
    if ver >= 32 {
        let not_annotative = r.b()?;
        if not_annotative != 0 {
            r.bs()?;
            r.b()?;
            if let Some((h, ho)) = hs.as_mut() {
                let s = h.h(handle)?;
                ho.set("mtextApp", s);
            }
            r.bl()?;
            r.bd3()?;
            r.bd3()?;
            for _ in 0..4 {
                r.bd()?;
            }
            let col_type = r.bs()?;
            if col_type != 0 {
                o.set("columns", crate::obj! { "type" => col_type });
                let count = r.bl()?;
                col_field(o, "count", V::from(count));
                let width = r.bd()?;
                col_field(o, "width", V::Num(width));
                let gutter = r.bd()?;
                col_field(o, "gutter", V::Num(gutter));
                let auto_h = r.bool()?;
                col_field(o, "autoHeight", V::Bool(auto_h));
                let rev = r.bool()?;
                col_field(o, "flowReversed", V::Bool(rev));
                if !auto_h && col_type == 2 {
                    col_field(o, "heights", V::Arr(Vec::new()));
                    for _ in 0..count.max(0) {
                        let v = r.bd()?;
                        if let Some(c) = o.obj_mut("columns") {
                            push(c, "heights", V::Num(v));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
/// `rec.columns.<k> = v`.
fn col_field(o: &mut Obj, k: &'static str, v: V) {
    if let Some(c) = o.obj_mut("columns") {
        c.set(k, v);
    }
}

// ---- 20.4.47 LEADER
fn leader(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let o = &mut rec.f;
    r.b()?;
    set_bs(r, o, "annotType")?;
    set_bs(r, o, "pathType")?;
    let n = r.bl()?;
    fits(r, f64::from(n), 6.0)?;
    o.set("vertices", V::Arr(Vec::new()));
    for _ in 0..n.max(0) {
        let p = r.bd3()?;
        push(o, "vertices", p);
    }
    for k in ["origin", "extrusion", "xDirection", "blockOffset"] {
        set_bd3(r, o, k)?;
    }
    if ver >= 14 {
        set_bd3(r, o, "endPointProj")?;
    }
    if ver < 15 {
        set_bd(r, o, "dimgap")?;
    }
    // Spec gap: R2010+ files have no box height/width here (13 bits follow the block offset whether or not the
    // leader has an annotation) — found with leaders saved by the ODA File Converter in R2007 and R2010.
    if ver < 24 {
        set_bd(r, o, "boxHeight")?;
        set_bd(r, o, "boxWidth")?;
    }
    let hk = r.bool()?;
    o.set("hookLineOnX", hk);
    let ah = r.bool()?;
    o.set("arrowhead", ah);
    if ver < 15 {
        r.bs()?;
        r.bd()?;
        r.b()?;
        r.b()?;
        r.bs()?;
        r.bs()?;
        r.b()?;
        r.b()?;
    } else {
        r.bs()?;
        r.b()?;
        r.b()?;
    }
    href(h, rec, "annotation")?;
    href(h, rec, "dimstyle")
}

// ---- 20.4.49 TOLERANCE, 20.4.50 MLINE
fn tolerance(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    if ctx.ver < 15 {
        r.bs()?;
        set_bd(r, o, "height")?;
        set_bd(r, o, "dimgap")?;
    }
    for k in ["insert", "xDirection", "extrusion"] {
        set_bd3(r, o, k)?;
    }
    let t = r.tv()?;
    o.set("text", t);
    href(h, rec, "dimstyle")
}
fn mline(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let o = &mut rec.f;
    set_bd(r, o, "scale")?;
    let j = r.rc()?;
    o.set("justification", j);
    set_bd3(r, o, "base")?;
    set_bd3(r, o, "extrusion")?;
    let closed = r.bs()? == 3;
    o.set("closed", closed);
    let lines = r.rc()?;
    let n = r.bs()?;
    o.set("vertices", V::Arr(Vec::new()));
    for _ in 0..n.max(0) {
        let point = r.bd3()?;
        let direction = r.bd3()?;
        let miter = r.bd3()?;
        let mut ls = Vec::new();
        for _ in 0..lines {
            let ns = r.bs()?;
            let mut seg = Vec::new();
            for _ in 0..ns.max(0) {
                seg.push(V::Num(r.bd()?));
            }
            let nf = r.bs()?;
            let mut fill = Vec::new();
            for _ in 0..nf.max(0) {
                fill.push(V::Num(r.bd()?));
            }
            ls.push(V::Obj(crate::obj! { "segments" => seg, "fill" => fill }));
        }
        push(o, "vertices", V::Obj(crate::obj! { "point" => point, "direction" => direction, "miter" => miter, "lines" => ls }));
    }
    href(h, rec, "style")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::Codepage;
    use std::collections::HashMap;

    fn ctx(ver: u32) -> Ctx {
        Ctx { ver, maint: 0, cp: Codepage::latin1(), classes: HashMap::new(), strict: false }
    }

    #[test]
    fn ray_from_default_bitdoubles() {
        // six BDs coded "01" (= 1.0)
        let buf = [0b0101_0101, 0b0101_0000];
        let hbuf = [0u8; 1];
        let mut r = BitReader::new(&buf, 15, Codepage::latin1());
        let mut h = BitReader::new(&hbuf, 15, Codepage::latin1());
        let mut rec = Record::default();
        let dec = decoder("XLINE").unwrap();
        dec(&mut r, &mut h, &mut rec, &ctx(15)).unwrap();
        assert_eq!(rec.f.get("start"), Some(&p3(1.0, 1.0, 1.0)));
        assert_eq!(rec.f.get("direction"), Some(&p3(1.0, 1.0, 1.0)));
    }

    #[test]
    fn spline_failure_leaves_holes() {
        // R2000 scenario 2 (BL "01"+RC 2), degree BL "10" (0), fit tolerance BD "10" (0), tangents 6 × BD "10",
        // fit count BL 1 then the stream ends: fit = [null] as JS `new Array(1)` leaves it
        let mut bits = String::from("0100000010");
        bits.push_str("10"); // degree 0
        bits.push_str("10"); // fitTolerance 0
        bits.push_str(&"10".repeat(6));
        bits.push_str("0100000001"); // nfit = 1
        bits.push_str("11000000"); // a BD of code 3: error (and enough bits for the count check)
        while bits.len() % 8 != 0 {
            bits.push('0');
        }
        let buf: Vec<u8> = bits.as_bytes().chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | (b - b'0'))).collect();
        let mut r = BitReader::new(&buf, 15, Codepage::latin1());
        let hbuf = [0u8; 1];
        let mut h = BitReader::new(&hbuf, 15, Codepage::latin1());
        let mut rec = Record::default();
        assert!(spline(&mut r, &mut h, &mut rec, &ctx(15)).is_err());
        assert_eq!(rec.f.num("scenario"), Some(2.0));
        assert_eq!(rec.f.get("fit"), Some(&V::Arr(vec![V::Null])));
    }

    #[test]
    fn helpers() {
        let mut o = Obj::new();
        o.set("a", V::Arr(vec![V::Null, V::Null]));
        put(&mut o, "a", 1, V::Num(3.0));
        put(&mut o, "a", 5, V::Num(9.0));
        push(&mut o, "a", V::Bool(true));
        push(&mut o, "missing", V::Bool(true));
        assert_eq!(o.get("a"), Some(&V::Arr(vec![V::Null, V::Num(3.0), V::Bool(true)])));
        assert!(!o.has("missing"));
        assert!(decoder("DICTIONARY").is_none());
        assert!(decoder("LARGE_RADIAL_DIMENSION").is_some());
    }
}
