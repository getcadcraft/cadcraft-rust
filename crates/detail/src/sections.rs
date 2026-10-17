//! shapes/sections.js — the canonical structural steel section table (0.5 MQ.3) and its geometry: the published rows
//! ([`crate::section_rows`], copied as written) turned into records in millimetres, `outline` (section loops in the
//! (u, v) plane, centred on their bounding box), `exactArea`, `bbox`, `shoelace`, `loopLength`. Same floating-point
//! order as the JS, operation for operation.

use crate::js::{cos, hypot, max, min, sin};
use crate::section_rows as R;
use std::f64::consts::PI;

pub const STEEL_DENSITY: f64 = 7850.0;
pub const IN: f64 = 25.4;
pub const LBFT: f64 = 1.48816394;

/// A section loop: points [u, v].
pub type Loop = Vec<[f64; 2]>;

/// A section record or a parametric / custom profile: the JS object's fields (absent = `None`).
#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub name: String,
    pub family: String,
    pub kind: String,
    pub source: String,
    pub d: Option<f64>,
    pub bf: Option<f64>,
    pub tw: Option<f64>,
    pub tf: Option<f64>,
    pub r: Option<f64>,
    pub b: Option<f64>,
    pub t: Option<f64>,
    pub h: Option<f64>,
    pub ro: Option<f64>,
    pub ri: Option<f64>,
    pub dd: Option<f64>, // `D` (round sections)
    pub t_nom: Option<f64>,
    pub area_pub: Option<f64>,
    pub mass_pub: Option<f64>,
    /// `pub`: the row as published (units + values in the table's order).
    pub pub_units: Option<&'static str>,
    pub pub_vals: Vec<(&'static str, f64)>,
}

#[inline]
fn g(v: Option<f64>) -> f64 {
    v.unwrap_or(f64::NAN)
}

impl Shape {
    pub fn d(&self) -> f64 {
        g(self.d)
    }
    pub fn bf(&self) -> f64 {
        g(self.bf)
    }
    pub fn tw(&self) -> f64 {
        g(self.tw)
    }
    pub fn tf(&self) -> f64 {
        g(self.tf)
    }
    pub fn r(&self) -> f64 {
        g(self.r)
    }
    pub fn b(&self) -> f64 {
        g(self.b)
    }
    pub fn t(&self) -> f64 {
        g(self.t)
    }
    pub fn ro(&self) -> f64 {
        g(self.ro)
    }
    pub fn ri(&self) -> f64 {
        g(self.ri)
    }
    pub fn big_d(&self) -> f64 {
        g(self.dd)
    }
    /// `p.r || 0`.
    pub fn r_or0(&self) -> f64 {
        crate::js::or(self.r(), 0.0)
    }
    /// The record's dimension fields in the JS object's order (name → value), for JSON.
    pub fn dims(&self) -> Vec<(&'static str, f64)> {
        let mut out = Vec::new();
        for (k, v) in [
            ("d", self.d),
            ("bf", self.bf),
            ("tw", self.tw),
            ("tf", self.tf),
            ("r", self.r),
            ("b", self.b),
            ("h", self.h),
            ("t", self.t),
            ("tNom", self.t_nom),
            ("ro", self.ro),
            ("ri", self.ri),
            ("D", self.dd),
        ] {
            if let Some(v) = v {
                out.push((k, v));
            }
        }
        out
    }
}

/// `displayName(n)`: an X between a digit (or / .) and a digit prints lowercase.
pub fn display_name(n: &str) -> String {
    let cs: Vec<char> = n.chars().collect();
    let mut out = String::with_capacity(n.len());
    for (i, &c) in cs.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| cs.get(j)).copied();
        let next = cs.get(i.saturating_add(1)).copied();
        if c == 'X' && prev.is_some_and(|p| p.is_ascii_digit() || p == '/' || p == '.') && next.is_some_and(|x| x.is_ascii_digit()) {
            out.push('x');
        } else {
            out.push(c);
        }
    }
    out
}

fn rec(name: String, family: &str, kind: &str, source: &str) -> Shape {
    Shape { name, family: family.into(), kind: kind.into(), source: source.into(), ..Shape::default() }
}

fn build() -> Vec<Shape> {
    let mut out = Vec::with_capacity(1400);
    for (n, row) in R::AISC_W.iter().rev() {
        let [d, bf, tw, tf, k, a, w] = *row;
        let mut p = rec(display_name(n), "W", "I", "aisc");
        (p.d, p.bf, p.tw, p.tf, p.r) = (Some(d * IN), Some(bf * IN), Some(tw * IN), Some(tf * IN), Some((k - tf) * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("d", d), ("bf", bf), ("tw", tw), ("tf", tf), ("kdes", k), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::AISC_C.iter().rev() {
        let [d, bf, tw, tf, a, w] = *row;
        let mut p = rec(display_name(n), "C", "C", "aisc");
        (p.d, p.bf, p.tw, p.tf, p.r) = (Some(d * IN), Some(bf * IN), Some(tw * IN), Some(tf * IN), Some(0.0));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("d", d), ("bf", bf), ("tw", tw), ("tf", tf), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::AISC_L.iter().rev() {
        let [b, t, a, w] = *row;
        let mut p = rec(display_name(n), "L", "L", "aisc");
        (p.b, p.t) = (Some(b * IN), Some(t * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("b", b), ("t", t), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::AISC_RHS.iter().rev() {
        let [hh, bb, tn, t, a, w] = *row;
        let mut p = rec(display_name(n), "HSS", "RHS", "aisc");
        (p.h, p.b, p.t, p.t_nom, p.ro, p.ri) =
            (Some(hh * IN), Some(bb * IN), Some(t * IN), Some(tn * IN), Some(2.0 * t * IN), Some(t * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("H", hh), ("B", bb), ("tNom", tn), ("t", t), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::AISC_CHS.iter().rev() {
        let [dd, tn, t, a, w] = *row;
        let mut p = rec(display_name(n), "HSS", "CHS", "aisc");
        (p.dd, p.t, p.t_nom) = (Some(dd * IN), Some(t * IN), Some(tn * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("D", dd), ("tNom", tn), ("t", t), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::EN_I {
        let [h, b, tw, tf, r, a, gg] = *row;
        let fam: String = n.chars().take(3).collect();
        let mut p = rec((*n).to_string(), &fam, "I", "en");
        (p.d, p.bf, p.tw, p.tf, p.r) = (Some(h), Some(b), Some(tw), Some(tf), Some(r));
        (p.area_pub, p.mass_pub) = (Some(a * 100.0), Some(gg));
        p.pub_units = Some("mm");
        p.pub_vals = vec![("h", h), ("b", b), ("tw", tw), ("tf", tf), ("r", r), ("A", a), ("G", gg)];
        out.push(p);
    }
    for (n, row) in R::AISC_S.iter().rev() {
        let [d, bf, tw, tf, k, a, w] = *row;
        let mut p = rec(display_name(n), "S", "I", "aisc");
        (p.d, p.bf, p.tw, p.tf, p.r) = (Some(d * IN), Some(bf * IN), Some(tw * IN), Some(tf * IN), Some(0.0));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("d", d), ("bf", bf), ("tw", tw), ("tf", tf), ("kdes", k), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::AISC_L2.iter().rev() {
        let [long, short, t, a, w] = *row;
        let mut p = rec(display_name(n), "L", "L", "aisc");
        (p.b, p.h, p.t) = (Some(short * IN), Some(long * IN), Some(t * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("long", long), ("short", short), ("t", t), ("A", a), ("W", w)];
        out.push(p);
    }
    let mut pipes: Vec<&(&str, [f64; 5])> = R::AISC_PIPE.iter().collect();
    // by OD, then wall (a stable sort, as V8's)
    pipes.sort_by(|x, y| {
        let c = x.1[0] - y.1[0];
        crate::js::ord(if c == 0.0 || c.is_nan() { x.1[1] - y.1[1] } else { c })
    });
    for (n, row) in pipes {
        let [dd, tn, t, a, w] = *row;
        let mut p = rec((*n).to_string(), "Pipe", "CHS", "aisc");
        (p.dd, p.t, p.t_nom) = (Some(dd * IN), Some(t * IN), Some(tn * IN));
        (p.area_pub, p.mass_pub) = (Some(a * IN * IN), Some(w * LBFT));
        p.pub_units = Some("in");
        p.pub_vals = vec![("D", dd), ("tNom", tn), ("t", t), ("A", a), ("W", w)];
        out.push(p);
    }
    for (n, row) in R::EN_UPN {
        let [h, b, tw, tf, r1, r2, a, gg] = *row;
        let mut p = rec((*n).to_string(), "UPN", "C", "en");
        (p.d, p.bf, p.tw, p.tf, p.r) = (Some(h), Some(b), Some(tw), Some(tf), Some(0.0));
        (p.area_pub, p.mass_pub) = (Some(a * 100.0), Some(gg));
        p.pub_units = Some("mm");
        p.pub_vals = vec![("h", h), ("b", b), ("tw", tw), ("tf", tf), ("r1", r1), ("r2", r2), ("A", a), ("G", gg)];
        out.push(p);
    }
    out
}

/// `sectionRows()`: every section, in the table's order (built once).
pub fn section_rows() -> &'static [Shape] {
    static ROWS: std::sync::OnceLock<Vec<Shape>> = std::sync::OnceLock::new();
    ROWS.get_or_init(build)
}

/// `exactArea(p)` (mm², arcs exact).
pub fn exact_area(p: &Shape) -> f64 {
    match p.kind.as_str() {
        "I" => 2.0 * p.bf() * p.tf() + (p.d() - 2.0 * p.tf()) * p.tw() + (4.0 - PI) * p.r() * p.r(),
        "C" => 2.0 * p.bf() * p.tf() + (p.d() - 2.0 * p.tf()) * p.tw(),
        "L" => p.t() * (p.b() + p.h.unwrap_or(p.b()) - p.t()),
        "RHS" => 2.0 * p.t() * (g(p.h) + p.b() - 2.0 * p.t()) - (4.0 - PI) * (p.ro() * p.ro() - p.ri() * p.ri()),
        "CHS" => PI * (p.big_d() - p.t()) * p.t(),
        "PL" => p.t() * p.b(),
        _ => 0.0,
    }
}

fn arc(pts: &mut Loop, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, n: usize) {
    for i in 1..n {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        pts.push([cx + r * cos(a), cy + r * sin(a)]);
    }
}

/// `outline(p, seg)`: loops of [u, v], the first outer (CCW), the others holes (CW), centred on the bounding box.
pub fn outline(p: &Shape, seg: usize) -> Vec<Loop> {
    let hh = PI / 2.0;
    match p.kind.as_str() {
        "I" => {
            let (d, bf, tw, tf, r) = (p.d(), p.bf(), p.tw(), p.tf(), p.r());
            let (a, w, h) = (bf / 2.0, tw / 2.0, d / 2.0);
            let mut pts: Loop = vec![[-a, -h], [a, -h], [a, -h + tf]];
            if r > 0.0 {
                pts.push([w + r, -h + tf]);
                arc(&mut pts, w + r, -h + tf + r, r, -hh, -2.0 * hh, seg);
                pts.push([w, -h + tf + r]);
                pts.push([w, h - tf - r]);
                arc(&mut pts, w + r, h - tf - r, r, 2.0 * hh, hh, seg);
                pts.push([w + r, h - tf]);
            } else {
                pts.push([w, -h + tf]);
                pts.push([w, h - tf]);
            }
            pts.extend([[a, h - tf], [a, h], [-a, h], [-a, h - tf]]);
            if r > 0.0 {
                pts.push([-w - r, h - tf]);
                arc(&mut pts, -w - r, h - tf - r, r, hh, 0.0, seg);
                pts.push([-w, h - tf - r]);
                pts.push([-w, -h + tf + r]);
                arc(&mut pts, -w - r, -h + tf + r, r, 0.0, -hh, seg);
                pts.push([-w - r, -h + tf]);
            } else {
                pts.push([-w, h - tf]);
                pts.push([-w, -h + tf]);
            }
            pts.push([-a, -h + tf]);
            vec![pts]
        }
        "C" => {
            let (d, bf, tw, tf) = (p.d(), p.bf(), p.tw(), p.tf());
            let (a, h) = (bf / 2.0, d / 2.0);
            vec![vec![[-a, -h], [a, -h], [a, -h + tf], [-a + tw, -h + tf], [-a + tw, h - tf], [a, h - tf], [a, h], [-a, h]]]
        }
        "L" => {
            let (b, t) = (p.b(), p.t());
            let (a, c) = (b / 2.0, p.h.unwrap_or(b) / 2.0);
            vec![vec![[-a, -c], [a, -c], [a, -c + t], [-a + t, -c + t], [-a + t, c], [-a, c]]]
        }
        "RHS" => {
            let (h, b, t, ro, ri) = (g(p.h), p.b(), p.t(), p.ro(), p.ri());
            vec![rounded_rect(b, h, ro, seg, false), rounded_rect(b - 2.0 * t, h - 2.0 * t, ri, seg, true)]
        }
        "CHS" => {
            let n = seg.saturating_mul(8);
            let rr = p.big_d() / 2.0;
            let r = rr - p.t();
            let (mut o, mut i) = (Vec::with_capacity(n), Vec::with_capacity(n));
            for k in 0..n {
                let a = 2.0 * PI * k as f64 / n as f64;
                o.push([rr * cos(a), rr * sin(a)]);
                i.push([r * cos(-a), r * sin(-a)]);
            }
            vec![o, i]
        }
        "PL" => {
            let (a, c) = (p.b() / 2.0, p.t() / 2.0);
            vec![vec![[-a, -c], [a, -c], [a, c], [-a, c]]]
        }
        _ => vec![vec![]],
    }
}

fn rounded_rect(w: f64, h: f64, r: f64, seg: usize, cw: bool) -> Loop {
    let (a, b) = (w / 2.0, h / 2.0);
    let mut pts = Vec::with_capacity(seg.saturating_add(1).saturating_mul(4));
    let corners = [[a - r, -b + r, -PI / 2.0], [a - r, b - r, 0.0], [-a + r, b - r, PI / 2.0], [-a + r, -b + r, PI]];
    for [cx, cy, a0] in corners {
        for i in 0..=seg {
            let t = a0 + (PI / 2.0) * i as f64 / seg as f64;
            pts.push([cx + r * cos(t), cy + r * sin(t)]);
        }
    }
    if cw {
        pts.reverse();
    }
    pts
}

/// `bbox(loop)` → {x0, y0, x1, y1, w, h}.
#[derive(Clone, Copy, Debug)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub w: f64,
    pub h: f64,
}
pub fn bbox(lp: &[[f64; 2]]) -> BBox {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &[x, y] in lp {
        x0 = min(x0, x);
        y0 = min(y0, y);
        x1 = max(x1, x);
        y1 = max(y1, y);
    }
    BBox { x0, y0, x1, y1, w: x1 - x0, h: y1 - y0 }
}
/// `shoelace(loop)` (signed area, CCW positive).
pub fn shoelace(lp: &[[f64; 2]]) -> f64 {
    let n = lp.len();
    let mut s = 0.0;
    for i in 0..n {
        let (Some(&[x1, y1]), Some(&[x2, y2])) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
        s += x1 * y2 - x2 * y1;
    }
    s / 2.0
}
/// `loopLength(loop)` (perimeter, closed).
pub fn loop_length(lp: &[[f64; 2]]) -> f64 {
    let n = lp.len();
    let mut s = 0.0;
    for i in 0..n {
        let (Some(&[x1, y1]), Some(&[x2, y2])) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
        s += hypot(x2 - x1, y2 - y1);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table() {
        let rows = section_rows();
        assert_eq!(rows.len(), 1335);
        assert_eq!(display_name("HSS4X4X1/4"), "HSS4x4x1/4");
        assert_eq!(display_name("L12X12X1-3/8"), "L12x12x1-3/8");
        let w = rows.iter().find(|p| p.name == "W16x26").map(|p| p.d());
        assert_eq!(w, Some(15.7 * IN));
    }
}
