//! Colours (AutoCAD Color Index) and type names: doc.js's `aciHex`, `nearestAci`, `resolveColor`, `typeName`.

use crate::entity::{Color, Entity};
use crate::js;

const ACI_BASE: [&str; 9] = ["#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff", "#ffffff", "#808080", "#c0c0c0"];
const ACI_GRAYS: [&str; 6] = ["#333333", "#505050", "#696969", "#828282", "#bebebe", "#ffffff"];

/// An integral ACI number as an index (None for fractions and out-of-range values).
fn idx(i: f64) -> Option<usize> {
    if i.fract() != 0.0 || !(0.0..=1000.0).contains(&i) {
        return None;
    }
    // integral and in 0..=1000: exact as usize
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = i as usize;
    Some(u)
}

/// '#rrggbb' for an ACI number; ACI 7 is 'ink' (black on paper, white on a dark screen). Fractions (no file
/// writes them) give 'ink' here; the JS gives undefined or '#NaNNaNNaN'.
pub fn aci_hex(i: f64) -> String {
    if i == 7.0 {
        return "ink".into();
    }
    let Some(u) = idx(i) else { return "ink".into() };
    if (1..=9).contains(&u) {
        return ACI_BASE.get(u.saturating_sub(1)).copied().unwrap_or("ink").into();
    }
    if (250..=255).contains(&u) {
        return ACI_GRAYS.get(u.saturating_sub(250)).copied().unwrap_or("ink").into();
    }
    if !(10..=249).contains(&u) {
        return "ink".into();
    }
    let hue = ((i - 10.0) / 10.0).floor() * 15.0;
    let j = u.saturating_sub(10) % 10;
    let v = [1.0, 1.0, 0.8, 0.8, 0.6, 0.6, 0.5, 0.5, 0.3, 0.3].get(j).copied().unwrap_or(1.0);
    let s = if j % 2 == 0 { 1.0 } else { 0.5 };
    hsv_hex(hue, s, v)
}

fn hsv_hex(h: f64, s: f64, v: f64) -> String {
    let f = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * js::max(0.0, js::min(js::min(k, 4.0 - k), 1.0))
    };
    let mut out = String::from("#");
    for x in [f(5.0), f(3.0), f(1.0)] {
        let c = js::round(x * 255.0);
        let c = if (0.0..=255.0).contains(&c) { c } else { 0.0 };
        // 0..=255 integral: exact as u8
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b = c as u8;
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `parseInt(h.slice(i, i + 2), 16)`: the leading hex digits, NaN when there are none.
fn hex2(h: &str, i: usize) -> f64 {
    let part: String = h.chars().skip(i).take(2).collect();
    let digits: String = part.chars().take_while(char::is_ascii_hexdigit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    f64::from(u32::from_str_radix(&digits, 16).unwrap_or(0))
}

/// The nearest ACI (1–255) for a '#rrggbb' (R12 DXF has no true colour).
pub fn nearest_aci(hex: &str) -> u32 {
    let (r, g, b) = (hex2(hex, 1), hex2(hex, 3), hex2(hex, 5));
    let mut best = 7u32;
    let mut bd = f64::INFINITY;
    for i in 1..=255u32 {
        let a = aci_hex(f64::from(i));
        let c = if a == "ink" { "#ffffff".to_string() } else { a };
        let (r2, g2, b2) = (hex2(&c, 1), hex2(&c, 3), hex2(&c, 5));
        let d = (r - r2).powi(2) + (g - g2).powi(2) + (b - b2).powi(2);
        if d < bd {
            bd = d;
            best = i;
        }
    }
    best
}

/// The colour an entity draws in: 'ink' or '#rrggbb'. `by_block` is the parent reference's resolved colour,
/// `layer_color(name)` the colour of a layer (None = no such layer).
pub fn resolve_color(
    e: &Entity,
    by_block: Option<&Color>,
    parent_layer: Option<&str>,
    layer_color: &dyn Fn(&str) -> Option<Color>,
) -> String {
    let mut c = Color::or_null(&e.color);
    if c == Color::Num(0.0) {
        c = by_block.cloned().unwrap_or(Color::Null);
    }
    if matches!(c, Color::Null) || c == Color::Num(256.0) {
        let lname = match (e.layer.as_deref(), parent_layer) {
            (Some("0"), Some(p)) if !p.is_empty() => p,
            (l, _) => l.unwrap_or(""),
        };
        c = layer_color(lname).filter(|c| !matches!(c, Color::Null)).unwrap_or(Color::Num(7.0));
    }
    match c {
        Color::Str(s) => s,
        Color::Num(n) => aci_hex(n),
        Color::Null => "ink".into(),
    }
}

/// The types with a name for people (`type.<k>` in the language catalogs).
pub const TYPE_KEYS: [&str; 14] =
    ["line", "circle", "arc", "poly", "ellipse", "text", "point", "insert", "hatch", "dim", "viewport", "mleader", "xline", "ray"];

/// What an entity is called (Properties title, LIST): `tr("type.<k>")` for the known types, else its type.
/// (Add-on name hooks are not ported.)
pub fn type_name(e: &Entity, tr: &dyn Fn(&str) -> String) -> String {
    let t = e.typ();
    if TYPE_KEYS.contains(&t) { tr(&format!("type.{t}")) } else { t.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aci() {
        assert_eq!(aci_hex(1.0), "#ff0000");
        assert_eq!(aci_hex(7.0), "ink");
        assert_eq!(aci_hex(10.0), "#ff0000");
        assert_eq!(aci_hex(253.0), "#828282");
        assert_eq!(nearest_aci("#ff0000"), 1);
        assert_eq!(nearest_aci("#fefefe"), 7);
    }
}
