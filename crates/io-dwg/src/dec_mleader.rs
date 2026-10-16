//! MULTILEADER / MLEADER / MLEADERSTYLE record decoders (port of lib/dwg/objects-read.js 20.4.48, 20.4.86,
//! 20.4.87: DECODERS.MULTILEADER, annotContext, DECODERS.MLEADERSTYLE).
//!
//! Fields are set as soon as they are read, so a record that fails part way keeps the same partial fields as the
//! JS record (objects assigned before they are filled — `rec.context`, `c.text`, `c.block`, `rec.arrowheads` — are
//! committed even when filling them fails; objects pushed after they are built are not).

use crate::bits::{BitReader, Res};
use crate::obj;
use crate::objects::{Ctx, Decoder, Record};
use crate::value::{Obj, V};

pub fn decoder(name: &str) -> Option<Decoder> {
    Some(match name {
        "MULTILEADER" | "MLEADER" => multileader,
        "MLEADERSTYLE" => mleaderstyle,
        _ => return None,
    })
}

/// `o[k].push(v)` for an array field.
fn push(o: &mut Obj, k: &str, v: impl Into<V>) {
    if let Some(V::Arr(a)) = o.get_mut(k) {
        a.push(v.into());
    }
}

// ---- 20.4.48/86 MULTILEADER
fn multileader(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let hd = rec.handle;
    if ver >= 24 {
        rec.f.set("mlVersion", r.bs()?);
    }
    annot_context(r, h, rec, ctx)?;
    rec.h.set("style", h.h(hd)?);
    rec.f.set("overrideFlags", r.bl()?);
    rec.f.set("leaderType", r.bs()?);
    rec.f.set("leaderColor", r.cmc()?);
    rec.h.set("leaderLtype", h.h(hd)?);
    rec.f.set("leaderLineweight", r.bl()?);
    rec.f.set("landingEnabled", r.bool()?);
    rec.f.set("doglegEnabled", r.bool()?);
    rec.f.set("landingDistance", r.bd()?);
    rec.h.set("arrowhead", h.h(hd)?);
    rec.f.set("arrowSize", r.bd()?);
    rec.f.set("contentType", r.bs()?);
    rec.h.set("textStyle", h.h(hd)?);
    rec.f.set("textLeftAttach", r.bs()?);
    rec.f.set("textRightAttach", r.bs()?);
    rec.f.set("textAngleType", r.bs()?);
    r.bs()?;
    rec.f.set("textColor", r.cmc()?);
    rec.f.set("textFrame", r.bool()?);
    rec.h.set("blockStyle", h.h(hd)?);
    rec.f.set("blockColor", r.cmc()?);
    rec.f.set("blockScale", r.bd3()?);
    rec.f.set("blockRotation", r.bd()?);
    rec.f.set("blockAttach", r.bs()?);
    rec.f.set("annotative", r.bool()?);
    if ver < 24 {
        let na = r.bl()?;
        rec.f.set("arrowheads", Vec::<V>::new());
        for _ in 0..na {
            let a = obj! { "isDefault" => r.bool()?, "_h" => h.h(hd)? };
            push(&mut rec.f, "arrowheads", a);
        }
    }
    let nl = r.bl()?;
    rec.f.set("blockLabels", Vec::<V>::new());
    for _ in 0..nl {
        let l = obj! { "_attdef" => h.h(hd)?, "text" => r.tv()?, "index" => r.bs()?, "width" => r.bd()? };
        push(&mut rec.f, "blockLabels", l);
    }
    rec.f.set("textDirectionNegative", r.bool()?);
    rec.f.set("ipeAlign", r.bs()?);
    rec.f.set("justification", r.bs()?);
    rec.f.set("scale", r.bd()?);
    if ver >= 24 {
        rec.f.set("attachDirection", r.bs()?);
        rec.f.set("textTopAttach", r.bs()?);
        rec.f.set("textBottomAttach", r.bs()?);
    }
    if ver >= 27 {
        rec.f.set("extendToText", r.bool()?);
    }
    Ok(())
}

/// `rec.context = { roots: [] }`, then filled; the partial context stays on the record when reading fails.
fn annot_context(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let mut c = obj! { "roots" => Vec::<V>::new() };
    let res = context_fill(r, h, rec.handle, ctx.ver, &mut c);
    rec.f.set("context", c);
    res
}

fn context_fill(r: &mut BitReader, h: &mut BitReader, hd: f64, ver: u32, c: &mut Obj) -> Res<()> {
    let nr = r.bl()?;
    for _ in 0..nr {
        let mut root = obj! {
            "contentValid" => r.bool()?, "unknown" => r.bool()?, "connection" => r.bd3()?, "direction" => r.bd3()?,
            "breaks" => Vec::<V>::new(), "lines" => Vec::<V>::new(),
        };
        let nb = r.bl()?;
        for _ in 0..nb {
            let br = vec![r.bd3()?, r.bd3()?];
            push(&mut root, "breaks", br);
        }
        root.set("index", r.bl()?);
        root.set("landingDistance", r.bd()?);
        let nl = r.bl()?;
        for _ in 0..nl {
            let mut line = obj! { "points" => Vec::<V>::new() };
            let np = r.bl()?;
            for _ in 0..np {
                let p = r.bd3()?;
                push(&mut line, "points", p);
            }
            let bic = r.bl()?;
            line.set("breakInfoCount", bic);
            if bic != 0 {
                line.set("segmentIndex", r.bl()?);
                let ns = r.bl()?;
                line.set("breaks", Vec::<V>::new());
                for _ in 0..ns {
                    let br = vec![r.bd3()?, r.bd3()?];
                    push(&mut line, "breaks", br);
                }
            }
            line.set("index", r.bl()?);
            if ver >= 24 {
                line.set("leaderType", r.bs()?);
                line.set("color", r.cmc()?);
                line.set("_ltype", h.h(hd)?);
                line.set("lineweight", r.bl()?);
                line.set("arrowSize", r.bd()?);
                line.set("_arrow", h.h(hd)?);
                line.set("overrideFlags", r.bl()?);
            }
            push(&mut root, "lines", line);
        }
        if ver >= 24 {
            root.set("attachDirection", r.bs()?);
        }
        push(c, "roots", root);
    }
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
        let res = text_fill(r, h, hd, &mut t);
        c.set("text", t);
        res?;
    } else {
        let has_block = r.bool()?;
        c.set("hasBlock", has_block);
        if has_block {
            let mut b = Obj::new();
            let res = block_fill(r, h, hd, &mut b);
            c.set("block", b);
            res?;
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
    Ok(())
}

fn text_fill(r: &mut BitReader, h: &mut BitReader, hd: f64, t: &mut Obj) -> Res<()> {
    t.set("label", r.tv()?);
    t.set("normal", r.bd3()?);
    t.set("_style", h.h(hd)?);
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
    let ncs = r.bl()?;
    t.set("columnSizes", Vec::<V>::new());
    for _ in 0..ncs {
        let s = r.bd()?;
        push(t, "columnSizes", s);
    }
    t.set("wordBreak", r.bool()?);
    r.b()?;
    Ok(())
}

fn block_fill(r: &mut BitReader, h: &mut BitReader, hd: f64, b: &mut Obj) -> Res<()> {
    b.set("_block", h.h(hd)?);
    b.set("normal", r.bd3()?);
    b.set("location", r.bd3()?);
    b.set("scale", r.bd3()?);
    b.set("rotation", r.bd()?);
    b.set("color", r.cmc()?);
    b.set("matrix", Vec::<V>::new());
    for _ in 0..16 {
        let m = r.bd()?;
        push(b, "matrix", m);
    }
    Ok(())
}

// ---- 20.4.87 MLEADERSTYLE
fn mleaderstyle(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let hd = rec.handle;
    if ver >= 24 {
        rec.f.set("styleVersion", r.bs()?);
    }
    rec.f.set("contentType", r.bs()?);
    rec.f.set("drawMLeaderOrder", r.bs()?);
    rec.f.set("drawLeaderOrder", r.bs()?);
    rec.f.set("maxPoints", r.bl()?);
    rec.f.set("firstAngle", r.bd()?);
    rec.f.set("secondAngle", r.bd()?);
    rec.f.set("leaderType", r.bs()?);
    rec.f.set("leaderColor", r.cmc()?);
    rec.h.set("leaderLtype", h.h(hd)?);
    rec.f.set("leaderLineweight", r.bl()?);
    rec.f.set("landingEnabled", r.bool()?);
    rec.f.set("landingGap", r.bd()?);
    rec.f.set("doglegEnabled", r.bool()?);
    rec.f.set("landingDistance", r.bd()?);
    rec.f.set("description", r.tv()?);
    rec.h.set("arrowhead", h.h(hd)?);
    rec.f.set("arrowSize", r.bd()?);
    rec.f.set("textDefault", r.tv()?);
    rec.h.set("textStyle", h.h(hd)?);
    rec.f.set("leftAttach", r.bs()?);
    rec.f.set("rightAttach", r.bs()?);
    // 20.4.87: "IsNewFormat" = R2010+, or extended data of ACAD_MLEADERVER (the only EED these objects carry)
    let new_format = ver >= 24 || rec.eed.is_some();
    if new_format {
        rec.f.set("textAngleType", r.bs()?);
    }
    rec.f.set("textAlign", r.bs()?);
    rec.f.set("textColor", r.cmc()?);
    rec.f.set("textHeight", r.bd()?);
    rec.f.set("textFrame", r.bool()?);
    if new_format {
        rec.f.set("alwaysAlignLeft", r.bool()?);
    }
    rec.f.set("alignSpace", r.bd()?);
    rec.h.set("block", h.h(hd)?);
    rec.f.set("blockColor", r.cmc()?);
    rec.f.set("blockScale", r.bd3()?);
    rec.f.set("blockScaleEnabled", r.bool()?);
    rec.f.set("blockRotation", r.bd()?);
    rec.f.set("blockRotationEnabled", r.bool()?);
    rec.f.set("blockConnection", r.bs()?);
    rec.f.set("scale", r.bd()?);
    rec.f.set("changed", r.bool()?);
    rec.f.set("annotative", r.bool()?);
    rec.f.set("breakSize", r.bd()?);
    if ver >= 24 {
        rec.f.set("attachDirection", r.bs()?);
        rec.f.set("topAttach", r.bs()?);
        rec.f.set("bottomAttach", r.bs()?);
    }
    Ok(())
}
