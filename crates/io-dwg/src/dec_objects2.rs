//! Type decoders for the remaining non-graphical objects and classed entities (port of lib/dwg/objects-read.js,
//! from 20.4.72 GROUP to XRECORD). ACAD_TABLE and FIELDLIST live in table_read.rs; MULTILEADER and MLEADERSTYLE in
//! dec_mleader.rs. Field names are the JS record's (`rec.<name>` → `rec.f`, `rec._h.<name>` → `rec.h`).

use crate::bits::{BitReader, Res, err};
use crate::messages::msg;
use crate::objects::{Ctx, Decoder, Record, fits, hrefs, rest_handles};
use crate::util::{hex_bytes, hex_of};
use crate::value::{Obj, V, p3};

pub fn decoder(name: &str) -> Option<Decoder> {
    Some(match name {
        "GROUP" => group,
        "MLINESTYLE" => mlinestyle,
        "DICTIONARYVAR" => dictionaryvar,
        "HATCH" => hatch,
        "IDBUFFER" => idbuffer,
        "IMAGE" | "WIPEOUT" => image_data,
        "IMAGEDEF" => imagedef,
        "IMAGEDEF_REACTOR" => imagedef_reactor,
        "LAYER_INDEX" => layer_index,
        "LAYOUT" => layout,
        "PLOTSETTINGS" => plotsettings,
        "LWPOLYLINE" => lwpolyline,
        "OLE2FRAME" | "OLEFRAME" => ole2frame,
        "ACAD_PROXY_ENTITY" | "ACAD_PROXY_OBJECT" => proxy,
        "RASTERVARIABLES" => rastervariables,
        "SCALE" => scale,
        "SORTENTSTABLE" => sortentstable,
        "SPATIAL_FILTER" => spatial_filter,
        "LIGHT" => light,
        "ACDBPLACEHOLDER" => placeholder,
        "WIPEOUTVARIABLES" => wipeoutvariables,
        "XRECORD" => xrecord,
        _ => return None,
    })
}

// ---- small helpers

/// `rec.<key>.push(v)` on an array field set earlier.
fn push(rec: &mut Record, key: &str, v: V) {
    if let Some(V::Arr(a)) = rec.f.get_mut(key) {
        a.push(v);
    }
}
/// `rec.plot.<k> = v` (the `plot` object is set before its fields are read).
fn pset(rec: &mut Record, k: &'static str, v: impl Into<V>) {
    if let Some(o) = rec.f.obj_mut("plot") {
        o.set(k, v);
    }
}
/// JS `Array.prototype.slice(start, end)` on numbers that may be negative or past the end.
fn js_slice(a: &[V], start: f64, end: f64) -> Vec<V> {
    let len = crate::util::usize_to_f64(a.len());
    let norm = |x: f64| if x < 0.0 { (len + x).max(0.0) } else { x.min(len) };
    let (s, e) = (crate::util::to_usize(norm(start)), crate::util::to_usize(norm(end)));
    if s >= e {
        return Vec::new();
    }
    a.get(s..e).map(<[V]>::to_vec).unwrap_or_default()
}
/// Move a reader's position by a signed number of bits (JS `r.pos += n`; a negative result stops at 0).
fn seek(r: &mut BitReader, to: i64) {
    r.pos = usize::try_from(to).unwrap_or(0);
}
fn pos_i64(r: &BitReader) -> i64 {
    i64::try_from(r.pos).unwrap_or(i64::MAX)
}

// ---- 20.4.72/73 GROUP MLINESTYLE
fn group(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let name = r.tv()?;
    rec.f.set("name", name);
    let u = r.bs()?;
    rec.f.set("unnamed", u);
    let s = r.bs()?;
    rec.f.set("selectable", s);
    let n = r.bl()?;
    let e = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("entities", e);
    Ok(())
}

fn mlinestyle(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let name = r.tv()?;
    rec.f.set("name", name);
    let d = r.tv()?;
    rec.f.set("description", d);
    let fl = r.bs()?;
    rec.f.set("flags", fl);
    let fc = r.cmc()?;
    rec.f.set("fillColor", fc);
    let sa = r.bd()?;
    rec.f.set("startAngle", sa);
    let ea = r.bd()?;
    rec.f.set("endAngle", ea);
    let n = r.rc()?;
    rec.f.set("elements", Vec::<V>::new());
    for _ in 0..n {
        let offset = r.bd()?;
        let color = r.cmc()?;
        let mut e = crate::obj! { "offset" => offset, "color" => color };
        if ctx.ver < 32 {
            e.set("linetypeIndex", r.bs()?);
        }
        push(rec, "elements", V::Obj(e));
    }
    if ctx.ver >= 32 {
        for i in 0..crate::util::usize_from_u32(n) {
            let v = h.h(rec.handle)?;
            if let Some(V::Arr(a)) = rec.f.get_mut("elements")
                && let Some(V::Obj(e)) = a.get_mut(i)
            {
                e.set("_ltype", v);
            }
        }
    }
    Ok(())
}

// ---- 20.4.74 DICTIONARYVAR
fn dictionaryvar(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let i = r.rc()?;
    rec.f.set("intval", i);
    let v = r.tv()?;
    rec.f.set("value", v);
    Ok(())
}

// ---- 20.4.75 HATCH
fn hatch_edge(r: &mut BitReader, ver: u32) -> Res<Obj> {
    let t = r.rc()?;
    Ok(match t {
        1 => {
            let start = r.rd2()?;
            let end = r.rd2()?;
            crate::obj! { "type" => "line", "start" => start, "end" => end }
        }
        2 => {
            let center = r.rd2()?;
            let radius = r.bd()?;
            let sa = r.bd()?;
            let ea = r.bd()?;
            let ccw = r.bool()?;
            crate::obj! { "type" => "arc", "center" => center, "radius" => radius, "startAngle" => sa, "endAngle" => ea, "ccw" => ccw }
        }
        3 => {
            let center = r.rd2()?;
            let major = r.rd2()?;
            let ratio = r.bd()?;
            let sa = r.bd()?;
            let ea = r.bd()?;
            let ccw = r.bool()?;
            crate::obj! { "type" => "ellipse", "center" => center, "majorAxis" => major, "ratio" => ratio, "startAngle" => sa, "endAngle" => ea, "ccw" => ccw }
        }
        4 => {
            let degree = r.bl()?;
            let rational = r.bool()?;
            let periodic = r.bool()?;
            let mut e = crate::obj! { "type" => "spline", "degree" => degree, "rational" => rational, "periodic" => periodic };
            let nk = r.bl()?;
            let nc = r.bl()?;
            let mut knots = Vec::new();
            for _ in 0..nk {
                knots.push(V::Num(r.bd()?));
            }
            e.set("knots", knots);
            let mut control = Vec::new();
            let mut weights = Vec::new();
            for _ in 0..nc {
                control.push(r.rd2()?);
                if rational {
                    weights.push(V::Num(r.bd()?));
                }
            }
            e.set("control", control);
            if rational {
                e.set("weights", weights);
            }
            if ver >= 24 {
                let nf = r.bl()?;
                if nf > 0 {
                    let mut fit = Vec::new();
                    for _ in 0..nf {
                        fit.push(r.rd2()?);
                    }
                    e.set("fit", fit);
                    let st = r.rd2()?;
                    e.set("startTangent", st);
                    let et = r.rd2()?;
                    e.set("endTangent", et);
                }
            }
            e
        }
        _ => return err(msg("ccdwg.err.hatch_edge", &[("type", &t.to_string())])),
    })
}

fn hatch(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    if ver >= 18 {
        let is_grad = r.bl()?;
        r.bl()?;
        let angle = r.bd()?;
        let shift = r.bd()?;
        let single = r.bl()?;
        let tint = r.bd()?;
        let nc = r.bl()?;
        let mut colors = Vec::new();
        for _ in 0..nc {
            let value = r.bd()?;
            r.bs()?;
            let rgb = crate::util::u32_from_i32(r.bl()?);
            r.rc()?;
            colors.push(V::Obj(crate::obj! { "value" => value, "rgb" => rgb & 0xFF_FFFF }));
        }
        let name = r.tv()?;
        if is_grad != 0 {
            rec.f.set(
                "gradient",
                crate::obj! { "name" => name, "angle" => angle, "shift" => shift, "singleColor" => single, "tint" => tint, "colors" => colors },
            );
        }
    }
    let el = r.bd()?;
    rec.f.set("elevation", el);
    let ex = r.bd3()?;
    rec.f.set("extrusion", ex);
    let pat = r.tv()?;
    rec.f.set("pattern", pat);
    let solid = r.bool()?;
    rec.f.set("solid", solid);
    let assoc = r.bool()?;
    rec.f.set("associative", assoc);
    let np = r.bl()?;
    rec.f.set("loops", Vec::<V>::new());
    let mut total_bound = 0.0f64;
    let mut any_pixel = false;
    for _ in 0..np {
        let flags = r.bl()?;
        let mut lp = crate::obj! { "flags" => flags };
        if flags & 4 != 0 {
            any_pixel = true;
        }
        if flags & 2 == 0 {
            let ns = r.bl()?;
            let mut edges = Vec::new();
            for _ in 0..ns {
                edges.push(V::Obj(hatch_edge(r, ver)?));
            }
            lp.set("edges", edges);
        } else {
            let bulges = r.b()?;
            let closed = r.b()?;
            let n = r.bl()?;
            let mut vs = Vec::new();
            for _ in 0..n {
                let [x, y] = r.rd2_raw()?;
                let bulge = if bulges != 0 { r.bd()? } else { 0.0 };
                vs.push(V::Obj(crate::obj! { "x" => x, "y" => y, "bulge" => bulge }));
            }
            lp.set("polyline", crate::obj! { "closed" => closed != 0, "hasBulges" => bulges != 0, "vertices" => vs });
        }
        let nb = r.bl()?;
        lp.set("_nbound", nb);
        total_bound += f64::from(nb);
        push(rec, "loops", V::Obj(lp));
    }
    let st = r.bs()?;
    rec.f.set("style", st);
    let pt = r.bs()?;
    rec.f.set("patternType", pt);
    if !solid {
        let a = r.bd()?;
        rec.f.set("angle", a);
        let s = r.bd()?;
        rec.f.set("scale", s);
        let d = r.bool()?;
        rec.f.set("double", d);
        let nd = r.bs()?;
        rec.f.set("patternLines", Vec::<V>::new());
        for _ in 0..nd {
            let angle = r.bd()?;
            let base = r.bd2()?;
            let offset = r.bd2()?;
            let n = r.bs()?;
            let mut dashes = Vec::new();
            for _ in 0..n {
                dashes.push(V::Num(r.bd()?));
            }
            push(rec, "patternLines", V::Obj(crate::obj! { "angle" => angle, "base" => base, "offset" => offset, "dashes" => dashes }));
        }
    }
    if any_pixel {
        let p = r.bd()?;
        rec.f.set("pixelSize", p);
    }
    let ns = r.bl()?;
    rec.f.set("seeds", Vec::<V>::new());
    for _ in 0..ns {
        let p = r.rd2()?;
        push(rec, "seeds", p);
    }
    let boundary = hrefs(h, rec.handle, total_bound)?;
    let mut bi = 0.0f64;
    if let Some(V::Arr(loops)) = rec.f.get_mut("loops") {
        for l in loops.iter_mut() {
            if let V::Obj(o) = l {
                let nb = o.num("_nbound").unwrap_or(0.0);
                o.set("_boundary", js_slice(&boundary, bi, bi + nb));
                bi += nb;
            }
        }
    }
    rec.h.set("boundary", boundary);
    Ok(())
}

// ---- 20.4.79–84
fn idbuffer(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    r.rc()?;
    let n = r.bl()?;
    let ids = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("ids", ids);
    Ok(())
}

fn image_data(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let cv = r.bl()?;
    rec.f.set("classVersion", cv);
    for k in ["insert", "u", "v"] {
        let p = r.bd3()?;
        rec.f.set(k, p);
    }
    let s = r.rd2()?;
    rec.f.set("size", s);
    let df = r.bs()?;
    rec.f.set("displayFlags", df);
    let c = r.bool()?;
    rec.f.set("clipping", c);
    for k in ["brightness", "contrast", "fade"] {
        let v = r.rc()?;
        rec.f.set(k, v);
    }
    if ctx.ver >= 24 {
        let m = r.b()?;
        rec.f.set("clipMode", m);
    }
    let ct = r.bs()?;
    rec.f.set("clipType", ct);
    if ct == 1 {
        let a = r.rd2()?;
        let b = r.rd2()?;
        rec.f.set("clip", vec![a, b]);
    } else {
        let n = r.bl()?;
        rec.f.set("clip", Vec::<V>::new());
        for _ in 0..n {
            let p = r.rd2()?;
            push(rec, "clip", p);
        }
    }
    let d = h.h(rec.handle)?;
    rec.h.set("def", d);
    let dr = h.h(rec.handle)?;
    rec.h.set("defReactor", dr);
    Ok(())
}

fn imagedef(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let cv = r.bl()?;
    rec.f.set("classVersion", cv);
    let s = r.rd2()?;
    rec.f.set("size", s);
    let f = r.tv()?;
    rec.f.set("fileName", f);
    let l = r.bool()?;
    rec.f.set("loaded", l);
    let ru = r.rc()?;
    rec.f.set("resUnits", ru);
    let ps = r.rd2()?;
    rec.f.set("pixelSize", ps);
    Ok(())
}

fn imagedef_reactor(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let cv = r.bl()?;
    rec.f.set("classVersion", cv);
    Ok(())
}

fn layer_index(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    r.bl()?;
    r.bl()?;
    let n = r.bl()?;
    rec.f.set("entries", Vec::<V>::new());
    for _ in 0..n {
        let index = r.bl()?;
        let name = r.tv()?;
        push(rec, "entries", V::Obj(crate::obj! { "index" => index, "name" => name }));
    }
    let e = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("entries", e);
    Ok(())
}

/// The plot settings shared by LAYOUT and PLOTSETTINGS (`rec.plot`).
fn plot_fields(r: &mut BitReader, rec: &mut Record, ver: u32) -> Res<()> {
    rec.f.set("plot", Obj::new());
    let v = r.tv()?;
    pset(rec, "pageSetupName", v);
    let v = r.tv()?;
    pset(rec, "printer", v);
    let v = r.bs()?;
    pset(rec, "flags", v);
    let left = r.bd()?;
    let bottom = r.bd()?;
    let right = r.bd()?;
    let top = r.bd()?;
    pset(rec, "margins", crate::obj! { "left" => left, "bottom" => bottom, "right" => right, "top" => top });
    let v = r.bd()?;
    pset(rec, "paperWidth", v);
    let v = r.bd()?;
    pset(rec, "paperHeight", v);
    let v = r.tv()?;
    pset(rec, "paperSize", v);
    let v = r.bd2()?;
    pset(rec, "origin", v);
    for k in ["paperUnits", "rotation", "plotType"] {
        let v = r.bs()?;
        pset(rec, k, v);
    }
    let v = r.bd2()?;
    pset(rec, "windowMin", v);
    let v = r.bd2()?;
    pset(rec, "windowMax", v);
    if ver <= 15 {
        let v = r.t()?;
        pset(rec, "viewName", v);
    }
    let v = r.bd()?;
    pset(rec, "realWorldUnits", v);
    let v = r.bd()?;
    pset(rec, "drawingUnits", v);
    let v = r.tv()?;
    pset(rec, "styleSheet", v);
    let v = r.bs()?;
    pset(rec, "scaleType", v);
    let v = r.bd()?;
    pset(rec, "scaleFactor", v);
    let v = r.bd2()?;
    pset(rec, "imageOrigin", v);
    if ver >= 18 {
        for k in ["shadePlotMode", "shadePlotRes", "shadePlotDpi"] {
            let v = r.bs()?;
            pset(rec, k, v);
        }
    }
    Ok(())
}

fn layout(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    plot_fields(r, rec, ver)?;
    let name = r.tv()?;
    rec.f.set("name", name);
    let t = r.bl()?;
    rec.f.set("tabOrder", t);
    let f = r.bs()?;
    rec.f.set("flags", f);
    let v = r.bd3()?;
    rec.f.set("ucsOrigin", v);
    let v = r.rd2()?;
    rec.f.set("limMin", v);
    let v = r.rd2()?;
    rec.f.set("limMax", v);
    for k in ["insBase", "ucsXAxis", "ucsYAxis"] {
        let v = r.bd3()?;
        rec.f.set(k, v);
    }
    let v = r.bd()?;
    rec.f.set("elevation", v);
    let v = r.bs()?;
    rec.f.set("orthoType", v);
    let v = r.bd3()?;
    rec.f.set("extMin", v);
    let v = r.bd3()?;
    rec.f.set("extMax", v);
    let mut nvp = 0;
    if ver >= 18 {
        // Spec gap: 20.4.84 types it RL; AutoCAD files carry a BL (found by experiment)
        nvp = r.bl()?;
    }
    let hd = rec.handle;
    if ver >= 18 {
        let v = h.h(hd)?;
        rec.h.set("plotView", v);
    }
    if ver >= 21 {
        let v = h.h(hd)?;
        rec.h.set("visualStyle", v);
    }
    for k in ["block", "lastViewport", "baseUcs", "namedUcs"] {
        let v = h.h(hd)?;
        rec.h.set(k, v);
    }
    if ver >= 18 {
        let v = hrefs(h, hd, f64::from(nvp))?;
        rec.h.set("viewports", v);
    }
    Ok(())
}

fn plotsettings(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    plot_fields(r, rec, ctx.ver)?;
    if ctx.ver >= 18 {
        let v = h.h(rec.handle)?;
        rec.h.set("plotView", v);
    }
    Ok(())
}

// ---- 20.4.85 LWPOLYLINE
fn vertex(x: f64, y: f64) -> V {
    V::Obj(crate::obj! { "x" => x, "y" => y, "bulge" => 0.0, "startWidth" => 0.0, "endWidth" => 0.0 })
}

fn lwpolyline(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let f = r.bs()?;
    rec.f.set("flags", f);
    let cw = if f & 4 != 0 { r.bd()? } else { 0.0 };
    rec.f.set("constantWidth", cw);
    let el = if f & 8 != 0 { r.bd()? } else { 0.0 };
    rec.f.set("elevation", el);
    let th = if f & 2 != 0 { r.bd()? } else { 0.0 };
    rec.f.set("thickness", th);
    let ex = if f & 1 != 0 { r.bd3()? } else { p3(0.0, 0.0, 1.0) };
    rec.f.set("extrusion", ex);
    let c = f64::from(r.bl()?);
    let n = crate::util::to_usize(fits(r, c, 2.0)?);
    let nb = if f & 16 != 0 {
        let c = f64::from(r.bl()?);
        crate::util::to_usize(fits(r, c, 2.0)?)
    } else {
        0
    };
    let nid = if ver >= 24 && f & 1024 != 0 {
        let c = f64::from(r.bl()?);
        crate::util::to_usize(fits(r, c, 2.0)?)
    } else {
        0
    };
    let nw = if f & 32 != 0 {
        let c = f64::from(r.bl()?);
        crate::util::to_usize(fits(r, c, 4.0)?)
    } else {
        0
    };
    rec.f.set("closed", f & 512 != 0);
    rec.f.set("plinegen", f & 256 != 0);
    // JS `new Array(n)` filled in place: a failure part way leaves holes (null in JSON)
    rec.f.set("vertices", Vec::<V>::new());
    let mut vs = vec![V::Null; n];
    let res = lw_vertices(r, ver, &mut vs, nb, nid, nw);
    rec.f.set("vertices", vs);
    res
}

fn lw_vertices(r: &mut BitReader, ver: u32, vs: &mut [V], nb: usize, nid: usize, nw: usize) -> Res<()> {
    let n = vs.len();
    if n > 0 {
        if ver < 15 {
            for v in vs.iter_mut() {
                let [x, y] = r.rd2_raw()?;
                *v = vertex(x, y);
            }
        } else {
            let mut x = r.rd()?;
            let mut y = r.rd()?;
            if let Some(v) = vs.first_mut() {
                *v = vertex(x, y);
            }
            for v in vs.iter_mut().skip(1) {
                x = r.dd(x)?;
                y = r.dd(y)?;
                *v = vertex(x, y);
            }
        }
    }
    for i in 0..nb {
        let b = r.bd()?;
        if let Some(V::Obj(o)) = vs.get_mut(i) {
            o.set("bulge", b);
        }
    }
    for i in 0..nid {
        let id = r.bl()?;
        if let Some(V::Obj(o)) = vs.get_mut(i) {
            o.set("id", id);
        }
    }
    for i in 0..nw {
        let s = r.bd()?;
        let e = r.bd()?;
        if let Some(V::Obj(o)) = vs.get_mut(i) {
            o.set("startWidth", s);
            o.set("endWidth", e);
        }
    }
    Ok(())
}

// ---- 20.4.88 OLE2FRAME
fn ole2frame(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let f = r.bs()?;
    rec.f.set("flags", f);
    if ctx.ver >= 15 {
        let m = r.bs()?;
        rec.f.set("mode", m);
    }
    let len = r.bl()?;
    rec.f.set("dataLength", len);
    seek(r, pos_i64(r).saturating_add(i64::from(len).saturating_mul(8)));
    if ctx.ver >= 15 {
        r.rc()?;
    }
    Ok(())
}

// ---- 20.4.90 PROXY
fn proxy(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    if ctx.ver >= 15 {
        let c = r.bl()?;
        rec.f.set("classId", c);
        if ctx.ver >= 32 {
            let v = r.bl()?;
            rec.f.set("proxyVersion", v);
            let m = r.bl()?;
            rec.f.set("proxyMaint", m);
        } else {
            let d = r.bl()?;
            rec.f.set("drawingFormat", d);
        }
        let o = r.b()?;
        rec.f.set("originalFormat", o);
    }
    Ok(())
}

// ---- 20.4.91–95
fn rastervariables(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let cv = r.bl()?;
    rec.f.set("classVersion", cv);
    for k in ["displayFrame", "displayQuality", "units"] {
        let v = r.bs()?;
        rec.f.set(k, v);
    }
    Ok(())
}

fn scale(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    r.bs()?;
    let n = r.tv()?;
    rec.f.set("name", n);
    let p = r.bd()?;
    rec.f.set("paperUnits", p);
    let d = r.bd()?;
    rec.f.set("drawingUnits", d);
    let u = r.bool()?;
    rec.f.set("unitScale", u);
    Ok(())
}

fn sortentstable(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let n = r.bl()?;
    rec.f.set("_sort", Vec::<V>::new());
    for _ in 0..n {
        let v = r.h(0.0)?;
        push(rec, "_sort", V::Num(v));
    }
    let ob = h.h(rec.handle)?;
    rec.h.set("ownerBlock", ob);
    let e = hrefs(h, rec.handle, f64::from(n))?;
    rec.h.set("ents", e);
    Ok(())
}

fn spatial_filter(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let n = r.bs()?;
    rec.f.set("points", Vec::<V>::new());
    for _ in 0..n {
        let p = r.rd2()?;
        push(rec, "points", p);
    }
    let v = r.bd3()?;
    rec.f.set("extrusion", v);
    let v = r.bd3()?;
    rec.f.set("origin", v);
    let v = r.bs()?;
    rec.f.set("displayBoundary", v);
    for k in ["frontClip", "backClip"] {
        let v = if r.bs()? != 0 { V::Num(r.bd()?) } else { V::Null };
        rec.f.set(k, v);
    }
    for k in ["inverseBlockTransform", "clipTransform"] {
        rec.f.set(k, Vec::<V>::new());
        for _ in 0..12 {
            let v = r.bd()?;
            push(rec, k, V::Num(v));
        }
    }
    Ok(())
}

// ---- LIGHT. Spec gap: not in the spec; the layout below was found by experiment (bit by bit against the
// ODA File Converter's DXF of the same light): version BL, name TV, type BS, status B, colour CMC, plot glyph B,
// intensity BD, position 3BD, target 3BD, then the attenuation / spot / shadow fields in DXF order.
fn light(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let v = r.bl()?;
    rec.f.set("lightVersion", v);
    let v = r.tv()?;
    rec.f.set("name", v);
    let v = r.bs()?;
    rec.f.set("lightType", v);
    let v = r.bool()?;
    rec.f.set("status", v);
    let v = r.cmc()?;
    rec.f.set("lightColor", v);
    let v = r.bool()?;
    rec.f.set("plotGlyph", v);
    let v = r.bd()?;
    rec.f.set("intensity", v);
    let v = r.bd3()?;
    rec.f.set("position", v);
    let v = r.bd3()?;
    rec.f.set("target", v);
    let v = r.bs()?;
    rec.f.set("attenuationType", v);
    let v = r.bool()?;
    rec.f.set("useAttenuationLimits", v);
    for k in ["attenuationStart", "attenuationEnd", "hotspot", "falloff"] {
        let v = r.bd()?;
        rec.f.set(k, v);
    }
    let v = r.bool()?;
    rec.f.set("castShadows", v);
    let v = r.bs()?;
    rec.f.set("shadowType", v);
    let v = r.bs()?;
    rec.f.set("shadowMapSize", v);
    let v = r.rc()?;
    rec.f.set("shadowMapSoftness", v);
    Ok(())
}

// ---- objects with no or one field
fn placeholder(_r: &mut BitReader, _h: &mut BitReader, _rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    Ok(())
}
/// Not in the spec: one BS, as RASTERVARIABLES' frame.
fn wipeoutvariables(r: &mut BitReader, _h: &mut BitReader, rec: &mut Record, _ctx: &Ctx) -> Res<()> {
    let v = r.bs()?;
    rec.f.set("displayFrame", v);
    Ok(())
}

// ---- 20.4.104 XRECORD
fn xrecord(r: &mut BitReader, h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let n = r.bl()?;
    let end = pos_i64(r).saturating_add(i64::from(n).saturating_mul(8));
    rec.f.set("data", Vec::<V>::new());
    while pos_i64(r) < end {
        let step = r.rs().and_then(|code| Ok((code, xrecord_value(r, code, ctx)?)));
        match step {
            Ok((code, v)) => push(rec, "data", V::Arr(vec![V::from(code), v])),
            Err(e) => {
                rec.f.set("_xrecError", e.0);
                break;
            }
        }
    }
    seek(r, end);
    if ctx.ver >= 15 {
        let c = r.bs()?;
        rec.f.set("cloning", c);
    }
    let o = rest_handles(h, rec.handle, 1_000_000);
    rec.h.set("objids", o);
    Ok(())
}

fn xrecord_value(r: &mut BitReader, code: i32, ctx: &Ctx) -> Res<V> {
    Ok(match group_type(code) {
        Some(GroupType::Str) => {
            if ctx.ver >= 21 {
                let n = r.rsu()?;
                let mut units: Vec<u16> = Vec::new();
                for _ in 0..n {
                    units.push(u16::try_from(r.rsu()?).unwrap_or(0));
                }
                // String.fromCharCode per unit (a lone surrogate becomes U+FFFD here)
                V::Str(String::from_utf16_lossy(&units))
            } else {
                let n = r.rsu()?;
                r.rc()?;
                let b = r.bytes(crate::util::usize_from_u32(n))?;
                V::Str(ctx.cp.decode(&b))
            }
        }
        Some(GroupType::Pt3) => r.rd3()?,
        Some(GroupType::Pt2) => r.rd2()?,
        Some(GroupType::Dbl) => V::Num(r.rd()?),
        Some(GroupType::I8 | GroupType::Bool) => V::from(r.rc()?),
        Some(GroupType::I16) => V::from(r.rs()?),
        Some(GroupType::I32) => V::from(r.rls()?),
        Some(GroupType::I64) => V::Num(r.rll()?),
        Some(GroupType::Hnd) => {
            let b = r.bytes(8)?;
            let mut v = 0.0f64;
            for x in b.iter().rev() {
                v = v * 256.0 + f64::from(*x);
            }
            V::Str(hex_upper(v))
        }
        Some(GroupType::Bin) => {
            let n = r.rc()?;
            let b = r.bytes(crate::util::usize_from_u32(n))?;
            V::Str(hex_bytes(&b))
        }
        None => return err(msg("ccdwg.err.xrecord_code", &[("code", &code.to_string())])),
    })
}

/// JS `v.toString(16).toUpperCase()` for the value of 8 bytes (at most 2^64 once rounded to a double).
fn hex_upper(v: f64) -> String {
    if v >= 18_446_744_073_709_551_616.0 {
        return "10000000000000000".into();
    }
    hex_of(v)
}

/// DXF group code → value kind (public DXF reference ranges).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupType {
    Str,
    Pt3,
    Pt2,
    Dbl,
    I8,
    I16,
    I32,
    I64,
    Hnd,
    Bin,
    Bool,
}

pub fn group_type(c: i32) -> Option<GroupType> {
    use GroupType::*;
    Some(match c {
        0..=9 => Str,
        10..=39 => Pt3,
        40..=59 => Dbl,
        60..=79 => I16,
        90..=99 => I32,
        100..=102 => Str,
        105 => Hnd,
        110..=139 => Pt3,
        140..=149 => Dbl,
        160..=169 => I64,
        170..=179 => I16,
        210..=239 => Pt3,
        270..=279 => I16,
        280..=289 => I8,
        290..=299 => Bool,
        300..=309 => Str,
        310..=319 => Bin,
        320..=369 => Hnd,
        370..=389 => I16,
        390..=399 => Hnd,
        400..=409 => I16,
        410..=419 => Str,
        420..=429 => I32,
        430..=439 => Str,
        440..=459 => I32,
        460..=469 => Dbl,
        470..=479 => Str,
        480..=481 => Hnd,
        999 => Str,
        1000..=1009 => Str,
        1010..=1039 => Pt3,
        1040..=1059 => Dbl,
        1060..=1070 => I16,
        1071 => I32,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_like_js() {
        let a: Vec<V> = (0..5).map(V::from).collect();
        assert_eq!(js_slice(&a, 1.0, 3.0), vec![V::from(1), V::from(2)]);
        assert_eq!(js_slice(&a, 3.0, 1.0), Vec::<V>::new());
        assert_eq!(js_slice(&a, 2.0, -1.0), vec![V::from(2), V::from(3)]);
        assert_eq!(js_slice(&a, 4.0, 9.0), vec![V::from(4)]);
    }

    #[test]
    fn group_types() {
        assert_eq!(group_type(1), Some(GroupType::Str));
        assert_eq!(group_type(105), Some(GroupType::Hnd));
        assert_eq!(group_type(145), Some(GroupType::Dbl));
        assert_eq!(group_type(1071), Some(GroupType::I32));
        assert_eq!(group_type(80), None);
    }

    #[test]
    fn hex_of_handles() {
        assert_eq!(hex_upper(255.0), "FF");
        assert_eq!(hex_upper(18_446_744_073_709_551_616.0), "10000000000000000");
    }
}
