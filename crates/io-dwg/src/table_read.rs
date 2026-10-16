//! ACAD_TABLE and its helper objects, decoded (port of lib/dwg/table-read.js, cc-dwg, Apache-2.0).
//!
//! Written from the ODA "Open Design Specification for .dwg files" v5.4.1: 20.4.96 TABLE (until R21, and R24+),
//! 20.4.97 TABLECONTENT, 20.4.98 cell content geometry, 20.4.99 value, 20.4.100 custom data, 20.4.101 TABLESTYLE
//! (until R21, R24) with 20.4.101.3 content format and 20.4.101.4 cell style, 20.4.102 CELLSTYLEMAP,
//! 20.4.103 TABLEGEOMETRY, 20.4.76 FIELD, 20.4.77 FIELDLIST. Where the spec is wrong or silent the layout was
//! found by experiment on the corpus; those places are marked "Spec gap:".
//!
//! * [`decoder`]: ACAD_TABLE, TABLESTYLE, CELLSTYLEMAP, TABLECONTENT, TABLEGEOMETRY, FIELD, FIELDLIST (these win
//!   over objects-read.js's own ACAD_TABLE / FIELDLIST). The decoded data is kept in `rec.f` under the JS
//!   record's keys (private ones start with `_`; handle references are absolute numbers under `_` keys).
//! * [`table_entity_fields`]: the model fields of an ACAD_TABLE entity (`f.table = {rows, columns, cells, …}`).
//! * [`table_object_fields`]: the same for TABLESTYLE / CELLSTYLEMAP / TABLECONTENT / TABLEGEOMETRY / FIELD.

use crate::bits::{BitReader, DwgError, Res, err};
use crate::messages::msg;
use crate::objects::{Ctx, Decoder, Record};
use crate::read::Build;
use crate::util::{handle_hex as hx, to_i32, usize_from_i32};
use crate::value::{Obj, V};

pub fn decoder(name: &str) -> Option<Decoder> {
    Some(match name {
        "ACAD_TABLE" => acad_table,
        "TABLESTYLE" => table_style,
        "CELLSTYLEMAP" => cell_style_map,
        "TABLECONTENT" => table_content,
        "TABLEGEOMETRY" => table_geometry,
        "FIELD" => field,
        "FIELDLIST" => field_list,
        _ => return None,
    })
}

// ================================================================ reading helpers

/// Colours of the table objects. Spec gap: R2000 files (written by the ODA File Converter) store the R2004 form
/// (BS index, BL rgb word, RC name flags) in TABLESTYLE / CELLSTYLEMAP / TABLECONTENT, not the R2000 BS index —
/// found by experiment (the R2000 objects decode to the end only so, and match the converter's DXF).
fn cmc(r: &mut BitReader) -> Res<V> {
    if r.version >= 18 {
        return Ok(V::Obj(r.cmc()?));
    }
    let v = r.version;
    r.version = 18;
    let out = r.cmc();
    r.version = v;
    Ok(V::Obj(out?))
}

/// A count of items of at least `min_bits` each must fit in what is left of the stream.
fn fits(r: &BitReader, n: i32, min_bits: f64) -> Res<usize> {
    crate::objects::fits(r, f64::from(n), min_bits)?;
    Ok(usize_from_i32(n))
}

/// `o.k = []` followed by pushes: whatever was pushed stays even when a read fails part way.
fn guard<T>(o: &mut Obj, k: &'static str, f: impl FnOnce(&mut Vec<V>) -> Res<T>) -> Res<T> {
    let mut v = Vec::new();
    let res = f(&mut v);
    o.set(k, v);
    res
}

/// `r.BS() << 16 >> 16`
fn sext16(x: i32) -> i32 {
    let y = x & 0xFFFF;
    if y & 0x8000 != 0 { y | !0xFFFF } else { y }
}

fn bit(base: i32, i: u32) -> i32 {
    base.checked_shl(i).unwrap_or(0)
}

fn utf16(b: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(b.len() / 2);
    for [lo, hi] in b.as_chunks::<2>().0 {
        let c = u16::from(*lo) | (u16::from(*hi) << 8);
        if c == 0 {
            break;
        }
        units.push(c);
    }
    char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect()
}

// ---------------------------------------------------------------- 20.4.99 value
/// A value (20.4.99). R2007+: flags BL, data type BL, the data unless flags & 1 (kGeneral), unit type BL, format
/// string TV, value string TV. Object ids (type 64) come from the handle stream.
fn read_value(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<Obj> {
    let mut v = Obj::new();
    let mut flags = 0;
    if ver >= 21 {
        flags = r.bl()?;
        v.set("flags", flags);
    }
    let dt = r.bl()?;
    v.set("dataType", dt);
    if !(ver >= 21 && flags & 1 != 0) {
        match dt {
            0 | 1 => v.set("value", r.bl()?),
            2 => v.set("value", r.bd()?),
            4 => {
                // Spec gap: from R2007 a string value is not a TV but BL byte count + UTF-16LE text with its NUL, in the
                // data stream (found by experiment: "test" = 10 bytes in example_2000 saved as R2007).
                if ver >= 21 {
                    let n = r.bl()?;
                    let n = fits(r, n, 8.0)?;
                    let b = r.bytes(n)?;
                    v.set("value", utf16(&b));
                } else {
                    v.set("value", r.tv()?);
                }
            }
            8 | 128 | 256 | 512 => {
                let n = r.bl()?;
                let n = fits(r, n, 8.0)?;
                let b = r.bytes(n)?;
                v.set("bytes", crate::util::hex_bytes(&b));
            }
            16 => {
                let n = r.bl()?;
                let n = fits(r, n, 8.0)?;
                if n >= 16 {
                    v.set("value", r.rd2()?);
                } else {
                    r.bytes(n)?;
                    v.set("value", V::Null);
                }
            }
            32 => {
                let n = r.bl()?;
                let n = fits(r, n, 8.0)?;
                if n >= 24 {
                    v.set("value", r.rd3()?);
                } else {
                    r.bytes(n)?;
                    v.set("value", V::Null);
                }
            }
            64 => v.set("_id", h.h(hd)?),
            _ => return err(msg("ccdwg.err.value_type", &[("type", &dt.to_string())])),
        }
    }
    if ver >= 21 {
        v.set("unitType", r.bl()?);
        v.set("format", r.tv()?);
        v.set("text", r.tv()?);
    }
    Ok(v)
}

// ---------------------------------------------------------------- 20.4.100 custom data
fn read_custom_data(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<V> {
    let n = r.bl()?;
    let n = fits(r, n, 2.0)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let name = r.tv()?;
        let value = read_value(r, h, hd, ver)?;
        out.push(V::Obj(crate::obj! { "name" => name, "value" => value }));
    }
    Ok(V::Arr(out))
}

// ---------------------------------------------------------------- 20.4.101.3 content format, 20.4.101.4 cell style
fn read_content_format(r: &mut BitReader, h: &mut BitReader, hd: f64) -> Res<Obj> {
    let mut o = Obj::new();
    o.set("overrides", r.bl()?);
    o.set("propertyFlags", r.bl()?);
    o.set("dataType", r.bl()?);
    o.set("unitType", r.bl()?);
    o.set("format", r.tv()?);
    o.set("rotation", r.bd()?);
    o.set("blockScale", r.bd()?);
    o.set("alignment", r.bl()?);
    o.set("color", cmc(r)?);
    o.set("_textStyle", h.h(hd)?);
    o.set("textHeight", r.bd()?);
    Ok(o)
}
fn read_cell_style(r: &mut BitReader, h: &mut BitReader, hd: f64) -> Res<Obj> {
    let mut s = Obj::new();
    s.set("styleType", r.bl()?);
    let has = r.bs()?;
    s.set("hasData", has);
    if has == 0 {
        return Ok(s);
    }
    s.set("overrides", r.bl()?);
    s.set("mergeFlags", r.bl()?);
    s.set("background", cmc(r)?);
    s.set("contentLayout", r.bl()?);
    s.set("content", read_content_format(r, h, hd)?);
    let mf = r.bs()?;
    s.set("marginFlags", mf);
    if mf & 1 != 0 {
        let mut m = Obj::new();
        for k in ["vertical", "horizontal", "bottom", "right", "hSpacing", "vSpacing"] {
            m.set(k, r.bd()?);
        }
        s.set("margins", m);
    }
    let nb = r.bl()?;
    let nb = fits(r, nb, 2.0)?;
    let mut borders = Vec::new();
    for _ in 0..nb {
        let mut b = Obj::new();
        let edge = r.bl()?;
        b.set("edge", edge);
        if edge != 0 {
            b.set("overrides", r.bl()?);
            b.set("borderType", r.bl()?);
            b.set("color", cmc(r)?);
            b.set("lineweight", r.bl()?);
            b.set("_linetype", h.h(hd)?);
            b.set("invisible", r.bl()?);
            b.set("doubleSpacing", r.bd()?);
        }
        borders.push(V::Obj(b));
    }
    s.set("borders", borders);
    Ok(s)
}
/// A cell style with its id, class and name (the cell style map and the R24 table style).
fn read_named_cell_style(r: &mut BitReader, h: &mut BitReader, hd: f64) -> Res<Obj> {
    let mut s = read_cell_style(r, h, hd)?;
    s.set("id", r.bl()?);
    s.set("styleClass", r.bl()?);
    s.set("name", r.tv()?);
    Ok(s)
}

// ---------------------------------------------------------------- 20.4.97 table content (without the common object data)
fn read_table_content(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<Obj> {
    let mut t = Obj::new();
    t.set("name", r.tv()?);
    t.set("description", r.tv()?);
    let nc = r.bl()?;
    let nc = fits(r, nc, 8.0)?;
    let mut columns = Vec::new();
    for _ in 0..nc {
        let mut c = Obj::new();
        c.set("name", r.tv()?);
        c.set("customData", r.bl()?);
        c.set("custom", read_custom_data(r, h, hd, ver)?);
        c.set("style", read_cell_style(r, h, hd)?);
        c.set("styleId", r.bl()?);
        c.set("width", r.bd()?);
        columns.push(V::Obj(c));
    }
    t.set("columns", columns);
    let nr = r.bl()?;
    let nr = fits(r, nr, 8.0)?;
    let mut rows = Vec::new();
    for _ in 0..nr {
        let n = r.bl()?;
        let n = fits(r, n, 8.0)?;
        let mut cells = Vec::new();
        for _ in 0..n {
            cells.push(V::Obj(read_content_cell(r, h, hd, ver)?));
        }
        let mut row = crate::obj! { "cells" => cells };
        row.set("customData", r.bl()?);
        row.set("custom", read_custom_data(r, h, hd, ver)?);
        row.set("style", read_cell_style(r, h, hd)?);
        row.set("styleId", r.bl()?);
        row.set("height", r.bd()?);
        rows.push(V::Obj(row));
    }
    t.set("rows", rows);
    let nf = r.bl()?;
    let nf = fits(r, nf, 0.0)?;
    let mut fields = Vec::new();
    for _ in 0..nf {
        fields.push(V::Num(h.h(hd)?));
    }
    t.set("_fields", fields);
    t.set("tableStyle", read_cell_style(r, h, hd)?); // AcDbFormattedTableData: the table's cell style overrides
    let nm = r.bl()?;
    let nm = fits(r, nm, 8.0)?;
    let mut merged = Vec::new();
    for _ in 0..nm {
        let mut m = Obj::new();
        for k in ["top", "left", "bottom", "right"] {
            m.set(k, r.bl()?);
        }
        merged.push(V::Obj(m));
    }
    t.set("merged", merged);
    t.set("_style", h.h(hd)?);
    Ok(t)
}
fn read_content_cell(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<Obj> {
    let mut c = Obj::new();
    c.set("state", r.bl()?);
    c.set("tooltip", r.tv()?);
    c.set("customData", r.bl()?);
    c.set("custom", read_custom_data(r, h, hd, ver)?);
    let link = r.bl()?;
    c.set("hasLink", link);
    if link != 0 {
        c.set("_link", h.h(hd)?);
        c.set("linkRows", r.bl()?);
        c.set("linkCols", r.bl()?);
        c.set("linkUnknown", r.bl()?);
    }
    let ncont = r.bl()?;
    let ncont = fits(r, ncont, 2.0)?;
    let mut contents = Vec::new();
    for _ in 0..ncont {
        let mut ct = Obj::new();
        let ty = r.bl()?;
        ct.set("type", ty);
        match ty {
            1 => ct.set("value", read_value(r, h, hd, ver)?),
            2 => ct.set("_field", h.h(hd)?),
            4 => ct.set("_block", h.h(hd)?),
            _ => {}
        }
        let na = r.bl()?;
        let na = fits(r, na, 2.0)?;
        let mut attrs = Vec::new();
        for _ in 0..na {
            let a = h.h(hd)?;
            let value = r.tv()?;
            let index = r.bl()?;
            attrs.push(V::Obj(crate::obj! { "_attdef" => a, "value" => value, "index" => index }));
        }
        ct.set("attrs", attrs);
        let hf = r.bs()?;
        ct.set("hasFormat", hf);
        if hf != 0 {
            ct.set("format", read_content_format(r, h, hd)?);
        }
        contents.push(V::Obj(ct));
    }
    c.set("contents", contents);
    c.set("style", read_cell_style(r, h, hd)?);
    c.set("styleId", r.bl()?);
    let gf = r.bl()?;
    c.set("geomFlag", gf);
    if gf != 0 {
        c.set("geomUnknown", r.bl()?);
        c.set("geomA", r.bd()?);
        c.set("geomB", r.bd()?);
        let gdf = r.bl()?;
        c.set("geomDataFlags", gdf);
        c.set("_geomUnknown", h.h(hd)?);
        if gdf != 0 {
            c.set("geometry", read_content_geometry(r, "flags")?);
        }
    }
    Ok(c)
}
fn read_content_geometry(r: &mut BitReader, last: &'static str) -> Res<Obj> {
    let mut o = Obj::new();
    o.set("distTopLeft", r.bd3()?);
    o.set("distCenter", r.bd3()?);
    o.set("contentWidth", r.bd()?);
    o.set("contentHeight", r.bd()?);
    o.set("width", r.bd()?);
    o.set("height", r.bd()?);
    o.set(last, r.bl()?);
    Ok(o)
}

// ---------------------------------------------------------------- 20.4.96 TABLE
fn insert_part(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ver: u32) -> Res<()> {
    let hd = rec.handle;
    rec.f.set("insert", r.bd3()?);
    let scale = if ver < 15 {
        let x = r.bd()?;
        let y = r.bd()?;
        crate::value::p3(x, y, r.bd()?)
    } else {
        match r.bb()? {
            3 => crate::value::p3(1.0, 1.0, 1.0),
            1 => {
                let y = r.dd(1.0)?;
                crate::value::p3(1.0, y, r.dd(1.0)?)
            }
            2 => {
                let x = r.rd()?;
                crate::value::p3(x, x, x)
            }
            _ => {
                let x = r.rd()?;
                let y = r.dd(x)?;
                crate::value::p3(x, y, r.dd(x)?)
            }
        }
    };
    rec.f.set("scale", scale);
    rec.f.set("rotation", r.bd()?);
    rec.f.set("extrusion", r.bd3()?);
    let has = r.b()?;
    rec.f.set("_hasAttribs", has);
    rec.f.set("_owned", 0);
    let mut owned = 0;
    if ver >= 18 && has != 0 {
        owned = r.bl()?;
        rec.f.set("_owned", owned);
    }
    rec.h.set("block", h.h(hd)?);
    if has != 0 {
        if ver < 18 {
            rec.h.set("firstAttrib", h.h(hd)?);
            rec.h.set("lastAttrib", h.h(hd)?);
        } else {
            guard(&mut rec.h, "attribs", |a| {
                for _ in 0..owned.max(0) {
                    a.push(V::Num(h.h(hd)?));
                }
                Ok(())
            })?;
        }
        rec.h.set("seqend", h.h(hd)?);
    }
    Ok(())
}

const SIDE_MASKS: [(i32, &str, i32, i32, i32); 4] = [
    (1, "top", 0x40, 0x400, 0x4000),
    (2, "right", 0x80, 0x800, 0x8000),
    (4, "bottom", 0x100, 0x1000, 0x10000),
    (8, "left", 0x200, 0x2000, 0x20000),
];

fn read_old_cell(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<Obj> {
    let mut c = Obj::new();
    let ty = r.bs()?;
    c.set("type", ty);
    c.set("edgeFlags", r.rc()?);
    c.set("merged", r.b()?);
    c.set("autoFit", r.b()?);
    c.set("mergedWidth", r.bl()?);
    c.set("mergedHeight", r.bl()?);
    c.set("rotation", r.bd()?);
    let value = h.h(hd)?;
    c.set("_value", value);
    if ty == 1 {
        // Spec gap: from R2007 a text cell stores no text of its own; the cell value (below) holds it (found by
        // experiment: the string stream of example_2000 saved as R2007 holds 2 strings per cell, not 3).
        if value == 0.0 && ver < 21 {
            c.set("text", r.tv()?);
        }
    } else if ty == 2 {
        c.set("blockScale", r.bd()?);
        let ha = r.b()?;
        c.set("hasAttrs", ha);
        if ha != 0 {
            let n = r.bs()?;
            let mut attrs = Vec::new();
            for _ in 0..n.max(0) {
                let a = h.h(hd)?;
                let index = r.bs()?;
                let v = r.tv()?;
                attrs.push(V::Obj(crate::obj! { "_attdef" => a, "index" => index, "value" => v }));
            }
            c.set("attrs", attrs);
        }
    }
    let ho = r.b()?;
    c.set("hasOverrides", ho);
    if ho != 0 {
        let f = r.bl()?;
        c.set("overrideFlags", f);
        let ve = r.rc()?;
        c.set("virtualEdges", ve);
        if f & 0x01 != 0 {
            c.set("alignment", r.bs()?); // Spec gap: the spec says RS; it is a BS (found by experiment: blocks_and_tables)
        }
        if f & 0x02 != 0 {
            c.set("fillNone", r.b()?);
        }
        if f & 0x04 != 0 {
            c.set("background", cmc(r)?);
        }
        if f & 0x08 != 0 {
            c.set("contentColor", cmc(r)?);
        }
        if f & 0x10 != 0 {
            c.set("_textStyle", h.h(hd)?);
        }
        if f & 0x20 != 0 {
            c.set("textHeight", r.bd()?);
        }
        let mut borders = Obj::new();
        // Spec gap: on an edge marked virtual (178) the border flags stay set but no values are stored: they are the
        // adjacent cell's (its opposite edge) — found by having the ODA File Converter write a border override.
        for (vbit, side, mc, mlw, mv) in SIDE_MASKS {
            if to_i32(f64::from(ve)) & vbit != 0 {
                continue;
            }
            let mut b = Obj::new();
            if f & mc != 0 {
                b.set("color", cmc(r)?);
            }
            if f & mlw != 0 {
                b.set("lineweight", sext16(r.bs()?));
            }
            // Spec gap: 1 = invisible (as the table-wide flags; the ODA File Converter turns 1 into R2010+ "invisible")
            if f & mv != 0 {
                b.set("visible", r.bs()? == 0);
            }
            if !b.is_empty() {
                borders.set(side, b);
            }
        }
        c.set("borders", borders);
    }
    if ver >= 21 {
        c.set("unknown", r.bl()?);
        c.set("value", read_value(r, h, hd, ver)?);
    }
    Ok(c)
}

/// R13–R21 table data: the grid, the cells and the table-wide overrides.
fn old_table(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ver: u32, t: &mut Obj) -> Res<()> {
    let hd = rec.handle;
    t.set("flags", r.bs()?);
    rec.f.set("horizDir", r.bd3()?);
    let nc = r.bl()?;
    let nc = fits(r, nc, 2.0)?;
    let nr = r.bl()?;
    let nr = fits(r, nr, 2.0)?;
    guard(t, "colWidths", |a| {
        for _ in 0..nc {
            a.push(V::Num(r.bd()?));
        }
        Ok(())
    })?;
    guard(t, "rowHeights", |a| {
        for _ in 0..nr {
            a.push(V::Num(r.bd()?));
        }
        Ok(())
    })?;
    rec.h.set("tableStyle", h.h(hd)?);
    guard(t, "cells", |a| {
        for _ in 0..nr.saturating_mul(nc) {
            a.push(V::Obj(read_old_cell(r, h, hd, ver)?));
        }
        Ok(())
    })?;
    // table-wide overrides
    let ho = r.b()?;
    t.set("hasOverrides", ho);
    if ho != 0 {
        let f = r.bl()?;
        t.set("overrideFlags", f);
        let mut o = Obj::new();
        let res = (|| -> Res<()> {
            if f & 1 != 0 {
                o.set("titleSuppressed", r.b()?);
            }
            if f & 4 != 0 {
                o.set("flowDirection", r.bs()?);
            }
            if f & 8 != 0 {
                o.set("hMargin", r.bd()?);
            }
            if f & 0x10 != 0 {
                o.set("vMargin", r.bd()?);
            }
            let rows = ["title", "header", "data"];
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x20, i) != 0 {
                    o.set(format!("{n}Color"), cmc(r)?);
                }
            }
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x100, i) != 0 {
                    o.set(format!("{n}FillNone"), r.b()?);
                }
            }
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x800, i) != 0 {
                    o.set(format!("{n}Fill"), cmc(r)?);
                }
            }
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x4000, i) != 0 {
                    o.set(format!("{n}Align"), r.bs()?);
                }
            }
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x20000, i) != 0 {
                    o.set(format!("_{n}Style"), h.h(hd)?);
                }
            }
            for (i, n) in (0u32..).zip(rows) {
                if f & bit(0x100000, i) != 0 {
                    o.set(format!("{n}Height"), r.bd()?);
                }
            }
            Ok(())
        })();
        t.set("overrides", o);
        res?;
    }
    let bc = old_border(r, cmc)?;
    t.set_opt("borderColors", bc);
    let bl = old_border(r, |r| Ok(V::from(sext16(r.bs()?))))?;
    t.set_opt("borderLineweights", bl);
    let bv = old_border(r, |r| Ok(V::from(r.bs()?)))?;
    t.set_opt("borderVisibility", bv);
    Ok(())
}
fn old_border(r: &mut BitReader, read: impl Fn(&mut BitReader) -> Res<V>) -> Res<Option<V>> {
    if r.b()? == 0 {
        return Ok(None);
    }
    let f = r.bl()?;
    let mut o = crate::obj! { "flags" => f };
    for i in 0..18u32 {
        if f & bit(1, i) != 0 {
            o.set(i.to_string(), read(r)?);
        }
    }
    Ok(Some(V::Obj(o)))
}

/// R24+ table: the INSERT part, a few unknowns, the table content inline, then the break data.
fn new_table(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ver: u32, t: &mut Obj) -> Res<()> {
    let hd = rec.handle;
    t.set("unknownRC", r.rc()?);
    t.set("_unknownH", h.h(hd)?);
    t.set("unknownBL", r.bl()?);
    if ver == 24 {
        t.set("unknownB", r.b()?);
    } else {
        t.set("unknownBL2", r.bl()?);
    }
    t.set("content", read_table_content(r, h, hd, ver)?);
    t.set("unknownBS", r.bs()?);
    rec.f.set("horizDir", r.bd3()?);
    let hb = r.bl()?;
    t.set("hasBreak", hb);
    if hb != 0 {
        let mut b = Obj::new();
        b.set("options", r.bl()?);
        b.set("flow", r.bl()?);
        b.set("spacing", r.bd()?);
        b.set("unknown1", r.bl()?);
        b.set("unknown2", r.bl()?);
        let res = (|| -> Res<()> {
            let n = r.bl()?;
            let n = fits(r, n, 8.0)?;
            guard(&mut b, "heights", |a| {
                for _ in 0..n {
                    let position = r.bd3()?;
                    let height = r.bd()?;
                    let flags = r.bl()?;
                    a.push(V::Obj(crate::obj! { "position" => position, "height" => height, "flags" => flags }));
                }
                Ok(())
            })
        })();
        t.set("break", b);
        res?;
    }
    let n = r.bl()?;
    let n = fits(r, n, 8.0)?;
    guard(t, "rowRanges", |a| {
        for _ in 0..n {
            let position = r.bd3()?;
            let start = r.bl()?;
            let end = r.bl()?;
            a.push(V::Obj(crate::obj! { "position" => position, "start" => start, "end" => end }));
        }
        Ok(())
    })
}

fn acad_table(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    insert_part(r, h, rec, ctx.ver)?;
    let mut t = Obj::new();
    let res = if ctx.ver >= 24 { new_table(r, h, rec, ctx.ver, &mut t) } else { old_table(r, h, rec, ctx.ver, &mut t) };
    rec.f.set("_table", t);
    res
}

// ---------------------------------------------------------------- 20.4.101 TABLESTYLE
fn table_style(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let hd = rec.handle;
    let ver = ctx.ver;
    if ver >= 24 {
        rec.f.set("unknownRC", r.rc()?);
        rec.f.set("description", r.tv()?);
        rec.f.set("unknownBL1", r.bl()?);
        rec.f.set("unknownBL2", r.bl()?);
        rec.h.set("unknown", h.h(hd)?);
        rec.f.set("tableCellStyle", read_named_cell_style(r, h, hd)?);
        // Spec gap: each of the cell styles that follow is preceded by a BL equal to its id (1 title, 2 header,
        // 3 data) — found by experiment (the R2010–R2018 table styles of the corpus decode to the end only so).
        let n = r.bl()?;
        let n = fits(r, n, 8.0)?;
        guard(&mut rec.f, "cellStyles", |a| {
            for _ in 0..n {
                let pre = r.bl()?;
                let mut cs = read_named_cell_style(r, h, hd)?;
                cs.set("preId", pre);
                a.push(V::Obj(cs));
            }
            Ok(())
        })?;
    } else {
        rec.f.set("description", r.tv()?);
        rec.f.set("flowDirection", r.bs()?);
        rec.f.set("flags", r.bs()?);
        rec.f.set("hMargin", r.bd()?);
        rec.f.set("vMargin", r.bd()?);
        rec.f.set("titleSuppressed", r.b()?);
        rec.f.set("headerSuppressed", r.b()?);
        guard(&mut rec.f, "rowStyles", |a| {
            for _ in 0..3 {
                let mut s = Obj::new();
                s.set("_textStyle", h.h(hd)?);
                s.set("textHeight", r.bd()?);
                s.set("alignment", r.bs()?);
                s.set("textColor", cmc(r)?);
                s.set("fillColor", cmc(r)?);
                s.set("fillEnabled", r.b()?);
                let mut borders = Vec::new();
                for _ in 0..6 {
                    let lw = sext16(r.bs()?);
                    let vis = r.b()?;
                    let color = cmc(r)?;
                    borders.push(V::Obj(crate::obj! { "lineweight" => lw, "visible" => vis, "color" => color }));
                }
                s.set("borders", borders);
                if ver >= 21 {
                    s.set("dataType", r.bl()?);
                    s.set("unitType", r.bl()?);
                    s.set("format", r.tv()?);
                }
                a.push(V::Obj(s));
            }
            Ok(())
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------- 20.4.102 CELLSTYLEMAP
fn cell_style_map(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let hd = rec.handle;
    let n = r.bl()?;
    let n = fits(r, n, 8.0)?;
    guard(&mut rec.f, "cellStyles", |a| {
        for _ in 0..n {
            a.push(V::Obj(read_named_cell_style(r, h, hd)?));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------- 20.4.97 TABLECONTENT (R21 round-trip object)
fn table_content(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let c = read_table_content(r, h, rec.handle, ctx.ver)?;
    rec.f.set("_content", c);
    Ok(())
}

// ---------------------------------------------------------------- 20.4.103 TABLEGEOMETRY
fn table_geometry(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let hd = rec.handle;
    rec.f.set("rowCount", r.bl()?);
    rec.f.set("colCount", r.bl()?);
    let n = r.bl()?;
    let n = fits(r, n, 8.0)?;
    guard(&mut rec.f, "cells", |a| {
        for _ in 0..n {
            let mut c = Obj::new();
            c.set("flags", r.bl()?);
            c.set("widthWithGap", r.bd()?);
            c.set("heightWithGap", r.bd()?);
            c.set("_unknown", h.h(hd)?);
            let k = r.bl()?;
            let k = fits(r, k, 8.0)?;
            let mut contents = Vec::new();
            for _ in 0..k {
                contents.push(V::Obj(read_content_geometry(r, "unknown")?));
            }
            c.set("contents", contents);
            a.push(V::Obj(c));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------- 20.4.76 FIELD, 20.4.77 FIELDLIST
fn field(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let hd = rec.handle;
    let ver = ctx.ver;
    rec.f.set("evaluatorId", r.tv()?);
    rec.f.set("code", r.tv()?);
    let nc = r.bl()?;
    let nc = fits(r, nc, 0.0)?;
    guard(&mut rec.h, "children", |a| {
        for _ in 0..nc {
            a.push(V::Num(h.h(hd)?));
        }
        Ok(())
    })?;
    let no = r.bl()?;
    let no = fits(r, no, 0.0)?;
    guard(&mut rec.h, "objects", |a| {
        for _ in 0..no {
            a.push(V::Num(h.h(hd)?));
        }
        Ok(())
    })?;
    if ver < 21 {
        rec.f.set("format", r.tv()?);
    }
    for k in ["evalOptions", "filingOptions", "state", "evalStatus", "evalError"] {
        rec.f.set(k, r.bl()?);
    }
    rec.f.set("evalMessage", r.tv()?);
    rec.f.set("value", read_value(r, h, hd, ver)?);
    rec.f.set("valueString", r.tv()?);
    rec.f.set("valueStringLength", r.bl()?);
    let nk = r.bl()?;
    let nk = fits(r, nk, 2.0)?;
    guard(&mut rec.f, "childValues", |a| {
        for _ in 0..nk {
            let key = r.tv()?;
            let value = read_value(r, h, hd, ver)?;
            a.push(V::Obj(crate::obj! { "key" => key, "value" => value }));
        }
        Ok(())
    })
}
fn field_list(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let hd = rec.handle;
    let n = r.bl()?;
    let n = fits(r, n, 0.0)?;
    rec.f.set("unknown", r.b()?);
    guard(&mut rec.h, "fields", |a| {
        for _ in 0..n {
            a.push(V::Num(h.h(hd)?));
        }
        Ok(())
    })
}

// ================================================================ model (version-neutral, see model.js ACAD_TABLE)

/// R24 border edge flag → side name (20.4.101.4)
fn edge_side(e: f64) -> Option<&'static str> {
    Some(match to_i32(e) {
        _ if e.fract() != 0.0 => return None,
        1 => "top",
        2 => "right",
        4 => "bottom",
        8 => "left",
        16 => "insideV",
        32 => "insideH",
        _ => return None,
    })
}
/// R13–R21 table style borders, in stored order (20.4.101.1)
const OLD_BORDER_ORDER: [&str; 6] = ["top", "insideH", "bottom", "left", "insideV", "right"];
fn default_style_name(id: f64) -> Option<&'static str> {
    Some(match id {
        1.0 => "_TITLE",
        2.0 => "_HEADER",
        3.0 => "_DATA",
        4.0 => "Table",
        _ => return None,
    })
}

/// A V8 TypeError text for reading a property of undefined (what the JS model code throws on a partial record).
fn undef(k: &str) -> DwgError {
    DwgError(format!("Cannot read properties of undefined (reading '{k}')"))
}

fn opt(o: &Obj, k: &str) -> Option<V> {
    o.get(k).cloned()
}
fn num(o: &Obj, k: &str) -> Option<f64> {
    o.num(k)
}
fn flags_of(o: &Obj, k: &str) -> i32 {
    o.num(k).map(to_i32).unwrap_or(0)
}
fn hx_opt(h: Option<f64>) -> String {
    h.map(hx).unwrap_or_default()
}
/// `res.nameOf(h) ?? res.hx(h)`
fn name_or_hx(b: &Build, h: Option<f64>) -> V {
    match h {
        Some(n) => match b.name_of(n) {
            Some(v) if v != V::Null => v,
            _ => V::Str(hx(n)),
        },
        None => V::Str(String::new()),
    }
}

/// JS `String(n)` for a number.
fn js_num_str(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let mut s = String::new();
    if n != 0.0 && n.abs() < 1e-6 {
        // JS switches to exponent form below 1e-6 (js_num only below 1e-7: the same number for JSON)
        s = format!("{n:e}");
    } else {
        crate::value::js_num(n, &mut s);
    }
    if let Some(p) = s.find('e') {
        let q = p.saturating_add(1);
        if !s.get(q..).is_some_and(|t| t.starts_with('-')) {
            s.insert(q, '+');
        }
    }
    s
}
fn js_string(v: &V) -> String {
    match v {
        V::Num(n) => js_num_str(*n),
        other => crate::read::js_string(other),
    }
}

/// A value (20.4.99) as model JSON: { dataType, unitType?, format?, text?, value? }
fn value_model(v: Option<&V>) -> Option<V> {
    let v = v.filter(|x| x.truthy())?.as_obj()?;
    let mut o = Obj::new();
    o.set_opt("dataType", opt(v, "dataType"));
    if let Some(x) = opt(v, "flags") {
        o.set("flags", x);
    }
    if let Some(x) = opt(v, "value") {
        o.set("value", x);
    }
    if let Some(x) = opt(v, "bytes") {
        o.set("bytes", x);
    }
    if let Some(x) = num(v, "_id") {
        o.set("id", hx(x));
    }
    for k in ["unitType", "format", "text"] {
        if v.truthy(k) {
            o.set_opt(k, opt(v, k));
        }
    }
    Some(V::Obj(o))
}
/// The text a value shows.
fn value_text(v: Option<&Obj>) -> V {
    let Some(v) = v else { return V::from("") };
    if v.has("flags") && flags_of(v, "flags") & 1 != 0 {
        return V::from("");
    }
    if v.truthy("text") {
        return opt(v, "text").unwrap_or_default();
    }
    let dt = num(v, "dataType");
    if dt == Some(4.0) {
        return match v.get("value") {
            None | Some(V::Null) => V::from(""),
            Some(x) => x.clone(),
        };
    }
    if dt == Some(1.0) || dt == Some(2.0) {
        return match v.get("value") {
            None => V::from(""),
            Some(x) => V::Str(js_string(x)),
        };
    }
    V::from("")
}

/// A FIELD record and its children as a model tree (`budget` bounds the whole tree, as depth bounds its height).
fn field_model(handle: f64, b: &Build, depth: u32, budget: &mut u32) -> Res<Option<Obj>> {
    let Some(r) = b.get(handle) else { return Ok(None) };
    if r.typ != "FIELD" || depth > 16 || *budget == 0 {
        return Ok(None);
    }
    *budget = budget.saturating_sub(1);
    let rf = &r.f;
    let mut f = Obj::new();
    f.set("handle", hx(handle));
    f.set_opt("evaluatorId", opt(rf, "evaluatorId"));
    f.set_opt("code", opt(rf, "code"));
    if rf.truthy("format") {
        f.set_opt("format", opt(rf, "format"));
    }
    for k in ["evalOptions", "filingOptions", "state", "evalStatus", "evalError"] {
        f.set_opt(k, opt(rf, k));
    }
    if rf.truthy("evalMessage") {
        f.set_opt("evalMessage", opt(rf, "evalMessage"));
    }
    f.set_opt("value", value_model(rf.get("value")));
    f.set_opt("valueString", opt(rf, "valueString"));
    let cv = rf.arr("childValues").ok_or_else(|| undef("length"))?;
    if !cv.is_empty() {
        let list: Vec<V> = cv
            .iter()
            .map(|c| {
                let c = c.as_obj();
                let mut o = Obj::new();
                o.set_opt("key", c.and_then(|c| opt(c, "key")));
                o.set_opt("value", value_model(c.and_then(|c| c.get("value"))));
                V::Obj(o)
            })
            .collect();
        f.set("childValues", list);
    }
    let mut kids = Vec::new();
    for h in r.hlist("children").unwrap_or_default() {
        if let Some(k) = field_model(h, b, depth.saturating_add(1), budget)? {
            kids.push(V::Obj(k));
        }
    }
    if !kids.is_empty() {
        f.set("children", kids);
    }
    let objs = r.hlist("objects").unwrap_or_default();
    if !objs.is_empty() {
        f.set("objects", objs.into_iter().map(|h| V::Str(hx(h))).collect::<Vec<V>>());
    }
    Ok(Some(f))
}
fn field_model_top(handle: f64, b: &Build) -> Res<Option<Obj>> {
    let mut budget = 100_000;
    field_model(handle, b, 0, &mut budget)
}

/// What a field shows: its cached value string, or its code with each %<\_FldIdx n>% replaced by child n.
fn field_text(f: Option<&Obj>) -> String {
    let Some(f) = f else { return String::new() };
    if f.truthy("valueString") {
        return f.get("valueString").map(js_string).unwrap_or_default();
    }
    let kids: &[V] = f.arr("children").map(Vec::as_slice).unwrap_or(&[]);
    let code = if f.truthy("code") { f.get("code").map(js_string).unwrap_or_default() } else { String::new() };
    const PRE: &str = "%<\\_FldIdx ";
    let mut out = String::new();
    let mut rest = code.as_str();
    while let Some(p) = rest.find(PRE) {
        let (before, from) = rest.split_at(p);
        let after = from.get(PRE.len()..).unwrap_or("");
        let nd = after.bytes().take_while(u8::is_ascii_digit).count();
        let digits = after.get(..nd).unwrap_or("");
        if nd > 0 && after.get(nd..).is_some_and(|t| t.starts_with(">%")) {
            out.push_str(before);
            let kid = digits.parse::<usize>().ok().and_then(|i| kids.get(i)).and_then(V::as_obj);
            if let Some(k) = kid {
                out.push_str(&field_text(Some(k)));
            }
            rest = after.get(nd.saturating_add(2)..).unwrap_or("");
        } else {
            // no match here: keep the '%' and search on from the next character
            let (head, tail) = from.split_at(1);
            out.push_str(before);
            out.push_str(head);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn borders_model(list: Option<&Vec<V>>, b: &Build) -> Obj {
    let mut out = Obj::new();
    for x in list.map(Vec::as_slice).unwrap_or(&[]).iter().filter_map(V::as_obj) {
        let Some(e) = num(x, "edge").filter(|e| *e != 0.0) else { continue };
        let Some(side) = edge_side(e) else { continue };
        let mut s = Obj::new();
        s.set_opt("overrides", opt(x, "overrides"));
        s.set_opt("type", opt(x, "borderType"));
        s.set_opt("color", opt(x, "color"));
        s.set_opt("lineweight", opt(x, "lineweight"));
        s.set("visible", !x.truthy("invisible"));
        s.set_opt("doubleSpacing", opt(x, "doubleSpacing"));
        if x.truthy("_linetype") {
            s.set("linetype", name_or_hx(b, num(x, "_linetype")));
        }
        out.set(side, s);
    }
    out
}
fn content_model(c: &Obj, b: &Build) -> Obj {
    let mut o = Obj::new();
    for k in ["overrides", "propertyFlags", "dataType", "unitType", "format", "rotation", "blockScale"] {
        o.set_opt(k, opt(c, k));
    }
    o.set_opt("align", opt(c, "alignment"));
    o.set_opt("color", opt(c, "color"));
    o.set("textStyle", name_or_hx(b, num(c, "_textStyle")));
    o.set_opt("textHeight", opt(c, "textHeight"));
    o
}
/// A cell style (20.4.101.4 + content format 20.4.101.3) as model JSON, handles resolved.
fn cell_style_model(s: Option<&V>, b: &Build) -> Res<Option<Obj>> {
    let Some(s) = s.filter(|x| x.truthy()).and_then(V::as_obj) else { return Ok(None) };
    let mut o = Obj::new();
    o.set_opt("type", opt(s, "styleType"));
    if s.has("id") {
        o.set_opt("id", opt(s, "id"));
        o.set_opt("styleClass", opt(s, "styleClass"));
        o.set_opt("name", opt(s, "name"));
    }
    if !s.truthy("hasData") {
        return Ok(Some(o));
    }
    let c = s.obj("content").ok_or_else(|| undef("overrides"))?;
    o.set_opt("overrides", opt(s, "overrides"));
    o.set_opt("mergeFlags", opt(s, "mergeFlags"));
    o.set_opt("background", opt(s, "background"));
    o.set_opt("contentLayout", opt(s, "contentLayout"));
    o.set("content", content_model(c, b));
    if let Some(m) = s.get("margins").filter(|m| m.truthy()) {
        o.set("margins", m.clone());
    }
    o.set("borders", borders_model(s.arr("borders"), b));
    Ok(Some(o))
}

/// The name a dictionary gives an object (its owner's entry), e.g. a table style's name.
fn entry_name(handle: f64, b: &Build) -> Option<V> {
    let r = b.get(handle)?;
    let d = b.get(r.hn("owner")?)?;
    let names = d.f.arr("names")?;
    let items = d.h.arr("items")?;
    let k = items.iter().position(|x| x.as_f64() == Some(handle))?;
    names.get(k).filter(|v| **v != V::Null).cloned()
}

/// Table style → neutral { name, description, flowDirection, flags, hMargin, vMargin, titleSuppressed, headerSuppressed, cellStyles[] }
fn table_style_model(r: &Record, b: &Build) -> Res<Obj> {
    let rf = &r.f;
    let mut o = Obj::new();
    o.set_opt("name", entry_name(r.handle, b));
    o.set_opt("description", opt(rf, "description"));
    if let Some(rows) = rf.get("rowStyles").filter(|v| v.truthy()) {
        // R13–R21
        for k in ["flowDirection", "flags", "hMargin", "vMargin"] {
            o.set_opt(k, opt(rf, k));
        }
        o.set("titleSuppressed", rf.truthy("titleSuppressed"));
        o.set("headerSuppressed", rf.truthy("headerSuppressed"));
        let rows = rows.as_arr().ok_or_else(|| undef("map"))?;
        let mut list = Vec::new();
        for (i, s) in rows.iter().enumerate() {
            let s = s.as_obj().ok_or_else(|| undef("borders"))?;
            let id = [3.0, 1.0, 2.0].get(i).copied();
            let mut borders = Obj::new();
            for (k, bd) in s.arr("borders").ok_or_else(|| undef("forEach"))?.iter().enumerate() {
                let bd = bd.as_obj().ok_or_else(|| undef("lineweight"))?;
                let mut x = Obj::new();
                x.set_opt("lineweight", opt(bd, "lineweight"));
                x.set("visible", bd.truthy("visible"));
                x.set_opt("color", opt(bd, "color"));
                borders.set(OLD_BORDER_ORDER.get(k).copied().unwrap_or("undefined"), x);
            }
            let mut cs = Obj::new();
            cs.set_opt("id", id.map(V::Num));
            cs.set_opt("name", id.and_then(default_style_name).map(V::from));
            cs.set("styleClass", if id == Some(3.0) { 1 } else { 2 });
            cs.set("textStyle", name_or_hx(b, num(s, "_textStyle")));
            cs.set_opt("textHeight", opt(s, "textHeight"));
            cs.set_opt("align", opt(s, "alignment"));
            cs.set_opt("color", opt(s, "textColor"));
            cs.set_opt("background", opt(s, "fillColor"));
            cs.set("backgroundEnabled", s.truthy("fillEnabled"));
            cs.set("borders", borders);
            if s.has("dataType") {
                cs.set_opt("dataType", opt(s, "dataType"));
                cs.set_opt("unitType", opt(s, "unitType"));
                cs.set_opt("format", opt(s, "format"));
            }
            list.push(V::Obj(cs));
        }
        o.set("cellStyles", list);
    } else {
        // R24+: the "Table" cell style carries the table-wide settings
        o.set_opt("flags", opt(rf, "unknownBL1"));
        let mut u = Obj::new();
        u.set_opt("rc", opt(rf, "unknownRC"));
        u.set_opt("bl2", opt(rf, "unknownBL2"));
        o.set("unknown", u);
        let tv = rf.get("tableCellStyle");
        o.set_opt("tableCellStyle", cell_style_model(tv, b)?.map(V::Obj));
        let t = tv.and_then(V::as_obj).ok_or_else(|| undef("overrides"))?;
        o.set("flowDirection", if t.has("overrides") && flags_of(t, "mergeFlags") & 0x10000 != 0 { 1 } else { 0 });
        let styles = rf.arr("cellStyles").ok_or_else(|| undef("map"))?;
        let mut list = Vec::new();
        for s in styles {
            let m = cell_style_model(Some(s), b)?.ok_or_else(|| undef("content"))?;
            let so = s.as_obj();
            let mut cs = Obj::new();
            cs.set_opt("id", so.and_then(|x| opt(x, "id")));
            cs.set_opt("name", so.and_then(|x| opt(x, "name")));
            cs.set_opt("styleClass", so.and_then(|x| opt(x, "styleClass")));
            if let Some(c) = m.obj("content") {
                cs.set_opt("textStyle", opt(c, "textStyle"));
                cs.set_opt("textHeight", opt(c, "textHeight"));
                cs.set_opt("align", opt(c, "align"));
                cs.set_opt("color", opt(c, "color"));
                cs.set_opt("background", opt(&m, "background"));
                let bg257 = m.get("background").filter(|v| v.truthy()).and_then(V::as_obj).is_some_and(|bg| bg.num("index") == Some(257.0));
                cs.set("backgroundEnabled", !bg257);
                cs.set_opt("dataType", opt(c, "dataType"));
                cs.set_opt("unitType", opt(c, "unitType"));
                cs.set_opt("format", opt(c, "format"));
                cs.set_opt("rotation", opt(c, "rotation"));
                cs.set_opt("margins", opt(&m, "margins"));
                cs.set_opt("borders", opt(&m, "borders"));
                cs.set("cell", m.clone());
            }
            list.push(V::Obj(cs));
        }
        o.set("cellStyles", list);
        if let Some(mg) = t.get("margins").filter(|v| v.truthy()).and_then(V::as_obj) {
            o.set_opt("vMargin", opt(mg, "vertical"));
            o.set_opt("hMargin", opt(mg, "horizontal"));
        }
    }
    Ok(o)
}

/// model fields of an ACAD_TABLE entity record (f = the entity being built)
pub fn table_entity_fields(rec: &Record, f: &mut Obj, b: &Build) -> Res<()> {
    let blk = rec.hn("block");
    f.set("block", name_or_hx(b, blk));
    f.set("blockRecord", hx_opt(blk));
    for k in ["partial", "tableFlags", "rows", "cols", "colWidths", "rowHeights"] {
        f.remove(k);
    }
    let Some(t) = rec.f.obj("_table") else { return Ok(()) };
    f.set_opt("horizDir", opt(&rec.f, "horizDir"));
    let table = if t.truthy("content") { new_table_model(t, b)? } else { old_table_model(rec, t, b)? };
    f.set("table", table);
    Ok(())
}

fn style_of(handle: Option<f64>, b: &Build) -> Res<Option<Obj>> {
    match handle.and_then(|h| b.get(h)) {
        Some(r) if r.typ == "TABLESTYLE" => Ok(Some(table_style_model(r, b)?)),
        _ => Ok(None),
    }
}
fn style_cells(style: &Option<Obj>) -> Vec<&Obj> {
    style.as_ref().and_then(|s| s.arr("cellStyles")).map(|a| a.iter().filter_map(V::as_obj).collect()).unwrap_or_default()
}

fn attr_model(a: &Obj, b: &Build) -> V {
    let ad = num(a, "_attdef");
    let mut o = Obj::new();
    o.set("attdef", hx_opt(ad));
    o.set_opt("value", opt(a, "value"));
    o.set_opt("index", opt(a, "index"));
    if let Some(d) = ad.and_then(|h| b.get(h))
        && let Some(tag) = d.f.get("tag")
    {
        o.set("tag", tag.clone());
    }
    V::Obj(o)
}

fn old_table_model(rec: &Record, t: &Obj, b: &Build) -> Res<Obj> {
    let ts_h = rec.hn("tableStyle");
    let style = style_of(ts_h, b)?;
    let col_w = t.arr("colWidths").ok_or_else(|| undef("length"))?;
    let row_h = t.arr("rowHeights").ok_or_else(|| undef("length"))?;
    let nc = col_w.len();
    let mut tt = Obj::new();
    tt.set("style", hx_opt(ts_h));
    tt.set_opt("styleName", style.as_ref().and_then(|s| opt(s, "name")));
    tt.set_opt("flags", opt(t, "flags"));
    let empty = Obj::new();
    let o = t.obj("overrides").unwrap_or(&empty);
    let flow = match o.get("flowDirection") {
        Some(v) if *v != V::Null => v.clone(),
        _ => match &style {
            Some(s) => match s.get("flowDirection") {
                Some(v) if *v != V::Null => v.clone(),
                _ => V::Num(0.0),
            },
            None => V::Num(0.0),
        },
    };
    tt.set("flowDirection", flow);
    let title_sup: Option<V> = if o.has("titleSuppressed") {
        Some(V::Bool(o.truthy("titleSuppressed")))
    } else {
        match &style {
            Some(s) => opt(s, "titleSuppressed"),
            None => Some(V::Bool(false)),
        }
    };
    tt.set_opt("titleSuppressed", title_sup.clone());
    let header_sup: Option<V> = match &style {
        Some(s) => opt(s, "headerSuppressed"),
        None => Some(V::Bool(false)),
    };
    tt.set_opt("headerSuppressed", header_sup.clone());
    let ts = title_sup.as_ref().is_some_and(V::truthy);
    let hs = header_sup.as_ref().is_some_and(V::truthy);
    let row_style = |i: usize| -> &'static str {
        let mut k = 0usize;
        if !ts {
            if i == k {
                return "_TITLE";
            }
            k = 1;
        }
        if !hs && i == k {
            return "_HEADER";
        }
        "_DATA"
    };
    let scs = style_cells(&style);
    let style_by_name = |name: &str| -> Option<&Obj> { scs.iter().rev().find(|s| s.str("name") == Some(name)).copied() };
    let rows: Vec<V> = row_h
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let mut x = Obj::new();
            x.set("height", h.clone());
            x.set("style", row_style(i));
            V::Obj(x)
        })
        .collect();
    tt.set("rows", rows);
    tt.set("columns", col_w.iter().map(|w| V::Obj(crate::obj! { "width" => w.clone() })).collect::<Vec<V>>());
    let cells_src = t.arr("cells").ok_or_else(|| undef("type"))?;
    let mut cells: Vec<Vec<Obj>> = Vec::new();
    let mut merged: Vec<V> = Vec::new();
    for i in 0..row_h.len() {
        let mut row = Vec::new();
        for j in 0..nc {
            let c = i
                .checked_mul(nc)
                .and_then(|x| x.checked_add(j))
                .and_then(|x| cells_src.get(x))
                .and_then(V::as_obj)
                .ok_or_else(|| undef("type"))?;
            let is_block = num(c, "type") == Some(2.0);
            let mut cell = Obj::new();
            cell.set("type", if is_block { "block" } else { "text" });
            cell.set("value", "");
            let val_h = num(c, "_value");
            if is_block {
                cell.set("block", name_or_hx(b, val_h));
                cell.set("blockRecord", hx_opt(val_h));
                cell.set_opt("blockScale", opt(c, "blockScale"));
                if let Some(a) = c.get("attrs").filter(|v| v.truthy()).and_then(V::as_arr) {
                    cell.set("attrs", a.iter().filter_map(V::as_obj).map(|a| attr_model(a, b)).collect::<Vec<V>>());
                }
            } else if let Some(vh) = val_h.filter(|v| *v != 0.0 && !v.is_nan()) {
                let fm = field_model_top(vh, b)?;
                let text = field_text(fm.as_ref());
                cell.set_opt("field", fm.map(V::Obj));
                cell.set("value", text);
            } else if c.truthy("value") {
                cell.set("value", value_text(c.obj("value")));
            } else {
                cell.set(
                    "value",
                    match c.get("text") {
                        Some(v) if *v != V::Null => v.clone(),
                        _ => V::from(""),
                    },
                );
            }
            if c.truthy("value") {
                cell.set_opt("data", value_model(c.get("value")));
            }
            cell.set("style", row_style(i));
            cell.set_opt("rotation", opt(c, "rotation"));
            if c.truthy("autoFit") {
                cell.set("autoFit", true);
            }
            let mw = num(c, "mergedWidth");
            let mh = num(c, "mergedHeight");
            if mw.is_some_and(|x| x > 1.0) || mh.is_some_and(|x| x > 1.0) {
                let rws = mh.filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(1.0);
                let cls = mw.filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(1.0);
                cell.set("merged", crate::obj! { "rows" => rws, "cols" => cls });
                let fi = crate::util::usize_to_f64(i);
                let fj = crate::util::usize_to_f64(j);
                merged.push(V::Obj(crate::obj! { "top" => fi, "left" => fj, "bottom" => fi + rws - 1.0, "right" => fj + cls - 1.0 }));
            }
            if c.truthy("merged") {
                cell.set("covered", true);
            }
            if c.truthy("edgeFlags") || c.truthy("virtualEdges") {
                let virt = if c.truthy("virtualEdges") { opt(c, "virtualEdges").unwrap_or_default() } else { V::Num(0.0) };
                let mut e = Obj::new();
                e.set_opt("flags", opt(c, "edgeFlags"));
                e.set("virtual", virt);
                cell.set("edges", e);
            }
            if c.truthy("hasOverrides") {
                let mut ov = Obj::new();
                let fl = flags_of(c, "overrideFlags");
                if fl & 0x01 != 0 {
                    ov.set_opt("align", opt(c, "alignment"));
                }
                if fl & 0x02 != 0 {
                    ov.set("fillNone", c.truthy("fillNone"));
                }
                if fl & 0x04 != 0 {
                    ov.set_opt("background", opt(c, "background"));
                }
                if fl & 0x08 != 0 {
                    ov.set_opt("color", opt(c, "contentColor"));
                }
                if fl & 0x10 != 0 {
                    ov.set("textStyle", name_or_hx(b, num(c, "_textStyle")));
                }
                if fl & 0x20 != 0 {
                    ov.set_opt("textHeight", opt(c, "textHeight"));
                }
                if let Some(bd) = c.obj("borders").filter(|x| !x.is_empty()) {
                    ov.set("borders", bd.clone());
                }
                if !ov.is_empty() {
                    cell.set("overrides", ov);
                }
            }
            let st = cell.str("style").and_then(style_by_name);
            let align = match cell.obj("overrides").and_then(|x| x.get("align")) {
                Some(a) => Some(a.clone()),
                None => st.and_then(|s| opt(s, "align")),
            };
            cell.set_opt("align", align);
            row.push(cell);
        }
        cells.push(row);
    }
    canonical_borders(&mut cells);
    tt.set("cells", cells.into_iter().map(|r| V::Arr(r.into_iter().map(V::Obj).collect())).collect::<Vec<V>>());
    tt.set("merged", merged);
    if t.truthy("hasOverrides") {
        let mut ov = Obj::new();
        for (k, v) in o.iter() {
            if let Some(name) = k.strip_prefix('_') {
                ov.set(name.to_string(), name_or_hx(b, v.as_f64()));
            } else {
                ov.set(k.to_string(), v.clone());
            }
        }
        tt.set("overrides", ov);
        tt.set_opt("overrideFlags", opt(t, "overrideFlags"));
    }
    for k in ["borderColors", "borderLineweights", "borderVisibility"] {
        if t.truthy(k) {
            tt.set_opt(k, opt(t, k));
        }
    }
    Ok(tt)
}

/// Border overrides on a shared edge belong to the upper cell (its bottom) and the left cell (its right): a top or
/// left override moves to the neighbour — as the ODA File Converter stores them (found by experiment).
fn canonical_borders(cells: &mut [Vec<Obj>]) {
    for i in 0..cells.len() {
        let n_j = cells.get(i).map_or(0, Vec::len);
        for j in 0..n_j {
            let Some(mut b) = cells.get(i).and_then(|r| r.get(j)).and_then(|c| c.obj("overrides")).and_then(|o| o.obj("borders")).cloned()
            else {
                continue;
            };
            for (side, up, to) in [("top", true, "bottom"), ("left", false, "right")] {
                let Some(moved) = b.get(side).filter(|v| v.truthy()).cloned() else { continue };
                let (ni, nj) = if up { (i.checked_sub(1), Some(j)) } else { (Some(i), j.checked_sub(1)) };
                let Some(n) = ni.zip(nj).and_then(|(a, c)| cells.get_mut(a).and_then(|r| r.get_mut(c))) else { continue };
                if n.obj("overrides").is_none() {
                    n.set("overrides", Obj::new());
                }
                if let Some(ov) = n.obj_mut("overrides") {
                    let mut borders = ov.obj("borders").cloned().unwrap_or_default();
                    let mut inner = borders.obj(to).cloned().unwrap_or_default();
                    if let Some(m) = moved.as_obj() {
                        inner.assign(m.clone());
                    }
                    borders.set(to, inner);
                    ov.set("borders", borders);
                }
                b.remove(side);
            }
            if let Some(ov) = cells.get_mut(i).and_then(|r| r.get_mut(j)).and_then(|c| c.obj_mut("overrides")) {
                if b.is_empty() {
                    ov.remove("borders");
                } else {
                    ov.set("borders", b);
                }
            }
            if let Some(c) = cells.get_mut(i).and_then(|r| r.get_mut(j))
                && c.obj("overrides").is_some_and(Obj::is_empty)
            {
                c.remove("overrides");
            }
        }
    }
}

fn custom_model(list: Option<&Vec<V>>) -> Option<V> {
    let l = list?;
    if l.is_empty() {
        return None;
    }
    Some(V::Arr(
        l.iter()
            .map(|x| {
                let x = x.as_obj();
                let mut o = Obj::new();
                o.set_opt("name", x.and_then(|x| opt(x, "name")));
                o.set_opt("value", value_model(x.and_then(|x| x.get("value"))));
                V::Obj(o)
            })
            .collect(),
    ))
}

fn new_table_model(t: &Obj, b: &Build) -> Res<Obj> {
    let c = t.obj("content").ok_or_else(|| undef("content"))?;
    let style_h = num(c, "_style");
    let style = style_of(style_h, b)?;
    let mut tt = Obj::new();
    tt.set("style", hx_opt(style_h));
    tt.set_opt("styleName", style.as_ref().and_then(|s| opt(s, "name")));
    if c.truthy("name") {
        tt.set_opt("name", opt(c, "name"));
    }
    if c.truthy("description") {
        tt.set_opt("description", opt(c, "description"));
    }
    let scs = style_cells(&style);
    let style_name = |id: Option<f64>| -> Option<V> {
        if let Some(n) = id.and_then(|id| scs.iter().rev().find(|s| s.num("id") == Some(id))).and_then(|s| s.get("name"))
            && *n != V::Null
        {
            return Some(n.clone());
        }
        let id = id?;
        if let Some(d) = default_style_name(id) {
            return Some(V::from(d));
        }
        if id != 0.0 && !id.is_nan() { Some(V::Str(format!("id{}", js_num_str(id)))) } else { None }
    };
    let ts = cell_style_model(c.get("tableStyle"), b)?;
    let ts_flow = ts.as_ref().is_some_and(|x| x.has("overrides") && flags_of(x, "mergeFlags") & 0x10000 != 0);
    let flow = if ts_flow {
        Some(V::Num(1.0))
    } else {
        match &style {
            Some(s) => opt(s, "flowDirection"),
            None => Some(V::Num(0.0)),
        }
    };
    tt.set_opt("flowDirection", flow);
    let rows_src = c.arr("rows").ok_or_else(|| undef("map"))?;
    let mut rows = Vec::new();
    for r in rows_src {
        let r = r.as_obj().ok_or_else(|| undef("height"))?;
        let mut o = Obj::new();
        o.set_opt("height", opt(r, "height"));
        o.set_opt("style", style_name(num(r, "styleId")));
        if let Some(st) = cell_style_model(r.get("style"), b)?
            && st.has("overrides")
        {
            o.set("cellStyle", st);
        }
        o.set_opt("custom", custom_model(r.arr("custom")));
        rows.push(o);
    }
    let mut cols = Vec::new();
    for col in c.arr("columns").ok_or_else(|| undef("map"))? {
        let col = col.as_obj().ok_or_else(|| undef("width"))?;
        let mut o = Obj::new();
        o.set_opt("width", opt(col, "width"));
        if col.truthy("name") {
            o.set_opt("name", opt(col, "name"));
        }
        if col.truthy("styleId") {
            o.set_opt("style", style_name(num(col, "styleId")));
        }
        if let Some(st) = cell_style_model(col.get("style"), b)?
            && st.has("overrides")
        {
            o.set("cellStyle", st);
        }
        o.set_opt("custom", custom_model(col.arr("custom")));
        cols.push(V::Obj(o));
    }
    tt.set("columns", cols);
    if let Some(ts) = &ts
        && ts.has("overrides")
    {
        tt.set("tableCellStyle", ts.clone());
    }
    let merged: Vec<Obj> = c.arr("merged").ok_or_else(|| undef("map"))?.iter().map(|m| m.as_obj().cloned().unwrap_or_default()).collect();
    let mnum = |m: &Obj, k: &str| m.num(k).unwrap_or(f64::NAN);
    let mut cells: Vec<Vec<Obj>> = Vec::new();
    for (i, row) in rows_src.iter().enumerate() {
        let row = row.as_obj().ok_or_else(|| undef("cells"))?;
        let fi = crate::util::usize_to_f64(i);
        let mut out_row = Vec::new();
        for (j, x) in row.arr("cells").ok_or_else(|| undef("map"))?.iter().enumerate() {
            let x = x.as_obj().ok_or_else(|| undef("contents"))?;
            let fj = crate::util::usize_to_f64(j);
            let mut cell = Obj::new();
            cell.set("type", "text");
            cell.set("value", "");
            let contents = x.arr("contents").ok_or_else(|| undef("0"))?;
            if let Some(ct) = contents.first().filter(|v| v.truthy()).and_then(V::as_obj) {
                let ty = num(ct, "type");
                let fmt = ct.get("format").filter(|v| v.truthy()).and_then(V::as_obj);
                if ty == Some(4.0) {
                    cell.set("type", "block");
                    let bh = num(ct, "_block");
                    cell.set("block", name_or_hx(b, bh));
                    cell.set("blockRecord", hx_opt(bh));
                    if let Some(f) = fmt {
                        cell.set_opt("blockScale", opt(f, "blockScale"));
                    }
                    let attrs = ct.arr("attrs").ok_or_else(|| undef("length"))?;
                    if !attrs.is_empty() {
                        cell.set("attrs", attrs.iter().filter_map(V::as_obj).map(|a| attr_model(a, b)).collect::<Vec<V>>());
                    }
                } else if ty == Some(2.0) {
                    let fm = match num(ct, "_field").filter(|h| *h != 0.0 && !h.is_nan()) {
                        Some(h) => field_model_top(h, b)?,
                        None => None,
                    };
                    let text = field_text(fm.as_ref());
                    cell.set_opt("field", fm.map(V::Obj));
                    cell.set("value", text);
                } else if ty == Some(1.0) {
                    cell.set("value", value_text(ct.obj("value")));
                    cell.set_opt("data", value_model(ct.get("value")));
                }
                if let Some(f) = fmt {
                    cell.set("contentFormat", content_model(f, b));
                }
                if contents.len() > 1 {
                    cell.set("extraContents", contents.len().saturating_sub(1)); // multiple contents per cell: only the first is modelled
                }
            }
            let style_v = if x.truthy("styleId") { style_name(num(x, "styleId")) } else { rows.get(i).and_then(|r| opt(r, "style")) };
            cell.set_opt("style", style_v.clone());
            let st = cell_style_model(x.get("style"), b)?;
            let cf = cell.obj("contentFormat").cloned();
            cell.set_opt(
                "rotation",
                match &cf {
                    Some(cf) => opt(cf, "rotation"),
                    None => Some(V::Num(0.0)),
                },
            );
            if x.truthy("state") {
                cell.set_opt("state", opt(x, "state"));
            }
            if x.truthy("tooltip") {
                cell.set_opt("tooltip", opt(x, "tooltip"));
            }
            cell.set_opt("custom", custom_model(x.arr("custom")));
            if let Some(m) = merged.iter().rev().find(|m| mnum(m, "top") == fi && mnum(m, "left") == fj) {
                let rws = mnum(m, "bottom") - mnum(m, "top") + 1.0;
                let cls = mnum(m, "right") - mnum(m, "left") + 1.0;
                cell.set("merged", crate::obj! { "rows" => rws, "cols" => cls });
            }
            let covered = merged.iter().any(|m| {
                let (t0, l0, b0, r0) = (mnum(m, "top"), mnum(m, "left"), mnum(m, "bottom"), mnum(m, "right"));
                t0 <= fi && fi <= b0 && l0 <= fj && fj <= r0 && (fi != t0 || fj != l0) && fi.fract() == 0.0
            });
            if covered {
                cell.set("covered", true);
            }
            // overrides: the cell style's and the content format's flagged properties (20.4.101.3 bits)
            let mut ov = Obj::new();
            let take = |ov: &mut Obj, fl: i32, src: Option<&Obj>| {
                let Some(src) = src else { return };
                if fl & 0x10 != 0 {
                    ov.set_opt("align", opt(src, "align"));
                }
                if fl & 0x20 != 0 {
                    ov.set_opt("color", opt(src, "color"));
                }
                if fl & 0x40 != 0 {
                    ov.set_opt("textStyle", opt(src, "textStyle"));
                }
                if fl & 0x80 != 0 {
                    ov.set_opt("textHeight", opt(src, "textHeight"));
                }
            };
            if let Some(st) = st.as_ref().filter(|s| s.has("overrides")) {
                let fl = flags_of(st, "overrides");
                take(&mut ov, fl, st.obj("content"));
                if fl & 0x200 != 0 {
                    ov.set_opt("background", opt(st, "background"));
                    if st.get("background").filter(|v| v.truthy()).and_then(V::as_obj).is_some_and(|bg| bg.num("index") == Some(257.0)) {
                        ov.set("fillNone", true);
                    }
                }
                cell.set("cellStyle", st.clone());
            }
            if let Some(cf) = &cf {
                take(&mut ov, flags_of(cf, "overrides"), Some(cf));
            }
            let sm = style.as_ref().and_then(|_| scs.iter().find(|s| s.get("name") == style_v.as_ref()).copied());
            // R2010+ cells carry a border record per edge (override bits 26 on every one): an override is a value that
            // differs from the cell style's (found by having the ODA File Converter save an R2004 border override as R2018)
            if let Some(sb) = st.as_ref().and_then(|s| s.obj("borders")) {
                for side in ["top", "right", "bottom", "left"] {
                    let Some(bs) = sb.obj(side) else { continue };
                    let dflt = crate::obj! { "color" => crate::obj!{ "index" => 0 }, "lineweight" => -2, "visible" => true };
                    let d = sm
                        .and_then(|s| s.obj("borders"))
                        .and_then(|x| x.get(side))
                        .filter(|v| v.truthy())
                        .and_then(V::as_obj)
                        .unwrap_or(&dflt);
                    let dcol = d.get("color").filter(|v| **v != V::Null).cloned().unwrap_or_else(|| V::Obj(crate::obj! { "index" => 0 }));
                    let dlw = d.get("lineweight").filter(|v| **v != V::Null).cloned().unwrap_or(V::Num(-2.0));
                    let dvis = d.get("visible").filter(|v| **v != V::Null).cloned().unwrap_or(V::Bool(true));
                    let mut x2 = Obj::new();
                    if let Some(bc) = bs.get("color")
                        && bc.to_json() != dcol.to_json()
                    {
                        x2.set("color", bc.clone());
                    }
                    if let Some(bl) = bs.get("lineweight")
                        && *bl != dlw
                    {
                        x2.set("lineweight", bl.clone());
                    }
                    if let Some(bv) = bs.get("visible")
                        && *bv != dvis
                    {
                        x2.set("visible", bv.clone());
                    }
                    if !x2.is_empty() {
                        if ov.obj("borders").is_none() {
                            ov.set("borders", Obj::new());
                        }
                        if let Some(ob) = ov.obj_mut("borders") {
                            ob.set(side, x2);
                        }
                    }
                }
            }
            let align = match ov.get("align") {
                Some(a) => Some(a.clone()),
                None => sm.and_then(|s| opt(s, "align")),
            };
            if !ov.is_empty() {
                cell.set("overrides", ov);
            }
            cell.set_opt("align", align);
            out_row.push(cell);
        }
        cells.push(out_row);
    }
    tt.set("rows", rows.into_iter().map(V::Obj).collect::<Vec<V>>());
    tt.set("merged", merged.into_iter().map(V::Obj).collect::<Vec<V>>());
    tt.set_opt("tableBits", opt(t, "unknownBS")); // the BS after the R24 table content (6 or 38 in the corpus; meaning unknown)
    canonical_borders(&mut cells);
    tt.set("cells", cells.into_iter().map(|r| V::Arr(r.into_iter().map(V::Obj).collect())).collect::<Vec<V>>());
    if t.truthy("hasBreak") {
        tt.set("breaks", t.obj("break").cloned().unwrap_or_default());
    }
    let rr = t.arr("rowRanges").ok_or_else(|| undef("length"))?;
    if !rr.is_empty() {
        tt.set("rowRanges", rr.clone());
    }
    Ok(tt)
}

/// model fields of the table objects (TABLESTYLE, CELLSTYLEMAP, TABLECONTENT, TABLEGEOMETRY, FIELD)
pub fn table_object_fields(rec: &Record, f: &mut Obj, b: &Build) -> Res<()> {
    match rec.typ.as_str() {
        "TABLESTYLE" => {
            f.0.retain(|(k, _)| ["handle", "owner", "reactors", "extDict", "type"].contains(&k.as_ref()));
            f.assign(table_style_model(rec, b)?);
        }
        "CELLSTYLEMAP" => {
            let list = rec.f.arr("cellStyles").ok_or_else(|| undef("map"))?;
            let mut out = Vec::new();
            for s in list {
                out.push(cell_style_model(Some(s), b)?.map(V::Obj).unwrap_or_default());
            }
            f.set("cellStyles", out);
        }
        "TABLECONTENT" => f.set_opt("content", rec.f.get("_content").map(|v| deep(v, 0))),
        "TABLEGEOMETRY" => f.set_opt("cells", rec.f.get("cells").map(|v| deep(v, 0))),
        "FIELD" => {
            if let Some(mut m) = field_model_top(rec.handle, b)? {
                m.remove("children");
                // Object.assign copies keys whose value is undefined too: they hide plain()'s copy
                for k in ["evaluatorId", "code", "evalOptions", "filingOptions", "state", "evalStatus", "evalError", "value", "valueString"]
                {
                    if !m.has(k) {
                        f.remove(k);
                    }
                }
                f.assign(m);
                let kids = rec.hlist("children").unwrap_or_default();
                f.set("children", kids.into_iter().map(|h| V::Str(hx(h))).collect::<Vec<V>>());
            }
        }
        _ => {}
    }
    Ok(())
}

/// a plain copy with "_name" handle numbers turned into "name" hex strings
fn deep(v: &V, depth: u32) -> V {
    if depth > 64 {
        return v.clone();
    }
    match v {
        V::Arr(a) => V::Arr(a.iter().map(|x| deep(x, depth.saturating_add(1))).collect()),
        V::Obj(o) => {
            let mut out = Obj::with_capacity(o.len());
            for (k, x) in o.iter() {
                if let Some(name) = k.strip_prefix('_') {
                    let nv = match x {
                        V::Arr(a) => V::Arr(a.iter().map(|h| V::Str(h.as_f64().map(hx).unwrap_or_default())).collect()),
                        V::Num(n) => V::Str(hx(*n)),
                        other => deep(other, depth.saturating_add(1)),
                    };
                    out.set(name.to_string(), nv);
                } else {
                    out.set(k.to_string(), deep(x, depth.saturating_add(1)));
                }
            }
            V::Obj(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_extension() {
        assert_eq!(sext16(0xFFFE), -2);
        assert_eq!(sext16(-2), -2);
        assert_eq!(sext16(256), 256);
        assert_eq!(sext16(0x7FFF), 0x7FFF);
    }

    #[test]
    fn field_text_replaces_child_indexes() {
        let kid = crate::obj! { "valueString" => "A" };
        let f = crate::obj! { "code" => "x %<\\_FldIdx 0>% y %<\\_FldIdx 7>% %<\\_FldIdx z>% %<\\_FldIdx 00>%", "children" => vec![V::Obj(kid)] };
        assert_eq!(field_text(Some(&f)), "x A y  %<\\_FldIdx z>% A");
        assert_eq!(field_text(None), "");
    }

    #[test]
    fn numbers_print_as_javascript() {
        assert_eq!(js_num_str(1.5), "1.5");
        assert_eq!(js_num_str(1e21), "1e+21");
        assert_eq!(js_num_str(1.5e-7), "1.5e-7");
        assert_eq!(js_num_str(-3.0), "-3");
    }

    #[test]
    fn utf16_stops_at_nul() {
        assert_eq!(utf16(&[b't', 0, b'e', 0, 0, 0, b'x', 0]), "te");
        assert_eq!(utf16(&[b'a', 0, b'b']), "a");
    }

    #[test]
    fn borders_move_to_the_neighbour() {
        let top = crate::obj! { "color" => crate::obj!{ "index" => 1 } };
        let a = Obj::new();
        let b = crate::obj! { "overrides" => crate::obj!{ "borders" => crate::obj!{ "top" => top } } };
        let mut cells = vec![vec![a], vec![b]];
        canonical_borders(&mut cells);
        assert!(cells[1][0].get("overrides").is_none());
        assert!(cells[0][0].obj("overrides").and_then(|o| o.obj("borders")).and_then(|o| o.obj("bottom")).is_some());
    }
}
