//! shop.js — a single-part shop drawing as a display list (landscape sheet, A3 unless another paper is named): I-shapes
//! (elevation at the web, top-flange plan when flanges have holes, section) and plates (face and edge); hole and cope
//! dimensions, hole schedule, title block. Other profiles are refused with a reason.

use crate::build::Part;
use crate::display::{Item, Pen, To};
use crate::i18n::{F, t, t0};
use crate::js::{Obj, max, min, num_to_string, round, sign, to_fixed};
use crate::paper::paper_of;
use crate::pdf::text_width;
use serde_json::Value;
use std::f64::consts::PI;

const SCALES: [f64; 15] = [1.0, 2.0, 2.5, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0, 200.0];
fn pick_scale(size: f64, room: f64) -> f64 {
    SCALES.iter().copied().find(|n| size / n <= room).unwrap_or(200.0)
}
fn fmt_len(v: f64) -> String {
    num_to_string(round(v))
}
fn ratio(n: f64) -> String {
    format!("1:{}", num_to_string(n))
}

/// What the page passes about the part.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub paper: String,
    pub mark: String,
    pub asm_mark: String,
    pub count: Option<f64>,
    pub weld: String,
    pub model: String,
    pub date: String,
}

/// A shop-drawing dimension.
#[derive(Clone, Debug)]
pub struct Dim {
    pub dir: &'static str,
    pub value: f64,
    pub label: String,
}

/// `shopDrawing(part, info)`.
#[derive(Clone, Debug)]
pub struct Shop {
    pub w: f64,
    pub h: f64,
    pub paper: &'static str,
    pub items: Vec<Item>,
    pub dims: Vec<Dim>,
    pub scale: (f64, f64),
}

/// `shopRefusal(part)`.
pub fn shop_refusal(part: Option<&Part>) -> Option<String> {
    let p = match part {
        None => return Some(t0("shopdwg.err.select")),
        Some(p) => p,
    };
    if p.profile.kind == "I" || p.profile.kind == "PL" {
        return None;
    }
    Some(t("shopdwg.err.kind", &[("profile", F::from(p.profile_name.as_str()))]))
}

struct B {
    items: Vec<Item>,
    dims: Vec<Dim>,
}
impl B {
    fn p(&mut self, pts: Vec<[f64; 2]>, pen: &Pen, closed: bool) {
        self.items.push(Item::Path { pts, w: pen.w, dash: pen.dash.clone(), closed, fill: false });
    }
    fn t(&mut self, s: &str, x: f64, y: f64, h: f64, o: To) {
        self.items.push(Item::Text {
            s: s.to_string(),
            x,
            y,
            h,
            align: if o.align.is_empty() { "left" } else { o.align },
            rot: o.rot,
            bold: o.bold,
        });
    }
    fn c(&mut self, c: [f64; 2], r: f64, pen: &Pen) {
        self.items.push(Item::Circle { c, r, w: pen.w, dash: pen.dash.clone(), fill: None });
    }
    fn dim_h(&mut self, x1: f64, x2: f64, y: f64, from: f64, value: f64) {
        let label = fmt_len(value);
        let thin = Pen::new(0.18);
        self.p(vec![[x1, from], [x1, y - 1.5]], &thin, false);
        self.p(vec![[x2, from], [x2, y - 1.5]], &thin, false);
        self.p(vec![[x1, y], [x2, y]], &thin, false);
        for x in [x1, x2] {
            self.p(vec![[x - 0.9, y - 0.9], [x + 0.9, y + 0.9]], &Pen::new(0.35), false);
        }
        let tw = text_width(&label, 2.2, false);
        let above = (x2 - x1).abs() >= tw + 1.0;
        self.t(&label, (x1 + x2) / 2.0, y + if above { 0.8 } else { -3.2 }, 2.2, To::center());
        self.dims.push(Dim { dir: "h", value: round(value), label });
    }
    fn dim_v(&mut self, y1: f64, y2: f64, x: f64, from: f64, value: f64) {
        let label = fmt_len(value);
        let thin = Pen::new(0.18);
        self.p(vec![[from, y1], [x + 1.5, y1]], &thin, false);
        self.p(vec![[from, y2], [x + 1.5, y2]], &thin, false);
        self.p(vec![[x, y1], [x, y2]], &thin, false);
        for y in [y1, y2] {
            self.p(vec![[x - 0.9, y - 0.9], [x + 0.9, y + 0.9]], &Pen::new(0.35), false);
        }
        self.t(&label, x - 0.8, (y1 + y2) / 2.0, 2.2, To::center().rot(PI / 2.0));
        self.dims.push(Dim { dir: "v", value: round(value), label });
    }
}

/// Unique sorted positions (within 0.5 mm).
fn uniq(vals: impl IntoIterator<Item = f64>) -> Vec<f64> {
    let mut s: Vec<f64> = vals.into_iter().collect();
    s.sort_by(|a, b| crate::js::ord(a - b));
    let mut o: Vec<f64> = Vec::new();
    for v in s {
        if o.last().is_none_or(|l| v - l > 0.5) {
            o.push(v);
        }
    }
    o
}

pub fn shop_drawing(part: &Part, info: &Info) -> Result<Shop, String> {
    if let Some(r) = shop_refusal(Some(part)) {
        return Err(r);
    }
    let sheet = paper_of(&info.paper);
    let k = sheet.k;
    let mut b = B { items: vec![], dims: vec![] };
    let thin = Pen::new(0.18);
    let hidden = Pen::dashed(0.18, &[1.5, 1.0]);
    let center = Pen::dashed(0.13, &[4.0, 1.0, 0.6, 1.0]);
    b.p(vec![[10.0, 10.0], [sheet.w - 10.0, 10.0], [sheet.w - 10.0, sheet.h - 10.0], [10.0, sheet.h - 10.0]], &Pen::new(0.5), true);

    let p = &part.profile;
    let l = part.length;
    let is_plate = p.kind == "PL";
    let hh = if is_plate { p.b() } else { p.d() };
    let face_holes: Vec<&crate::build::Hole> =
        part.holes.iter().filter(|h| if is_plate { h.face == "plate" } else { h.face == "web" }).collect();
    let flange_holes: Vec<&crate::build::Hole> =
        if is_plate { vec![] } else { part.holes.iter().filter(|h| h.face == "top" || h.face == "bottom").collect() };

    let x0 = 30.0;
    let y0 = sheet.h - 122.0 * k;
    let room_h = sheet.h - 52.0 - y0;
    let mut nx = pick_scale(l, sheet.w - 30.0 - 55.0 * k - 70.0);
    if hh / nx > room_h {
        nx = pick_scale(hh, room_h);
    }
    let mut ny = nx;
    if hh / nx < 30.0 * min(k, 1.0) {
        ny = pick_scale(hh, min(60.0 * k, room_h));
    }
    let (sx, sy) = (1.0 / nx, 1.0 / ny);
    let ex = |x: f64| x0 + x * sx;
    let ey = |y: f64| y0 + (y + hh / 2.0) * sy;
    let view_name = if is_plate { t0("shopdwg.view.plate") } else { t0("shopdwg.view.elevation") };
    let vt = if nx == ny {
        format!("{view_name} {}", ratio(nx))
    } else {
        format!("{view_name} — {}", t("shopdwg.view.two-scales", &[("length", F::S(ratio(nx))), ("depth", F::S(ratio(ny)))]))
    };
    b.t(&vt, x0, min(ey(hh / 2.0) + 14.0, sheet.h - 37.0), 3.0, To::left().bold());

    let copes: Vec<&crate::state::Cut> = part.cuts.iter().filter(|c| c.kind == "cope").collect();
    let cope = |end: &str, side: &str| copes.iter().find(|c| c.end == end && c.side == side).copied();
    let mut out: Vec<[f64; 2]> = Vec::new();
    let h2 = hh / 2.0;
    let (bs, be, te, ts) = (cope("start", "bottom"), cope("end", "bottom"), cope("end", "top"), cope("start", "top"));
    match bs {
        Some(c) => out.extend([[0.0, -h2 + c.depth], [c.length, -h2 + c.depth], [c.length, -h2]]),
        None => out.push([0.0, -h2]),
    }
    match be {
        Some(c) => out.extend([[l - c.length, -h2], [l - c.length, -h2 + c.depth], [l, -h2 + c.depth]]),
        None => out.push([l, -h2]),
    }
    match te {
        Some(c) => out.extend([[l, h2 - c.depth], [l - c.length, h2 - c.depth], [l - c.length, h2]]),
        None => out.push([l, h2]),
    }
    match ts {
        Some(c) => out.extend([[c.length, h2], [c.length, h2 - c.depth], [0.0, h2 - c.depth]]),
        None => out.push([0.0, h2]),
    }
    b.p(out.iter().map(|q| [ex(q[0]), ey(q[1])]).collect(), &Pen::new(0.5), true);
    if !is_plate {
        for (side, y) in [("top", h2 - p.tf()), ("bottom", -h2 + p.tf())] {
            let (a, c) = (cope("start", side), cope("end", side));
            let xa = match a {
                Some(a) if a.depth > p.tf() => a.length,
                _ => 0.0,
            };
            let xb = match c {
                Some(c) if c.depth > p.tf() => l - c.length,
                _ => l,
            };
            if xb > xa {
                b.p(vec![[ex(xa), ey(y)], [ex(xb), ey(y)]], &Pen::new(0.3), false);
            }
        }
    }
    for h in &face_holes {
        let r = max(0.6, h.d / 2.0 * sy);
        b.c([ex(h.x), ey(h.y)], r, &Pen::new(0.3));
        b.p(vec![[ex(h.x) - r - 1.0, ey(h.y)], [ex(h.x) + r + 1.0, ey(h.y)]], &center, false);
    }
    for h in &flange_holes {
        let top = h.face == "top";
        let ya = if top { h2 } else { -h2 };
        let yb = if top { h2 - p.tf() } else { -h2 + p.tf() };
        for dx in [-h.d / 2.0, h.d / 2.0] {
            b.p(vec![[ex(h.x + dx), ey(ya)], [ex(h.x + dx), ey(yb)]], &hidden, false);
        }
        b.p(vec![[ex(h.x), ey(ya) + if top { 1.5 } else { -1.5 }], [ex(h.x), ey(yb) + if top { -1.5 } else { 1.5 }]], &center, false);
    }

    let stations = uniq(std::iter::once(0.0).chain(part.holes.iter().map(|h| h.x)).chain(std::iter::once(l)));
    let y_chain = ey(-h2) - 9.0;
    let y_all = ey(-h2) - 17.0;
    if stations.len() > 2 {
        for i in 1..stations.len() {
            let (a, c) = (stations.get(i - 1).copied().unwrap_or(0.0), stations.get(i).copied().unwrap_or(0.0));
            b.dim_h(ex(a), ex(c), y_chain, ey(-h2) - 1.0, c - a);
        }
    }
    b.dim_h(ex(0.0), ex(l), y_all, ey(-h2) - 1.0, l);
    let ys = uniq(face_holes.iter().map(|h| h.y));
    if !ys.is_empty() {
        let mut chain = vec![-h2];
        chain.extend(&ys);
        chain.push(h2);
        for i in 1..chain.len() {
            let (a, c) = (chain.get(i - 1).copied().unwrap_or(0.0), chain.get(i).copied().unwrap_or(0.0));
            b.dim_v(ey(a), ey(c), ex(l) + 10.0, ex(l) + 1.0, c - a);
        }
    }
    b.dim_v(ey(-h2), ey(h2), ex(l) + 18.0, ex(l) + 1.0, hh);
    for (i, c) in copes.iter().enumerate() {
        let x = if c.end == "start" { ex(c.length / 2.0) } else { ex(l - c.length / 2.0) };
        let fi = i as f64;
        let y = if c.side == "top" { ey(h2) + 3.0 + fi * 3.0 } else { ey(-h2) - 4.0 - fi * 3.0 };
        b.t(&t("shopdwg.cope", &[("length", F::S(fmt_len(c.length))), ("depth", F::S(fmt_len(c.depth)))]), x, y, 2.2, To::center());
    }

    if !flange_holes.is_empty() {
        let bf = p.bf();
        let nf = pick_scale(bf, 40.0 * k);
        let sf = max(sy, 1.0 / nf);
        let y1 = y0 - 80.0 * k;
        let fy = |u: f64| y1 + (u + bf / 2.0) * sf;
        b.t(
            &t("shopdwg.view.top-flange", &[("scale", F::S(ratio(round(1.0 / sf * 100.0) / 100.0)))]),
            x0,
            fy(bf / 2.0) + 5.0,
            2.6,
            To::left().bold(),
        );
        let (a, c) = (cope("start", "top"), cope("end", "top"));
        let xa = match a {
            Some(a) if a.depth > p.tf() => a.length,
            _ => 0.0,
        };
        let xb = match c {
            Some(c) if c.depth > p.tf() => l - c.length,
            _ => l,
        };
        b.p(vec![[ex(xa), fy(-bf / 2.0)], [ex(xb), fy(-bf / 2.0)], [ex(xb), fy(bf / 2.0)], [ex(xa), fy(bf / 2.0)]], &Pen::new(0.5), true);
        b.p(vec![[ex(0.0) - 3.0, fy(0.0)], [ex(l) + 3.0, fy(0.0)]], &center, false);
        let solid = Pen::new(0.3);
        for h in &flange_holes {
            b.c([ex(h.x), fy(h.y)], max(0.6, h.d / 2.0 * sf), if h.face == "top" { &solid } else { &hidden });
        }
        let us = uniq(flange_holes.iter().map(|h| h.y));
        let mut chain = vec![-bf / 2.0];
        chain.extend(&us);
        chain.push(bf / 2.0);
        for i in 1..chain.len() {
            let (a, c) = (chain.get(i - 1).copied().unwrap_or(0.0), chain.get(i).copied().unwrap_or(0.0));
            b.dim_v(fy(a), fy(c), ex(l) + 10.0, ex(l) + 1.0, c - a);
        }
    }

    {
        let w = if is_plate { p.b() } else { p.width };
        let d = if is_plate { p.t() } else { p.depth };
        let ns = pick_scale(max(w, d), 55.0 * k);
        let ss = 1.0 / ns;
        let cx = sheet.w - 48.0 * max(k, 1.0);
        let cy = sheet.h - 92.0 * k;
        b.t(&t("shopdwg.view.section", &[("scale", F::S(ratio(ns)))]), cx, cy + max(d * ss, 20.0) / 2.0 + 12.0, 3.0, To::center().bold());
        if is_plate {
            b.p(
                vec![
                    [cx - w / 2.0 * ss, cy - d / 2.0 * ss],
                    [cx + w / 2.0 * ss, cy - d / 2.0 * ss],
                    [cx + w / 2.0 * ss, cy + d / 2.0 * ss],
                    [cx - w / 2.0 * ss, cy + d / 2.0 * ss],
                ],
                &Pen::new(0.5),
                true,
            );
            for u in uniq(face_holes.iter().map(|h| h.y)) {
                b.p(vec![[cx + u * ss, cy - d / 2.0 * ss - 2.0], [cx + u * ss, cy + d / 2.0 * ss + 2.0]], &center, false);
            }
            b.dim_h(cx - w / 2.0 * ss, cx + w / 2.0 * ss, cy - d / 2.0 * ss - 8.0, cy - d / 2.0 * ss - 1.0, w);
            b.t(&format!("t = {}", fmt_len(d)), cx + w / 2.0 * ss + 3.0, cy - 1.0, 2.2, To::left());
        } else {
            for lp in &p.loops {
                b.p(lp.iter().map(|q| [cx + q[0] * ss, cy + q[1] * ss]).collect(), &Pen::new(0.5), true);
            }
            for y in uniq(face_holes.iter().map(|h| h.y)) {
                b.p(vec![[cx - p.tw() * ss / 2.0 - 3.0, cy + y * ss], [cx + p.tw() * ss / 2.0 + 3.0, cy + y * ss]], &center, false);
            }
            for h in &flange_holes {
                let top = h.face == "top";
                let v = if top { d / 2.0 } else { -d / 2.0 };
                b.p(
                    vec![
                        [cx + h.y * ss, cy + v * ss + if top { 2.0 } else { -2.0 }],
                        [cx + h.y * ss, cy + (v - sign(v) * p.tf()) * ss + if top { -2.0 } else { 2.0 }],
                    ],
                    &center,
                    false,
                );
            }
            b.dim_h(cx - w / 2.0 * ss, cx + w / 2.0 * ss, cy - d / 2.0 * ss - 8.0, cy - d / 2.0 * ss - 1.0, w);
            b.dim_v(cy - d / 2.0 * ss, cy + d / 2.0 * ss, cx - w / 2.0 * ss - 6.0, cx - w / 2.0 * ss - 1.0, d);
        }
    }

    let mut groups: Vec<(String, usize)> = Vec::new();
    for h in &part.holes {
        let where_ = if is_plate {
            "plate".to_string()
        } else if h.face == "web" {
            "web".to_string()
        } else {
            h.face.clone()
        };
        let key = format!("{where_}|{}", num_to_string(h.d));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => g.1 = g.1.saturating_add(1),
            None => groups.push((key, 1)),
        }
    }
    let mut ly = min(72.0, y0 - 103.0 * k);
    b.t(&t0("shopdwg.holes"), 30.0, ly, 2.8, To::left().bold());
    if groups.is_empty() {
        ly -= 4.5;
        b.t(&t0("shopdwg.holes.none"), 30.0, ly, 2.5, To::left());
    }
    for (key, n) in &groups {
        ly -= 4.5;
        let (where_, d) = key.split_once('|').unwrap_or((key.as_str(), ""));
        let hk = match where_ {
            "plate" => Some("shopdwg.holes.plate"),
            "web" => Some("shopdwg.holes.web"),
            "top" => Some("shopdwg.holes.top"),
            "bottom" => Some("shopdwg.holes.bottom"),
            _ => None,
        };
        let s = match hk {
            Some(hk) => t(hk, &[("n", F::N(*n as f64)), ("d", F::from(d))]),
            None => format!("{n} x Ø{d} {where_}"),
        };
        b.t(&s, 30.0, ly, 2.5, To::left());
    }
    if !info.weld.is_empty() {
        ly -= 6.0;
        b.t(&t("shopdwg.weld", &[("weld", F::from(info.weld.as_str()))]), 30.0, ly, 2.5, To::left());
    }

    let tb = [sheet.w - 190.0, 10.0, sheet.w - 10.0, 64.0];
    b.p(vec![[tb[0], tb[1]], [tb[2], tb[1]], [tb[2], tb[3]], [tb[0], tb[3]]], &Pen::new(0.5), true);
    let count = info.count.filter(|c| *c != 0.0 && !c.is_nan()).unwrap_or(1.0);
    let rows = [
        [t0("shopdwg.tb.part-mark"), info.mark.clone(), t0("shopdwg.tb.assembly"), info.asm_mark.clone()],
        [t0("shopdwg.tb.profile"), part.profile_name.clone(), t0("shopdwg.tb.material"), part.material.clone()],
        [t0("shopdwg.tb.length"), format!("{} mm", fmt_len(l)), t0("shopdwg.tb.quantity"), num_to_string(count)],
        [
            t0("shopdwg.tb.weight-each"),
            format!("{} kg", to_fixed(part.weight, 1)),
            t0("shopdwg.tb.total"),
            format!("{} kg", to_fixed(part.weight * count, 1)),
        ],
        [t0("shopdwg.tb.model"), info.model.clone(), t0("shopdwg.tb.date"), info.date.clone()],
    ];
    for (i, r) in rows.iter().enumerate() {
        let y = tb[3] - 9.0 - i as f64 * 9.0;
        b.p(vec![[tb[0], y - 2.2], [tb[2], y - 2.2]], &thin, false);
        let bo = if i == 0 { To::left().bold() } else { To::left() };
        b.t(&r[0], tb[0] + 2.0, y + 2.6, 1.9, To::left());
        b.t(&r[1], tb[0] + 2.0, y - 0.6, 3.0, bo);
        b.t(&r[2], tb[0] + 92.0, y + 2.6, 1.9, To::left());
        b.t(&r[3], tb[0] + 92.0, y - 0.6, 3.0, bo);
    }
    b.p(vec![[tb[0] + 90.0, tb[1]], [tb[0] + 90.0, tb[3]]], &thin, false);
    b.t(&t0("shopdwg.tb.footer"), tb[0] + 2.0, tb[1] + 2.5, 1.9, To::left());
    b.t(&t("shopdwg.title", &[("mark", F::from(info.mark.as_str()))]), 30.0, sheet.h - 22.0, 5.0, To::left().bold());
    let key = if info.count.is_some_and(|c| c > 1.0) { "shopdwg.sub.off" } else { "shopdwg.sub" };
    let mut vars = Vec::new();
    if let Some(r) = &part.role {
        vars.push(("role", F::from(r.as_str())));
    }
    vars.push(("profile", F::from(part.profile_name.as_str())));
    vars.push(("length", F::S(fmt_len(l))));
    if let Some(c) = info.count {
        vars.push(("count", F::N(c)));
    }
    b.t(&t(key, &vars), 30.0, sheet.h - 29.0, 3.0, To::left());
    b.t(&crate::paper::key_label(sheet.key), tb[2] - 2.0, tb[1] + 2.5, 1.9, To::right());
    Ok(Shop { w: sheet.w, h: sheet.h, paper: sheet.key, items: b.items, dims: b.dims, scale: (nx, ny) })
}

/// The JSON of a shop drawing (or `{ refused }`).
pub fn shop_json(r: &Result<Shop, String>) -> Value {
    match r {
        Err(e) => Obj::new().set("refused", e.as_str()).done(),
        Ok(s) => Obj::new()
            .set("w", s.w)
            .set("h", s.h)
            .set("paper", s.paper)
            .set("items", Value::Array(s.items.iter().map(Item::j).collect()))
            .set(
                "dims",
                Value::Array(
                    s.dims
                        .iter()
                        .map(|d| Obj::new().set("dir", d.dir).set("value", d.value).set("label", d.label.as_str()).done())
                        .collect(),
                ),
            )
            .set("scale", Obj::new().set("length", s.scale.0).set("depth", s.scale.1).done())
            .done(),
    }
}
