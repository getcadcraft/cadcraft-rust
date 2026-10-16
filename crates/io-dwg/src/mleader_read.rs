//! MULTILEADER (MLEADER) and MLEADERSTYLE into the model, every field (port of lib/dwg/mleader-read.js).
//! Written from the ODA "Open Design Specification for .dwg files" v5.4.1: 20.4.48 (MLEADER), 20.4.86
//! (MLeaderAnnotContext), 20.4.87 (MLEADERSTYLE), 20.1 (the R2007+ string stream), 2.13 (handles).
//!
//! The record is decoded again from its own bits (`data_start` / `endbit` / `h_pos` / `obj_end`), so every field —
//! including the ones the record decoder skips — reaches the model, with handle references as hex strings.
//! Any failure returns `Err` and read.rs keeps the decoder's fields (the JS `catch`).
//!
//! Spec gap: the spec puts the "Common AcDbAnnotScaleObjectContextData data" (20.4.71) before the leader roots;
//! inside MLEADER the context starts directly with the root count — found by builder R's reader on AutoCAD/ODA
//! files (objects-read.js) and confirmed here against the ODA File Converter's DXF of every corpus multileader.
//! Spec gap: the arrowhead list is present up to R2007 only, the block labels in every version (as 20.4.48's
//! indentation suggests) — the corpus multileaders of all six versions decode to the end of their data that way.

use crate::bits::{BitReader, Res};
use crate::obj;
use crate::objects::{Record, fits, string_stream_start};
use crate::read::Build;
use crate::util::{b64, handle_hex, handle_num, slice_bits, to_usize, usize_to_f64};
use crate::value::{Obj, V};

/// A data reader and a handle reader positioned at the record's own data (after its common part).
fn readers<'a>(rec: &Record, b: &Build<'a>) -> (BitReader<'a>, BitReader<'a>) {
    let ver = b.ctx.ver;
    let cp = b.ctx.cp;
    let mut r = BitReader::range(b.buf, ver, cp, rec.data_start, rec.endbit);
    if ver >= 21 {
        // the string stream lies inside [dataStart, endbit)
        match string_stream_start(b.buf, rec.endbit, rec.data_start) {
            Some((s, e)) => {
                r.set_str(s, e);
                r.end = s;
            }
            None => r.set_str(rec.endbit, rec.endbit),
        }
    }
    let h = BitReader::range(b.buf, ver, cp, rec.h_pos, rec.obj_end);
    (r, h)
}

/// `count(r, n, minBits)`: a count that cannot fit in what is left fails.
fn count(r: &BitReader, n: i32, min_bits: f64) -> Res<i32> {
    fits(r, f64::from(n), min_bits)?;
    Ok(n)
}

/// `hx(h.H(rec.handle))`.
fn hh(h: &mut BitReader, handle: f64) -> Res<String> {
    Ok(handle_hex(h.h(handle)?))
}

/// `nameOf(hex)`: hex ? nm(parseInt(hex, 16)) : undefined (JS `undefined` and `null` both → None for `??`).
fn name_of(b: &Build, hex: &str) -> Option<V> {
    if hex.is_empty() {
        return None;
    }
    b.name_of(handle_num(hex))
}
fn nn(v: Option<V>) -> Option<V> {
    v.filter(|x| !matches!(x, V::Null))
}

pub fn read_mleader(rec: &Record, b: &Build) -> Res<Option<Obj>> {
    if b.buf.is_empty() {
        return Ok(None);
    }
    let (mut r, mut h) = readers(rec, b);
    let r = &mut r;
    let h = &mut h;
    let ver = b.ctx.ver;
    let hd = rec.handle;
    let mut f = Obj::new();
    if ver >= 24 {
        f.set("version", r.bs()?);
    }
    let mut ctxo = context(r, h, hd, ver)?;
    f.set("context", Obj::new()); // keeps the JS key order; filled below
    let style_handle = hh(h, hd)?;
    f.set("styleHandle", style_handle.clone());
    f.set("overrideFlags", r.bl()?);
    f.set("leaderType", r.bs()?);
    f.set("leaderColor", r.cmc()?);
    let lt_handle = hh(h, hd)?;
    f.set("leaderLinetypeHandle", lt_handle.clone());
    f.set("leaderLineweight", r.bl()?);
    f.set("landingEnabled", r.bool()?);
    f.set("doglegEnabled", r.bool()?);
    f.set("landingDistance", r.bd()?);
    let arrow_handle = hh(h, hd)?;
    f.set("arrowheadHandle", arrow_handle.clone());
    f.set("arrowSize", r.bd()?);
    f.set("contentType", r.bs()?);
    f.set("styleTextStyleHandle", hh(h, hd)?);
    f.set("textLeftAttach", r.bs()?);
    f.set("textRightAttach", r.bs()?);
    f.set("textAngleType", r.bs()?);
    f.set("unknown175", r.bs()?);
    f.set("textColor", r.cmc()?);
    f.set("textFrame", r.bool()?);
    f.set("styleBlockHandle", hh(h, hd)?);
    f.set("blockColor", r.cmc()?);
    f.set("blockScale", r.bd3()?);
    f.set("blockRotation", r.bd()?);
    f.set("blockAttach", r.bs()?);
    f.set("annotative", r.bool()?);
    if ver < 24 {
        let n0 = r.bl()?;
        let na = count(r, n0, 2.0)?;
        let mut a: Vec<V> = Vec::new();
        for _ in 0..na {
            a.push(obj! { "isDefault" => r.bool()?, "handle" => hh(h, hd)? }.into());
        }
        f.set("arrowheads", a);
    }
    let n0 = r.bl()?;
    let nl = count(r, n0, 4.0)?;
    let mut labels: Vec<V> = Vec::new();
    for _ in 0..nl {
        labels.push(obj! { "attdefHandle" => hh(h, hd)?, "text" => r.tv()?, "index" => r.bs()?, "width" => r.bd()? }.into());
    }
    f.set("blockLabels", labels);
    f.set("textDirectionNegative", r.bool()?);
    f.set("ipeAlign", r.bs()?);
    f.set("justification", r.bs()?);
    f.set("scale", r.bd()?);
    if ver >= 24 {
        f.set("attachDirection", r.bs()?);
        f.set("textTopAttach", r.bs()?);
        f.set("textBottomAttach", r.bs()?);
    }
    if ver >= 27 {
        f.set("extendToText", r.bool()?);
    }
    // names, for programs that look things up by name (nameOf takes the numeric handle)
    f.set("style", nn(name_of(b, &style_handle)).unwrap_or_else(|| V::Str(style_handle.clone())));
    let block_handle = ctxo.obj("block").and_then(|bl| bl.str("blockHandle")).map(str::to_string);
    if let Some(bh) = block_handle {
        let name = name_of(b, &bh);
        if let Some(bl) = ctxo.obj_mut("block") {
            bl.set_opt("name", name.clone());
        }
        f.set("contentBlock", nn(name).unwrap_or(V::Str(bh)));
    }
    let text_style = ctxo.obj("text").and_then(|t| t.str("styleHandle")).map(str::to_string);
    if let Some(sh) = text_style {
        f.set_opt("textStyle", name_of(b, &sh));
    }
    f.set_opt("leaderLinetype", name_of(b, &lt_handle));
    f.set_opt("arrowheadName", name_of(b, &arrow_handle));
    f.set("context", ctxo);
    // the proxy graphics (ch. 29) programs without multileaders draw; kept for the writer
    if let Some(g) = rec.gfx_pos
        && rec.graphics > 0.0
    {
        let end = to_usize(usize_to_f64(g) + rec.graphics * 8.0);
        f.set("proxyGraphics", b64(&slice_bits(b.buf, g, end)));
    }
    Ok(Some(f))
}

fn pair(r: &mut BitReader) -> Res<V> {
    Ok(V::Arr(vec![r.bd3()?, r.bd3()?]))
}

/// 20.4.86 MLeaderAnnotContext as embedded in MLEADER.
fn context(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32) -> Res<Obj> {
    let mut roots: Vec<V> = Vec::new();
    let n0 = r.bl()?;
    let nr = count(r, n0, 8.0)?;
    for _ in 0..nr {
        let mut root = obj! { "contentValid" => r.bool()?, "unknown" => r.bool()?, "connection" => r.bd3()?, "direction" => r.bd3()? };
        let n0 = r.bl()?;
        let nb = count(r, n0, 12.0)?;
        let mut breaks: Vec<V> = Vec::new();
        for _ in 0..nb {
            breaks.push(pair(r)?);
        }
        root.set("breaks", breaks);
        root.set("lines", Vec::<V>::new());
        root.set("index", r.bl()?);
        root.set("landingDistance", r.bd()?);
        let n0 = r.bl()?;
        let nl = count(r, n0, 4.0)?;
        let mut lines: Vec<V> = Vec::new();
        for _ in 0..nl {
            let mut line = Obj::new();
            let n0 = r.bl()?;
            let np = count(r, n0, 6.0)?;
            let mut pts: Vec<V> = Vec::new();
            for _ in 0..np {
                pts.push(r.bd3()?);
            }
            line.set("points", pts);
            let bic = r.bl()?;
            line.set("breakInfoCount", bic);
            if bic != 0 {
                line.set("segmentIndex", r.bl()?);
                let n0 = r.bl()?;
                let ns = count(r, n0, 12.0)?;
                let mut lb: Vec<V> = Vec::new();
                for _ in 0..ns {
                    lb.push(pair(r)?);
                }
                line.set("breaks", lb);
            }
            line.set("index", r.bl()?);
            if ver >= 24 {
                line.set("leaderType", r.bs()?);
                line.set("color", r.cmc()?);
                line.set("linetypeHandle", hh(h, hd)?);
                line.set("lineweight", r.bl()?);
                line.set("arrowSize", r.bd()?);
                line.set("arrowheadHandle", hh(h, hd)?);
                line.set("overrideFlags", r.bl()?);
            }
            lines.push(line.into());
        }
        root.set("lines", lines);
        if ver >= 24 {
            root.set("attachDirection", r.bs()?);
        }
        roots.push(root.into());
    }
    let mut c = obj! { "roots" => roots };
    c.set("scale", r.bd()?);
    c.set("contentBase", r.bd3()?);
    c.set("textHeight", r.bd()?);
    c.set("arrowSize", r.bd()?);
    c.set("landingGap", r.bd()?);
    c.set("leftAttach", r.bs()?);
    c.set("rightAttach", r.bs()?);
    c.set("textAlign", r.bs()?);
    c.set("attachType", r.bs()?);
    let has_text = r.bool()?;
    c.set("hasText", has_text);
    if has_text {
        let mut t = Obj::new();
        t.set("label", r.tv()?);
        t.set("normal", r.bd3()?);
        t.set("styleHandle", hh(h, hd)?);
        t.set("location", r.bd3()?);
        t.set("direction", r.bd3()?);
        t.set("rotation", r.bd()?);
        t.set("width", r.bd()?);
        t.set("height", r.bd()?);
        t.set("lineSpacing", r.bd()?);
        t.set("lineSpacingStyle", r.bs()?);
        t.set("color", r.cmc()?);
        t.set("alignment", r.bs()?);
        t.set("flow", r.bs()?);
        t.set("bgColor", r.cmc()?);
        t.set("bgScale", r.bd()?);
        t.set("bgTransparency", r.bl()?);
        t.set("bgFill", r.bool()?);
        t.set("bgMask", r.bool()?);
        t.set("columnType", r.bs()?);
        t.set("autoHeight", r.bool()?);
        t.set("columnWidth", r.bd()?);
        t.set("columnGutter", r.bd()?);
        t.set("columnFlowReversed", r.bool()?);
        let n0 = r.bl()?;
        let ncs = count(r, n0, 2.0)?;
        let mut cs: Vec<V> = Vec::new();
        for _ in 0..ncs {
            cs.push(V::Num(r.bd()?));
        }
        t.set("columnSizes", cs);
        t.set("wordBreak", r.bool()?);
        t.set("unknown", r.bool()?);
        c.set("text", t);
        c.set("hasBlock", false);
    } else {
        let has_block = r.bool()?;
        c.set("hasBlock", has_block);
        if has_block {
            let mut bl = Obj::new();
            bl.set("blockHandle", hh(h, hd)?);
            bl.set("normal", r.bd3()?);
            bl.set("location", r.bd3()?);
            bl.set("scale", r.bd3()?);
            bl.set("rotation", r.bd()?);
            bl.set("color", r.cmc()?);
            let mut m: Vec<V> = Vec::with_capacity(16);
            for _ in 0..16 {
                m.push(V::Num(r.bd()?));
            }
            bl.set("matrix", m);
            c.set("block", bl);
        }
    }
    c.set("basePoint", r.bd3()?);
    c.set("baseDirection", r.bd3()?);
    c.set("baseVertical", r.bd3()?);
    c.set("normalReversed", r.bool()?);
    if ver >= 24 {
        c.set("topAttach", r.bs()?);
        c.set("bottomAttach", r.bs()?);
    }
    Ok(c)
}

/// 20.4.87 MLEADERSTYLE. IsNewFormat: R2010+, or the ACAD_MLEADERVER extended data (the only EED it carries).
pub fn read_mleader_style(rec: &Record, b: &Build) -> Res<Option<Obj>> {
    if b.buf.is_empty() {
        return Ok(None);
    }
    let (mut r, mut h) = readers(rec, b);
    let r = &mut r;
    let h = &mut h;
    let ver = b.ctx.ver;
    let hd = rec.handle;
    let mut f = Obj::new();
    if ver >= 24 {
        f.set("styleVersion", r.bs()?);
    }
    f.set("contentType", r.bs()?);
    f.set("drawMLeaderOrder", r.bs()?);
    f.set("drawLeaderOrder", r.bs()?);
    f.set("maxPoints", r.bl()?);
    f.set("firstAngle", r.bd()?);
    f.set("secondAngle", r.bd()?);
    f.set("leaderType", r.bs()?);
    f.set("leaderColor", r.cmc()?);
    f.set("leaderLinetype", hh(h, hd)?);
    f.set("leaderLineweight", r.bl()?);
    f.set("landingEnabled", r.bool()?);
    f.set("landingGap", r.bd()?);
    f.set("doglegEnabled", r.bool()?);
    f.set("landingDistance", r.bd()?);
    f.set("description", r.tv()?);
    f.set("arrowhead", hh(h, hd)?);
    f.set("arrowSize", r.bd()?);
    f.set("textDefault", r.tv()?);
    f.set("textStyle", hh(h, hd)?);
    f.set("leftAttach", r.bs()?);
    f.set("rightAttach", r.bs()?);
    let new_format = ver >= 24 || rec.eed.is_some();
    if new_format {
        f.set("textAngleType", r.bs()?);
    }
    f.set("textAlign", r.bs()?);
    f.set("textColor", r.cmc()?);
    f.set("textHeight", r.bd()?);
    f.set("textFrame", r.bool()?);
    if new_format {
        f.set("alwaysAlignLeft", r.bool()?);
    }
    f.set("alignSpace", r.bd()?);
    f.set("block", hh(h, hd)?);
    f.set("blockColor", r.cmc()?);
    f.set("blockScale", r.bd3()?);
    f.set("blockScaleEnabled", r.bool()?);
    f.set("blockRotation", r.bd()?);
    f.set("blockRotationEnabled", r.bool()?);
    f.set("blockConnection", r.bs()?);
    f.set("scale", r.bd()?);
    f.set("changed", r.bool()?);
    f.set("annotative", r.bool()?);
    f.set("breakSize", r.bd()?);
    if ver >= 24 {
        f.set("attachDirection", r.bs()?);
        f.set("topAttach", r.bs()?);
        f.set("bottomAttach", r.bs()?);
    }
    // Spec gap: R2013+ styles have one more bit after the bottom attachment — the ODA File Converter writes it as DXF
    // 298 (0 in every corpus style); found by the one bit left over in the R2013/R2018 corpus styles.
    if ver >= 27 {
        f.set("unknown298", r.bool()?);
    }
    Ok(Some(f))
}
