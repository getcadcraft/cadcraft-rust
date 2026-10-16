//! Collada (DAE) 1.4.1: io.js `exportDAE` and `importDAE`.
//!
//! Export: Z up, metres (`<unit meter="1">`). One `<geometry>` per definition that has faces or loose edges: its faces
//! in one `<polylist>` per colour (faces with holes or concave outlines go in as triangles, the rest as polygons),
//! loose edges as `<lines>`. Colours are lambert materials (sRGB diffuse). Groups are nested `<node>`s with their
//! `<matrix>`; a component is written once in `<library_nodes>` and placed with `<instance_node>`.
//!
//! Import: geometry (triangles, polylist, polygons with holes, trifans, tristrips; lines and linestrips as edges),
//! node transforms (matrix, translate, rotate, scale, in order), instance_geometry, instance_node, bind_material →
//! diffuse colour; `<up_axis>` and `<unit meter>` are applied. A node placed by instance_node comes in as a
//! component, so does a geometry used by more than one node; other nodes come in as groups.

use std::collections::HashSet;

use cc_m3d::Model;
use cc_m3d::def::{edge_faces, face_normal, face_tris, key};
use cc_m3d::js;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::model::place;
use cc_m3d::omap::{FxMap, kb};
use cc_m3d::paint::is_material_paint;
use cc_m3d::vec::{IDENT, M4, P3, apply, frame, invert, len, mmul, rotation, to2, translation};
use serde_json::{Map, Value};
use unicode_general_category::{GeneralCategory as G, get_general_category};

use crate::Res;
use crate::i18n::{err, err_with, t};
use crate::num::{is_space, parse_float, parse_int, precision_num};
use crate::obj::{Imported, parse_hex, slice_units};
use crate::util::{MAX_VERTS, clean_name, face_paint, or_paint, paint_hex, skip, xp};
use crate::xml::{Doc, parse_xml};

/// Most nodes and geometry placements an export writes (definitions placed in each other multiply; the JS would run
/// out of memory first).
const MAX_NODES: usize = 4_000_000;

/// `xmlText(s)`: characters XML cannot hold dropped, then & < > " escaped.
pub fn xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// `fx(x)`: 12 significant digits (−0 and |x| < 1e-12 → 0).
pub fn fx(x: f64, out: &mut String) {
    let mut v = precision_num(x, 12);
    if v == 0.0 || v.abs() < 1e-12 {
        v = 0.0;
    }
    js::push_num(out, v);
}
fn fxs(x: f64) -> String {
    let mut s = String::new();
    fx(x, &mut s);
    s
}

fn is_letter(c: char) -> bool {
    matches!(get_general_category(c), G::UppercaseLetter | G::LowercaseLetter | G::TitlecaseLetter | G::ModifierLetter | G::OtherLetter)
}
fn is_number(c: char) -> bool {
    matches!(get_general_category(c), G::DecimalNumber | G::LetterNumber | G::OtherNumber)
}

/// `ncName(s)`: Collada 1.4.1 `name` attributes are NCNames (no spaces or punctuation, no leading digit). Spec gap:
/// \p{L} / \p{N} come from Unicode 16 here (V8 in Node 24: Unicode 17).
pub fn nc_name(s: &str) -> String {
    let mut t = String::with_capacity(s.len());
    let mut run = false;
    for c in s.chars() {
        if is_letter(c) || is_number(c) || c == '.' || c == '_' || c == '-' {
            t.push(c);
            run = false;
        } else {
            if !run {
                t.push('_');
            }
            run = true;
        }
    }
    let ok = t.chars().next().is_some_and(|c| is_letter(c) || c == '_');
    if !ok {
        t.insert(0, '_');
    }
    t
}

fn name_extra(s: &str) -> String {
    format!("<extra><technique profile=\"ModelCraft\"><name>{}</name></technique></extra>", xml_text(s))
}

fn row_major(m: &M4) -> String {
    let mut s = String::new();
    for r in 0..4usize {
        for c in 0..4usize {
            if r > 0 || c > 0 {
                s.push(' ');
            }
            fx(m.get(c.saturating_mul(4).saturating_add(r)).copied().unwrap_or(f64::NAN), &mut s);
        }
    }
    s
}

/// `convexFace(def, f, n)`: is a one-loop face convex (every turn the same way, seen from its front)?
fn convex_face(def: &cc_m3d::Def, f: &cc_m3d::def::Face, n: P3) -> bool {
    let fr = frame(n);
    let ps: Vec<[f64; 2]> = f.loops.first().map_or(&[][..], Vec::as_slice).iter().map(|&v| to2(&fr, def.v(v))).collect();
    let k = ps.len();
    if k <= 3 {
        return true;
    }
    let p0 = ps.first().copied().unwrap_or([0.0; 2]);
    let mut size = 0.0f64;
    for p in &ps {
        size = js::max(size, js::max((p[0] - p0[0]).abs(), (p[1] - p0[1]).abs()));
    }
    let tol = -1e-12 * size * size;
    for i in 0..k {
        let g = |j: usize| ps.get(j.checked_rem(k).unwrap_or(0)).copied().unwrap_or([0.0; 2]);
        let (a, b, c) = (g(i), g(i.saturating_add(1)), g(i.saturating_add(2)));
        if (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) < tol {
            return false;
        }
    }
    true
}

struct GeomInfo {
    id: String,
    mats: Vec<Option<usize>>,
}

struct DaeOut<'m> {
    model: &'m Model,
    color_idx: Vec<String>,
    geoms: Vec<String>,
    geom_info: FxMap<String, Option<GeomInfo>>,
    node_n: f64,
    comps_wanted: Vec<(String, Option<String>, String)>,
    comp_seen: HashSet<String>,
    budget: usize,
}

impl DaeOut<'_> {
    fn cidx(&self, c: &str) -> Option<usize> {
        self.color_idx.iter().position(|x| x == c)
    }
    fn cidx_s(&self, c: &str) -> String {
        self.cidx(c).map_or_else(|| "undefined".to_string(), |i| i.to_string())
    }
    /// `geomFor(d, dflt)`.
    fn geom_for(&mut self, did: &str, dflt: Option<&str>) -> Option<(String, Vec<Option<usize>>)> {
        let key_s = format!("{did}|{}", dflt.unwrap_or(""));
        if let Some(g) = self.geom_info.get(&key_s) {
            return g.as_ref().map(|g| (g.id.clone(), g.mats.clone()));
        }
        self.geom_info.insert(key_s.clone(), None);
        let d = self.model.def(did)?;
        let gid = format!("geom-{}{}", d.id, dflt.map_or_else(String::new, |c| format!("-c{}", self.cidx_s(c))));
        let mut vi: FxMap<u64, usize> = FxMap::default();
        let mut pos: Vec<f64> = Vec::new();
        let mut nrm: Vec<f64> = Vec::new();
        let mut vid = |v: f64, pos: &mut Vec<f64>| -> usize {
            if let Some(&i) = vi.get(&kb(v)) {
                return i;
            }
            let i = pos.len() / 3;
            vi.insert(kb(v), i);
            pos.extend_from_slice(&d.v(v));
            i
        };
        let mut sets: Vec<(String, Vec<usize>, Vec<usize>)> = Vec::new();
        for f in d.faces.values() {
            let n = face_normal(d, f);
            if !js::truthy(len(n)) {
                continue;
            }
            let ni = nrm.len() / 3;
            nrm.extend_from_slice(&n);
            let k = face_paint(f.color.as_deref(), dflt).unwrap_or_default();
            let si = match sets.iter().position(|s| s.0 == k) {
                Some(i) => i,
                None => {
                    sets.push((k, Vec::new(), Vec::new()));
                    sets.len().saturating_sub(1)
                }
            };
            let polys: Vec<Vec<f64>> = if f.loops.len() == 1 && convex_face(d, f, n) {
                vec![f.loops.first().cloned().unwrap_or_default()]
            } else {
                face_tris(d, f).into_iter().map(|t| t.to_vec()).collect()
            };
            for lp in polys {
                let ids: Vec<usize> = lp.iter().map(|&v| vid(v, &mut pos)).collect();
                if let Some(set) = sets.get_mut(si) {
                    set.1.push(lp.len());
                    for i in ids {
                        set.2.push(i);
                        set.2.push(ni);
                    }
                }
            }
        }
        let ef = edge_faces(d);
        let mut lines: Vec<usize> = Vec::new();
        for e in d.edges.values() {
            if !ef.has(&key(e.a, e.b)) {
                let a = vid(e.a, &mut pos);
                let b = vid(e.b, &mut pos);
                lines.push(a);
                lines.push(b);
            }
        }
        if sets.is_empty() && lines.is_empty() {
            return None;
        }
        let src = |sid: &str, arr: &[f64]| -> String {
            let mut s = format!("<source id=\"{sid}\"><float_array id=\"{sid}-array\" count=\"{}\">", arr.len());
            for (i, x) in arr.iter().enumerate() {
                if i > 0 {
                    s.push(' ');
                }
                fx(*x, &mut s);
            }
            s.push_str(&format!(
                "</float_array><technique_common><accessor source=\"#{sid}-array\" count=\"{}\" stride=\"3\"><param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/></accessor></technique_common></source>",
                arr.len() / 3
            ));
            s
        };
        let mut parts = src(&format!("{gid}-pos"), &pos);
        if !nrm.is_empty() {
            parts.push_str(&src(&format!("{gid}-nrm"), &nrm));
        }
        parts.push_str(&format!("<vertices id=\"{gid}-vtx\"><input semantic=\"POSITION\" source=\"#{gid}-pos\"/></vertices>"));
        let mut mats = Vec::new();
        let join = |v: &[usize]| v.iter().map(usize::to_string).collect::<Vec<_>>().join(" ");
        for (k, vcount, p) in &sets {
            let mat = if k.is_empty() { String::new() } else { format!(" material=\"mat-{}\"", self.cidx_s(k)) };
            if !k.is_empty() {
                mats.push(self.cidx(k));
            }
            parts.push_str(&format!(
                "<polylist count=\"{}\"{mat}><input semantic=\"VERTEX\" source=\"#{gid}-vtx\" offset=\"0\"/><input semantic=\"NORMAL\" source=\"#{gid}-nrm\" offset=\"1\"/><vcount>{}</vcount><p>{}</p></polylist>",
                vcount.len(),
                join(vcount),
                join(p)
            ));
        }
        if !lines.is_empty() {
            parts.push_str(&format!(
                "<lines count=\"{}\"><input semantic=\"VERTEX\" source=\"#{gid}-vtx\" offset=\"0\"/><p>{}</p></lines>",
                lines.len() / 2,
                join(&lines)
            ));
        }
        self.geoms.push(format!("<geometry id=\"{gid}\" name=\"{}\"><mesh>{parts}</mesh></geometry>", xml_text(&nc_name(&d.name))));
        self.geom_info.insert(key_s, Some(GeomInfo { id: gid.clone(), mats: mats.clone() }));
        Some((gid, mats))
    }
    fn inst_geom(&mut self, did: &str, dflt: Option<&str>) -> String {
        let Some((id, mats)) = self.geom_for(did, dflt) else { return String::new() };
        let ms = |i: &Option<usize>| i.map_or_else(|| "undefined".to_string(), |i| i.to_string());
        let bind = if mats.is_empty() {
            String::new()
        } else {
            let mut b = String::from("<bind_material><technique_common>");
            for i in &mats {
                b.push_str(&format!("<instance_material symbol=\"mat-{}\" target=\"#mat-{}\"/>", ms(i), ms(i)));
            }
            b.push_str("</technique_common></bind_material>");
            b
        };
        format!("<instance_geometry url=\"#{id}\">{bind}</instance_geometry>")
    }
    /// `content(d, dflt, depth)`.
    fn content(&mut self, did: &str, dflt: Option<&str>, depth: u32) -> String {
        let mut s = self.inst_geom(did, dflt);
        if depth > 32 {
            return s;
        }
        let insts: Vec<cc_m3d::def::Inst> = self.model.def(did).map(|d| d.insts.values().cloned().collect()).unwrap_or_default();
        for inst in &insts {
            if self.budget == 0 {
                break;
            }
            self.budget = self.budget.saturating_sub(1);
            s.push_str(&self.inst_node(inst, dflt, depth.saturating_add(1)));
        }
        s
    }
    fn inst_node(&mut self, inst: &cc_m3d::def::Inst, dflt: Option<&str>, depth: u32) -> String {
        let Some(d) = self.model.def(&inst.def) else { return String::new() };
        let (did, dkind, dname) = (d.id.clone(), d.kind.clone(), d.name.clone());
        let cc = or_paint(inst.color.as_deref(), dflt).map(str::to_string);
        let nm = inst.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| dname.clone());
        self.node_n += 1.0;
        let head = format!(
            "<node id=\"node-{}\" name=\"{}\" type=\"NODE\"><matrix sid=\"transform\">{}</matrix>",
            js::num_str(self.node_n),
            xml_text(&nc_name(&nm)),
            row_major(&inst.m)
        );
        if dkind == "component" {
            let cid = format!("comp-{did}{}", cc.as_deref().map_or_else(String::new, |c| format!("-c{}", self.cidx_s(c))));
            if self.comp_seen.insert(cid.clone()) {
                self.comps_wanted.push((did, cc.clone(), cid.clone()));
            }
            return format!("{head}<instance_node url=\"#{cid}\"/>{}</node>", name_extra(&nm));
        }
        let c = self.content(&did, cc.as_deref(), depth);
        format!("{head}{c}{}</node>", name_extra(&nm))
    }
}

/// `date.toISOString()` → the form Collada writes (`.sss` dropped).
pub fn iso_no_ms(iso: &str) -> String {
    match (iso.rfind('.'), iso.ends_with('Z')) {
        (Some(i), true)
            if iso
                .get(i.saturating_add(1)..iso.len().saturating_sub(1))
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) =>
        {
            format!("{}Z", iso.get(..i).unwrap_or(""))
        }
        _ => iso.to_string(),
    }
}

/// `exportDAE(model, {name, date})` → the text. `date_iso` is `date.toISOString()`.
pub fn export_dae(model: &Model, name: &str, date_iso: &str) -> String {
    let iso = iso_no_ms(date_iso);
    let mut color_idx: Vec<String> = Vec::new();
    model.walk("root", &IDENT, &mut |d, _wm, _p, pc| {
        for f in d.faces.values() {
            if let Some(c) = or_paint(f.color.as_deref(), pc).map(xp).filter(|c| !c.is_empty())
                && !color_idx.contains(&c)
            {
                color_idx.push(c);
            }
        }
        true
    });
    let mut o = DaeOut {
        model,
        color_idx,
        geoms: Vec::new(),
        geom_info: FxMap::default(),
        node_n: 0.0,
        comps_wanted: Vec::new(),
        comp_seen: HashSet::new(),
        budget: MAX_NODES,
    };
    let root_name =
        model.root().map(|r| if r.name.is_empty() { "Model".to_string() } else { r.name.clone() }).unwrap_or_else(|| "Model".into());
    let content = o.content("root", None, 0);
    let scene = format!(
        "<visual_scene id=\"scene\" name=\"{}\"><node id=\"model\" name=\"{}\" type=\"NODE\">{content}</node></visual_scene>",
        xml_text(&nc_name(name)),
        xml_text(&nc_name(&root_name))
    );
    let mut lib_nodes: Vec<String> = Vec::new();
    let mut i = 0usize;
    while let Some((did, cc, cid)) = o.comps_wanted.get(i).cloned() {
        i = i.saturating_add(1);
        let dname = model.def(&did).map(|d| d.name.clone()).unwrap_or_default();
        let c = o.content(&did, cc.as_deref(), 0);
        lib_nodes.push(format!(
            "<node id=\"{cid}\" name=\"{}\" type=\"NODE\">{c}{}</node>",
            xml_text(&nc_name(&dname)),
            name_extra(&dname)
        ));
    }
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>".to_string(),
        "<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\">".to_string(),
        format!(
            "<asset><contributor><authoring_tool>ModelCraft</authoring_tool></contributor><created>{iso}</created><modified>{iso}</modified><unit name=\"meter\" meter=\"1\"/><up_axis>Z_UP</up_axis></asset>"
        ),
    ];
    if !o.color_idx.is_empty() {
        let mut eff = String::from("<library_effects>");
        let mut mats = String::from("<library_materials>");
        for (i, c) in o.color_idx.iter().enumerate() {
            let h = paint_hex(c);
            let rgb: Vec<String> =
                [1usize, 3, 5].iter().map(|&k| fxs(parse_hex(&slice_units(&h, k, k.saturating_add(2))) / 255.0)).collect();
            eff.push_str(&format!(
                "<effect id=\"eff-{i}\"><profile_COMMON><technique sid=\"common\"><lambert><diffuse><color sid=\"diffuse\">{} 1</color></diffuse></lambert></technique></profile_COMMON></effect>",
                rgb.join(" ")
            ));
            let mname = if is_material_paint(c) {
                format!("Material_{}", c.get(4..).unwrap_or(""))
            } else {
                let mut it = c.chars();
                it.next();
                format!("Colour_{}", it.as_str())
            };
            mats.push_str(&format!("<material id=\"mat-{i}\" name=\"{mname}\"><instance_effect url=\"#eff-{i}\"/></material>"));
        }
        eff.push_str("</library_effects>");
        mats.push_str("</library_materials>");
        out.push(eff);
        out.push(mats);
    }
    if !o.geoms.is_empty() {
        out.push(format!("<library_geometries>{}</library_geometries>", o.geoms.join("\n")));
    }
    if !lib_nodes.is_empty() {
        out.push(format!("<library_nodes>{}</library_nodes>", lib_nodes.join("\n")));
    }
    out.push(format!("<library_visual_scenes>{scene}</library_visual_scenes>"));
    out.push("<scene><instance_visual_scene url=\"#scene\"/></scene>".into());
    out.push("</COLLADA>".into());
    out.push(String::new());
    out.join("\n")
}

// ---------------------------------------------------------------------------------------------------------------
// import

/// `decodeURIComponent(s)`, or None where it throws a URIError.
pub fn decode_uri_component(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_string());
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    let hex2 = |i: usize| -> Option<u8> {
        let h = b.get(i.saturating_add(1)..i.saturating_add(3))?;
        let s = std::str::from_utf8(h).ok()?;
        if !s.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        u8::from_str_radix(s, 16).ok()
    };
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        if c != b'%' {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        let first = hex2(i)?;
        i = i.saturating_add(3);
        if first < 0x80 {
            out.push(first);
            continue;
        }
        let n = first.leading_ones() as usize;
        if !(2..=4).contains(&n) {
            return None;
        }
        let mut seq = vec![first];
        for _ in 1..n {
            if b.get(i) != Some(&b'%') {
                return None;
            }
            let x = hex2(i)?;
            if x & 0xC0 != 0x80 {
                return None;
            }
            seq.push(x);
            i = i.saturating_add(3);
        }
        std::str::from_utf8(&seq).ok()?;
        out.extend_from_slice(&seq);
    }
    String::from_utf8(out).ok()
}

struct Geo {
    pts: Vec<P3>,
    polys: Vec<(Vec<Vec<usize>>, String)>,
    segs: Vec<[usize; 2]>,
    name: String,
}

struct Dae<'d> {
    doc: &'d Doc,
    by_id: FxMap<String, usize>,
    skipped: Map<String, Value>,
    budget: i64,
    c: M4,
    ci: M4,
    mat_color: FxMap<usize, Option<String>>,
    geom_cache: FxMap<usize, Option<std::rc::Rc<Geo>>>,
    geometries: f64,
    geom_uses: FxMap<usize, usize>,
    node_refs: FxMap<usize, usize>,
    made: Vec<String>,
    geom_defs: FxMap<usize, FxMap<String, Option<String>>>,
    node_defs: FxMap<usize, Option<(String, M4, Option<String>)>>,
    building: HashSet<usize>,
    visits: u64,
    nodes: f64,
}

fn dfail(m: &str) -> cc_m3d::Error {
    err_with("m3d.io.dae_bad", &[("why", m)])
}

impl<'d> Dae<'d> {
    fn skip(&mut self, k: &str) {
        skip(&mut self.skipped, k, 1.0);
    }
    /// `ref(url, kind)`.
    fn refer(&mut self, url: Option<&str>, kind: Option<&str>) -> Option<usize> {
        let url = url.filter(|u| !u.is_empty())?;
        if !url.starts_with('#') {
            self.skip(&t("m3d.io.references_to_other_files", &[]));
            return None;
        }
        let raw = url.get(1..).unwrap_or("");
        let id = decode_uri_component(raw).unwrap_or_else(|| raw.to_string());
        let e = self.by_id.get(&id).copied();
        match e {
            Some(e) if kind.is_none_or(|k| self.doc.el(e).name == k) => Some(e),
            _ => {
                self.skip(&t("m3d.io.missing", &[("kind", kind.unwrap_or("element"))]));
                None
            }
        }
    }
    /// `nums(s)`.
    fn nums(&mut self, s: Option<&str>) -> Res<Vec<f64>> {
        let st = js::trim(s.unwrap_or(""));
        if st.is_empty() {
            return Ok(Vec::new());
        }
        let a: Vec<&str> = st.split(is_space).filter(|x| !x.is_empty()).collect();
        self.budget = self.budget.saturating_sub(i64::try_from(a.len()).unwrap_or(i64::MAX));
        if self.budget < 0 {
            return Err(err("m3d.io.this_collada_file_is_too_big"));
        }
        Ok(a.iter().map(|x| crate::num::number(x)).collect())
    }
    fn ints(&mut self, s: Option<&str>) -> Res<Vec<f64>> {
        Ok(self.nums(s)?.into_iter().map(|x| if js::is_integer(x) && x >= 0.0 { x } else { -1.0 }).collect())
    }
    fn child(&self, el: Option<usize>, name: &str) -> Option<usize> {
        self.doc.child(el, name)
    }
    fn kids(&self, el: Option<usize>, name: &str) -> Vec<usize> {
        self.doc.kids(el, name)
    }
    fn attr(&self, el: Option<usize>, k: &str) -> Option<&'d str> {
        let doc: &'d Doc = self.doc;
        el.and_then(move |e| doc.el(e).attr(k))
    }
    fn text(&self, el: Option<usize>) -> Option<&'d str> {
        let doc: &'d Doc = self.doc;
        el.map(move |e| doc.el(e).text.as_str())
    }
    fn name(&self, el: usize) -> &'d str {
        let doc: &'d Doc = self.doc;
        doc.el(el).name.as_str()
    }

    /// `colorOfMat(mat)`.
    fn color_of_mat(&mut self, mat: Option<usize>) -> Res<Option<String>> {
        let Some(mat) = mat.filter(|&m| self.name(m) == "material") else { return Ok(None) };
        if let Some(c) = self.mat_color.get(&mat) {
            return Ok(c.clone());
        }
        let mut c = None;
        let ie = self.child(Some(mat), "instance_effect");
        let url = self.attr(ie, "url");
        let eff = if ie.is_some() { self.refer(url, Some("effect")) } else { None };
        let tech = self.child(self.child(eff, "profile_COMMON"), "technique");
        let shader = tech.and_then(|t| {
            self.child(Some(t), "lambert")
                .or_else(|| self.child(Some(t), "phong"))
                .or_else(|| self.child(Some(t), "blinn"))
                .or_else(|| self.child(Some(t), "constant"))
        });
        let slot =
            shader.and_then(|s| if self.name(s) == "constant" { self.child(Some(s), "emission") } else { self.child(Some(s), "diffuse") });
        let col = self.child(slot, "color");
        if col.is_some() {
            let v = self.nums(self.text(col))?;
            if v.len() >= 3 && v.iter().take(3).all(|x| x.is_finite()) {
                let mut s = String::from("#");
                for x in v.iter().take(3) {
                    let h = crate::num::hex(js::round(1.0f64.min(0.0f64.max(*x)) * 255.0));
                    if h.len() < 2 {
                        s.push('0');
                    }
                    s.push_str(&h);
                }
                c = Some(s);
            }
        } else if self.child(slot, "texture").is_some() {
            self.skip(&t("m3d.io.textures_faces_keep_no_colour", &[]));
        }
        self.mat_color.insert(mat, c.clone());
        Ok(c)
    }

    /// `bindingsOf(ig)`: symbol → colour (insertion ordered).
    fn bindings_of(&mut self, ig: usize) -> Res<Vec<(String, Option<String>)>> {
        let mut b: Vec<(String, Option<String>)> = Vec::new();
        let tc = self.child(self.child(Some(ig), "bind_material"), "technique_common");
        for im in self.kids(tc, "instance_material") {
            let Some(sym) = self.attr(Some(im), "symbol").filter(|s| !s.is_empty()) else { continue };
            let tgt = self.attr(Some(im), "target");
            let c = match tgt {
                Some(t) if t.starts_with('#') => {
                    let e = self.by_id.get(t.get(1..).unwrap_or("")).copied();
                    self.color_of_mat(e)?
                }
                _ => None,
            };
            match b.iter_mut().find(|(s, _)| s == sym) {
                Some(slot) => slot.1 = c,
                None => b.push((sym.to_string(), c)),
            }
        }
        Ok(b)
    }

    /// `readGeometry(g)`.
    fn read_geometry(&mut self, g: usize) -> Res<Option<std::rc::Rc<Geo>>> {
        if let Some(c) = self.geom_cache.get(&g) {
            return Ok(c.clone());
        }
        let mut out = None;
        let mesh = self.child(Some(g), "mesh");
        match mesh {
            None => {
                let k = if self.child(Some(g), "convex_mesh").is_some() { "m3d.io.convex_meshes" } else { "m3d.io.curves_and_surfaces" };
                self.skip(&t(k, &[]));
            }
            Some(mesh) => {
                let gname = self
                    .attr(Some(g), "name")
                    .filter(|s| !s.is_empty())
                    .or_else(|| self.attr(Some(g), "id").filter(|s| !s.is_empty()))
                    .unwrap_or("Geometry")
                    .to_string();
                out = self.read_mesh(mesh, gname)?.map(std::rc::Rc::new);
            }
        }
        self.geom_cache.insert(g, out.clone());
        if out.is_some() {
            self.geometries += 1.0;
        }
        Ok(out)
    }

    #[allow(clippy::too_many_lines)]
    fn read_mesh(&mut self, mesh: usize, gname: String) -> Res<Option<Geo>> {
        let verts_el = self.child(Some(mesh), "vertices");
        let pos_in = self.kids(verts_el, "input").into_iter().find(|&i| self.attr(Some(i), "semantic") == Some("POSITION"));
        let Some(pos_in) = pos_in else {
            self.skip(&t("m3d.io.meshes_without_positions", &[]));
            return Ok(None);
        };
        let Some(src) = self.refer(self.attr(Some(pos_in), "source"), Some("source")) else { return Ok(None) };
        let acc = self.child(self.child(Some(src), "technique_common"), "accessor");
        let fa = self.child(Some(src), "float_array");
        let (Some(acc), Some(fa)) = (acc, fa) else { return Err(dfail(&t("m3d.io.a_position_source_has_no_data", &[]))) };
        let data = self.nums(self.text(Some(fa)))?;
        let stride = match self.attr(Some(acc), "stride") {
            None => 1.0,
            Some(s) => parse_int(s),
        };
        let count = parse_int(self.attr(Some(acc), "count").unwrap_or("undefined"));
        let off_s = self.attr(Some(acc), "offset").filter(|s| !s.is_empty()).unwrap_or("0");
        let off = parse_int(off_s);
        let short = !(stride >= 3.0)
            || !(count >= 0.0)
            || !(off >= 0.0)
            || (count != 0.0 && !count.is_nan() && off + stride * (count - 1.0) + 3.0 > data.len() as f64);
        if short {
            return Err(dfail(&t("m3d.io.dae_short_source", &[("name", &clean_name(&gname))])));
        }
        let n = crate::num::index(count).unwrap_or(0);
        let mut pts: Vec<P3> = Vec::with_capacity(n);
        for i in 0..n {
            let base = off + stride * i as f64;
            let g = |k: f64| crate::num::index(base + k).and_then(|j| data.get(j)).copied().unwrap_or(f64::NAN);
            let p = apply(&self.c, [g(0.0), g(1.0), g(2.0)]);
            if !p.iter().all(|x| x.is_finite() && x.abs() < 1e7) {
                return Err(dfail(&t("m3d.io.a_vertex_is_not_a_number", &[])));
            }
            pts.push(p);
        }
        let count_f = count;
        let check = |i: f64| -> Res<usize> {
            if !(i >= 0.0 && i < count_f) {
                return Err(dfail(&t("m3d.io.an_index_points_past_the_vertices", &[])));
            }
            Ok(crate::num::index(i).unwrap_or(0))
        };
        let mut polys: Vec<(Vec<Vec<usize>>, String)> = Vec::new();
        let mut segs: Vec<[usize; 2]> = Vec::new();
        let children = self.doc.el(mesh).children.clone();
        for pr in children {
            let kind = self.name(pr);
            if !["triangles", "polylist", "polygons", "trifans", "tristrips", "lines", "linestrips"].contains(&kind) {
                continue;
            }
            let inputs = self.kids(Some(pr), "input");
            let Some(vin) = inputs.iter().copied().find(|&i| self.attr(Some(i), "semantic") == Some("VERTEX")) else {
                self.skip(&t("m3d.io.without_vertices", &[("kind", kind)]));
                continue;
            };
            let offs = |i: usize| -> f64 {
                let v = parse_int(self.attr(Some(i), "offset").filter(|s| !s.is_empty()).unwrap_or("0"));
                if v.is_nan() || v == 0.0 { 0.0 } else { v }
            };
            let mut n_in = 1.0f64;
            for &i in &inputs {
                n_in = js::max(n_in, offs(i) + 1.0);
            }
            let vo = offs(vin);
            let sym = self.attr(Some(pr), "material").unwrap_or("").to_string();
            let pick = |p: &[f64]| -> Res<Vec<usize>> {
                let mut r = Vec::new();
                let mut k = vo;
                while k < p.len() as f64 {
                    let v = crate::num::index(k).and_then(|j| p.get(j)).copied().unwrap_or(f64::NAN);
                    r.push(check(v)?);
                    k += n_in;
                    if r.len() > MAX_VERTS.saturating_mul(6) {
                        break;
                    }
                }
                Ok(r)
            };
            let mut p_list: Vec<Vec<usize>> = Vec::new();
            for p in self.kids(Some(pr), "p") {
                let v = self.ints(self.text(Some(p)))?;
                p_list.push(pick(&v)?);
            }
            let tri = |a: usize, b: usize, c: usize| vec![vec![a, b, c]];
            match kind {
                "triangles" => {
                    for ix in &p_list {
                        let mut k = 0usize;
                        while k.saturating_add(2) < ix.len() {
                            let g = |j: usize| ix.get(j).copied().unwrap_or(0);
                            polys.push((tri(g(k), g(k.saturating_add(1)), g(k.saturating_add(2))), sym.clone()));
                            k = k.saturating_add(3);
                        }
                    }
                }
                "polylist" => {
                    let vc_el = self.child(Some(pr), "vcount");
                    let vc = self.ints(self.text(vc_el))?;
                    let empty = Vec::new();
                    let ix = p_list.first().unwrap_or(&empty);
                    let mut k = 0usize;
                    for n in vc {
                        if n < 0.0 || k as f64 + n > ix.len() as f64 {
                            return Err(dfail(&t("m3d.io.dae_vcount", &[])));
                        }
                        let nu = crate::num::index(n).unwrap_or(0);
                        if nu >= 3 {
                            polys.push((vec![ix.get(k..k.saturating_add(nu)).unwrap_or(&[]).to_vec()], sym.clone()));
                        }
                        k = k.saturating_add(nu);
                    }
                }
                "polygons" => {
                    for ix in &p_list {
                        if ix.len() >= 3 {
                            polys.push((vec![ix.clone()], sym.clone()));
                        }
                    }
                    for ph in self.kids(Some(pr), "ph") {
                        let pe = self.child(Some(ph), "p");
                        let ov = self.ints(self.text(pe))?;
                        let outer = pick(&ov)?;
                        if outer.len() >= 3 {
                            let mut loops = vec![outer];
                            for h in self.kids(Some(ph), "h") {
                                let hv = self.ints(self.text(Some(h)))?;
                                let hp = pick(&hv)?;
                                if hp.len() >= 3 {
                                    loops.push(hp);
                                }
                            }
                            polys.push((loops, sym.clone()));
                        }
                    }
                }
                "trifans" => {
                    for ix in &p_list {
                        let g = |j: usize| ix.get(j).copied().unwrap_or(0);
                        let mut k = 1usize;
                        while k.saturating_add(1) < ix.len() {
                            polys.push((tri(g(0), g(k), g(k.saturating_add(1))), sym.clone()));
                            k = k.saturating_add(1);
                        }
                    }
                }
                "tristrips" => {
                    for ix in &p_list {
                        let g = |j: usize| ix.get(j).copied().unwrap_or(0);
                        let mut k = 0usize;
                        while k.saturating_add(2) < ix.len() {
                            let (a, b, c) = (g(k), g(k.saturating_add(1)), g(k.saturating_add(2)));
                            polys.push((if k % 2 == 1 { tri(b, a, c) } else { tri(a, b, c) }, sym.clone()));
                            k = k.saturating_add(1);
                        }
                    }
                }
                "lines" => {
                    for ix in &p_list {
                        let mut k = 0usize;
                        while k.saturating_add(1) < ix.len() {
                            segs.push([ix.get(k).copied().unwrap_or(0), ix.get(k.saturating_add(1)).copied().unwrap_or(0)]);
                            k = k.saturating_add(2);
                        }
                    }
                }
                _ => {
                    for ix in &p_list {
                        let mut k = 0usize;
                        while k.saturating_add(1) < ix.len() {
                            segs.push([ix.get(k).copied().unwrap_or(0), ix.get(k.saturating_add(1)).copied().unwrap_or(0)]);
                            k = k.saturating_add(1);
                        }
                    }
                }
            }
            if polys.len() > MAX_VERTS {
                return Err(err("m3d.io.this_collada_file_has_more_than"));
            }
            if segs.len() > MAX_VERTS.saturating_mul(6) {
                return Err(err("m3d.io.this_collada_file_is_too_big"));
            }
        }
        Ok(Some(Geo { pts, polys, segs, name: gname }))
    }

    /// `polysOf(geo, bind)`.
    fn polys_of(&mut self, geo: &Geo, bind: &[(String, Option<String>)]) -> Res<(Vec<PolyIn>, Vec<[P3; 2]>)> {
        let mut out = Vec::with_capacity(geo.polys.len());
        for (loops, sym) in &geo.polys {
            let color = match bind.iter().find(|(s, _)| s == sym) {
                Some((_, c)) => c.clone(),
                None if !sym.is_empty() => {
                    let e = self.by_id.get(sym).copied();
                    self.color_of_mat(e)?
                }
                None => None,
            };
            let pl: Vec<Vec<P3>> =
                loops.iter().map(|l| l.iter().map(|&i| geo.pts.get(i).copied().unwrap_or([f64::NAN; 3])).collect()).collect();
            out.push(PolyIn { loops: pl, color });
        }
        let segs = geo
            .segs
            .iter()
            .map(|[a, b]| [geo.pts.get(*a).copied().unwrap_or([f64::NAN; 3]), geo.pts.get(*b).copied().unwrap_or([f64::NAN; 3])])
            .collect();
        Ok((out, segs))
    }

    /// `geomDef(g, bind)`: a geometry used by several nodes becomes a component (one per binding).
    fn geom_def(&mut self, model: &mut Model, g: usize, bind: &[(String, Option<String>)]) -> Res<Option<String>> {
        let mut ks: Vec<String> = bind.iter().map(|(a, b)| format!("{a}={}", b.as_deref().unwrap_or("null"))).collect();
        ks.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let k = ks.join("|");
        if let Some(id) = self.geom_defs.get(&g).and_then(|m| m.get(&k)) {
            return Ok(id.clone());
        }
        let geo = self.read_geometry(g)?;
        let mut id = None;
        if let Some(geo) = geo {
            let (polys, segs) = self.polys_of(&geo, bind)?;
            let did = mesh_def_in(model, &polys, Some(&clean_name(&geo.name)), "component", &segs);
            let keep = model.def(&did).is_some_and(|d| !d.faces.is_empty() || !d.edges.is_empty());
            if keep {
                self.made.push(did.clone());
                id = Some(did);
            } else {
                model.defs.delete(&did);
            }
        }
        self.geom_defs.entry(g).or_default().insert(k, id.clone());
        Ok(id)
    }

    /// `nodeMatrix(n)`.
    fn node_matrix(&mut self, n: usize) -> Res<M4> {
        let mut m = IDENT;
        let children = self.doc.el(n).children.clone();
        for c in children {
            let nm = self.name(c);
            let v = if matches!(nm, "matrix" | "translate" | "rotate" | "scale") { Some(self.nums(self.text(Some(c)))?) } else { None };
            if let Some(v) = &v
                && !v.iter().all(|x| x.is_finite())
            {
                return Err(dfail(&t("m3d.io.a_node_transform_is_not_a", &[])));
            }
            let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
            match (nm, &v) {
                ("matrix", Some(v)) if v.len() == 16 => {
                    let mut t = [0.0; 16];
                    for col in 0..4usize {
                        for r in 0..4usize {
                            if let Some(s) = t.get_mut(col.saturating_mul(4).saturating_add(r)) {
                                *s = g(v, r.saturating_mul(4).saturating_add(col));
                            }
                        }
                    }
                    m = mmul(&m, &t);
                }
                ("translate", Some(v)) if v.len() == 3 => m = mmul(&m, &translation([g(v, 0), g(v, 1), g(v, 2)])),
                ("rotate", Some(v)) if v.len() == 4 => {
                    if js::hypot(&[g(v, 0), g(v, 1), g(v, 2)]) > 0.0 {
                        m = mmul(&m, &rotation([g(v, 0), g(v, 1), g(v, 2)], g(v, 3) * std::f64::consts::PI / 180.0, [0.0; 3]));
                    }
                }
                ("scale", Some(v)) if v.len() == 3 => {
                    m = mmul(&m, &[g(v, 0), 0.0, 0.0, 0.0, 0.0, g(v, 1), 0.0, 0.0, 0.0, 0.0, g(v, 2), 0.0, 0.0, 0.0, 0.0, 1.0]);
                }
                ("lookat" | "skew", _) => self.skip(&t("m3d.io.transforms", &[("name", nm)])),
                (_, Some(_)) => return Err(dfail(&format!("a <{nm}> has the wrong number of values"))),
                _ => {}
            }
        }
        Ok(m)
    }

    fn conv(&self, m: &M4) -> M4 {
        mmul(&self.c, &mmul(m, &self.ci))
    }

    /// `nodeDef(n, depth)` → {id, m, name}: the definition to place, and a matrix to apply after the node's own.
    fn node_def(&mut self, model: &mut Model, n: usize, depth: u32) -> Res<Option<(String, M4, Option<String>)>> {
        if let Some(r) = self.node_defs.get(&n) {
            return Ok(r.clone());
        }
        let stop = depth > 64 || self.building.contains(&n) || {
            self.visits = self.visits.saturating_add(1);
            self.visits > 200_000
        };
        if stop {
            self.skip(&t("m3d.io.nodes_nested_too_deep_or_inside", &[]));
            return Ok(None);
        }
        self.building.insert(n);
        self.nodes += 1.0;
        let mut polys: Vec<PolyIn> = Vec::new();
        let mut segs: Vec<[P3; 2]> = Vec::new();
        let mut places: Vec<(String, M4, Option<String>)> = Vec::new();
        let children = self.doc.el(n).children.clone();
        for c in children {
            match self.name(c) {
                "instance_geometry" => {
                    let Some(g) = self.refer(self.attr(Some(c), "url"), Some("geometry")) else { continue };
                    let bind = self.bindings_of(c)?;
                    if self.geom_uses.get(&g).copied().unwrap_or(0) > 1 {
                        if let Some(id) = self.geom_def(model, g, &bind)? {
                            places.push((id, IDENT, None));
                        }
                    } else if let Some(geo) = self.read_geometry(g)? {
                        let (p, s) = self.polys_of(&geo, &bind)?;
                        polys.extend(p);
                        segs.extend(s);
                        if polys.len() > MAX_VERTS.saturating_mul(4) {
                            return Err(err("m3d.io.this_collada_file_has_more_than"));
                        }
                    }
                }
                "instance_node" | "node" => {
                    let tn = if self.name(c) == "node" { Some(c) } else { self.refer(self.attr(Some(c), "url"), Some("node")) };
                    let Some(tn) = tn else { continue };
                    if let Some((id, rm, rname)) = self.node_def(model, tn, depth.saturating_add(1))? {
                        let nm = self.node_matrix(tn)?;
                        places.push((id, mmul(&self.conv(&nm), &rm), rname));
                    }
                }
                "instance_controller" => self.skip(&t("m3d.io.skinned_or_morphed_meshes", &[])),
                "instance_camera" | "instance_light" => self.skip(&t("m3d.io.cameras_and_lights", &[])),
                _ => {}
            }
        }
        self.building.remove(&n);
        let own = self
            .kids(self.child(Some(n), "extra"), "technique")
            .into_iter()
            .find(|&tq| self.attr(Some(tq), "profile") == Some("ModelCraft"));
        let own_name = self.text(self.child(own, "name")).filter(|s| !s.is_empty());
        let raw = own_name
            .or_else(|| self.attr(Some(n), "name").filter(|s| !s.is_empty()))
            .or_else(|| self.attr(Some(n), "id").filter(|s| !s.is_empty()))
            .unwrap_or("Node");
        let nm = clean_name(raw);
        let comp = self.node_refs.get(&n).copied().unwrap_or(0) > 0;
        let mut out = None;
        if polys.is_empty() && segs.is_empty() && places.len() == 1 && !comp {
            if let Some((id, m, pname)) = places.into_iter().next() {
                out = Some((id, m, Some(pname.filter(|s| !s.is_empty()).unwrap_or(nm))));
            }
        } else if !polys.is_empty() || !segs.is_empty() || !places.is_empty() {
            let did = mesh_def_in(model, &polys, Some(&nm), if comp { "component" } else { "group" }, &segs);
            for (pid, pm, pname) in places {
                let pdname = model.def(&pid).map(|d| d.name.clone());
                if let Some(d) = model.def_mut(&did) {
                    let iid = place(d, &pid, &pm);
                    if let Some(pn) = pname.filter(|s| !s.is_empty())
                        && Some(&pn) != pdname.as_ref()
                        && let Some(inst) = d.insts.get_mut(iid)
                    {
                        inst.name = Some(pn);
                    }
                }
            }
            self.made.push(did.clone());
            out = Some((did, IDENT, None));
        }
        self.node_defs.insert(n, out.clone());
        Ok(out)
    }
}

/// `importDAE(model, text, name)` → {def, report}.
#[allow(clippy::too_many_lines)]
pub fn import_dae(model: &mut Model, text: &str, name: &str) -> Res<Imported> {
    let doc = match parse_xml(text, 5_000_000) {
        Ok(d) => d,
        Err(e) => return Err(err_with("m3d.io.dae_bad", &[("why", &e.to_string())])),
    };
    let root = doc.root;
    if doc.el(root).name != "COLLADA" {
        return Err(err_with("m3d.io.dae_root", &[("name", &crate::util::slice16(&doc.el(root).name, 40))]));
    }
    let mut by_id: FxMap<String, usize> = FxMap::default();
    doc.walk(root, |e| {
        if let Some(id) = doc.el(e).attr("id").filter(|s| !s.is_empty())
            && !by_id.contains_key(id)
        {
            by_id.insert(id.to_string(), e);
        }
    });
    // axes and units → a similarity C (Z up, metres); node matrices become C·M·C⁻¹, points C·p
    let asset = doc.child(Some(root), "asset");
    let unit = doc.child(asset, "unit");
    let meter = unit.and_then(|u| doc.el(u).attr("meter")).map_or(f64::NAN, parse_float);
    let s = if meter > 0.0 && meter < 1e6 { meter } else { 1.0 };
    let upel = doc.child(asset, "up_axis");
    let up_raw = upel.map(|u| doc.el(u).text.as_str()).filter(|s| !s.is_empty()).unwrap_or("Y_UP");
    let up = js::trim(up_raw).to_uppercase();
    let up = if ["X_UP", "Y_UP", "Z_UP"].contains(&up.as_str()) { up } else { "Y_UP".to_string() };
    let r: M4 = match up.as_str() {
        "Z_UP" => IDENT,
        "X_UP" => [0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        _ => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    };
    let c = mmul(&r, &[s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let ci = invert(&c);
    let mut x = Dae {
        doc: &doc,
        by_id,
        skipped: Map::new(),
        budget: i64::try_from(MAX_VERTS * 6).unwrap_or(i64::MAX),
        c,
        ci,
        mat_color: FxMap::default(),
        geom_cache: FxMap::default(),
        geometries: 0.0,
        geom_uses: FxMap::default(),
        node_refs: FxMap::default(),
        made: Vec::new(),
        geom_defs: FxMap::default(),
        node_defs: FxMap::default(),
        building: HashSet::new(),
        visits: 0,
        nodes: 0.0,
    };
    // how often each geometry and node is used (each node element counted once)
    {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack: Vec<(usize, u32)> = Vec::new();
        let lvs = doc.child(Some(root), "library_visual_scenes");
        let mut starts: Vec<usize> = Vec::new();
        for vs in doc.kids(lvs, "visual_scene") {
            starts.extend(doc.kids(Some(vs), "node"));
        }
        // count(n, depth) in the JS order: depth first, each node once
        for st in starts {
            stack.push((st, 0));
            // a stack of (node, depth, next child) frames
            let mut frames: Vec<(usize, u32, usize)> = Vec::new();
            while let Some((n, depth)) = stack.pop() {
                if seen.contains(&n) || depth > 64 {
                    continue;
                }
                seen.insert(n);
                frames.push((n, depth, 0));
                while let Some(&mut (fnode, fdepth, ref mut next)) = frames.last_mut() {
                    let kids = &doc.el(fnode).children;
                    let Some(&ch) = kids.get(*next) else {
                        frames.pop();
                        continue;
                    };
                    *next = next.saturating_add(1);
                    let e = doc.el(ch);
                    let target = |url: Option<&str>| x.by_id.get(url.unwrap_or("").get(1..).unwrap_or("")).copied();
                    match e.name.as_str() {
                        "instance_geometry" => {
                            if let Some(g) = target(e.attr("url").filter(|s| !s.is_empty())) {
                                let c = x.geom_uses.entry(g).or_insert(0);
                                *c = c.saturating_add(1);
                            }
                        }
                        "instance_node" => {
                            if let Some(tn) = target(e.attr("url").filter(|s| !s.is_empty()))
                                && doc.el(tn).name == "node"
                            {
                                let c = x.node_refs.entry(tn).or_insert(0);
                                *c = c.saturating_add(1);
                                if !seen.contains(&tn) && fdepth.saturating_add(1) <= 64 {
                                    seen.insert(tn);
                                    frames.push((tn, fdepth.saturating_add(1), 0));
                                }
                            }
                        }
                        "node" if !seen.contains(&ch) && fdepth.saturating_add(1) <= 64 => {
                            seen.insert(ch);
                            frames.push((ch, fdepth.saturating_add(1), 0));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let top = model.new_def("group", Some(&clean_name(name)));
    let scene_el = doc.child(doc.child(Some(root), "scene"), "instance_visual_scene");
    let scene_url = scene_el.and_then(|e| doc.el(e).attr("url")).filter(|s| !s.is_empty());
    let vs_all = doc.kids(doc.child(Some(root), "library_visual_scenes"), "visual_scene");
    let vs = match scene_url {
        Some(u) => x.refer(Some(u), Some("visual_scene")),
        None => None,
    }
    .or_else(|| vs_all.first().copied());
    let fail_all = |model: &mut Model, x: &Dae<'_>, top: &str| {
        model.defs.delete(top);
        for d in &x.made {
            model.defs.delete(d);
        }
    };
    let res: Res<()> = (|| {
        if let Some(vs) = vs {
            for n in doc.kids(Some(vs), "node") {
                let Some((id, rm, rname)) = x.node_def(model, n, 0)? else { continue };
                let nm = x.node_matrix(n)?;
                let m = mmul(&x.conv(&nm), &rm);
                let dname = model.def(&id).map(|d| d.name.clone());
                if let Some(t) = model.def_mut(&top) {
                    let iid = place(t, &id, &m);
                    if let Some(rn) = rname.filter(|s| !s.is_empty())
                        && Some(&rn) != dname.as_ref()
                        && let Some(inst) = t.insts.get_mut(iid)
                    {
                        inst.name = Some(rn);
                    }
                }
            }
        } else {
            let lg = doc.child(Some(root), "library_geometries");
            for g in doc.kids(lg, "geometry") {
                if let Some(id) = x.geom_def(model, g, &[])?
                    && let Some(t) = model.def_mut(&top)
                {
                    place(t, &id, &IDENT);
                }
            }
        }
        Ok(())
    })();
    res?;
    let n_insts = model.def(&top).map_or(0, |d| d.insts.len());
    if n_insts == 0 {
        fail_all(model, &x, &top);
        return Err(err("m3d.io.this_collada_file_has_no_geometry"));
    }
    let mut result = top.clone();
    // one plain group at the top (e.g. the "model" node our own export writes): that group is the import
    if n_insts == 1
        && let Some(only) = model.def(&top).and_then(|d| d.insts.values().next().cloned())
        && let Some(d) = model.def(&only.def)
        && d.kind == "group"
        && x.made.contains(&d.id)
        && only.m.iter().enumerate().all(|(i, v)| (v - if i % 5 == 0 { 1.0 } else { 0.0 }).abs() < 1e-12)
    {
        let did = d.id.clone();
        model.defs.delete(&top);
        if let Some(d) = model.def_mut(&did) {
            d.name = clean_name(name);
        }
        result = did;
    }
    let mut faces = 0.0;
    let mut edges = 0.0;
    let mut comps = 0.0;
    for id in &x.made {
        if let Some(d) = model.def(id) {
            faces += d.faces.len() as f64;
            edges += d.edges.len() as f64;
            if d.kind == "component" {
                comps += 1.0;
            }
        }
    }
    let mut report = Map::new();
    report.insert("faces".into(), Value::from(faces));
    report.insert("edges".into(), Value::from(edges));
    report.insert("geometries".into(), Value::from(x.geometries));
    report.insert("nodes".into(), Value::from(x.nodes));
    report.insert("components".into(), Value::from(comps));
    report.insert("up".into(), Value::from(up));
    report.insert("meter".into(), Value::from(s));
    report.insert("skipped".into(), Value::Object(std::mem::take(&mut x.skipped)));
    Ok(Imported { def: result, report, units: None })
}
