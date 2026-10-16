//! An ASCII DXF file → the drawing model `read_dwg` returns: a line-by-line port of BimCraft's `dxf-model.js`
//! (`readDxfModel`), written from the public DXF reference (group codes) only.
//!
//! `read_dxf_model(text)` → `{ version, maintenance, codepage, header, classes: [], tables, blocks, layouts, objects,
//! report }`, the same shape as the JS, so [`crate::compile::compile_drawing`] and the library adapters read DWG and DXF
//! through the same code. What is read: HEADER variables; LAYER, LTYPE (names), STYLE, DIMSTYLE (DIMSCALE …),
//! BLOCK_RECORD, APPID, VPORT tables; every block; the entities the JS reads (POLYLINE vertices and INSERT attributes
//! folded in); objects LAYOUT and IMAGEDEF. Anything else is `{ type: 'UNKNOWN', dxfName }`, counted in
//! `report.unknown`. Entities of the ENTITIES section go to the block of their owner (330), else *Model_Space /
//! *Paper_Space by group 67; an R12 file without those blocks or layouts gets them made up.
//!
//! JS semantics kept: `parseFloat` / `parseInt` prefixes, `String.prototype.trim`'s whitespace, `||` / `??`
//! defaults, `x & n` through ToInt32, the order in which made-up handles (`DXF<n>`, `R<n>`) are counted.
//! Where the JS would throw a TypeError on a malformed file (a `null` leader, a hatch cut off mid-edge), this port
//! carries on instead; such files are not readable by the JS at all.

use std::collections::HashMap;

use crate::read::js_string;
use crate::util::to_i32;
use crate::value::{Obj, V, p2, p3};

const D2R: f64 = std::f64::consts::PI / 180.0;
/// Caps a loop count read from the file that does not consume groups (the JS would allocate without limit).
const MAX_LOOP: usize = 1_000_000;

/// The file's bytes → text as BimCraft decodes a DXF: UTF-8 (a BOM dropped), else windows-1252 when the UTF-8
/// decode needed replacement characters.
pub fn decode_dxf_bytes(bytes: &[u8]) -> String {
    let (t, _) = encoding_rs::UTF_8.decode_with_bom_removal(bytes);
    if t.contains('\u{FFFD}') {
        return encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned();
    }
    t.into_owned()
}

fn msg(key: &str, line: Option<usize>) -> String {
    match key {
        "dxfmodel.binary" => "This is a binary DXF. Save it as ASCII DXF and try again.".into(),
        "dxfmodel.bad_code" => format!("This file is not a readable DXF (bad group code on line {}).", line.unwrap_or(0)),
        _ => "This DXF file could not be read as text.".into(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JS number parsing

fn js_space(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(crate::compile::js_space)
}
fn js_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}
/// `parseFloat(s)`: the longest decimal prefix after leading whitespace (NaN when there is none).
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(js_space);
    let b = t.as_bytes();
    let mut i = 0usize;
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    if at(i) == b'+' || at(i) == b'-' {
        i = i.saturating_add(1);
    }
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if at(0) == b'-' { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let int_start = i;
    while at(i).is_ascii_digit() {
        i = i.saturating_add(1);
    }
    let mut digits = i > int_start;
    if at(i) == b'.' {
        let f0 = i.saturating_add(1);
        let mut j = f0;
        while at(j).is_ascii_digit() {
            j = j.saturating_add(1);
        }
        if j > f0 || digits {
            digits = digits || j > f0;
            i = j;
        }
    }
    if !digits {
        return f64::NAN;
    }
    if at(i) == b'e' || at(i) == b'E' {
        let mut j = i.saturating_add(1);
        if at(j) == b'+' || at(j) == b'-' {
            j = j.saturating_add(1);
        }
        let e0 = j;
        while at(j).is_ascii_digit() {
            j = j.saturating_add(1);
        }
        if j > e0 {
            i = j;
        }
    }
    t.get(..i).and_then(|p| p.parse::<f64>().ok()).unwrap_or(f64::NAN)
}
/// `parseInt(s, 10)`.
pub fn parse_int(s: &str) -> f64 {
    let t = s.trim_start_matches(js_space);
    let b = t.as_bytes();
    let mut i = 0usize;
    let neg = b.first() == Some(&b'-');
    if neg || b.first() == Some(&b'+') {
        i = 1;
    }
    let d0 = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i == d0 {
        return f64::NAN;
    }
    let v = t.get(d0..i).and_then(|d| d.parse::<f64>().ok()).unwrap_or(f64::NAN);
    if neg { -v } else { v }
}

fn in_ranges(code: f64, ranges: &[(f64, f64)]) -> bool {
    ranges.iter().any(|&(a, b)| code >= a && code <= b)
}
/// The value of a group by its code range (DXF reference, "Group code value types").
fn typed(code: f64, v: &str) -> V {
    if in_ranges(code, &[(10.0, 59.0), (110.0, 149.0), (210.0, 239.0), (460.0, 469.0), (1010.0, 1059.0)]) {
        let x = parse_float(v);
        return V::Num(if x.is_finite() { x } else { 0.0 });
    }
    if in_ranges(
        code,
        &[(60.0, 99.0), (160.0, 179.0), (270.0, 289.0), (370.0, 389.0), (400.0, 409.0), (420.0, 429.0), (440.0, 459.0), (1060.0, 1071.0)],
    ) {
        let x = parse_int(v);
        return V::Num(if x.is_finite() { x } else { 0.0 });
    }
    if (290.0..=299.0).contains(&code) {
        return V::Bool(parse_int(v) == 1.0);
    }
    V::Str(v.to_string())
}

// ---------------------------------------------------------------------------------------------------------------
// Small readers over one entity's groups

type G = [(f64, V)];
/// A POLYLINE vertex or INSERT attribute that follows its owner: (type, groups).
type Follow = (String, Vec<(f64, V)>);

/// JS `ToNumber` of a group value.
fn num(v: Option<&V>) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Num(n)) => *n,
        Some(V::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(V::Null) => 0.0,
        Some(V::Str(s)) => {
            let t = js_trim(s);
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        Some(_) => f64::NAN,
    }
}
fn val(g: &G, code: f64) -> Option<&V> {
    g.iter().find(|(c, _)| *c == code).map(|(_, v)| v)
}
fn has(g: &G, code: f64) -> bool {
    g.iter().any(|(c, _)| *c == code)
}
/// `v || d` (keeps the value itself).
fn or(v: Option<&V>, d: impl Into<V>) -> V {
    match v {
        Some(x) if x.truthy() => x.clone(),
        _ => d.into(),
    }
}
/// `v ?? d`
fn nn(v: Option<&V>, d: impl Into<V>) -> V {
    match v {
        None | Some(V::Null) => d.into(),
        Some(x) => x.clone(),
    }
}
/// `(v || 0) * D2R`
fn angle(v: Option<&V>) -> V {
    V::Num(num(Some(&or(v, 0.0))) * D2R)
}
/// `String(v ?? d)`
fn sv(v: Option<&V>, d: &str) -> V {
    V::Str(match v {
        None | Some(V::Null) => d.to_string(),
        Some(x) => js_string(x),
    })
}
fn bit(v: &V, b: i32) -> bool {
    to_i32(num(Some(v))) & b != 0
}
fn is_n(v: Option<&V>, n: f64) -> bool {
    matches!(v, Some(V::Num(x)) if *x == n)
}
/// `hx(v)`: handles upper-case hex without leading zeros.
fn hx(v: Option<&V>) -> String {
    let s = match v {
        Some(x) if x.truthy() => js_string(x),
        _ => String::new(),
    };
    js_trim(&s).to_uppercase().trim_start_matches('0').to_string()
}
fn point(g: &G, code: f64, z: bool, dflt: Option<V>) -> V {
    let (mut x, mut y, mut zz) = (None, None, None);
    for (c, v) in g {
        if *c == code && x.is_none() {
            x = Some(v);
        } else if *c == code + 10.0 && y.is_none() {
            y = Some(v);
        } else if *c == code + 20.0 && zz.is_none() {
            zz = Some(v);
        }
    }
    if x.is_none()
        && let Some(d) = dflt
    {
        return d;
    }
    let mut p = Obj::with_capacity(3);
    p.set("x", or(x, 0.0));
    p.set("y", or(y, 0.0));
    if z {
        p.set("z", or(zz, 0.0));
    }
    V::Obj(p)
}
fn pt(g: &G, code: f64) -> V {
    point(g, code, true, None)
}
/// Every point of a repeated code (x at `code`, y at code + 10, z at code + 20), in order.
fn points(g: &G, code: f64, z: bool) -> Vec<V> {
    let mut out: Vec<Obj> = Vec::new();
    for (c, v) in g {
        if *c == code {
            let mut p = Obj::with_capacity(3);
            p.set("x", v.clone());
            p.set("y", 0.0);
            if z {
                p.set("z", 0.0);
            }
            out.push(p);
        } else if *c == code + 10.0 {
            if let Some(p) = out.last_mut() {
                p.set("y", v.clone());
            }
        } else if z
            && *c == code + 20.0
            && let Some(p) = out.last_mut()
        {
            p.set("z", v.clone());
        }
    }
    out.into_iter().map(V::Obj).collect()
}
/// A value inside one subclass (after its 100 marker).
fn sub_val<'a>(g: &'a G, sub: &str, code: f64) -> Option<&'a V> {
    let mut inside = false;
    for (c, v) in g {
        if *c == 100.0 {
            inside = matches!(v, V::Str(s) if s == sub);
        } else if inside && *c == code {
            return Some(v);
        }
    }
    None
}
fn sub_point(g: &G, sub: &str, code: f64, z: bool) -> V {
    let mut inside = false;
    let mut p = Obj::with_capacity(3);
    p.set("x", 0.0);
    p.set("y", 0.0);
    if z {
        p.set("z", 0.0);
    }
    for (c, v) in g {
        if *c == 100.0 {
            inside = matches!(v, V::Str(s) if s == sub);
        } else if inside && *c == code {
            p.set("x", v.clone());
        } else if inside && *c == code + 10.0 {
            p.set("y", v.clone());
        } else if inside && *c == code + 20.0 {
            p.set("z", v.clone());
        }
    }
    V::Obj(p)
}
/// DXF caret escapes in text: "^ " is a caret, ^J and the like control characters. (`.` matches one UTF-16 unit
/// other than a line terminator.)
fn decode(v: Option<&V>) -> V {
    let s = match v {
        None | Some(V::Null) => String::new(),
        Some(x) => js_string(x),
    };
    if !s.contains('^') {
        return V::Str(s);
    }
    let u: Vec<u16> = s.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(u.len());
    let mut i = 0usize;
    while i < u.len() {
        let c = u.get(i).copied().unwrap_or(0);
        let next = u.get(i.saturating_add(1)).copied();
        match next {
            Some(n) if c == u16::from(b'^') && !matches!(n, 0x0A | 0x0D | 0x2028 | 0x2029) => {
                out.push(if n == u16::from(b' ') { u16::from(b'^') } else { n & 31 });
                i = i.saturating_add(2);
            }
            _ => {
                out.push(c);
                i = i.saturating_add(1);
            }
        }
    }
    V::Str(String::from_utf16_lossy(&out))
}
fn color_of(index: V, rgb: Option<&V>) -> V {
    let mut o = Obj::with_capacity(2);
    if let Some(rgb) = rgb {
        o.set("rgb", crate::util::u32_from_i32(to_i32(num(Some(rgb))) & 0xFF_FFFF));
    }
    o.set("index", index);
    V::Obj(o)
}
fn xdata(g: &G) -> Option<Obj> {
    let mut out: Option<Obj> = None;
    let mut app: Option<String> = None;
    let mut k = 0usize;
    while k < g.len() {
        let Some((c, v)) = g.get(k) else { break };
        let c = *c;
        if c == 1001.0 {
            let name = js_string(v);
            let o = out.get_or_insert_with(Obj::new);
            if !o.has(&name) {
                o.set(name.clone(), V::Arr(Vec::new()));
            }
            app = Some(name);
            k = k.saturating_add(1);
            continue;
        }
        let (Some(name), Some(o)) = (app.as_ref(), out.as_mut()) else {
            k = k.saturating_add(1);
            continue;
        };
        if c < 1000.0 {
            k = k.saturating_add(1);
            continue;
        }
        let item = if (1010.0..=1013.0).contains(&c) {
            let mut p = Obj::with_capacity(3);
            p.set("x", v.clone());
            p.set("y", 0.0);
            p.set("z", 0.0);
            if let Some((c2, v2)) = g.get(k.saturating_add(1))
                && *c2 == c + 10.0
            {
                p.set("y", v2.clone());
                k = k.saturating_add(1);
            }
            if let Some((c3, v3)) = g.get(k.saturating_add(1))
                && *c3 == c + 20.0
            {
                p.set("z", v3.clone());
                k = k.saturating_add(1);
            }
            V::Arr(vec![V::Num(c), V::Obj(p)])
        } else {
            V::Arr(vec![V::Num(c), v.clone()])
        };
        if let Some(V::Arr(a)) = o.get_mut(name) {
            a.push(item);
        }
        k = k.saturating_add(1);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------

struct Block {
    obj: Obj,
    entities: Vec<Obj>,
}

struct Reader<'t> {
    codes: Vec<f64>,
    vals: Vec<V>,
    pairs: Vec<(f64, V)>,
    _text: std::marker::PhantomData<&'t str>,
    i: usize,
    auto: u64,
    header: Obj,
    layers: Vec<V>,
    linetypes: Vec<V>,
    styles: Vec<V>,
    dimstyles: Vec<V>,
    vports: Vec<V>,
    appids: Vec<V>,
    records: Vec<Obj>,
    blocks: Vec<Block>,
    objects: Vec<Obj>,
    loose: Vec<(Obj, bool)>,
}

const HATCH_EDGE: [&str; 4] = ["line", "arc", "ellipse", "spline"];
const DIM_TYPES: [&str; 7] = [
    "DIMENSION_LINEAR",
    "DIMENSION_ALIGNED",
    "DIMENSION_ANG2LN",
    "DIMENSION_DIAMETER",
    "DIMENSION_RADIUS",
    "DIMENSION_ANG3PT",
    "DIMENSION_ORDINATE",
];
fn dimvar(code: f64) -> Option<&'static str> {
    Some(match code {
        40.0 => "DIMSCALE",
        41.0 => "DIMASZ",
        42.0 => "DIMEXO",
        43.0 => "DIMDLI",
        44.0 => "DIMEXE",
        140.0 => "DIMTXT",
        147.0 => "DIMGAP",
        77.0 => "DIMTAD",
        78.0 => "DIMZIN",
        271.0 => "DIMDEC",
        179.0 => "DIMADEC",
        144.0 => "DIMLFAC",
        141.0 => "DIMCEN",
        _ => return None,
    })
}

impl Reader<'_> {
    fn n(&self) -> usize {
        self.codes.len()
    }
    fn code(&self, i: usize) -> f64 {
        self.codes.get(i).copied().unwrap_or(f64::NAN)
    }
    fn is0(&self, name: &str) -> bool {
        self.code(self.i) == 0.0 && matches!(self.vals.get(self.i), Some(V::Str(s)) if s == name)
    }
    /// The groups up to the next 0 group (a slice of `pairs`, as start..end).
    fn group(&mut self) -> (usize, usize) {
        let s = self.i;
        while self.i < self.n() && self.code(self.i) != 0.0 {
            self.i = self.i.saturating_add(1);
        }
        (s, self.i)
    }
    fn g(&self, r: (usize, usize)) -> &G {
        self.pairs.get(r.0..r.1).unwrap_or(&[])
    }
    fn next_auto(&mut self) -> String {
        self.auto = self.auto.saturating_add(1);
        format!("{:X}", self.auto)
    }
    fn handle_of(&mut self, g: &G) -> String {
        let v = match val(g, 5.0) {
            None | Some(V::Null) => val(g, 105.0),
            x => x,
        };
        let h = hx(v);
        if h.is_empty() { format!("DXF{}", self.next_auto()) } else { h }
    }

    fn read_header(&mut self, r: (usize, usize)) {
        let g: Vec<(f64, V)> = self.g(r).to_vec();
        let mut name: Option<String> = None;
        for (c, v) in &g {
            if *c == 9.0 {
                let s = js_string(v);
                name = Some(s.strip_prefix('$').map(str::to_string).unwrap_or(s));
                continue;
            }
            let Some(nm) = name.as_ref().filter(|n| !n.is_empty()) else { continue };
            if (10.0..=39.0).contains(c) {
                if !matches!(self.header.get(nm), Some(V::Obj(_))) {
                    self.header.set(nm.clone(), p2(0.0, 0.0));
                }
                let k = if *c < 20.0 {
                    "x"
                } else if *c < 30.0 {
                    "y"
                } else {
                    "z"
                };
                if let Some(p) = self.header.obj_mut(nm) {
                    p.set(k, v.clone());
                }
            } else {
                self.header.set(nm.clone(), v.clone());
            }
        }
    }

    fn read_tables(&mut self) {
        while self.i < self.n() && !self.is0("ENDSEC") {
            if !self.is0("TABLE") {
                self.i = self.i.saturating_add(1);
                self.group();
                continue;
            }
            self.i = self.i.saturating_add(1);
            let gr = self.group();
            let table = val(self.g(gr), 2.0).cloned();
            while self.i < self.n() && !self.is0("ENDTAB") && !self.is0("ENDSEC") {
                let kind = self.vals.get(self.i).cloned();
                self.i = self.i.saturating_add(1);
                let r = self.group();
                if let (Some(V::Str(k)), Some(V::Str(t))) = (&kind, &table)
                    && k == t
                {
                    let g: Vec<(f64, V)> = self.g(r).to_vec();
                    self.table_entry(k, &g);
                }
            }
            if self.is0("ENDTAB") {
                self.i = self.i.saturating_add(1);
                self.group();
            }
        }
    }

    fn table_entry(&mut self, kind: &str, g: &G) {
        let handle = self.handle_of(g);
        let name = sv(val(g, 2.0), "");
        let flags = or(val(g, 70.0), 0.0);
        let mut o = Obj::with_capacity(12);
        o.set("handle", handle);
        o.set("name", name);
        match kind {
            "LAYER" => {
                let ci = nn(val(g, 62.0), 7.0);
                let lw = val(g, 370.0);
                let idx = num(Some(&ci)).abs();
                let idx = if idx != 0.0 && !idx.is_nan() { idx } else { 7.0 };
                o.set("flags", flags.clone());
                o.set("color", color_of(V::Num(idx), val(g, 420.0)));
                o.set("linetypeName", sv(val(g, 6.0), "Continuous"));
                o.set("lineweight", nn(lw, -3.0));
                o.set("plot", !matches!(val(g, 290.0), Some(V::Bool(false))));
                o.set("off", num(Some(&ci)) < 0.0);
                o.set("frozen", bit(&flags, 1));
                o.set("locked", bit(&flags, 4));
                self.layers.push(V::Obj(o));
            }
            "LTYPE" => {
                o.set("description", sv(val(g, 3.0), ""));
                o.set("patternLength", or(val(g, 40.0), 0.0));
                o.set("elements", V::Arr(Vec::new()));
                self.linetypes.push(V::Obj(o));
            }
            "STYLE" => {
                o.set("flags", flags);
                o.set("height", or(val(g, 40.0), 0.0));
                o.set("widthFactor", nn(val(g, 41.0), 1.0));
                o.set("oblique", angle(val(g, 50.0)));
                o.set("generation", or(val(g, 71.0), 0.0));
                o.set("lastHeight", or(val(g, 42.0), 0.0));
                o.set("font", sv(val(g, 3.0), ""));
                o.set("bigFont", sv(val(g, 4.0), ""));
                self.styles.push(V::Obj(o));
            }
            "DIMSTYLE" => {
                let mut vars = Obj::new();
                for (c, v) in g {
                    if let Some(k) = dimvar(*c) {
                        vars.set(k, v.clone());
                    }
                }
                o.set("vars", vars);
                self.dimstyles.push(V::Obj(o));
            }
            "BLOCK_RECORD" => {
                o.set("flags", 0.0);
                o.set("layout", hx(val(g, 340.0)));
                o.set("insertUnits", or(val(g, 70.0), 0.0));
                o.set("explodable", !is_n(val(g, 280.0), 0.0));
                o.set("scalable", or(val(g, 281.0), 0.0));
                self.records.push(o);
            }
            "APPID" => {
                o.set("flags", flags);
                self.appids.push(V::Obj(o));
            }
            "VPORT" => {
                o.set("flags", flags);
                self.vports.push(V::Obj(o));
            }
            _ => {}
        }
    }

    fn read_blocks(&mut self) {
        while self.i < self.n() && !self.is0("ENDSEC") {
            if !self.is0("BLOCK") {
                self.i = self.i.saturating_add(1);
                self.group();
                continue;
            }
            self.i = self.i.saturating_add(1);
            let r = self.group();
            let g: Vec<(f64, V)> = self.g(r).to_vec();
            let mut b = Obj::with_capacity(10);
            let nm = match val(&g, 2.0) {
                None | Some(V::Null) => val(&g, 3.0),
                x => x,
            };
            b.set("name", sv(nm, ""));
            let h = self.handle_of(&g);
            b.set("handle", h);
            b.set("record", hx(val(&g, 330.0)));
            b.set("base", pt(&g, 10.0));
            b.set("flags", or(val(&g, 70.0), 0.0));
            b.set("xrefPath", sv(val(&g, 1.0), ""));
            b.set("description", sv(val(&g, 4.0), ""));
            let mut ents = Vec::new();
            self.read_entities(&mut ents, false, "ENDBLK");
            if self.is0("ENDBLK") {
                self.i = self.i.saturating_add(1);
                let r = self.group();
                let g: Vec<(f64, V)> = self.g(r).to_vec();
                let eh = self.handle_of(&g);
                b.set("endHandle", eh);
            }
            self.blocks.push(Block { obj: b, entities: ents.into_iter().map(|(e, _)| e).collect() });
        }
    }

    /// Entities up to ENDSEC (or `stop`), POLYLINE vertices and INSERT attributes folded into their owner.
    fn read_entities(&mut self, list: &mut Vec<(Obj, bool)>, top: bool, stop: &str) {
        while self.i < self.n() && !self.is0("ENDSEC") && !self.is0(stop) {
            if self.code(self.i) != 0.0 {
                self.i = self.i.saturating_add(1);
                continue;
            }
            let ty = self.vals.get(self.i).map(js_string).unwrap_or_default();
            self.i = self.i.saturating_add(1);
            let r = self.group();
            let g: Vec<(f64, V)> = self.g(r).to_vec();
            let mut follow: Option<Vec<Follow>> = None;
            if ty == "POLYLINE" || (ty == "INSERT" && is_n(val(&g, 66.0), 1.0)) {
                let mut fl = Vec::new();
                while self.i < self.n() && self.code(self.i) == 0.0 && (self.is0("VERTEX") || self.is0("ATTRIB")) {
                    let t = self.vals.get(self.i).map(js_string).unwrap_or_default();
                    self.i = self.i.saturating_add(1);
                    let r = self.group();
                    fl.push((t, self.g(r).to_vec()));
                }
                if self.is0("SEQEND") {
                    self.i = self.i.saturating_add(1);
                    self.group();
                }
                follow = Some(fl);
            }
            let e = self.entity(&ty, &g, follow.as_deref());
            let paper = top && is_n(val(&g, 67.0), 1.0);
            list.push((e, paper));
        }
    }

    fn read_objects(&mut self) {
        while self.i < self.n() && !self.is0("ENDSEC") {
            if self.code(self.i) != 0.0 {
                self.i = self.i.saturating_add(1);
                continue;
            }
            let ty = self.vals.get(self.i).map(js_string).unwrap_or_default();
            self.i = self.i.saturating_add(1);
            let r = self.group();
            let g: Vec<(f64, V)> = self.g(r).to_vec();
            if ty == "LAYOUT" {
                // two 330s: the owner dictionary, then (in the AcDbLayout part) the paper space block record
                let (mut sub, mut block, mut owner) = (String::new(), String::new(), String::new());
                for (c, v) in &g {
                    if *c == 100.0 {
                        sub = js_string(v);
                    } else if *c == 330.0 {
                        if sub == "AcDbLayout" {
                            block = hx(Some(v));
                        } else if owner.is_empty() {
                            owner = hx(Some(v));
                        }
                    }
                }
                let mut lay = Obj::with_capacity(16);
                let h = self.handle_of(&g);
                lay.set("handle", h);
                lay.set("owner", owner);
                lay.set("type", ty);
                let sl = |c: f64| sub_val(&g, "AcDbLayout", c);
                lay.set("name", sv(sl(1.0), ""));
                lay.set("tabOrder", or(sl(71.0), 0.0));
                lay.set("flags", or(sl(70.0), 0.0));
                lay.set("block", block);
                lay.set("limMin", sub_point(&g, "AcDbLayout", 10.0, false));
                lay.set("limMax", sub_point(&g, "AcDbLayout", 11.0, false));
                lay.set("insBase", sub_point(&g, "AcDbLayout", 12.0, false));
                lay.set("extMin", sub_point(&g, "AcDbLayout", 14.0, false));
                lay.set("extMax", sub_point(&g, "AcDbLayout", 15.0, false));
                let mut plot = Obj::with_capacity(7);
                plot.set("pageSetupName", sv(val(&g, 1.0), ""));
                plot.set("printer", sv(val(&g, 2.0), ""));
                plot.set("paperSize", sv(val(&g, 4.0), ""));
                plot.set("paperWidth", or(val(&g, 44.0), 0.0));
                plot.set("paperHeight", or(val(&g, 45.0), 0.0));
                plot.set("paperUnits", or(val(&g, 72.0), 0.0));
                plot.set("rotation", or(val(&g, 73.0), 0.0));
                lay.set("plot", plot);
                self.objects.push(lay);
            } else if ty == "IMAGEDEF" {
                let mut o = Obj::with_capacity(7);
                let h = self.handle_of(&g);
                o.set("handle", h);
                o.set("owner", hx(val(&g, 330.0)));
                o.set("type", ty);
                o.set("fileName", sv(val(&g, 1.0), ""));
                o.set("size", point(&g, 10.0, false, None));
                o.set("loaded", is_n(val(&g, 280.0), 1.0));
                self.objects.push(o);
            } else if ty != "DICTIONARY" && ty != "ENDSEC" {
                let mut o = Obj::with_capacity(3);
                let h = self.handle_of(&g);
                o.set("handle", h);
                o.set("owner", hx(val(&g, 330.0)));
                o.set("type", ty);
                self.objects.push(o);
            }
        }
    }

    // ---- one entity ----
    #[allow(clippy::too_many_lines)]
    fn entity(&mut self, ty: &str, g: &G, follow: Option<&[Follow]>) -> Obj {
        let lw = val(g, 370.0);
        let mut e = Obj::with_capacity(24);
        let h = self.handle_of(g);
        e.set("handle", h.clone());
        e.set("type", ty);
        e.set("owner", hx(val(g, 330.0)));
        e.set("layer", sv(val(g, 8.0), "0"));
        e.set("linetype", sv(val(g, 6.0), "ByLayer"));
        e.set("color", color_of(nn(val(g, 62.0), 256.0), val(g, 420.0)));
        e.set("lineweight", nn(lw, -1.0));
        e.set("ltscale", nn(val(g, 48.0), 1.0));
        e.set("invisible", is_n(val(g, 60.0), 1.0));
        if let Some(xd) = xdata(g) {
            e.set("xdata", xd);
        }
        let ext = point(g, 210.0, true, Some(p3(0.0, 0.0, 1.0)));
        let p = |c: f64| pt(g, c);
        match ty {
            "LINE" => {
                e.set("start", p(10.0));
                e.set("end", p(11.0));
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("extrusion", ext);
            }
            "POINT" => {
                e.set("position", p(10.0));
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("extrusion", ext);
                e.set("xAxisAngle", angle(val(g, 50.0)));
            }
            "CIRCLE" => {
                e.set("center", p(10.0));
                e.set("radius", or(val(g, 40.0), 0.0));
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("extrusion", ext);
            }
            "ARC" => {
                e.set("center", p(10.0));
                e.set("radius", or(val(g, 40.0), 0.0));
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("extrusion", ext);
                e.set("startAngle", angle(val(g, 50.0)));
                e.set("endAngle", angle(val(g, 51.0)));
            }
            "ELLIPSE" => {
                e.set("center", p(10.0));
                e.set("majorAxis", p(11.0));
                e.set("extrusion", ext);
                e.set("ratio", nn(val(g, 40.0), 1.0));
                e.set("startParam", or(val(g, 41.0), 0.0));
                e.set("endParam", nn(val(g, 42.0), 2.0 * std::f64::consts::PI));
            }
            "LWPOLYLINE" => {
                let mut vs: Vec<Obj> = Vec::new();
                for (c, v) in g {
                    if *c == 10.0 {
                        let mut o = Obj::with_capacity(5);
                        o.set("x", v.clone());
                        o.set("y", 0.0);
                        o.set("bulge", 0.0);
                        o.set("startWidth", 0.0);
                        o.set("endWidth", 0.0);
                        vs.push(o);
                        continue;
                    }
                    let Some(last) = vs.last_mut() else { continue };
                    let k = match *c {
                        20.0 => "y",
                        40.0 => "startWidth",
                        41.0 => "endWidth",
                        42.0 => "bulge",
                        _ => continue,
                    };
                    last.set(k, v.clone());
                }
                let fl = or(val(g, 70.0), 0.0);
                let cw = or(val(g, 43.0), 0.0);
                if cw.truthy() && !has(g, 40.0) && !has(g, 41.0) {
                    // as DWG stores it
                    for v in &mut vs {
                        v.set("startWidth", cw.clone());
                        v.set("endWidth", cw.clone());
                    }
                }
                e.set("constantWidth", cw);
                e.set("elevation", or(val(g, 38.0), 0.0));
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("extrusion", ext);
                e.set("closed", bit(&fl, 1));
                e.set("plinegen", bit(&fl, 128));
                e.set("vertices", V::Arr(vs.into_iter().map(V::Obj).collect()));
            }
            "POLYLINE" => {
                let flags = or(val(g, 70.0), 0.0);
                let vx: Vec<(&[(f64, V)], V)> = follow
                    .unwrap_or(&[])
                    .iter()
                    .filter(|(t, _)| t == "VERTEX")
                    .map(|(_, sg)| (sg.as_slice(), or(val(sg, 70.0), 0.0)))
                    .collect();
                let vert = |sg: &G, fv: &V| -> V {
                    let mut o = match pt(sg, 10.0) {
                        V::Obj(o) => o,
                        _ => Obj::new(),
                    };
                    o.set("flags", fv.clone());
                    V::Obj(o)
                };
                if bit(&flags, 64) {
                    let vertices: Vec<V> = vx.iter().filter(|(_, fv)| bit(fv, 64)).map(|(sg, fv)| vert(sg, fv)).collect();
                    let faces: Vec<V> = vx
                        .iter()
                        .filter(|(_, fv)| bit(fv, 128) && !bit(fv, 64))
                        .map(|(sg, _)| {
                            V::Arr(vec![or(val(sg, 71.0), 0.0), or(val(sg, 72.0), 0.0), or(val(sg, 73.0), 0.0), or(val(sg, 74.0), 0.0)])
                        })
                        .collect();
                    e.set("type", "POLYLINE_PFACE");
                    e.set("numVertices", or(val(g, 71.0), vertices.len()));
                    e.set("numFaces", or(val(g, 72.0), faces.len()));
                    e.set("flags", flags);
                    e.set("vertices", vertices);
                    e.set("faces", faces);
                    e.set("closed", false);
                } else if bit(&flags, 16) {
                    e.set("type", "POLYLINE_MESH");
                    e.set("flags", flags.clone());
                    e.set("curveType", or(val(g, 75.0), 0.0));
                    e.set("m", or(val(g, 71.0), 0.0));
                    e.set("n", or(val(g, 72.0), 0.0));
                    e.set("mDensity", or(val(g, 73.0), 0.0));
                    e.set("nDensity", or(val(g, 74.0), 0.0));
                    e.set("vertices", vx.iter().map(|(sg, fv)| vert(sg, fv)).collect::<Vec<V>>());
                    e.set("closed", bit(&flags, 1));
                } else if bit(&flags, 8) {
                    e.set("type", "POLYLINE_3D");
                    e.set("flags", flags.clone());
                    e.set("curveType", or(val(g, 75.0), 0.0));
                    e.set("closed", bit(&flags, 1));
                    e.set("vertices", vx.iter().map(|(sg, fv)| vert(sg, fv)).collect::<Vec<V>>());
                } else {
                    e.set("type", "POLYLINE_2D");
                    e.set("flags", flags.clone());
                    e.set("curveType", or(val(g, 75.0), 0.0));
                    e.set("startWidth", or(val(g, 40.0), 0.0));
                    e.set("endWidth", or(val(g, 41.0), 0.0));
                    e.set("thickness", or(val(g, 39.0), 0.0));
                    e.set("elevation", p(10.0).as_obj().and_then(|o| o.get("z")).cloned().unwrap_or(V::Num(0.0)));
                    e.set("extrusion", ext);
                    e.set("closed", bit(&flags, 1));
                    let verts: Vec<V> = vx
                        .iter()
                        .map(|(sg, fv)| {
                            let mut o = match pt(sg, 10.0) {
                                V::Obj(o) => o,
                                _ => Obj::new(),
                            };
                            o.set("startWidth", or(val(sg, 40.0), 0.0));
                            o.set("endWidth", or(val(sg, 41.0), 0.0));
                            o.set("bulge", or(val(sg, 42.0), 0.0));
                            o.set("tangent", angle(val(sg, 50.0)));
                            o.set("flags", fv.clone());
                            V::Obj(o)
                        })
                        .collect();
                    e.set("vertices", verts);
                }
            }
            "TEXT" | "ATTRIB" | "ATTDEF" => self.text_ent(&mut e, g, ext),
            "MTEXT" => {
                let cut = g.iter().position(|(c, _)| *c == 101.0);
                // an embedded object (columns, R2018) repeats codes
                let g = match cut {
                    Some(c) => g.get(..c).unwrap_or(g),
                    None => g,
                };
                let x_axis = if val(g, 11.0).is_some() {
                    pt(g, 11.0)
                } else {
                    let a = num(Some(&or(val(g, 50.0), 0.0))) * D2R;
                    p3(a.cos(), a.sin(), 0.0)
                };
                let mut value = String::new();
                for (c, v) in g {
                    if *c == 3.0 {
                        value.push_str(&js_string(v));
                    }
                }
                if let V::Str(s) = sv(val(g, 1.0), "") {
                    value.push_str(&s);
                }
                e.set("insert", pt(g, 10.0));
                e.set("extrusion", ext);
                e.set("xAxis", x_axis);
                e.set("width", or(val(g, 41.0), 0.0));
                e.set("rectHeight", or(val(g, 46.0), 0.0));
                e.set("height", or(val(g, 40.0), 0.0));
                e.set("attach", or(val(g, 71.0), 1.0));
                e.set("direction", or(val(g, 72.0), 1.0));
                e.set("value", decode(Some(&V::Str(value))));
                e.set("style", sv(val(g, 7.0), "Standard"));
                e.set("lineSpacingStyle", or(val(g, 73.0), 1.0));
                e.set("lineSpacing", or(val(g, 44.0), 1.0));
            }
            "INSERT" => {
                let cols = or(val(g, 70.0), 1.0);
                let rows = or(val(g, 71.0), 1.0);
                let mut attribs = Vec::new();
                for (t, sg) in follow.unwrap_or(&[]) {
                    if t == "ATTRIB" {
                        let mut a = self.entity("ATTRIB", sg, None);
                        a.set("owner", h.clone());
                        attribs.push(V::Obj(a));
                    }
                }
                let multi = num(Some(&cols)) * num(Some(&rows)) > 1.0;
                e.set("type", if multi { "MINSERT" } else { "INSERT" });
                e.set("insert", p(10.0));
                let mut sc = Obj::with_capacity(3);
                sc.set("x", nn(val(g, 41.0), 1.0));
                sc.set("y", nn(val(g, 42.0), 1.0));
                sc.set("z", nn(val(g, 43.0), 1.0));
                e.set("scale", sc);
                e.set("rotation", angle(val(g, 50.0)));
                e.set("extrusion", ext);
                e.set("block", sv(val(g, 2.0), ""));
                e.set("attribs", attribs);
                if multi {
                    e.set("cols", cols);
                    e.set("rows", rows);
                    e.set("colSpacing", or(val(g, 44.0), 0.0));
                    e.set("rowSpacing", or(val(g, 45.0), 0.0));
                }
            }
            "DIMENSION" | "ARC_DIMENSION" => {
                let flags = or(val(g, 70.0), 0.0);
                let arc = ty == "ARC_DIMENSION";
                let dim_type = if arc { 8 } else { to_i32(num(Some(&flags))) & 7 };
                let tname = if arc {
                    ty
                } else {
                    usize::try_from(dim_type).ok().and_then(|k| DIM_TYPES.get(k)).copied().unwrap_or("DIMENSION_LINEAR")
                };
                e.set("type", tname);
                e.set("dimType", dim_type);
                e.set("flags", flags);
                e.set("block", sv(val(g, 2.0), ""));
                e.set("style", sv(val(g, 3.0), "Standard"));
                e.set("extrusion", ext);
                e.set("defPoint", p(10.0));
                e.set("textMid", p(11.0));
                e.set("insert", p(12.0));
                e.set("text", sv(val(g, 1.0), ""));
                e.set("measurement", or(val(g, 42.0), 0.0));
                e.set("attach", or(val(g, 71.0), 5.0));
                e.set("lineSpacingStyle", or(val(g, 72.0), 1.0));
                e.set("lineSpacing", or(val(g, 41.0), 1.0));
                e.set("textRotation", angle(val(g, 53.0)));
                e.set("horizDir", angle(val(g, 51.0)));
                for (c, k) in [(13.0, "xline1"), (14.0, "xline2"), (15.0, "center"), (16.0, "arcPoint")] {
                    if has(g, c) {
                        e.set(k, p(c));
                    }
                }
                if let Some(l) = val(g, 40.0) {
                    e.set("leaderLen", l.clone());
                }
                if dim_type <= 1 {
                    e.set("angle", angle(val(g, 50.0)));
                    e.set("obliqueAngle", angle(val(g, 52.0)));
                }
            }
            "LEADER" => {
                e.set("vertices", points(g, 10.0, true));
                e.set("style", sv(val(g, 3.0), ""));
                e.set("arrowhead", nn(val(g, 71.0), 1.0));
                e.set("pathType", or(val(g, 72.0), 0.0));
                e.set("annotType", nn(val(g, 73.0), 3.0));
                e.set("annotation", hx(val(g, 340.0)));
                e.set("extrusion", ext);
            }
            "HATCH" => self.hatch(&mut e, g, ext),
            "SOLID" | "TRACE" => {
                let pts = vec![p(10.0), p(11.0), p(12.0), if has(g, 13.0) { p(13.0) } else { p(12.0) }];
                e.set("thickness", or(val(g, 39.0), 0.0));
                e.set("elevation", pts.first().and_then(V::as_obj).and_then(|o| o.get("z")).cloned().unwrap_or(V::Num(0.0)));
                e.set("points", pts);
                e.set("extrusion", ext);
            }
            "3DFACE" => {
                e.set("points", vec![p(10.0), p(11.0), p(12.0), if has(g, 13.0) { p(13.0) } else { p(12.0) }]);
                e.set("invisibleEdges", or(val(g, 70.0), 0.0));
            }
            "SPLINE" => {
                let fl = or(val(g, 70.0), 0.0);
                e.set("degree", or(val(g, 71.0), 3.0));
                e.set("flags", fl.clone());
                e.set("closed", bit(&fl, 1));
                e.set("periodic", bit(&fl, 2));
                e.set("rational", bit(&fl, 4));
                e.set("extrusion", ext);
                e.set("knots", g.iter().filter(|(c, _)| *c == 40.0).map(|(_, v)| v.clone()).collect::<Vec<V>>());
                e.set("control", points(g, 10.0, true));
                e.set("fit", points(g, 11.0, true));
                e.set("knotTolerance", nn(val(g, 42.0), 1e-10));
                e.set("controlTolerance", nn(val(g, 43.0), 1e-10));
                e.set("fitTolerance", nn(val(g, 44.0), 1e-10));
                let w: Vec<V> = g.iter().filter(|(c, _)| *c == 41.0).map(|(_, v)| v.clone()).collect();
                if !w.is_empty() {
                    e.set("weights", w);
                }
                if has(g, 12.0) {
                    e.set("startTangent", p(12.0));
                }
                if has(g, 13.0) {
                    e.set("endTangent", p(13.0));
                }
            }
            "RAY" | "XLINE" => {
                e.set("start", p(10.0));
                e.set("direction", p(11.0));
            }
            "VIEWPORT" => {
                e.set("center", p(10.0));
                e.set("width", or(val(g, 40.0), 0.0));
                e.set("height", or(val(g, 41.0), 0.0));
                e.set("id", or(val(g, 69.0), 0.0));
                e.set("viewCenter", point(g, 12.0, false, None));
                e.set("viewTarget", p(17.0));
                e.set("viewDirection", point(g, 16.0, true, Some(p3(0.0, 0.0, 1.0))));
                e.set("viewHeight", or(val(g, 45.0), 0.0));
                e.set("twist", angle(val(g, 51.0)));
                e.set("lensLength", nn(val(g, 42.0), 50.0));
                e.set("frontClip", or(val(g, 43.0), 0.0));
                e.set("backClip", or(val(g, 44.0), 0.0));
                e.set("status", or(val(g, 90.0), 0.0));
                e.set("frozenLayers", g.iter().filter(|(c, _)| *c == 331.0).map(|(_, v)| V::Str(hx(Some(v)))).collect::<Vec<V>>());
            }
            "IMAGE" | "WIPEOUT" => {
                e.set("insert", p(10.0));
                e.set("u", p(11.0));
                e.set("v", p(12.0));
                e.set("size", point(g, 13.0, false, None));
                e.set("def", hx(val(g, 340.0)));
                e.set("defReactor", hx(val(g, 360.0)));
                e.set("displayFlags", or(val(g, 70.0), 0.0));
                e.set("clipping", is_n(val(g, 280.0), 1.0));
                e.set("brightness", nn(val(g, 281.0), 50.0));
                e.set("contrast", nn(val(g, 282.0), 50.0));
                e.set("fade", or(val(g, 283.0), 0.0));
                e.set("clipType", or(val(g, 71.0), 1.0));
                e.set("clip", points(g, 14.0, false));
            }
            "TOLERANCE" => {
                e.set("insert", p(10.0));
                e.set("xDirection", p(11.0));
                e.set("extrusion", ext);
                e.set("text", sv(val(g, 1.0), ""));
                e.set("style", sv(val(g, 3.0), ""));
            }
            "MLINE" => {
                let (p11, p12, p13) = (points(g, 11.0, true), points(g, 12.0, true), points(g, 13.0, true));
                let vs: Vec<V> = p11
                    .iter()
                    .enumerate()
                    .map(|(k, q)| {
                        let mut o = Obj::with_capacity(3);
                        o.set("point", q.clone());
                        o.set("direction", p12.get(k).cloned().unwrap_or_else(|| p3(1.0, 0.0, 0.0)));
                        o.set("miter", p13.get(k).cloned().unwrap_or_else(|| p3(0.0, 1.0, 0.0)));
                        V::Obj(o)
                    })
                    .collect();
                e.set("style", sv(val(g, 2.0), "STANDARD"));
                e.set("scale", nn(val(g, 40.0), 1.0));
                e.set("justification", or(val(g, 70.0), 0.0));
                e.set("closed", bit(&or(val(g, 71.0), 0.0), 2));
                e.set("base", p(10.0));
                e.set("extrusion", ext);
                e.set("vertices", vs);
            }
            "MULTILEADER" => self.mleader(&mut e, g),
            "ACAD_TABLE" => {
                e.set("insert", p(10.0));
                e.set("extrusion", ext);
                e.set("block", sv(val(g, 2.0), ""));
                e.set("partial", true);
            }
            "3DSOLID" | "REGION" | "BODY" | "LIGHT" | "ACAD_PROXY_ENTITY" => {} // type only
            _ => {
                e.set("type", "UNKNOWN");
                e.set("dxfName", ty);
            }
        }
        e
    }

    fn text_ent(&mut self, e: &mut Obj, g: &G, ext: V) {
        // With subclass markers, the text's own values are the AcDbText ones; ATTRIB / ATTDEF repeat some codes in
        // their own subclass (and keep the vertical alignment in 74, 73 being the field length there).
        let ty = e.str("type").unwrap_or("").to_string();
        let att = ty != "TEXT";
        let marked = has(g, 100.0);
        let own = if ty == "ATTRIB" { "AcDbAttribute" } else { "AcDbAttributeDefinition" };
        let t = |c: f64| if marked { sub_val(g, "AcDbText", c) } else { val(g, c) };
        let a = |c: f64| if marked { sub_val(g, own, c) } else { val(g, c) };
        let insert = if marked { sub_point(g, "AcDbText", 10.0, true) } else { pt(g, 10.0) };
        let align = if marked {
            if sub_val(g, "AcDbText", 11.0).is_some() { Some(sub_point(g, "AcDbText", 11.0, true)) } else { None }
        } else if has(g, 11.0) {
            Some(pt(g, 11.0))
        } else {
            None
        };
        e.set("insert", insert.clone());
        e.set("align", align.unwrap_or_else(|| insert.clone()));
        e.set("height", or(t(40.0), 0.0));
        e.set("value", decode(t(1.0)));
        e.set("rotation", angle(t(50.0)));
        e.set("widthFactor", nn(t(41.0), 1.0));
        e.set("oblique", angle(t(51.0)));
        e.set("style", sv(t(7.0), "Standard"));
        e.set("hAlign", or(t(72.0), 0.0));
        e.set("vAlign", or(if att { a(74.0) } else { t(73.0) }, 0.0));
        e.set("generation", or(t(71.0), 0.0));
        e.set("thickness", or(t(39.0), 0.0));
        e.set("elevation", insert.as_obj().and_then(|o| o.get("z")).cloned().unwrap_or(V::Num(0.0)));
        e.set("extrusion", ext);
        if att {
            e.set("tag", sv(a(2.0), ""));
            e.set("flags", or(a(70.0), 0.0));
            e.set("lockPosition", is_n(a(280.0), 1.0));
            e.set("fieldLength", or(a(73.0), 0.0));
            if ty == "ATTDEF" {
                e.set("prompt", decode(a(3.0)));
            }
        }
    }

    /// HATCH: its groups repeat per loop and edge, so they are read in order.
    #[allow(clippy::too_many_lines)]
    fn hatch(&mut self, h: &mut Obj, g: &G, ext: V) {
        let mut k = 0usize;
        let at = |k: usize, c: f64| g.get(k).is_some_and(|(cc, _)| *cc == c);
        let take = |k: &mut usize| -> V {
            let v = g.get(*k).map(|(_, v)| v.clone()).unwrap_or(V::Null);
            *k = k.saturating_add(1);
            v
        };
        let xy = |k: &mut usize, cx: f64| -> V {
            let mut p = Obj::with_capacity(2);
            p.set("x", 0.0);
            p.set("y", 0.0);
            if at(*k, cx) {
                p.set("x", take(k));
            }
            if at(*k, cx + 10.0) {
                p.set("y", take(k));
            }
            V::Obj(p)
        };
        h.set("pattern", sv(val(g, 2.0), ""));
        h.set("solid", is_n(val(g, 70.0), 1.0));
        h.set("associative", is_n(val(g, 71.0), 1.0));
        h.set("elevation", pt(g, 10.0).as_obj().and_then(|o| o.get("z")).cloned().unwrap_or(V::Num(0.0)));
        h.set("extrusion", ext);
        let mut loops: Vec<V> = Vec::new();
        let mut seeds: Vec<V> = Vec::new();
        while k < g.len() && !at(k, 91.0) {
            k = k.saturating_add(1);
        }
        let n_loops = if k < g.len() { num(Some(&take(&mut k))) } else { 0.0 };
        let mut l = 0.0;
        while l < n_loops && k < g.len() {
            l += 1.0;
            while k < g.len() && !at(k, 92.0) {
                k = k.saturating_add(1);
            }
            if k >= g.len() {
                break;
            }
            let flags = take(&mut k);
            let mut lp = Obj::with_capacity(4);
            lp.set("flags", flags.clone());
            if bit(&flags, 2) {
                let has_bulges = if at(k, 72.0) { is_n(Some(&take(&mut k)), 1.0) } else { false };
                let closed = if at(k, 73.0) { is_n(Some(&take(&mut k)), 1.0) } else { true };
                let cnt = if at(k, 93.0) { num(Some(&take(&mut k))) } else { 0.0 };
                let mut vertices = Vec::new();
                let mut v = 0.0;
                while v < cnt && k < g.len() && vertices.len() < MAX_LOOP.saturating_add(g.len()) {
                    v += 1.0;
                    let p = xy(&mut k, 10.0);
                    let mut o = Obj::with_capacity(3);
                    if let V::Obj(p) = p {
                        o.set("x", p.get("x").cloned().unwrap_or(V::Num(0.0)));
                        o.set("y", p.get("y").cloned().unwrap_or(V::Num(0.0)));
                    }
                    o.set("bulge", if at(k, 42.0) { take(&mut k) } else { V::Num(0.0) });
                    vertices.push(V::Obj(o));
                }
                let mut pl = Obj::with_capacity(3);
                pl.set("closed", closed);
                pl.set("hasBulges", has_bulges);
                pl.set("vertices", vertices);
                lp.set("polyline", pl);
            } else {
                let cnt = if at(k, 93.0) { num(Some(&take(&mut k))) } else { 0.0 };
                let mut edges = Vec::new();
                let mut d = 0.0;
                while d < cnt && k < g.len() {
                    d += 1.0;
                    while k < g.len() && !at(k, 72.0) {
                        k = k.saturating_add(1);
                    }
                    if k >= g.len() {
                        break; // the JS reads past the end here (TypeError)
                    }
                    let tv = num(Some(&take(&mut k)));
                    let ty = if (1.0..=4.0).contains(&tv) && tv == tv.trunc() {
                        HATCH_EDGE.get(crate::util::to_usize(tv - 1.0)).copied()
                    } else {
                        None
                    };
                    let mut ed = Obj::with_capacity(8);
                    match ty {
                        Some("line") => {
                            ed.set("type", "line");
                            ed.set("start", xy(&mut k, 10.0));
                            ed.set("end", xy(&mut k, 11.0));
                        }
                        Some(t @ ("arc" | "ellipse")) => {
                            ed.set("type", t);
                            ed.set("center", xy(&mut k, 10.0));
                            if t == "ellipse" {
                                ed.set("majorAxis", xy(&mut k, 11.0));
                                ed.set("ratio", if at(k, 40.0) { take(&mut k) } else { V::Num(1.0) });
                            } else {
                                ed.set("radius", if at(k, 40.0) { take(&mut k) } else { V::Num(0.0) });
                            }
                            let s = if at(k, 50.0) { num(Some(&take(&mut k))) } else { 0.0 };
                            let tt = if at(k, 51.0) { num(Some(&take(&mut k))) } else { 360.0 };
                            ed.set("startAngle", s * D2R);
                            ed.set("endAngle", tt * D2R);
                            ed.set("ccw", if at(k, 73.0) { is_n(Some(&take(&mut k)), 1.0) } else { true });
                        }
                        Some("spline") => {
                            ed.set("type", "spline");
                            ed.set("degree", if at(k, 94.0) { take(&mut k) } else { V::Num(3.0) });
                            ed.set("rational", if at(k, 73.0) { is_n(Some(&take(&mut k)), 1.0) } else { false });
                            ed.set("periodic", if at(k, 74.0) { is_n(Some(&take(&mut k)), 1.0) } else { false });
                            let nk = if at(k, 95.0) { num(Some(&take(&mut k))) } else { 0.0 };
                            let nc = if at(k, 96.0) { num(Some(&take(&mut k))) } else { 0.0 };
                            let mut knots = Vec::new();
                            let mut q = 0.0;
                            while q < nk && at(k, 40.0) {
                                q += 1.0;
                                knots.push(take(&mut k));
                            }
                            let mut control = Vec::new();
                            let mut w = Vec::new();
                            q = 0.0;
                            while q < nc && at(k, 10.0) {
                                q += 1.0;
                                control.push(xy(&mut k, 10.0));
                                if at(k, 42.0) {
                                    w.push(take(&mut k));
                                }
                            }
                            ed.set("knots", knots);
                            ed.set("control", control);
                            if !w.is_empty() {
                                ed.set("weights", w);
                            }
                            if at(k, 97.0) {
                                let nf = num(Some(&take(&mut k)));
                                let mut fit = Vec::new();
                                q = 0.0;
                                while q < nf && at(k, 11.0) {
                                    q += 1.0;
                                    fit.push(xy(&mut k, 11.0));
                                }
                                ed.set("fit", fit);
                            }
                            if at(k, 12.0) {
                                ed.set("startTangent", xy(&mut k, 12.0));
                            }
                            if at(k, 13.0) {
                                ed.set("endTangent", xy(&mut k, 13.0));
                            }
                        }
                        _ => break,
                    }
                    edges.push(V::Obj(ed));
                }
                lp.set("edges", edges);
            }
            let mut boundary = Vec::new();
            if at(k, 97.0) {
                let nb = num(Some(&take(&mut k)));
                let mut b = 0.0;
                while b < nb && at(k, 330.0) {
                    b += 1.0;
                    boundary.push(V::Str(hx(Some(&take(&mut k)))));
                }
            }
            lp.set("boundary", boundary);
            loops.push(V::Obj(lp));
        }
        while k < g.len() {
            let Some((c, v)) = g.get(k) else { break };
            let c = *c;
            if c == 75.0 {
                h.set("style", v.clone());
            } else if c == 76.0 {
                h.set("patternType", v.clone());
            } else if c == 52.0 {
                h.set("angle", num(Some(v)) * D2R);
            } else if c == 41.0 {
                h.set("scale", v.clone());
            } else if c == 77.0 {
                h.set("double", is_n(Some(v), 1.0));
            } else if c == 98.0 {
                let ns = num(Some(v));
                k = k.saturating_add(1);
                let mut s = 0.0;
                while s < ns && at(k, 10.0) {
                    s += 1.0;
                    let mut p = Obj::with_capacity(2);
                    p.set("x", g.get(k).map(|(_, v)| v.clone()).unwrap_or(V::Null));
                    p.set("y", 0.0);
                    k = k.saturating_add(1);
                    if at(k, 20.0) {
                        p.set("y", g.get(k).map(|(_, v)| v.clone()).unwrap_or(V::Null));
                        k = k.saturating_add(1);
                    }
                    seeds.push(V::Obj(p));
                }
                k = k.saturating_sub(1);
            }
            k = k.saturating_add(1);
        }
        h.set("loops", loops);
        h.set("seeds", seeds);
    }

    /// MULTILEADER: the context data's leaders (lines), text and block, as cc-dwg models them.
    #[allow(clippy::too_many_lines)]
    fn mleader(&mut self, m: &mut Obj, g: &G) {
        struct Root {
            connection: V,
            direction: V,
            lines: Vec<Vec<V>>,
            dogleg: Option<V>,
        }
        let mut roots: Vec<Root> = Vec::new();
        let mut text: Option<Obj> = None;
        let mut block: Option<Obj> = None;
        let mut extra = Obj::new(); // textHeight, arrowSize, hasText, hasBlock (set as read)
        let mut has_root = false;
        let mut has_line = false;
        let mut depth = "";
        // the MLEADER's own values follow the context block (301 "}"); codes inside it mean other things
        let end = g.iter().position(|(c, v)| *c == 301.0 && matches!(v, V::Str(s) if s == "}"));
        let own: &G = match end {
            Some(e) => g.get(e.saturating_add(1)..).unwrap_or(&[]),
            None => g,
        };
        m.set("style", hx(val(own, 340.0)));
        m.set("leaderType", nn(val(own, 170.0), 1.0));
        m.set("contentType", nn(val(own, 172.0), 2.0));
        m.set("doglegEnabled", !matches!(val(own, 291.0), Some(V::Bool(false))));
        m.set("landingEnabled", !matches!(val(own, 290.0), Some(V::Bool(false))));
        m.set("arrowSize", or(val(own, 42.0), 0.0));
        let is_s = |v: &V, s: &str| matches!(v, V::Str(x) if x == s);
        let mut k = 0usize;
        while k < g.len() {
            let Some((c, v)) = g.get(k) else { break };
            let c = *c;
            let pt_at = |k: &mut usize, cx: f64| -> V {
                let mut p = Obj::with_capacity(3);
                p.set("x", v.clone());
                p.set("y", 0.0);
                p.set("z", 0.0);
                if let Some((c2, v2)) = g.get(k.saturating_add(1))
                    && *c2 == cx + 10.0
                {
                    p.set("y", v2.clone());
                    *k = k.saturating_add(1);
                }
                if let Some((c3, v3)) = g.get(k.saturating_add(1))
                    && *c3 == cx + 20.0
                {
                    p.set("z", v3.clone());
                    *k = k.saturating_add(1);
                }
                V::Obj(p)
            };
            // section markers: 300 CONTEXT_DATA{ · 302 LEADER{ · 304 LEADER_LINE{ · 305 } · 303 } · 301 }
            if c == 300.0 && is_s(v, "CONTEXT_DATA{") {
                depth = "ctx";
            } else if c == 302.0 && is_s(v, "LEADER{") {
                roots.push(Root { connection: V::Null, direction: V::Null, lines: Vec::new(), dogleg: None });
                has_root = true;
                has_line = false;
                depth = "leader";
            } else if c == 304.0 && is_s(v, "LEADER_LINE{") && has_root {
                if let Some(r) = roots.last_mut() {
                    r.lines.push(Vec::new());
                }
                has_line = true;
                depth = "line";
            } else if c == 304.0 && depth == "ctx" {
                text.get_or_insert_with(Obj::new).set("label", js_string(v));
            } else if c == 305.0 && is_s(v, "}") {
                depth = "leader";
            } else if c == 303.0 && is_s(v, "}") {
                depth = "ctx";
            } else if c == 301.0 && is_s(v, "}") {
                depth = "";
            } else if depth == "line" && c == 10.0 {
                let p = pt_at(&mut k, 10.0);
                if has_line && let Some(l) = roots.last_mut().and_then(|r| r.lines.last_mut()) {
                    l.push(p);
                }
            } else if depth == "leader" && c == 10.0 {
                let p = pt_at(&mut k, 10.0);
                if let Some(r) = roots.last_mut() {
                    r.connection = p;
                }
            } else if depth == "leader" && c == 11.0 {
                let p = pt_at(&mut k, 11.0);
                if let Some(r) = roots.last_mut() {
                    r.direction = p;
                }
            } else if depth == "leader" && c == 40.0 {
                if let Some(r) = roots.last_mut() {
                    r.dogleg = Some(v.clone());
                }
            } else if depth == "ctx" && c == 41.0 {
                extra.set("textHeight", v.clone());
            } else if depth == "ctx" && c == 140.0 {
                extra.set("arrowSize", v.clone());
            } else if depth == "ctx" && c == 12.0 {
                let p = pt_at(&mut k, 12.0);
                text.get_or_insert_with(Obj::new).set("location", p);
            } else if depth == "ctx" && c == 13.0 {
                let p = pt_at(&mut k, 13.0);
                text.get_or_insert_with(Obj::new).set("direction", p);
            } else if depth == "ctx" && c == 290.0 {
                extra.set("hasText", v.clone());
            } else if depth == "ctx" && c == 296.0 {
                extra.set("hasBlock", v.clone());
            } else if depth == "ctx" && c == 341.0 {
                block.get_or_insert_with(Obj::new).set("handle", hx(Some(v)));
            } else if depth == "ctx" && c == 15.0 {
                let p = pt_at(&mut k, 15.0);
                block.get_or_insert_with(Obj::new).set("location", p);
            } else if depth == "ctx" && c == 16.0 {
                let p = pt_at(&mut k, 16.0);
                block.get_or_insert_with(Obj::new).set("scale", p);
            } else if depth == "ctx" && c == 46.0 {
                block.get_or_insert_with(Obj::new).set("rotation", v.clone());
            } else if depth == "ctx"
                && c == 47.0
                && let Some(b) = block.as_mut()
            {
                if !matches!(b.get("matrix"), Some(V::Arr(_))) {
                    b.set("matrix", V::Arr(Vec::new()));
                }
                if let Some(V::Arr(a)) = b.get_mut("matrix") {
                    a.push(v.clone());
                }
            }
            k = k.saturating_add(1);
        }
        let mut ctx = Obj::with_capacity(8);
        ctx.set(
            "roots",
            roots
                .into_iter()
                .map(|r| {
                    let mut o = Obj::with_capacity(4);
                    o.set("connection", r.connection);
                    o.set("direction", r.direction);
                    o.set(
                        "lines",
                        r.lines
                            .into_iter()
                            .map(|l| {
                                let mut lo = Obj::new();
                                lo.set("points", l);
                                V::Obj(lo)
                            })
                            .collect::<Vec<V>>(),
                    );
                    if let Some(d) = r.dogleg {
                        o.set("doglegLength", d);
                    }
                    V::Obj(o)
                })
                .collect::<Vec<V>>(),
        );
        let block_handle = block.as_ref().and_then(|b| b.get("handle")).filter(|v| v.truthy()).cloned();
        ctx.set("text", text.map_or(V::Null, V::Obj));
        ctx.set("block", block.map_or(V::Null, V::Obj));
        ctx.assign(extra);
        m.set("context", ctx);
        // block attribute values: 330 attdef, 177 index, 44 width, 302 text, in that order
        let mut labels = Vec::new();
        let mut k = 0usize;
        while k < own.len() {
            if let Some((c, v)) = own.get(k)
                && *c == 330.0
            {
                let mut l = Obj::with_capacity(4);
                l.set("attdefHandle", hx(Some(v)));
                l.set("text", "");
                l.set("index", 0.0);
                l.set("width", 0.0);
                k = k.saturating_add(1);
                while let Some((c2, v2)) = own.get(k) {
                    if *c2 == 177.0 {
                        l.set("index", v2.clone());
                    } else if *c2 == 44.0 {
                        l.set("width", v2.clone());
                    } else if *c2 == 302.0 {
                        l.set("text", decode(Some(v2)));
                    } else {
                        break;
                    }
                    k = k.saturating_add(1);
                }
                k = k.saturating_sub(1);
                labels.push(V::Obj(l));
            }
            k = k.saturating_add(1);
        }
        m.set("blockLabels", labels);
        if is_n(m.get("contentType"), 1.0)
            && let Some(h) = block_handle
        {
            m.set("contentBlock", h);
        }
    }

    /// Blocks, records and layouts tied together; loose entities into their space; counts.
    #[allow(clippy::too_many_lines)]
    fn place(&mut self) -> Vec<V> {
        for b in &mut self.blocks {
            let name = b.obj.str("name").unwrap_or("").to_string();
            let l = name.to_ascii_lowercase();
            if l == "$model_space" || l == "$paper_space" {
                b.obj.set("name", if l.contains("model") { "*Model_Space" } else { "*Paper_Space" }); // R12 names
            }
        }
        let mut rec_by_name: HashMap<String, usize> = HashMap::new();
        let mut rec_by_handle: HashMap<String, usize> = HashMap::new();
        for (i, r) in self.records.iter().enumerate() {
            rec_by_name.insert(r.str("name").unwrap_or("").to_uppercase(), i);
            rec_by_handle.entry(r.str("handle").unwrap_or("").to_string()).or_insert(i);
        }
        for bi in 0..self.blocks.len() {
            let Some(b) = self.blocks.get(bi) else { continue };
            let rec = b.obj.str("record").unwrap_or("").to_string();
            let uname = b.obj.str("name").unwrap_or("").to_uppercase();
            let bname = b.obj.get("name").cloned().unwrap_or(V::Str(String::new()));
            let found = if rec.is_empty() { None } else { rec_by_handle.get(&rec).copied() }.or_else(|| rec_by_name.get(&uname).copied());
            let ri = match found {
                Some(ri) => ri,
                None => {
                    let h = format!("R{}", self.next_auto());
                    let mut r = Obj::with_capacity(10);
                    r.set("handle", h.clone());
                    r.set("name", bname);
                    r.set("flags", 0.0);
                    r.set("layout", "");
                    r.set("insertUnits", 0.0);
                    r.set("explodable", true);
                    r.set("scalable", 0.0);
                    self.records.push(r);
                    let ri = self.records.len().saturating_sub(1);
                    rec_by_handle.entry(h).or_insert(ri);
                    rec_by_name.insert(uname, ri);
                    ri
                }
            };
            let Some(b) = self.blocks.get_mut(bi) else { continue };
            let Some(r) = self.records.get_mut(ri) else { continue };
            let rh = r.str("handle").unwrap_or("").to_string();
            b.obj.set("record", rh.clone());
            for k in ["flags", "base", "xrefPath", "description"] {
                r.set(k, b.obj.get(k).cloned().unwrap_or(V::Null));
            }
            for e in &mut b.entities {
                e.set("owner", rh.clone());
            }
        }
        // `space(name)`: the first block of that name, made up (with its record) when there is none (R12)
        let mut space_cache: HashMap<&'static str, usize> = HashMap::new();
        let space = |me: &mut Self, cache: &mut HashMap<&'static str, usize>, name: &'static str| -> usize {
            if let Some(&i) = cache.get(name) {
                return i;
            }
            let found = me.blocks.iter().position(|b| b.obj.str("name").unwrap_or("").to_uppercase() == name);
            let i = match found {
                Some(i) => i,
                None => {
                    let h = format!("R{}", me.next_auto());
                    let nm = if name == "*MODEL_SPACE" { "*Model_Space" } else { "*Paper_Space" };
                    let mut r = Obj::with_capacity(10);
                    r.set("handle", h.clone());
                    r.set("name", nm);
                    r.set("flags", 0.0);
                    r.set("layout", "");
                    r.set("insertUnits", 0.0);
                    r.set("explodable", true);
                    r.set("scalable", 0.0);
                    r.set("base", p3(0.0, 0.0, 0.0));
                    me.records.push(r);
                    let mut b = Obj::with_capacity(8);
                    b.set("name", nm);
                    b.set("handle", h.clone());
                    b.set("record", h);
                    b.set("base", p3(0.0, 0.0, 0.0));
                    b.set("flags", 0.0);
                    b.set("xrefPath", "");
                    b.set("description", "");
                    me.blocks.push(Block { obj: b, entities: Vec::new() });
                    me.blocks.len().saturating_sub(1)
                }
            };
            cache.insert(name, i);
            i
        };
        let mut by_record: HashMap<String, usize> = HashMap::new();
        for (i, b) in self.blocks.iter().enumerate() {
            by_record.insert(b.obj.str("record").unwrap_or("").to_string(), i);
        }
        let link = |e: &mut Obj, rec_by_name: &HashMap<String, usize>, records: &[Obj]| {
            let t = e.str("type").unwrap_or("");
            if t == "INSERT" || t == "MINSERT" || t == "ACAD_TABLE" {
                let up = e.str("block").unwrap_or("").to_uppercase();
                if let Some(h) = rec_by_name.get(&up).and_then(|&ri| records.get(ri)).and_then(|r| r.get("handle")).cloned() {
                    e.set("blockRecord", h);
                }
            }
        };
        for (e, _) in &mut self.loose {
            link(e, &rec_by_name, &self.records);
        }
        for b in &mut self.blocks {
            for e in &mut b.entities {
                link(e, &rec_by_name, &self.records);
            }
        }
        let loose = std::mem::take(&mut self.loose);
        for (mut e, paper) in loose {
            let owner = e.str("owner").unwrap_or("").to_string();
            let bi = match (!owner.is_empty()).then(|| by_record.get(&owner).copied()).flatten() {
                Some(bi) => bi,
                None => space(self, &mut space_cache, if paper { "*PAPER_SPACE" } else { "*MODEL_SPACE" }),
            };
            if let Some(b) = self.blocks.get_mut(bi) {
                e.set("owner", b.obj.get("record").cloned().unwrap_or(V::Null));
                b.entities.push(e);
            }
        }
        // layouts: the LAYOUT objects, else Model (+ Layout1 when there is paper space)
        let mut layouts: Vec<Obj> = Vec::new();
        for o in &self.objects {
            if o.str("type") != Some("LAYOUT") {
                continue;
            }
            let g = |k: &str| o.get(k).cloned().unwrap_or(V::Null);
            let plot = o.obj("plot");
            let pg = |k: &str| plot.and_then(|p| p.get(k)).cloned().unwrap_or(V::Null);
            let mut l = Obj::with_capacity(12);
            l.set("handle", g("handle"));
            l.set("owner", g("owner"));
            l.set("name", g("name"));
            l.set("tabOrder", g("tabOrder"));
            l.set("flags", g("flags"));
            l.set("block", g("block"));
            let mut paper = Obj::with_capacity(7);
            paper.set("name", pg("paperSize"));
            paper.set("w", pg("paperWidth"));
            paper.set("h", pg("paperHeight"));
            paper.set("units", pg("paperUnits"));
            paper.set("rotation", pg("rotation"));
            paper.set("pageSetupName", pg("pageSetupName"));
            paper.set("printer", pg("printer"));
            l.set("paper", paper);
            let mut lim = Obj::with_capacity(2);
            lim.set("min", g("limMin"));
            lim.set("max", g("limMax"));
            l.set("limits", lim);
            l.set("insbase", g("insBase"));
            let mut ex = Obj::with_capacity(2);
            ex.set("min", g("extMin"));
            ex.set("max", g("extMax"));
            l.set("extents", ex);
            l.set("viewports", V::Arr(Vec::new()));
            layouts.push(l);
        }
        if layouts.is_empty() {
            let ms = space(self, &mut space_cache, "*MODEL_SPACE");
            let hv = |k: &str, d: V| self.header.get(k).filter(|v| v.truthy()).cloned().unwrap_or(d);
            let mut l = Obj::with_capacity(7);
            l.set("handle", "L1");
            l.set("name", "Model");
            l.set("tabOrder", 0.0);
            l.set("flags", 1.0);
            l.set("block", self.blocks.get(ms).and_then(|b| b.obj.get("record")).cloned().unwrap_or(V::Null));
            let mut lim = Obj::with_capacity(2);
            lim.set("min", hv("LIMMIN", p2(0.0, 0.0)));
            lim.set("max", hv("LIMMAX", p2(12.0, 9.0)));
            l.set("limits", lim);
            l.set("viewports", V::Arr(Vec::new()));
            layouts.push(l);
            let ps = self.blocks.iter().find(|b| b.obj.str("name").unwrap_or("").to_uppercase() == "*PAPER_SPACE");
            if let Some(ps) = ps.filter(|b| !b.entities.is_empty()) {
                let mut l = Obj::with_capacity(7);
                l.set("handle", "L2");
                l.set("name", "Layout1");
                l.set("tabOrder", 1.0);
                l.set("flags", 0.0);
                l.set("block", ps.obj.get("record").cloned().unwrap_or(V::Null));
                let mut lim = Obj::with_capacity(2);
                lim.set("min", hv("PLIMMIN", p2(0.0, 0.0)));
                lim.set("max", hv("PLIMMAX", p2(0.0, 0.0)));
                l.set("limits", lim);
                l.set("viewports", V::Arr(Vec::new()));
                layouts.push(l);
            }
        }
        for l in &layouts {
            let bh = l.get("block").map(js_string).unwrap_or_default();
            let ri = if l.str("block").is_some() { rec_by_handle_find(&self.records, &bh) } else { None };
            if let Some(r) = ri.and_then(|ri| self.records.get_mut(ri))
                && !r.truthy("layout")
            {
                r.set("layout", l.get("handle").cloned().unwrap_or(V::Null));
            }
        }
        space(self, &mut space_cache, "*MODEL_SPACE");
        layouts.into_iter().map(V::Obj).collect()
    }
}

/// `T.blockRecords.find((x) => x.handle === h)` (first match).
fn rec_by_handle_find(records: &[Obj], h: &str) -> Option<usize> {
    records.iter().position(|r| r.str("handle") == Some(h))
}

/// `readDxfModel(text)` → the model, or the error message the JS throws.
pub fn try_read_dxf_model(text: &str) -> Result<Obj, String> {
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(msg("dxfmodel.binary", None));
    }
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut codes = Vec::with_capacity(lines.len() / 2);
    let mut vals = Vec::with_capacity(lines.len() / 2);
    let mut i = 0usize;
    while i.saturating_add(1) < lines.len() {
        let line = lines.get(i).copied().unwrap_or("");
        let c = parse_int(line);
        if c.is_nan() {
            if js_trim(line).is_empty() && i.saturating_add(2) >= lines.len() {
                break;
            }
            return Err(msg("dxfmodel.bad_code", Some(i.saturating_add(1))));
        }
        let raw = lines.get(i.saturating_add(1)).copied().unwrap_or("");
        codes.push(c);
        let keep = c == 1.0 || c == 3.0 || (1000.0..=1009.0).contains(&c);
        vals.push(typed(c, if keep { raw } else { js_trim(raw) }));
        i = i.saturating_add(2);
    }
    let pairs: Vec<(f64, V)> = codes.iter().copied().zip(vals.iter().cloned()).collect();
    let mut r = Reader {
        codes,
        vals,
        pairs,
        _text: std::marker::PhantomData,
        i: 0,
        auto: 0,
        header: Obj::new(),
        layers: Vec::new(),
        linetypes: Vec::new(),
        styles: Vec::new(),
        dimstyles: Vec::new(),
        vports: Vec::new(),
        appids: Vec::new(),
        records: Vec::new(),
        blocks: Vec::new(),
        objects: Vec::new(),
        loose: Vec::new(),
    };
    while r.i < r.n() {
        if !r.is0("SECTION") {
            r.i = r.i.saturating_add(1);
            continue;
        }
        r.i = r.i.saturating_add(1);
        let sec = if r.code(r.i) == 2.0 {
            let s = r.vals.get(r.i).map(js_string).unwrap_or_default();
            r.i = r.i.saturating_add(1);
            s
        } else {
            String::new()
        };
        match sec.as_str() {
            "HEADER" => {
                let g = r.group();
                r.read_header(g);
            }
            "TABLES" => r.read_tables(),
            "BLOCKS" => r.read_blocks(),
            "ENTITIES" => {
                let mut loose = std::mem::take(&mut r.loose);
                r.read_entities(&mut loose, true, "ENDSEC");
                r.loose = loose;
            }
            "OBJECTS" => r.read_objects(),
            _ => {}
        }
        while r.i < r.n() && !r.is0("ENDSEC") {
            r.i = r.i.saturating_add(1);
            r.group();
        }
        r.i = r.i.saturating_add(1);
    }
    let layouts = r.place();

    // counts, and each space's entities marked
    let mut counts = Obj::new();
    let mut unknown = Obj::new();
    let mut blocks = Vec::with_capacity(r.blocks.len());
    for b in std::mem::take(&mut r.blocks) {
        let lname = b.obj.str("name").unwrap_or("").to_ascii_lowercase();
        let sp = if lname == "*model_space" {
            Some("model")
        } else if lname.starts_with("*paper_space") {
            Some("paper")
        } else {
            None
        };
        let mut ents = Vec::with_capacity(b.entities.len());
        for mut e in b.entities {
            if let Some(sp) = sp {
                e.set("space", sp);
            }
            let t = e.get("type").map(js_string).unwrap_or_default();
            let n = counts.num(&t).unwrap_or(0.0);
            counts.set(t.clone(), n + 1.0);
            if t == "UNKNOWN" {
                let d = e.get("dxfName").map(js_string).unwrap_or_default();
                let n = unknown.num(&d).unwrap_or(0.0);
                unknown.set(d, n + 1.0);
            }
            ents.push(V::Obj(e));
        }
        let mut o = b.obj;
        o.set("entities", ents);
        blocks.push(V::Obj(o));
    }
    let h = &r.header;
    let mut model = Obj::with_capacity(10);
    model.set("version", h.get("ACADVER").filter(|v| v.truthy()).map(js_string).unwrap_or_default());
    model.set("maintenance", h.get("ACADMAINTVER").filter(|v| v.truthy()).cloned().unwrap_or(V::Num(0.0)));
    model.set("codepage", h.get("DWGCODEPAGE").filter(|v| v.truthy()).map(js_string).unwrap_or_default());
    model.set("header", r.header.clone());
    model.set("classes", V::Arr(Vec::new()));
    let mut tables = Obj::with_capacity(9);
    tables.set("layers", std::mem::take(&mut r.layers));
    tables.set("linetypes", std::mem::take(&mut r.linetypes));
    tables.set("styles", std::mem::take(&mut r.styles));
    tables.set("dimstyles", std::mem::take(&mut r.dimstyles));
    tables.set("views", V::Arr(Vec::new()));
    tables.set("ucs", V::Arr(Vec::new()));
    tables.set("vports", std::mem::take(&mut r.vports));
    tables.set("appids", std::mem::take(&mut r.appids));
    tables.set("blockRecords", std::mem::take(&mut r.records).into_iter().map(V::Obj).collect::<Vec<V>>());
    model.set("tables", tables);
    model.set("blocks", blocks);
    model.set("layouts", layouts);
    model.set("objects", std::mem::take(&mut r.objects).into_iter().map(V::Obj).collect::<Vec<V>>());
    let mut report = Obj::with_capacity(3);
    report.set("counts", counts);
    report.set("notes", V::Arr(Vec::new()));
    report.set("unknown", unknown);
    model.set("report", report);
    Ok(model)
}

/// `readDxfModel(text)`; a file the JS refuses (binary DXF, a bad group code) gives a model holding only
/// `report.error` with the JS message.
pub fn read_dxf_model(text: &str) -> Obj {
    match try_read_dxf_model(text) {
        Ok(m) => m,
        Err(e) => {
            let mut report = Obj::new();
            report.set("error", e);
            let mut m = Obj::new();
            m.set("report", report);
            m
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_parsing() {
        assert_eq!(parse_float("  1.5abc"), 1.5);
        assert_eq!(parse_float("-.5"), -0.5);
        assert_eq!(parse_float("1e"), 1.0);
        assert_eq!(parse_float("2E+3x"), 2000.0);
        assert!(parse_float(".e5").is_nan());
        assert!(parse_float("-").is_nan());
        assert_eq!(parse_int(" 12.9"), 12.0);
        assert_eq!(parse_int("+7"), 7.0);
        assert!(parse_int("x1").is_nan());
        assert_eq!(hx(Some(&V::Str(" 00ab ".into()))), "AB");
    }

    #[test]
    fn minimal_files() {
        let m = read_dxf_model("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWALLS\n10\n1\n20\n2\n11\n3\n21\n4\n0\nENDSEC\n0\nEOF\n");
        let blocks = m.arr("blocks").cloned().unwrap_or_default();
        assert_eq!(blocks.len(), 1);
        let ms = blocks.first().and_then(V::as_obj).cloned().unwrap_or_default();
        assert_eq!(ms.str("name"), Some("*Model_Space"));
        assert_eq!(m.arr("layouts").map(Vec::len), Some(1));
        let bad = read_dxf_model("0\nSECTION\nxx\nyy\n");
        assert!(bad.obj("report").and_then(|r| r.str("error")).is_some_and(|e| e.contains("line 3")));
        assert!(try_read_dxf_model("AutoCAD Binary DXF\r\n").is_err());
        assert_eq!(decode(Some(&V::Str("a^Jb^ c".into()))), V::Str("a\nb^c".into()));
    }
}
