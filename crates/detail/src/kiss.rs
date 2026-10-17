//! kiss.js — the bill of materials as a KISS 1.1 file for fabrication software (0.4 D16): identification line, H
//! header, per assembly D / W / M / S / L lines and its attached parts, loose bolts and anchor rods on BOLTS / FIELD;
//! `readKiss`, a strict reader for the tests.

use crate::build::{Built, Part};
use crate::drawings::assembly_geometry;
use crate::js::{Obj, num_to_string, round, slice16, str_to_number, to_fixed, trim};
use crate::marks::{Marks, bolt_list};
use crate::profiles::Profile;
use serde_json::{Value, json};

const KG_LB: f64 = 2.20462262185;
const IN: f64 = 25.4;

/// `clean(s, n)`.
pub fn clean(s: &str, n: usize) -> String {
    let a: String =
        s.chars().map(|c| if c == ',' || c == '\r' || c == '\n' || (c as u32) < 0x20 || c == '\u{7f}' { ' ' } else { c }).collect();
    let mut b = String::with_capacity(a.len());
    let mut ws = false;
    for c in a.chars() {
        if cc_cad::js::is_ws(c) {
            if !ws {
                b.push(' ');
            }
            ws = true;
        } else {
            b.push(c);
            ws = false;
        }
    }
    slice16(trim(&b), n)
}
fn f2(v: f64) -> String {
    to_fixed(round(v * 100.0) / 100.0, 2)
}
fn pad2(n: u32) -> String {
    format!("{n:02}")
}

/// `inchFraction(inches, denom)`.
pub fn inch_fraction(inches: f64, denom: f64) -> String {
    let n = round(inches * denom);
    let whole = (n / denom).floor();
    let mut num = n - whole * denom;
    let mut den = denom;
    while num != 0.0 && num % 2.0 == 0.0 {
        num /= 2.0;
        den /= 2.0;
    }
    if num == 0.0 {
        return num_to_string(whole);
    }
    format!(
        "{}{}/{}",
        if whole != 0.0 { format!("{}-", num_to_string(whole)) } else { String::new() },
        num_to_string(num),
        num_to_string(den)
    )
}
fn dec_wall(s: &str) -> String {
    let frac = s
        .split_once('/')
        .filter(|(a, b)| !a.is_empty() && !b.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()));
    let v = match frac {
        Some((a, b)) => str_to_number(a) / str_to_number(b),
        None => str_to_number(s),
    };
    if v.is_finite() {
        let t = to_fixed(v, 3);
        t.strip_prefix('0').map_or(t.clone(), str::to_string)
    } else {
        s.to_string()
    }
}

/// `kissMetric(built)`.
pub fn kiss_metric(built: &Built) -> bool {
    built.parts.iter().any(|p| !(p.profile.source == "aisc" || p.profile.kind == "PL"))
}
fn f2c(v: f64) -> String {
    num_to_string(round(v * 100.0) / 100.0)
}
/// `kissShape(profile, metric)` → (type, size).
pub fn kiss_shape(p: &Profile, metric: bool) -> (String, String) {
    let name = p.name.to_uppercase();
    if p.kind == "PL" {
        let (tk, w) = (p.t(), p.b());
        return (
            "PL".into(),
            if metric {
                format!("{}X{}", f2c(tk), f2c(w))
            } else {
                format!("{}X{}", inch_fraction(tk / IN, 16.0), inch_fraction(w / IN, 16.0))
            },
        );
    }
    if p.source == "aisc" {
        for pre in ["W", "S", "C", "L", "HSS"] {
            if let Some(rest) = name.strip_prefix(pre) {
                if pre == "HSS" {
                    let mut parts: Vec<String> = rest.split('X').map(str::to_string).collect();
                    if let Some(last) = parts.last_mut() {
                        *last = dec_wall(last);
                    }
                    return ("HSS".into(), parts.join("X"));
                }
                return (pre.into(), rest.to_string());
            }
        }
    }
    if p.source == "en" {
        return (if p.kind == "C" { "C" } else { "W" }.into(), name);
    }
    ("MI".into(), name)
}
fn grade(g: &str) -> String {
    // /\s+Gr\s+/i → '-' (first), then every white space run removed
    let cs: Vec<char> = g.chars().collect();
    let mut out = String::new();
    let mut done = false;
    let mut i = 0usize;
    while i < cs.len() {
        if !done && cs.get(i).is_some_and(|c| cc_cad::js::is_ws(*c)) {
            let mut j = i;
            while cs.get(j).is_some_and(|c| cc_cad::js::is_ws(*c)) {
                j += 1;
            }
            let gr = cs.get(j).is_some_and(|c| *c == 'G' || *c == 'g') && cs.get(j + 1).is_some_and(|c| *c == 'R' || *c == 'r');
            let mut k = j + 2;
            if gr && cs.get(k).is_some_and(|c| cc_cad::js::is_ws(*c)) {
                while cs.get(k).is_some_and(|c| cc_cad::js::is_ws(*c)) {
                    k += 1;
                }
                out.push('-');
                done = true;
                i = k;
                continue;
            }
        }
        if let Some(c) = cs.get(i) {
            out.push(*c);
        }
        i += 1;
    }
    let squeezed: String = out.chars().filter(|c| !cc_cad::js::is_ws(*c)).collect();
    clean(&squeezed, 8)
}
fn role_of(kind: &str) -> Option<&'static str> {
    match kind {
        "beam" => Some("Beam"),
        "column" => Some("Column"),
        "brace" => Some("Brace"),
        "plate" => Some("Plate"),
        _ => None,
    }
}
fn asm_type(kind: &str) -> &'static str {
    match kind {
        "beam" => "B",
        "column" => "C",
        "brace" => "BR",
        "plate" => "P",
        _ => "",
    }
}

/// The local date and time the file carries.
#[derive(Clone, Copy, Debug)]
pub struct When {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `kissFile(...)`.
pub struct Kiss {
    pub text: String,
    pub lines: Vec<String>,
    pub counts: Vec<(String, usize)>,
    pub metric: bool,
    pub job: String,
}

pub fn kiss_file(
    state: &Value,
    built: &Built,
    marks: &Marks,
    metric: Option<bool>,
    date: When,
    job: &str,
    revs: Option<&[(String, String)]>,
) -> Kiss {
    let metric = metric.unwrap_or_else(|| kiss_metric(built));
    let mut lines: Vec<String> = Vec::new();
    let mut counts: Vec<(String, usize)> = ["D", "M", "W", "S", "L"].iter().map(|k| ((*k).to_string(), 0)).collect();
    let mut put = |f: Vec<String>| {
        let l = slice16(&f.join(","), 254);
        lines.push(l);
        let k = f.first().cloned().unwrap_or_default();
        match counts.iter_mut().find(|x| x.0 == k) {
            Some(e) => e.1 = e.1.saturating_add(1),
            None => counts.push((k, 1)),
        }
    };
    let mmddyy = format!("{}/{}/{}", pad2(date.month), pad2(date.day), pad2(u32::try_from(date.year.rem_euclid(100)).unwrap_or(0)));
    let hhmmss = format!("{}:{}:{}", pad2(date.hour), pad2(date.minute), pad2(date.second));
    let name = crate::js::string_or_empty(state.get("name"));
    let src = if !job.is_empty() {
        job.to_string()
    } else if crate::js::truthy(state.get("job")) {
        crate::js::string_or_empty(state.get("job"))
    } else {
        name.clone()
    };
    let j1: String =
        clean(&src, 50).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect::<String>().to_uppercase();
    let job_no = if j1.is_empty() { "JOB".to_string() } else { slice16(&j1, 8) };
    let wf = |kg: f64| f2(if metric { kg } else { kg * KG_LB });
    let area = |m2: f64| f2(if metric { m2 } else { m2 * 10.7639104167 });
    let s = |x: &str| x.to_string();
    put(vec![s("KISS"), s("1.1"), s("DetailCraft")]);
    put(vec![
        s("H"),
        job_no.clone(),
        clean(&name, 50),
        String::new(),
        mmddyy.clone(),
        hhmmss,
        s(if metric { "T" } else { "F" }),
        s("Final Bill"),
    ]);
    let labor = |put: &mut dyn FnMut(Vec<String>), part: &Part| {
        let mut holes: Vec<(String, usize)> = Vec::new();
        let pr = &part.profile;
        for h in &part.holes {
            let tk = if pr.kind == "PL" {
                pr.t()
            } else if h.face == "web" {
                if pr.kind == "RHS" { pr.t() * 2.0 } else { pr.tw() }
            } else if pr.kind == "RHS" {
                pr.t()
            } else {
                pr.tf()
            };
            let k = format!("{}|{}", f2(h.d), f2(tk));
            match holes.iter_mut().find(|x| x.0 == k) {
                Some(e) => e.1 = e.1.saturating_add(1),
                None => holes.push((k, 1)),
            }
        }
        for (k, n) in holes {
            let (dd, tk) = k.split_once('|').unwrap_or((k.as_str(), ""));
            put(vec![s("L"), s("Holes"), n.to_string(), dd.to_string(), tk.to_string(), s("Round")]);
        }
        for c in part.cuts.iter().filter(|x| x.kind == "cope") {
            put(vec![s("L"), s("Copes"), s("1"), f2(c.depth), f2(c.length), clean(&format!("Cope {} at {}", c.side, c.end), 40)]);
        }
    };
    let dline = |put: &mut dyn FnMut(Vec<String>), dwg: &str, rev: &str, asm: &str, mark: &str, qty: f64, part: &Part| {
        let (ty, size) = kiss_shape(&part.profile, metric);
        let role = part.role.clone().filter(|r| !r.is_empty()).or_else(|| role_of(&part.kind).map(str::to_string)).unwrap_or_default();
        let pname = part.name.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| role.clone());
        put(vec![
            s("D"),
            clean(dwg, 30),
            clean(rev, 5),
            clean(asm, 16),
            clean(mark, 16),
            num_to_string(round(qty)),
            ty,
            clean(&size, 24),
            grade(&part.material),
            f2(part.length),
            String::new(),
            clean(&role, 40),
            clean(&pname, 30),
            String::new(),
            wf(part.weight),
            String::new(),
            if part.profile.kind == "PL" { area(part.profile.b() * part.length / 1e6) } else { String::new() },
        ]);
    };
    for a in &marks.assemblies {
        let Some(main) = a.ids.first().and_then(|id| built.get(id)) else { continue };
        let Some(g) = assembly_geometry(built, marks, &main.id) else { continue };
        let dwg = format!("A-{}", a.mark);
        let rev = revs.and_then(|r| r.iter().rev().find(|x| x.0 == dwg)).map(|x| x.1.clone()).filter(|r| !r.is_empty()).unwrap_or_default();
        dline(&mut put, &dwg, &rev, &a.mark, &a.main, a.count as f64, main);
        put(vec![s("W"), clean(&dwg, 30), clean(&rev, 5), clean(&format!("Assembly {}", a.mark), 30), mmddyy.clone(), s("DC")]);
        let [ex, eu, ev] = g.ext;
        let role = role_of(&main.kind).unwrap_or("undefined");
        put(vec![
            s("M"),
            clean(&a.mark, 20),
            a.count.to_string(),
            clean(&format!("{role} {}", main.profile_name), 30),
            s(asm_type(&main.kind)),
            clean(&format!("{role} assembly"), 50),
            String::new(),
            wf(a.weight),
            String::new(),
            f2(ex[1] - ex[0]),
            f2(ev[1] - ev[0]),
            f2(eu[1] - eu[0]),
        ]);
        let mut seq: Vec<(f64, usize)> = Vec::new();
        for id in &a.ids {
            let ph = built.get(id).map_or(1.0, |p| crate::js::or(p.phase, 1.0));
            match seq.iter_mut().find(|x| x.0 == ph) {
                Some(e) => e.1 = e.1.saturating_add(1),
                None => seq.push((ph, 1)),
            }
        }
        seq.sort_by(|x, y| crate::js::ord(x.0 - y.0));
        for (ph, n) in seq {
            let p = num_to_string(ph);
            put(vec![s("S"), clean(&p, 10), n.to_string(), clean(&format!("Phase {p}"), 16)]);
        }
        labor(&mut put, main);
        let mut welds: Vec<(String, usize)> = Vec::new();
        for w in &built.welds {
            if !(w.between.contains(&main.id) || w.between.iter().any(|id| marks.asm_of(id) == Some(main.id.as_str()))) {
                continue;
            }
            let k = format!("{}|{}", f2(crate::js::or(w.length, 0.0)), f2(crate::js::or(w.size, 0.0)));
            match welds.iter_mut().find(|x| x.0 == k) {
                Some(e) => e.1 = e.1.saturating_add(1),
                None => welds.push((k, 1)),
            }
        }
        for (k, n) in welds {
            let (len, size) = k.split_once('|').unwrap_or((k.as_str(), ""));
            put(vec![s("L"), s("Weld"), n.to_string(), len.to_string(), size.to_string(), s("Fillet")]);
        }
        let mut subs: Vec<(String, usize, usize)> = Vec::new();
        for &si in &g.subs {
            let Some(sp) = built.parts.get(si) else { continue };
            let m = marks.part_mark(&sp.id).unwrap_or("").to_string();
            match subs.iter_mut().find(|x| x.0 == m) {
                Some(e) => e.2 = e.2.saturating_add(1),
                None => subs.push((m, si, 1)),
            }
        }
        for (m, si, n) in subs {
            let Some(sp) = built.parts.get(si) else { continue };
            dline(&mut put, &dwg, &rev, &a.mark, &m, n as f64 * a.count as f64, sp);
            labor(&mut put, sp);
        }
    }
    for (i, b) in bolt_list(&built.bolts).iter().enumerate() {
        let anchor = b.kind == "ANCHORBOLT";
        let size = if metric {
            format!("{}X{}", num_to_string(b.d), num_to_string(b.length))
        } else {
            format!("{}X{}", inch_fraction(b.d / IN, 16.0), inch_fraction(b.length / IN, 16.0))
        };
        put(vec![
            s("D"),
            s("BOLTS"),
            String::new(),
            s("FIELD"),
            format!("{}{}", if anchor { "AB" } else { "HS" }, i + 1),
            b.count.to_string(),
            s(if anchor { "AB" } else { "HS" }),
            clean(&size, 24),
            grade(&b.grade),
            f2(b.length),
            String::new(),
            s(if anchor { "Anchor rod - Field" } else { "Bolt - Field" }),
            s(if anchor { "Anchor rod" } else { "Bolt" }),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
    }
    let text = format!("{}\r\n", lines.join("\r\n"));
    Kiss { text, lines, counts, metric, job: job_no }
}

/// The JSON of a KISS file result.
pub fn kiss_json(k: &Kiss) -> Value {
    let mut c = serde_json::Map::new();
    for (key, n) in &k.counts {
        c.insert(key.clone(), Value::from(*n));
    }
    Obj::new()
        .set("text", k.text.as_str())
        .set("lines", k.lines.clone())
        .set("counts", Value::Object(c))
        .set("metric", k.metric)
        .set("job", k.job.as_str())
        .done()
}

/// `readKiss(text)` → (records, problems).
pub fn read_kiss(text: &str) -> (Vec<Vec<String>>, Vec<String>) {
    let mut problems = Vec::new();
    let mut records = Vec::new();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !l.is_empty()).collect();
    let first = lines.first().copied().unwrap_or("");
    let ok_first = first
        .strip_prefix("KISS,1.")
        .is_some_and(|r| (r.starts_with('0') || r.starts_with('1')) && (r.len() == 1 || r.get(1..2) == Some(",")));
    if !ok_first {
        problems.push("first line is not KISS,1.x".to_string());
    }
    let lim = |k: &str| -> Option<&'static [usize]> {
        Some(match k {
            "H" => &[1, 8, 50, 40, 8, 8, 1, 20],
            "D" => &[1, 30, 5, 16, 16, 4, 3, 24, 8, 12, 24, 254, 30, 6, 10, 10, 10],
            "M" => &[1, 20, 6, 30, 4, 50, 6, 10, 10, 10, 10, 10],
            "W" => &[1, 30, 5, 30, 8, 8],
            "S" => &[1, 10, 4, 16],
            "L" => &[1, 15, 6, 12, 12, 40],
            _ => return None,
        })
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    let dec2 = |s: &str| s.split_once('.').is_some_and(|(a, b)| digits(a) && b.len() == 2 && digits(b));
    for (i, l) in lines.iter().enumerate() {
        let ln = i + 1;
        if l.encode_utf16().count() > 254 {
            problems.push(format!("line {ln} is longer than 254 characters"));
        }
        let f: Vec<String> = l.split(',').map(str::to_string).collect();
        let g = |k: usize| f.get(k).map_or("", String::as_str);
        let lm = lim(g(0));
        if i > 0 && lm.is_none() && g(0) != "*" {
            problems.push(format!("line {ln}: unknown record {}", g(0)));
        }
        if let Some(lm) = lm {
            for (k, v) in f.iter().enumerate() {
                if let Some(m) = lm.get(k)
                    && v.encode_utf16().count() > *m
                {
                    problems.push(format!("line {ln} field {} longer than {m}", k + 1));
                }
            }
        }
        if g(0) == "D"
            && (g(1).is_empty() || g(3).is_empty() || g(4).is_empty() || !digits(g(5)) || g(6).is_empty() || g(7).is_empty() || !dec2(g(9)))
        {
            problems.push(format!("line {ln}: a required D field is missing or malformed"));
        }
        if g(0) == "H" && !(g(6) == "T" || g(6) == "F") {
            problems.push("H line: Metric is not T or F".into());
        }
        if g(0) == "H" && i != 1 {
            problems.push("the H line is not the second line".into());
        }
        records.push(f);
    }
    (records, problems)
}

/// The JSON of `readKiss`.
pub fn read_kiss_json(text: &str) -> Value {
    let (r, p) = read_kiss(text);
    json!({ "records": r, "problems": p })
}
