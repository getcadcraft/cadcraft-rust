//! res-sheets.js (0.5 part 4, RC.5): a print-ready drawing set made from the model — plans of each level, the four
//! exterior elevations, sections from section lines, schedules, framing elevations — laid out on sheets with title
//! blocks and scales (`buildSet`), one vector PDF page a sheet (`setPdf`), and one plan on its own sheet (`planSheet`,
//! RC.8's takeoff). Items are in sheet millimetres, y up from the sheet's bottom, as the JS lays them out.

use crate::draw::m::{self, V2};
use crate::draw::plan::Scene;
use crate::draw::resframe::{Piece, Shape};

/// A framing piece as its wall elevation draws it: its role, along the wall (x) and up (z).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameBox {
    pub role: String,
    pub x0: f64,
    pub x1: f64,
    pub z0: f64,
    pub z1: f64,
}
/// The boxes of `frameWall`'s pieces.
pub fn frame_boxes(pieces: &[Piece]) -> Vec<FrameBox> {
    pieces
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Box { x0, x1, z0, z1, .. } => Some(FrameBox { role: p.role.to_string(), x0, x1, z0, z1 }),
            _ => None,
        })
        .collect()
}
use crate::draw::resviews::View;
use crate::draw::schedule::Schedule;
use crate::draw::tr::t0;

/// PAPERS (mm, landscape)
pub const PAPERS: [(&str, [f64; 2]); 5] =
    [("letter", [279.4, 215.9]), ("tabloid", [431.8, 279.4]), ("archd", [914.4, 609.6]), ("a3", [420.0, 297.0]), ("a1", [841.0, 594.0])];
pub fn paper(name: &str) -> [f64; 2] {
    PAPERS.iter().find(|p| p.0 == name).map_or([431.8, 279.4], |p| p.1)
}
const SCALES_FTIN: [(f64, &str); 8] = [
    (16.0, "3/4\" = 1'-0\""),
    (24.0, "1/2\" = 1'-0\""),
    (32.0, "3/8\" = 1'-0\""),
    (48.0, "1/4\" = 1'-0\""),
    (64.0, "3/16\" = 1'-0\""),
    (96.0, "1/8\" = 1'-0\""),
    (192.0, "1/16\" = 1'-0\""),
    (384.0, "1/32\" = 1'-0\""),
];
const SCALES_MM: [(f64, &str); 7] =
    [(20.0, "1:20"), (25.0, "1:25"), (50.0, "1:50"), (100.0, "1:100"), (200.0, "1:200"), (500.0, "1:500"), (1000.0, "1:1000")];
fn scales(units: &str) -> &'static [(f64, &'static str)] {
    if units == "mm" { &SCALES_MM } else { &SCALES_FTIN }
}

/// `scaleFor(w, h, room, units, minRatio)` → (ratio, label)
pub fn scale_for(w: f64, h: f64, room: [f64; 2], units: &str, min_ratio: f64) -> (f64, String) {
    if let Some(s) = scales(units).iter().filter(|s| s.0 >= min_ratio).find(|s| w / s.0 <= room[0] && h / s.0 <= room[1]) {
        return (s.0, s.1.to_string());
    }
    let r = m::max(w / room[0], h / room[1]).ceil();
    (r, format!("1:{}", crate::js::num_str(r)))
}

/// Text alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}
/// A drawing item on a sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Line { pts: Vec<V2>, w: f64, g: f64, dash: Option<[f64; 2]> },
    Fill { polys: Vec<Vec<V2>>, g: f64 },
    Text { s: String, x: f64, y: f64, size: f64, align: Align, bold: bool, rot: f64, g: f64 },
}
fn line(pts: Vec<V2>, w: f64, g: f64) -> Item {
    Item::Line { pts, w, g, dash: None }
}
fn text(s: &str, x: f64, y: f64, size: f64, align: Align, bold: bool) -> Item {
    Item::Text { s: s.to_string(), x, y, size, align, bold, rot: 0.0, g: 0.0 }
}

/// Plan mm → sheet mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub k: f64,
    pub ox: f64,
    pub oy: f64,
    pub ratio: f64,
}
impl Place {
    pub fn p(&self, q: V2) -> V2 {
        [self.ox + q[0] * self.k, self.oy + q[1] * self.k]
    }
}

/// A sheet: number, title, items; the place of its drawings (with their scale labels).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub number: String,
    pub title: String,
    pub items: Vec<Item>,
    pub views: Vec<(String, Option<String>, Option<Place>)>,
    pub scale: Option<String>,
}

// ------------------------------------------------------------------------------------------------- drawing → items

fn plan_items(scene: &Scene, pl: &Place) -> Vec<Item> {
    let k = pl.k;
    let p = |q: V2| pl.p(q);
    let mut out = Vec::new();
    let seg = |out: &mut Vec<Item>, a: &[f64], w: f64, g: f64, dash: Option<[f64; 2]>| {
        for q in a.as_chunks::<4>().0 {
            out.push(Item::Line { pts: vec![p([q[0], q[1]]), p([q[2], q[3]])], w, g, dash });
        }
    };
    let seg4 = |out: &mut Vec<Item>, a: &[[f64; 4]], w: f64, dash: Option<[f64; 2]>| {
        for q in a {
            out.push(Item::Line { pts: vec![p([q[0], q[1]]), p([q[2], q[3]])], w, g: 0.0, dash });
        }
    };
    for b in &scene.below {
        seg(&mut out, &b.segs, 0.13, 0.45, None);
    }
    for c in &scene.cut {
        if !c.loops.is_empty() {
            let polys = c.loops.iter().map(|l| l.as_chunks::<2>().0.iter().map(|q| p(*q)).collect()).collect();
            out.push(Item::Fill { polys, g: 0.75 });
        }
        seg(&mut out, &c.segs, 0.25, 0.0, None);
    }
    for w in &scene.walls {
        for pc in &w.pieces {
            if !pc.is_empty() {
                out.push(Item::Fill { polys: vec![pc.iter().map(|q| p(*q)).collect()], g: 0.55 });
            }
        }
    }
    for e in scene.wall_edges.iter().flatten() {
        let mut pts: Vec<V2> = e.iter().map(|q| p(*q)).collect();
        if let Some(f) = e.first() {
            pts.push(p(*f));
        }
        out.push(line(pts, 0.5, 0.0));
    }
    for s in &scene.symbols {
        seg4(&mut out, &s.sym.lines, 0.35, None);
        for a in &s.sym.arcs {
            let steps = m::max(8.0, ((a.a1 - a.a0).abs() / (std::f64::consts::PI / 32.0)).ceil());
            if !(steps <= 100_000.0) {
                continue;
            }
            let mut pts = Vec::new();
            let mut i = 0.0;
            while i <= steps {
                let t = a.a0 + (a.a1 - a.a0) * i / steps;
                pts.push(p([a.c[0] + a.r * m::cos(t), a.c[1] + a.r * m::sin(t)]));
                i += 1.0;
            }
            out.push(line(pts, 0.18, 0.0));
        }
        seg4(&mut out, &s.sym.thin, 0.18, None);
        seg4(&mut out, &s.sym.dashed, 0.18, Some([1.5, 1.0]));
    }
    for d in &scene.dims {
        let l = m::hypot(d.b[0] - d.a[0], d.b[1] - d.a[1]);
        if l < 1.0 || l.is_nan() {
            continue;
        }
        let u = [(d.b[0] - d.a[0]) / l, (d.b[1] - d.a[1]) / l];
        let n = [-u[1], u[0]];
        let off = match d.off_px {
            Some(px) => px * 0.3 / k,
            None => d.off.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0),
        };
        let a = [d.a[0] + n[0] * off, d.a[1] + n[1] * off];
        let b = [d.b[0] + n[0] * off, d.b[1] + n[1] * off];
        out.push(line(vec![p(a), p(b)], 0.13, 0.0));
        let tk = 1.0 / k;
        let tick = [(u[0] - n[0]) * tk, (u[1] - n[1]) * tk];
        for q in [a, b] {
            out.push(line(vec![p([q[0] - tick[0], q[1] - tick[1]]), p([q[0] + tick[0], q[1] + tick[1]])], 0.13, 0.0));
        }
        let mut rot = m::atan2(u[1], u[0]);
        if rot > std::f64::consts::FRAC_PI_2 + 1e-9 || rot <= -std::f64::consts::FRAC_PI_2 {
            rot += std::f64::consts::PI;
        }
        let mid = p([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
        out.push(Item::Text {
            s: d.text.clone(),
            x: mid[0] - m::sin(rot) * 0.8,
            y: mid[1] + m::cos(rot) * 0.8,
            size: 6.0,
            align: Align::Center,
            bold: false,
            rot,
            g: 0.0,
        });
    }
    for r in &scene.rooms {
        if r.label.is_empty() {
            continue;
        }
        let q = p(r.at);
        let n = r.label.len() as f64;
        for (i, s) in r.label.iter().enumerate() {
            let fi = i as f64;
            out.push(text(s, q[0], q[1] - (fi - (n - 1.0) / 2.0) * 3.4, if i == 0 { 7.5 } else { 6.5 }, Align::Center, i == 0));
        }
    }
    out
}

fn view_items(view: &View, pl: &Place) -> Vec<Item> {
    let p = |q: V2| pl.p(q);
    let mut out = Vec::new();
    for c in &view.cuts {
        let polys: Vec<Vec<V2>> = c.loops.iter().map(|l| l.iter().map(|q| p(*q)).collect()).collect();
        out.push(Item::Fill { polys: polys.clone(), g: if c.cls.contains("Geographic") { 0.82 } else { 0.6 } });
        for pl in polys {
            let mut pts = pl.clone();
            if let Some(f) = pl.first() {
                pts.push(*f);
            }
            out.push(line(pts, 0.45, 0.0));
        }
    }
    for l in &view.lines {
        out.push(line(vec![p([l[0], l[1]]), p([l[2], l[3]])], 0.18, 0.0));
    }
    if let Some(b) = view.bounds
        && !view.ground
    {
        out.push(line(vec![p([b[0] - 450.0, b[1]]), p([b[2] + 450.0, b[1]])], 0.6, 0.0));
    }
    out
}

fn framing_items(pieces: &[FrameBox], pl: &Place) -> Vec<Item> {
    let mut out = Vec::new();
    for pc in pieces {
        let (x0, x1, z0, z1) = (pc.x0, pc.x1, pc.z0, pc.z1);
        let r = vec![pl.p([x0, z0]), pl.p([x1, z0]), pl.p([x1, z1]), pl.p([x0, z1])];
        let role = pc.role.as_str();
        if role.contains("header") || role.contains("sill") || role.contains("plate") {
            out.push(Item::Fill { polys: vec![r.clone()], g: if role.contains("header") { 0.8 } else { 0.9 } });
        }
        let mut pts = r.clone();
        pts.push(pl.p([x0, z0]));
        out.push(line(pts, 0.13, 0.0));
    }
    out
}

fn table_items(sc: &Schedule, x: f64, y_top: f64, width: f64) -> Vec<Item> {
    let (row_h, cols) = (5.0, m::max(1.0, sc.head.len() as f64));
    let cw = width / cols;
    let mut out = Vec::new();
    let row = |out: &mut Vec<Item>, cells: &[String], y: f64, bold: bool| {
        for (i, c) in cells.iter().enumerate() {
            out.push(text(c, x + i as f64 * cw + 1.5, y - row_h + 1.6, 7.0, Align::Left, bold));
        }
    };
    out.push(line(vec![[x, y_top], [x + width, y_top]], 0.35, 0.0));
    row(&mut out, &sc.head, y_top, true);
    out.push(line(vec![[x, y_top - row_h], [x + width, y_top - row_h]], 0.25, 0.0));
    let mut y = y_top - row_h;
    for r in &sc.rows {
        row(&mut out, r, y, false);
        y -= row_h;
        out.push(line(vec![[x, y], [x + width, y]], 0.1, 0.6));
    }
    if let Some(t) = &sc.total {
        row(&mut out, t, y, true);
        y -= row_h;
    }
    out.push(line(vec![[x, y], [x + width, y]], 0.35, 0.0));
    out
}

// ------------------------------------------------------------------------------------------------------------ sheets

fn bounds_of_scene(scene: &Scene) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut g = |x: f64, y: f64| {
        if x < b[0] {
            b[0] = x;
        }
        if x > b[2] {
            b[2] = x;
        }
        if y < b[1] {
            b[1] = y;
        }
        if y > b[3] {
            b[3] = y;
        }
    };
    for w in &scene.walls {
        for q in &w.poly {
            g(q[0], q[1]);
        }
    }
    for c in scene.cut.iter().chain(scene.below.iter()) {
        for q in c.segs.as_chunks::<2>().0 {
            g(q[0], q[1]);
        }
    }
    for s in &scene.symbols {
        for q in &s.sym.poly {
            g(q[0], q[1]);
        }
    }
    if !b[0].is_finite() {
        return None;
    }
    let pad = m::max(b[2] - b[0], b[3] - b[1]) * 0.03 + 300.0;
    Some([b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad])
}
fn pad(b: [f64; 4], p: f64) -> [f64; 4] {
    [b[0] - p, b[1] - p, b[2] + p, b[3] + p]
}

/// What a cell draws.
enum Content<'a> {
    Plan(&'a Scene),
    View(&'a View),
    Framing(&'a [FrameBox]),
    Table(&'a Schedule),
}
/// A drawing cell: its content's bounds, drawn at the scale that fits (or a forced one).
struct Cell<'a> {
    title: String,
    bb: Option<[f64; 4]>,
    content: Content<'a>,
    forced: Option<f64>,
    min_ratio: f64,
}

/// `sheet(number, title, cells, opts)`
fn sheet(number: &str, title: &str, cells: &[Cell<'_>], paper_name: &str, units: &str, project: &str, date: &str) -> Sheet {
    let [pw, ph] = paper(paper_name);
    let (mg, strip) = (10.0, 18.0);
    let mut items = vec![
        line(vec![[mg, mg], [pw - mg, mg], [pw - mg, ph - mg], [mg, ph - mg], [mg, mg]], 0.5, 0.0),
        line(vec![[mg, mg + strip], [pw - mg, mg + strip]], 0.35, 0.0),
        line(vec![[pw - mg - 40.0, mg], [pw - mg - 40.0, mg + strip]], 0.35, 0.0),
        text(number, pw - mg - 20.0, mg + 6.0, 18.0, Align::Center, true),
        text(title, mg + 4.0, mg + 10.0, 12.0, Align::Left, true),
    ];
    let made = t0("res.sh.madewith");
    let strip_text: Vec<&str> = [project, date, made.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    items.push(Item::Text {
        s: strip_text.join("    "),
        x: mg + 4.0,
        y: mg + 4.0,
        size: 7.0,
        align: Align::Left,
        bold: false,
        rot: 0.0,
        g: 0.25,
    });
    let room = [pw - 2.0 * mg, ph - 2.0 * mg - strip];
    let cw = room[0] / m::max(1.0, cells.len() as f64);
    let mut views = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        let (x, y) = (mg + i as f64 * cw, mg + strip);
        let (dx, dy, dw, dh) = (x + 6.0, y + 14.0, cw - 12.0, room[1] - 22.0);
        let mut scale: Option<String> = None;
        let mut place: Option<Place> = None;
        match (&c.content, c.bb) {
            (Content::Table(sc), _) => items.extend(table_items(sc, dx, dy + dh, dw)),
            (content, Some(bb)) => {
                let (w, h) = (m::max(1.0, bb[2] - bb[0]), m::max(1.0, bb[3] - bb[1]));
                let (ratio, label) = match c.forced {
                    Some(r) => (
                        r,
                        scales(units).iter().find(|s| s.0 == r).map_or_else(|| format!("1:{}", crate::js::num_str(r)), |s| s.1.to_string()),
                    ),
                    None => scale_for(w, h, [dw, dh], units, c.min_ratio),
                };
                let k = 1.0 / ratio;
                let pl = Place { k, ox: dx + (dw - w * k) / 2.0 - bb[0] * k, oy: dy + (dh - h * k) / 2.0 - bb[1] * k, ratio };
                items.extend(match content {
                    Content::Plan(s) => plan_items(s, &pl),
                    Content::View(v) => view_items(v, &pl),
                    Content::Framing(p) => framing_items(p, &pl),
                    Content::Table(_) => Vec::new(),
                });
                scale = Some(label);
                place = Some(pl);
            }
            _ => {}
        }
        if !c.title.is_empty() {
            items.push(text(&c.title, x + 6.0, y + 7.0, 9.0, Align::Left, true));
            if let Some(sc) = &scale {
                items.push(Item::Text {
                    s: sc.clone(),
                    x: x + 6.0,
                    y: y + 3.0,
                    size: 7.0,
                    align: Align::Left,
                    bold: false,
                    rot: 0.0,
                    g: 0.25,
                });
            }
            items.push(line(vec![[x + 6.0, y + 6.0], [x + m::min(cw - 12.0, 90.0), y + 6.0]], 0.35, 0.0));
        }
        views.push((c.title.clone(), scale, place));
    }
    Sheet { number: number.to_string(), title: title.to_string(), items, views, scale: None }
}

/// `buildSet(input)`'s input.
#[derive(Default)]
pub struct SetInput<'a> {
    pub plans: Vec<(String, &'a Scene)>,
    pub elevations: Vec<(String, &'a View)>,
    pub sections: Vec<(String, &'a View)>,
    pub schedules: Vec<(String, &'a Schedule)>,
    pub framing: Vec<(String, &'a [FrameBox])>,
}

fn num2(i: usize) -> String {
    format!("{:02}", i.saturating_add(1))
}

/// `buildSet(input, { paper, units, project, date })`
pub fn build_set(input: &SetInput<'_>, paper_name: &str, units: &str, project: &str, date: &str) -> Vec<Sheet> {
    let [pw, ph] = paper(paper_name);
    let room_for = |n: f64| [(pw - 20.0) / n - 12.0, ph - 20.0 - 18.0 - 22.0];
    let building = if units == "mm" { 50.0 } else { 48.0 };
    let mut sheets = Vec::new();
    let mk = |title: &str, bb, content, forced, min_ratio| Cell { title: title.to_string(), bb, content, forced, min_ratio };
    // plans: one level a sheet
    let mut i = 0usize;
    for (title, scene) in &input.plans {
        let Some(bb) = bounds_of_scene(scene) else { continue };
        let mut s = sheet(
            &format!("A-1{}", num2(i)),
            title,
            &[mk(title, Some(bb), Content::Plan(scene), None, building)],
            paper_name,
            units,
            project,
            date,
        );
        s.scale = s.views.first().and_then(|v| v.1.clone());
        sheets.push(s);
        i = i.saturating_add(1);
    }
    // elevations: two a sheet, one scale for all
    let els: Vec<&(String, &View)> = input.elevations.iter().filter(|e| e.1.bounds.is_some()).collect();
    if !els.is_empty() {
        let ratio = els
            .iter()
            .filter_map(|e| e.1.bounds)
            .map(|b| scale_for(b[2] - b[0] + 1000.0, b[3] - b[1] + 1000.0, room_for(2.0), units, building).0)
            .fold(f64::NEG_INFINITY, m::max);
        for (n, pair) in els.chunks(2).enumerate() {
            let cells: Vec<Cell<'_>> =
                pair.iter().map(|e| mk(&e.0, e.1.bounds.map(|b| pad(b, 500.0)), Content::View(e.1), Some(ratio), 0.0)).collect();
            sheets.push(sheet(&format!("A-2{}", num2(n)), &t0("res.sh.elevations"), &cells, paper_name, units, project, date));
        }
    }
    // sections: two a sheet
    let secs: Vec<&(String, &View)> = input.sections.iter().filter(|e| e.1.bounds.is_some()).collect();
    for (n, pair) in secs.chunks(2).enumerate() {
        let cells: Vec<Cell<'_>> =
            pair.iter().map(|e| mk(&e.0, e.1.bounds.map(|b| pad(b, 500.0)), Content::View(e.1), None, building)).collect();
        sheets.push(sheet(&format!("A-3{}", num2(n)), &t0("res.sh.sections"), &cells, paper_name, units, project, date));
    }
    // schedules: side by side; a long one goes on in the next column
    let per_sheet = m::max(1.0, m::min(4.0, ((pw - 20.0) / 130.0).floor()));
    let fit = m::max(4.0, ((ph - 20.0 - 18.0 - 22.0) / 5.0).floor() - 3.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (per_sheet, fit) = (per_sheet as usize, fit as usize);
    let mut pieces: Vec<(String, Schedule)> = Vec::new();
    for (title, sc) in input.schedules.iter().filter(|s| !s.1.rows.is_empty()) {
        let mut r = 0usize;
        while r < sc.rows.len() {
            let last = r.saturating_add(fit) >= sc.rows.len();
            let rows = sc.rows.iter().skip(r).take(fit).cloned().collect();
            let t = if r > 0 { t0("res.sh.continued").replace("{title}", title) } else { title.clone() };
            pieces.push((t, Schedule { head: sc.head.clone(), rows, total: if last { sc.total.clone() } else { None }, nums: Vec::new() }));
            r = r.saturating_add(fit);
        }
    }
    for (n, group) in pieces.chunks(per_sheet).enumerate() {
        let cells: Vec<Cell<'_>> = group.iter().map(|p| mk(&p.0, None, Content::Table(&p.1), None, 0.0)).collect();
        sheets.push(sheet(&format!("A-4{}", num2(n)), &t0("res.sh.schedules"), &cells, paper_name, units, project, date));
    }
    // framing elevations: three walls a sheet
    let fr: Vec<&(String, &[FrameBox])> = input.framing.iter().filter(|f| !f.1.is_empty()).collect();
    for (n, group) in fr.chunks(3).enumerate() {
        let cells: Vec<Cell<'_>> = group
            .iter()
            .map(|f| {
                let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
                for pc in f.1.iter() {
                    b = [
                        m::min(m::min(b[0], pc.x0), pc.x1),
                        m::min(m::min(b[1], pc.z0), pc.z1),
                        m::max(m::max(b[2], pc.x0), pc.x1),
                        m::max(m::max(b[3], pc.z0), pc.z1),
                    ];
                }
                mk(&f.0, Some(pad(b, 300.0)), Content::Framing(f.1), None, 0.0)
            })
            .collect();
        sheets.push(sheet(&format!("S-1{}", num2(n)), &t0("res.sh.framing"), &cells, paper_name, units, project, date));
    }
    sheets
}

/// `planSheet(number, title, scene, { extra })`: one plan on its own sheet; `extra` = more plan points to take in.
#[allow(clippy::too_many_arguments)]
pub fn plan_sheet(
    number: &str,
    title: &str,
    scene: &Scene,
    paper_name: &str,
    units: &str,
    project: &str,
    date: &str,
    extra: &[V2],
) -> Option<Sheet> {
    let mut bb = bounds_of_scene(scene);
    for q in extra {
        let b = bb.unwrap_or([q[0] - 1000.0, q[1] - 1000.0, q[0] + 1000.0, q[1] + 1000.0]);
        bb = Some([m::min(b[0], q[0] - 300.0), m::min(b[1], q[1] - 300.0), m::max(b[2], q[0] + 300.0), m::max(b[3], q[1] + 300.0)]);
    }
    let bb = bb?;
    let min = if units == "mm" { 50.0 } else { 48.0 };
    Some(sheet(
        number,
        title,
        &[Cell { title: title.to_string(), bb: Some(bb), content: Content::Plan(scene), forced: None, min_ratio: min }],
        paper_name,
        units,
        project,
        date,
    ))
}

// ------------------------------------------------------------------------------------------------------------ PDF

const PT: f64 = 72.0 / 25.4;
const HW: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
/// `textW(s, size)`: Helvetica's width (pt).
pub fn text_w(s: &str, size: f64) -> f64 {
    let mut w = 0.0;
    for ch in s.chars() {
        let c = u32::from(ch);
        // a character outside the BMP counts by its first UTF-16 unit, as `charCodeAt(0)` does: a surrogate, 556
        w += if (32..=126).contains(&c) { HW.get(c.saturating_sub(32) as usize).map_or(556.0, |v| f64::from(*v)) } else { 556.0 };
    }
    w / 1000.0 * size
}
/// `lit(s)`: a PDF string in WinAnsi.
pub fn lit(s: &str) -> String {
    let mut out = String::from("(");
    for ch in s.chars() {
        let mapped = match ch {
            '²' => Some(0xb2),
            '³' => Some(0xb3),
            '°' => Some(0xb0),
            '×' => Some(0xd7),
            '—' => Some(0x97),
            '–' => Some(0x96),
            '’' => Some(0x92),
            '“' => Some(0x93),
            '”' => Some(0x94),
            '…' => Some(0x85),
            '½' => Some(0xbd),
            '¼' => Some(0xbc),
            '¾' => Some(0xbe),
            _ => None,
        };
        let mut c = mapped.unwrap_or(u32::from(ch));
        if c > 0xff || ((0x7f..0xa0).contains(&c) && mapped.is_none()) || c < 0x20 {
            c = 0x3f;
        }
        if c == 0x28 || c == 0x29 || c == 0x5c {
            out.push('\\');
            out.push(char::from_u32(c).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&c) {
            out.push(char::from_u32(c).unwrap_or('?'));
        } else {
            out.push_str(&format!("\\{c:03o}"));
        }
    }
    out.push(')');
    out
}
/// `num(v)`: toFixed(3) with trailing zeros dropped.
fn num(v: f64) -> String {
    let s = crate::js::to_fixed(if v.abs() < 5e-5 { 0.0 } else { v }, 3);
    let t = s.trim_end_matches('0');
    let t = t.strip_suffix('.').unwrap_or(t);
    if t.is_empty() || t == "-" { "0".into() } else { t.to_string() }
}

fn page_ops(items: &[Item]) -> String {
    let mut ops = vec!["1 J 1 j".to_string()];
    let pt = |q: &V2, i: usize| format!("{} {}{}", num(q[0] * PT), num(q[1] * PT), if i > 0 { " l" } else { " m" });
    for it in items {
        match it {
            Item::Fill { polys, g } => {
                ops.push(format!("{} g", num(*g)));
                for p in polys {
                    for (i, q) in p.iter().enumerate() {
                        ops.push(pt(q, i));
                    }
                    ops.push("h".into());
                }
                ops.push("f*".into());
            }
            Item::Line { pts, w, g, dash } => {
                let d = match dash {
                    Some(d) => format!("[{} {}] 0 d", num(d[0] * PT), num(d[1] * PT)),
                    None => "[] 0 d".into(),
                };
                ops.push(format!("{} w {} G {d}", num(w * PT), num(*g)));
                for (i, q) in pts.iter().enumerate() {
                    ops.push(pt(q, i));
                }
                ops.push("S".into());
            }
            Item::Text { s, x, y, size, align, bold, rot, g } => {
                let w = text_w(s, *size);
                let dx = match align {
                    Align::Left => 0.0,
                    Align::Right => -w,
                    Align::Center => -w / 2.0,
                };
                let (c, sn) = (m::cos(*rot), m::sin(*rot));
                let m6 = [c, sn, -sn, c, x * PT + dx * c, y * PT + dx * sn].map(num).join(" ");
                ops.push(format!("BT /{} {} Tf {} g {m6} Tm {} Tj ET", if *bold { "F2" } else { "F1" }, num(*size), num(*g), lit(s)));
            }
        }
    }
    ops.join("\n")
}

/// A file attached to the PDF (EmbeddedFiles).
pub struct Attach<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub mime: &'a str,
    pub desc: &'a str,
}

/// `setPdf(sheets, { paper, title, attach })`
pub fn set_pdf(sheets: &[Sheet], paper_name: &str, title: &str, attach: Option<&Attach<'_>>) -> Vec<u8> {
    let [pw, ph] = paper(paper_name);
    let n = sheets.len();
    let fs_no = n.saturating_mul(2).saturating_add(6);
    let mut objs: Vec<Option<String>> = Vec::new();
    let mut streams: Vec<(usize, Vec<u8>, String)> = Vec::new();
    let names = attach.map(|a| format!(" /Names << /EmbeddedFiles << /Names [{} {fs_no} 0 R] >> >>", lit(a.name))).unwrap_or_default();
    objs.push(Some(format!("<< /Type /Catalog /Pages 2 0 R{names} >>")));
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", i.saturating_mul(2).saturating_add(6))).collect();
    objs.push(Some(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" "))));
    objs.push(Some("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into()));
    objs.push(Some("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into()));
    objs.push(Some(format!("<< /Title {} /Producer (BimCraft) >>", lit(title))));
    for (i, s) in sheets.iter().enumerate() {
        objs.push(Some(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
            num(pw * PT),
            num(ph * PT),
            i.saturating_mul(2).saturating_add(7)
        )));
        objs.push(None);
        streams.push((objs.len().saturating_sub(1), enc(&page_ops(&s.items)), String::new()));
    }
    if let Some(a) = attach {
        objs.push(Some(format!(
            "<< /Type /Filespec /F {} /UF {} /Desc {} /EF << /F {} 0 R >> >>",
            lit(a.name),
            lit(a.name),
            lit(a.desc),
            fs_no.saturating_add(1)
        )));
        objs.push(None);
        let mime = if a.mime.is_empty() { "application/octet-stream" } else { a.mime };
        streams.push((
            objs.len().saturating_sub(1),
            a.bytes.to_vec(),
            format!("/Type /EmbeddedFile /Subtype /{} /Params << /Size {} >> ", mime.replace('/', "#2F"), a.bytes.len()),
        ));
    }
    let mut out: Vec<u8> = Vec::new();
    out.extend(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n");
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        match o {
            Some(text) => out.extend(enc(&format!("{} 0 obj\n{text}\nendobj\n", i.saturating_add(1)))),
            None => {
                let Some((_, b, head)) = streams.iter().find(|s| s.0 == i) else { continue };
                out.extend(enc(&format!("{} 0 obj\n<< {head}/Length {} >>\nstream\n", i.saturating_add(1), b.len())));
                out.extend(b);
                out.extend(b"\nendstream\nendobj\n");
            }
        }
    }
    let xref = out.len();
    let size = objs.len().saturating_add(1);
    let mut tail = format!("xref\n0 {size}\n0000000000 65535 f \n");
    for o in offs {
        tail.push_str(&format!("{o:010} 00000 n \n"));
    }
    tail.push_str(&format!("trailer\n<< /Size {size} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n"));
    out.extend(enc(&tail));
    out
}
/// `enc(s)`: each UTF-16 unit's low byte.
fn enc(s: &str) -> Vec<u8> {
    s.encode_utf16().map(|u| (u & 0xff) as u8).collect()
}
