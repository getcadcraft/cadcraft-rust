//! Check for 3D printing (0.5 SG.4): print3d.js's report on the selection or everything shown, as the dialog of
//! modelcraft.js print3d() words it (verdict, problems, warnings, size / volume / triangles in millimetres).

use cc_io_m3d::print3d::{Mesh, Report};
use serde_json::Value;

use crate::modelcraft::mt;

/// The report the panel shows.
#[derive(Clone, Debug)]
pub struct PrintReport {
    pub ok: bool,
    /// "selection" or "all"
    pub what: &'static str,
    pub size: Option<[f64; 3]>,
    pub volume: Option<f64>,
    pub triangles: f64,
    /// (kind, n)
    pub problems: Vec<(String, f64)>,
    /// (kind, n, share, angle)
    pub warnings: Vec<(String, f64, f64, f64)>,
    pub mesh: Mesh,
    /// how long the check took (ms)
    pub ms: f64,
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `x.toLocaleString('en')` for a number already rounded to tenths: grouping commas, up to three decimals.
pub fn locale_num(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "NaN".into()
        } else if x > 0.0 {
            "∞".into()
        } else {
            "-∞".into()
        };
    }
    let s = format!("{:.3}", x.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = frac.trim_end_matches('0');
    let mut g = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            g.push(',');
        }
        g.push(c);
    }
    let sign = if x < 0.0 && (int != "0" || !frac.is_empty()) { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{g}") } else { format!("{sign}{g}.{frac}") }
}

/// `mm(v)`: rounded to tenths, in the page's numbers.
pub fn mm(v: f64) -> String {
    locale_num((v * 10.0).round() / 10.0)
}

impl PrintReport {
    pub fn of(rep: Report, what: &'static str, ms: f64) -> PrintReport {
        let v = &rep.value;
        let size = v.get("size").and_then(Value::as_array).and_then(|a| Some([num(a.first())?, num(a.get(1))?, num(a.get(2))?]));
        let list = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let problems = list("problems")
            .iter()
            .map(|p| (p.get("kind").and_then(Value::as_str).unwrap_or("").to_string(), num(p.get("n")).unwrap_or(0.0)))
            .collect();
        let warnings = list("warnings")
            .iter()
            .map(|w| {
                (
                    w.get("kind").and_then(Value::as_str).unwrap_or("").to_string(),
                    num(w.get("n")).unwrap_or(0.0),
                    num(w.get("share")).unwrap_or(0.0),
                    num(w.get("angle")).filter(|a| *a != 0.0).unwrap_or(45.0),
                )
            })
            .collect();
        PrintReport {
            ok: rep.ok,
            what,
            size,
            volume: num(v.get("volume")),
            triangles: num(v.get("triangles")).unwrap_or(0.0),
            problems,
            warnings,
            mesh: rep.mesh,
            ms,
        }
    }

    /// The verdict line.
    pub fn verdict(&self) -> String {
        mt(if self.ok { "app.print3d.ready" } else { "app.print3d.notready" }, &[])
    }

    /// The problem lines.
    pub fn problem_lines(&self) -> Vec<String> {
        self.problems.iter().map(|(k, n)| mt(&format!("app.print3d.p.{k}"), &[("n", &cc_m3d::js::num_str(*n))])).collect()
    }

    /// The warning lines.
    pub fn warning_lines(&self) -> Vec<String> {
        self.warnings
            .iter()
            .map(|(k, n, share, angle)| {
                let sh = (share * 100.0).round().max(1.0);
                mt(
                    &format!("app.print3d.w.{k}"),
                    &[("n", &cc_m3d::js::num_str(*n)), ("share", &cc_m3d::js::num_str(sh)), ("angle", &cc_m3d::js::num_str(*angle))],
                )
            })
            .collect()
    }

    /// The facts: what prints, size, volume, triangles.
    pub fn fact_lines(&self) -> Vec<String> {
        let mut out = vec![mt(if self.what == "selection" { "app.print3d.what.sel" } else { "app.print3d.what.all" }, &[])];
        if let Some(s) = self.size {
            out.push(mt("app.print3d.size", &[("w", &mm(s[0])), ("d", &mm(s[1])), ("h", &mm(s[2]))]));
        }
        if let Some(v) = self.volume {
            out.push(mt("app.print3d.volume", &[("v", &mm(v / 1000.0))]));
        }
        out.push(mt("app.print3d.tris", &[("n", &cc_m3d::js::num_str(self.triangles))]));
        out
    }

    /// The fix hint shows when there are problems other than "empty".
    pub fn fix_hint(&self) -> bool {
        self.problems.first().is_some_and(|(k, _)| k != "empty")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers() {
        assert_eq!(locale_num(1234.5), "1,234.5");
        assert_eq!(locale_num(10.0), "10");
        assert_eq!(locale_num(0.0), "0");
        assert_eq!(locale_num(-3.2), "-3.2");
        assert_eq!(mm(999.96), "1,000");
        assert_eq!(mm(12.34), "12.3");
    }
}
