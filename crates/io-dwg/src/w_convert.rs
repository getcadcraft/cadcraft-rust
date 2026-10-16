//! write.js convert(e, owner): one model entity → records (the first is the entity; sub-objects follow), a skip
//! reason, or "invalid" (empty / not drawable).

use crate::obj;
use crate::util::usize_to_f64;
use crate::value::{Obj, V, p3};
use crate::w_js::{arr, field, hex_key, int32, js_string, nn, num, ok_p, p3_of, str_or, to_number, truthy};
use crate::w_objects::Rec;
use crate::w_write::{Conv, Kind, W};

/// V8's Math.hypot (Kahan summation over the values scaled by the largest).
pub fn js_hypot(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    let mut max = 0.0f64;
    let mut nan = false;
    let mut abs = Vec::with_capacity(vals.len());
    for &v in vals {
        if v.is_nan() {
            nan = true;
            abs.push(0.0);
        } else {
            let a = v.abs();
            abs.push(a);
            if a > max {
                max = a;
            }
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for a in abs {
        let n = a / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `x > 0` with JS coercion.
fn gt0(v: Option<&V>) -> bool {
    to_number(v) > 0.0
}
fn fin(v: Option<&V>) -> bool {
    matches!(v, Some(V::Num(n)) if n.is_finite())
}
/// write.js P3(p): `p ? {x: num(p.x), …} : origin`
pub fn p3w(p: Option<&V>) -> V {
    if truthy(p) { p3_of(p) } else { p3(0.0, 0.0, 0.0) }
}
/// `e.k | 0 || d`
fn int_or(v: Option<&V>, d: f64) -> f64 {
    let i = int32(v);
    if i == 0 { d } else { f64::from(i) }
}
fn set_v(o: &mut Obj, k: &'static str, v: Option<&V>) {
    o.set_opt(k, v.cloned());
}
/// write.js tagOf
pub fn tag_of(t: Option<&V>) -> String {
    let s = js_string(t).to_uppercase();
    let mut out = String::new();
    let mut in_space = false;
    for c in s.chars() {
        if crate::w_js::is_js_space(c) {
            if !in_space {
                out.push('_');
            }
            in_space = true;
        } else {
            in_space = false;
            if (c as u32) >= 0x20 {
                out.push(c);
            }
        }
    }
    if out.is_empty() { "TAG".into() } else { out }
}
fn hs(v: &[f64]) -> V {
    V::Arr(v.iter().map(|x| V::Num(*x)).collect())
}

impl W<'_> {
    /// textFields(e)
    fn text_fields(&self, e: &Obj, o: &mut Obj) {
        o.set("insert", p3w(e.get("insert")));
        o.set("align", if truthy(e.get("align")) { p3w(e.get("align")) } else { V::Null });
        o.set("height", num(e.get("height"), 0.2));
        let val = nn(e.get("value")).or_else(|| nn(e.get("text")));
        o.set("value", str_or(val, ""));
        o.set("rotation", num(e.get("rotation"), 0.0));
        o.set("widthFactor", num(e.get("widthFactor"), 1.0));
        o.set("oblique", num(e.get("oblique"), 0.0));
        o.set("style", self.style_ref(e.get("style")));
        o.set("hAlign", int32(e.get("hAlign")));
        o.set("vAlign", int32(e.get("vAlign")));
        o.set("generation", int32(e.get("generation")));
        o.set("thickness", num(e.get("thickness"), 0.0));
        set_v(o, "extrusion", e.get("extrusion"));
    }
    /// blockByName.get(String(e.block ?? '').toUpperCase()) || blocks.find((x) => hexKey(x.src.record) === hexKey(rec))
    pub fn find_block(&self, name: Option<&V>, rec: Option<&V>) -> Option<usize> {
        if let Some(i) = self.block_by_name.get(&str_or(name, "").to_uppercase()) {
            return Some(*i);
        }
        let k = hex_key(rec);
        self.blocks.iter().position(|x| hex_key(x.src.get("record")) == k)
    }
    fn image_def(&mut self, src: &Obj) -> (f64, usize) {
        let k = hex_key(src.get("handle")).unwrap_or_else(|| "\u{0}null".into());
        if let Some(i) = self.written_defs.iter().position(|(kk, _)| *kk == k) {
            return (self.written_defs.get(i).map_or(0.0, |d| d.1.h()), i);
        }
        let fname = if truthy(src.get("fileName")) { js_string(src.get("fileName")) } else { "image".into() };
        let last = fname.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        let stem = match last.rfind('.') {
            Some(p) => last.get(..p).unwrap_or("").to_string(),
            None => last,
        };
        let stem: String = stem
            .chars()
            .map(|c| {
                if matches!(c, '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | ',' | '=' | '`') || (c as u32) < 0x20 {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let base = if stem.is_empty() { "image".to_string() } else { stem };
        let mut n = base.clone();
        let mut i = 2u32;
        while self.def_names.contains(&n.to_uppercase()) {
            n = format!("{base}_{i}");
            i = i.saturating_add(1);
        }
        self.def_names.insert(n.to_uppercase());
        let h = self.next();
        let mut o = obj! { "t" => "IMAGEDEF", "h" => h, "name" => n, "classVersion" => int32(src.get("classVersion")) };
        set_v(&mut o, "size", src.get("size"));
        o.set("fileName", if truthy(src.get("fileName")) { src.get("fileName").cloned().unwrap_or(V::Null) } else { V::from("") });
        o.set("loaded", !matches!(src.get("loaded"), Some(V::Bool(false))));
        o.set("resUnits", int32(src.get("resUnits")));
        set_v(&mut o, "pixelSize", src.get("pixelSize"));
        o.set("reactors", Vec::<V>::new());
        o.set("ownerCode", 3);
        self.written_defs.push((k, Rec::new(o)));
        self.map_src(src.get("handle"), h);
        (h, self.written_defs.len().saturating_sub(1))
    }

    /// One model entity → records.
    pub fn convert(&mut self, e: &Obj, owner: f64) -> Conv {
        let mut ty = (if truthy(e.get("type")) { js_string(e.get("type")) } else { String::new() }).to_uppercase();
        if ty == "DIMENSION" {
            let d = nn(e.get("dimType")).map_or(0, |v| int32(Some(v))) & 7;
            ty = match d {
                0 => "DIMENSION_LINEAR",
                1 => "DIMENSION_ALIGNED",
                2 => "DIMENSION_ANG2LN",
                3 => "DIMENSION_DIAMETER",
                4 => "DIMENSION_RADIUS",
                5 => "DIMENSION_ANG3PT",
                6 => "DIMENSION_ORDINATE",
                _ => "DIMENSION_LINEAR",
            }
            .into();
        }
        if ty == "POLYLINE" {
            let f = int32(e.get("flags"));
            ty = if f & 64 != 0 {
                "POLYLINE_PFACE"
            } else if f & 16 != 0 {
                "POLYLINE_MESH"
            } else if f & 8 != 0 {
                "POLYLINE_3D"
            } else {
                "POLYLINE_2D"
            }
            .into();
        }
        if let Some(x) = self.x_convert(e, &ty, owner) {
            return x;
        }
        if ty == "ACAD_TABLE" {
            return self.tbl_convert(e, owner);
        }
        if let Some(x) = self.surf_convert(e, &ty, owner) {
            return x;
        }
        if let Some(x) = self.sol_convert(e, &ty, owner) {
            return x;
        }
        let one = |r: Rec| Conv::Recs(vec![r]);
        match ty.as_str() {
            "LINE" => {
                if !ok_p(e.get("start")) || !ok_p(e.get("end")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "LINE", owner);
                r.o.set("start", p3w(e.get("start")));
                r.o.set("end", p3w(e.get("end")));
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                one(r)
            }
            "POINT" => {
                if !ok_p(e.get("position")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "POINT", owner);
                r.o.set("position", p3w(e.get("position")));
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                r.o.set("xAxisAngle", num(e.get("xAxisAngle"), 0.0));
                one(r)
            }
            "CIRCLE" => {
                if !ok_p(e.get("center")) || !gt0(e.get("radius")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "CIRCLE", owner);
                r.o.set("center", p3w(e.get("center")));
                set_v(&mut r.o, "radius", e.get("radius"));
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                one(r)
            }
            "ARC" => {
                if !ok_p(e.get("center")) || !gt0(e.get("radius")) || !fin(e.get("startAngle")) || !fin(e.get("endAngle")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "ARC", owner);
                r.o.set("center", p3w(e.get("center")));
                set_v(&mut r.o, "radius", e.get("radius"));
                set_v(&mut r.o, "startAngle", e.get("startAngle"));
                set_v(&mut r.o, "endAngle", e.get("endAngle"));
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                one(r)
            }
            "ELLIPSE" => {
                if !ok_p(e.get("center")) || !ok_p(e.get("majorAxis")) || !gt0(e.get("ratio")) {
                    return Conv::Invalid;
                }
                let mut maj = e.get("majorAxis").cloned().unwrap_or(V::Null);
                let mut ratio = e.get("ratio").cloned().unwrap_or(V::Null);
                let mut s = e.get("startParam").cloned().unwrap_or(V::Num(0.0));
                let mut en = e.get("endParam").cloned().unwrap_or(V::Num(2.0 * std::f64::consts::PI));
                let rn = to_number(Some(&ratio));
                if rn > 1.0 {
                    let mx = to_number(field(Some(&maj), "x"));
                    let my = to_number(field(Some(&maj), "y"));
                    let mz = num(field(Some(&maj), "z"), 0.0);
                    maj = p3(-my * rn, mx * rn, mz * rn);
                    ratio = V::Num(1.0 / rn);
                    s = V::Num(to_number(Some(&s)) - std::f64::consts::PI / 2.0);
                    en = V::Num(to_number(Some(&en)) - std::f64::consts::PI / 2.0);
                }
                let mut r = self.common(e, "ELLIPSE", owner);
                r.o.set("center", p3w(e.get("center")));
                r.o.set("majorAxis", p3w(Some(&maj)));
                r.o.set("ratio", ratio);
                r.o.set("startParam", s);
                r.o.set("endParam", en);
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                one(r)
            }
            "RAY" | "XLINE" => {
                if !ok_p(e.get("start")) || !ok_p(e.get("direction")) {
                    return Conv::Invalid;
                }
                let d = p3w(e.get("direction"));
                let (dx, dy, dz) = (num(field(Some(&d), "x"), 0.0), num(field(Some(&d), "y"), 0.0), num(field(Some(&d), "z"), 0.0));
                let n = js_hypot(&[dx, dy, dz]);
                if n.is_nan() || n <= 0.0 {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, &ty, owner);
                r.o.set("start", p3w(e.get("start")));
                r.o.set("direction", p3(dx / n, dy / n, dz / n));
                one(r)
            }
            "SOLID" | "TRACE" | "3DFACE" => {
                let mut pts: Vec<V> = arr(e.get("points")).iter().filter(|p| ok_p(Some(p))).map(|p| p3w(Some(p))).collect();
                if pts.len() < 3 {
                    return Conv::Invalid;
                }
                while pts.len() < 4 {
                    let l = pts.last().cloned().unwrap_or(V::Null);
                    pts.push(l);
                }
                let z0 = num(pts.first().and_then(|p| field(Some(p), "z")), 0.0);
                let mut r = self.common(e, &ty, owner);
                r.o.set("points", pts);
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                r.o.set("invisibleEdges", int32(e.get("invisibleEdges")));
                r.o.set("elevation", num(e.get("elevation"), z0));
                one(r)
            }
            "LWPOLYLINE" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                if v.len() < 2 {
                    return Conv::Invalid;
                }
                let closed = if e.get("closed").is_some() { truthy(e.get("closed")) } else { int32(e.get("flags")) & 512 != 0 };
                let mut r = self.common(e, "LWPOLYLINE", owner);
                r.o.set("vertices", v);
                r.o.set("closed", closed);
                r.o.set("constantWidth", num(e.get("constantWidth"), 0.0));
                r.o.set("elevation", num(e.get("elevation"), 0.0));
                r.o.set("thickness", num(e.get("thickness"), 0.0));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                r.o.set("plinegen", truthy(e.get("plinegen")));
                one(r)
            }
            "POLYLINE_2D" | "POLYLINE_3D" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                if v.len() < 2 {
                    return Conv::Invalid;
                }
                let is3d = ty == "POLYLINE_3D";
                let mut head = self.common(e, &ty, owner);
                let closed = if e.get("closed").is_some() { truthy(e.get("closed")) } else { int32(e.get("flags")) & 1 != 0 };
                head.o.set("closed", closed);
                let vt = if is3d { "VERTEX_3D" } else { "VERTEX_2D" };
                let hh = head.h();
                let mut verts = Vec::with_capacity(v.len());
                for p in &v {
                    let p = Some(p);
                    let mut x = self.common(e, vt, hh);
                    x.o.set("mode", 0);
                    x.o.set("point", p3w(p));
                    x.o.set("flags", if is3d { 32 } else { int32(field(p, "flags")) });
                    x.o.set("bulge", num(field(p, "bulge"), 0.0));
                    x.o.set("startWidth", num(field(p, "startWidth"), 0.0));
                    x.o.set("endWidth", num(field(p, "endWidth"), 0.0));
                    x.o.set("id", int32(field(p, "id")));
                    x.o.set("tangent", num(field(p, "tangent"), 0.0));
                    verts.push(x);
                }
                let mut seq = self.common(e, "SEQEND", hh);
                seq.o.set("mode", 0);
                let f = int32(e.get("flags"));
                if is3d {
                    head.o.set("splineFlags", if f & 4 != 0 { 1 } else { 0 });
                } else {
                    head.o.set("flags", f | if closed { 1 } else { 0 });
                    head.o.set("curveType", int32(e.get("curveType")));
                    head.o.set("startWidth", num(e.get("startWidth"), 0.0));
                    head.o.set("endWidth", num(e.get("endWidth"), 0.0));
                    head.o.set("thickness", num(e.get("thickness"), 0.0));
                    head.o.set("elevation", num(e.get("elevation"), 0.0));
                    set_v(&mut head.o, "extrusion", e.get("extrusion"));
                }
                let vh: Vec<f64> = verts.iter().map(Rec::h).collect();
                head.o.set("vertices", hs(&vh));
                head.o.set("seqend", seq.h());
                let mut out = vec![head];
                out.extend(verts);
                out.push(seq);
                Conv::Recs(out)
            }
            "POLYLINE_PFACE" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                let faces = arr(e.get("faces")).to_vec();
                if v.len() < 3 || faces.is_empty() {
                    return Conv::Invalid;
                }
                let mut head = self.common(e, &ty, owner);
                head.o.set("numVerts", v.len());
                head.o.set("numFaces", faces.len());
                let hh = head.h();
                let mut verts = Vec::new();
                for p in &v {
                    let mut x = self.common(e, "VERTEX_PFACE", hh);
                    x.o.set("mode", 0);
                    x.o.set("point", p3w(Some(p)));
                    x.o.set("flags", 192);
                    verts.push(x);
                }
                let mut fr = Vec::new();
                for f in &faces {
                    let mut x = self.common(e, "VERTEX_PFACE_FACE", hh);
                    x.o.set("mode", 0);
                    let src = if truthy(field(Some(f), "indices")) { arr(field(Some(f), "indices")) } else { arr(Some(f)) };
                    let mut idx: Vec<V> = src.iter().take(4).cloned().collect();
                    while idx.len() < 4 {
                        idx.push(V::Num(0.0));
                    }
                    x.o.set("indices", idx);
                    fr.push(x);
                }
                let mut seq = self.common(e, "SEQEND", hh);
                seq.o.set("mode", 0);
                let vh: Vec<f64> = verts.iter().chain(fr.iter()).map(Rec::h).collect();
                head.o.set("vertices", hs(&vh));
                head.o.set("seqend", seq.h());
                let mut out = vec![head];
                out.extend(verts);
                out.extend(fr);
                out.push(seq);
                Conv::Recs(out)
            }
            "POLYLINE_MESH" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                let m = int32(e.get("m"));
                let n = int32(e.get("n"));
                if m == 0 || n == 0 || usize_to_f64(v.len()) != f64::from(m) * f64::from(n) {
                    return Conv::Invalid;
                }
                let mut head = self.common(e, &ty, owner);
                head.o.set("flags", int32(e.get("flags")) | 16);
                head.o.set("curveType", int32(e.get("curveType")));
                head.o.set("m", m);
                head.o.set("n", n);
                head.o.set("mDensity", int32(e.get("mDensity")));
                head.o.set("nDensity", int32(e.get("nDensity")));
                let hh = head.h();
                let mut verts = Vec::new();
                for p in &v {
                    let mut x = self.common(e, "VERTEX_MESH", hh);
                    x.o.set("mode", 0);
                    x.o.set("point", p3w(Some(p)));
                    x.o.set("flags", 64);
                    verts.push(x);
                }
                let mut seq = self.common(e, "SEQEND", hh);
                seq.o.set("mode", 0);
                let vh: Vec<f64> = verts.iter().map(Rec::h).collect();
                head.o.set("vertices", hs(&vh));
                head.o.set("seqend", seq.h());
                let mut out = vec![head];
                out.extend(verts);
                out.push(seq);
                Conv::Recs(out)
            }
            "TEXT" => {
                if !ok_p(e.get("insert")) || !gt0(e.get("height")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "TEXT", owner);
                self.text_fields(e, &mut r.o);
                one(r)
            }
            "ATTDEF" => {
                if !ok_p(e.get("insert")) || !gt0(e.get("height")) || !truthy(e.get("tag")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "ATTDEF", owner);
                self.text_fields(e, &mut r.o);
                r.o.set("tag", tag_of(e.get("tag")));
                r.o.set("prompt", str_or(e.get("prompt"), ""));
                r.o.set("flags", int32(e.get("flags")));
                r.o.set("lockPosition", truthy(e.get("lockPosition")));
                one(r)
            }
            "MTEXT" => {
                if !ok_p(e.get("insert")) {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "MTEXT", owner);
                let o = &mut r.o;
                o.set("insert", p3w(e.get("insert")));
                o.set("xAxis", if truthy(e.get("xAxis")) { p3w(e.get("xAxis")) } else { V::Null });
                o.set("rotation", num(e.get("rotation"), 0.0));
                o.set("width", num(e.get("width"), 0.0));
                o.set("height", num(e.get("height"), 0.2));
                o.set("attach", int_or(e.get("attach"), 1.0));
                o.set("direction", int_or(e.get("direction"), 1.0));
                let val = nn(e.get("value")).or_else(|| nn(e.get("text")));
                o.set("value", str_or(val, ""));
                let st = self.style_ref(e.get("style"));
                let o = &mut r.o;
                o.set("style", st);
                o.set("lineSpacing", num(e.get("lineSpacing"), 1.0));
                o.set("lineSpacingStyle", int_or(e.get("lineSpacingStyle"), 1.0));
                o.set("extWidth", num(e.get("extWidth"), 0.0));
                o.set("extHeight", num(e.get("extHeight"), 0.0));
                o.set("rectHeight", num(e.get("rectHeight"), 0.0));
                set_v(o, "extrusion", e.get("extrusion"));
                o.set("backgroundFlags", int32(e.get("backgroundFlags")));
                o.set("backgroundScale", num(e.get("backgroundScale"), 1.5));
                set_v(o, "backgroundColor", e.get("backgroundColor"));
                o.set("backgroundTransparency", int32(e.get("backgroundTransparency")));
                one(r)
            }
            "INSERT" | "MINSERT" => {
                let b = self.find_block(e.get("block"), e.get("block"));
                let Some(bi) = b.filter(|&i| self.blocks.get(i).is_some_and(|x| x.kind == Kind::Block)) else {
                    return Conv::Skip("INSERT of a block the drawing does not define".into());
                };
                if !ok_p(e.get("insert")) {
                    return Conv::Invalid;
                }
                let mut head = self.common(e, &ty, owner);
                head.o.set("insert", p3w(e.get("insert")));
                let sc = e.get("scale");
                head.o.set("scale", p3(num(field(sc, "x"), 1.0), num(field(sc, "y"), 1.0), num(field(sc, "z"), 1.0)));
                head.o.set("rotation", num(e.get("rotation"), 0.0));
                set_v(&mut head.o, "extrusion", e.get("extrusion"));
                head.block_ref = Some(Some(bi));
                head.o.set("cols", int_or(e.get("cols"), 1.0));
                head.o.set("rows", int_or(e.get("rows"), 1.0));
                head.o.set("colSpacing", num(e.get("colSpacing"), 0.0));
                head.o.set("rowSpacing", num(e.get("rowSpacing"), 0.0));
                let hh = head.h();
                let mut atts = Vec::new();
                for a in arr(e.get("attribs")) {
                    let Some(ao) = a.as_obj() else { continue };
                    if !(ok_p(ao.get("insert")) && gt0(ao.get("height")) && truthy(ao.get("tag"))) {
                        continue;
                    }
                    // `{ ...e, ...a, layer: a.layer ?? e.layer, color: a.color ?? e.color }`: only the fields common() reads
                    let mut merged = Obj::with_capacity(8);
                    for k in ["linetype", "transparency", "ltscale", "lineweight", "invisible", "xdata"] {
                        merged.set_opt(k, ao.get(k).or_else(|| e.get(k)).cloned());
                    }
                    merged.set_opt("layer", nn(ao.get("layer")).or_else(|| e.get("layer")).cloned());
                    merged.set_opt("color", nn(ao.get("color")).or_else(|| e.get("color")).cloned());
                    let mut x = self.common(&merged, "ATTRIB", hh);
                    x.o.set("mode", 0);
                    self.text_fields(ao, &mut x.o);
                    x.o.set("tag", tag_of(ao.get("tag")));
                    x.o.set("flags", int32(ao.get("flags")));
                    x.o.set("lockPosition", truthy(ao.get("lockPosition")));
                    atts.push(x);
                }
                let seq = if atts.is_empty() {
                    None
                } else {
                    let mut s = self.common(e, "SEQEND", hh);
                    s.o.set("mode", 0);
                    Some(s)
                };
                let ah: Vec<f64> = atts.iter().map(Rec::h).collect();
                head.o.set("attribs", hs(&ah));
                head.o.set("seqend", seq.as_ref().map_or(0.0, Rec::h));
                if let Some(b) = self.blocks.get_mut(bi) {
                    b.inserts.push(hh);
                }
                if !atts.is_empty() {
                    let n = num(self.counts.get("ATTRIB"), 0.0) + usize_to_f64(atts.len());
                    self.counts.set("ATTRIB", n);
                }
                let mut out = vec![head];
                out.extend(atts);
                out.extend(seq);
                Conv::Recs(out)
            }
            "DIMENSION_LINEAR"
            | "DIMENSION_ALIGNED"
            | "DIMENSION_ANG2LN"
            | "DIMENSION_DIAMETER"
            | "DIMENSION_RADIUS"
            | "DIMENSION_ANG3PT"
            | "DIMENSION_ORDINATE"
            | "ARC_DIMENSION"
            | "LARGE_RADIAL_DIMENSION" => {
                let b = self.find_block(e.get("block"), e.get("block"));
                let ok = b.is_some_and(|i| self.blocks.get(i).is_some_and(|x| x.kind == Kind::Block));
                if !ok && truthy(e.get("block")) {
                    return Conv::Skip("DIMENSION without its block".into());
                }
                let f70 = nn(e.get("flags")).or_else(|| nn(e.get("dimType"))).map_or(0, |v| int32(Some(v)));
                let chord = nn(e.get("chordPoint"))
                    .or_else(|| if ty == "DIMENSION_RADIUS" || ty == "DIMENSION_DIAMETER" { e.get("center") } else { e.get("xline1") });
                let chord = chord.cloned();
                let dimstyle = self.dim_ref(e.get("style"));
                let mut r = self.common(e, &ty, owner);
                r.block_ref = Some(b);
                let o = &mut r.o;
                o.set("dimstyle", dimstyle);
                o.set("defPoint", p3w(e.get("defPoint")));
                o.set("textMid", p3w(e.get("textMid")));
                o.set("insert", if truthy(e.get("insert")) { p3w(e.get("insert")) } else { V::Null });
                o.set("xline1", p3w(e.get("xline1")));
                o.set("xline2", p3w(e.get("xline2")));
                o.set("center", p3w(nn(e.get("center")).or_else(|| e.get("xline1End"))));
                o.set("arcPoint", p3w(e.get("arcPoint")));
                o.set("chordPoint", p3w(chord.as_ref()));
                o.set("angle", num(e.get("angle"), 0.0));
                o.set("obliqueAngle", num(e.get("obliqueAngle"), 0.0));
                o.set("leaderLen", num(e.get("leaderLen"), 0.0));
                o.set("measurement", num(e.get("measurement"), 0.0));
                o.set("text", str_or(e.get("text"), ""));
                o.set("attach", int_or(e.get("attach"), 5.0));
                o.set("textRotation", num(nn(e.get("textRotation")).or_else(|| e.get("rotation")), 0.0));
                o.set("horizDir", num(e.get("horizDir"), 0.0));
                set_v(o, "extrusion", e.get("extrusion"));
                set_v(o, "insScale", e.get("insScale"));
                o.set("insRotation", num(e.get("insRotation"), 0.0));
                o.set("lineSpacing", num(e.get("lineSpacing"), 1.0));
                o.set("lineSpacingStyle", int_or(e.get("lineSpacingStyle"), 1.0));
                o.set("flipArrow1", truthy(e.get("flipArrow1")));
                o.set("flipArrow2", truthy(e.get("flipArrow2")));
                o.set("ordinateX", truthy(e.get("ordinateX")) || f70 & 64 != 0);
                o.set("flags1", (if f70 & 128 != 0 { 0 } else { 1 }) | (if f70 & 32 != 0 { 2 } else { 0 }));
                o.set("isPartial", truthy(e.get("isPartial")));
                o.set("arcStart", num(e.get("arcStart"), 0.0));
                o.set("arcEnd", num(e.get("arcEnd"), 0.0));
                o.set("hasLeader", truthy(e.get("hasLeader")));
                o.set("leader1", p3w(e.get("leader1")));
                o.set("leader2", p3w(e.get("leader2")));
                o.set("overrideCenter", p3w(e.get("overrideCenter")));
                o.set("jogPoint", p3w(e.get("jogPoint")));
                one(r)
            }
            "LEADER" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                if v.len() < 2 {
                    return Conv::Invalid;
                }
                let dimstyle = self.dim_ref(e.get("style"));
                let mut r = self.common(e, "LEADER", owner);
                let o = &mut r.o;
                o.set("vertices", v.iter().map(|p| p3w(Some(p))).collect::<Vec<V>>());
                o.set("annotType", nn(e.get("annotType")).cloned().unwrap_or(V::Num(3.0)));
                o.set("pathType", int32(e.get("pathType")));
                o.set("origin", if truthy(e.get("origin")) { p3w(e.get("origin")) } else { p3w(v.first()) });
                set_v(o, "extrusion", e.get("extrusion"));
                set_v(o, "xDirection", e.get("xDirection"));
                set_v(o, "blockOffset", e.get("blockOffset"));
                set_v(o, "endPointProj", e.get("endPointProj"));
                o.set("boxHeight", num(e.get("boxHeight"), 0.0));
                o.set("boxWidth", num(e.get("boxWidth"), 0.0));
                o.set("hookLineOnX", truthy(e.get("hookLineOnX")));
                o.set("arrowhead", !matches!(e.get("arrowhead"), Some(V::Bool(false))));
                set_v(o, "annotationSrc", e.get("annotation"));
                o.set("dimstyle", dimstyle);
                one(r)
            }
            "IMAGE" | "WIPEOUT" => {
                if !ok_p(e.get("insert")) || !ok_p(e.get("u")) || !ok_p(e.get("v")) {
                    return Conv::Invalid;
                }
                let mut head = self.common(e, &ty, owner);
                let o = &mut head.o;
                o.set("classVersion", int32(e.get("classVersion")));
                o.set("insert", p3w(e.get("insert")));
                o.set("u", p3w(e.get("u")));
                o.set("v", p3w(e.get("v")));
                set_v(o, "size", e.get("size"));
                o.set("displayFlags", nn(e.get("displayFlags")).cloned().unwrap_or(V::Num(7.0)));
                o.set("clipping", truthy(e.get("clipping")));
                o.set("brightness", nn(e.get("brightness")).cloned().unwrap_or(V::Num(50.0)));
                o.set("contrast", nn(e.get("contrast")).cloned().unwrap_or(V::Num(50.0)));
                o.set("fade", int32(e.get("fade")));
                o.set("clipMode", truthy(e.get("clipMode")));
                set_v(o, "clipType", e.get("clipType"));
                o.set("clip", if truthy(e.get("clip")) { e.get("clip").cloned().unwrap_or(V::Null) } else { V::Arr(Vec::new()) });
                let mut reactor = None;
                let mut def_h = 0.0;
                if ty == "IMAGE" {
                    let Some(src) = hex_key(e.get("def")).and_then(|k| self.image_defs.get(&k).cloned()) else {
                        return Conv::Skip("IMAGE whose image definition is not in the model".into());
                    };
                    let (dh, di) = self.image_def(&src);
                    def_h = dh;
                    let rh = self.next();
                    reactor = Some(Rec::new(obj! { "t" => "IMAGEDEF_REACTOR", "h" => rh, "owner" => head.h(), "classVersion" => 2 }));
                    if let Some((_, d)) = self.written_defs.get_mut(di) {
                        let mut rs = arr(d.g("reactors")).to_vec();
                        rs.push(V::Num(rh));
                        d.o.set("reactors", rs);
                    }
                } else {
                    self.need_wipeout_vars = true;
                }
                head.o.set("def", def_h);
                head.o.set("defReactor", reactor.as_ref().map_or(0.0, Rec::h));
                let mut out = vec![head];
                out.extend(reactor);
                Conv::Recs(out)
            }
            "TOLERANCE" => {
                if !ok_p(e.get("insert")) {
                    return Conv::Invalid;
                }
                let dimstyle = self.dim_ref(e.get("style"));
                let mut r = self.common(e, "TOLERANCE", owner);
                r.o.set("insert", p3w(e.get("insert")));
                set_v(&mut r.o, "xDirection", e.get("xDirection"));
                set_v(&mut r.o, "extrusion", e.get("extrusion"));
                r.o.set("text", str_or(e.get("text"), ""));
                r.o.set("dimstyle", dimstyle);
                one(r)
            }
            "MLINE" => {
                let v: Vec<V> = arr(e.get("vertices")).iter().filter(|x| ok_p(field(Some(x), "point"))).cloned().collect();
                if v.len() < 2 {
                    return Conv::Invalid;
                }
                // `(x.lines?.length ?? 2) !== 2`
                let two = |x: &V| {
                    let l = field(Some(x), "lines").filter(|l| !matches!(l, V::Null)).and_then(|l| crate::w_js::length_of(Some(l)));
                    match l {
                        None | Some(V::Null) => true,
                        Some(V::Num(n)) => n == 2.0,
                        Some(_) => false,
                    }
                };
                if !v.first().is_none_or(two) || v.iter().any(|x| !two(x)) {
                    return Conv::Skip("MLINE (multiline style other than Standard)".into());
                }
                let mut r = self.common(e, "MLINE", owner);
                let o = &mut r.o;
                o.set("scale", num(e.get("scale"), 1.0));
                o.set("justification", int32(e.get("justification")));
                o.set("base", p3w(nn(e.get("base")).or_else(|| v.first().and_then(|x| field(Some(x), "point")))));
                set_v(o, "extrusion", e.get("extrusion"));
                o.set("closed", truthy(e.get("closed")));
                o.set("lines", 2);
                let verts: Vec<V> = v
                    .iter()
                    .map(|x| {
                        let x = Some(x);
                        let mut vo = obj! { "point" => p3w(field(x, "point")), "direction" => p3w(field(x, "direction")), "miter" => p3w(field(x, "miter")) };
                        vo.set_opt("lines", field(x, "lines").cloned());
                        V::Obj(vo)
                    })
                    .collect();
                o.set("vertices", verts);
                o.set("style", self.h.mlstyle);
                one(r)
            }
            "SHAPE" => {
                if !ok_p(e.get("insert")) {
                    return Conv::Invalid;
                }
                let st = {
                    let a = self.style_by_name.get(&str_or(e.get("style"), "").to_uppercase()).copied().unwrap_or(0.0);
                    if a != 0.0 { a } else { self.src(e.get("style")) }
                };
                if st == 0.0 {
                    return Conv::Skip("SHAPE (its shape file is not in the styles table)".into());
                }
                let mut r = self.common(e, "SHAPE", owner);
                let o = &mut r.o;
                o.set("insert", p3w(e.get("insert")));
                o.set("scale", num(e.get("scale"), 1.0));
                o.set("rotation", num(e.get("rotation"), 0.0));
                o.set("widthFactor", num(e.get("widthFactor"), 1.0));
                o.set("oblique", num(e.get("oblique"), 0.0));
                o.set("thickness", num(e.get("thickness"), 0.0));
                o.set("shapeNumber", int32(e.get("shapeNumber")));
                set_v(o, "extrusion", e.get("extrusion"));
                o.set("style", st);
                one(r)
            }
            "HATCH" => {
                let mut loops = Vec::new();
                for l in arr(e.get("loops")) {
                    let Some(lo) = l.as_obj() else { continue };
                    if truthy(lo.get("polyline")) || truthy(lo.get("vertices")) {
                        let pl = if truthy(lo.get("polyline")) {
                            lo.get("polyline").cloned().unwrap_or(V::Null)
                        } else {
                            let mut p = Obj::new();
                            p.set_opt("vertices", lo.get("vertices").cloned());
                            p.set_opt("closed", lo.get("closed").cloned());
                            V::Obj(p)
                        };
                        let v: Vec<V> = arr(field(Some(&pl), "vertices")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                        if v.len() >= 2 {
                            let closed = !matches!(field(Some(&pl), "closed"), Some(V::Bool(false)));
                            loops.push(V::Obj(
                                obj! { "flags" => int32(lo.get("flags")), "polyline" => obj! { "vertices" => v, "closed" => closed } },
                            ));
                        }
                        continue;
                    }
                    let edges: Vec<V> =
                        arr(lo.get("edges")).iter().filter(|x| x.truthy() && truthy(field(Some(x), "type"))).cloned().collect();
                    if !edges.is_empty() {
                        loops.push(V::Obj(obj! { "flags" => int32(lo.get("flags")), "edges" => edges }));
                    }
                }
                if loops.is_empty() {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "HATCH", owner);
                let o = &mut r.o;
                let solid = truthy(e.get("solid"));
                o.set(
                    "pattern",
                    if truthy(e.get("pattern")) {
                        e.get("pattern").cloned().unwrap_or(V::Null)
                    } else {
                        V::from(if solid { "SOLID" } else { "ANSI31" })
                    },
                );
                o.set("solid", solid);
                o.set("style", int32(e.get("style")));
                o.set("patternType", nn(e.get("patternType")).cloned().unwrap_or(V::Num(1.0)));
                o.set("angle", num(e.get("angle"), 0.0));
                o.set("scale", num(e.get("scale"), 1.0));
                o.set("double", truthy(e.get("double")));
                let lines = if truthy(e.get("lines")) {
                    e.get("lines").cloned()
                } else if truthy(e.get("patternLines")) {
                    e.get("patternLines").cloned()
                } else {
                    None
                };
                o.set("lines", lines.unwrap_or(V::Arr(Vec::new())));
                o.set("loops", loops);
                o.set("seeds", arr(e.get("seeds")).iter().filter(|p| ok_p(Some(p))).cloned().collect::<Vec<V>>());
                set_v(o, "gradient", e.get("gradient"));
                o.set("elevation", num(e.get("elevation"), 0.0));
                set_v(o, "extrusion", e.get("extrusion"));
                one(r)
            }
            "SPLINE" => {
                let fit: Vec<V> = arr(e.get("fit")).iter().filter(|p| ok_p(Some(p))).cloned().collect();
                let ctrl: Vec<V> = if truthy(e.get("controlComputed")) && fit.len() >= 2 {
                    Vec::new()
                } else {
                    arr(e.get("control")).iter().filter(|p| ok_p(Some(p))).cloned().collect()
                };
                if ctrl.len() < 2 && fit.len() < 2 {
                    return Conv::Invalid;
                }
                let mut r = self.common(e, "SPLINE", owner);
                let o = &mut r.o;
                o.set("degree", int_or(e.get("degree"), 3.0));
                o.set("flags", int32(e.get("flags")));
                set_v(o, "knotParam", e.get("knotParam"));
                o.set(
                    "knots",
                    if ctrl.is_empty() {
                        V::Arr(Vec::new())
                    } else if truthy(e.get("knots")) {
                        e.get("knots").cloned().unwrap_or(V::Null)
                    } else {
                        V::Arr(Vec::new())
                    },
                );
                set_v(o, "weights", e.get("weights"));
                let weights = arr(e.get("weights"));
                let control: Vec<V> = ctrl
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let mut q = p3_of(Some(p)).as_obj().cloned().unwrap_or_default();
                        let w = nn(field(Some(p), "w")).or_else(|| weights.get(i)).cloned();
                        q.set_opt("w", w);
                        V::Obj(q)
                    })
                    .collect();
                o.set("control", control);
                o.set("fit", fit.iter().map(|p| p3w(Some(p))).collect::<Vec<V>>());
                set_v(o, "startTangent", e.get("startTangent"));
                set_v(o, "endTangent", e.get("endTangent"));
                let tol = e.get("tolerances");
                o.set("fitTolerance", num(nn(field(tol, "fit")).or_else(|| e.get("fitTolerance")), 0.0));
                o.set("knotTolerance", num(nn(field(tol, "knot")).or_else(|| e.get("knotTolerance")), 1e-10));
                o.set("controlTolerance", num(nn(field(tol, "control")).or_else(|| e.get("controlTolerance")), 1e-10));
                one(r)
            }
            "VIEWPORT" => {
                if !ok_p(e.get("center")) {
                    return Conv::Invalid;
                }
                let frozen: Vec<V> = arr(e.get("frozenLayers"))
                    .iter()
                    .map(|x| {
                        let a = self.src(Some(x));
                        if a != 0.0 {
                            a
                        } else {
                            let k = js_string(Some(x));
                            self.layer_by_name.get(&k).and_then(|&i| self.layers.get(i)).map_or(0.0, Rec::h)
                        }
                    })
                    .filter(|h| *h != 0.0)
                    .map(V::Num)
                    .collect();
                let mut r = self.common(e, "VIEWPORT", owner);
                let o = &mut r.o;
                o.set("center", p3w(e.get("center")));
                o.set("width", num(e.get("width"), 1.0));
                o.set("height", num(e.get("height"), 1.0));
                o.set("viewCenter", p3w(e.get("viewCenter")));
                o.set("viewTarget", p3w(e.get("viewTarget")));
                o.set("viewHeight", num(e.get("viewHeight"), num(e.get("height"), 1.0)));
                o.set("twist", num(e.get("twist"), 0.0));
                o.set("status", nn(e.get("status")).cloned().unwrap_or(V::Num(f64::from(0x8060))));
                o.set("id", int32(e.get("id")));
                o.set("frozenLayers", frozen);
                one(r)
            }
            "" => Conv::Skip("(no type)".into()),
            other => Conv::Skip(other.to_string()),
        }
    }
}
