//! Wavefront OBJ (+ its .mtl): io.js `exportOBJ`, `exportMTL`, `importOBJ`.
//!
//! OBJ is written Y up, the way its readers expect: (x, y, z) → (x, z, −y); flattened to world coordinates, one `o`
//! per placed definition; edges without faces as `l`. With `mtl` it names that materials file and switches material
//! per colour (`usemtl`).

use cc_m3d::Model;
use cc_m3d::def::{edge_faces, face_tris, key};
use cc_m3d::edges::smooth_normals;
use cc_m3d::js;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::omap::{FxMap, kb};
use cc_m3d::paint::is_material_paint;
use cc_m3d::vec::{IDENT, P3, add, apply, apply_dir, cross, det3, mul, norm};
use serde_json::{Map, Value};

use crate::Res;
use crate::i18n::{err, err_with, t};
use crate::num::{fixed_num, is_space, parse_float, parse_int};
use crate::util::{MAX_VERTS, clean_name, or_paint, paint_hex, skip, slice16, xp};

/// `f6(x)`: six decimals, as a number's text (−0 → 0).
pub fn f6(x: f64, out: &mut String) {
    js::push_num(out, fixed_num(x, 6));
}

fn zup_to_yup(p: P3) -> P3 {
    [p[0], p[2], -p[1]]
}
fn yup_to_zup(p: P3) -> P3 {
    [p[0], -p[2], p[1]]
}

/// `s.replace(/\s+/g, '_')`.
pub fn ws_to_underscore(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if is_space(c) {
            if !in_ws {
                out.push('_');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `matName(c)`: the material name a colour gets in OBJ / MTL.
pub fn mat_name(c: Option<&str>) -> String {
    match c.filter(|x| !x.is_empty()) {
        None => "Default".into(),
        Some(c) if is_material_paint(c) => format!("Material_{}", c.get(4..).unwrap_or("")),
        Some(c) => {
            let mut it = c.chars();
            it.next();
            format!("Colour_{}", it.as_str().to_lowercase())
        }
    }
}

struct FaceOut {
    ids: Vec<f64>,
    ns: Vec<f64>,
    mat: Option<String>,
}

/// `exportOBJ(model, {mtl})` → the text.
pub fn export_obj(model: &Model, mtl: Option<&str>) -> String {
    let mut out = String::from("# ModelCraft OBJ export — meters, Y up (converted from the model's Z up)\n");
    if let Some(m) = mtl {
        out.push_str("mtllib ");
        out.push_str(&ws_to_underscore(m));
        out.push('\n');
    }
    let mut v_base = 1.0f64;
    let mut n_base = 1.0f64;
    let mut cur_mat: Option<String> = None;
    let mut key_buf = String::new();
    model.walk("root", &IDENT, &mut |d, wm, path, pc| {
        let ef = edge_faces(d);
        let loose: Vec<(f64, f64)> = d.edges.values().filter(|e| !ef.has(&key(e.a, e.b))).map(|e| (e.a, e.b)).collect();
        if d.faces.is_empty() && loose.is_empty() {
            return true;
        }
        let mirror = det3(wm) < 0.0;
        let raw = if path.is_empty() {
            if d.name.is_empty() { "Model".to_string() } else { d.name.clone() }
        } else {
            let mut s = d.name.clone();
            s.push(' ');
            for (i, id) in path.iter().enumerate() {
                if i > 0 {
                    s.push('.');
                }
                js::push_num(&mut s, *id);
            }
            s
        };
        out.push_str("o ");
        out.push_str(&ws_to_underscore(&raw));
        out.push('\n');
        let mut vi: FxMap<u64, f64> = FxMap::default();
        let mut vs: Vec<P3> = Vec::new();
        let mut faces: Vec<FaceOut> = Vec::new();
        let mut norms: Vec<P3> = Vec::new();
        let mut n_index: FxMap<String, f64> = FxMap::default();
        let smooth = smooth_normals(d);
        let vid = |id: f64, vi: &mut FxMap<u64, f64>, vs: &mut Vec<P3>| -> f64 {
            if let Some(&i) = vi.get(&kb(id)) {
                return i;
            }
            let i = v_base + vs.len() as f64;
            vi.insert(kb(id), i);
            vs.push(zup_to_yup(apply(wm, d.v(id))));
            i
        };
        let nid = |n: P3, norms: &mut Vec<P3>, n_index: &mut FxMap<String, f64>, kb_: &mut String| -> f64 {
            kb_.clear();
            for (k, x) in n.iter().enumerate() {
                if k > 0 {
                    kb_.push(' ');
                }
                f6(*x, kb_);
            }
            if let Some(&i) = n_index.get(kb_.as_str()) {
                return i;
            }
            let i = n_base + norms.len() as f64;
            n_index.insert(kb_.clone(), i);
            norms.push(n);
            i
        };
        for (fid, f) in d.faces.iter() {
            let l0 = f.loops.first().map_or(&[][..], Vec::as_slice);
            let pts: Vec<P3> = l0.iter().map(|&v| apply(wm, d.v(v))).collect();
            let mut nn = [0.0; 3];
            let np = pts.len();
            for (i, p) in pts.iter().enumerate() {
                let q = pts.get(i.saturating_add(1).checked_rem(np).unwrap_or(0)).copied().unwrap_or(*p);
                nn = add(nn, cross(*p, q));
            }
            let mut flat_i: Option<f64> = None;
            let pieces: Vec<Vec<f64>> =
                if f.loops.len() == 1 { vec![l0.to_vec()] } else { face_tris(d, f).into_iter().map(|t| t.to_vec()).collect() };
            let mat = mtl.map(|_| mat_name(or_paint(f.color.as_deref(), pc).map(xp).as_deref()));
            let mut fkey = String::new();
            if !smooth.is_empty() {
                js::push_num(&mut fkey, fid);
                fkey.push('|');
            }
            for mut lp in pieces {
                if mirror {
                    lp.reverse();
                }
                let ids: Vec<f64> = lp.iter().map(|&v| vid(v, &mut vi, &mut vs)).collect();
                let mut ns = Vec::with_capacity(lp.len());
                for &v in &lp {
                    let q = if smooth.is_empty() {
                        None
                    } else {
                        let mut k = fkey.clone();
                        js::push_num(&mut k, v);
                        smooth.get(&k).copied()
                    };
                    let i = match q {
                        Some(q) => nid(zup_to_yup(norm(apply_dir(wm, q))), &mut norms, &mut n_index, &mut key_buf),
                        None => match flat_i {
                            Some(i) => i,
                            None => {
                                let i =
                                    nid(zup_to_yup(norm(if mirror { mul(nn, -1.0) } else { nn })), &mut norms, &mut n_index, &mut key_buf);
                                flat_i = Some(i);
                                i
                            }
                        },
                    };
                    ns.push(i);
                }
                faces.push(FaceOut { ids, ns, mat: mat.clone() });
            }
        }
        for &(a, b) in &loose {
            vid(a, &mut vi, &mut vs);
            vid(b, &mut vi, &mut vs);
        }
        for p in &vs {
            out.push('v');
            for x in p {
                out.push(' ');
                f6(*x, &mut out);
            }
            out.push('\n');
        }
        for n in &norms {
            out.push_str("vn");
            for x in n {
                out.push(' ');
                f6(*x, &mut out);
            }
            out.push('\n');
        }
        for f in &faces {
            if let Some(m) = &f.mat
                && !m.is_empty()
                && cur_mat.as_deref() != Some(m.as_str())
            {
                out.push_str("usemtl ");
                out.push_str(m);
                out.push('\n');
                cur_mat = Some(m.clone());
            }
            out.push('f');
            for (k, i) in f.ids.iter().enumerate() {
                out.push(' ');
                js::push_num(&mut out, *i);
                out.push_str("//");
                js::push_num(&mut out, f.ns.get(k).copied().unwrap_or(f64::NAN));
            }
            out.push('\n');
        }
        for &(a, b) in &loose {
            out.push_str("l ");
            js::push_num(&mut out, vi.get(&kb(a)).copied().unwrap_or(f64::NAN));
            out.push(' ');
            js::push_num(&mut out, vi.get(&kb(b)).copied().unwrap_or(f64::NAN));
            out.push('\n');
        }
        v_base += vs.len() as f64;
        n_base += norms.len() as f64;
        true
    });
    out
}

/// `parseInt(s, 16)`: whitespace, sign, an optional 0x, hex digits; NaN when there are none.
pub fn parse_hex(s: &str) -> f64 {
    let t = s.trim_start_matches(is_space);
    let (neg, mut rest) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    if rest.starts_with("0x") || rest.starts_with("0X") {
        rest = rest.get(2..).unwrap_or("");
    }
    let mut v = 0.0f64;
    let mut any = false;
    for c in rest.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = v * 16.0 + f64::from(d);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// `s.slice(a, b)` in UTF-16 units.
pub fn slice_units(s: &str, a: usize, b: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let b = b.min(u.len());
    String::from_utf16_lossy(u.get(a.min(b)..b).unwrap_or(&[]))
}

/// `exportMTL(model)`: one material per colour used (Kd in sRGB, as .mtl readers take it).
pub fn export_mtl(model: &Model) -> String {
    let mut used: Vec<String> = Vec::new();
    model.walk("root", &IDENT, &mut |d, _wm, _path, pc| {
        for f in d.faces.values() {
            let c = or_paint(f.color.as_deref(), pc).map(xp).unwrap_or_default();
            if !used.contains(&c) {
                used.push(c);
            }
        }
        true
    });
    used.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let mut out = String::from("# ModelCraft materials");
    for c in &used {
        let rgb: Vec<String> = if c.is_empty() {
            vec!["0.800000".into(), "0.800000".into(), "0.800000".into()]
        } else {
            let h = paint_hex(c);
            [1usize, 3, 5].iter().map(|&o| crate::num::fixed(parse_hex(&slice_units(&h, o, o.saturating_add(2))) / 255.0, 6)).collect()
        };
        out.push_str("\n\nnewmtl ");
        out.push_str(&mat_name(Some(c.as_str())));
        out.push_str("\nKd ");
        out.push_str(&rgb.join(" "));
        out.push_str("\nKa 0 0 0\nKs 0 0 0\nd 1\nillum 1");
    }
    out.push('\n');
    out
}

/// What an importer returns: the new definition (not placed yet) and its report.
#[derive(Clone, Debug)]
pub struct Imported {
    pub def: String,
    pub report: Map<String, Value>,
    /// DXF / DWG only: the unit the file says (`None` = it says nothing; the caller's unit was used).
    pub units: Option<String>,
}

/// `s.split(/\s+/)` of an already trimmed, non-empty string.
fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|x| !x.is_empty()).collect()
}

/// `importOBJ(model, text, name)` → {def, report}.
pub fn import_obj(model: &mut Model, text: &str, name: &str) -> Res<Imported> {
    let mut verts: Vec<P3> = Vec::new();
    let mut polys: Vec<PolyIn> = Vec::new();
    let mut skipped = Map::new();
    let mut pos = 0usize;
    let l = text.len();
    let find_nl = |from: usize| text.get(from..).and_then(|r| r.find('\n')).map_or(l, |i| i.saturating_add(from));
    while pos < l {
        let end = find_nl(pos);
        let mut line: String = text.get(pos..end).unwrap_or("").to_string();
        pos = end.saturating_add(1);
        while line.ends_with('\\') && pos < l {
            let e2 = find_nl(pos);
            line.pop();
            line.push(' ');
            line.push_str(text.get(pos..e2).unwrap_or(""));
            pos = e2.saturating_add(1);
        }
        let s = js::trim(&line);
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let parts = split_ws(s);
        let tag = parts.first().copied().unwrap_or("");
        if tag == "v" {
            let pf = |k: usize| parts.get(k).map_or(f64::NAN, |x| parse_float(x));
            let p = [pf(1), pf(2), pf(3)];
            if !p.iter().all(|x| x.is_finite() && x.abs() < 1e7) {
                return Err(err_with("m3d.io.obj_vertex", &[("line", &slice16(s, 40))]));
            }
            verts.push(yup_to_zup(p));
            if verts.len() > MAX_VERTS {
                return Err(err("m3d.io.this_obj_file_has_more_than"));
            }
        } else if tag == "f" {
            let mut poly: Vec<P3> = Vec::new();
            let mut bad = false;
            for tok in parts.iter().skip(1) {
                let mut i = parse_int(tok.split('/').next().unwrap_or(""));
                if !js::is_integer(i) || i == 0.0 {
                    bad = true;
                    break;
                }
                if i < 0.0 {
                    i += verts.len() as f64 + 1.0;
                }
                if i < 1.0 || i > verts.len() as f64 {
                    bad = true;
                    break;
                }
                match crate::num::index(i - 1.0).and_then(|k| verts.get(k)) {
                    Some(p) => poly.push(*p),
                    None => {
                        bad = true;
                        break;
                    }
                }
            }
            if bad || poly.len() < 3 {
                skip(&mut skipped, &t("m3d.io.bad_face", &[]), 1.0);
                continue;
            }
            polys.push(PolyIn::outline(poly));
            if polys.len() > MAX_VERTS {
                return Err(err("m3d.io.this_obj_file_has_more_than_2"));
            }
        } else if matches!(tag, "l" | "p" | "curv" | "surf") {
            let k = match tag {
                "l" => "m3d.io.lines",
                "p" => "m3d.io.points",
                _ => "m3d.io.curves_and_surfaces",
            };
            skip(&mut skipped, &t(k, &[]), 1.0);
        }
    }
    let id = mesh_def_in(model, &polys, Some(&clean_name(name)), "group", &[]);
    let nf = model.def(&id).map_or(0, |d| d.faces.len());
    if polys.len() > nf {
        skipped.insert(t("m3d.io.degenerate_face", &[]), Value::from(polys.len().saturating_sub(nf) as f64));
    }
    let mut report = Map::new();
    report.insert("faces".into(), Value::from(nf as f64));
    report.insert("vertices".into(), Value::from(verts.len() as f64));
    report.insert("skipped".into(), Value::Object(skipped));
    Ok(Imported { def: id, report, units: None })
}
