//! dimstyle.js: every dimension style setting on top of dims.js's geometry — the value text (units, round-off,
//! prefix/suffix, zero suppression, alternate units, angle formats, tolerances), arrowheads, colours, line weights,
//! hidden halves, text fill and the basic-dimension box.

use crate::jsmath::JsMath;
use cc_cad::entity::{Color, Entity, Geo, Hatch, Obj, Text};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::geom::*;
use crate::units::{format_length, lunit_code_of};

/// DIM_EXTRA (all optional; absent = these).
pub fn dim_extra() -> Obj {
    const EXTRA: &str = r#"{"blk1":"","blk2":"","tsz":0,"clrd":0,"clre":0,"lwd":-2,"lwe":-2,"sd1":false,"sd2":false,"se1":false,"se2":false,
        "txsty":"Standard","clrt":0,"tfill":0,"tfillclr":0,"just":0,"tih":false,"atfit":3,"tmove":0,"tix":false,"tofl":false,
        "rnd":0,"pre":"","suf":"","lfac":1,"dsep":".","aunit":0,"azin":0,
        "alt":false,"altu":2,"altd":2,"altf":25.4,"altrnd":0,"apre":"","asuf":"","altz":0,
        "tol":"none","tp":0,"tm":0,"tdec":2,"tfac":1,"tolj":1,"tzin":0}"#;
    serde_json::from_str::<Obj>(EXTRA).unwrap_or_default()
}
pub const TOL_MODES: [&str; 5] = ["none", "sym", "dev", "lim", "basic"];
pub const ARROWS: [&str; 12] =
    ["", "CLOSEDBLANK", "CLOSED", "OPEN", "OPEN30", "DOT", "DOTSMALL", "ARCHTICK", "OBLIQUE", "ORIGIN", "OPEN90", "NONE"];

const D2R: f64 = PI / 180.0;

/// A dimension style object with JS reads.
pub struct St(pub Obj);
impl St {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k)
    }
    /// `+st.k`
    pub fn n(&self, k: &str) -> f64 {
        js::to_number_opt(self.0.get(k))
    }
    /// `!!st.k`
    pub fn b(&self, k: &str) -> bool {
        js::truthy(self.0.get(k))
    }
    /// `st.k | 0`
    pub fn i(&self, k: &str) -> f64 {
        js::int32(self.n(k))
    }
    /// `st.k || d` as a number
    pub fn or(&self, k: &str, d: f64) -> f64 {
        if js::truthy(self.0.get(k)) { self.n(k) } else { d }
    }
    /// `st.k === n`
    pub fn is(&self, k: &str, n: f64) -> bool {
        matches!(self.0.get(k), Some(Value::Number(x)) if x.as_f64() == Some(n))
    }
    /// `st.k || ''` as text
    pub fn s(&self, k: &str) -> String {
        match self.0.get(k) {
            Some(v) if js::truthy(Some(v)) => js::to_js_string(v),
            _ => String::new(),
        }
    }
    pub fn str_is(&self, k: &str, s: &str) -> bool {
        self.0.get(k).and_then(Value::as_str) == Some(s)
    }
}

/// `{ ...DIM_EXTRA, ...DIM_DEFAULTS, ...(e.st || {}) }`
pub fn full(st: Option<&Obj>) -> St {
    let mut o = dim_extra();
    for (k, v) in cc_cad::doc::dim_defaults() {
        o.insert(k, v);
    }
    if let Some(s) = st {
        for (k, v) in s {
            o.insert(k.clone(), v.clone());
        }
    }
    St(o)
}

/// Zero handling for a decimal number written with `dec` places.
fn decimal(v: f64, dec: f64, lead: bool, trail: bool, sep: &str) -> String {
    let d = js::max(0.0, js::min(8.0, js::int32(js::round(dec))));
    let du = if d.is_finite() && d >= 0.0 { usize::try_from(format!("{d}").parse::<u64>().unwrap_or(0)).unwrap_or(0) } else { 0 };
    let mut s = js::to_fixed(if v.abs() < 0.5 * 10f64.jpow(-d) { 0.0 } else { v }, du);
    if trail && s.contains('.') {
        s = s.trim_end_matches('0').to_string();
        if s.ends_with('.') {
            s.pop();
        }
    }
    if lead {
        if let Some(r) = s.strip_prefix("-0.") {
            s = format!("-.{r}");
        } else if let Some(r) = s.strip_prefix("0.") {
            s = format!(".{r}");
        }
    }
    if sep == "." { s } else { s.replacen('.', sep, 1) }
}
/// Round to the nearest multiple of r (DIMRND); 0 = none.
fn round_to(v: f64, r: f64) -> f64 {
    if r > 0.0 { js::round(v / r) * r } else { v }
}

fn linear(v: f64, unit: Option<&Value>, dec: Option<&Value>, zin: f64, sep: &str) -> String {
    let u = match unit {
        Some(x) if js::truthy(Some(x)) => Some(x.clone()),
        _ => Some(Value::from(2)),
    };
    let code = lunit_code_of(u.as_ref());
    if code != 2 {
        return format_length(v, f64::from(code), dec);
    }
    let z = js::int32(zin);
    decimal(v, js::to_number_opt(dec), and(z, 4), and(z, 8), sep)
}
fn and(z: f64, bit: i64) -> bool {
    // z is an int32 value
    #[allow(clippy::cast_possible_truncation)]
    let zi = z as i64;
    zi & bit != 0
}

fn angle(rads: f64, aunit: f64, adec: f64, azin: f64, sep: &str) -> String {
    let z = js::int32(azin);
    let (lead, trail) = (and(z, 1), and(z, 2));
    let d = rads / D2R;
    let s = js::num_to_string;
    match js::int32(aunit) {
        1.0 => {
            let n = js::max(0.0, js::int32(adec));
            let sgn = if d < 0.0 { "-" } else { "" };
            let a = d.abs();
            if n == 0.0 {
                return format!("{sgn}{}°", s(js::round(a)));
            }
            let mut deg = a.floor();
            let mut min = (a - deg) * 60.0;
            if n <= 2.0 {
                min = js::round(min);
                if min == 60.0 {
                    deg += 1.0;
                    min = 0.0;
                }
                return format!("{sgn}{}°{}'", s(deg), s(min));
            }
            let mut m = min.floor();
            let mut sec = (min - m) * 60.0;
            let sd = js::max(0.0, n - 4.0);
            let sdu = usize::try_from(format!("{sd}").parse::<u64>().unwrap_or(0)).unwrap_or(0).min(100);
            sec = js::to_fixed(sec, sdu).parse::<f64>().unwrap_or(f64::NAN);
            if sec >= 60.0 {
                sec = 0.0;
                m += 1.0;
            }
            if m >= 60.0 {
                m = 0.0;
                deg += 1.0;
            }
            format!("{sgn}{}°{}'{}\"", s(deg), s(m), decimal(sec, sd, false, trail, sep))
        }
        2.0 => format!("{}g", decimal(d / 0.9, adec, lead, trail, sep)),
        3.0 => format!("{}r", decimal(rads, adec, lead, trail, sep)),
        _ => format!("{}°", decimal(d, adec, lead, trail, sep)),
    }
}

/// A tolerance: 'sym' with its text, 'dev' / 'lim' with upper and lower, or 'basic'.
#[derive(Clone, Debug, PartialEq)]
pub enum Tol {
    Sym(String),
    Dev(String, String),
    Lim(String, String),
    Basic,
}
impl Tol {
    pub fn to_value(&self) -> Value {
        match self {
            Tol::Sym(t) => serde_json::json!({ "kind": "sym", "text": t }),
            Tol::Dev(u, l) => serde_json::json!({ "kind": "dev", "upper": u, "lower": l }),
            Tol::Lim(u, l) => serde_json::json!({ "kind": "lim", "upper": u, "lower": l }),
            Tol::Basic => serde_json::json!({ "kind": "basic" }),
        }
    }
}

fn dim_parts_of(e: &Entity) -> (Option<&Obj>, &str) {
    match &e.geo {
        Geo::Dim(d) => (d.st.as_ref(), d.kind()),
        _ => (None, ""),
    }
}

/// The text a style writes for the measurement v (drawing units, or radians for angular): (main, tol).
pub fn dim_value(e: &Entity, v: f64) -> (String, Option<Tol>) {
    let (st, kind) = dim_parts_of(e);
    let st = full(st);
    let sep = if st.str_is("dsep", ",") { "," } else { "." };
    let mut main;
    if kind == "angular" {
        main = angle(v, st.n("aunit"), st.n("adec"), st.n("azin"), sep);
    } else {
        let lfac = st.get("lfac").and_then(Value::as_f64).filter(|x| x.is_finite() && *x != 0.0).map_or(1.0, f64::abs);
        let m = round_to(v * lfac, st.n("rnd"));
        main = linear(m, st.get("lunit"), st.get("dec"), st.n("zin"), sep);
        if st.b("alt") {
            let a = round_to(m * st.or("altf", 25.4), st.n("altrnd"));
            main = format!("{main} [{}{}{}]", st.s("apre"), linear(a, st.get("altu"), st.get("altd"), st.n("altz"), sep), st.s("asuf"));
        }
    }
    let sym = match kind {
        "radius" => "R",
        "diameter" => "⌀",
        _ => "",
    };
    let pre = st.s("pre");
    main = format!("{}{main}{}", if pre.is_empty() { sym.to_string() } else { pre }, st.s("suf"));
    let tfmt = |x: f64| {
        if kind == "angular" {
            angle(x * D2R, st.n("aunit"), st.n("tdec"), st.n("tzin"), sep)
        } else {
            linear(x, st.get("lunit"), st.get("tdec"), st.n("tzin"), sep)
        }
    };
    let tol = match st.get("tol").and_then(Value::as_str) {
        Some("sym") => Some(Tol::Sym(format!("±{}", tfmt(st.n("tp").abs())))),
        Some("dev") => {
            let tm = st.n("tm");
            Some(Tol::Dev(
                format!("+{}", tfmt(st.n("tp"))),
                format!(
                    "{}{}",
                    if tm > 0.0 {
                        "-"
                    } else if tm < 0.0 {
                        "+"
                    } else {
                        ""
                    },
                    tfmt(tm.abs())
                ),
            ))
        }
        Some("lim") => {
            let base = if kind == "angular" { v / D2R } else { round_to(v * st.or("lfac", 1.0), st.n("rnd")) };
            let t = Tol::Lim(tfmt(base + st.n("tp")), tfmt(base - st.n("tm")));
            main = String::new();
            Some(t)
        }
        Some("basic") => Some(Tol::Basic),
        _ => None,
    };
    (main, tol)
}

/// `over.replace(/<>/g, main)` with JS replacement patterns ($$, $&) honoured.
pub fn replace_value(over: &str, main: &str) -> String {
    let rep = main.replace("$$", "\u{0}").replace("$&", "<>").replace('\u{0}', "$");
    over.replace("<>", &rep)
}

/// One line: what the dimension says, tolerances written inline.
pub fn dim_style_text(e: &Entity, v: f64) -> String {
    let (main, tol) = dim_value(e, v);
    let over = match &e.geo {
        Geo::Dim(d) => d.text.clone().unwrap_or_default(),
        _ => String::new(),
    };
    let body = if over.is_empty() || over == "<>" { main } else { replace_value(&over, &main) };
    match tol {
        None => body,
        Some(Tol::Sym(t)) => format!("{body}{t}"),
        Some(Tol::Dev(u, l)) => format!("{body} {u}/{l}"),
        Some(Tol::Lim(u, l)) => format!("{u}/{l}"),
        Some(Tol::Basic) => format!("[{body}]"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Arrowheads

fn at(tip: P2, ang: f64, along: f64, across: f64) -> P2 {
    p(tip.x - ang.jcos() * along - ang.jsin() * across, tip.y - ang.jsin() * along + ang.jcos() * across)
}

/// Layer / colour / role fields an arrowhead's parts copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Base {
    pub layer: Option<String>,
    pub color: Option<Color>,
    pub role: Option<String>,
}
fn based(geo: Geo, base: &Base) -> Entity {
    let mut e = Entity::with(geo, base.layer.clone(), base.color.clone());
    if let Some(r) = &base.role {
        e.extra.insert("role".into(), Value::String(r.clone()));
    }
    e
}
fn seg(a: P2, b: P2, base: &Base) -> Entity {
    based(line_ent(a.x, a.y, b.x, b.y), base)
}

/// Parts for one arrowhead at `tip`, pointing along `ang`, `size` long.
pub fn arrow_parts(kind: &str, tip: P2, ang: f64, size: f64, base: &Base) -> Vec<Entity> {
    let s = size;
    let w = s / 6.0;
    let tri = |half: f64| [tip, at(tip, ang, s, half), at(tip, ang, s, -half)];
    match kind {
        "NONE" => Vec::new(),
        "CLOSEDBLANK" | "CLOSED" => {
            let [a, b, c] = tri(w);
            let mut out = vec![seg(a, b, base), seg(b, c, base), seg(c, a, base)];
            if kind == "CLOSED" {
                out.push(seg(tip, at(tip, ang, s, 0.0), base));
            }
            out
        }
        "OPEN" => {
            let [a, b, c] = tri(w);
            vec![seg(a, b, base), seg(a, c, base)]
        }
        "OPEN30" => {
            let h = (15.0 * D2R).jtan() * s;
            let [a, b, c] = tri(h);
            vec![seg(a, b, base), seg(a, c, base)]
        }
        "OPEN90" => {
            let [a, b, c] = tri(s);
            vec![seg(a, b, base), seg(a, c, base)]
        }
        "DOT" | "DOTSMALL" => {
            let r = if kind == "DOT" { s / 4.0 } else { s / 8.0 };
            let n = 16.0;
            let mut ring = Vec::new();
            for i in 0..=16 {
                let i = f64::from(i);
                ring.push(tip.x + r * (i * 2.0 * PI / n).jcos());
                ring.push(tip.y + r * (i * 2.0 * PI / n).jsin());
            }
            vec![based(Geo::Hatch(Hatch { loops: vec![ring], solid: Some(true) }), base)]
        }
        "ORIGIN" => vec![based(Geo::Circle(cc_cad::entity::Circle { cx: tip.x, cy: tip.y, r: s / 4.0 }), base)],
        "ARCHTICK" | "OBLIQUE" => {
            let a = ang + PI / 4.0;
            let h = s / 2.0;
            let mut e = seg(p(tip.x - a.jcos() * h, tip.y - a.jsin() * h), p(tip.x + a.jcos() * h, tip.y + a.jsin() * h), base);
            if kind == "ARCHTICK" {
                e.lw = Some(Some(0.5));
            }
            vec![e]
        }
        _ => {
            let [a, b, c] = tri(w);
            vec![based(Geo::Hatch(Hatch { loops: vec![vec![a.x, a.y, b.x, b.y, c.x, c.y, a.x, a.y]], solid: Some(true) }), base)]
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The parts

fn role(e: &Entity) -> Option<&str> {
    e.extra.get("role").and_then(Value::as_str)
}
/// col(c): c === undefined ? 0 : c
fn col(st: &St, k: &str) -> Color {
    match st.get(k) {
        None => Color::Num(0.0),
        Some(v) => Color::from_value(v).unwrap_or(Color::Null),
    }
}
/// lwOf(w): {} for -2 / undefined, else { lw: w === -1 ? null : w }
fn apply_lw(e: &mut Entity, st: &St, k: &str) {
    match st.get(k) {
        None => {}
        Some(Value::Number(n)) if n.as_f64() == Some(-2.0) => {}
        Some(Value::Number(n)) if n.as_f64() == Some(-1.0) => e.lw = Some(None),
        Some(Value::Number(n)) => e.lw = Some(n.as_f64()),
        Some(Value::Null) => e.lw = Some(None),
        Some(v) => {
            e.lw = None;
            e.extra.insert("lw".into(), v.clone());
        }
    }
}

/// The parts dims.js built, with this dimension's style applied (unmarked parts are left alone).
pub fn style_dim_parts(e: &Entity, parts: Vec<Entity>) -> Vec<Entity> {
    if !parts.iter().any(|q| js::truthy(q.extra.get("role"))) {
        return parts;
    }
    let (st0, _) = dim_parts_of(e);
    let st = full(st0);
    let s_scale = st.or("scale", 1.0);
    let mut out = Vec::new();
    let mut text: Option<Entity> = None;
    for q in parts {
        match role(&q) {
            Some(r @ ("ext1" | "ext2")) => {
                if (r == "ext1" && st.b("se1")) || (r == "ext2" && st.b("se2")) {
                    continue;
                }
                let mut c = q.clone();
                c.color = Some(col(&st, "clre"));
                apply_lw(&mut c, &st, "lwe");
                out.push(c);
            }
            Some(r @ ("dim1" | "dim2" | "dim")) => {
                if (r == "dim1" && st.b("sd1")) || (r == "dim2" && st.b("sd2")) {
                    continue;
                }
                let mut c = q.clone();
                c.color = Some(col(&st, "clrd"));
                apply_lw(&mut c, &st, "lwd");
                out.push(c);
            }
            Some(r @ ("arrow1" | "arrow2")) => {
                if (r == "arrow1" && st.b("sd1")) || (r == "arrow2" && st.b("sd2")) {
                    continue;
                }
                let kind = if st.n("tsz") > 0.0 { "ARCHTICK".to_string() } else { st.s(if r == "arrow1" { "blk1" } else { "blk2" }) };
                let size = if st.n("tsz") > 0.0 { st.n("tsz") * s_scale * 2.0 } else { st.n("asz") * s_scale };
                let tip = q.extra.get("tip").filter(|t| js::truthy(Some(t)));
                let ang = js::finite(q.extra.get("ang"));
                match (kind.is_empty(), tip, ang) {
                    (false, Some(tip), Some(ang)) => {
                        let tp = p(js::to_number_opt(tip.get("x")), js::to_number_opt(tip.get("y")));
                        let base = Base {
                            layer: Some(q.layer.clone().unwrap_or_else(|| "0".into())),
                            color: Some(col(&st, "clrd")),
                            role: Some(r.to_string()),
                        };
                        out.extend(arrow_parts(&kind, tp, ang, size, &base));
                    }
                    _ => {
                        let mut c = q.clone();
                        c.color = Some(col(&st, "clrd"));
                        out.push(c);
                    }
                }
            }
            Some("text") => text = Some(q),
            _ => out.push(q),
        }
    }
    if let Some(t) = text {
        out.extend(style_text(e, &st, &t));
    }
    out
}

fn text_of(e: &Entity) -> Option<&Text> {
    if let Geo::Text(t) = &e.geo { Some(t) } else { None }
}
fn corners_of(e: &Entity) -> [P2; 4] {
    text_of(e).map_or([p(f64::NAN, f64::NAN); 4], text_corners)
}

/// The text part: rotation override, colour, tolerance stack, basic box, background fill.
fn style_text(e: &Entity, st: &St, pt: &Entity) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    let mut tx = pt.clone();
    tx.color = Some(match st.get("clrt") {
        None => Color::Num(0.0),
        Some(v) => Color::from_value(v).unwrap_or(Color::Null),
    });
    let trot = e.num("trot").filter(|x| x.is_finite() && *x != 0.0);
    if let Geo::Text(t) = &mut tx.geo {
        if let Some(r) = trot {
            t.rot = Some(r);
        } else if st.b("tih") {
            t.rot = Some(0.0);
        }
    }
    let set_s = |x: &mut Entity, s: String| {
        if let Geo::Text(t) = &mut x.geo {
            t.s = s;
        }
    };
    // parts = [tx, ...stacked], or only the stack (limits with no text)
    let mut keep_tx = true;
    let mut stacked: Vec<Entity> = Vec::new();
    if let Some(v) = js::finite(pt.extra.get("measure")) {
        let (main, tol) = dim_value(e, v);
        let over = match &e.geo {
            Geo::Dim(d) => d.text.clone().unwrap_or_default(),
            _ => String::new(),
        };
        let body = if over.is_empty() || over == "<>" { main.clone() } else { replace_value(&over, &main) };
        match &tol {
            Some(Tol::Sym(t)) => set_s(&mut tx, format!("{body}{t}")),
            Some(Tol::Dev(upper, lower) | Tol::Lim(upper, lower)) => {
                let is_lim = matches!(tol, Some(Tol::Lim(..)));
                let (th, r) = text_of(&tx).map_or((f64::NAN, 0.0), |t| (t.h, js::or(t.rot, 0.0)));
                let h2 = th * if st.n("tfac") > 0.0 { st.n("tfac") } else { 1.0 };
                let gap = th * 0.25;
                let (c, s) = (r.jcos(), r.jsin());
                let stack = |tx: &Entity, o: P2, ha: &str, shift: f64| -> Vec<Entity> {
                    let mk = |s_: &str, va: &str, x: f64, y: f64| {
                        let mut q = tx.clone();
                        if let Geo::Text(t) = &mut q.geo {
                            t.s = s_.to_string();
                            t.h = h2;
                            t.ha = Some(ha.to_string());
                            t.va = Some(va.to_string());
                            t.x = x;
                            t.y = y;
                        }
                        q
                    };
                    vec![
                        mk(upper, "bottom", o.x - s * (gap / 2.0 + shift), o.y + c * (gap / 2.0 + shift)),
                        mk(lower, "top", o.x + s * (gap / 2.0 - shift), o.y - c * (gap / 2.0 - shift)),
                    ]
                };
                if is_lim && body.is_empty() {
                    let mut xx = tx.clone();
                    set_s(&mut xx, "X".into());
                    let k = corners_of(&xx);
                    let mid = p((k[0].x + k[1].x + k[2].x + k[3].x) / 4.0, (k[0].y + k[1].y + k[2].y + k[3].y) / 4.0);
                    keep_tx = false;
                    stacked = stack(&tx, mid, "center", 0.0);
                } else {
                    set_s(&mut tx, body.clone());
                    let k = corners_of(&tx);
                    let right = p((k[1].x + k[2].x) / 2.0 + c * gap, (k[1].y + k[2].y) / 2.0 + s * gap);
                    let half = (k[2].x - k[1].x) * -s + (k[2].y - k[1].y) * c;
                    let tolj = st.i("tolj");
                    let shift = if tolj == 0.0 {
                        h2 + gap / 2.0 - half / 2.0
                    } else if tolj == 2.0 {
                        -(h2 + gap / 2.0 - half / 2.0)
                    } else {
                        0.0
                    };
                    stacked = stack(&tx, right, "left", shift);
                }
            }
            _ => set_s(&mut tx, body.clone()),
        }
        if matches!(tol, Some(Tol::Basic)) {
            let k = corners_of(&tx);
            let g = st.or("gap", 0.625).abs() * st.or("scale", 1.0);
            out.extend(boxed(&k, g, &tx));
        }
    }
    let mut parts: Vec<Entity> = if keep_tx { vec![tx] } else { Vec::new() };
    parts.extend(stacked);
    let fill = st.i("tfill");
    if fill != 0.0
        && let Some(first) = parts.first()
    {
        let k = corners_of(first);
        let g = st.or("gap", 0.625) * st.or("scale", 1.0);
        let r = grow(&k, g);
        let color = if fill == 2.0 {
            match st.get("tfillclr") {
                Some(v) if js::truthy(Some(v)) => Color::from_value(v).unwrap_or(Color::Null),
                _ => Color::Num(0.0),
            }
        } else {
            Color::Str("bg".into())
        };
        let mut h = Entity::with(
            Geo::Hatch(Hatch {
                loops: vec![vec![r[0].x, r[0].y, r[1].x, r[1].y, r[2].x, r[2].y, r[3].x, r[3].y, r[0].x, r[0].y]],
                solid: Some(true),
            }),
            Some(pt.layer.clone().unwrap_or_else(|| "0".into())),
            Some(color),
        );
        h.extra.insert("wipe".into(), true.into());
        h.extra.insert("role".into(), "fill".into());
        out.insert(0, h);
    }
    out.extend(parts);
    out
}

fn grow(k: &[P2; 4], g: f64) -> [P2; 4] {
    let (ux, uy) = (k[1].x - k[0].x, k[1].y - k[0].y);
    let l = js::or(Some(js::hypot(ux, uy)), 1.0);
    let (ax, ay) = (ux / l * g, uy / l * g);
    let (vx, vy) = (k[3].x - k[0].x, k[3].y - k[0].y);
    let m = js::or(Some(js::hypot(vx, vy)), 1.0);
    let (bx, by) = (vx / m * g, vy / m * g);
    [
        p(k[0].x - ax - bx, k[0].y - ay - by),
        p(k[1].x + ax - bx, k[1].y + ay - by),
        p(k[2].x + ax + bx, k[2].y + ay + by),
        p(k[3].x - ax + bx, k[3].y - ay + by),
    ]
}
fn boxed(k: &[P2; 4], g: f64, tx: &Entity) -> Vec<Entity> {
    let r = grow(k, g);
    let base = Base { layer: Some(tx.layer.clone().unwrap_or_else(|| "0".into())), color: tx.color.clone(), role: Some("box".into()) };
    vec![seg(r[0], r[1], &base), seg(r[1], r[2], &base), seg(r[2], r[3], &base), seg(r[3], r[0], &base)]
}
