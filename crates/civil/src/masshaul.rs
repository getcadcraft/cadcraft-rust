//! masshaul.js — the mass-haul diagram (0.4 V24) from an alignment's section rows.

use crate::i18n::t;
use crate::js::{J, max, num_to_string, sign};
use crate::sections::Row;
use crate::tr;
use crate::units::{fixed, unit_of};
use serde_json::Value;

/// A span between balance points.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub from: f64,
    pub to: f64,
    pub volume: f64,
    pub ahead: bool,
    pub haul: f64,
}
impl J for Span {
    fn j(&self) -> Value {
        crate::jo! { "from" => self.from, "to" => self.to, "volume" => self.volume, "dir" => if self.ahead { "ahead" } else { "back" }, "haul" => self.haul }
    }
}
/// `massHaul(rows, { factor })`.
#[derive(Clone, Debug, Default)]
pub struct MassHaul {
    pub points: Vec<(f64, f64)>,
    pub balance: Vec<f64>,
    pub spans: Vec<Span>,
    pub cut: f64,
    pub cut_adj: f64,
    pub fill: f64,
    pub end: f64,
    pub kind: &'static str,
    pub factor: f64,
}
impl J for MassHaul {
    fn j(&self) -> Value {
        let pts: Vec<Value> = self.points.iter().map(|(s, m)| crate::jo! { "sta" => *s, "m" => *m }).collect();
        crate::jo! {
            "points" => pts, "balance" => self.balance, "spans" => self.spans,
            "totals" => crate::jo! { "cut" => self.cut, "cutAdj" => self.cut_adj, "fill" => self.fill, "end" => self.end, "kind" => self.kind },
            "factor" => self.factor,
        }
    }
}

fn or0(v: f64) -> f64 {
    if v != 0.0 && !v.is_nan() { v } else { 0.0 }
}

/// `massHaul(rows, { factor })`.
pub fn mass_haul(rows: &[Row], factor: f64) -> MassHaul {
    let k = if factor.is_finite() && factor > 0.0 && factor < 5.0 { factor } else { 1.0 };
    let mut points: Vec<(f64, f64)> = Vec::new();
    let (mut m, mut cut, mut fill) = (0.0f64, 0.0f64, 0.0f64);
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            m += or0(r.cut_vol) * k - or0(r.fill_vol);
            cut += or0(r.cut_vol);
            fill += or0(r.fill_vol);
        }
        points.push((r.sta, m));
    }
    let mut balance = Vec::new();
    let np = points.len();
    for i in 1..np {
        let (Some(a), Some(b)) = (points.get(i - 1), points.get(i)) else { continue };
        if (a.1 > 0.0 && b.1 < 0.0) || (a.1 < 0.0 && b.1 > 0.0) {
            balance.push(a.0 + ((b.0 - a.0) * a.1) / (a.1 - b.1));
        } else if b.1 == 0.0 && a.1 != 0.0 && i < np - 1 {
            balance.push(b.0);
        }
    }
    let mut cuts: Vec<f64> = Vec::new();
    if let Some(p) = points.first() {
        cuts.push(p.0);
    }
    cuts.extend(balance.iter().copied());
    if let Some(p) = points.last() {
        cuts.push(p.0);
    }
    let at = |s: f64| -> f64 {
        for i in 1..np {
            let (Some(a), Some(b)) = (points.get(i - 1), points.get(i)) else { continue };
            if s <= b.0 + 1e-9 {
                let f = if b.0 > a.0 { (s - a.0) / (b.0 - a.0) } else { 0.0 };
                return a.1 + (b.1 - a.1) * f;
            }
        }
        points.last().map_or(0.0, |p| p.1)
    };
    let mut spans = Vec::new();
    for j in 1..cuts.len() {
        let (Some(&s0), Some(&s1)) = (cuts.get(j - 1), cuts.get(j)) else { continue };
        if !(s1 > s0) {
            continue;
        }
        let mut pts: Vec<(f64, f64)> = vec![(s0, at(s0))];
        pts.extend(points.iter().filter(|p| p.0 > s0 + 1e-9 && p.0 < s1 - 1e-9).copied());
        pts.push((s1, at(s1)));
        let e = pts.last().map_or(0.0, |p| p.1);
        let mut peak = 0.0f64;
        for q in &pts {
            if q.1.abs() > peak.abs() {
                peak = q.1;
            }
        }
        let volume = peak.abs() - e.abs();
        if !(volume > 1e-9) {
            continue;
        }
        let sg = sign(peak);
        let ex = |v: f64| max(0.0, sg * v - e.abs());
        let mut area = 0.0;
        for w in pts.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let (fa, fb, l) = (ex(a.1), ex(b.1), b.0 - a.0);
            if fa > 0.0 && fb > 0.0 {
                area += ((fa + fb) / 2.0) * l;
            } else if fa > 0.0 || fb > 0.0 {
                let ga = sg * a.1 - e.abs();
                let gb = sg * b.1 - e.abs();
                let x = (ga / (ga - gb)) * l;
                area += if fa > 0.0 { (fa * x) / 2.0 } else { (fb * (l - x)) / 2.0 };
            }
        }
        spans.push(Span { from: s0, to: s1, volume, ahead: peak > 0.0, haul: area / volume });
    }
    let end = m;
    let kind = if end.abs() < 1e-6 * max(1.0, cut + fill) {
        "balanced"
    } else if end > 0.0 {
        "waste"
    } else {
        "borrow"
    };
    MassHaul { points, balance, spans, cut, cut_adj: cut * k, fill, end, kind, factor: k }
}

/// `massHaulCsv(res, { units, station })`.
pub fn mass_haul_csv(res: &MassHaul, units: &str, station: &dyn Fn(f64) -> String) -> String {
    let u = unit_of(units);
    let v = |x: f64| fixed(x / u.vol_per, 1).replace(',', "");
    let d = |x: f64| fixed(x, 2).replace(',', "");
    let mut rows: Vec<Vec<String>> = vec![vec![t("mh.c.sta", &[]), tr!("mh.c.m", u = u.vol)]];
    for p in &res.points {
        rows.push(vec![station(p.0), v(p.1)]);
    }
    rows.push(vec![]);
    rows.push(vec![
        t("mh.c.from", &[]),
        t("mh.c.to", &[]),
        tr!("mh.c.vol", u = u.vol),
        t("mh.c.dir", &[]),
        tr!("mh.c.haul", u = if units == "m" { "m" } else { "ft" }),
    ]);
    for s in &res.spans {
        rows.push(vec![
            station(s.from),
            station(s.to),
            v(s.volume),
            t(if s.ahead { "mh.dir.ahead" } else { "mh.dir.back" }, &[]),
            d(s.haul),
        ]);
    }
    rows.push(vec![]);
    rows.push(vec![t("mh.c.factor", &[]), num_to_string(res.factor)]);
    rows.push(vec![tr!("mh.c.end", u = u.vol), v(res.end)]);
    let lines: Vec<String> = rows
        .into_iter()
        .map(|r| {
            r.into_iter()
                .map(|x| if x.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", x.replace('"', "\"\"")) } else { x })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    format!("{}\r\n", lines.join("\r\n"))
}
