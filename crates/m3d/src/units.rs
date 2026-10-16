//! units.js, the parts a kernel caller needs without a page: the model's unit keys, metres per bare number, and the
//! length / area / volume text the model's numbers are shown with. (The value box's parser, parseLength / parseValue,
//! belongs to the tools layer and is not ported here.)

use crate::i18n::t;
use crate::js;

/// The inch (exact by definition) and the foot, in metres.
pub const IN: f64 = 0.0254;
pub const FT: f64 = 0.3048;

/// Metres per bare number in a unit (feet-and-inches: inches).
pub fn unit_scale(unit: &str) -> f64 {
    match unit {
        "ft-in" | "in" => IN,
        "m" => 1.0,
        "cm" => 0.01,
        "mm" => 0.001,
        _ => 1.0,
    }
}

/// `trim(x, d)`: x.toFixed(d) without trailing zeros (and point).
fn trim(x: f64, d: usize) -> String {
    let s = js::to_fixed(x, d);
    if !s.contains('.') {
        return s;
    }
    // /\.?0+$/
    let t = s.trim_end_matches('0');
    if t.len() == s.len() {
        return s;
    }
    t.strip_suffix('.').unwrap_or(t).to_string()
}
fn gcd(a: f64, b: f64) -> f64 {
    let (mut a, mut b) = (a, b);
    let mut guard = 0u32;
    while js::truthy(b) && guard < 2000 {
        let r = a % b;
        a = b;
        b = r;
        guard += 1;
    }
    a
}
/// Inches to "6 1/2" (nearest 1/16).
fn inch_text(inches: f64) -> String {
    let sx = js::round(inches * 16.0);
    let whole = (sx / 16.0).floor();
    let frac = sx % 16.0;
    if !js::truthy(frac) {
        return js::num_str(whole);
    }
    let g = gcd(frac, 16.0);
    let mut s = String::new();
    if js::truthy(whole) {
        js::push_num(&mut s, whole);
        s.push(' ');
    }
    js::push_num(&mut s, frac / g);
    s.push('/');
    js::push_num(&mut s, 16.0 / g);
    s
}

/// `formatLength(m, unit)`.
pub fn format_length(m: f64, unit: &str) -> String {
    if !m.is_finite() {
        return String::new();
    }
    let neg = m < -1e-9;
    let a = m.abs();
    let s = match unit {
        "ft-in" => {
            let total16 = js::round((a / IN) * 16.0);
            let ft = (total16 / (12.0 * 16.0)).floor();
            let rest = (total16 - ft * 12.0 * 16.0) / 16.0;
            if js::truthy(ft) { format!("{}' {}\"", js::num_str(ft), inch_text(rest)) } else { format!("{}\"", inch_text(rest)) }
        }
        "in" => format!("{}\"", inch_text(a / IN)),
        "mm" => format!("{} mm", trim(a * 1000.0, 1)),
        "cm" => format!("{} cm", trim(a * 100.0, 2)),
        _ => format!("{} m", trim(a, 3)),
    };
    if neg { format!("-{s}") } else { s }
}
/// `formatArea(a, unit)`.
pub fn format_area(a: f64, unit: &str) -> String {
    match unit {
        "ft-in" => t("m3d.units.sqft", &[("v", &trim(a / (FT * FT), 2))]),
        "in" => t("m3d.units.sqin", &[("v", &trim(a / (IN * IN), 2))]),
        "mm" => {
            if a.abs() >= 0.01 {
                format!("{} m²", trim(a, 3))
            } else {
                format!("{} mm²", trim(a * 1e6, 0))
            }
        }
        "cm" => format!("{} cm²", trim(a * 1e4, 1)),
        _ => format!("{} m²", trim(a, 3)),
    }
}
/// `formatVolume(v, unit)`.
pub fn format_volume(v: f64, unit: &str) -> String {
    use crate::jsmath::JsMath;
    match unit {
        "ft-in" => t("m3d.units.cuft", &[("v", &trim(v / FT.jpow(3.0), 2))]),
        "in" => t("m3d.units.cuin", &[("v", &trim(v / IN.jpow(3.0), 1))]),
        "mm" => {
            if v.abs() >= 0.01 {
                format!("{} m³", trim(v, 3))
            } else {
                format!("{} mm³", trim(v * 1e9, 0))
            }
        }
        "cm" => format!("{} cm³", trim(v * 1e6, 1)),
        _ => format!("{} m³", trim(v, 3)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lengths() {
        assert_eq!(format_length(3.2, "m"), "3.2 m");
        assert_eq!(format_length(0.3048 * 10.0 + 0.0254 * 6.5, "ft-in"), "10' 6 1/2\"");
        assert_eq!(format_length(-0.5, "mm"), "-500 mm");
        assert_eq!(format_area(12.0, "mm"), "12 m²");
        assert_eq!(format_volume(1.0, "cm"), "1000000 cm³");
    }
}
