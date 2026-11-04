//! res-terrain.js (0.5 part 4, RC.10), the half the Terrain, Pad and Paving tools run: survey points from text
//! (`parsePointText`), contours from a DWG / DXF model (`drawingContours`), `thinToGrid`, `toModel`, `autoOffset`,
//! LandXML (`landXml`), and the ops `placeTerrain` / `changeTerrain` / `removeTerrain`, `placePad` / `changePad` /
//! `removePads`, `placeHardscape` / `changeHardscape` / `removeHardscape`. What follows the walls (the finished grade,
//! the bodies, `syncTerrain`) is resterrain.rs.
//!
//! The JS loads tin.js on first use and its ops throw `res.err.tinload` before that; here the same moment is
//! [`set_ground_loaded`](crate::draw::resterrain::set_ground_loaded).

use std::collections::HashMap;

use cc_civil::geo::{P2, P3};
use cc_civil::tin::{Breakline, Surface, ZPt, build_surface};
use cc_io_dwg::value::{Obj, V};
use cc_io_ifc::model::Id;
use serde_json::Value as J;

use crate::api::R;
use crate::draw::levels::read_levels;
use crate::draw::m;
use crate::draw::resterrain::{
    HARD_KINDS, HARD_PSET, PAD_PSET, Pad, TERRAIN_PSET, Terrain, finished_surface, ground_loaded, pad_shape, read_hardscape, read_pads,
    read_terrain, site_of, write_hardscape, write_terrain,
};
use crate::draw::tr::t0;
use crate::draw::writer::{W, key_err, round_d, round6, row};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;
const FT: f64 = 304.8;
/// TERRAIN_LIMITS.points
pub const MAX_POINTS: usize = 20_000;
/// PAD_DEFAULTS: margin, slope, depth
pub const PAD_DEFAULTS: (f64, f64, f64) = (5.0 * FT, 2.0, 8.0 * IN);

type Out = (Option<Id>, Vec<Id>, Vec<(String, Id)>);

fn fail(key: &str) -> crate::api::JsErr {
    key_err(key)
}
fn need_tin() -> R<()> {
    if ground_loaded() { Ok(()) } else { Err(fail("res.err.tinload")) }
}

// ------------------------------------------------------------------------------------------------------------ inputs

/// `Number.isFinite(Number(v))` of a cell that is not empty.
fn cell_num(v: Option<&&str>) -> Option<f64> {
    let s = v?;
    if s.is_empty() {
        return None;
    }
    let x = crate::js::str_num(s);
    x.is_finite().then_some(x)
}

/// `parsePointText(text, { format })`: PNEZD / PENZD / PNEZ / PENZ / NEZ / ENZ (XYZ) rows, comma, tab or space separated;
/// headers and other rows skipped. At most `cap` points.
pub fn parse_point_text(text: &str, format: &str, cap: usize) -> Vec<P3> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let s = crate::js::trim(line.strip_suffix('\r').unwrap_or(line));
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = if s.contains(',') {
            s.split(',').map(crate::js::trim).collect()
        } else if s.contains('\t') {
            s.split('\t').map(crate::js::trim).collect()
        } else {
            s.split(|c: char| c.is_whitespace()).filter(|x| !x.is_empty()).collect()
        };
        let n = |i: usize| cell_num(parts.get(i));
        let mut f = format;
        if f == "auto" {
            if parts.len() >= 4 && (0..4).all(|i| n(i).is_some()) && n(0).is_some_and(|x| x.fract() == 0.0) {
                f = "PNEZ";
            } else if (0..3).all(|i| n(i).is_some()) {
                f = "NEZ";
            } else {
                continue;
            }
        }
        let at: [usize; 3] = match f {
            "PNEZD" | "PNEZ" => [2, 1, 3],
            "PENZD" | "PENZ" => [1, 2, 3],
            "NEZ" | "NEZD" => [1, 0, 2],
            "ENZ" | "ENZD" | "XYZ" => [0, 1, 2],
            _ => continue,
        };
        let (Some(x), Some(y), Some(z)) = (n(at[0]), n(at[1]), n(at[2])) else { continue };
        out.push(P3 { x, y, z });
        if out.len() >= cap {
            break;
        }
    }
    out
}

fn vn(v: Option<&V>) -> f64 {
    match v {
        Some(V::Num(x)) => *x,
        Some(V::Bool(b)) => f64::from(u8::from(*b)),
        Some(V::Str(s)) => crate::js::str_num(s),
        Some(V::Null) => 0.0,
        _ => f64::NAN,
    }
}
/// `x || 0`
fn or0(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}
fn truthy_num(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

fn verts(e: &Obj) -> Vec<&Obj> {
    e.arr("vertices").map_or(&[][..], Vec::as_slice).iter().filter_map(V::as_obj).collect()
}

/// `drawingContours(model)`: contours from a DWG / DXF model — LWPOLYLINE / 2D polylines at their elevation, 3D
/// polylines, LINEs and POINTs with z. → (lines, points)
pub fn drawing_contours(model: &Obj, cap: usize) -> (Vec<Vec<P3>>, Vec<P3>) {
    let (mut lines, mut points) = (Vec::new(), Vec::new());
    let blocks = model.arr("blocks").map_or(&[][..], Vec::as_slice);
    let space = blocks.iter().filter_map(V::as_obj).find(|b| b.str("name").is_some_and(|n| n.eq_ignore_ascii_case("*model_space")));
    let Some(space) = space else { return (lines, points) };
    let mut count = 0usize;
    for e in space.arr("entities").map_or(&[][..], Vec::as_slice).iter().filter_map(V::as_obj) {
        if count >= cap {
            break;
        }
        let ty = e.str("type").unwrap_or("");
        match ty {
            "LWPOLYLINE" | "POLYLINE_2D" => {
                let z = or0(vn(e.get("elevation")));
                if !truthy_num(z) {
                    continue; // a contour has its elevation
                }
                let mut pts: Vec<P3> = verts(e).iter().map(|v| P3 { x: vn(v.get("x")), y: vn(v.get("y")), z }).collect();
                if e.get("closed").is_some_and(V::truthy)
                    && let Some(f) = pts.first().copied()
                {
                    pts.push(f);
                }
                if pts.len() >= 2 {
                    count = count.saturating_add(pts.len());
                    lines.push(pts);
                }
            }
            "POLYLINE_3D" => {
                let pts: Vec<P3> = verts(e).iter().map(|v| P3 { x: vn(v.get("x")), y: vn(v.get("y")), z: or0(vn(v.get("z"))) }).collect();
                if pts.len() >= 2 && pts.iter().any(|p| truthy_num(p.z)) {
                    count = count.saturating_add(pts.len());
                    lines.push(pts);
                }
            }
            "LINE" => {
                let p = |k: &str| e.obj(k).map(|o| (vn(o.get("x")), vn(o.get("y")), vn(o.get("z"))));
                let (a, b) = (p("start"), p("end"));
                if a.is_some_and(|a| truthy_num(a.2)) || b.is_some_and(|b| truthy_num(b.2)) {
                    let (a, b) = (a.unwrap_or((f64::NAN, f64::NAN, 0.0)), b.unwrap_or((f64::NAN, f64::NAN, 0.0)));
                    lines.push(vec![P3 { x: a.0, y: a.1, z: or0(a.2) }, P3 { x: b.0, y: b.1, z: or0(b.2) }]);
                    count = count.saturating_add(2);
                }
            }
            "POINT" => {
                if let Some(o) = e.obj("position")
                    && truthy_num(vn(o.get("z")))
                {
                    points.push(P3 { x: vn(o.get("x")), y: vn(o.get("y")), z: vn(o.get("z")) });
                    count = count.saturating_add(1);
                }
            }
            _ => {}
        }
    }
    (lines, points)
}

/// `thinToGrid(points, cell)`: the lowest point in each cell (in the order the cells were first met).
pub fn thin_to_grid(points: &[P3], cell: f64) -> Vec<P3> {
    let mut order: Vec<(i64, i64)> = Vec::new();
    let mut map: HashMap<(i64, i64), P3> = HashMap::new();
    let cell = if cell > 0.0 && cell.is_finite() { cell } else { 1.0 };
    #[allow(clippy::cast_possible_truncation)]
    let k = |v: f64| -> i64 { if v.is_finite() { (v / cell).floor() as i64 } else { i64::MIN } };
    for p in points {
        let key = (k(p.x), k(p.y));
        match map.get_mut(&key) {
            Some(q) => {
                if p.z < q.z {
                    *q = *p;
                }
            }
            None => {
                order.push(key);
                map.insert(key, *p);
            }
        }
    }
    order.iter().filter_map(|k| map.get(k).copied()).collect()
}

/// `toModel(points, { unitMm, offset, datum })`
pub fn to_model(points: &[P3], unit_mm: f64, offset: [f64; 2], datum: f64) -> Vec<P3> {
    points.iter().map(|p| P3 { x: p.x * unit_mm - offset[0], y: p.y * unit_mm - offset[1], z: (p.z - datum) * unit_mm }).collect()
}

/// `autoOffset(points, unitMm, to)`: far-off survey coordinates moved near the model (their middle onto `to`).
pub fn auto_offset(points: &[P3], unit_mm: f64, to: [f64; 2]) -> [f64; 2] {
    if points.is_empty() {
        return [0.0, 0.0];
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        x0 = m::min(x0, p.x);
        y0 = m::min(y0, p.y);
        x1 = m::max(x1, p.x);
        y1 = m::max(y1, p.y);
    }
    let (cx, cy) = ((x0 + x1) / 2.0 * unit_mm, (y0 + y1) / 2.0 * unit_mm);
    if m::hypot(cx, cy) > 2e6 { [cx - to[0], cy - to[1]] } else { [0.0, 0.0] }
}

/// `landXml(surfaces, { name, unit })` with the date it is written on (YYYY-MM-DD).
pub fn land_xml(surfaces: &[(String, &Surface)], name: &str, unit: &str, date: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    let k = match unit {
        "m" => 0.001,
        "ft" => 1.0 / FT,
        _ => 1.0,
    };
    let units = if unit == "ft" {
        "Imperial linearUnit=\"foot\" areaUnit=\"squareFoot\" volumeUnit=\"cubicFeet\"".to_string()
    } else {
        format!(
            "Metric linearUnit=\"{}\" areaUnit=\"squareMeter\" volumeUnit=\"cubicMeter\"",
            if unit == "m" { "meter" } else { "millimeter" }
        )
    };
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        format!("<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\" date=\"{date}\">"),
        format!("  <Units><{units}/></Units>"),
        format!("  <Project name=\"{}\"/>", esc(name)),
        "  <Surfaces>".into(),
    ];
    let fx = |v: f64| crate::js::to_fixed(v, 4);
    for (nm, s) in surfaces {
        let (pts, faces) = s.to_faces();
        out.push(format!("    <Surface name=\"{}\">", esc(nm)));
        out.push("      <Definition surfType=\"TIN\">".into());
        out.push("        <Pnts>".into());
        for (i, p) in pts.iter().enumerate() {
            out.push(format!("          <P id=\"{}\">{} {} {}</P>", i.saturating_add(1), fx(p.y * k), fx(p.x * k), fx(p.z * k)));
        }
        out.push("        </Pnts>".into());
        out.push("        <Faces>".into());
        for f in &faces {
            out.push(format!("          <F>{} {} {}</F>", u64::from(f[0]) + 1, u64::from(f[1]) + 1, u64::from(f[2]) + 1));
        }
        out.push("        </Faces>".into());
        out.push("      </Definition>".into());
        out.push("    </Surface>".into());
    }
    out.push("  </Surfaces>".into());
    out.push("</LandXML>".into());
    out.join("\n") + "\n"
}

// ------------------------------------------------------------------------------------------------------------ ops

fn nf(o: &J, k: &str) -> Option<f64> {
    o.get(k).and_then(J::as_f64).filter(|v| v.is_finite())
}
fn st(o: &J, k: &str) -> Option<String> {
    o.get(k).and_then(J::as_str).map(str::to_string)
}
fn jpt(v: &J) -> Option<P3> {
    Some(P3 { x: v.get("x")?.as_f64()?, y: v.get("y")?.as_f64()?, z: v.get("z")?.as_f64()? })
}
fn pts3(v: Option<&J>) -> Vec<P3> {
    v.and_then(J::as_array).map(|a| a.iter().filter_map(jpt).collect()).unwrap_or_default()
}
fn fix_guid(w: &mut W<'_>, spec: &mut J) -> String {
    if let Some(g) = spec.get("guid").and_then(J::as_str).filter(|g| !g.is_empty()) {
        return g.to_string();
    }
    let g = w.guid().val_str().unwrap_or_default();
    if let Some(o) = spec.as_object_mut() {
        o.insert("guid".into(), J::String(g.clone()));
    }
    g
}
fn on_site(w: &mut W<'_>, site: Id) -> R<JV> {
    let lp = w.must(site)?.a("ObjectPlacement").href();
    Ok(JV::H(w.place_at(lp, 0.0, 0.0, 0.0, 0.0)?))
}
fn bounds_area(p: &[P3]) -> f64 {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in p {
        x0 = m::min(x0, q.x);
        y0 = m::min(y0, q.y);
        x1 = m::max(x1, q.x);
        y1 = m::max(y1, q.y);
    }
    m::max(1.0, (x1 - x0) * (y1 - y0))
}
fn pack_pts(p: &[P3]) -> String {
    let r = |v: f64| crate::js::num_str(m::round(v));
    p.iter().map(|q| format!("{},{},{}", r(q.x), r(q.y), r(q.z))).collect::<Vec<_>>().join(";")
}
fn pack_faces(f: &[[u32; 3]]) -> String {
    f.iter().map(|x| format!("{},{},{}", x[0], x[1], x[2])).collect::<Vec<_>>().join(";")
}

/// `placeTerrain(spec)`: { points, lines, boundary, source, datum, offset, unit, interval }.
pub fn place_terrain(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    if !w.v4 {
        return Err(fail("res.err.ifc4"));
    }
    if read_terrain(w)?.is_some() {
        return Err(fail("res.err.terrainthere"));
    }
    let site = site_of(w).ok_or_else(|| fail("res.err.nosite"))?;
    need_tin()?;
    let mut pts = pts3(spec.get("points"));
    if pts.len() > MAX_POINTS {
        pts = thin_to_grid(&pts, (bounds_area(&pts) / MAX_POINTS as f64).sqrt() * 1.2);
    }
    let lines: Vec<Breakline> = spec
        .get("lines")
        .and_then(J::as_array)
        .map(|a| a.iter().take(100_000).map(|l| Breakline { name: String::new(), pts: pts3(Some(l)), from_group: false }).collect())
        .unwrap_or_default();
    let boundary: Option<Vec<P2>> = spec
        .get("boundary")
        .and_then(J::as_array)
        .map(|a| a.iter().filter_map(|q| Some(P2 { x: q.get("x")?.as_f64()?, y: q.get("y")?.as_f64()? })).collect());
    let zp: Vec<ZPt> = pts.iter().map(|p| ZPt { x: p.x, y: p.y, z: Some(p.z) }).collect();
    let surface = build_surface(&zp, &lines, boundary.as_deref(), 0.0).surface.ok_or_else(|| fail("res.err.surface"))?;
    let (pp, faces) = surface.to_faces();
    let guid = fix_guid(w, spec);
    let source = st(spec, "source").unwrap_or_else(|| "points".into());
    let datum = spec.get("datum").and_then(J::as_f64).unwrap_or(0.0);
    let offset = spec
        .get("offset")
        .and_then(J::as_array)
        .map_or([0.0, 0.0], |a| [a.first().and_then(J::as_f64).unwrap_or(f64::NAN), a.get(1).and_then(J::as_f64).unwrap_or(f64::NAN)]);
    let unit = st(spec, "unit").filter(|u| !u.is_empty()).unwrap_or_else(|| "mm".into());
    let interval = spec.get("interval").and_then(J::as_f64).unwrap_or(FT);
    let nm = w.l(&t0("res.terrain"));
    let pl = on_site(w, site)?;
    let oh = w.owner_history();
    let gj = w.gid_jv(&guid);
    let id = w.make("IFCGEOGRAPHICELEMENT", vec![gj, oh, nm.clone(), JV::Null, nm, pl, JV::Null, JV::Null, JV::s("TERRAIN")])?;
    w.note_guid(&guid, id);
    w.contain(id, site)?;
    let rows = vec![
        row("Source", "IFCLABEL", P::s(&source)),
        row("Datum", "IFCREAL", P::Num(round_d(datum, 6))),
        row("Unit", "IFCLABEL", P::s(&unit)),
        row("OffsetX", "IFCREAL", P::Num(round_d(offset[0], 3))),
        row("OffsetY", "IFCREAL", P::Num(round_d(offset[1], 3))),
        row("Interval", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(interval)))),
        row("Existing", "IFCTEXT", P::s(&pack_pts(&pp))),
        row("Faces", "IFCTEXT", P::s(&pack_faces(&faces))),
        row("Points", "IFCINTEGER", P::Num(pp.len() as f64)),
        row("ShapeKey", "IFCIDENTIFIER", P::s("")),
    ];
    w.upsert_pset(id, TERRAIN_PSET, &rows)?;
    let trn = Terrain {
        id,
        guid: guid.clone(),
        pts: pp.iter().map(|p| P3 { x: m::round(p.x), y: m::round(p.y), z: m::round(p.z) }).collect(),
        faces: faces.iter().map(|f| f.iter().map(|v| f64::from(*v)).collect()).collect(),
        key: String::new(),
        source,
        datum,
        offset,
        interval,
        cut: 0.0,
        fill: 0.0,
        unit,
    };
    let levels = read_levels(w)?;
    write_terrain(w, &trn, &levels)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

/// `changeTerrain(guid, { interval, raise })`
pub fn change_terrain(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let mut trn = read_terrain(w)?.filter(|t| t.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    need_tin()?;
    let mut rows = Vec::new();
    if let Some(iv) = ch.get("interval").and_then(J::as_f64).filter(|v| *v > 0.0) {
        rows.push(row("Interval", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(iv)))));
    }
    if let Some(raise) = nf(ch, "raise").filter(|v| *v != 0.0) {
        for p in &mut trn.pts {
            p.z += raise;
        }
        let k = match trn.unit.as_str() {
            "m" => 1000.0,
            "ft" => FT,
            _ => 1.0,
        };
        rows.push(row("Existing", "IFCTEXT", P::s(&pack_pts(&trn.pts))));
        rows.push(row("Datum", "IFCREAL", P::Num(round_d(trn.datum - raise / k, 6))));
    }
    if !rows.is_empty() {
        w.upsert_pset(trn.id, TERRAIN_PSET, &rows)?;
    }
    trn.key = String::new();
    let levels = read_levels(w)?;
    write_terrain(w, &trn, &levels)?;
    Ok((Some(trn.id), vec![trn.id], Vec::new()))
}

/// `removeTerrain(guid)`: the ground, its pads and the paving on it.
pub fn remove_terrain(w: &mut W<'_>, guid: &str) -> R<Out> {
    let trn = read_terrain(w)?.filter(|t| t.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut ids = vec![trn.id];
    ids.extend(read_pads(w)?.iter().map(|p| p.id));
    ids.extend(read_hardscape(w)?.iter().map(|h| h.id));
    w.drop_product(&ids)?;
    Ok((None, Vec::new(), Vec::new()))
}

/// The terrain made again (pads changed), then the footings on `level`'s storey step to the new grade.
fn regrade(w: &mut W<'_>, levels_of: &[String]) -> R<()> {
    let levels = read_levels(w)?;
    if let Some(mut trn) = read_terrain(w)? {
        need_tin()?;
        trn.key = String::new();
        write_terrain(w, &trn, &levels)?;
    }
    for g in levels_of {
        if let Some(st) = w.by_guid(g)? {
            crate::draw::resfound::sync_foundations(w, st, &crate::draw::res::ResCtx { levels: None, l: None })?;
        }
    }
    Ok(())
}

/// `placePad({ level, seed, margin, slope, depth })`
pub fn place_pad(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let (dm, ds, dd) = PAD_DEFAULTS;
    let num_or = |k: &str, d: f64| spec.get(k).map_or(d, |v| v.as_f64().unwrap_or(f64::NAN));
    let (margin, slope, depth) = (num_or("margin", dm), num_or("slope", ds), num_or("depth", dd));
    let level = st(spec, "level").unwrap_or_default();
    let seed = spec.get("seed").and_then(J::as_array).map_or([f64::NAN, f64::NAN], |a| {
        [a.first().and_then(J::as_f64).unwrap_or(f64::NAN), a.get(1).and_then(J::as_f64).unwrap_or(f64::NAN)]
    });
    if read_terrain(w)?.is_none() {
        return Err(fail("res.err.noterrain"));
    }
    let guid = fix_guid(w, spec);
    let levels = read_levels(w)?;
    let probe = Pad { id: 0.0, guid: String::new(), level: level.clone(), seed, margin, slope, depth };
    if pad_shape(w, &probe, &levels)?.is_none() {
        return Err(fail("model.err.nobuilding2"));
    }
    if !(1.0..=10.0).contains(&slope) {
        return Err(fail("res.err.padslope"));
    }
    let site = site_of(w).ok_or_else(|| fail("res.err.nosite"))?;
    let nm = w.l(&t0("res.pad"));
    let ot = w.l("Building pad");
    let pl = on_site(w, site)?;
    let oh = w.owner_history();
    let gj = w.gid_jv(&guid);
    let id = w.make("IFCGEOGRAPHICELEMENT", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s("USERDEFINED")])?;
    w.note_guid(&guid, id);
    w.contain(id, site)?;
    let rows = vec![
        row("Level", "IFCIDENTIFIER", P::s(&level)),
        row("SeedX", "IFCLENGTHMEASURE", P::Num(round6(w.u(seed[0])))),
        row("SeedY", "IFCLENGTHMEASURE", P::Num(round6(w.u(seed[1])))),
        row("Margin", "IFCLENGTHMEASURE", P::Num(round6(w.u(margin)))),
        row("Slope", "IFCREAL", P::Num(round_d(slope, 3))),
        row("Depth", "IFCLENGTHMEASURE", P::Num(round6(w.u(depth)))),
    ];
    w.upsert_pset(id, PAD_PSET, &rows)?;
    regrade(w, &[level])?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

/// `changePad(guid, { margin, depth, slope })`
pub fn change_pad(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let pd = read_pads(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut rows = Vec::new();
    if let Some(v) = nf(ch, "margin").filter(|v| *v >= 0.0) {
        rows.push(row("Margin", "IFCLENGTHMEASURE", P::Num(round6(w.u(v)))));
    }
    if let Some(v) = nf(ch, "depth") {
        rows.push(row("Depth", "IFCLENGTHMEASURE", P::Num(round6(w.u(v)))));
    }
    if let Some(v) = nf(ch, "slope") {
        if !(1.0..=10.0).contains(&v) {
            return Err(fail("res.err.padslope"));
        }
        rows.push(row("Slope", "IFCREAL", P::Num(round_d(v, 3))));
    }
    w.upsert_pset(pd.id, PAD_PSET, &rows)?;
    regrade(w, &[pd.level])?;
    Ok((Some(pd.id), vec![pd.id], Vec::new()))
}

/// `removePads([guid])`
pub fn remove_pads(w: &mut W<'_>, gl: &[String]) -> R<Out> {
    let list: Vec<Pad> = read_pads(w)?.into_iter().filter(|x| gl.contains(&x.guid)).collect();
    if !list.is_empty() {
        w.drop_product(&list.iter().map(|x| x.id).collect::<Vec<_>>())?;
    }
    let levels: Vec<String> = list.iter().map(|p| p.level.clone()).collect();
    regrade(w, &levels)?;
    Ok((None, Vec::new(), Vec::new()))
}

/// `placeHardscape({ level, points, kind, thickness })`
pub fn place_hardscape(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let kind = st(spec, "kind").unwrap_or_else(|| "walk".into());
    let points: Vec<[f64; 2]> = spec
        .get("points")
        .and_then(J::as_array)
        .map(|a| {
            a.iter()
                .take(10_000)
                .map(|p| [p.get(0).and_then(J::as_f64).unwrap_or(f64::NAN), p.get(1).and_then(J::as_f64).unwrap_or(f64::NAN)])
                .collect()
        })
        .unwrap_or_default();
    let thickness = spec.get("thickness").and_then(J::as_f64).filter(|t| *t > 0.0).unwrap_or(4.0 * IN);
    if points.len() < 3 {
        return Err(fail("res.err.hardscape"));
    }
    let trn = read_terrain(w)?.ok_or_else(|| fail("res.err.noterrain"))?;
    need_tin()?;
    let guid = fix_guid(w, spec);
    let level = st(spec, "level").unwrap_or_default();
    let site = site_of(w).ok_or_else(|| fail("res.err.nosite"))?;
    let nm = w.l(&t0(&format!("res.hard.{kind}")));
    let ot = w.l("Paving");
    let pl = on_site(w, site)?;
    let oh = w.owner_history();
    let gj = w.gid_jv(&guid);
    let id = w.make("IFCSLAB", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s("USERDEFINED")])?;
    w.note_guid(&guid, id);
    w.contain(id, site)?;
    let ptxt = points
        .iter()
        .map(|p| format!("{},{}", crate::js::num_str(round_d(w.u(p[0]), 3)), crate::js::num_str(round_d(w.u(p[1]), 3))))
        .collect::<Vec<_>>()
        .join(";");
    let rows = vec![
        row("Level", "IFCIDENTIFIER", P::s(&level)),
        row("Kind", "IFCLABEL", P::s(&kind)),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(thickness)))),
        row("Points", "IFCTEXT", P::s(&ptxt)),
    ];
    w.upsert_pset(id, HARD_PSET, &rows)?;
    let fin = finished_surface(w, &trn)?.ok_or_else(|| fail("res.err.hardscapeoff"))?;
    let h = crate::draw::resterrain::Hard { id, guid: guid.clone(), storey: None, level, kind, points, thickness };
    let levels = read_levels(w)?;
    if !write_hardscape(w, &h, &fin, &levels)? {
        return Err(fail("res.err.hardscapeoff"));
    }
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

/// `changeHardscape(guid, { thickness, kind })`
pub fn change_hardscape(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let mut h = read_hardscape(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut rows = Vec::new();
    if let Some(t) = ch.get("thickness").and_then(J::as_f64).filter(|t| *t > 0.0) {
        h.thickness = t;
        rows.push(row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(t)))));
    }
    if let Some(k) = st(ch, "kind").filter(|k| HARD_KINDS.contains(&k.as_str())) {
        h.kind = k.clone();
        rows.push(row("Kind", "IFCLABEL", P::s(&k)));
        let mut l = w.touch(h.id)?;
        l.set("Name", w.l(&t0(&format!("res.hard.{k}"))));
        w.write(&mut l)?;
    }
    if !rows.is_empty() {
        w.upsert_pset(h.id, HARD_PSET, &rows)?;
    }
    if let Some(trn) = read_terrain(w)? {
        need_tin()?;
        if let Some(fin) = finished_surface(w, &trn)? {
            let levels = read_levels(w)?;
            write_hardscape(w, &h, &fin, &levels)?;
        }
    }
    Ok((Some(h.id), vec![h.id], Vec::new()))
}

/// `removeHardscape([guid])`
pub fn remove_hardscape(w: &mut W<'_>, gl: &[String]) -> R<Out> {
    let ids: Vec<Id> = read_hardscape(w)?.into_iter().filter(|x| gl.contains(&x.guid)).map(|x| x.id).collect();
    if !ids.is_empty() {
        w.drop_product(&ids)?;
    }
    Ok((None, Vec::new(), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_text_formats() {
        let p = parse_point_text("P,N,E,Z\n1,100,200,30.5,TOP\n2 110 210 31\n# note\n5000\t6000\t12\n", "auto", 100);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].x, p[0].y, p[0].z), (200.0, 100.0, 30.5));
        assert_eq!((p[2].x, p[2].y, p[2].z), (6000.0, 5000.0, 12.0));
        assert_eq!(parse_point_text("1,2,3\n4,5,6\n", "auto", 1).len(), 1);
    }

    #[test]
    fn grid_offset() {
        let pts = vec![P3 { x: 0.0, y: 0.0, z: 5.0 }, P3 { x: 0.5, y: 0.2, z: 3.0 }, P3 { x: 2.0, y: 0.0, z: 1.0 }];
        let t = thin_to_grid(&pts, 1.0);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].z, 3.0);
        assert_eq!(auto_offset(&pts, 1000.0, [0.0, 0.0]), [0.0, 0.0]);
        let far = vec![P3 { x: 500_000.0, y: 4_000_000.0, z: 0.0 }];
        assert_eq!(auto_offset(&far, 1000.0, [10.0, 20.0]), [500_000_000.0 - 10.0, 4_000_000_000.0 - 20.0]);
    }
}
