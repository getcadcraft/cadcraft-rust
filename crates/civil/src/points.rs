//! points.js — survey point files (CSV / TXT), point groups, lasso picks (with lasso-core.js's `lassoSelect`).
//!
//! Point: `{ n (number/name, text), x (easting), y (northing), z (elevation | null), d (description) }` (+ `blk`, the
//! block it came from, 0.5 WL.19).

use crate::i18n::F;
use crate::js::{
    G, J, Obj, hypot, is_ws, locale_compare, locale_compare_num, lower, max, min, slice16, str_to_number, to_fixed, trim, upper,
};
use crate::tr;
use crate::units::is_num_str;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const FORMATS: [&str; 8] = ["PNEZD", "PENZD", "PNEZ", "PENZ", "NEZD", "ENZD", "NEZ", "ENZ"];
const MAX_BAD: usize = 200;

/// A survey point.
#[derive(Clone, Debug, PartialEq)]
pub struct Pt {
    pub n: String,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub d: String,
    pub blk: Option<Value>,
}
impl J for Pt {
    fn j(&self) -> Value {
        Obj::new()
            .set("n", &self.n)
            .set("x", self.x)
            .set("y", self.y)
            .set("z", self.z)
            .set("d", &self.d)
            .opt("blk", self.blk.clone())
            .done()
    }
}
impl crate::geo::XY for Pt {
    fn x(&self) -> f64 {
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
}
/// A point from JSON `{n, x, y, z, d, blk?}` (the harness and the project's own state; values as given).
pub fn read_pt(v: &Value) -> Pt {
    Pt {
        n: crate::js::string_of(v.get("n")),
        x: crate::js::numf(v.get("x")),
        y: crate::js::numf(v.get("y")),
        z: match v.get("z") {
            None | Some(Value::Null) => None,
            z => Some(crate::js::numf(z)),
        },
        d: crate::js::string_or_empty(v.get("d")),
        blk: v.get("blk").filter(|b| !b.is_null()).cloned(),
    }
}

/// A row that could not be read.
#[derive(Clone, Debug)]
pub struct BadRow {
    pub line: usize,
    pub text: String,
    pub why: String,
}
impl J for BadRow {
    fn j(&self) -> Value {
        crate::jo! { "line" => self.line, "text" => self.text, "why" => self.why }
    }
}

/// parsePoints' result.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub points: Vec<Pt>,
    pub format: Option<String>,
    pub format_from: String,
    pub delimiter: Option<String>,
    pub header: Option<Vec<String>>,
    pub bad: Vec<BadRow>,
    pub bad_count: usize,
    pub duplicates: usize,
    pub latlon: bool,
}
impl J for Parsed {
    fn j(&self) -> Value {
        let o = Obj::new()
            .set("points", &self.points)
            .set("format", &self.format)
            .set("formatFrom", &self.format_from)
            .set("delimiter", &self.delimiter)
            .set("header", &self.header)
            .set("bad", &self.bad)
            .set("badCount", self.bad_count)
            .set("duplicates", self.duplicates);
        if self.latlon { o.set("latlon", true).done() } else { o.done() }
    }
}

/// Split on `/\r\n|\r|\n/`.
pub fn split_lines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(b'\r') => {
                out.push(s.get(start..i).unwrap_or(""));
                i = i.saturating_add(if b.get(i.saturating_add(1)) == Some(&b'\n') { 2 } else { 1 });
                start = i;
            }
            Some(b'\n') => {
                out.push(s.get(start..i).unwrap_or(""));
                i = i.saturating_add(1);
                start = i;
            }
            _ => i = i.saturating_add(1),
        }
    }
    out.push(s.get(start..).unwrap_or(""));
    out
}

/// `parsePoints(text, { format, prefer })`.
pub fn parse_points(text: &str, format: &str, prefer: &str) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    struct Row<'a> {
        line: usize,
        text: &'a str,
    }
    let mut rows: Vec<Row> = Vec::new();
    for (i, line) in split_lines(text).into_iter().enumerate() {
        let t = trim(line);
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') || t.starts_with("//") {
            continue;
        }
        rows.push(Row { line: i.saturating_add(1), text: line });
    }
    let mut out = Parsed::default();
    if rows.is_empty() {
        return out;
    }
    let delim = detect_delimiter(&rows.iter().take(20).map(|r| r.text).collect::<Vec<_>>());
    out.delimiter = Some(delim.to_string());
    let split = |s: &str| split_row(s, delim);

    // header?
    let mut start = 0usize;
    let mut fmt: Option<String> = None;
    let first = split(rows.first().map_or("", |r| r.text));
    let numeric_cols = first.iter().filter(|c| is_num_str(c)).count();
    if numeric_cols < 2 {
        start = 1;
        fmt = format_from_header(&first);
        if fmt.is_some() {
            out.format_from = "header".into();
        }
        // GIS: columns named latitude / longitude hold degrees, not northing / easting
        let names: Vec<String> = first.iter().map(|c| head_key(c)).collect();
        if fmt.is_some()
            && names.iter().any(|c| c == "lat" || c == "latitude")
            && names.iter().any(|c| c == "lon" || c == "long" || c == "longitude")
        {
            out.latlon = true;
        }
        out.header = Some(first);
    }
    if format != "auto" && FORMATS.contains(&format) {
        fmt = Some(format.to_string());
        out.format_from = "chosen".into();
    }
    let fmt = match fmt {
        Some(f) => f,
        None => {
            let sample: Vec<Vec<String>> = rows.iter().skip(start).take(20).map(|r| split(r.text)).collect();
            let n = mode(&sample.iter().map(Vec::len).collect::<Vec<_>>());
            let mut f = if n >= 5 {
                "PNEZD"
            } else if n == 4 {
                if sample.iter().all(|c| {
                    let c0 = c.first().map_or("", String::as_str);
                    !c0.is_empty() && c0.bytes().all(|b| b.is_ascii_digit()) && is_num_str(c.get(3).map_or("", String::as_str))
                }) {
                    "PNEZ"
                } else {
                    "NEZD"
                }
            } else {
                "NEZ"
            };
            if prefer == "PENZD" {
                f = match f {
                    "PNEZD" => "PENZD",
                    "PNEZ" => "PENZ",
                    "NEZD" => "ENZD",
                    _ => "ENZ",
                };
            }
            out.format_from = "columns".into();
            f.to_string()
        }
    };
    out.format = Some(fmt.clone());

    let cols: Vec<char> = fmt.chars().collect();
    let idx = |c: char| cols.iter().position(|&x| x == c);
    let (ip, i_n, ie, iz, id) = (idx('P'), idx('N'), idx('E'), idx('Z'), idx('D'));
    let mut seen: HashSet<String> = HashSet::new();
    let mut auto = 0u64;
    let bad = |out: &mut Parsed, r: &Row, why: String| {
        out.bad_count = out.bad_count.saturating_add(1);
        if out.bad.len() < MAX_BAD {
            out.bad.push(BadRow { line: r.line, text: slice16(r.text, 160), why });
        }
    };
    for r in rows.iter().skip(start) {
        let mut c = split(r.text);
        // whitespace-separated descriptions may contain spaces: glue the tail back on
        if let Some(d) = id
            && d == cols.len().saturating_sub(1)
            && c.len() > cols.len()
        {
            let glue = if delim == "," { "," } else { " " };
            let tail = c.get(d..).map(|t| t.join(glue)).unwrap_or_default();
            c.truncate(d);
            c.push(tail);
        }
        let need = cols.len().saturating_sub(if id.is_some() { 1 } else { 0 });
        if c.len() < need {
            let why = tr!("pts.bad.cols", n = c.len(), want = cols.len(), format = fmt.as_str());
            bad(&mut out, r, why);
            continue;
        }
        let cell = |i: Option<usize>| i.and_then(|i| c.get(i)).map(String::as_str);
        let (nn, ee, zz) = (cell(i_n), cell(ie), cell(iz));
        if !nn.is_some_and(is_num_str) || !ee.is_some_and(is_num_str) {
            bad(&mut out, r, tr!("pts.bad.ne"));
            continue;
        }
        if let Some(z) = zz
            && !z.is_empty()
            && !is_num_str(z)
        {
            bad(&mut out, r, tr!("pts.bad.z"));
            continue;
        }
        let nv = str_to_number(nn.unwrap_or(""));
        let ev = str_to_number(ee.unwrap_or(""));
        if out.latlon && (nv.abs() > 90.0 || ev.abs() > 180.0) {
            bad(&mut out, r, tr!("pts.bad.latlon"));
            continue;
        }
        let mut n = if ip.is_some() { trim(cell(ip).unwrap_or("")).to_string() } else { String::new() };
        if ip.is_some() && n.is_empty() {
            bad(&mut out, r, tr!("pts.bad.nonum"));
            continue;
        }
        if ip.is_none() {
            loop {
                auto = auto.saturating_add(1);
                n = auto.to_string();
                if !seen.contains(&n) {
                    break;
                }
            }
        }
        if seen.contains(&n) {
            out.duplicates = out.duplicates.saturating_add(1);
            bad(&mut out, r, tr!("pts.bad.twice", num = n.as_str()));
            continue;
        }
        seen.insert(n.clone());
        let z = match zz {
            None | Some("") => None,
            Some(z) => Some(str_to_number(z)),
        };
        let d = if id.is_some() { clean_text(cell(id).unwrap_or("")) } else { String::new() };
        out.points.push(Pt { n, x: ev, y: nv, z, d, blk: None });
    }
    out
}

fn clean_text(s: &str) -> String {
    slice16(trim(&crate::js::strip_controls(s, " ")), 200)
}

fn detect_delimiter(lines: &[&str]) -> &'static str {
    let need = (lines.len() as f64 * 0.6).ceil();
    let score = |c: char| lines.iter().filter(|l| l.contains(c)).count() as f64;
    if score(',') >= need {
        ","
    } else if score('\t') >= need {
        "\t"
    } else if score(';') >= need {
        ";"
    } else {
        "ws"
    }
}

/// `s.trim().split(/\s+/)`.
pub fn split_ws(s: &str) -> Vec<String> {
    let t = trim(s);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_ws = false;
    for c in t.chars() {
        if is_ws(c) {
            if !in_ws {
                out.push(std::mem::take(&mut cur));
                in_ws = true;
            }
        } else {
            in_ws = false;
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// One row split on the delimiter; "quoted, fields" kept whole (CSV quoting with "" escapes).
pub fn split_row(s: &str, delim: &str) -> Vec<String> {
    if delim == "ws" {
        return split_ws(s);
    }
    let dc = delim.chars().next().unwrap_or(',');
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < cs.len() {
        let ch = cs.g(i);
        if q {
            if ch == '"' {
                if cs.get(i.saturating_add(1)) == Some(&'"') {
                    cur.push('"');
                    i = i.saturating_add(1);
                } else {
                    q = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' && trim(&cur).is_empty() {
            q = true;
            cur.clear();
        } else if ch == dc {
            out.push(trim(&cur).to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
        i = i.saturating_add(1);
    }
    out.push(trim(&cur).to_string());
    out
}
impl crate::js::Und for char {
    fn und() -> Self {
        '\0'
    }
}

/// `c.toLowerCase().replace(/[\s._()-]/g, '')`.
fn head_key(c: &str) -> String {
    lower(c).chars().filter(|&ch| !(is_ws(ch) || matches!(ch, '.' | '_' | '(' | ')' | '-'))).collect()
}

fn head_letter(k: &str) -> Option<char> {
    const P: &[&str] = &["pt", "pnt", "point", "pointno", "pointnumber", "number", "no", "id", "name", "pt#", "point#", "#"];
    const N: &[&str] = &["n", "north", "northing", "y", "ycoord", "lat", "latitude"];
    const E: &[&str] = &["e", "east", "easting", "x", "xcoord", "lon", "long", "longitude"];
    const Z: &[&str] = &["z", "elev", "elevation", "el", "height", "h", "zcoord", "rl"];
    const D: &[&str] = &["d", "desc", "description", "code", "rawdescription", "fulldescription", "note", "notes", "feature"];
    if P.contains(&k) {
        return Some('P');
    }
    if N.contains(&k) {
        return Some('N');
    }
    if E.contains(&k) {
        return Some('E');
    }
    if Z.contains(&k) {
        return Some('Z');
    }
    // the D pattern is case-blind (/i); k is lower case already except for characters lower() leaves alone
    if D.iter().any(|d| k.eq_ignore_ascii_case(d)) {
        return Some('D');
    }
    None
}

fn format_from_header(cells: &[String]) -> Option<String> {
    let letters: Vec<Option<char>> = cells.iter().map(|c| head_letter(&head_key(c))).collect();
    if !letters.contains(&Some('N')) || !letters.contains(&Some('E')) {
        return None;
    }
    // keep only the recognised columns in order; unrecognised columns break the mapping
    if letters.iter().any(Option::is_none) {
        return None;
    }
    let f: String = letters.iter().flatten().collect();
    if FORMATS.contains(&f.as_str()) { Some(f) } else { None }
}

fn mode(arr: &[usize]) -> usize {
    let mut order: Vec<usize> = Vec::new();
    let mut m: HashMap<usize, usize> = HashMap::new();
    for &v in arr {
        let e = m.entry(v).or_insert_with(|| {
            order.push(v);
            0
        });
        *e = e.saturating_add(1);
    }
    let (mut best, mut bv) = (0usize, 0usize);
    for v in order {
        let c = m.get(&v).copied().unwrap_or(0);
        if c > best {
            best = c;
            bv = v;
        }
    }
    bv
}

/// `groupKey(desc)`: the first word, upper case ('' → '(no description)').
pub fn group_key(desc: &str) -> String {
    let w = split_ws(desc).into_iter().next().unwrap_or_default();
    if w.is_empty() { "(no description)".to_string() } else { upper(&w) }
}

/// A point → its group.
pub type KeyOf<'a> = &'a dyn Fn(&Pt) -> String;

/// `pointGroups(points, keyOf)` → [(key, count)] sorted by key.
pub fn point_groups(points: &[Pt], key_of: Option<KeyOf>) -> Vec<(String, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut m: HashMap<String, usize> = HashMap::new();
    for p in points {
        let k = match key_of {
            Some(f) => f(p),
            None => group_key(&p.d),
        };
        match m.get_mut(&k) {
            Some(c) => *c = c.saturating_add(1),
            None => {
                order.push(k.clone());
                m.insert(k, 1);
            }
        }
    }
    let mut out: Vec<(String, usize)> = order.into_iter().map(|k| (k.clone(), m.get(&k).copied().unwrap_or(0))).collect();
    crate::js::sort_by(&mut out, |a, b| locale_compare(&a.0, &b.0));
    out
}

/// Points of a group in point-number order.
pub fn group_points(points: &[Pt], key: &str, key_of: Option<KeyOf>) -> Vec<Pt> {
    let mut out: Vec<Pt> = points
        .iter()
        .filter(|p| match key_of {
            Some(f) => f(p) == key,
            None => group_key(&p.d) == key,
        })
        .cloned()
        .collect();
    crate::js::sort_by(&mut out, by_number);
    out
}
/// `byNumber(a, b)`: numbers compare numerically when both read as numbers.
pub fn by_number(a: &Pt, b: &Pt) -> f64 {
    let na = str_to_number(&a.n);
    let nb = str_to_number(&b.n);
    if na.is_finite() && nb.is_finite() {
        return na - nb;
    }
    locale_compare_num(&a.n, &b.n)
}

/// A PNEZD comma file of the points.
pub fn write_points(points: &[Pt], dec: usize) -> String {
    let q = |s: &str| if s.contains('"') || s.contains(',') { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let rows: Vec<String> = points
        .iter()
        .map(|p| [q(&p.n), to_fixed(p.y, dec), to_fixed(p.x, dec), p.z.map_or(String::new(), |z| to_fixed(z, dec)), q(&p.d)].join(","))
        .collect();
    format!("{}\r\n", rows.join("\r\n"))
}

// ---------------------------------------------------------------------------
// lasso-core.js (lassoSelect) for points

fn lasso_point_in_poly(px: f64, py: f64, pts: &[crate::geo::P2]) -> bool {
    crate::tin::point_in_poly(px, py, pts)
}

/// lasso-core.js `segsCross`: do segments AB and CD touch (collinear overlaps included).
#[allow(clippy::too_many_arguments)]
pub fn segs_cross(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, dx: f64, dy: f64) -> bool {
    let o = |px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64| {
        let v = (qx - px) * (ry - py) - (qy - py) * (rx - px);
        if v.abs() < 1e-12 {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    };
    let on = |px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64| {
        min(px, qx) - 1e-12 <= rx && rx <= max(px, qx) + 1e-12 && min(py, qy) - 1e-12 <= ry && ry <= max(py, qy) + 1e-12
    };
    let o1 = o(ax, ay, bx, by, cx, cy);
    let o2 = o(ax, ay, bx, by, dx, dy);
    let o3 = o(cx, cy, dx, dy, ax, ay);
    let o4 = o(cx, cy, dx, dy, bx, by);
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on(ax, ay, bx, by, cx, cy))
        || (o2 == 0 && on(ax, ay, bx, by, dx, dy))
        || (o3 == 0 && on(cx, cy, dx, dy, ax, ay))
        || (o4 == 0 && on(cx, cy, dx, dy, bx, by))
}

/// `lassoPoints(points, path, mode, r)` → indices of the points picked, in order.
pub fn lasso_points(points: &[Pt], pts: &[crate::geo::P2], mode: &str, r: f64) -> Vec<usize> {
    let mut out = Vec::new();
    if pts.len() < 2 {
        return out;
    }
    let mut path: Vec<[f64; 4]> = Vec::new();
    let n = if mode == "fence" { pts.len().saturating_sub(1) } else { pts.len() };
    for i in 0..n {
        let a = pts.g(i);
        let b = pts.g((i.saturating_add(1)) % pts.len());
        if hypot(a.x - b.x, a.y - b.y) > 1e-12 {
            path.push([a.x, a.y, b.x, b.y]);
        }
    }
    if path.is_empty() || (mode != "fence" && pts.len() < 3) {
        return out;
    }
    let (mut lx0, mut ly0, mut lx1, mut ly1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lx0 = min(lx0, p.x);
        ly0 = min(ly0, p.y);
        lx1 = max(lx1, p.x);
        ly1 = max(ly1, p.y);
    }
    let crosses = |ss: &[[f64; 4]]| ss.iter().any(|s| path.iter().any(|q| segs_cross(s[0], s[1], s[2], s[3], q[0], q[1], q[2], q[3])));
    for (k, p) in points.iter().enumerate() {
        let (min_x, min_y, max_x, max_y) = (p.x - r, p.y - r, p.x + r, p.y + r);
        if !(max_x >= lx0 && min_x <= lx1 && max_y >= ly0 && min_y <= ly1) {
            continue;
        }
        let ss = [[p.x - r, p.y, p.x + r, p.y], [p.x, p.y - r, p.x, p.y + r]];
        if mode == "fence" {
            if crosses(&ss) {
                out.push(k);
            }
            continue;
        }
        if mode == "window" {
            if !(min_x >= lx0 && max_x <= lx1 && min_y >= ly0 && max_y <= ly1) {
                continue;
            }
            if !crosses(&ss) && ss.iter().all(|s| lasso_point_in_poly(s[0], s[1], pts)) {
                out.push(k);
            }
            continue;
        }
        if crosses(&ss) || ss.iter().any(|s| lasso_point_in_poly(s[0], s[1], pts)) {
            out.push(k);
        }
    }
    out
}

/// Fill values of a key (for messages built elsewhere).
pub fn fill_n(n: usize) -> F {
    F::from(n)
}
