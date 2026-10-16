//! Type decoders for tables and non-graphical objects (port of lib/dwg/objects-read.js, 20.4.44/45 and
//! 20.4.51 onward). Field names are the JS record's (`rec.<name>` → `rec.f`, `rec._h.<name>` → `rec.h`).

use crate::bits::{BitReader, Res, err};
use crate::messages::msg;
use crate::objects::{Ctx, Decoder, Record, hrefs, rest_handles, table_entry, table_entry_handles};
use crate::value::{Obj, V, p2};

pub fn decoder(name: &str) -> Option<Decoder> {
    Some(match name {
        "DICTIONARY" => dictionary_plain,
        "DICTIONARYWDFLT" | "ACDBDICTIONARYWDFLT" => dictionary_wdflt,
        "BLOCK_CONTROL" | "LAYER_CONTROL" | "STYLE_CONTROL" | "LTYPE_CONTROL" | "VIEW_CONTROL" | "UCS_CONTROL" | "VPORT_CONTROL"
        | "APPID_CONTROL" | "DIMSTYLE_CONTROL" | "VPENT_CONTROL" => control,
        "BLOCK_HEADER" => block_header,
        "LAYER" => layer,
        "STYLE" => style,
        "LTYPE" => ltype,
        "VIEW" => view,
        "UCS" => ucs,
        "VPORT" => vport,
        "APPID" => appid,
        "DIMSTYLE" => dimstyle,
        "VPENT_HEADER" => vpent_header,
        _ => return crate::dec_objects2::decoder(name),
    })
}

// ---- 20.4.44/45 DICTIONARY
fn dictionary(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx, wdflt: bool) -> Res<()> {
    let n = r.bl()?;
    if !(0..=10_000_000).contains(&n) {
        return err(msg("ccdwg.err.dictionary_size", &[("n", &n.to_string())]));
    }
    if ctx.ver == 14 {
        r.rc()?;
    }
    if ctx.ver >= 15 {
        let c = r.bs()?;
        rec.f.set("cloning", c);
        let ho = r.rc()?;
        rec.f.set("hardOwner", ho);
    }
    // JS `new Array(n)` then filled: a failure part way leaves holes (null in JSON). The array grows as names are
    // read (a damaged count of up to 10^7 must not allocate up front); on a failure the holes are added back, up to
    // a cap (JS arrays are sparse, ours are not).
    let nu = crate::util::usize_from_i32(n);
    let mut names: Vec<V> = Vec::new();
    let mut failed = None;
    for _ in 0..nu {
        match r.tv() {
            Ok(s) => names.push(V::Str(s)),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    if let Some(e) = failed {
        if nu <= 1_000_000 {
            names.resize(nu, V::Null);
        }
        rec.f.set("names", names);
        return Err(e);
    }
    rec.f.set("names", names);
    let items = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("items", items);
    if wdflt {
        let d = h.h(rec.handle)?;
        rec.h.set("defaultEntry", d);
    }
    Ok(())
}
fn dictionary_plain(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dictionary(r, h, rec, ctx, false)
}
fn dictionary_wdflt(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    dictionary(r, h, rec, ctx, true)
}

// ---- 20.4.51–70 tables
fn control(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let n = r.bl()?;
    rec.f.set("count", n);
    if rec.typ == "DIMSTYLE_CONTROL" && ctx.ver >= 15 {
        let x = r.rc()?;
        rec.f.set("_extra", x);
    }
    let e = rest_handles(h, rec.handle, 1_000_000);
    rec.h.set("entries", e);
    Ok(())
}

fn block_header(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    table_entry(r, rec, ctx)?;
    for k in ["anonymous", "hasAttribs", "isXref", "isOverlaid"] {
        let v = r.bool()?;
        rec.f.set(k, v);
    }
    if ver >= 15 {
        let v = r.bool()?;
        rec.f.set("loaded", v);
    }
    let is_x = rec.f.truthy("isXref") || rec.f.truthy("isOverlaid");
    let mut owned = 0;
    if ver >= 18 && !is_x {
        owned = r.bl()?;
    }
    let b = r.bd3()?;
    rec.f.set("base", b);
    let x = r.tv()?;
    rec.f.set("xrefPath", x);
    let mut inserts: u32 = 0;
    if ver >= 15 {
        while r.rc()? != 0 {
            inserts = inserts.saturating_add(1);
        }
        let d = r.tv()?;
        rec.f.set("description", d);
        let n = r.bl()?;
        if n > 0 {
            let p = r.bytes(crate::util::usize_from_i32(n))?;
            rec.f.set("preview", V::Bytes(p));
        }
    }
    if ver >= 21 {
        let iu = r.bs()?;
        rec.f.set("insertUnits", iu);
        let ex = r.bool()?;
        rec.f.set("explodable", ex);
        let sc = r.rc()?;
        rec.f.set("scalable", sc);
    }
    table_entry_handles(h, rec)?;
    let hd = rec.handle;
    let b = h.h(hd)?;
    rec.h.set("block", b);
    if ver < 18 {
        if !is_x {
            let f = h.h(hd)?;
            rec.h.set("firstEntity", f);
            let l = h.h(hd)?;
            rec.h.set("lastEntity", l);
        }
    } else {
        let e = hrefs(h, hd, f64::from(owned))?;
        rec.h.set("entities", e);
    }
    let e = h.h(hd)?;
    rec.h.set("endblk", e);
    if ver >= 15 {
        let i = hrefs(h, hd, f64::from(inserts))?;
        rec.h.set("inserts", i);
        let l = h.h(hd)?;
        rec.h.set("layout", l);
    }
    Ok(())
}

fn layer(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    if ctx.ver < 15 {
        let fr = r.bool()?;
        rec.f.set("frozen", fr);
        let on = !r.bool()?;
        rec.f.set("on", on);
        let fnw = r.bool()?;
        rec.f.set("frozenNew", fnw);
        let lk = r.bool()?;
        rec.f.set("locked", lk);
        rec.f.set("plot", true);
        rec.f.set("lineweight", -3);
    } else {
        let v = r.bs()?;
        rec.f.set("_values", v);
        rec.f.set("frozen", v & 1 != 0);
        rec.f.set("on", v & 2 == 0);
        rec.f.set("frozenNew", v & 4 != 0);
        rec.f.set("locked", v & 8 != 0);
        rec.f.set("plot", v & 16 != 0);
        rec.f.set("_lwIndex", (v & 0x3E0) >> 5);
    }
    rec.color = Some(r.cmc()?);
    table_entry_handles(h, rec)?;
    let hd = rec.handle;
    if ctx.ver >= 15 {
        let v = h.h(hd)?;
        rec.h.set("plotstyle", v);
    }
    if ctx.ver >= 21 {
        let v = h.h(hd)?;
        rec.h.set("material", v);
    }
    let v = h.h(hd)?;
    rec.h.set("ltype", v);
    if ctx.ver >= 27 {
        let v = h.h(hd)?;
        rec.h.set("unknown", v);
    }
    Ok(())
}

fn style(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    let v = r.bool()?;
    rec.f.set("vertical", v);
    let s = r.bool()?;
    rec.f.set("isShapeFile", s);
    for k in ["height", "widthFactor", "oblique"] {
        let x = r.bd()?;
        rec.f.set(k, x);
    }
    let g = r.rc()?;
    rec.f.set("generation", g);
    let lh = r.bd()?;
    rec.f.set("lastHeight", lh);
    let f = r.tv()?;
    rec.f.set("font", f);
    let bf = r.tv()?;
    rec.f.set("bigFont", bf);
    table_entry_handles(h, rec)
}

fn ltype(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    let d = r.tv()?;
    rec.f.set("description", d);
    let pl = r.bd()?;
    rec.f.set("patternLength", pl);
    let al = r.rc()?;
    rec.f.set("alignment", al);
    let n = r.rc()?;
    rec.f.set("elements", Vec::<V>::new());
    let mut any_text = false;
    let mut shapes: Vec<(i32, i32)> = Vec::new(); // (shape, flags) per element
    for _ in 0..n {
        let length = r.bd()?;
        let shape = r.bs()?;
        let ox = r.rd()?;
        let oy = r.rd()?;
        let scale = r.bd()?;
        let rotation = r.bd()?;
        let flags = r.bs()?;
        if flags & 2 != 0 {
            any_text = true;
        }
        let e = crate::obj! { "length" => length, "shape" => shape, "offset" => p2(ox, oy), "scale" => scale, "rotation" => rotation, "flags" => flags };
        shapes.push((shape, flags));
        if let Some(V::Arr(a)) = rec.f.get_mut("elements") {
            a.push(V::Obj(e));
        }
    }
    let area = if ctx.ver <= 18 {
        Some(r.bytes(256)?)
    } else if any_text {
        Some(r.bytes(512)?)
    } else {
        None
    };
    if let Some(area) = area
        && let Some(V::Arr(els)) = rec.f.get_mut("elements")
    {
        for (e, &(shape, flags)) in els.iter_mut().zip(shapes.iter()) {
            if flags & 2 == 0 {
                continue;
            }
            let mut s = String::new();
            let start = crate::util::usize_from_i32(shape);
            if ctx.ver <= 18 {
                if shape >= 0 {
                    for &c in area.get(start..).unwrap_or_default() {
                        if c == 0 {
                            break;
                        }
                        s.push(char::from(c));
                    }
                }
            } else {
                let mut units = Vec::new();
                let mut k = if shape >= 0 { start.saturating_mul(2) } else { 0 };
                while k.saturating_add(1) < area.len() {
                    let c = u16::from(area.get(k).copied().unwrap_or(0))
                        | (u16::from(area.get(k.saturating_add(1)).copied().unwrap_or(0)) << 8);
                    if c == 0 {
                        break;
                    }
                    units.push(c);
                    k = k.saturating_add(2);
                }
                s = String::from_utf16_lossy(&units);
            }
            if let V::Obj(o) = e {
                o.set("text", s);
            }
        }
    }
    table_entry_handles(h, rec)?;
    let sh = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("shapes", sh);
    Ok(())
}

fn set_bd(r: &mut BitReader, rec: &mut Record, keys: &[&'static str]) -> Res<()> {
    for k in keys {
        let v = r.bd()?;
        rec.f.set(*k, v);
    }
    Ok(())
}
fn set_bd3(r: &mut BitReader, rec: &mut Record, keys: &[&'static str]) -> Res<()> {
    for k in keys {
        let v = r.bd3()?;
        rec.f.set(*k, v);
    }
    Ok(())
}
fn lights(r: &mut BitReader, rec: &mut Record) -> Res<()> {
    let u = r.bool()?;
    rec.f.set("useDefaultLights", u);
    let t = r.rc()?;
    rec.f.set("defaultLightingType", t);
    set_bd(r, rec, &["brightness", "contrast"])?;
    let c = r.cmc()?;
    rec.f.set("ambientColor", c);
    Ok(())
}
fn hset(h: &mut BitReader, rec: &mut Record, keys: &[&'static str]) -> Res<()> {
    for k in keys {
        let v = h.h(rec.handle)?;
        rec.h.set(*k, v);
    }
    Ok(())
}

fn view(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    table_entry(r, rec, ctx)?;
    set_bd(r, rec, &["height", "width"])?;
    let c = r.rd2()?;
    rec.f.set("center", c);
    set_bd3(r, rec, &["target", "direction"])?;
    set_bd(r, rec, &["twist", "lensLength", "frontClip", "backClip"])?;
    let vm = r.bits(4)?;
    rec.f.set("viewMode", vm);
    if ver >= 15 {
        let rm = r.rc()?;
        rec.f.set("renderMode", rm);
    }
    if ver >= 21 {
        lights(r, rec)?;
    }
    let ps = r.bool()?;
    rec.f.set("paperSpace", ps);
    let mut assoc = false;
    if ver >= 15 {
        assoc = r.bool()?;
        rec.f.set("associatedUcs", assoc);
        if assoc {
            set_bd3(r, rec, &["ucsOrigin", "ucsXAxis", "ucsYAxis"])?;
            set_bd(r, rec, &["ucsElevation"])?;
            let t = r.bs()?;
            rec.f.set("ucsOrthoType", t);
        }
    }
    if ver >= 21 {
        let cp = r.bool()?;
        rec.f.set("cameraPlottable", cp);
    }
    table_entry_handles(h, rec)?;
    if ver >= 21 {
        hset(h, rec, &["background", "visualStyle", "sun"])?;
    }
    if ver >= 15 && assoc {
        hset(h, rec, &["baseUcs", "namedUcs"])?;
    }
    if ver >= 21 {
        hset(h, rec, &["liveSection"])?;
    }
    Ok(())
}

fn ucs(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    set_bd3(r, rec, &["origin", "xAxis", "yAxis"])?;
    if ctx.ver >= 15 {
        set_bd(r, rec, &["elevation"])?;
        let a = r.bs()?;
        rec.f.set("orthoViewType", a);
        let b = r.bs()?;
        rec.f.set("orthoType", b);
    }
    table_entry_handles(h, rec)
}

fn vport(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    table_entry(r, rec, ctx)?;
    set_bd(r, rec, &["viewHeight", "aspectRatio"])?;
    let c = r.rd2()?;
    rec.f.set("viewCenter", c);
    set_bd3(r, rec, &["viewTarget", "viewDirection"])?;
    set_bd(r, rec, &["twist", "lensLength", "frontClip", "backClip"])?;
    let vm = r.bits(4)?;
    rec.f.set("viewMode", vm);
    if ver >= 15 {
        let rm = r.rc()?;
        rec.f.set("renderMode", rm);
    }
    if ver >= 21 {
        lights(r, rec)?;
    }
    let ll = r.rd2()?;
    rec.f.set("lowerLeft", ll);
    let ur = r.rd2()?;
    rec.f.set("upperRight", ur);
    let uf = r.bool()?;
    rec.f.set("ucsFollow", uf);
    let cz = r.bs()?;
    rec.f.set("circleZoom", cz);
    let fz = r.bool()?;
    rec.f.set("fastZoom", fz);
    let ui = r.bits(2)?;
    rec.f.set("ucsIcon", ui);
    let go = r.bool()?;
    rec.f.set("gridOn", go);
    let gs = r.rd2()?;
    rec.f.set("gridSpacing", gs);
    let so = r.bool()?;
    rec.f.set("snapOn", so);
    let ss = r.bool()?;
    rec.f.set("snapStyle", ss);
    let ip = r.bs()?;
    rec.f.set("snapIsoPair", ip);
    set_bd(r, rec, &["snapRotation"])?;
    let sb = r.rd2()?;
    rec.f.set("snapBase", sb);
    let sp = r.rd2()?;
    rec.f.set("snapSpacing", sp);
    if ver >= 15 {
        r.b()?;
        let upv = r.bool()?;
        rec.f.set("ucsPerViewport", upv);
        set_bd3(r, rec, &["ucsOrigin", "ucsXAxis", "ucsYAxis"])?;
        set_bd(r, rec, &["ucsElevation"])?;
        let t = r.bs()?;
        rec.f.set("ucsOrthoType", t);
    }
    if ver >= 21 {
        let gf = r.bs()?;
        rec.f.set("gridFlags", gf);
        let gm = r.bs()?;
        rec.f.set("gridMajor", gm);
    }
    table_entry_handles(h, rec)?;
    if ver >= 21 {
        hset(h, rec, &["background", "visualStyle", "sun"])?;
    }
    if ver >= 15 {
        hset(h, rec, &["namedUcs", "baseUcs"])?;
    }
    Ok(())
}

fn appid(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    let u = r.rc()?;
    rec.f.set("unknown", u);
    table_entry_handles(h, rec)
}

#[derive(Clone, Copy)]
enum Ty {
    B,
    Bs,
    Bd,
    Rc,
    Tv,
    T,
    Cmc,
}
const DIMSTYLE_R2000: &[(&str, Ty, u32)] = &[
    ("DIMPOST", Ty::Tv, 0),
    ("DIMAPOST", Ty::Tv, 0),
    ("DIMSCALE", Ty::Bd, 0),
    ("DIMASZ", Ty::Bd, 0),
    ("DIMEXO", Ty::Bd, 0),
    ("DIMDLI", Ty::Bd, 0),
    ("DIMEXE", Ty::Bd, 0),
    ("DIMRND", Ty::Bd, 0),
    ("DIMDLE", Ty::Bd, 0),
    ("DIMTP", Ty::Bd, 0),
    ("DIMTM", Ty::Bd, 0),
    ("DIMFXL", Ty::Bd, 21),
    ("DIMJOGANG", Ty::Bd, 21),
    ("DIMTFILL", Ty::Bs, 21),
    ("DIMTFILLCLR", Ty::Cmc, 21),
    ("DIMTOL", Ty::B, 0),
    ("DIMLIM", Ty::B, 0),
    ("DIMTIH", Ty::B, 0),
    ("DIMTOH", Ty::B, 0),
    ("DIMSE1", Ty::B, 0),
    ("DIMSE2", Ty::B, 0),
    ("DIMTAD", Ty::Bs, 0),
    ("DIMZIN", Ty::Bs, 0),
    ("DIMAZIN", Ty::Bs, 0),
    ("DIMARCSYM", Ty::Bs, 21),
    ("DIMTXT", Ty::Bd, 0),
    ("DIMCEN", Ty::Bd, 0),
    ("DIMTSZ", Ty::Bd, 0),
    ("DIMALTF", Ty::Bd, 0),
    ("DIMLFAC", Ty::Bd, 0),
    ("DIMTVP", Ty::Bd, 0),
    ("DIMTFAC", Ty::Bd, 0),
    ("DIMGAP", Ty::Bd, 0),
    ("DIMALTRND", Ty::Bd, 0),
    ("DIMALT", Ty::B, 0),
    ("DIMALTD", Ty::Bs, 0),
    ("DIMTOFL", Ty::B, 0),
    ("DIMSAH", Ty::B, 0),
    ("DIMTIX", Ty::B, 0),
    ("DIMSOXD", Ty::B, 0),
    ("DIMCLRD", Ty::Cmc, 0),
    ("DIMCLRE", Ty::Cmc, 0),
    ("DIMCLRT", Ty::Cmc, 0),
    ("DIMADEC", Ty::Bs, 0),
    ("DIMDEC", Ty::Bs, 0),
    ("DIMTDEC", Ty::Bs, 0),
    ("DIMALTU", Ty::Bs, 0),
    ("DIMALTTD", Ty::Bs, 0),
    ("DIMAUNIT", Ty::Bs, 0),
    ("DIMFRAC", Ty::Bs, 0),
    ("DIMLUNIT", Ty::Bs, 0),
    ("DIMDSEP", Ty::Bs, 0),
    ("DIMTMOVE", Ty::Bs, 0),
    ("DIMJUST", Ty::Bs, 0),
    ("DIMSD1", Ty::B, 0),
    ("DIMSD2", Ty::B, 0),
    ("DIMTOLJ", Ty::Bs, 0),
    ("DIMTZIN", Ty::Bs, 0),
    ("DIMALTZ", Ty::Bs, 0),
    ("DIMALTTZ", Ty::Bs, 0),
    ("DIMUPT", Ty::B, 0),
    ("DIMATFIT", Ty::Bs, 0),
    ("DIMFXLON", Ty::B, 21),
    ("DIMTXTDIRECTION", Ty::B, 24),
    ("DIMALTMZF", Ty::Bd, 24),
    ("DIMALTMZS", Ty::Tv, 24),
    ("DIMMZF", Ty::Bd, 24),
    ("DIMMZS", Ty::Tv, 24),
    ("DIMLWD", Ty::Bs, 0),
    ("DIMLWE", Ty::Bs, 0),
];
const DIMSTYLE_R13: &[(&str, Ty, u32)] = &[
    ("DIMTOL", Ty::B, 0),
    ("DIMLIM", Ty::B, 0),
    ("DIMTIH", Ty::B, 0),
    ("DIMTOH", Ty::B, 0),
    ("DIMSE1", Ty::B, 0),
    ("DIMSE2", Ty::B, 0),
    ("DIMALT", Ty::B, 0),
    ("DIMTOFL", Ty::B, 0),
    ("DIMSAH", Ty::B, 0),
    ("DIMTIX", Ty::B, 0),
    ("DIMSOXD", Ty::B, 0),
    ("DIMALTD", Ty::Rc, 0),
    ("DIMZIN", Ty::Rc, 0),
    ("DIMSD1", Ty::B, 0),
    ("DIMSD2", Ty::B, 0),
    ("DIMTOLJ", Ty::Rc, 0),
    ("DIMJUST", Ty::Rc, 0),
    ("DIMFIT", Ty::Rc, 0),
    ("DIMUPT", Ty::B, 0),
    ("DIMTZIN", Ty::Rc, 0),
    ("DIMALTZ", Ty::Rc, 0),
    ("DIMALTTZ", Ty::Rc, 0),
    ("DIMTAD", Ty::Rc, 0),
    ("DIMUNIT", Ty::Bs, 0),
    ("DIMAUNIT", Ty::Bs, 0),
    ("DIMDEC", Ty::Bs, 0),
    ("DIMTDEC", Ty::Bs, 0),
    ("DIMALTU", Ty::Bs, 0),
    ("DIMALTTD", Ty::Bs, 0),
    ("DIMSCALE", Ty::Bd, 0),
    ("DIMASZ", Ty::Bd, 0),
    ("DIMEXO", Ty::Bd, 0),
    ("DIMDLI", Ty::Bd, 0),
    ("DIMEXE", Ty::Bd, 0),
    ("DIMRND", Ty::Bd, 0),
    ("DIMDLE", Ty::Bd, 0),
    ("DIMTP", Ty::Bd, 0),
    ("DIMTM", Ty::Bd, 0),
    ("DIMTXT", Ty::Bd, 0),
    ("DIMCEN", Ty::Bd, 0),
    ("DIMTSZ", Ty::Bd, 0),
    ("DIMALTF", Ty::Bd, 0),
    ("DIMLFAC", Ty::Bd, 0),
    ("DIMTVP", Ty::Bd, 0),
    ("DIMTFAC", Ty::Bd, 0),
    ("DIMGAP", Ty::Bd, 0),
    ("DIMPOST", Ty::T, 0),
    ("DIMAPOST", Ty::T, 0),
    ("DIMBLK_NAME", Ty::T, 0),
    ("DIMBLK1_NAME", Ty::T, 0),
    ("DIMBLK2_NAME", Ty::T, 0),
    ("DIMCLRD", Ty::Bs, 0),
    ("DIMCLRE", Ty::Bs, 0),
    ("DIMCLRT", Ty::Bs, 0),
];

fn dimstyle(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    let mut vars = Obj::with_capacity(72);
    for &(name, t, from) in if ctx.ver < 15 { DIMSTYLE_R13 } else { DIMSTYLE_R2000 } {
        if from != 0 && ctx.ver < from {
            continue;
        }
        let v: V = match t {
            Ty::B => V::from(r.b()?),
            Ty::Bs => V::from(r.bs()?),
            Ty::Bd => V::Num(r.bd()?),
            Ty::Rc => V::from(r.rc()?),
            Ty::Tv => V::Str(r.tv()?),
            Ty::T => V::Str(r.t()?),
            Ty::Cmc => V::Obj(r.cmc()?),
        };
        vars.set(name, v);
    }
    let ub = r.b()?;
    rec.f.set("_unknownBit", ub);
    rec.f.set("vars", vars);
    table_entry_handles(h, rec)?;
    hset(h, rec, &["DIMTXSTY"])?;
    if ctx.ver >= 15 {
        hset(h, rec, &["DIMLDRBLK", "DIMBLK", "DIMBLK1", "DIMBLK2"])?;
    }
    if ctx.ver >= 21 {
        hset(h, rec, &["DIMLTYPE", "DIMLTEX1", "DIMLTEX2"])?;
    }
    Ok(())
}

fn vpent_header(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    table_entry(r, rec, ctx)?;
    let f = r.b()?;
    rec.f.set("flag1", f);
    table_entry_handles(h, rec)?;
    hset(h, rec, &["viewport", "prevHeader"])
}
