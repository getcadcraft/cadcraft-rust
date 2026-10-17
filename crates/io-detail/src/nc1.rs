//! nc1.js — DSTV NC1 files (DSTV "Standard Description for Steel Structure Pieces for the Numerical Controls", 7th
//! ed. 1998): the ST header, AK outer contours (copes notched), BO holes, EN; I-shapes, channels, angles, tubes and
//! plates; `ncRefusal`, and `readNc1`, the strict reader the tests use.

use cc_detail::build::Part;
use cc_detail::i18n::{F, t, t0};
use cc_detail::js::{num_to_string, str_to_number};
use cc_detail::state::Cut;
use serde_json::{Value, json};

const STEEL_KG_PER_M3: f64 = cc_detail::sections::STEEL_DENSITY;

fn f2(v: f64) -> String {
    crate::num::fixed(if v.abs() < 0.005 { 0.0 } else { v }, 2)
}
fn pad(s: &str, n: usize) -> String {
    let len = s.encode_utf16().count();
    if len >= n { s.to_string() } else { format!("{}{s}", " ".repeat(n.saturating_sub(len))) }
}
fn pad12(s: &str) -> String {
    pad(s, 12)
}
/// `txt(s)`: printable ASCII (each other UTF-16 unit → '?'), at most 50, '-' when empty.
pub fn txt(s: &str) -> String {
    let out: String = s
        .encode_utf16()
        .map(|u| if (0x20..=0x7e).contains(&u) { char::from(u8::try_from(u).unwrap_or(b'?')) } else { '?' })
        .take(50)
        .collect();
    if out.is_empty() { "-".into() } else { out }
}

fn code(kind: &str) -> Option<&'static str> {
    match kind {
        "I" => Some("I"),
        "C" => Some("U"),
        "L" => Some("L"),
        "RHS" => Some("M"),
        "CHS" => Some("RO"),
        "PL" => Some("B"),
        _ => None,
    }
}

/// `ncRefusal(part)`: None if the part can go to an NC file, else the reason.
pub fn nc_refusal(part: &Part) -> Option<String> {
    let k = part.profile.kind.as_str();
    if (k == "L" || k == "CHS") && part.cuts.iter().any(|c| c.kind == "cope") {
        let kind = t0(if k == "L" { "nc.kind.l" } else { "nc.kind.chs" });
        return Some(t("nc.refusal.cope", &[("profile", F::from(part.profile_name.as_str())), ("kind", F::from(kind))]));
    }
    if code(k).is_some() {
        return None;
    }
    Some(t("nc.refusal.profile", &[("profile", F::from(part.profile_name.as_str()))]))
}

fn web_outline(l: f64, h: f64, copes: &[&Cut]) -> Vec<[f64; 2]> {
    let cs = |end: &str, side: &str| copes.iter().find(|c| c.end == end && c.side == side).copied();
    let mut pts = Vec::new();
    let (bs, be, te, ts) = (cs("start", "bottom"), cs("end", "bottom"), cs("end", "top"), cs("start", "top"));
    match bs {
        Some(c) => pts.extend([[0.0, c.depth], [c.length, c.depth], [c.length, 0.0]]),
        None => pts.push([0.0, 0.0]),
    }
    match be {
        Some(c) => pts.extend([[l - c.length, 0.0], [l - c.length, c.depth], [l, c.depth]]),
        None => pts.push([l, 0.0]),
    }
    match te {
        Some(c) => pts.extend([[l, h - c.depth], [l - c.length, h - c.depth], [l - c.length, h]]),
        None => pts.push([l, h]),
    }
    match ts {
        Some(c) => pts.extend([[c.length, h], [c.length, h - c.depth], [0.0, h - c.depth]]),
        None => pts.push([0.0, h]),
    }
    pts
}
fn flange_span(l: f64, copes: &[&Cut], side: &str, tk: f64) -> (f64, f64) {
    let a = copes.iter().find(|c| c.end == "start" && c.side == side);
    let b = copes.iter().find(|c| c.end == "end" && c.side == side);
    (a.filter(|a| a.depth >= tk).map_or(0.0, |a| a.length), b.filter(|b| b.depth >= tk).map_or(l, |b| l - b.length))
}

/// `info` of `writeNc1`: order, mark, count, phase (as the JS values, `String(…)` / `||` applied here).
pub struct NcInfo {
    pub order: String,
    pub mark: String,
    pub count: String,
    pub phase: String,
}

/// `writeNc1(part, info)`.
pub fn write_nc1(part: &Part, info: &NcInfo) -> String {
    let p = &part.profile;
    let l = part.length;
    let k = p.kind.as_str();
    let is_plate = k == "PL";
    let mut lines: Vec<String> = vec!["ST".into(), format!("** DetailCraft {}.nc1", txt(&info.mark))];
    let field = |lines: &mut Vec<String>, v: &str| lines.push(format!("  {v}"));
    field(&mut lines, &txt(&info.order));
    field(&mut lines, &txt(&info.mark));
    field(&mut lines, &txt(&info.phase));
    field(&mut lines, &txt(&info.mark));
    field(&mut lines, &txt(&part.material));
    field(&mut lines, &info.count);
    let prof = if is_plate { format!("PL{}*{}", num_to_string(p.t()), num_to_string(p.b())) } else { part.profile_name.clone() };
    field(&mut lines, &txt(&prof));
    field(&mut lines, code(k).unwrap_or("undefined"));
    let per = p.perimeter / 1000.0;
    let nums: [f64; 8] = if is_plate {
        [l, p.b(), 0.0, 0.0, p.t(), 0.0, p.t() * STEEL_KG_PER_M3 / 1000.0, 2.0]
    } else if k == "L" {
        [l, p.b(), p.b(), p.t(), p.t(), 0.0, p.mass, per]
    } else if k == "RHS" {
        [l, p.h.unwrap_or(f64::NAN), p.b(), p.t(), p.t(), p.ro(), p.mass, per]
    } else if k == "CHS" {
        [l, p.big_d(), p.big_d(), p.t(), p.t(), 0.0, p.mass, per]
    } else {
        [l, p.d(), p.bf(), p.tf(), p.tw(), p.r(), p.mass, per]
    };
    for n in nums {
        lines.push(pad12(&f2(n)));
    }
    for _ in 0..4 {
        lines.push(pad12("0.00000"));
    }
    for _ in 0..4 {
        field(&mut lines, "-");
    }
    let ak = |lines: &mut Vec<String>, face: &str, pts: &[[f64; 2]]| {
        lines.push("AK".into());
        for q in pts.iter().chain(pts.first()) {
            lines.push(format!("  {face}{}{}{}", pad12(&f2(q[0])), pad12(&f2(q[1])), pad12("0.00")));
        }
    };
    let copes: Vec<&Cut> = part.cuts.iter().filter(|c| c.kind == "cope").collect();
    let rect = |lines: &mut Vec<String>, face: &str, x0: f64, x1: f64, w: f64| ak(lines, face, &[[x0, 0.0], [x1, 0.0], [x1, w], [x0, w]]);
    if is_plate {
        rect(&mut lines, "v", 0.0, l, p.b());
    } else if k == "L" {
        rect(&mut lines, "v", 0.0, l, p.b());
        rect(&mut lines, "u", 0.0, l, p.b());
    } else if k == "CHS" {
        rect(&mut lines, "v", 0.0, l, p.big_d());
    } else if k == "C" {
        let mut o: Vec<[f64; 2]> = web_outline(l, p.d(), &copes).iter().map(|q| [q[0], p.d() - q[1]]).collect();
        o.reverse();
        ak(&mut lines, "v", &o);
        for (face, side) in [("o", "bottom"), ("u", "top")] {
            let (x0, x1) = flange_span(l, &copes, side, p.tf());
            rect(&mut lines, face, x0, x1, p.bf());
        }
    } else {
        let h = if k == "RHS" { p.h.unwrap_or(f64::NAN) } else { p.d() };
        let tf = if k == "RHS" { p.t() } else { p.tf() };
        let w = if k == "RHS" { p.b() } else { p.bf() };
        ak(&mut lines, "v", &web_outline(l, h, &copes));
        if k == "RHS" {
            ak(&mut lines, "h", &web_outline(l, h, &copes));
        }
        for (face, side) in [("o", "top"), ("u", "bottom")] {
            let (x0, x1) = flange_span(l, &copes, side, tf);
            rect(&mut lines, face, x0, x1, w);
        }
    }
    let mut holes: Vec<(&str, f64, f64, f64)> = Vec::new();
    for hh in &part.holes {
        if is_plate {
            holes.push(("v", hh.x, hh.y + p.b() / 2.0, hh.d));
        } else if k == "C" {
            if hh.face == "web" {
                holes.push(("v", hh.x, p.d() / 2.0 - hh.y, hh.d));
            } else {
                holes.push((if hh.face == "top" { "u" } else { "o" }, hh.x, hh.y + p.bf() / 2.0, hh.d));
            }
        } else if k == "RHS" {
            let hgt = p.h.unwrap_or(f64::NAN);
            if hh.face == "web" {
                holes.push(("v", hh.x, hh.y + hgt / 2.0, hh.d));
                holes.push(("h", hh.x, hh.y + hgt / 2.0, hh.d));
            } else {
                holes.push((if hh.face == "top" { "o" } else { "u" }, hh.x, p.b() / 2.0 - hh.y, hh.d));
            }
        } else if k == "L" {
            holes.push((if hh.face == "web" { "u" } else { "v" }, hh.x, hh.y + p.b() / 2.0, hh.d));
        } else if hh.face == "web" {
            holes.push(("v", hh.x, hh.y + p.d() / 2.0, hh.d));
        } else {
            holes.push((if hh.face == "top" { "o" } else { "u" }, hh.x, p.bf() / 2.0 - hh.y, hh.d));
        }
    }
    if !holes.is_empty() {
        lines.push("BO".into());
        for (face, x, y, d) in holes {
            lines.push(format!("  {face}{}{}{}{}", pad12(&f2(x)), pad12(&f2(y)), pad12(&f2(d)), pad12(&f2(0.0))));
        }
    }
    lines.push("EN".into());
    format!("{}\r\n", lines.join("\r\n"))
}

fn is_ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}
fn line_term(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}
fn two_upper(l: &str) -> bool {
    let b = l.as_bytes();
    b.len() == 2 && b.iter().all(u8::is_ascii_uppercase)
}
/// /^\s+([vohu])\s+(.*)$/ → (face, rest).
fn row_match(l: &str) -> Option<(char, &str)> {
    let after = l.trim_start_matches(is_ws);
    if after.len() == l.len() {
        return None;
    }
    let mut it = after.char_indices();
    let (_, face) = it.next()?;
    if !matches!(face, 'v' | 'o' | 'h' | 'u') {
        return None;
    }
    let rest0 = after.get(face.len_utf8()..)?;
    let rest = rest0.trim_start_matches(is_ws);
    if rest.len() == rest0.len() || rest.chars().any(line_term) {
        return None;
    }
    Some((face, rest))
}

/// The most rows and blocks a strict read takes (a file far beyond any part is refused, not walked).
const MAX_LINES: usize = 1_000_000;

/// `readNc1(text)` → `{ header, blocks }`, or the refusal.
pub fn read_nc1(text: &str) -> Result<Value, String> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !cc_cad::js::trim(l).is_empty() && !l.starts_with("**"))
        .collect();
    if lines.len() > MAX_LINES {
        return Err(t0("nc.read.start"));
    }
    if lines.first() != Some(&"ST") {
        return Err(t0("nc.read.start"));
    }
    let mut header: Vec<String> = Vec::new();
    let mut i = 1usize;
    while let Some(l) = lines.get(i) {
        if two_upper(l) {
            break;
        }
        header.push(cc_cad::js::trim(l).to_string());
        i = i.saturating_add(1);
    }
    if header.len() < 20 {
        return Err(t("nc.read.fields", &[("n", F::N(header.len() as f64)), ("min", F::N(20.0))]));
    }
    let hs = |k: usize| header.get(k).cloned().unwrap_or_default();
    let hn = |k: usize| cc_detail::js::jnum(str_to_number(&hs(k)));
    let h = json!({
        "order": hs(0), "drawing": hs(1), "phase": hs(2), "piece": hs(3), "steel": hs(4), "quantity": hn(5),
        "profile": hs(6), "code": hs(7), "length": hn(8), "height": hn(9), "flangeWidth": hn(10),
        "flangeThickness": hn(11), "webThickness": hn(12), "radius": hn(13), "kgPerM": hn(14), "paintPerM": hn(15),
    });
    let mut blocks: Vec<(String, Vec<Value>)> = Vec::new();
    let mut cur: Option<usize> = None;
    while let Some(l) = lines.get(i) {
        i = i.saturating_add(1);
        if two_upper(l) {
            if *l == "EN" {
                blocks.push(("EN".into(), vec![]));
                cur = None;
                continue;
            }
            blocks.push(((*l).to_string(), vec![]));
            cur = Some(blocks.len().saturating_sub(1));
            continue;
        }
        let Some(ci) = cur else { return Err(t("nc.read.outside", &[("line", F::from(*l))])) };
        let bt = blocks.get(ci).map(|b| b.0.clone()).unwrap_or_default();
        let Some((face, rest)) = row_match(l) else { return Err(t("nc.read.bad-line", &[("block", F::from(bt)), ("line", F::from(*l))])) };
        let trimmed = cc_cad::js::trim(rest);
        let nums: Vec<f64> = trimmed
            .split(is_ws)
            .filter(|s| !(s.is_empty() && !trimmed.is_empty()))
            .map(|s| {
                let s = match s.chars().last() {
                    Some(c) if c.is_ascii_lowercase() => s.get(..s.len().saturating_sub(1)).unwrap_or(""),
                    _ => s,
                };
                str_to_number(s)
            })
            .collect();
        if nums.iter().any(|n| !n.is_finite()) {
            return Err(t("nc.read.nan", &[("block", F::from(bt)), ("line", F::from(*l))]));
        }
        if let Some(b) = blocks.get_mut(ci) {
            b.1.push(json!({ "face": face.to_string(), "nums": nums }));
        }
    }
    if blocks.last().is_none_or(|b| b.0 != "EN") {
        return Err(t0("nc.read.no-en"));
    }
    let blocks: Vec<Value> = blocks.into_iter().map(|(t, rows)| json!({ "type": t, "rows": rows })).collect();
    Ok(json!({ "header": h, "blocks": blocks }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_reader() {
        assert!(read_nc1("").is_err());
        assert!(read_nc1("ST\nA\nEN").is_err());
        let head = "ST\n".to_string() + &"  x\n".repeat(20);
        let ok = read_nc1(&(head.clone() + "BO\n  v  1.00s  2.00\nEN\n")).unwrap();
        assert_eq!(ok["blocks"][0]["rows"][0]["nums"][0], 1.0);
        assert!(read_nc1(&(head.clone() + "BO\n  z  1 2\nEN\n")).is_err());
        assert!(read_nc1(&(head + "BO\n  v  1 2\n")).is_err());
        assert_eq!(txt("é😀"), "???");
        assert_eq!(txt(""), "-");
    }
}
