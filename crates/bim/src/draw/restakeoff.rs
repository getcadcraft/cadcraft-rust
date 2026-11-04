//! res-takeoff.js (0.5 part 4, RC.8): the model's quantities to TakeoffCraft in one file — a PDF of the plan of each
//! level at a standard scale with the takeoff attached inside it (TakeoffCraft's embedded format), its marks laid on the
//! drawing (walls as runs, roofs as areas with their pitch, ridges / hips / valleys / eaves / rakes as runs, foundations
//! as runs and areas, doors, windows and devices as counts, cabinet fronts as runs), and the lumber cut list counted on
//! a sheet of its own. Items take the trade packs' assemblies where one fits (takeoff_packs.json, the JS's
//! res-takeoff-packs.js — CC0, generated from TakeoffCraft's trade packs), without rates.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};

use crate::draw::m::{self, V2};
use crate::draw::plan::Scene;
use crate::draw::ressheets::{Align, Attach, Item, PAPERS, Place, Sheet, blank_sheet, paper as paper_size, plan_sheet, set_pdf};
use crate::draw::tr::t;

pub const EMBED_NAME: &str = "takeoff.takeoffcraft.json";
const PT: f64 = 72.0 / 25.4;
const FT: f64 = 304.8;
const IN: f64 = 25.4;
const COLORS: [&str; 12] =
    ["#D9480F", "#1971C2", "#2F9E44", "#9C36B5", "#E8590C", "#0C8599", "#C2255C", "#5C940D", "#3B5BDB", "#F08C00", "#862E9C", "#087F5B"];
/// `ftPerUnit(ratio)`: feet per PDF point on a sheet drawn at 1:ratio.
pub fn ft_per_unit(ratio: f64) -> f64 {
    ratio / 864.0
}
fn r2(v: f64) -> f64 {
    m::round(v * 100.0) / 100.0
}

fn packs() -> &'static J {
    static P: OnceLock<J> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str(include_str!("takeoff_packs.json")).unwrap_or(J::Null))
}

/// An item of the takeoff as the model names it (`input.items[key]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemDef {
    pub name: String,
    /// linear | area | count
    pub kind: &'static str,
    pub pack: Option<String>,
    pub height_ft: Option<f64>,
    pub pitch: Option<f64>,
    /// rake | hip
    pub slope: Option<&'static str>,
}

/// One level's plan and its marks (plan mm).
#[derive(Clone, Debug, Default)]
pub struct TLevel {
    pub title: String,
    pub scene: Scene,
    pub runs: Vec<(String, Vec<V2>)>,
    pub areas: Vec<(String, Vec<V2>, Vec<Vec<V2>>)>,
    pub counts: Vec<(String, V2)>,
}

/// `buildTakeoff(input)`'s input.
#[derive(Clone, Debug, Default)]
pub struct TInput {
    pub name: String,
    pub units: String,
    pub paper: Option<String>,
    pub project: String,
    pub date: String,
    pub levels: Vec<TLevel>,
    pub items: Vec<(String, ItemDef)>,
    /// (item, how many, the row's label)
    pub cut_list: Vec<(String, f64, String)>,
}

/// What `buildTakeoff` makes.
pub struct Built {
    pub pdf: Vec<u8>,
    pub takeoff: J,
    pub stands_for: String,
    pub name: String,
    pub sheets: Vec<Sheet>,
}

/// A sheet's place and scale label (None: the cut list's, no scale).
type Scaled = Option<(Place, String)>;

fn unit_for(kind: &str, units: &str) -> &'static str {
    match kind {
        "count" => "EA",
        "area" => {
            if units == "mm" {
                "m²"
            } else {
                "SF"
            }
        }
        _ => {
            if units == "mm" {
                "m"
            } else {
                "LF"
            }
        }
    }
}

/// `buildTakeoff(input)` → None when there is no sheet.
pub fn build_takeoff(input: &TInput) -> Option<Built> {
    let units = if input.units == "mm" { "mm" } else { "ftin" };
    let paper = match &input.paper {
        Some(p) if PAPERS.iter().any(|x| x.0 == p) => p.clone(),
        _ => (if units == "mm" { "a3" } else { "tabloid" }).to_string(),
    };
    let def = |k: &str| input.items.iter().find(|x| x.0 == k).map(|x| &x.1);
    // items: the keys used, in the order first met
    let mut used: Vec<String> = Vec::new();
    let mut use_ = |k: &str| {
        if def(k).is_some() && !used.iter().any(|u| u == k) {
            used.push(k.to_string());
        }
    };
    for lv in &input.levels {
        for (k, _) in &lv.runs {
            use_(k);
        }
        for (k, _, _) in &lv.areas {
            use_(k);
        }
        for (k, _) in &lv.counts {
            use_(k);
        }
    }
    for (k, _, _) in &input.cut_list {
        use_(k);
    }
    let mut seq = 0u64;
    let mut id = |p: &str| {
        seq = seq.saturating_add(1);
        format!("{p}{seq}")
    };
    let mut item_id: HashMap<String, String> = HashMap::new();
    let mut items = Vec::new();
    for (i, k) in used.iter().enumerate() {
        let Some(d) = def(k) else { continue };
        let pk = d.pack.as_ref().and_then(|p| packs().get(p)).and_then(|p| p.get(units));
        let base = pk.filter(|b| b.get("type").and_then(J::as_str) == Some(d.kind));
        let bs = |f: &str| base.and_then(|b| b.get(f)).and_then(J::as_str).map(str::to_string);
        let bn = |f: &str| base.and_then(|b| b.get(f)).and_then(J::as_f64).filter(|v| *v != 0.0 && !v.is_nan());
        let name_src = if !d.name.is_empty() { d.name.clone() } else { bs("name").filter(|s| !s.is_empty()).unwrap_or_else(|| k.clone()) };
        let iid = id("i");
        let mut comps = Vec::new();
        for c in base.and_then(|b| b.get("components")).and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
            let w = c.get("waste").and_then(J::as_f64).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
            comps.push(json!({ "id": id("c"), "name": c.get("name").cloned().unwrap_or(J::Null), "unit": c.get("unit").cloned().unwrap_or(J::Null), "formula": c.get("formula").cloned().unwrap_or(J::Null), "waste": w, "mat": 0, "lab": 0 }));
        }
        let mut it = serde_json::Map::new();
        it.insert("id".into(), J::String(iid.clone()));
        it.insert("name".into(), J::String(crate::js::slice16(&name_src, 200)));
        it.insert("type".into(), J::String(d.kind.into()));
        it.insert("color".into(), J::String(COLORS.get(i.checked_rem(COLORS.len()).unwrap_or(0)).copied().unwrap_or("#D9480F").into()));
        it.insert("unit".into(), J::String(bs("unit").filter(|s| !s.is_empty()).unwrap_or_else(|| unit_for(d.kind, units).into())));
        let hf = match d.height_ft.filter(|v| v.is_finite()) {
            Some(v) => v,
            None => bn("heightFt").unwrap_or(0.0),
        };
        it.insert("heightFt".into(), crate::js::jn(hf));
        it.insert("waste".into(), crate::js::jn(bn("waste").unwrap_or(0.0)));
        it.insert("mat".into(), json!(0));
        it.insert("lab".into(), json!(0));
        it.insert("components".into(), J::Array(comps));
        if let Some(csi) = bs("csi").filter(|s| !s.is_empty()) {
            it.insert("csi".into(), J::String(csi));
        }
        if d.kind != "count"
            && let Some(p) = d.pitch.filter(|p| *p > 0.0)
        {
            it.insert("pitch".into(), crate::js::jn(m::round(12.0 * m::tan(p * std::f64::consts::PI / 180.0) * 10000.0) / 10000.0));
        }
        if d.kind == "linear"
            && let Some(s) = d.slope
        {
            it.insert("slope".into(), J::String(s.into()));
        }
        item_id.insert(k.clone(), iid);
        items.push(J::Object(it));
    }
    // the sheets: one plan a level (marks laid on it), then the cut list
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut page_marks: Vec<Vec<J>> = Vec::new();
    let mut scales: Vec<Scaled> = Vec::new();
    for (i, lv) in input.levels.iter().enumerate() {
        let mut extra: Vec<V2> = Vec::new();
        for (_, p) in &lv.runs {
            extra.extend(p.iter().copied());
        }
        for (_, p, _) in &lv.areas {
            extra.extend(p.iter().copied());
        }
        extra.extend(lv.counts.iter().map(|c| c.1));
        let Some(sh) =
            plan_sheet(&format!("Q-{}", i.saturating_add(1)), &lv.title, &lv.scene, &paper, units, &input.project, &input.date, &extra)
        else {
            continue;
        };
        let Some((_, label, Some(pl))) = sh.views.first().cloned() else { continue };
        let pp = |q: &V2| -> J {
            let s = pl.p(*q);
            json!([crate::js::jn(r2(s[0] * PT)), crate::js::jn(r2(s[1] * PT))])
        };
        let mut marks = Vec::new();
        for (k, pts) in &lv.runs {
            if let Some(iid) = item_id.get(k)
                && pts.len() >= 2
            {
                marks.push(json!({ "item": iid, "kind": "linear", "pts": pts.iter().map(pp).collect::<Vec<_>>() }));
            }
        }
        for (k, pts, holes) in &lv.areas {
            if let Some(iid) = item_id.get(k)
                && pts.len() >= 3
            {
                let hs: Vec<J> = holes.iter().filter(|h| h.len() >= 3).map(|h| J::Array(h.iter().map(pp).collect())).collect();
                marks.push(json!({ "item": iid, "kind": "area", "pts": pts.iter().map(pp).collect::<Vec<_>>(), "holes": hs }));
            }
        }
        for (k, at) in &lv.counts {
            if let Some(iid) = item_id.get(k) {
                marks.push(json!({ "item": iid, "kind": "count", "pts": [pp(at)] }));
            }
        }
        sheets.push(sh);
        page_marks.push(marks);
        scales.push(Some((pl, label.unwrap_or_default())));
    }
    let cut: Vec<&(String, f64, String)> = input.cut_list.iter().filter(|c| item_id.contains_key(&c.0) && c.1 > 0.0).collect();
    if !cut.is_empty() {
        let [pw, ph] = paper_size(&paper);
        let (per_line, gap, x0) = (30.0, 5.0, 130.0);
        let mut y = 0.0;
        let next = |sheets: &mut Vec<Sheet>, page_marks: &mut Vec<Vec<J>>, scales: &mut Vec<Scaled>, y: &mut f64| {
            let mut page = blank_sheet(
                &format!("Q-{}", sheets.len().saturating_add(1)),
                &t("res.to.cutsheet", &[]),
                &paper,
                units,
                &input.project,
                &input.date,
            );
            page.items.push(Item::Text {
                s: t("res.to.cutnote", &[]),
                x: 20.0,
                y: ph - 22.0,
                size: 9.0,
                align: Align::Left,
                bold: false,
                rot: 0.0,
                g: 0.25,
            });
            sheets.push(page);
            page_marks.push(Vec::new());
            scales.push(None);
            *y = ph - 34.0;
        };
        next(&mut sheets, &mut page_marks, &mut scales, &mut y);
        for c in cut {
            let lines = (c.1 / per_line).ceil();
            let h = lines * gap + 3.0;
            if y - h < 34.0 {
                next(&mut sheets, &mut page_marks, &mut scales, &mut y);
            }
            if let Some(page) = sheets.last_mut() {
                page.items.push(Item::Text {
                    s: c.2.clone(),
                    x: 20.0,
                    y: y - 3.5,
                    size: 9.0,
                    align: Align::Left,
                    bold: false,
                    rot: 0.0,
                    g: 0.0,
                });
                page.items.push(Item::Line { pts: vec![[20.0, y - h + 1.0], [pw - 20.0, y - h + 1.0]], w: 0.1, g: 0.7, dash: None });
            }
            let iid = item_id.get(&c.0).cloned().unwrap_or_default();
            let n = m::min(c.1, 100_000.0);
            let mut k = 0.0;
            while k < n {
                let cx = x0 + (k % per_line) * gap;
                let cy = y - 3.0 - (k / per_line).floor() * gap;
                if let Some(mk) = page_marks.last_mut() {
                    mk.push(json!({ "item": iid, "kind": "count", "pts": [[crate::js::jn(r2(cx * PT)), crate::js::jn(r2(cy * PT))]] }));
                }
                k += 1.0;
            }
            y -= h + 2.0;
        }
    }
    if sheets.is_empty() {
        return None;
    }
    let title = if input.name.is_empty() { "Takeoff".to_string() } else { input.name.clone() };
    let plain = set_pdf(&sheets, &paper, &title, None);
    let stands_for: String = Sha256::digest(&plain).iter().map(|b| format!("{b:02x}")).collect();
    let name = format!("{title}.pdf");
    let mut sheet_map = serde_json::Map::new();
    let mut marks = Vec::new();
    for (i, pm) in page_marks.into_iter().enumerate() {
        let key = format!("{stands_for}:{i}");
        if let Some(Some((pl, label))) = scales.get(i) {
            sheet_map.insert(key.clone(), json!({ "scale": { "method": "preset", "ftPerUnit": crate::js::jn(ft_per_unit(pl.ratio)), "preset": null, "label": label } }));
        }
        for mk in pm {
            let mut o = serde_json::Map::new();
            o.insert("id".into(), J::String(id("m")));
            o.insert("sheet".into(), J::String(key.clone()));
            if let J::Object(src) = mk {
                o.extend(src);
            }
            marks.push(J::Object(o));
        }
    }
    let takeoff = json!({
        "format": "takeoffcraft-project", "version": 1, "name": title, "seq": seq,
        "files": [{ "hash": stands_for, "name": name, "size": plain.len(), "pages": sheets.len() }],
        "sheets": J::Object(sheet_map), "items": items, "marks": marks, "areas": [],
    });
    let payload = cc_civil::project::js_stringify(
        &json!({ "format": "takeoffcraft-embedded", "version": 1, "standsFor": stands_for, "takeoff": takeoff }),
    );
    let attach = Attach { name: EMBED_NAME, bytes: payload.as_bytes(), mime: "application/json", desc: "TakeoffCraft takeoff" };
    let pdf = set_pdf(&sheets, &paper, &title, Some(&attach));
    Some(Built { pdf, takeoff, stands_for, name, sheets })
}

// ------------------------------------------------------------------------------------------------------------ the model

/// A base cabinet: its storey, footprint in plan, front edges.
pub type CabinetAt = (f64, Vec<V2>, Vec<(usize, usize)>);

/// `modelQuantities(d)`'s data (res-ui.js `takeoffData`).
#[derive(Default)]
pub struct QData {
    pub name: String,
    pub units: String,
    pub date: String,
    pub levels: Vec<crate::draw::levels::Level>,
    pub scenes: Vec<(f64, Scene)>,
    /// (wall, its type's name, exterior)
    pub walls: Vec<(crate::draw::walls::Wall, String, bool)>,
    /// (opening, its type's name)
    pub openings: Vec<(crate::draw::openings::Opening, String)>,
    /// (storey, its roof's plan)
    pub roofs: Vec<(Option<f64>, crate::draw::roof::RoofPlan)>,
    pub founds: Vec<(Option<f64>, Vec<crate::draw::resfound::Part>)>,
    /// (storey, footprint in plan, front edges)
    pub cabinets: Vec<CabinetAt>,
    pub devs: Vec<crate::draw::reselec::Device>,
    /// (storey, poly, holes)
    pub rooms: Vec<(f64, Vec<V2>, Vec<Vec<V2>>)>,
    pub lumber: Vec<crate::draw::resops::LumberRow>,
}

/// `takeoffData()`: what the model holds now (the scenes are the caller's).
pub fn gather(w: &mut crate::draw::writer::W<'_>, sn: &crate::draw::ops::Snapshot, units: &str) -> crate::api::R<QData> {
    use crate::draw::rooms::loop_at;
    let mut d = QData { units: units.to_string(), levels: sn.levels.clone(), ..QData::default() };
    for x in &sn.walls {
        let ty = crate::draw::walls::wall_type(&x.type_key);
        let name = match &ty {
            Some(t) => crate::draw::walls::type_name(t),
            None => {
                if x.type_name.is_empty() {
                    x.type_key.clone()
                } else {
                    x.type_name.clone()
                }
            }
        };
        d.walls.push((x.clone(), name, ty.is_none_or(|t| t.exterior)));
    }
    for o in crate::draw::openings::read_openings(w, None, &sn.walls)? {
        let name = match crate::draw::openings::opening_type(&o.type_key) {
            Some(t) => crate::draw::openings::opening_type_name(&t),
            None => o.type_name.clone(),
        };
        d.openings.push((o, name));
    }
    let mut loops: Vec<(f64, crate::draw::slabs::StoreyLoops)> = Vec::new();
    let mut loops_for = |w: &mut crate::draw::writer::W<'_>, id: f64| -> crate::api::R<crate::draw::rooms::Loops> {
        if let Some(l) = loops.iter().find(|x| x.0 == id) {
            return Ok(l.1.loops.clone());
        }
        let l = crate::draw::slabs::storey_loops(w, id)?;
        let out = l.loops.clone();
        loops.push((id, l));
        Ok(out)
    };
    for r in &sn.derived.roofs {
        let Some(wl) = sn.levels.iter().find(|l| l.guid.as_deref() == Some(r.walls.as_str())) else { continue };
        let lp = loops_for(w, wl.id)?;
        let Some(o) = loop_at(&lp.outlines, r.seed) else { continue };
        let ws: Vec<crate::draw::walls::Wall> = sn.walls.iter().filter(|x| x.storey == Some(wl.id)).cloned().collect();
        let kinds = crate::draw::roof::edge_kinds(&o.poly, &ws, &r.set());
        if let Ok(plan) = crate::draw::roof::roof_geometry(
            &o.poly,
            &kinds,
            r.thickness,
            0.0,
            r.fascia,
            crate::draw::roof::roof_defaults().fascia_t,
            r.soffit,
        ) {
            d.roofs.push((r.storey, plan));
        }
    }
    for f in crate::draw::resfound::read_foundations(w)? {
        let Some(st) = f.storey else { continue };
        let sl = crate::draw::slabs::storey_loops(w, st)?;
        let (stories, heavy) = crate::draw::resfound::loads_on(w, st, Some(&sl.walls))?;
        let prm = crate::draw::resfound::foundation_params(&f.kind, &f.stored, stories, heavy);
        if let Some(o) = loop_at(&sl.loops.outlines, f.seed) {
            d.founds.push((f.storey, crate::draw::resfound::foundation_geometry(&o.poly, &prm, None)));
        }
    }
    for lv in &sn.levels {
        for cb in crate::draw::rescounter::counter_cabinets(w, lv.id)? {
            d.cabinets.push((lv.id, cb.poly(), cb.fronts.clone()));
        }
    }
    d.devs = crate::draw::reselec::read_devices(w)?;
    for lv in &sn.levels {
        let lp = loops_for(w, lv.id)?;
        for r in &lp.rooms {
            d.rooms.push((lv.id, r.poly.clone(), r.holes.clone()));
        }
    }
    d.lumber = crate::draw::resops::lumber_list(w, units == "mm")?.0;
    Ok(d)
}

fn nearest(v: f64, list: &[f64]) -> f64 {
    let mut b = list.first().copied().unwrap_or(0.0);
    for x in list {
        if (x - v).abs() < (b - v).abs() {
            b = *x;
        }
    }
    b
}
fn ns(v: f64) -> String {
    crate::js::num_str(v)
}
/// `polyPerimeterGap(outer, inner)`
fn gap(outer: &[V2], inner: &[V2]) -> f64 {
    let (a, b, q) = (m::at(outer, 0), m::at(outer, 1), m::at(inner, 0));
    let l0 = m::hypot(b[0] - a[0], b[1] - a[1]);
    let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
    ((b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0])) / l
}
fn el_pack(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "socket" | "socket.double" | "socket.earth" | "appliance" => "el.receptacle",
        "switch" => "el.switch",
        "switch.2way" => "el.switch3",
        "switch.dimmer" => "el.dimmer",
        "junction" => "el.junction",
        "downlight" => "el.downlight",
        "luminaire" => "el.fixture",
        "board" => "el.panel",
        _ => return None,
    })
}

/// `level(id)`: a storey's marks, made when first asked for.
fn level_ix(by: &mut Vec<(u64, TLevel)>, id: Option<f64>) -> usize {
    let k = id.map_or(u64::MAX, crate::draw::plan::key_u64);
    match by.iter().position(|x| x.0 == k) {
        Some(i) => i,
        None => {
            by.push((k, TLevel::default()));
            by.len().saturating_sub(1)
        }
    }
}
fn push_runs(by: &mut Vec<(u64, TLevel)>, id: Option<f64>, x: (String, Vec<V2>)) {
    let i = level_ix(by, id);
    if let Some(l) = by.get_mut(i) {
        l.1.runs.push(x);
    }
}
fn push_areas(by: &mut Vec<(u64, TLevel)>, id: Option<f64>, x: (String, Vec<V2>, Vec<Vec<V2>>)) {
    let i = level_ix(by, id);
    if let Some(l) = by.get_mut(i) {
        l.1.areas.push(x);
    }
}
fn push_counts(by: &mut Vec<(u64, TLevel)>, id: Option<f64>, x: (String, V2)) {
    let i = level_ix(by, id);
    if let Some(l) = by.get_mut(i) {
        l.1.counts.push(x);
    }
}

/// `modelQuantities(d)`: the takeoff's input from the model.
pub fn model_quantities(d: &QData) -> TInput {
    let fl = |mm: f64| crate::draw::units::fmt_len(mm, &d.units);
    let mut items: Vec<(String, ItemDef)> = Vec::new();
    let mut by: Vec<(u64, TLevel)> = Vec::new();
    let def = |items: &mut Vec<(String, ItemDef)>, key: String, v: ItemDef| -> String {
        if !items.iter().any(|x| x.0 == key) {
            items.push((key.clone(), v));
        }
        key
    };
    let item = |name: String, kind: &'static str| ItemDef { name, kind, ..ItemDef::default() };
    // walls by type and height
    for (w, type_name, _) in &d.walls {
        let h_ft = m::round(w.height / FT * 100.0) / 100.0;
        let pack = if w.type_key.starts_with("ext-2x6") {
            Some("wall.ext.2x6".to_string())
        } else if w.type_key.starts_with("int-2x4") {
            Some(format!("wall.int.2x4.{}", ns(nearest(h_ft, &[8.0, 9.0, 10.0, 12.0]))))
        } else {
            None
        };
        let name = t("res.to.walls", &[("type", if type_name.is_empty() { &w.type_key } else { type_name }), ("h", &fl(w.height))]);
        let k =
            def(&mut items, format!("wall:{}:{}", w.type_key, ns(h_ft)), ItemDef { height_ft: Some(h_ft), pack, ..item(name, "linear") });
        push_runs(&mut by, w.storey, (k, vec![w.a, w.b]));
    }
    // doors and windows
    for (o, type_name) in &d.openings {
        let Some((w, _, exterior)) = d.walls.iter().find(|x| x.0.id == o.wall) else { continue };
        let l0 = m::hypot(w.b[0] - w.a[0], w.b[1] - w.a[1]);
        let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
        let s = o.offset + o.width / 2.0;
        let at = [w.a[0] + (w.b[0] - w.a[0]) / l * s, w.a[1] + (w.b[1] - w.a[1]) / l * s];
        let interior = !*exterior;
        let kind = o.kind();
        let tag = if kind == "door" { if interior { "i" } else { "x" } } else { "" };
        let nm = if type_name.is_empty() { o.type_key.clone() } else { type_name.clone() };
        let name = if kind == "door" && interior { format!("{nm} {}", t("res.to.interior", &[])) } else { nm };
        let pack = (kind == "door" && interior).then(|| "door.interior".to_string());
        let k = def(&mut items, format!("{kind}:{}:{tag}", o.type_key), ItemDef { pack, ..item(name, "count") });
        push_counts(&mut by, w.storey, (k, at));
        if kind == "window" {
            let k = def(
                &mut items,
                "window.trim".into(),
                ItemDef { pack: Some("window.trim".into()), ..item(t("res.to.windowtrim", &[]), "count") },
            );
            push_counts(&mut by, w.storey, (k, at));
        }
    }
    // roofs
    for (storey, p) in &d.roofs {
        level_ix(&mut by, *storey);
        for f in &p.faces {
            let pi = m::round(f.pitch * 100.0) / 100.0;
            let k = def(
                &mut items,
                format!("roof:{}", ns(pi)),
                ItemDef {
                    pitch: Some(pi),
                    pack: Some("roof.shingles".into()),
                    ..item(t("res.to.roof", &[("pitch", &crate::js::to_fixed(pi, 1))]), "area")
                },
            );
            push_areas(&mut by, *storey, (k, f.f.poly.clone(), Vec::new()));
        }
        let pitch = p.faces.iter().map(|f| f.pitch).fold(f64::NEG_INFINITY, m::max);
        for l in &p.lines {
            let kind = match l.kind {
                "ridge" => "ridge",
                "hip" => "hip",
                _ => "valley",
            };
            let key =
                if kind == "ridge" { "roofline:ridge".to_string() } else { format!("roofline:{kind}:{}", crate::js::to_fixed(pitch, 2)) };
            let mut dd = item(t(&format!("res.to.roofline.{kind}"), &[]), "linear");
            dd.pack = Some(if kind == "valley" { "roof.valley" } else { "roof.ridge" }.into());
            if kind != "ridge" {
                dd.slope = Some("hip");
                dd.pitch = Some(pitch);
            }
            let k = def(&mut items, key, dd);
            push_runs(&mut by, *storey, (k, vec![[l.a[0], l.a[1]], [l.b[0], l.b[1]]]));
        }
        for fa in &p.fascia {
            let k = if fa.kind == "eave" {
                def(
                    &mut items,
                    "roofline:eave".into(),
                    ItemDef { pack: Some("roof.eave".into()), ..item(t("res.to.roofline.eave", &[]), "linear") },
                )
            } else {
                def(
                    &mut items,
                    format!("roofline:rake:{}", crate::js::to_fixed(pitch, 2)),
                    ItemDef { slope: Some("rake"), pitch: Some(pitch), ..item(t("res.to.roofline.rake", &[]), "linear") },
                )
            };
            push_runs(&mut by, *storey, (k, vec![[fa.a[0], fa.a[1]], [fa.b[0], fa.b[1]]]));
        }
    }
    // foundations
    for (storey, geo) in &d.founds {
        level_ix(&mut by, *storey);
        for p in geo {
            if p.role == "slab" {
                let t_in = (p.z1 - p.z0) / IN;
                let k = def(
                    &mut items,
                    format!("slab:{}", ns(m::round(t_in * 100.0) / 100.0)),
                    ItemDef {
                        height_ft: Some((p.z1 - p.z0) / FT),
                        pack: Some(format!("slab.{}", ns(nearest(t_in, &[4.0, 5.0, 6.0])))),
                        ..item(t("res.to.slab", &[("t", &fl(p.z1 - p.z0))]), "area")
                    },
                );
                push_areas(&mut by, *storey, (k, p.outer.clone(), Vec::new()));
                continue;
            }
            let Some(inner) = p.inner.as_ref().filter(|i| i.len() == p.outer.len()) else { continue };
            let mut mid: Vec<V2> = p.outer.iter().zip(inner).map(|(q, r)| [(q[0] + r[0]) / 2.0, (q[1] + r[1]) / 2.0]).collect();
            let w_in = gap(&p.outer, inner).abs() / IN;
            let h_ft = (p.z1 - p.z0) / FT;
            let k = match p.role {
                "footing" => def(
                    &mut items,
                    format!("footing:{}:{}", ns(m::round(w_in)), ns(m::round((p.z1 - p.z0) / IN))),
                    ItemDef {
                        pack: Some(format!("footing.{}", ns(nearest(w_in, &[16.0, 20.0, 24.0])))),
                        ..item(t("res.to.footing", &[("w", &fl(w_in * IN)), ("d", &fl(p.z1 - p.z0))]), "linear")
                    },
                ),
                "stem" => def(
                    &mut items,
                    format!("stem:{}:{}", ns(m::round(w_in)), ns(m::round(h_ft * 100.0))),
                    ItemDef {
                        height_ft: Some(m::round(h_ft * 100.0) / 100.0),
                        pack: Some(format!("stem.{}.{}", ns(nearest(w_in, &[8.0, 10.0])), ns(nearest(h_ft, &[4.0, 8.0])))),
                        ..item(t("res.to.stem", &[("w", &fl(w_in * IN)), ("h", &fl(p.z1 - p.z0))]), "linear")
                    },
                ),
                _ => def(
                    &mut items,
                    format!("edge:{}", ns(m::round(w_in))),
                    ItemDef {
                        height_ft: Some(m::round(h_ft * 100.0) / 100.0),
                        ..item(t("res.to.edge", &[("w", &fl(w_in * IN))]), "linear")
                    },
                ),
            };
            if let Some(f) = mid.first().copied() {
                mid.push(f);
            }
            push_runs(&mut by, *storey, (k, mid));
        }
    }
    // base cabinets: their fronts
    for (storey, poly, fronts) in &d.cabinets {
        for &(i, j) in fronts {
            let k =
                def(&mut items, "cab.base".into(), ItemDef { pack: Some("cab.base".into()), ..item(t("res.to.cabinets", &[]), "linear") });
            push_runs(&mut by, Some(*storey), (k, vec![m::at(poly, i), m::at(poly, j)]));
        }
    }
    // electrical devices
    for v in &d.devs {
        let k = def(
            &mut items,
            format!("el:{}", v.kind),
            ItemDef { pack: el_pack(&v.kind).map(str::to_string), ..item(t(&format!("res.el.kind.{}", v.kind), &[]), "count") },
        );
        push_counts(&mut by, v.storey, (k, v.at));
    }
    // floor area per level
    for (storey, poly, holes) in &d.rooms {
        let lv_name = d.levels.iter().find(|l| l.id == *storey).map(|l| l.name.clone()).unwrap_or_default();
        let k = def(&mut items, format!("floor:{}", ns(*storey)), item(t("res.to.floor", &[("level", &lv_name)]), "area"));
        push_areas(&mut by, Some(*storey), (k, poly.clone(), holes.clone()));
    }
    let mut levels = Vec::new();
    for l in &d.levels {
        let key = crate::draw::plan::key_u64(l.id);
        let (Some(i), Some(sc)) = (by.iter().position(|x| x.0 == key), d.scenes.iter().find(|s| s.0 == l.id)) else { continue };
        let Some((_, mut tl)) = by.get(i).cloned() else { continue };
        tl.title = t("res.sh.plan", &[("level", &l.name)]);
        tl.scene = sc.1.clone();
        levels.push(tl);
    }
    let cut_list = d
        .lumber
        .iter()
        .map(|r| {
            let k = def(
                &mut items,
                format!("lumber:{}:{}", r.size, ns(m::round(r.stock))),
                item(t("res.to.lumber", &[("size", &r.size), ("len", &fl(r.stock))]), "count"),
            );
            (k, r.count, t("res.to.lumberrow", &[("size", &r.size), ("len", &fl(r.stock)), ("n", &ns(r.count))]))
        })
        .collect();
    TInput {
        name: d.name.clone(),
        units: d.units.clone(),
        paper: None,
        project: d.name.clone(),
        date: d.date.clone(),
        levels,
        items,
        cut_list,
    }
}
