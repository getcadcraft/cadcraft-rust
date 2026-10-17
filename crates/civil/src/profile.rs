//! profile.js — vertical profiles: existing ground sampled along an alignment, and design grades by PVIs with
//! symmetric parabolic vertical curves (L, or K: L = K·|A|).

use crate::alignment::Alignment;
use crate::js::{G, J, Obj, round_fixed, to_fixed};
use crate::tin::Surface;
use crate::tr;
use serde_json::Value;

/// `{ sta, z | null }`.
#[derive(Clone, Copy, Debug)]
pub struct StaZ {
    pub sta: f64,
    pub z: Option<f64>,
}
impl J for StaZ {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "z" => self.z }
    }
}

/// `sampleSurface(surface, alignment, { step })`.
pub fn sample_surface(surface: Option<&Surface>, al: &Alignment, step: f64) -> Vec<StaZ> {
    let Some(surface) = surface else { return Vec::new() };
    if !al.ok() {
        return Vec::new();
    }
    let st = if step > 0.0 { step } else { crate::js::max(0.5, al.length / 1000.0) };
    let mut stas: Vec<f64> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |v: f64, stas: &mut Vec<f64>| {
        let k = if v == 0.0 { 0u64 } else { v.to_bits() };
        if seen.insert(k) {
            stas.push(v);
        }
    };
    let mut s = al.start;
    let mut guard = 0u32;
    while s < al.end && guard < 10_000_000 {
        add(round_fixed(s, 6), &mut stas);
        s += st;
        guard = guard.saturating_add(1);
    }
    add(al.end, &mut stas);
    for (sta, _) in al.key_stations() {
        add(sta, &mut stas);
    }
    crate::js::sort_by(&mut stas, |a, b| a - b);
    stas.into_iter()
        .map(|sta| {
            let z = al.point_at(sta).and_then(|p| surface.elevation_at(p.x, p.y, 1e-6));
            StaZ { sta, z }
        })
        .collect()
}

/// A PVI `{ sta, z, L, K? }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pvi {
    pub sta: f64,
    pub z: f64,
    /// the JS's `L` as given (None = no key); buildProfile writes the length it used back at interior PVIs
    pub l: Option<f64>,
    pub k: Option<f64>,
}
impl J for Pvi {
    fn j(&self) -> Value {
        Obj::new().set("sta", self.sta).set("z", self.z).opt("L", self.l).opt("K", self.k).done()
    }
}
pub fn read_pvi(v: &Value) -> Pvi {
    Pvi {
        sta: crate::js::numf(v.get("sta")),
        z: crate::js::numf(v.get("z")),
        l: v.get("L").map(|l| crate::js::number(Some(l))),
        k: v.get("K").filter(|k| !k.is_null()).map(|k| crate::js::number(Some(k))),
    }
}

/// A grade `{ from, to, g }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Grade {
    pub from: f64,
    pub to: f64,
    pub g: f64,
}
impl J for Grade {
    fn j(&self) -> Value {
        crate::jo! { "from" => self.from, "to" => self.to, "g" => self.g }
    }
}
/// A vertical curve.
#[derive(Clone, Copy, Debug, Default)]
pub struct VCurve {
    pub i: usize,
    pub pvi: f64,
    pub z_pvi: f64,
    pub bvc: f64,
    pub evc: f64,
    pub l: f64,
    pub g1: f64,
    pub g2: f64,
    pub a: f64,
    pub k: f64,
    pub z_bvc: f64,
    pub z_evc: f64,
    pub crest: bool,
    /// high (crest) or low (sag) point
    pub hl: Option<(f64, f64)>,
}
impl J for VCurve {
    fn j(&self) -> Value {
        let mut o = Obj::new()
            .set("i", self.i)
            .set("pvi", self.pvi)
            .set("zPvi", self.z_pvi)
            .set("bvc", self.bvc)
            .set("evc", self.evc)
            .set("L", self.l)
            .set("g1", self.g1)
            .set("g2", self.g2)
            .set("A", self.a)
            .set("K", self.k)
            .set("zBvc", self.z_bvc)
            .set("zEvc", self.z_evc)
            .set("kind", if self.crest { "crest" } else { "sag" });
        if let Some((sta, z)) = self.hl {
            o = o.set(if self.crest { "high" } else { "low" }, crate::jo! { "sta" => sta, "z" => z });
        }
        o.done()
    }
}

/// A design profile.
#[derive(Clone, Debug)]
pub struct Profile {
    pub pvis: Vec<Pvi>,
    pub grades: Vec<Grade>,
    pub curves: Vec<VCurve>,
    pub errors: Vec<String>,
    pub start: f64,
    pub end: f64,
}

/// `buildProfile(pvis)`.
pub fn build_profile(pvis_in: &[Pvi]) -> Profile {
    let mut errors = Vec::new();
    let mut pvis: Vec<Pvi> = pvis_in.iter().filter(|p| p.sta.is_finite() && p.z.is_finite()).copied().collect();
    crate::js::sort_by(&mut pvis, |a, b| a.sta - b.sta);
    for i in 1..pvis.len() {
        if pvis.g(i).sta - pvis.g(i - 1).sta < 1e-6 {
            errors.push(tr!("prof.err.two", sta = pvis.g(i).sta));
        }
    }
    let mut grades = Vec::new();
    for i in 0..pvis.len().saturating_sub(1) {
        let (a, b) = (pvis.g(i), pvis.g(i + 1));
        grades.push(Grade { from: a.sta, to: b.sta, g: (b.z - a.z) / (b.sta - a.sta) });
    }
    let mut curves = Vec::new();
    for i in 1..pvis.len().saturating_sub(1) {
        let v = pvis.g(i);
        let g1 = grades.g(i - 1).g;
        let g2 = grades.g(i).g;
        let a = (g2 - g1) * 100.0;
        let lv = v.l.unwrap_or(f64::NAN);
        let mut l = if lv != 0.0 && !lv.is_nan() { lv } else { 0.0 };
        if l == 0.0 && v.k.is_some_and(|k| k > 0.0) {
            l = v.k.unwrap_or(0.0) * a.abs();
        }
        if let Some(p) = pvis.get_mut(i) {
            p.l = Some(l);
        }
        if l <= 0.0 || a.abs() < 1e-9 {
            continue;
        }
        let bvc = v.sta - l / 2.0;
        let evc = v.sta + l / 2.0;
        let z_bvc = v.z - g1 * l / 2.0;
        let crest = g2 < g1;
        let mut c =
            VCurve { i, pvi: v.sta, z_pvi: v.z, bvc, evc, l, g1, g2, a, k: l / a.abs(), z_bvc, z_evc: v.z + g2 * l / 2.0, crest, hl: None };
        if g1 * g2 < 0.0 {
            let x = -g1 * l / (g2 - g1);
            c.hl = Some((bvc + x, z_bvc + g1 * x + (g2 - g1) * x * x / (2.0 * l)));
        }
        curves.push(c);
    }
    // curves must stay between their neighbours and not overlap
    for k in 0..curves.len() {
        let c = curves.g(k);
        let prev_end = if k > 0 { curves.g(k - 1).evc } else { pvis.g(0).sta };
        let left = pvis.g(c.i - 1).sta;
        let right = pvis.g(c.i + 1).sta;
        if c.bvc < left - 1e-9 || c.bvc < prev_end - 1e-9 {
            errors.push(tr!("prof.err.before", pvi = c.i + 1, len = to_fixed(c.l, 2)));
        }
        if c.evc > right + 1e-9 {
            errors.push(tr!("prof.err.past", pvi = c.i + 1, len = to_fixed(c.l, 2)));
        }
    }
    let start = pvis.first().map_or(0.0, |p| p.sta);
    let end = pvis.last().map_or(0.0, |p| p.sta);
    Profile { pvis, grades, curves, errors, start, end }
}

impl Profile {
    pub fn ok(&self) -> bool {
        self.pvis.len() >= 2 && self.errors.is_empty()
    }

    pub fn elevation_at(&self, sta: f64) -> Option<f64> {
        let p = &self.pvis;
        if p.len() < 2 || sta < self.start - 1e-9 || sta > self.end + 1e-9 {
            return None;
        }
        for c in &self.curves {
            if sta >= c.bvc && sta <= c.evc {
                let x = sta - c.bvc;
                return Some(c.z_bvc + c.g1 * x + (c.g2 - c.g1) * x * x / (2.0 * c.l));
            }
        }
        for i in 0..p.len() - 1 {
            if sta <= p.g(i + 1).sta + 1e-9 {
                return Some(p.g(i).z + self.grades.g(i).g * (sta - p.g(i).sta));
            }
        }
        None
    }
    pub fn grade_at(&self, sta: f64) -> Option<f64> {
        for c in &self.curves {
            if sta >= c.bvc && sta <= c.evc {
                return Some(c.g1 + (c.g2 - c.g1) * (sta - c.bvc) / c.l);
            }
        }
        for g in &self.grades {
            if sta <= g.to + 1e-9 {
                return Some(g.g);
            }
        }
        None
    }
    /// Stations to draw at: every step plus curve ends and PVIs.
    pub fn sample(&self, step: f64) -> Vec<StaZ> {
        let mut s: Vec<f64> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |v: f64, s: &mut Vec<f64>| {
            let k = if v == 0.0 { 0u64 } else { v.to_bits() };
            if seen.insert(k) {
                s.push(v);
            }
        };
        for p in &self.pvis {
            add(p.sta, &mut s);
        }
        for c in &self.curves {
            let st = if step != 0.0 && !step.is_nan() { step } else { c.l / 32.0 };
            let n = crate::js::max(8.0, (c.l / st).ceil());
            let mut k = 0.0;
            while k <= n && k < 1e7 {
                add(c.bvc + (c.l * k) / n, &mut s);
                k += 1.0;
            }
        }
        crate::js::sort_by(&mut s, |a, b| a - b);
        s.into_iter().map(|sta| StaZ { sta, z: self.elevation_at(sta) }).collect()
    }
}

impl J for Profile {
    fn j(&self) -> Value {
        crate::jo! {
            "pvis" => self.pvis, "grades" => self.grades, "curves" => self.curves, "errors" => self.errors,
            "start" => self.start, "end" => self.end, "ok" => self.ok(),
        }
    }
}

crate::und_default!(Pvi, Grade, VCurve);
