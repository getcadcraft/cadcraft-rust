//! shapes.js — what a markup looks like, once, for the screen and for the saved PDF: `shapes(m, env)` → a display
//! list in PDF user space of `path` items (M / L / C / Z, stroke, fill, alphas, multiply for the highlighter), `text`
//! items and `image` items. `placeText`, `flatten`, `listBounds`, `listDistance` serve drawing and hit-testing.

use crate::arcs::{angle_arc, angle_at, arc_beziers, arc_through, format_angle};
use crate::font::{text_width, wrap_text};
use crate::i18n::{Fill, t, t0};
use crate::js::{JsMath, hypot, max, min, ns, parse_date, round, short_date, upper};
use crate::model::Markup;
use crate::units::{Cal, measure, measure_area};
use cc_m3d::json::Out;

pub type P = [f64; 2];

const K: f64 = 0.5522847498;
pub const NOTE_SIZE: f64 = 18.0;
pub const LINE_H: f64 = 1.2;

/// One path command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

/// A `path` item.
#[derive(Debug, Clone, PartialEq)]
pub struct PathItem {
    pub d: Vec<Seg>,
    pub stroke: Option<String>,
    pub fill: Option<String>,
    pub w: f64,
    pub alpha: f64,
    pub falpha: f64,
    pub cap: &'static str,
    pub join: &'static str,
    pub blend: Option<&'static str>,
    pub dash: Option<Vec<f64>>,
    pub rule: Option<&'static str>,
    pub nopick: bool,
    pub badge: bool,
}

/// A `text` item.
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    pub lines: Vec<String>,
    pub bbox: [f64; 4],
    pub size: f64,
    pub bold: bool,
    pub color: String,
    pub rot: f64,
    pub align: &'static str,
    pub valign: &'static str,
    pub pad: f64,
    pub lh: f64,
    pub alpha: f64,
    pub badge: bool,
}

/// An `image` item.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageItem {
    pub bbox: [f64; 4],
    pub src: String,
    pub rot: f64,
    pub alpha: f64,
}

/// One display-list item.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Path(PathItem),
    Text(TextItem),
    Image(ImageItem),
}

/// A legend's row (legend.js `legendRows`).
#[derive(Debug, Clone, PartialEq)]
pub struct LegendRow {
    pub color: String,
    pub label: String,
    pub n: f64,
}

/// `env` of `shapes(m, env)`.
#[derive(Debug, Clone, Default)]
pub struct ShapeEnv {
    pub cal: Option<Cal>,
    pub badge: f64,
    pub legend: Option<Vec<LegendRow>>,
}

/// The reading frame of a page rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub o: P,
    pub u: P,
    pub v: P,
    pub w: f64,
    pub h: f64,
}

/// `((rot % 360) + 360) % 360`
pub fn norm_rot(rot: f64) -> f64 {
    ((rot % 360.0) + 360.0) % 360.0
}

/// `frame(rot, box)`.
pub fn frame(rot: f64, b: [f64; 4]) -> Frame {
    let [x0, y0, x1, y1] = b;
    let r = norm_rot(rot);
    if r == 90.0 {
        Frame { o: [x0, y0], u: [0.0, 1.0], v: [1.0, 0.0], w: y1 - y0, h: x1 - x0 }
    } else if r == 180.0 {
        Frame { o: [x1, y0], u: [-1.0, 0.0], v: [0.0, 1.0], w: x1 - x0, h: y1 - y0 }
    } else if r == 270.0 {
        Frame { o: [x1, y1], u: [0.0, -1.0], v: [-1.0, 0.0], w: y1 - y0, h: x1 - x0 }
    } else {
        Frame { o: [x0, y1], u: [1.0, 0.0], v: [0.0, -1.0], w: x1 - x0, h: y1 - y0 }
    }
}

/// `m.rot || 0`
fn rot_of(r: f64) -> f64 {
    if r == 0.0 || r.is_nan() { 0.0 } else { r }
}

/// A line's baseline origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub x: f64,
    pub y: f64,
    pub s: String,
}

/// `placeText(t)` → (lines, angle).
pub fn place_text(t: &TextItem) -> (Vec<Placed>, f64) {
    let f = frame(rot_of(t.rot), t.bbox);
    let lhk = if t.lh == 0.0 || t.lh.is_nan() { LINE_H } else { t.lh };
    let lh = t.size * lhk;
    #[allow(clippy::cast_precision_loss)]
    let n = t.lines.len() as f64;
    let block_h = t.size + (n - 1.0) * lh;
    let top = if t.valign == "middle" { (f.h - block_h) / 2.0 } else { t.pad };
    let mut out = Vec::with_capacity(t.lines.len());
    for (i, s) in t.lines.iter().enumerate() {
        let w = text_width(s, t.size, t.bold);
        let du = if t.align == "center" {
            (f.w - w) / 2.0
        } else if t.align == "right" {
            f.w - t.pad - w
        } else {
            t.pad
        };
        #[allow(clippy::cast_precision_loss)]
        let dv = top + t.size * 0.8 + i as f64 * lh;
        out.push(Placed { x: f.o[0] + f.u[0] * du + f.v[0] * dv, y: f.o[1] + f.u[1] * du + f.v[1] * dv, s: s.clone() });
    }
    (out, norm_rot(rot_of(t.rot)))
}

// ---- path builders ----

/// `P.poly(pts, closed)`.
pub fn poly(pts: &[P], closed: bool) -> Vec<Seg> {
    let mut d: Vec<Seg> = pts.iter().enumerate().map(|(i, p)| if i > 0 { Seg::L(p[0], p[1]) } else { Seg::M(p[0], p[1]) }).collect();
    if closed && pts.len() > 2 {
        d.push(Seg::Z);
    }
    d
}
/// `P.rect(box)`.
pub fn rect(b: [f64; 4]) -> Vec<Seg> {
    let [x0, y0, x1, y1] = b;
    vec![Seg::M(x0, y0), Seg::L(x1, y0), Seg::L(x1, y1), Seg::L(x0, y1), Seg::Z]
}
/// `P.roundRect(box, r)`.
pub fn round_rect(b: [f64; 4], r: f64) -> Vec<Seg> {
    let [x0, y0, x1, y1] = b;
    let r = max(0.0, min(min(r, (x1 - x0) / 2.0), (y1 - y0) / 2.0));
    if r == 0.0 || r.is_nan() {
        return rect(b);
    }
    let k = r * (1.0 - K);
    vec![
        Seg::M(x0 + r, y0),
        Seg::L(x1 - r, y0),
        Seg::C(x1 - k, y0, x1, y0 + k, x1, y0 + r),
        Seg::L(x1, y1 - r),
        Seg::C(x1, y1 - k, x1 - k, y1, x1 - r, y1),
        Seg::L(x0 + r, y1),
        Seg::C(x0 + k, y1, x0, y1 - k, x0, y1 - r),
        Seg::L(x0, y0 + r),
        Seg::C(x0, y0 + k, x0 + k, y0, x0 + r, y0),
        Seg::Z,
    ]
}
/// `P.ellipse(box)`.
pub fn ellipse(b: [f64; 4]) -> Vec<Seg> {
    let [x0, y0, x1, y1] = b;
    let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (kx, ky) = (rx * K, ry * K);
    vec![
        Seg::M(cx + rx, cy),
        Seg::C(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry),
        Seg::C(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy),
        Seg::C(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry),
        Seg::C(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy),
        Seg::Z,
    ]
}
/// `P.circle(cx, cy, r)`.
pub fn circle(cx: f64, cy: f64, r: f64) -> Vec<Seg> {
    ellipse([cx - r, cy - r, cx + r, cy + r])
}

/// `area(pts)` (signed; > 0 = counterclockwise).
pub fn area(pts: &[P]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get(crate::js::next_i(i, n))) else { continue };
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// Most scallops one cloud edge gets. Spec gap (safety): the JS has no cap (an edge 10^7 pt long with 4 pt scallops
/// is 2.5 million curves); here an edge stops at a million so a hostile file cannot exhaust memory.
const MAX_SCALLOPS: f64 = 1_000_000.0;

/// `cloudPath(pts, arc)`.
pub fn cloud_path(pts: &[P], arc: f64) -> Vec<Seg> {
    let mut ring: Vec<P> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        if i == 0 {
            ring.push(*p);
            continue;
        }
        if let Some(q) = pts.get(i.wrapping_sub(1))
            && hypot(p[0] - q[0], p[1] - q[1]) > 1e-6
        {
            ring.push(*p);
        }
    }
    if ring.len() > 2
        && let (Some(a), Some(z)) = (ring.first(), ring.last())
        && hypot(a[0] - z[0], a[1] - z[1]) < 1e-6
    {
        ring.pop();
    }
    if ring.len() < 2 {
        return Vec::new();
    }
    let ccw = area(&ring) >= 0.0;
    let Some(first) = ring.first() else { return Vec::new() };
    let mut d = vec![Seg::M(first[0], first[1])];
    let rn = ring.len();
    for i in 0..rn {
        let (Some(a), Some(b)) = (ring.get(i), ring.get(crate::js::next_i(i, rn))) else { continue };
        let len = hypot(b[0] - a[0], b[1] - a[1]);
        if len < 1e-6 {
            continue;
        }
        let n = max(1.0, round(len / arc));
        let (ux, uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
        let (nx, ny) = if ccw { (uy, -ux) } else { (-uy, ux) };
        let step = len / n;
        let h = step * 0.6667;
        let count = if n.is_finite() && n > 0.0 { min(n, MAX_SCALLOPS) } else { 0.0 };
        let mut j = 0.0f64;
        while j < count {
            let (sx, sy) = (a[0] + ux * step * j, a[1] + uy * step * j);
            let (ex, ey) = (sx + ux * step, sy + uy * step);
            d.push(Seg::C(
                sx + nx * h - ux * step * 0.1,
                sy + ny * h - uy * step * 0.1,
                ex + nx * h + ux * step * 0.1,
                ey + ny * h + uy * step * 0.1,
                ex,
                ey,
            ));
            j += 1.0;
        }
    }
    d.push(Seg::Z);
    d
}

/// `arrowHead(from, tip, w)` → (head, base).
pub fn arrow_head(from: P, tip: P, w: f64) -> (Vec<Seg>, P) {
    let l0 = hypot(tip[0] - from[0], tip[1] - from[1]);
    let len = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
    let (ux, uy) = ((tip[0] - from[0]) / len, (tip[1] - from[1]) / len);
    let hl = min(len * 0.6, max(7.0, w * 4.0 + 4.0));
    let hw = hl * 0.38;
    let (bx, by) = (tip[0] - ux * hl, tip[1] - uy * hl);
    (
        vec![Seg::M(tip[0], tip[1]), Seg::L(bx - uy * hw, by + ux * hw), Seg::L(bx + uy * hw, by - ux * hw), Seg::Z],
        [bx + ux * hl * 0.15, by + uy * hl * 0.15],
    )
}

/// `calloutAttach(rect, knee)`.
pub fn callout_attach(r: [f64; 4], knee: P) -> P {
    let [x0, y0, x1, y1] = r;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let cands = [[x0, cy], [x1, cy], [cx, y0], [cx, y1]];
    let mut best = cands[0];
    let mut bd = f64::INFINITY;
    for c in cands {
        let dd = hypot(c[0] - knee[0], c[1] - knee[1]);
        if dd < bd {
            bd = dd;
            best = c;
        }
    }
    best
}

fn fill_truthy(f: &Option<String>) -> bool {
    f.as_ref().is_some_and(|s| !s.is_empty())
}

fn fmt_date(iso: &str) -> String {
    let t = parse_date(iso);
    if t.is_nan() { String::new() } else { short_date(t) }
}

/// `stampSub(m)`: the stamp's second line.
pub fn stamp_sub(m: &Markup) -> String {
    let date = fmt_date(&m.created);
    if !m.author.is_empty() {
        if !date.is_empty() {
            t("stamp.by_date", &[("author", Fill::from(&m.author)), ("date", Fill::S(date))])
        } else {
            t("stamp.by", &[("author", Fill::from(&m.author))])
        }
    } else {
        date
    }
}

/// `centroid(pts)`.
pub fn centroid(pts: &[P]) -> P {
    let n = pts.len();
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get(crate::js::next_i(i, n))) else { continue };
        let k = p[0] * q[1] - q[0] * p[1];
        a += k;
        cx += (p[0] + q[0]) * k;
        cy += (p[1] + q[1]) * k;
    }
    if a.abs() < 1e-9 {
        #[allow(clippy::cast_precision_loss)]
        let nf = n as f64;
        let sx = pts.iter().fold(0.0, |t, p| t + p[0]);
        let sy = pts.iter().fold(0.0, |t, p| t + p[1]);
        return [sx / nf, sy / nf];
    }
    [cx / (3.0 * a), cy / (3.0 * a)]
}

/// `labelItems(c, lines, size, rot, color, alpha)`.
pub fn label_items(c: P, lines: Vec<String>, size: f64, rot: f64, color: &str, alpha: f64) -> [Item; 2] {
    let w = lines.iter().map(|l| text_width(l, size, false)).fold(f64::NEG_INFINITY, max) + 6.0;
    #[allow(clippy::cast_precision_loss)]
    let h = size * (1.0 + (lines.len() as f64 - 1.0) * LINE_H) + size * 0.4;
    let side = ((rot % 180.0) + 180.0) % 180.0 == 90.0;
    let b = if side {
        [c[0] - h / 2.0, c[1] - w / 2.0, c[0] + h / 2.0, c[1] + w / 2.0]
    } else {
        [c[0] - w / 2.0, c[1] - h / 2.0, c[0] + w / 2.0, c[1] + h / 2.0]
    };
    [
        Item::Path(PathItem {
            d: rect(b),
            stroke: None,
            fill: Some("paper".into()),
            w: 0.0,
            alpha: 1.0,
            falpha: 0.92,
            cap: "butt",
            join: "miter",
            blend: None,
            dash: None,
            rule: None,
            nopick: false,
            badge: false,
        }),
        Item::Text(TextItem {
            lines,
            bbox: b,
            size,
            bold: false,
            color: color.to_string(),
            rot,
            align: "center",
            valign: "middle",
            pad: 0.0,
            lh: LINE_H,
            alpha,
            badge: false,
        }),
    ]
}

#[allow(clippy::too_many_arguments)]
fn path(
    d: Vec<Seg>,
    stroke: Option<&str>,
    fill: Option<&str>,
    w: f64,
    alpha: f64,
    falpha: f64,
    cap: &'static str,
    join: &'static str,
) -> PathItem {
    PathItem {
        d,
        stroke: stroke.map(str::to_string),
        fill: fill.map(str::to_string),
        w,
        alpha,
        falpha,
        cap,
        join,
        blend: None,
        dash: None,
        rule: None,
        nopick: false,
        badge: false,
    }
}

/// `stroke(d, s)` with the round caps and joins (callers change cap / join / blend / dash after).
fn stroke(d: Vec<Seg>, color: &str, width: f64, opacity: f64) -> PathItem {
    path(d, Some(color), None, max(0.1, width), opacity, 1.0, "round", "round")
}

/// `shapes(m, env)`: the display list of one markup.
pub fn shapes(m: &Markup, env: &ShapeEnv) -> Vec<Item> {
    let s = &m.style;
    let mut out: Vec<Item> = Vec::new();
    let pts = m.pts();
    let rot = rot_of(m.rot);
    let op = s.opacity;
    let fill_item = |d: Vec<Seg>| -> Option<PathItem> {
        if fill_truthy(&s.fill) {
            Some(path(d, None, s.fill.as_deref(), 0.0, 1.0, s.fill_opacity * s.opacity, "butt", "miter"))
        } else {
            None
        }
    };
    let text_item = |b: [f64; 4], lines: Vec<String>, size: f64| TextItem {
        lines,
        bbox: b,
        size,
        bold: false,
        color: s.color.clone(),
        rot,
        align: "left",
        valign: "top",
        pad: 4.0,
        lh: LINE_H,
        alpha: op,
        badge: false,
    };
    let wrap_in = |b: [f64; 4], text: &str, size: f64, pad: f64| {
        let f = frame(rot, b);
        wrap_text(text, size, f.w - 2.0 * pad, false)
    };
    match m.kind.as_str() {
        "pen" | "highlight" => {
            if let Some(p0) = pts.first() {
                let line: Vec<P> = if pts.len() == 1 { vec![*p0, *p0] } else { pts.to_vec() };
                let mut it = stroke(poly(&line, false), &s.color, s.width, op);
                if m.kind == "highlight" {
                    it.blend = Some("multiply");
                    it.cap = "butt";
                }
                out.push(Item::Path(it));
            }
        }
        "line" | "polyline" => out.push(Item::Path(stroke(poly(pts, false), &s.color, s.width, op))),
        "legend" => {
            let Some(r) = m.rect else { return badge(m, env, out) };
            let empty = Vec::new();
            let rows = env.legend.as_ref().unwrap_or(&empty);
            out.push(Item::Path(path(
                rect(r),
                if s.width > 0.0 { Some(&s.color) } else { None },
                Some(if fill_truthy(&s.fill) { s.fill.as_deref().unwrap_or("#ffffff") } else { "#ffffff" }),
                max(0.1, s.width),
                op,
                s.fill_opacity,
                "butt",
                "miter",
            )));
            let f = frame(rot, r);
            let size = s.font_size;
            let lh = size * 1.55;
            let pad = size * 0.8;
            let at = |x: f64, y: f64| [f.o[0] + f.u[0] * x + f.v[0] * y, f.o[1] + f.u[1] * x + f.v[1] * y];
            let bx = |x0: f64, y0: f64, x1: f64, y1: f64| {
                let (a, b) = (at(x0, y0), at(x1, y1));
                [min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1])]
            };
            let title = if m.label.is_empty() { t0("legend.title") } else { m.label.clone() };
            let mut ti = text_item(bx(pad, pad * 0.6, f.w - pad, pad * 0.6 + size * 1.4), vec![title], size * 1.15);
            ti.bold = true;
            ti.pad = 0.0;
            out.push(Item::Text(ti));
            let fit = max(0.0, ((f.h - pad * 1.6 - size * 1.6) / lh).floor());
            #[allow(clippy::cast_precision_loss)]
            let nrows = rows.len() as f64;
            let shown: &[crate::shapes::LegendRow] = if nrows > fit {
                let k = max(0.0, fit - 1.0);
                let k = crate::js::f2usize(k);
                rows.get(..k.min(rows.len())).unwrap_or(&[])
            } else {
                rows
            };
            for (i, row) in shown.iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let y = pad * 0.6 + size * 1.8 + i as f64 * lh;
                let sw = bx(pad, y + lh * 0.2, pad + size * 1.6, y + lh * 0.75);
                out.push(Item::Path(path(rect(sw), Some(&row.color), Some(&row.color), 0.6, 1.0, 0.35, "butt", "miter")));
                let mut a = text_item(bx(pad + size * 2.2, y, f.w - pad - size * 3.0, y + lh), vec![row.label.clone()], size);
                a.valign = "middle";
                a.pad = 0.0;
                out.push(Item::Text(a));
                let mut b = text_item(bx(f.w - pad - size * 3.0, y, f.w - pad, y + lh), vec![ns(row.n)], size);
                b.bold = true;
                b.align = "right";
                b.valign = "middle";
                b.pad = 0.0;
                out.push(Item::Text(b));
            }
            #[allow(clippy::cast_precision_loss)]
            let sl = shown.len() as f64;
            if shown.len() < rows.len() {
                let more = t("legend.more", &[("n", Fill::N(nrows - sl))]);
                let mut a = text_item(
                    bx(pad, pad * 0.6 + size * 1.8 + sl * lh, f.w - pad, pad * 0.6 + size * 1.8 + (sl + 1.0) * lh),
                    vec![more],
                    size,
                );
                a.valign = "middle";
                a.pad = 0.0;
                out.push(Item::Text(a));
            }
            if rows.is_empty() {
                let mut a =
                    text_item(bx(pad, pad * 0.6 + size * 1.8, f.w - pad, pad * 0.6 + size * 1.8 + lh), vec![t0("legend.empty")], size);
                a.valign = "middle";
                a.pad = 0.0;
                out.push(Item::Text(a));
            }
        }
        "space" => {
            if pts.len() >= 2 {
                let d = poly(pts, true);
                if let Some(mut f) = fill_item(d.clone()) {
                    f.nopick = true;
                    out.push(Item::Path(f));
                }
                let mut st = stroke(d, &s.color, s.width, op);
                st.join = "miter";
                st.dash = Some(vec![6.0, 4.0]);
                out.push(Item::Path(st));
                if pts.len() >= 3 && !m.label.is_empty() {
                    out.extend(label_items(centroid(pts), vec![m.label.clone()], s.font_size, rot, &s.color, op));
                }
            }
        }
        "image" => {
            if let Some(r) = m.rect {
                out.push(Item::Image(ImageItem { bbox: r, src: m.img.clone(), rot, alpha: op }));
                if s.width > 0.0 {
                    let mut st = stroke(rect(r), &s.color, s.width, op);
                    st.join = "miter";
                    st.cap = "butt";
                    out.push(Item::Path(st));
                }
            }
        }
        "arc" => {
            let arc = if let [a, m3, b] = pts { arc_through(*a, *m3, *b) } else { None };
            match arc {
                None => {
                    let line: Vec<P> = if let [a, _, b] = pts { vec![*a, *b] } else { pts.to_vec() };
                    out.push(Item::Path(stroke(poly(&line, false), &s.color, s.width, op)));
                }
                Some(a) => out.push(Item::Path(stroke(bezier_path(&arc_beziers(&a)), &s.color, s.width, op))),
            }
        }
        "angle" => {
            if let [a, v, b, ..] = *pts {
                let mut st = stroke(poly(&[a, v, b], false), &s.color, s.width, op);
                st.join = "miter";
                out.push(Item::Path(st));
                let r = max(6.0, min(min(36.0, hypot(a[0] - v[0], a[1] - v[1]) * 0.3), hypot(b[0] - v[0], b[1] - v[1]) * 0.3));
                let ind = angle_arc(a, v, b, r);
                if ind.sweep.abs() > 1e-6 {
                    out.push(Item::Path(stroke(bezier_path(&arc_beziers(&ind)), &s.color, max(0.5, s.width * 0.75), op)));
                }
                let label = format_angle(angle_at(a, v, b), 1);
                let mid = ind.a0 + ind.sweep / 2.0;
                let rr = r + s.font_size * 1.2;
                let (cx, cy) = (v[0] + mid.jcos() * rr, v[1] + mid.jsin() * rr);
                let size = s.font_size;
                let w = text_width(&label, size, false) + 6.0;
                let h = size * 1.3;
                let r180 = ((rot % 180.0) + 180.0) % 180.0;
                let bx = if r180 == 90.0 {
                    [cx - h / 2.0, cy - w / 2.0, cx + h / 2.0, cy + w / 2.0]
                } else {
                    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
                };
                out.push(Item::Path(path(rect(bx), None, Some("paper"), 0.0, 1.0, 1.0, "butt", "miter")));
                let mut ti = text_item(bx, vec![label], size);
                ti.align = "center";
                ti.valign = "middle";
                ti.pad = 0.0;
                out.push(Item::Text(ti));
            } else {
                out.push(Item::Path(stroke(poly(pts, false), &s.color, s.width, op)));
            }
        }
        "arrow" => {
            if let [p0, p1, ..] = *pts {
                let (head, base) = arrow_head(p0, p1, s.width);
                let mut st = stroke(poly(&[p0, base], false), &s.color, s.width, op);
                st.cap = "butt";
                out.push(Item::Path(st));
                out.push(Item::Path(path(head, Some(&s.color), Some(&s.color), max(0.1, s.width * 0.5), op, op, "butt", "miter")));
            }
        }
        "rect" | "ellipse" => {
            let r = m.rect.unwrap_or([f64::NAN; 4]);
            let d = if m.kind == "rect" { rect(r) } else { ellipse(r) };
            if let Some(f) = fill_item(d.clone()) {
                out.push(Item::Path(f));
            }
            if s.width > 0.0 {
                let mut st = stroke(d, &s.color, s.width, op);
                st.join = "miter";
                st.cap = "butt";
                out.push(Item::Path(st));
            }
        }
        "polygon" => {
            let d = poly(pts, true);
            if let Some(f) = fill_item(d.clone()) {
                out.push(Item::Path(f));
            }
            out.push(Item::Path(stroke(d, &s.color, s.width, op)));
        }
        "cloud" | "cloudrect" => {
            let ring: Vec<P> = if m.kind == "cloudrect" {
                let r = m.rect.unwrap_or([f64::NAN; 4]);
                vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
            } else {
                pts.to_vec()
            };
            // Math.max(4, s.arc || s.width * 6 + 8)
            let a = match s.arc {
                Some(a) if a != 0.0 && !a.is_nan() => a,
                _ => s.width * 6.0 + 8.0,
            };
            let d = cloud_path(&ring, max(4.0, a));
            if !d.is_empty() {
                if let Some(f) = fill_item(d.clone()) {
                    out.push(Item::Path(f));
                }
                out.push(Item::Path(stroke(d, &s.color, s.width, op)));
            }
        }
        "callout" => {
            let Some(r) = m.rect else { return badge(m, env, out) };
            if let Some(&tip) = pts.first() {
                let knee = pts.get(1).copied().unwrap_or(tip);
                let at = callout_attach(r, knee);
                let (head, base) = arrow_head(knee, tip, s.width);
                let lead: Vec<P> = if hypot(knee[0] - tip[0], knee[1] - tip[1]) > 1e-6 { vec![base, knee, at] } else { vec![base, at] };
                let mut st = stroke(poly(&lead, false), &s.color, s.width, op);
                st.join = "miter";
                out.push(Item::Path(st));
                out.push(Item::Path(path(head, Some(&s.color), Some(&s.color), max(0.1, s.width * 0.5), op, op, "butt", "miter")));
            }
            let bx = rect(r);
            let has_fill = fill_truthy(&s.fill);
            out.push(Item::Path(path(
                bx.clone(),
                None,
                Some(if has_fill { s.fill.as_deref().unwrap_or("paper") } else { "paper" }),
                0.0,
                1.0,
                if has_fill { s.fill_opacity * s.opacity } else { 0.9 },
                "butt",
                "miter",
            )));
            if s.width > 0.0 {
                let mut st = stroke(bx, &s.color, s.width, op);
                st.join = "miter";
                out.push(Item::Path(st));
            }
            out.push(Item::Text(text_item(r, wrap_in(r, &m.text, s.font_size, 4.0), s.font_size)));
        }
        "text" => {
            let Some(r) = m.rect else { return badge(m, env, out) };
            let d = rect(r);
            if let Some(f) = fill_item(d.clone()) {
                out.push(Item::Path(f));
            }
            if s.width > 0.0 {
                let mut st = stroke(d, &s.color, s.width, op);
                st.join = "miter";
                out.push(Item::Path(st));
            }
            let mut ti = text_item(r, wrap_in(r, &m.text, s.font_size, 3.0), s.font_size);
            ti.pad = 3.0;
            out.push(Item::Text(ti));
        }
        "note" => {
            let [x, y] = pts.first().copied().unwrap_or([0.0, 0.0]);
            let r = [x, y - NOTE_SIZE, x + NOTE_SIZE, y];
            let fold = NOTE_SIZE * 0.3;
            out.push(Item::Path(path(
                vec![
                    Seg::M(r[0], r[1]),
                    Seg::L(r[2] - fold, r[1]),
                    Seg::L(r[2], r[1] + fold),
                    Seg::L(r[2], r[3]),
                    Seg::L(r[0], r[3]),
                    Seg::Z,
                ],
                Some("#5c4a00"),
                Some(&s.color),
                0.75,
                1.0,
                1.0,
                "butt",
                "miter",
            )));
            let mut lines = Vec::new();
            for i in 0..3 {
                let fi = f64::from(i);
                let yy = r[3] - NOTE_SIZE * (0.3 + fi * 0.2);
                lines.push(Seg::M(r[0] + NOTE_SIZE * 0.2, yy));
                lines.push(Seg::L(r[2] - NOTE_SIZE * (if i == 2 { 0.45 } else { 0.2 }), yy));
            }
            out.push(Item::Path(path(lines, Some("#5c4a00"), None, 0.9, 1.0, 1.0, "butt", "miter")));
        }
        "stamp" => {
            let Some(rr) = m.rect else { return badge(m, env, out) };
            let [x0, y0, x1, y1] = rr;
            let f = frame(rot, rr);
            let r = min(f.w, f.h) * 0.12;
            out.push(Item::Path(path(round_rect(rr, r), None, Some("paper"), 0.0, 1.0, 0.85 * op, "butt", "miter")));
            let mut st = stroke(round_rect(rr, r), &s.color, s.width, op);
            st.join = "miter";
            out.push(Item::Path(st));
            let ins = min(f.w, f.h) * 0.07 + s.width;
            if x1 - x0 > ins * 4.0 && y1 - y0 > ins * 4.0 {
                let mut st =
                    stroke(round_rect([x0 + ins, y0 + ins, x1 - ins, y1 - ins], max(0.0, r - ins)), &s.color, max(0.5, s.width * 0.4), op);
                st.join = "miter";
                out.push(Item::Path(st));
            }
            let label = upper(if m.label.is_empty() { "Stamp" } else { &m.label });
            let sub = stamp_sub(m);
            let pad = ins + f.h * 0.06;
            let avail = f.w - 2.0 * pad;
            let big = max(2.0, min(f.h * if sub.is_empty() { 0.55 } else { 0.38 }, avail / max(1e-6, text_width(&label, 1.0, true))));
            let small = if sub.is_empty() { 0.0 } else { max(2.0, min(f.h * 0.17, avail / max(1e-6, text_width(&sub, 1.0, false)))) };
            if !sub.is_empty() {
                let gap = f.h * 0.06;
                let total = big + gap + small;
                let top_pad = (f.h - total) / 2.0 - big * 0.1;
                let mut a = text_item(rr, vec![label], big);
                a.bold = true;
                a.align = "center";
                a.pad = top_pad;
                out.push(Item::Text(a));
                let mut b = text_item(rr, vec![sub], small);
                b.align = "center";
                b.pad = top_pad + big + gap - small * 0.05;
                out.push(Item::Text(b));
            } else {
                let mut a = text_item(rr, vec![label], big);
                a.bold = true;
                a.align = "center";
                a.valign = "middle";
                a.pad = pad;
                out.push(Item::Text(a));
            }
        }
        "area" => {
            if pts.len() >= 2 {
                let mut d = poly(pts, true);
                if let Some(h) = &m.holes {
                    for ring in h {
                        d.extend(poly(ring, true));
                    }
                }
                if let Some(mut f) = fill_item(d.clone()) {
                    f.rule = Some("evenodd");
                    out.push(Item::Path(f));
                }
                let mut st = stroke(d, &s.color, s.width, op);
                st.join = "miter";
                out.push(Item::Path(st));
                if pts.len() >= 3 {
                    let holes: &[Vec<P>] = m.holes.as_deref().unwrap_or(&[]);
                    let (av, pv) = measure_area(pts, env.cal.as_ref(), holes);
                    let lines = if env.cal.is_some() {
                        vec![av, t("area.perimeter", &[("v", Fill::S(pv))])]
                    } else {
                        vec![if m.text.is_empty() { t0("status.scale.none") } else { m.text.clone() }]
                    };
                    out.extend(label_items(centroid(pts), lines, s.font_size, rot, &s.color, op));
                }
            }
        }
        "count" => {
            let r = max(3.0, s.font_size * 0.8);
            for (i, p) in pts.iter().enumerate() {
                out.push(Item::Path(path(
                    circle(p[0], p[1], r),
                    Some("#ffffff"),
                    Some(&s.color),
                    max(0.5, r * 0.14),
                    op,
                    op,
                    "butt",
                    "miter",
                )));
                let label = (i.saturating_add(1)).to_string();
                let size = min(r * 1.15, (r * 1.55) / max(1e-6, text_width(&label, 1.0, true)));
                out.push(Item::Text(TextItem {
                    lines: vec![label],
                    bbox: [p[0] - r, p[1] - r, p[0] + r, p[1] + r],
                    size,
                    bold: true,
                    color: "#ffffff".into(),
                    rot,
                    align: "center",
                    valign: "middle",
                    pad: 0.0,
                    lh: LINE_H,
                    alpha: op,
                    badge: false,
                }));
            }
            if let Some(p0) = pts.first() {
                let f = frame(rot, [0.0, 0.0, 1.0, 1.0]);
                let group = if m.label.is_empty() { t0("count.default") } else { m.label.clone() };
                let tag = t("count.tag", &[("group", Fill::S(group)), ("n", Fill::from(pts.len()))]);
                let off = r * 1.4 + (text_width(&tag, s.font_size, false) + 6.0) / 2.0;
                let c = [p0[0] + f.u[0] * off - f.v[0] * r * 1.4, p0[1] + f.u[1] * off - f.v[1] * r * 1.4];
                out.extend(label_items(c, vec![tag], s.font_size, rot, &s.color, op));
            }
        }
        "measure" => {
            if let [a, b, ..] = *pts {
                let len = hypot(b[0] - a[0], b[1] - a[1]);
                let lz = len == 0.0 || len.is_nan();
                let ux = if lz { 1.0 } else { (b[0] - a[0]) / len };
                let uy = if lz { 0.0 } else { (b[1] - a[1]) / len };
                let tk = max(4.0, s.width * 3.0 + 3.0);
                let mut d = vec![Seg::M(a[0], a[1]), Seg::L(b[0], b[1])];
                for p in [a, b] {
                    d.push(Seg::M(p[0] - uy * tk, p[1] + ux * tk));
                    d.push(Seg::L(p[0] + uy * tk, p[1] - ux * tk));
                }
                for p in [a, b] {
                    let (dx, dy) = ((ux - uy) * tk * 0.7, (uy + ux) * tk * 0.7);
                    d.push(Seg::M(p[0] - dx, p[1] - dy));
                    d.push(Seg::L(p[0] + dx, p[1] + dy));
                }
                let mut st = stroke(d, &s.color, s.width, op);
                st.cap = "butt";
                out.push(Item::Path(st));
                let label = if env.cal.is_some() {
                    measure(len, env.cal.as_ref())
                } else if m.text.is_empty() {
                    t0("status.scale.none")
                } else {
                    m.text.clone()
                };
                let size = s.font_size;
                let w = text_width(&label, size, false) + 6.0;
                let h = size * 1.3;
                let (cx, cy) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
                let r180 = ((rot % 180.0) + 180.0) % 180.0;
                let bx = if r180 == 90.0 {
                    [cx - h / 2.0, cy - w / 2.0, cx + h / 2.0, cy + w / 2.0]
                } else {
                    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
                };
                out.push(Item::Path(path(rect(bx), None, Some("paper"), 0.0, 1.0, 1.0, "butt", "miter")));
                let mut ti = text_item(bx, vec![label], size);
                ti.align = "center";
                ti.valign = "middle";
                ti.pad = 0.0;
                out.push(Item::Text(ti));
            }
        }
        _ => {}
    }
    badge(m, env, out)
}

fn bezier_path(bz: &[[P; 4]]) -> Vec<Seg> {
    let mut d = Vec::with_capacity(bz.len().saturating_add(1));
    if let Some(b0) = bz.first() {
        d.push(Seg::M(b0[0][0], b0[0][1]));
    }
    for [_, c1, c2, p3] in bz {
        d.push(Seg::C(c1[0], c1[1], c2[0], c2[1], p3[0], p3[1]));
    }
    d
}

/// The punch badge (`if (m.punch && env.badge)`), then the list.
fn badge(m: &Markup, env: &ShapeEnv, mut out: Vec<Item>) -> Vec<Item> {
    if m.punch.is_some()
        && env.badge != 0.0
        && !env.badge.is_nan()
        && let Some(b) = list_bounds(&out)
    {
        let r = env.badge;
        let rot = rot_of(m.rot);
        let f = frame(rot, b);
        let cx = f.o[0] - (f.u[0] + f.v[0]) * r * 0.2;
        let cy = f.o[1] - (f.u[1] + f.v[1]) * r * 0.2;
        let mut c = path(circle(cx, cy, r), Some("#ffffff"), Some("#17202b"), r * 0.12, 1.0, 1.0, "butt", "miter");
        c.badge = true;
        out.push(Item::Path(c));
        let label = m.punch.as_ref().map_or_else(String::new, |p| ns(p.n));
        let size = min(r * 1.1, (r * 1.6) / max(1e-6, text_width(&label, 1.0, true)));
        out.push(Item::Text(TextItem {
            lines: vec![label],
            bbox: [cx - r, cy - r, cx + r, cy + r],
            size,
            bold: true,
            color: "#ffffff".into(),
            rot,
            align: "center",
            valign: "middle",
            pad: 0.0,
            lh: LINE_H,
            alpha: 1.0,
            badge: true,
        }));
    }
    out
}

// ---- flattening, bounds, hit-testing ----

/// One flattened sub-path.
#[derive(Debug, Clone, PartialEq)]
pub struct Sub {
    pub pts: Vec<P>,
    pub closed: bool,
}

/// `flatten(d)`: sub-paths as polylines (Béziers split into 12).
pub fn flatten(d: &[Seg]) -> Vec<Sub> {
    let mut subs: Vec<Sub> = Vec::new();
    let mut cur: Option<usize> = None;
    let (mut x, mut y) = (0.0, 0.0);
    for c in d {
        match *c {
            Seg::M(a, b) => {
                subs.push(Sub { pts: vec![[a, b]], closed: false });
                cur = Some(subs.len().saturating_sub(1));
                x = a;
                y = b;
            }
            Seg::L(a, b) => {
                let k = match cur {
                    Some(k) => k,
                    None => {
                        subs.push(Sub { pts: vec![[x, y]], closed: false });
                        let k = subs.len().saturating_sub(1);
                        cur = Some(k);
                        k
                    }
                };
                if let Some(s) = subs.get_mut(k) {
                    s.pts.push([a, b]);
                }
                x = a;
                y = b;
            }
            Seg::C(x1, y1, x2, y2, x3, y3) => {
                let k = match cur {
                    Some(k) => k,
                    None => {
                        subs.push(Sub { pts: vec![[x, y]], closed: false });
                        let k = subs.len().saturating_sub(1);
                        cur = Some(k);
                        k
                    }
                };
                if let Some(s) = subs.get_mut(k) {
                    for i in 1..=12 {
                        let t = f64::from(i) / 12.0;
                        let mt = 1.0 - t;
                        s.pts.push([
                            mt * mt * mt * x + 3.0 * mt * mt * t * x1 + 3.0 * mt * t * t * x2 + t * t * t * x3,
                            mt * mt * mt * y + 3.0 * mt * mt * t * y1 + 3.0 * mt * t * t * y2 + t * t * t * y3,
                        ]);
                    }
                }
                x = x3;
                y = y3;
            }
            Seg::Z => {
                if let Some(s) = cur.and_then(|k| subs.get_mut(k)) {
                    s.closed = true;
                    if let Some(p) = s.pts.first() {
                        x = p[0];
                        y = p[1];
                    }
                }
            }
        }
    }
    subs
}

/// `listBounds(list)` including half the stroke width (None when empty).
pub fn list_bounds(list: &[Item]) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for it in list {
        match it {
            Item::Path(p) => {
                let h = if p.stroke.as_ref().is_some_and(|s| !s.is_empty()) { p.w / 2.0 } else { 0.0 };
                for sp in flatten(&p.d) {
                    for q in &sp.pts {
                        let (x, y) = (q[0], q[1]);
                        if x - h < x0 {
                            x0 = x - h;
                        }
                        if y - h < y0 {
                            y0 = y - h;
                        }
                        if x + h > x1 {
                            x1 = x + h;
                        }
                        if y + h > y1 {
                            y1 = y + h;
                        }
                    }
                }
            }
            Item::Text(TextItem { bbox, .. }) | Item::Image(ImageItem { bbox, .. }) => {
                x0 = min(x0, bbox[0]);
                y0 = min(y0, bbox[1]);
                x1 = max(x1, bbox[2]);
                y1 = max(y1, bbox[3]);
            }
        }
    }
    if x0.is_finite() { Some([x0, y0, x1, y1]) } else { None }
}

/// `distToSeg(p, a, b)`.
pub fn dist_to_seg(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx * dx + dy * dy;
    let t = if l != 0.0 && !l.is_nan() { ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l } else { 0.0 };
    let t = max(0.0, min(1.0, t));
    hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))
}

/// `insidePoly(p, pts)`.
pub fn inside_poly(p: P, pts: &[P]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(j)) {
            let (xi, yi, xj, yj) = (a[0], a[1], b[0], b[1]);
            if (yi > p[1]) != (yj > p[1]) && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// `listDistance(list, p)`: distance from p to what a display list draws (0 inside fills and text boxes).
pub fn list_distance(list: &[Item], p: P) -> f64 {
    let mut best = f64::INFINITY;
    for it in list {
        match it {
            Item::Text(TextItem { bbox, .. }) | Item::Image(ImageItem { bbox, .. }) => {
                if p[0] >= bbox[0] && p[0] <= bbox[2] && p[1] >= bbox[1] && p[1] <= bbox[3] {
                    return 0.0;
                }
            }
            Item::Path(pi) => {
                let stroked = pi.stroke.as_ref().is_some_and(|s| !s.is_empty());
                let filled = pi.fill.as_ref().is_some_and(|s| !s.is_empty());
                for sp in flatten(&pi.d) {
                    let mut ring = sp.pts.clone();
                    if sp.closed
                        && let Some(f) = sp.pts.first()
                    {
                        ring.push(*f);
                    }
                    if filled && pi.falpha > 0.02 && !pi.nopick && sp.closed && inside_poly(p, &sp.pts) {
                        return 0.0;
                    }
                    for w in ring.windows(2) {
                        if let [a, b] = w {
                            let dd = dist_to_seg(p, *a, *b) - if stroked { pi.w / 2.0 } else { 0.0 };
                            if dd < best {
                                best = dd;
                            }
                        }
                    }
                    if ring.len() == 1
                        && let Some(q) = ring.first()
                    {
                        best = min(best, hypot(p[0] - q[0], p[1] - q[1]) - pi.w / 2.0);
                    }
                }
            }
        }
    }
    max(0.0, best)
}

// ---- JSON ----

fn write_color(o: &mut Out, c: &Option<String>) {
    match c {
        Some(s) => o.str(s),
        None => o.raw("null"),
    }
}

/// Write a path's `d`.
pub fn write_d(o: &mut Out, d: &[Seg]) {
    o.ch('[');
    for (i, c) in d.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        match *c {
            Seg::M(x, y) | Seg::L(x, y) => {
                o.raw(if matches!(c, Seg::M(..)) { "[\"M\"," } else { "[\"L\"," });
                o.num(x);
                o.ch(',');
                o.num(y);
                o.ch(']');
            }
            Seg::C(a, b, c2, d2, e, f) => {
                o.raw("[\"C\"");
                for v in [a, b, c2, d2, e, f] {
                    o.ch(',');
                    o.num(v);
                }
                o.ch(']');
            }
            Seg::Z => o.raw("[\"Z\"]"),
        }
    }
    o.ch(']');
}

/// Write one display item with the JS's keys.
pub fn write_item(o: &mut Out, it: &Item) {
    match it {
        Item::Path(p) => {
            o.raw("{\"k\":\"path\",\"d\":");
            write_d(o, &p.d);
            o.raw(",\"stroke\":");
            write_color(o, &p.stroke);
            o.raw(",\"fill\":");
            write_color(o, &p.fill);
            o.raw(",\"w\":");
            o.num(p.w);
            o.raw(",\"alpha\":");
            o.num(p.alpha);
            o.raw(",\"falpha\":");
            o.num(p.falpha);
            o.raw(",\"cap\":");
            o.str(p.cap);
            o.raw(",\"join\":");
            o.str(p.join);
            o.raw(",\"blend\":");
            match p.blend {
                Some(b) => o.str(b),
                None => o.raw("null"),
            }
            if let Some(d) = &p.dash {
                o.raw(",\"dash\":");
                o.nums(d);
            }
            if let Some(r) = p.rule {
                o.raw(",\"rule\":");
                o.str(r);
            }
            if p.nopick {
                o.raw(",\"nopick\":true");
            }
            if p.badge {
                o.raw(",\"badge\":true");
            }
            o.ch('}');
        }
        Item::Text(t) => {
            o.raw("{\"k\":\"text\",\"lines\":");
            o.strs(&t.lines);
            o.raw(",\"box\":");
            o.nums(&t.bbox);
            o.raw(",\"size\":");
            o.num(t.size);
            o.raw(",\"bold\":");
            o.bool(t.bold);
            o.raw(",\"color\":");
            o.str(&t.color);
            o.raw(",\"rot\":");
            o.num(t.rot);
            o.raw(",\"align\":");
            o.str(t.align);
            o.raw(",\"valign\":");
            o.str(t.valign);
            o.raw(",\"pad\":");
            o.num(t.pad);
            o.raw(",\"lh\":");
            o.num(t.lh);
            o.raw(",\"alpha\":");
            o.num(t.alpha);
            if t.badge {
                o.raw(",\"badge\":true");
            }
            o.ch('}');
        }
        Item::Image(i) => {
            o.raw("{\"k\":\"image\",\"box\":");
            o.nums(&i.bbox);
            o.raw(",\"src\":");
            o.str(&i.src);
            o.raw(",\"rot\":");
            o.num(i.rot);
            o.raw(",\"alpha\":");
            o.num(i.alpha);
            o.ch('}');
        }
    }
}

/// Write a display list.
pub fn write_list(o: &mut Out, list: &[Item]) {
    o.ch('[');
    for (i, it) in list.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        write_item(o, it);
    }
    o.ch(']');
}
