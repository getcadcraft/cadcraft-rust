//! asm-drawing.js — the assembly drawing (one view of the placed or exploded parts, balloons per BOM row, trails,
//! the bill of materials above the title block) and the assembly's mesh exports (one world mesh for STL, a 3MF with
//! each part once and one build item per instance).

use crate::drawing::{self as dr, Seg};
use crate::export;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::vec::{self as v, M4, V3};
use serde_json::Value;

fn text_w(s: &str, size: f64) -> f64 {
    dr::win_ansi(s).1 / 1000.0 * size
}
/// `fit(s, size, w)`.
pub fn fit(s: &str, size: f64, w: f64) -> String {
    if text_w(s, size) <= w {
        return s.to_string();
    }
    let mut s = s.to_string();
    let mut guard = 0usize;
    while js::len16(&s) > 1 && text_w(&format!("{s}…"), size) > w && guard < 1_000_000 {
        guard = guard.saturating_add(1);
        s = js::slice16(&s, js::len16(&s).saturating_sub(1));
    }
    format!("{s}…")
}
fn fmt_mass(g: Option<f64>) -> String {
    match g {
        None => String::new(),
        Some(g) if g >= 1000.0 => format!("{} kg", js::num_str(js::fixed_num(g / 1000.0, 3))),
        Some(g) => format!("{} g", js::num_str(js::fixed_num(g, 2))),
    }
}

/// `balloonsFor(rows, instances, anchors)` → (item, anchor index).
pub fn balloons_for(rows: &[Value], instances: &[Value], anchors: &[[f64; 3]]) -> Vec<(Value, usize)> {
    let depth = |k: usize| anchors.get(k).map_or(f64::NAN, |a| a[2]);
    rows.iter()
        .filter_map(|r| {
            let mut best: Option<usize> = None;
            for (k, i) in instances.iter().enumerate() {
                if jv::strict_eq(i.get("part"), r.get("pid")) && best.is_none_or(|b| depth(k) > depth(b)) {
                    best = Some(k);
                }
            }
            best.map(|b| (r.get("item").cloned().unwrap_or(Value::Null), b))
        })
        .collect()
}

const BOM_COLS: [(&str, f64); 6] = [
    ("xch.asmdwg.col.item", 12.0),
    ("xch.asmdwg.col.qty", 12.0),
    ("xch.asmdwg.col.number", 34.0),
    ("xch.asmdwg.col.name", 58.0),
    ("xch.asmdwg.col.material", 38.0),
    ("xch.asmdwg.col.mass_each", 26.0),
];

fn circle(cx: f64, cy: f64, r: f64) -> Vec<Seg> {
    (0..48)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * js::f(i) / 48.0;
            let b = 2.0 * std::f64::consts::PI * (js::f(i) + 1.0) / 48.0;
            [cx + r * js::cos(a), cy + r * js::sin(a), cx + r * js::cos(b), cy + r * js::sin(b)]
        })
        .collect()
}

fn text(x: f64, y: f64, size: f64, s: impl Into<String>, anchor: &str, bold: bool) -> Value {
    let mut o =
        jv::obj(vec![("x", jv::num(x)), ("y", jv::num(y)), ("size", jv::num(size)), ("text", s.into().into()), ("anchor", anchor.into())]);
    if bold {
        jv::set(&mut o, "bold", true.into());
    }
    o
}

/// `assemblySheet(data, rows, opts)`; view = data.view, anchors = data.anchors; balloons (item, anchor index); trails world
/// points; date_ymd = the date's ISO day.
#[allow(clippy::too_many_arguments)]
pub fn assembly_sheet(
    view: &dr::View,
    anchors: &[[f64; 3]],
    exact: bool,
    rows_in: &[Value],
    name: &str,
    date_ymd: &str,
    balloons: &[(Value, usize)],
    trails: &[Vec<V3>],
    exploded: Option<&str>,
    paper: &str,
) -> Value {
    let (w, h) = dr::PAPERS.iter().find(|(k, _, _)| *k == paper).map_or((420.0, 297.0), |(_, a, b)| (*a, *b));
    let (margin, gap, tb_w, tb_h, row_h) = (10.0, 12.0, 180.0, 32.0, 6.0);
    let (mut st_vis, mut st_thin, mut st_chain, mut st_border): (Vec<Seg>, Vec<Seg>, Vec<Seg>, Vec<Seg>) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut fills: Vec<Value> = Vec::new();
    let mut texts: Vec<Value> = Vec::new();
    let rect = |st: &mut Vec<Seg>, x: f64, y: f64, ww: f64, hh: f64| {
        st.push([x, y, x + ww, y]);
        st.push([x + ww, y, x + ww, y + hh]);
        st.push([x + ww, y + hh, x, y + hh]);
        st.push([x, y + hh, x, y]);
    };
    let bx = w - margin - tb_w;
    let max_rows = ((h - 2.0 * margin - tb_h - 12.0) / row_h).floor() - 1.0;
    let more = js::max(0.0, js::f(rows_in.len()) - max_rows);
    let mut rows: Vec<Value> = rows_in.to_vec();
    if more > 0.0 {
        let keep = js::usize_of(js::i64_of(max_rows - 1.0));
        rows.truncate(keep);
        rows.push(jv::obj(vec![
            ("item", "".into()),
            ("qty", "".into()),
            ("number", "".into()),
            ("name", t("xch.asmdwg.more_parts", &[("n", F::N(more + 1.0))]).into()),
            ("material", "".into()),
            ("massEach", Value::Null),
        ]));
    }
    let table_h = (js::f(rows.len()) + 1.0) * row_h;
    let ty0 = margin + tb_h;
    let ty1 = ty0 + table_h;
    if !rows.is_empty() {
        rect(&mut st_border, bx, ty0, tb_w, table_h);
        let mut x = bx;
        for (_, cw) in BOM_COLS.iter().take(5) {
            x += cw;
            st_thin.push([x, ty0, x, ty1]);
        }
        for r in 1..=rows.len() {
            st_thin.push([bx, ty0 + js::f(r) * row_h, bx + tb_w, ty0 + js::f(r) * row_h]);
        }
        let base_y = |r: usize| ty1 - (js::f(r) + 1.0) * row_h + 1.9;
        x = bx;
        for (key, cw) in BOM_COLS {
            texts.push(text(x + 1.5, base_y(0), 2.5, t(key, &[]), "start", true));
            x += cw;
        }
        for (i, r) in rows.iter().enumerate() {
            let s_or = |k: &str| if jv::truthy(r.get(k)) { js::string_of(r.get(k)) } else { String::new() };
            let nm = match r.get("name") {
                None | Some(Value::Null) => String::new(),
                x => js::string_of(x),
            };
            let cells = [
                js::string_of(r.get("item")),
                js::string_of(r.get("qty")),
                s_or("number"),
                nm,
                s_or("material"),
                fmt_mass(jv::as_num(r.get("massEach"))),
            ];
            let mut cx = bx;
            for (k, c) in cells.iter().enumerate() {
                let cw = BOM_COLS.get(k).map_or(0.0, |x| x.1);
                texts.push(text(cx + 1.5, base_y(i.saturating_add(1)), 2.5, fit(c, 2.5, cw - 3.0), "start", false));
                cx += cw;
            }
        }
    }
    let (vx0, vy0, vx1, vy1) = (margin + gap, margin + gap, bx - gap, h - margin - gap - 6.0);
    let bw = view.bx[2] - view.bx[0];
    let bh = view.bx[3] - view.bx[1];
    let scale = dr::SCALES.iter().copied().find(|s| bw * s <= (vx1 - vx0) * 0.78 && bh * s <= (vy1 - vy0) * 0.78).unwrap_or(dr::SCALES[12]);
    let ox = vx0 + ((vx1 - vx0) - bw * scale) / 2.0;
    let oy = vy0 + ((vy1 - vy0) - bh * scale) / 2.0;
    let to_sheet = |u: f64, ww: f64| [ox + (u - view.bx[0]) * scale, oy + (ww - view.bx[1]) * scale];
    for g in &view.visible {
        let a = to_sheet(g[0], g[1]);
        let b = to_sheet(g[2], g[3]);
        st_vis.push([a[0], a[1], b[0], b[1]]);
    }
    let vw = dr::view_dir("iso").unwrap_or(dr::ViewDir { dir: [0.0; 3], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0] });
    for pts in trails {
        for pr in pts.windows(2) {
            if let [p0, p1] = pr {
                let a = to_sheet(v::dot(*p0, vw.right), v::dot(*p0, vw.up));
                let b = to_sheet(v::dot(*p1, vw.right), v::dot(*p1, vw.up));
                st_chain.push([a[0], a[1], b[0], b[1]]);
            }
        }
    }
    let label = match exploded {
        Some(e) => t("xch.asmdwg.view_exploded", &[("view", F::S(e.to_string())), ("scale", F::S(dr::scale_label(scale)))]),
        None => t("xch.asmdwg.view_iso", &[("scale", F::S(dr::scale_label(scale)))]),
    };
    texts.push(text((vx0 + vx1) / 2.0, vy1 + 1.0, 3.5, label, "middle", false));
    let mut placed: Vec<Value> = Vec::new();
    if !balloons.is_empty() {
        let cx = ox + bw * scale / 2.0;
        let cy = oy + bh * scale / 2.0;
        let rr = js::hypot2(bw * scale, bh * scale) / 2.0 + 12.0;
        let br = 4.0;
        #[derive(Clone)]
        struct B {
            item: Value,
            a: [f64; 2],
            ang: f64,
        }
        let mut list: Vec<B> = balloons
            .iter()
            .map(|(item, ai)| {
                let an = anchors.get(*ai).copied().unwrap_or([f64::NAN; 3]);
                let a = to_sheet(an[0], an[1]);
                B { item: item.clone(), a, ang: js::atan2(a[1] - cy, a[0] - cx) }
            })
            .collect();
        cc_m3d::sort::timsort(&mut list, |p, q| p.ang - q.ang);
        let min_a = (2.0 * br + 3.0) / rr;
        for i in 1..list.len() {
            let prev = list.get(i.saturating_sub(1)).map_or(f64::NAN, |b| b.ang);
            if let Some(b) = list.get_mut(i)
                && b.ang - prev < min_a
            {
                b.ang = prev + min_a;
            }
        }
        for b in &list {
            let x = js::max(vx0 + br, js::min(vx1 - br, cx + rr * js::cos(b.ang)));
            let y = js::max(vy0 + br, js::min(vy1 - br, cy + rr * js::sin(b.ang)));
            st_thin.extend(circle(x, y, br));
            let d0 = js::hypot2(b.a[0] - x, b.a[1] - y);
            let d = if d0 == 0.0 || d0.is_nan() { 1.0 } else { d0 };
            st_thin.push([x + (b.a[0] - x) / d * br, y + (b.a[1] - y) / d * br, b.a[0], b.a[1]]);
            let pts: Vec<Value> = (0..12)
                .map(|k| {
                    let ang = js::f(k) * std::f64::consts::PI / 6.0;
                    jv::pt2([b.a[0] + 0.6 * js::cos(ang), b.a[1] + 0.6 * js::sin(ang)])
                })
                .collect();
            fills.push(jv::obj(vec![("pts", Value::Array(pts))]));
            texts.push(text(x, y - 1.25, 3.5, js::string_of(Some(&b.item)), "middle", true));
            placed.push(jv::obj(vec![("item", b.item.clone()), ("x", jv::num(x)), ("y", jv::num(y)), ("anchor", jv::pt2(b.a))]));
        }
    }
    rect(&mut st_border, margin, margin, w - 2.0 * margin, h - 2.0 * margin);
    let by = margin;
    rect(&mut st_border, bx, by, tb_w, tb_h);
    st_thin.push([bx, by + 16.0, bx + tb_w, by + 16.0]);
    st_thin.push([bx + 110.0, by, bx + 110.0, by + 16.0]);
    st_thin.push([bx + 110.0, by + 16.0, bx + 110.0, by + tb_h]);
    texts.push(text(bx + 3.0, by + 23.0, 5.0, fit(name, 5.0, 104.0), "start", true));
    texts.push(text(bx + 113.0, by + 26.0, 2.5, t("drawing.tb.scale", &[]), "start", false));
    texts.push(text(bx + 113.0, by + 19.5, 4.0, dr::scale_label(scale), "start", false));
    texts.push(text(
        bx + 3.0,
        by + 10.0,
        2.5,
        t(if exact { "xch.asmdwg.units_exact" } else { "xch.asmdwg.units_facets" }, &[]),
        "start",
        false,
    ));
    texts.push(text(bx + 3.0, by + 4.0, 2.5, t(if exact { "xch.asmdwg.made_occt" } else { "drawing.tb.made" }, &[]), "start", false));
    texts.push(text(bx + 113.0, by + 10.0, 2.5, t("drawing.tb.date", &[]), "start", false));
    texts.push(text(bx + 113.0, by + 4.0, 3.0, date_ymd, "start", false));
    let segs = |s: &[Seg]| Value::Array(s.iter().map(|g| jv::nums(g)).collect());
    let stroke = |wd: f64, dash: Option<&[f64]>, s: &[Seg]| {
        jv::obj(vec![("width", jv::num(wd)), ("dash", dash.map_or(Value::Null, jv::nums)), ("segs", segs(s))])
    };
    let listed = rows.len().saturating_sub(usize::from(more > 0.0));
    jv::obj(vec![
        ("w", jv::num(w)),
        ("h", jv::num(h)),
        ("name", name.into()),
        ("scale", jv::num(scale)),
        (
            "strokes",
            Value::Array(vec![
                stroke(0.5, None, &st_vis),
                stroke(0.18, None, &st_thin),
                stroke(0.18, Some(&[8.0, 1.5, 1.5, 1.5]), &st_chain),
                stroke(0.7, None, &st_border),
            ]),
        ),
        ("fills", Value::Array(fills)),
        ("texts", Value::Array(texts)),
        ("balloons", Value::Array(placed)),
        ("rows", jv::unum(rows_in.len())),
        ("listed", jv::unum(listed)),
        ("view", jv::obj(vec![("x", jv::num(ox)), ("y", jv::num(oy)), ("w", jv::num(bw * scale)), ("h", jv::num(bh * scale))])),
        ("exact", exact.into()),
    ])
}

#[allow(clippy::cast_possible_truncation)]
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}

/// `assemblyMesh(items)` → (positions, indices).
pub fn assembly_mesh(items: &[(Vec<f64>, Vec<u32>, M4)]) -> (Vec<f64>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (p, ix, m) in items {
        let base = u32::try_from(positions.len() / 3).unwrap_or(u32::MAX);
        for c in p.chunks(3) {
            let g = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
            let q = v::apply_mat(m, [g(0), g(1), g(2)]);
            positions.extend(q.iter().map(|x| fround(*x)));
        }
        indices.extend(ix.iter().map(|k| base.wrapping_add(*k)));
    }
    (positions, indices)
}

fn num9(x: f64) -> String {
    let r = if x.abs() < 1e-12 { 0.0 } else { x };
    js::num_str(js::prec_num(r, 9))
}

/// `assembly3mf(name, parts, items)`; parts = (pid, name, positions, indices) in Map order, items = (pid, matrix).
pub fn assembly_3mf(name: &str, parts: &[(String, String, Vec<f64>, Vec<u32>)], items: &[(String, M4)]) -> Vec<u8> {
    use std::fmt::Write;
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    let _ = write!(
        o,
        " <metadata name=\"Title\">{}</metadata>\n <metadata name=\"Application\">PartCraft</metadata>\n <resources>\n",
        export::xml_esc(name)
    );
    let mut ids: Vec<(String, usize)> = Vec::new();
    for (n, (pid, pname, p, ix)) in parts.iter().enumerate() {
        let n = n.saturating_add(1);
        match ids.iter_mut().find(|(k, _)| k == pid) {
            Some(e) => e.1 = n,
            None => ids.push((pid.clone(), n)),
        }
        let _ = write!(o, "  <object id=\"{n}\" type=\"model\" name=\"{}\">\n   <mesh>\n    <vertices>\n", export::xml_esc(pname));
        for c in p.chunks(3) {
            let g = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
            let (x, y, z) = (g(0), -g(2), g(1));
            let _ = writeln!(o, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", num9(x), num9(y), num9(z));
        }
        o.push_str("    </vertices>\n    <triangles>\n");
        for tri in ix.chunks(3) {
            let g = |i: usize| tri.get(i).map_or_else(|| "undefined".to_string(), u32::to_string);
            let _ = writeln!(o, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", g(0), g(1), g(2));
        }
        o.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    o.push_str(" </resources>\n <build>\n");
    const Y_TO_Z: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    const Z_TO_Y: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for (pid, m) in items {
        let mm = v::mat_mul(&Y_TO_Z, &v::mat_mul(m, &Z_TO_Y));
        let tt = [mm[0], mm[1], mm[2], mm[4], mm[5], mm[6], mm[8], mm[9], mm[10], mm[12], mm[13], mm[14]]
            .iter()
            .map(|x| num9(*x))
            .collect::<Vec<_>>()
            .join(" ");
        let id = ids.iter().find(|(k, _)| k == pid).map_or_else(|| "undefined".to_string(), |(_, n)| n.to_string());
        let _ = writeln!(o, "  <item objectid=\"{id}\" transform=\"{tt}\"/>");
    }
    o.push_str(" </build>\n</model>\n");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";
    export::zip_stored(&[
        ("[Content_Types].xml".to_string(), types.as_bytes().to_vec()),
        ("_rels/.rels".to_string(), rels.as_bytes().to_vec()),
        ("3D/3dmodel.model".to_string(), o.into_bytes()),
    ])
}
