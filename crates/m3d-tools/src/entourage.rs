//! Entourage (lib/entourage/entourage.js, one for one; 0.5 RC.11 (e)): trees, shrubs, people and cars drawn from
//! numbers (seeded shapes, CC0 in 0.5), each a list of one-colour triangle parts in millimetres, Z up, the front facing
//! −Y, the insertion point the middle of the item at the ground. Numbers as the JS has them: fdlibm `Math.sin` /
//! `cos`, V8's `Math.hypot`, `Math.round` to 0.1 mm, mulberry32. Only the parts are ported (the starter library builds
//! them); the library item's plan symbol and size are LibraryCraft's.

use std::collections::HashMap;
use std::f64::consts::PI;

use cc_m3d::js::{hypot, or, round};
use cc_m3d::jsmath::JsMath;

use crate::matlib::Rng;

/// One mesh part: a name (`app.starter.part.<name>`), a colour, an alpha (1 opaque), positions (mm) and triangles.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: &'static str,
    pub color: &'static str,
    pub alpha: f64,
    pub p: Vec<f64>,
    pub i: Vec<usize>,
}

/// One entourage item: its slug, category and parts.
#[derive(Clone, Debug)]
pub struct Item {
    pub slug: &'static str,
    pub cat: &'static str,
    pub parts: Vec<Part>,
}

fn r1(v: f64) -> f64 {
    round(v * 10.0) / 10.0
}

/// A loft ring: z, r, centre (cx, cy), radii (rx, ry: r when None).
#[derive(Clone, Copy, Default)]
struct Ring {
    z: f64,
    r: f64,
    cx: f64,
    cy: f64,
    rx: Option<f64>,
    ry: Option<f64>,
}

fn ring(z: f64, r: f64) -> Ring {
    Ring { z, r, ..Ring::default() }
}

impl Part {
    fn new(name: &'static str, color: &'static str, alpha: f64) -> Part {
        Part { name, color, alpha, p: Vec::new(), i: Vec::new() }
    }
    fn n(&self) -> usize {
        self.p.len() / 3
    }
    fn v(&mut self, x: f64, y: f64, z: f64) -> usize {
        self.p.extend([r1(x), r1(y), r1(z)]);
        self.n().saturating_sub(1)
    }
    fn tri(&mut self, a: usize, b: usize, c: usize) {
        self.i.extend([a, b, c]);
    }
    fn loft(&mut self, rings: &[Ring], seg: usize) {
        let base = self.n();
        let sf = seg as f64;
        for g in rings {
            for i in 0..seg {
                let a = (2.0 * PI * i as f64) / sf;
                self.v(or(g.cx, 0.0) + g.rx.unwrap_or(g.r) * a.jcos(), or(g.cy, 0.0) + g.ry.unwrap_or(g.r) * a.jsin(), g.z);
            }
        }
        for k in 0..rings.len().saturating_sub(1) {
            for i in 0..seg {
                let a = base + k * seg + i;
                let b = base + k * seg + (i + 1) % seg;
                let (c, d) = (a + seg, b + seg);
                self.tri(a, b, d);
                self.tri(a, d, c);
            }
        }
        let (Some(f), Some(l)) = (rings.first(), rings.last()) else { return };
        let bot = self.v(or(f.cx, 0.0), or(f.cy, 0.0), f.z);
        let top = self.v(or(l.cx, 0.0), or(l.cy, 0.0), l.z);
        let t0 = base + (rings.len() - 1) * seg;
        for i in 0..seg {
            self.tri(bot, base + (i + 1) % seg, base + i);
            self.tri(top, t0 + i, t0 + (i + 1) % seg);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn blob(&mut self, c: [f64; 3], rx: f64, ry: f64, rz: f64, sub: usize, bump: f64, r: &mut Rng) {
        let t = (1.0 + 5f64.sqrt()) / 2.0;
        let raw: [[f64; 3]; 12] = [
            [-1.0, t, 0.0],
            [1.0, t, 0.0],
            [-1.0, -t, 0.0],
            [1.0, -t, 0.0],
            [0.0, -1.0, t],
            [0.0, 1.0, t],
            [0.0, -1.0, -t],
            [0.0, 1.0, -t],
            [t, 0.0, -1.0],
            [t, 0.0, 1.0],
            [-t, 0.0, -1.0],
            [-t, 0.0, 1.0],
        ];
        let norm = |p: [f64; 3]| {
            let l = hypot(&p);
            [p[0] / l, p[1] / l, p[2] / l]
        };
        let mut vs: Vec<[f64; 3]> = raw.iter().map(|p| norm(*p)).collect();
        let mut fs: Vec<[usize; 3]> = vec![
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];
        for _ in 0..sub {
            let mut mid: HashMap<(usize, usize), usize> = HashMap::new();
            let mut m = |vs: &mut Vec<[f64; 3]>, a: usize, b: usize| -> usize {
                let k = if a < b { (a, b) } else { (b, a) };
                if let Some(&i) = mid.get(&k) {
                    return i;
                }
                let (pa, pb) = (vs.get(a).copied().unwrap_or_default(), vs.get(b).copied().unwrap_or_default());
                vs.push(norm([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0]));
                let i = vs.len() - 1;
                mid.insert(k, i);
                i
            };
            let mut nf = Vec::with_capacity(fs.len().saturating_mul(4));
            for [a, b, c] in fs {
                let ab = m(&mut vs, a, b);
                let bc = m(&mut vs, b, c);
                let ca = m(&mut vs, c, a);
                nf.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
            }
            fs = nf;
        }
        let base = self.n();
        for p in &vs {
            let k = 1.0 + bump * (r.next() * 2.0 - 1.0);
            self.v(c[0] + p[0] * rx * k, c[1] + p[1] * ry * k, c[2] + p[2] * rz * k);
        }
        for [a, b, d] in fs {
            self.tri(base + a, base + b, base + d);
        }
    }
    fn ground(&mut self) {
        for z in self.p.iter_mut().skip(2).step_by(3) {
            if *z < 0.0 {
                *z = 0.0;
            }
        }
    }
    fn bx(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) {
        let b = self.n();
        for z in [z0, z1] {
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                self.v(x, y, z);
            }
        }
        for t in [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ] {
            self.tri(b + t[0], b + t[1], b + t[2]);
        }
    }
    fn sweep_y(&mut self, prof: &[[f64; 2]], y0: f64, y1: f64) {
        let (n, b) = (prof.len(), self.n());
        for y in [y0, y1] {
            for [x, z] in prof {
                self.v(*x, y, *z);
            }
        }
        for i in 1..n.saturating_sub(1) {
            self.tri(b, b + i, b + i + 1);
            self.tri(b + n, b + n + i + 1, b + n + i);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            self.tri(b + i, b + n + j, b + j);
            self.tri(b + i, b + n + i, b + n + j);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn wheel(&mut self, x: f64, z: f64, r: f64, y0: f64, y1: f64, seg: usize) {
        let b = self.n();
        for y in [y0, y1] {
            for i in 0..seg {
                let a = (2.0 * PI * i as f64) / seg as f64 - PI / 2.0;
                self.v(x + r * a.jcos(), y, z + r * a.jsin());
            }
        }
        let c0 = self.v(x, y0, z);
        let c1 = self.v(x, y1, z);
        for i in 0..seg {
            let j = (i + 1) % seg;
            self.tri(b + i, b + seg + j, b + j);
            self.tri(b + i, b + seg + i, b + seg + j);
            self.tri(c0, b + i, b + j);
            self.tri(c1, b + seg + j, b + seg + i);
        }
    }
}

const LEAF: [&str; 4] = ["#4f7a3a", "#5d8a3f", "#3f6b35", "#6f8f3c"];

fn deciduous(slug: &'static str, h: f64, seed: u32, leaf: &'static str) -> Item {
    let mut r = Rng::new(seed);
    let (mut trunk, mut crown) = (Part::new("trunk", "#6b4a2f", 1.0), Part::new("leaves", leaf, 1.0));
    let tr = h * 0.035;
    trunk.loft(&[ring(0.0, tr * 1.3), ring(h * 0.25, tr), ring(h * 0.48, tr * 0.7)], 10);
    let (cz, cr) = (h * 0.66, h * 0.27);
    crown.blob([0.0, 0.0, cz], cr, cr, cr * 0.92, 2, 0.08, &mut r);
    for k in 0..4 {
        let a = (2.0 * PI * f64::from(k)) / 4.0 + r.next();
        let d = cr * 0.55;
        let c = [d * a.jcos(), d * a.jsin(), cz + (r.next() - 0.4) * cr * 0.6];
        crown.blob(c, cr * 0.62, cr * 0.62, cr * 0.55, 1, 0.12, &mut r);
    }
    Item { slug, cat: "site/landscape", parts: vec![trunk, crown] }
}

fn conifer(slug: &'static str, h: f64, seed: u32) -> Item {
    let mut r = Rng::new(seed);
    let (mut trunk, mut crown) = (Part::new("trunk", "#5a3d28", 1.0), Part::new("needles", "#2f5a3a", 1.0));
    trunk.loft(&[ring(0.0, h * 0.03), ring(h * 0.25, h * 0.022)], 8);
    let (base, r0) = (h * 0.18, h * 0.2);
    for k in 0..4 {
        let kf = f64::from(k);
        let z0 = base + kf * h * 0.17;
        let rr = r0 * (1.0 - kf * 0.2) * (0.95 + r.next() * 0.1);
        crown.loft(&[ring(z0, rr), ring(z0 + h * 0.3, rr * 0.08)], 12);
    }
    Item { slug, cat: "site/landscape", parts: vec![trunk, crown] }
}

fn columnar(slug: &'static str, h: f64, seed: u32) -> Item {
    let mut r = Rng::new(seed);
    let (mut trunk, mut crown) = (Part::new("trunk", "#5a3d28", 1.0), Part::new("leaves", "#3d6233", 1.0));
    trunk.loft(&[ring(0.0, h * 0.025), ring(h * 0.12, h * 0.02)], 8);
    crown.blob([0.0, 0.0, h * 0.55], h * 0.11, h * 0.11, h * 0.45, 2, 0.06, &mut r);
    Item { slug, cat: "site/landscape", parts: vec![trunk, crown] }
}

fn shrub(slug: &'static str, w: f64, h: f64, seed: u32, leaf: &'static str) -> Item {
    let mut r = Rng::new(seed);
    let mut p = Part::new("leaves", leaf, 1.0);
    p.blob([0.0, 0.0, h * 0.48], w / 2.0, w / 2.0, h * 0.52, 2, 0.1, &mut r);
    for k in 0..3 {
        let a = (2.0 * PI * f64::from(k)) / 3.0 + r.next();
        p.blob([w * 0.25 * a.jcos(), w * 0.25 * a.jsin(), h * 0.4], w * 0.3, w * 0.3, h * 0.38, 1, 0.12, &mut r);
    }
    p.ground();
    Item { slug, cat: "site/landscape", parts: vec![p] }
}

fn hedge(slug: &'static str, l: f64, w: f64, h: f64, seed: u32) -> Item {
    let mut r = Rng::new(seed);
    let mut p = Part::new("leaves", "#46703a", 1.0);
    let n = cc_m3d::js::max(2.0, round(l / 500.0));
    let mut k = 0.0;
    while k < n {
        let x = -l / 2.0 + (k + 0.5) * (l / n);
        p.blob([x, 0.0, h * 0.5], (l / n) * 0.62, w * 0.52, h * 0.52, 1, 0.08, &mut r);
        k += 1.0;
    }
    p.ground();
    Item { slug, cat: "site/landscape", parts: vec![p] }
}

fn potted(slug: &'static str, seed: u32) -> Item {
    let mut r = Rng::new(seed);
    let (mut pot, mut leaf) = (Part::new("pot", "#b0603e", 1.0), Part::new("leaves", "#4f7a3a", 1.0));
    pot.loft(&[ring(0.0, 170.0), ring(380.0, 230.0)], 16);
    leaf.blob([0.0, 0.0, 720.0], 330.0, 330.0, 380.0, 2, 0.12, &mut r);
    Item { slug, cat: "site/landscape", parts: vec![pot, leaf] }
}

struct Who {
    h: f64,
    walk: f64,
    top: &'static str,
    legs: &'static str,
    skin: &'static str,
    hair: &'static str,
    seed: u32,
}

const WHO: Who = Who { h: 1750.0, walk: 0.0, top: "#3e5c7a", legs: "#3a3d42", skin: "#c9a184", hair: "#3b2b22", seed: 1 };

fn person(slug: &'static str, o: &Who) -> Item {
    let k = o.h / 1750.0;
    let mut body = Part::new("clothes", o.top, 1.0);
    let mut lower = Part::new("trousers", o.legs, 1.0);
    let mut head = Part::new("skin", o.skin, 1.0);
    let mut hair = Part::new("hair", o.hair, 1.0);
    let mut r = Rng::new(o.seed);
    let step = o.walk * 160.0 * k;
    for (sx, sy) in [(-90.0, step), (90.0, -step)] {
        let a = Ring { z: 0.0, r: 60.0 * k, cx: sx * k, cy: sy - 20.0 * k, ..Ring::default() };
        let b = Ring { z: 820.0 * k, r: 75.0 * k, cx: sx * k * 0.85, cy: sy * 0.2, ..Ring::default() };
        lower.loft(&[a, b], 8);
    }
    let el = |z: f64, rx: f64, ry: f64| Ring { z: z * k, r: 0.0, rx: Some(rx * k), ry: Some(ry * k), ..Ring::default() };
    body.loft(&[el(790.0, 180.0, 105.0), el(1150.0, 190.0, 110.0), el(1430.0, 210.0, 115.0), el(1480.0, 120.0, 80.0)], 12);
    for sx in [-1.0, 1.0] {
        let swing = -sx * o.walk * 140.0 * k;
        let a = Ring { z: 820.0 * k, r: 42.0 * k, cx: sx * 245.0 * k, cy: swing, ..Ring::default() };
        let b = Ring { z: 1420.0 * k, r: 50.0 * k, cx: sx * 228.0 * k, cy: 0.0, ..Ring::default() };
        body.loft(&[a, b], 8);
    }
    head.loft(&[ring(1470.0 * k, 45.0 * k), ring(1540.0 * k, 50.0 * k)], 8);
    head.blob([0.0, -10.0 * k, 1630.0 * k], 95.0 * k, 105.0 * k, 120.0 * k, 2, 0.01, &mut r);
    hair.blob([0.0, 15.0 * k, 1690.0 * k], 98.0 * k, 100.0 * k, 75.0 * k, 1, 0.02, &mut r);
    Item { slug, cat: "people/entourage", parts: vec![lower, body, head, hair] }
}

#[allow(clippy::too_many_arguments)]
fn car(slug: &'static str, l: f64, w: f64, h: f64, color: &'static str, suv: bool) -> Item {
    let mut body = Part::new("body", color, 1.0);
    let mut glass = Part::new("glass", "#2d3a44", 0.55);
    let mut tyres = Part::new("tyres", "#1e2023", 1.0);
    let mut lights = Part::new("lights", "#e9e4d6", 1.0);
    let (x0, x1, sill, belt, roof) = (-l / 2.0, l / 2.0, 330.0, if suv { 1000.0 } else { 860.0 }, h);
    body.sweep_y(
        &[
            [x0 + 60.0, sill],
            [x1 - 60.0, sill],
            [x1, sill + 180.0],
            [x1 - 40.0, belt - 60.0],
            [x1 - 300.0, belt],
            [x0 + 200.0, belt],
            [x0, belt - 120.0],
            [x0, sill + 140.0],
        ],
        -w / 2.0,
        w / 2.0,
    );
    let c0 = x0 + if suv { 600.0 } else { 1050.0 };
    let c1 = x1 - if suv { 950.0 } else { 1250.0 };
    let inset = 70.0;
    glass.sweep_y(
        &[[c0 - if suv { 120.0 } else { 300.0 }, belt], [c1 + if suv { 250.0 } else { 500.0 }, belt], [c1, roof - 60.0], [c0, roof - 60.0]],
        -w / 2.0 + inset,
        w / 2.0 - inset,
    );
    body.bx(c0 + 10.0, -w / 2.0 + inset + 20.0, roof - 70.0, c1 - 10.0, w / 2.0 - inset - 20.0, roof);
    let (wr, wb) = (if suv { 380.0 } else { 330.0 }, l * 0.6);
    for x in [-wb / 2.0, wb / 2.0] {
        for (y0, y1) in [(-w / 2.0 - 5.0, -w / 2.0 + 230.0), (w / 2.0 - 230.0, w / 2.0 + 5.0)] {
            tyres.wheel(x, wr, wr, y0, y1, 14);
        }
    }
    for y in [-w / 2.0 + 120.0, w / 2.0 - 420.0] {
        lights.bx(x1 - 30.0, y, sill + 300.0, x1 + 5.0, y + 300.0, sill + 400.0);
    }
    Item { slug, cat: "site/vehicles", parts: vec![body, glass, tyres, lights] }
}

/// The slugs, in entourage()'s order (the starter library lists them as `ent-<slug>`).
pub const SLUGS: [&str; 15] = [
    "tree-deciduous-6m",
    "tree-deciduous-10m",
    "tree-ornamental-4m",
    "tree-conifer-8m",
    "tree-columnar-7m",
    "shrub-round-1200",
    "shrub-small-600",
    "hedge-2000",
    "plant-potted",
    "person-standing",
    "person-walking",
    "person-standing-2",
    "child-standing",
    "car-sedan",
    "car-suv",
];

/// One item by slug (entourage().find).
pub fn item(slug: &str) -> Option<Item> {
    Some(match slug {
        "tree-deciduous-6m" => deciduous("tree-deciduous-6m", 6000.0, 101, LEAF[0]),
        "tree-deciduous-10m" => deciduous("tree-deciduous-10m", 10000.0, 102, LEAF[1]),
        "tree-ornamental-4m" => deciduous("tree-ornamental-4m", 4000.0, 103, LEAF[3]),
        "tree-conifer-8m" => conifer("tree-conifer-8m", 8000.0, 104),
        "tree-columnar-7m" => columnar("tree-columnar-7m", 7000.0, 105),
        "shrub-round-1200" => shrub("shrub-round-1200", 1200.0, 1000.0, 106, LEAF[2]),
        "shrub-small-600" => shrub("shrub-small-600", 600.0, 550.0, 107, LEAF[1]),
        "hedge-2000" => hedge("hedge-2000", 2000.0, 600.0, 1200.0, 108),
        "plant-potted" => potted("plant-potted", 109),
        "person-standing" => person("person-standing", &Who { seed: 201, ..WHO }),
        "person-walking" => person("person-walking", &Who { walk: 1.0, top: "#7a3e3e", legs: "#2f3b4f", seed: 202, ..WHO }),
        "person-standing-2" => {
            person("person-standing-2", &Who { h: 1650.0, top: "#c8c1b3", legs: "#55606b", hair: "#6b4a2f", seed: 203, ..WHO })
        }
        "child-standing" => person("child-standing", &Who { h: 1200.0, top: "#3e7a5c", legs: "#3a3d42", seed: 204, ..WHO }),
        "car-sedan" => car("car-sedan", 4700.0, 1820.0, 1450.0, "#8a1f24", false),
        "car-suv" => car("car-suv", 4800.0, 1900.0, 1750.0, "#4b5560", true),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_made_and_indexes_its_own_points() {
        for s in SLUGS {
            let it = item(s).unwrap();
            for p in &it.parts {
                let n = p.p.len() / 3;
                assert!(n > 0 && p.i.len() % 3 == 0 && p.i.iter().all(|&i| i < n), "{s} {}", p.name);
            }
        }
        // a shrub sits on its flattened base
        assert!(item("shrub-small-600").unwrap().parts[0].p.iter().skip(2).step_by(3).all(|z| *z >= 0.0));
        assert!(item("nothing").is_none());
    }
}
