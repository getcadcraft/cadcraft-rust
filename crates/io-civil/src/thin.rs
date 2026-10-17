//! thin.js — thinning point clouds and grids to a spacing: one point per square cell (lowest, nearest the centre, or
//! the cell's mean), a class filter and a region, a cap on cells; and the coarse per-class coverage that estimates how
//! many points a thinning keeps.

use crate::terrain::BoxXY;
use cc_civil::i18n::t;
use cc_civil::js::{J, max, min, round};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Thinning mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Lowest,
    Nearest,
    Average,
}

/// A class value as the JS Set / Map see it (SameValueZero on numbers).
fn class_key(c: f64) -> u64 {
    if c == 0.0 { 0f64.to_bits() } else { c.to_bits() }
}

/// `new Thinner({...})`.
#[derive(Clone, Debug)]
pub struct Thinner {
    pub cell: f64,
    pub mode: Mode,
    ox: f64,
    oy: f64,
    classes: Option<Vec<u64>>,
    region: Option<BoxXY>,
    max: f64,
    map: HashMap<u64, usize>,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    w: Vec<f64>,
    pub count: usize,
    pub seen: f64,
    pub filtered: f64,
    pub outside: f64,
    pub capped: bool,
    pub dropped: f64,
}

impl Thinner {
    /// `mode`: "lowest" | "nearest" | "average"; `classes`: the classes to keep (None = all).
    pub fn new(
        cell: f64,
        mode: &str,
        origin: [f64; 2],
        classes: Option<&[f64]>,
        region: Option<BoxXY>,
        max: f64,
    ) -> Result<Thinner, String> {
        if !(cell > 0.0) || !cell.is_finite() {
            return Err(t("thin.spacing", &[]));
        }
        let mode = match mode {
            "lowest" => Mode::Lowest,
            "nearest" => Mode::Nearest,
            "average" => Mode::Average,
            other => return Err(format!("Unknown thinning: {other}")),
        };
        Ok(Thinner {
            cell,
            mode,
            ox: origin[0],
            oy: origin[1],
            classes: classes.map(|c| c.iter().map(|&v| class_key(v)).collect()),
            region,
            max,
            map: HashMap::new(),
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            w: Vec::new(),
            count: 0,
            seen: 0.0,
            filtered: 0.0,
            outside: 0.0,
            capped: false,
            dropped: 0.0,
        })
    }

    /// `t.add(x, y, z, cls)`.
    pub fn add(&mut self, x: f64, y: f64, z: f64, cls: f64) -> bool {
        self.seen += 1.0;
        if let Some(c) = &self.classes
            && !c.contains(&class_key(cls))
        {
            self.filtered += 1.0;
            return false;
        }
        if let Some(r) = &self.region
            && (x < r.min_x || x > r.max_x || y < r.min_y || y > r.max_y)
        {
            self.outside += 1.0;
            return false;
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return false;
        }
        let fx = (x - self.ox) / self.cell;
        let fy = (y - self.oy) / self.cell;
        let ix = fx.floor();
        let iy = fy.floor();
        if ix.abs() >= 2097152.0 || iy.abs() >= 2147483647.0 {
            self.outside += 1.0;
            return false;
        }
        let key = ix * 4294967296.0 + (iy + 2147483648.0);
        let kb = if key == 0.0 { 0f64.to_bits() } else { key.to_bits() };
        let d = (fx - ix - 0.5) * (fx - ix - 0.5) + (fy - iy - 0.5) * (fy - iy - 0.5);
        let Some(&k) = self.map.get(&kb) else {
            if (self.count as f64) >= self.max {
                self.capped = true;
                self.dropped += 1.0;
                return false;
            }
            let k = self.count;
            self.count = self.count.saturating_add(1);
            self.map.insert(kb, k);
            self.x.push(x);
            self.y.push(y);
            self.z.push(z);
            self.w.push(match self.mode {
                Mode::Average => 1.0,
                Mode::Nearest => d,
                Mode::Lowest => 0.0,
            });
            return true;
        };
        let (Some(px), Some(py), Some(pz), Some(pw)) = (self.x.get_mut(k), self.y.get_mut(k), self.z.get_mut(k), self.w.get_mut(k)) else {
            return true;
        };
        match self.mode {
            Mode::Lowest => {
                if z < *pz {
                    *px = x;
                    *py = y;
                    *pz = z;
                }
            }
            Mode::Nearest => {
                if d < *pw {
                    *px = x;
                    *py = y;
                    *pz = z;
                    *pw = d;
                }
            }
            Mode::Average => {
                *px += x;
                *py += y;
                *pz += z;
                *pw += 1.0;
            }
        }
        true
    }

    /// `t.points()`.
    pub fn points(&self) -> Vec<[f64; 3]> {
        let avg = self.mode == Mode::Average;
        (0..self.count)
            .map(|k| {
                let g = |v: &Vec<f64>| v.get(k).copied().unwrap_or(f64::NAN);
                if avg {
                    let w = g(&self.w);
                    [g(&self.x) / w, g(&self.y) / w, g(&self.z) / w]
                } else {
                    [g(&self.x), g(&self.y), g(&self.z)]
                }
            })
            .collect()
    }
}

/// `(fx - ix - 0.5) ** 2`: V8 computes `**` 2 as a multiplication.
#[cfg(test)]
mod pow_check {
    #[test]
    fn square() {
        let v: f64 = 0.123456789;
        assert_eq!(v * v, cc_civil::js::pow(v, 2.0));
    }
}

/// `gridStep(spacing, cellSize)`.
pub fn grid_step(spacing: f64, cell_size: f64) -> f64 {
    if spacing > 0.0 && cell_size > 0.0 { max(1.0, round(spacing / cell_size)) } else { 1.0 }
}

/// `new Coverage(box, n)`.
#[derive(Clone, Debug)]
pub struct Coverage {
    pub b: BoxXY,
    pub n: usize,
    pub w: f64,
    pub h: f64,
    by_class: Vec<(u64, f64, Vec<u32>)>,
    counts: Vec<(f64, f64)>,
    pub total: f64,
}

impl Coverage {
    pub fn new(b: BoxXY, n: usize) -> Coverage {
        let nn = n as f64;
        Coverage {
            b,
            n,
            w: max(b.max_x - b.min_x, 1e-9) / nn,
            h: max(b.max_y - b.min_y, 1e-9) / nn,
            by_class: Vec::new(),
            counts: Vec::new(),
            total: 0.0,
        }
    }
    pub fn add(&mut self, x: f64, y: f64, cls: f64) {
        let nm1 = (self.n as f64) - 1.0;
        let i = min(nm1, max(0.0, ((x - self.b.min_x) / self.w).floor()));
        let j = min(nm1, max(0.0, ((y - self.b.min_y) / self.h).floor()));
        let key = class_key(cls);
        let cells = self.n.saturating_mul(self.n);
        let pos = match self.by_class.iter().position(|(k, _, _)| *k == key) {
            Some(p) => p,
            None => {
                self.by_class.push((key, cls, vec![0; cells]));
                self.by_class.len().saturating_sub(1)
            }
        };
        // NaN coordinates index `a[NaN]`: a property write that the counts never read
        let idx = j * (self.n as f64) + i;
        if let (Some(e), Some(ix)) = (self.by_class.get_mut(pos), cc_civil::js::usize_of(idx))
            && let Some(c) = e.2.get_mut(ix)
        {
            *c = c.wrapping_add(1);
        }
        match self.counts.iter_mut().find(|(c, _)| class_key(*c) == key) {
            Some(e) => e.1 += 1.0,
            None => self.counts.push((cls, 1.0)),
        }
        self.total += 1.0;
    }
    /// The total per class, in first-seen order.
    pub fn counts(&self) -> &[(f64, f64)] {
        &self.counts
    }
    /// `estimate({ spacing, classes, box })` → (points, inArea).
    pub fn estimate(&self, spacing: f64, classes: Option<&[f64]>, b: Option<&BoxXY>) -> (f64, f64) {
        let n = self.n;
        let cells = n.saturating_mul(n);
        let mut per = vec![0.0f64; cells];
        let keep_cls: Option<Vec<u64>> = classes.map(|c| c.iter().map(|&v| class_key(v)).collect());
        for (k, _, a) in &self.by_class {
            if keep_cls.as_ref().is_none_or(|c| c.contains(k)) {
                for (p, v) in per.iter_mut().zip(a.iter()) {
                    *p += f64::from(*v);
                }
            }
        }
        let (mut keep, mut in_area) = (0.0, 0.0);
        let cell_area = self.w * self.h;
        for j in 0..n {
            for i in 0..n {
                let v = per.get(j.saturating_mul(n).saturating_add(i)).copied().unwrap_or(0.0);
                if v == 0.0 || v.is_nan() {
                    continue;
                }
                let mut f = 1.0;
                if let Some(bx) = b {
                    let x0 = self.b.min_x + (i as f64) * self.w;
                    let y0 = self.b.min_y + (j as f64) * self.h;
                    let ox = max(0.0, min(bx.max_x, x0 + self.w) - max(bx.min_x, x0));
                    let oy = max(0.0, min(bx.max_y, y0 + self.h) - max(bx.min_y, y0));
                    f = (ox * oy) / cell_area;
                    if f == 0.0 || f.is_nan() {
                        continue;
                    }
                }
                in_area += v * f;
                keep += min(v * f, (cell_area * f) / (spacing * spacing) + 1.0);
            }
        }
        (round(keep), round(in_area))
    }
}

impl J for Thinner {
    fn j(&self) -> Value {
        json!({ "count": self.count, "seen": self.seen.j(), "filtered": self.filtered.j(), "outside": self.outside.j(), "capped": self.capped, "dropped": self.dropped.j() })
    }
}
