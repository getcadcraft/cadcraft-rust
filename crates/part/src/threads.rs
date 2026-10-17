//! threads.js — the Thread feature's data and the thread geometry that needs no kernel: ISO 261 / ISO 2904 tables,
//! `threadDims`, `sizeFor`, the basic profiles (`profileCorners`, `profileAt`), the helical solid's cross-section
//! (`boltSection`), callouts and drawing notes. The helical solid itself (`boltSolid`, `threadTool`) is built by the
//! mesh kernel through [`crate::kernel::Solids`].

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use serde_json::Value;

pub const DEFAULT_CLEARANCE: f64 = 0.1;

/// ISO 261 coarse pitches, M1.6–M64: (size, d, pitch).
pub const ISO_METRIC: [(&str, f64, f64); 29] = [
    ("M1.6", 1.6, 0.35),
    ("M2", 2.0, 0.4),
    ("M2.5", 2.5, 0.45),
    ("M3", 3.0, 0.5),
    ("M3.5", 3.5, 0.6),
    ("M4", 4.0, 0.7),
    ("M5", 5.0, 0.8),
    ("M6", 6.0, 1.0),
    ("M8", 8.0, 1.25),
    ("M10", 10.0, 1.5),
    ("M12", 12.0, 1.75),
    ("M14", 14.0, 2.0),
    ("M16", 16.0, 2.0),
    ("M18", 18.0, 2.5),
    ("M20", 20.0, 2.5),
    ("M22", 22.0, 2.5),
    ("M24", 24.0, 3.0),
    ("M27", 27.0, 3.0),
    ("M30", 30.0, 3.5),
    ("M33", 33.0, 3.5),
    ("M36", 36.0, 4.0),
    ("M39", 39.0, 4.0),
    ("M42", 42.0, 4.5),
    ("M45", 45.0, 4.5),
    ("M48", 48.0, 5.0),
    ("M52", 52.0, 5.0),
    ("M56", 56.0, 5.5),
    ("M60", 60.0, 5.5),
    ("M64", 64.0, 6.0),
];
/// ISO 2904 trapezoidal, preferred pitches 8–60.
pub const ISO_TRAPEZOIDAL: [(&str, f64, f64); 19] = [
    ("Tr8x1_5", 8.0, 1.5),
    ("Tr10x2", 10.0, 2.0),
    ("Tr12x3", 12.0, 3.0),
    ("Tr14x3", 14.0, 3.0),
    ("Tr16x4", 16.0, 4.0),
    ("Tr18x4", 18.0, 4.0),
    ("Tr20x4", 20.0, 4.0),
    ("Tr22x5", 22.0, 5.0),
    ("Tr24x5", 24.0, 5.0),
    ("Tr26x5", 26.0, 5.0),
    ("Tr28x5", 28.0, 5.0),
    ("Tr30x6", 30.0, 6.0),
    ("Tr32x6", 32.0, 6.0),
    ("Tr36x6", 36.0, 6.0),
    ("Tr40x7", 40.0, 7.0),
    ("Tr44x7", 44.0, 7.0),
    ("Tr48x8", 48.0, 8.0),
    ("Tr52x8", 52.0, 8.0),
    ("Tr60x9", 60.0, 9.0),
];

fn table(form: &str) -> &'static [(&'static str, f64, f64)] {
    match form {
        "iso" => &ISO_METRIC,
        "tr" => &ISO_TRAPEZOIDAL,
        _ => &[],
    }
}
/// `THREAD_TABLES[form][size]` → (d, p).
pub fn table_row(form: &str, size: &str) -> Option<(f64, f64)> {
    table(form).iter().find(|r| r.0 == size).map(|r| (r.1, r.2))
}

/// `sizeLabel(form, size)`.
pub fn size_label(form: &str, size: &str) -> String {
    if form == "tr" { size.replacen('x', "×", 1).replacen('_', ".", 1) } else { size.to_string() }
}

/// `threadDims(form, size)`.
#[derive(Clone, Copy, Debug)]
pub struct Dims {
    pub d: f64,
    pub p: f64,
    pub d1: f64,
    pub d2: f64,
    pub depth: f64,
}
impl Dims {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("d", jv::num(self.d)),
            ("p", jv::num(self.p)),
            ("d1", jv::num(self.d1)),
            ("d2", jv::num(self.d2)),
            ("depth", jv::num(self.depth)),
        ])
    }
}
pub fn thread_dims(form: &str, size: &str) -> Option<Dims> {
    thread_dims_js(form, size).ok().flatten()
}
/// `threadDims` as the JS runs it: an `Object.prototype` size in a real table destructures a function and throws.
pub fn thread_dims_js(form: &str, size: &str) -> Result<Option<Dims>, String> {
    if (form == "iso" || form == "tr") && crate::sketch::proto_key(size) {
        return Err("row is not iterable".into());
    }
    Ok(dims_of(form, size))
}
fn dims_of(form: &str, size: &str) -> Option<Dims> {
    let (d, p) = table_row(form, size)?;
    let depth = if form == "tr" { p / 2.0 } else { 5.0 * 3.0f64.sqrt() / 16.0 * p };
    Some(Dims { d, p, d1: d - 2.0 * depth, d2: if form == "tr" { d - p / 2.0 } else { d - 0.649519 * p }, depth })
}

/// `sizeFor(form, dia, external)`.
pub fn size_for(form: &str, dia: f64, external: bool) -> Option<&'static str> {
    let mut best: Option<&'static str> = None;
    let mut bd = f64::INFINITY;
    for (size, _, _) in table(form) {
        let Some(g) = thread_dims(form, size) else { continue };
        let want = if external { g.d } else { (g.d1 + g.d2) / 2.0 };
        let dd = (dia - want).abs();
        if dd < bd {
            bd = dd;
            best = Some(size);
        }
    }
    if best.is_some() && bd <= 0.12 * dia { best } else { None }
}

/// `profileCorners(form, size, shift)`: (u, r) corners over one pitch, crest first.
pub fn profile_corners(form: &str, size: &str, shift: f64) -> Result<Option<Vec<[f64; 2]>>, String> {
    let Some(g) = thread_dims_js(form, size)? else { return Ok(None) };
    let (rr, r, p) = (g.d / 2.0 + shift, g.d1 / 2.0 + shift, g.p);
    let crest = if form == "tr" { p * (0.5 - js::tan(std::f64::consts::PI / 12.0) / 2.0) } else { p / 8.0 };
    let root = if form == "tr" { crest } else { p / 4.0 };
    let flank = (p - crest - root) / 2.0;
    Ok(Some(vec![[0.0, rr], [crest, rr], [crest + flank, r], [crest + flank + root, r], [p, rr]]))
}

/// `profileAt(corners, u)`.
pub fn profile_at(corners: &[[f64; 2]], u: f64) -> f64 {
    let p = corners.last().map_or(f64::NAN, |c| c[0]);
    let w = ((u % p) + p) % p;
    for k in 1..corners.len() {
        let (Some(a), Some(b)) = (corners.get(k.saturating_sub(1)), corners.get(k)) else { break };
        let ([u0, r0], [u1, r1]) = (*a, *b);
        if w <= u1 + 1e-15 {
            return if u1 - u0 < 1e-15 { r1 } else { r0 + (r1 - r0) * (w - u0) / (u1 - u0) };
        }
    }
    corners.first().map_or(f64::NAN, |c| c[1])
}

/// `boltSection(corners, { hand, segments })`: one CCW polygon square to the axis at z = 0.
pub fn bolt_section(corners: &[[f64; 2]], hand: &str, segments: f64) -> Vec<[f64; 2]> {
    let p = corners.last().map_or(f64::NAN, |c| c[0]);
    let s = if hand == "left" { 1.0 } else { -1.0 };
    let tau = 2.0 * std::f64::consts::PI;
    // a Set of numbers: insertion order, SameValueZero
    let mut th: Vec<f64> = vec![0.0, tau];
    let mut add = |x: f64| {
        let x = if x == 0.0 { 0.0 } else { x };
        if !th.iter().any(|y| *y == x || (y.is_nan() && x.is_nan())) {
            th.push(x);
        }
    };
    for c in corners {
        add((((s * c[0] / p) % 1.0 + 1.0) % 1.0) * 2.0 * std::f64::consts::PI);
    }
    let n = js::max(24.0, segments);
    let nn = js::count(n.ceil(), 10_000_000);
    for k in 1..nn {
        let kf = js::f(k);
        if kf >= n {
            break;
        }
        add(tau * kf / n);
    }
    let mut sorted = th.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut list: Vec<f64> = Vec::with_capacity(sorted.len());
    for (i, a) in sorted.iter().enumerate() {
        if i == 0 || *a - sorted.get(i.saturating_sub(1)).copied().unwrap_or(f64::NAN) > 1e-9 {
            list.push(*a);
        }
    }
    if let Some(last) = list.last()
        && tau - last < 1e-9
    {
        list.pop();
    }
    list.iter()
        .map(|&a| {
            let r = profile_at(corners, s * p * a / tau);
            [r * js::cos(a), r * js::sin(a)]
        })
        .collect()
}

fn fmt3(v: f64) -> String {
    js::fmt_fixed(v, 3)
}

/// `threadCallout(form, size, { external, length, through, modelled })`.
pub fn thread_callout(form: &str, size: &str, external: bool, length: f64, through: bool, modelled: bool) -> Result<String, String> {
    let Some(g) = thread_dims_js(form, size)? else { return Ok(String::new()) };
    let name = if form == "tr" { size_label(form, size) } else { format!("{size}×{}", fmt3(g.p)) };
    let cls = if form == "tr" {
        if external { "7e" } else { "7H" }
    } else if external {
        "6g"
    } else {
        "6H"
    };
    let key = if through {
        "thread.callout.through"
    } else if external {
        "thread.callout.long"
    } else {
        "thread.callout.deep"
    };
    let text = t(key, &[("size", F::S(name)), ("cls", F::from(cls)), ("length", F::S(fmt3(length)))]);
    Ok(if modelled { t("thread.callout.modelled", &[("callout", F::S(text))]) } else { text })
}

/// `threadNotes(threads)`: one note per callout with how many there are.
pub fn thread_notes(threads: &[(String, Value)]) -> Vec<Value> {
    let mut out = Vec::new();
    for (_, th) in threads {
        match th.get("callouts").and_then(Value::as_array) {
            Some(cs) => out.extend(cs.iter().cloned()),
            None => out.push(jv::obj(vec![
                ("callout", th.get("callout").cloned().unwrap_or(Value::Null)),
                ("count", jv::unum(jv::arr(th.get("list")).len())),
            ])),
        }
    }
    out
}
