//! gdt.js — GD&T and surface-finish annotations (ASME Y14.5-2018, ISO 1302): the characteristics table, datum letters,
//! `newAnnotation`, `validate`, the untrusted reader `readNotes`, `describe`, and the symbol geometry
//! (`annotationGeometry`: our own strokes and fills; texts carry only letters and numbers).

use crate::font::char_advance;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use serde_json::Value;
use std::collections::HashSet;

pub const H: f64 = 3.5;
pub const FRAME: f64 = 0.35;
pub const LEADER: f64 = 0.25;
pub const ARROW_L: f64 = 3.0;
pub const ARROW_W: f64 = 0.9;
pub const PAD: f64 = 1.2;

/// One characteristic: label key, datums rule, whether ⌀ and material modifiers are allowed.
#[derive(Clone, Copy, Debug)]
pub struct Char {
    pub key: &'static str,
    pub label_key: &'static str,
    pub group: &'static str,
    pub datums: &'static str,
    pub dia: bool,
    pub md: bool,
}
pub const CHARACTERISTICS: [Char; 14] = [
    Char { key: "straightness", label_key: "model.gdt.char.straightness", group: "form", datums: "none", dia: true, md: true },
    Char { key: "flatness", label_key: "model.gdt.char.flatness", group: "form", datums: "none", dia: false, md: false },
    Char { key: "circularity", label_key: "model.gdt.char.circularity", group: "form", datums: "none", dia: false, md: false },
    Char { key: "cylindricity", label_key: "model.gdt.char.cylindricity", group: "form", datums: "none", dia: false, md: false },
    Char { key: "profileLine", label_key: "model.gdt.char.profile_line", group: "profile", datums: "optional", dia: false, md: false },
    Char {
        key: "profileSurface",
        label_key: "model.gdt.char.profile_surface",
        group: "profile",
        datums: "optional",
        dia: false,
        md: false,
    },
    Char {
        key: "perpendicularity",
        label_key: "model.gdt.char.perpendicularity",
        group: "orientation",
        datums: "required",
        dia: true,
        md: true,
    },
    Char { key: "angularity", label_key: "model.gdt.char.angularity", group: "orientation", datums: "required", dia: true, md: true },
    Char { key: "parallelism", label_key: "model.gdt.char.parallelism", group: "orientation", datums: "required", dia: true, md: true },
    Char { key: "position", label_key: "model.gdt.char.position", group: "location", datums: "required", dia: true, md: true },
    Char { key: "concentricity", label_key: "model.gdt.char.concentricity", group: "location", datums: "required", dia: true, md: false },
    Char { key: "symmetry", label_key: "model.gdt.char.symmetry", group: "location", datums: "required", dia: false, md: false },
    Char { key: "circularRunout", label_key: "model.gdt.char.circular_runout", group: "runout", datums: "required", dia: false, md: false },
    Char { key: "totalRunout", label_key: "model.gdt.char.total_runout", group: "runout", datums: "required", dia: false, md: false },
];
pub const DATUM_LETTERS: &str = "ABCDEFGHJKLMNPRSTUVWXYZ";
pub const VIEW_NAMES: [&str; 6] = ["front", "top", "right", "iso", "section", "detail"];

/// `CHARACTERISTICS[key]` (an `Object.prototype` name is truthy with no fields: `Err(())`).
pub fn char_of(key: &str) -> Option<Result<&'static Char, ()>> {
    if let Some(c) = CHARACTERISTICS.iter().find(|c| c.key == key) {
        return Some(Ok(c));
    }
    if crate::sketch::proto_key(key) { Some(Err(())) } else { None }
}
fn label(c: &Char) -> String {
    t(c.label_key, &[])
}
fn datum_letter(s: Option<&Value>) -> bool {
    s.and_then(Value::as_str).is_some_and(|l| l.chars().count() == 1 && DATUM_LETTERS.contains(l))
}
/// `PROCESSES[p]` (truthy).
fn process_ok(p: Option<&Value>) -> bool {
    p.and_then(Value::as_str).is_some_and(|s| matches!(s, "any" | "removal" | "noRemoval") || crate::sketch::proto_key(s))
}
fn process_label(p: &str) -> String {
    match p {
        "any" => t("model.gdt.process.any", &[]),
        "removal" => t("model.gdt.process.removal", &[]),
        "noRemoval" => t("model.gdt.process.no_removal", &[]),
        k if crate::sketch::proto_key(k) => crate::sketch::proto_text(k),
        _ => "undefined".into(),
    }
}
fn mod_ok(m: Option<&Value>) -> bool {
    m.and_then(Value::as_str).is_some_and(|s| matches!(s, "" | "M" | "L"))
}

/// `newAnnotation(kind, props)`.
pub fn new_annotation(kind: &str, props: &Value) -> Result<Value, String> {
    let pick = |k: &str, d: Value| props.get(k).filter(|v| jv::truthy(Some(v))).cloned().unwrap_or(d);
    let pair = |k: &str, d: [f64; 2]| -> Value {
        match props.get(k).filter(|v| jv::truthy(Some(v))) {
            Some(v) => Value::Array(jv::arr(Some(v)).to_vec()),
            None => jv::nums(&d),
        }
    };
    let mut base = jv::obj(vec![
        ("id", pick("id", "g1".into())),
        ("kind", kind.into()),
        ("view", pick("view", "front".into())),
        ("at", pair("at", [0.0, 0.0])),
        ("off", pair("off", [12.0, 10.0])),
    ]);
    let nn = |k: &str, d: Value| props.get(k).filter(|v| !v.is_null()).cloned().unwrap_or(d);
    match kind {
        "datum" => {
            jv::set(&mut base, "letter", pick("letter", "A".into()));
            Ok(base)
        }
        "fcf" => {
            let ch = pick("char", "position".into());
            let chs = jv::as_str(Some(&ch)).unwrap_or("").to_string();
            jv::set(&mut base, "char", ch);
            jv::set(&mut base, "dia", nn("dia", Value::Bool(chs == "position")));
            jv::set(&mut base, "tol", nn("tol", jv::num(0.1)));
            jv::set(&mut base, "mod", pick("mod", "".into()));
            let datums = match props.get("datums").filter(|v| jv::truthy(Some(v))) {
                Some(d) => Value::Array(
                    jv::arr(Some(d))
                        .iter()
                        .map(|x| {
                            jv::obj(vec![
                                ("letter", x.get("letter").cloned().unwrap_or(Value::Null)),
                                ("mod", x.get("mod").filter(|v| jv::truthy(Some(v))).cloned().unwrap_or_else(|| "".into())),
                            ])
                        })
                        .collect(),
                ),
                None => match char_of(&chs) {
                    Some(Ok(c)) if c.datums == "required" => Value::Array(vec![jv::obj(vec![("letter", "A".into()), ("mod", "".into())])]),
                    _ => Value::Array(Vec::new()),
                },
            };
            jv::set(&mut base, "datums", datums);
            Ok(base)
        }
        "finish" => {
            jv::set(&mut base, "ra", nn("ra", jv::num(1.6)));
            jv::set(&mut base, "process", pick("process", "removal".into()));
            jv::set(&mut base, "note", pick("note", "".into()));
            Ok(base)
        }
        _ => Err(t("model.gdt.unknown_kind", &[])),
    }
}

fn fin(v: Option<&Value>) -> Option<f64> {
    jv::finite(v)
}

/// `validate(a)`: None when right, else the reason.
pub fn validate(a: Option<&Value>) -> Option<String> {
    let Some(a) = a.filter(|a| jv::truthy(Some(a)) && jv::is_obj_like(Some(a))) else { return Some(t("model.gdt.missing", &[])) };
    if !a.get("view").and_then(Value::as_str).is_some_and(|v| VIEW_NAMES.contains(&v)) {
        return Some(t("model.gdt.unknown_view", &[]));
    }
    let kind = jv::s(a, "kind");
    if kind == "datum" && a.get("kind").is_some_and(Value::is_string) {
        return if datum_letter(a.get("letter")) { None } else { Some(t("model.gdt.datum_letter", &[])) };
    }
    if kind == "finish" {
        let ra = fin(a.get("ra"));
        if ra.is_none_or(|r| r <= 0.0 || r > 1000.0) {
            return Some(t("model.gdt.ra_range", &[]));
        }
        if !process_ok(a.get("process")) {
            return Some(t("model.gdt.process_unknown", &[]));
        }
        match a.get("note") {
            Some(Value::String(s)) if js::len16(s) <= 40 => return None,
            _ => return Some(t("model.gdt.note_long", &[])),
        }
    }
    if kind != "fcf" {
        return Some(t("model.gdt.unknown_kind", &[]));
    }
    let Some(c) = a.get("char").and_then(Value::as_str).and_then(char_of) else { return Some(t("model.gdt.unknown_char", &[])) };
    if fin(a.get("tol")).is_none_or(|v| v <= 0.0 || v > 1000.0) {
        return Some(t("model.gdt.tol_range", &[]));
    }
    let Ok(c) = c else {
        // an `Object.prototype` name: its fields are undefined (an undefined fill stays "{label}")
        if jv::truthy(a.get("dia")) {
            return Some(t("model.gdt.no_dia", &[]));
        }
        if !mod_ok(a.get("mod")) {
            return Some(t("model.gdt.mod_values", &[]));
        }
        if jv::truthy(a.get("mod")) {
            return Some(t("model.gdt.no_mod", &[]));
        }
        return validate_datums(a, None);
    };
    if jv::truthy(a.get("dia")) && !c.dia {
        return Some(t("model.gdt.no_dia", &[("label", F::S(label(c)))]));
    }
    if !mod_ok(a.get("mod")) {
        return Some(t("model.gdt.mod_values", &[]));
    }
    if jv::truthy(a.get("mod")) && !c.md {
        return Some(t("model.gdt.no_mod", &[("label", F::S(label(c)))]));
    }
    validate_datums(a, Some(c))
}

fn validate_datums(a: &Value, c: Option<&Char>) -> Option<String> {
    let Some(ds) = jv::as_arr(a.get("datums")).filter(|d| d.len() <= 3) else { return Some(t("model.gdt.max_datums", &[])) };
    let fills: Vec<(&str, F)> = c.map_or_else(Vec::new, |c| vec![("label", F::S(label(c)))]);
    let rule = c.map_or("", |c| c.datums);
    if rule == "none" && !ds.is_empty() {
        return Some(t("model.gdt.form_no_datums", &fills));
    }
    if rule == "required" && ds.is_empty() {
        return Some(t("model.gdt.needs_datum", &fills));
    }
    let mut seen = HashSet::new();
    for d in ds {
        if !jv::truthy(Some(d)) || !datum_letter(d.get("letter")) {
            return Some(t("model.gdt.datum_ref_letter", &[]));
        }
        let l = jv::s(d, "letter").to_string();
        if !seen.insert(l.clone()) {
            return Some(t("model.gdt.datum_twice", &[("letter", F::S(l))]));
        }
        if !mod_ok(d.get("mod")) {
            return Some(t("model.gdt.datum_mod_values", &[]));
        }
        if jv::truthy(d.get("mod")) && !c.is_some_and(|c| c.md) {
            return Some(t("model.gdt.no_datum_mod", &fills));
        }
    }
    None
}

/// `clean(s, max)`: controls (C0 and C1) out, UTF-16 slice.
fn clean(v: Option<&Value>, max: usize) -> String {
    js::slice16(&js::strip_ctrl_c1(&js::str_of(v)), max)
}

/// `readNotes(raw)`: untrusted notes → clean copies, or the message.
pub fn read_notes(raw: Option<&Value>) -> Result<Vec<Value>, String> {
    let Some(raw) = raw.filter(|r| !r.is_null()) else { return Ok(Vec::new()) };
    let Some(list) = raw.as_array() else { return Err(t("model.gdt.notes_not_list", &[])) };
    if list.len() > 500 {
        return Err(t("model.gdt.notes_too_many", &[]));
    }
    let mut ids: HashSet<String> = HashSet::new();
    let pair = |v: Option<&Value>, lim: f64| {
        jv::as_arr(v).is_some_and(|a| a.len() == 2 && a.iter().all(|x| fin(Some(x)).is_some_and(|x| x.abs() <= lim)))
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, n) in list.iter().enumerate() {
        let nf = F::from(i.saturating_add(1));
        let bad = |k: &str| t(k, &[("n", nf.clone())]);
        if !jv::truthy(Some(n)) || !jv::is_obj_like(Some(n)) {
            return Err(bad("model.gdt.note.not_note"));
        }
        let id = n.get("id").and_then(Value::as_str);
        let id_ok = id.is_some_and(|s| {
            let b = s.as_bytes();
            b.len() >= 2 && b.len() <= 10 && b.first() == Some(&b'g') && b.iter().skip(1).all(u8::is_ascii_digit) && !ids.contains(s)
        });
        if !id_ok {
            return Err(bad("model.gdt.note.bad_id"));
        }
        let id = id.unwrap_or("").to_string();
        ids.insert(id.clone());
        let kind = n.get("kind").and_then(Value::as_str).filter(|k| matches!(*k, "datum" | "fcf" | "finish"));
        let Some(kind) = kind else { return Err(bad("model.gdt.note.unknown_kind")) };
        if !n.get("view").and_then(Value::as_str).is_some_and(|v| VIEW_NAMES.contains(&v)) {
            return Err(bad("model.gdt.note.unknown_view"));
        }
        if !pair(n.get("at"), 1e6) {
            return Err(bad("model.gdt.note.bad_at"));
        }
        if !pair(n.get("off"), 2000.0) {
            return Err(bad("model.gdt.note.bad_off"));
        }
        let mut a = jv::obj(vec![
            ("id", id.into()),
            ("kind", kind.into()),
            ("view", n.get("view").cloned().unwrap_or(Value::Null)),
            ("at", n.get("at").cloned().unwrap_or(Value::Null)),
            ("off", n.get("off").cloned().unwrap_or(Value::Null)),
        ]);
        match kind {
            "datum" => jv::set(&mut a, "letter", clean(n.get("letter"), 2).into()),
            "finish" => {
                jv::set(&mut a, "ra", n.get("ra").cloned().unwrap_or(Value::Null));
                jv::set(&mut a, "process", n.get("process").cloned().unwrap_or(Value::Null));
                jv::set(&mut a, "note", clean(n.get("note"), 40).into());
                if n.get("ra").is_none() {
                    jv::del(&mut a, "ra");
                }
                if n.get("process").is_none() {
                    jv::del(&mut a, "process");
                }
            }
            _ => {
                let Some(ds) = jv::as_arr(n.get("datums")).filter(|d| d.len() <= 3) else { return Err(bad("model.gdt.note.bad_datums")) };
                jv::set(&mut a, "char", n.get("char").cloned().unwrap_or(Value::Null));
                if n.get("char").is_none() {
                    jv::del(&mut a, "char");
                }
                jv::set(&mut a, "dia", jv::is_true(n.get("dia")).into());
                jv::set(&mut a, "tol", n.get("tol").cloned().unwrap_or(Value::Null));
                if n.get("tol").is_none() {
                    jv::del(&mut a, "tol");
                }
                jv::set(&mut a, "mod", n.get("mod").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| "".into()));
                let dl: Vec<Value> = ds
                    .iter()
                    .map(|d| {
                        let letter = if jv::truthy(Some(d)) { clean(d.get("letter"), 2) } else { clean(Some(d), 2) };
                        let md = if jv::truthy(Some(d)) {
                            d.get("mod").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| "".into())
                        } else {
                            d.clone()
                        };
                        jv::obj(vec![("letter", letter.into()), ("mod", if md.is_null() { "".into() } else { md })])
                    })
                    .collect();
                jv::set(&mut a, "datums", Value::Array(dl));
            }
        }
        if let Some(why) = validate(Some(&a)) {
            let w = js::drop_final_dot(&why);
            let mut cs = w.chars();
            let w = match cs.next() {
                Some(c0) if c0 != '\n' && c0 != '\r' => {
                    let mut s: String = c0.to_lowercase().collect();
                    s.push_str(cs.as_str());
                    s
                }
                _ => w,
            };
            return Err(t("model.gdt.note.invalid", &[("n", nf), ("why", F::S(w))]));
        }
        out.push(a);
    }
    Ok(out)
}

fn num4(v: f64) -> String {
    js::fmt_fixed(v, 4)
}

/// `describe(a)`.
pub fn describe(a: &Value) -> String {
    let kind = jv::s(a, "kind");
    if kind == "datum" {
        return t("model.gdt.describe.datum", &[("letter", F::S(js::str_of(a.get("letter"))))]);
    }
    if kind == "finish" {
        let ra = num4(jv::n(a, "ra"));
        let pr = process_label(jv::s(a, "process"));
        return if jv::truthy(a.get("note")) {
            t("model.gdt.describe.finish_note", &[("ra", F::S(ra)), ("process", F::S(pr)), ("note", F::S(js::str_of(a.get("note"))))])
        } else {
            t("model.gdt.describe.finish", &[("ra", F::S(ra)), ("process", F::S(pr))])
        };
    }
    let lab = match char_of(jv::s(a, "char")) {
        Some(Ok(c)) => label(c),
        _ => String::new(),
    };
    let lab = match lab.rfind(" (") {
        Some(i) if lab.ends_with(')') => lab.get(..i).unwrap_or("").to_string(),
        _ => lab,
    };
    let m = |x: &str| if x.is_empty() { String::new() } else { format!(" ({x})") };
    let tol = num4(jv::n(a, "tol"));
    let first = if jv::truthy(a.get("dia")) {
        t("model.gdt.describe.tol_dia", &[("label", F::S(lab.clone())), ("tol", F::S(tol.clone()))])
    } else {
        t("model.gdt.describe.tol", &[("label", F::S(lab)), ("tol", F::S(tol))])
    } + &m(&js::str_of(a.get("mod")));
    let mut parts = vec![first];
    for d in jv::arr(a.get("datums")) {
        parts.push(format!("{}{}", js::str_of(d.get("letter")), m(&js::str_of(d.get("mod")))));
    }
    parts.join(" | ")
}

// ---------------------------------------------------------------- geometry

pub type Seg = [f64; 4];

/// `circlePts(c, r, n, a0, sweep)`.
pub fn circle_pts(c: [f64; 2], r: f64, n: usize, a0: f64, sweep: f64) -> Vec<[f64; 2]> {
    let mut pts = Vec::with_capacity(n.saturating_add(1));
    for i in 0..=n {
        let tt = a0 + sweep * js::f(i) / js::f(n);
        pts.push([c[0] + r * js::cos(tt), c[1] + r * js::sin(tt)]);
    }
    pts
}
fn poly(pts: &[[f64; 2]], segs: &mut Vec<Seg>) {
    for w in pts.windows(2) {
        if let [a, b] = w {
            segs.push([a[0], a[1], b[0], b[1]]);
        }
    }
}
/// `textW(s, size)`.
pub fn text_w(s: &str, size: f64) -> f64 {
    let mut w = 0.0;
    for ch in s.chars() {
        w += char_advance(ch);
    }
    w / 1000.0 * size
}

/// A filled polygon.
pub type Fill = Vec<[f64; 2]>;
/// A text item: x, y, size, text, anchor.
#[derive(Clone, Debug)]
pub struct Text {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub text: String,
    pub anchor: &'static str,
}
impl Text {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("x", jv::num(self.x)),
            ("y", jv::num(self.y)),
            ("size", jv::num(self.size)),
            ("text", self.text.as_str().into()),
            ("anchor", self.anchor.into()),
        ])
    }
}

fn arrow_head(fills: &mut Vec<Fill>, tip: [f64; 2], from: [f64; 2]) {
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let l = js::hypot2(dx, dy);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    let (ux, uy) = (dx / l, dy / l);
    let (al, w) = (ARROW_L, ARROW_W);
    fills.push(vec![
        [tip[0], tip[1]],
        [tip[0] - ux * al - uy * w, tip[1] - uy * al + ux * w],
        [tip[0] - ux * al + uy * w, tip[1] - uy * al - ux * w],
    ]);
}

fn char_symbol(key: &str, c: [f64; 2], segs: &mut Vec<Seg>, fills: &mut Vec<Fill>) -> Result<(), String> {
    let r = 0.75 * H;
    let [cx, cy] = c;
    let pi = std::f64::consts::PI;
    let l = |segs: &mut Vec<Seg>, x1: f64, y1: f64, x2: f64, y2: f64| segs.push([cx + x1 * r, cy + y1 * r, cx + x2 * r, cy + y2 * r]);
    let cc = |segs: &mut Vec<Seg>, x: f64, y: f64, rr: f64, a0: f64, sw: f64, n: usize| {
        poly(&circle_pts([cx + x * r, cy + y * r], rr * r, n, a0, sw), segs)
    };
    let s60 = js::sin(pi / 3.0);
    let c60 = js::cos(pi / 3.0);
    let full = 2.0 * pi;
    match key {
        "straightness" => l(segs, -1.0, 0.0, 1.0, 0.0),
        "flatness" => {
            l(segs, -1.0, -0.4, 0.55, -0.4);
            l(segs, 0.55, -0.4, 1.0, 0.4);
            l(segs, 1.0, 0.4, -0.55, 0.4);
            l(segs, -0.55, 0.4, -1.0, -0.4);
        }
        "circularity" => cc(segs, 0.0, 0.0, 0.85, 0.0, full, 32),
        "cylindricity" => {
            cc(segs, 0.0, 0.0, 0.55, 0.0, full, 32);
            let (nx, ny) = (-s60, c60);
            for s in [1.0, -1.0] {
                l(segs, nx * 0.55 * s - c60 * 1.1, ny * 0.55 * s - s60 * 1.1, nx * 0.55 * s + c60 * 1.1, ny * 0.55 * s + s60 * 1.1);
            }
        }
        "profileLine" => cc(segs, 0.0, -0.45, 0.95, 0.0, pi, 24),
        "profileSurface" => {
            cc(segs, 0.0, -0.45, 0.95, 0.0, pi, 24);
            l(segs, -0.95, -0.45, 0.95, -0.45);
        }
        "perpendicularity" => {
            l(segs, -0.9, -0.85, 0.9, -0.85);
            l(segs, 0.0, -0.85, 0.0, 0.95);
        }
        "angularity" => {
            let (len, a) = (2.1, pi / 6.0);
            l(segs, -0.95, -0.75, 0.95, -0.75);
            l(segs, -0.95, -0.75, -0.95 + len * js::cos(a), -0.75 + len * js::sin(a));
        }
        "parallelism" => {
            for dx in [-0.35, 0.35] {
                l(segs, dx - c60 * 1.05, -s60 * 1.05, dx + c60 * 1.05, s60 * 1.05);
            }
        }
        "position" => {
            cc(segs, 0.0, 0.0, 0.6, 0.0, full, 32);
            l(segs, -1.0, 0.0, 1.0, 0.0);
            l(segs, 0.0, -1.0, 0.0, 1.0);
        }
        "concentricity" => {
            cc(segs, 0.0, 0.0, 0.4, 0.0, full, 32);
            cc(segs, 0.0, 0.0, 0.9, 0.0, full, 32);
        }
        "symmetry" => {
            l(segs, -0.6, 0.5, 0.6, 0.5);
            l(segs, -1.0, 0.0, 1.0, 0.0);
            l(segs, -0.6, -0.5, 0.6, -0.5);
        }
        "circularRunout" | "totalRunout" => {
            let xs: &[f64] = if key == "totalRunout" { &[-0.75, 0.15] } else { &[-0.4] };
            for &x0 in xs {
                let a = [cx + x0 * r, cy - 0.9 * r];
                let b = [cx + (x0 + 1.1) * r, cy + 0.6 * r];
                segs.push([a[0], a[1], b[0], b[1]]);
                let (ux, uy) = (b[0] - a[0], b[1] - a[1]);
                let ll = js::hypot2(ux, uy);
                let tip = [b[0] + ux / ll * 0.45 * r, b[1] + uy / ll * 0.45 * r];
                let (k, w) = (0.5 * r, 0.22 * r);
                fills.push(vec![
                    tip,
                    [tip[0] - ux / ll * k - uy / ll * w, tip[1] - uy / ll * k + ux / ll * w],
                    [tip[0] - ux / ll * k + uy / ll * w, tip[1] - uy / ll * k - ux / ll * w],
                ]);
            }
            if key == "totalRunout" {
                l(segs, -0.75, -0.9, 0.15, -0.9);
            }
        }
        _ => return Err(format!("Unknown characteristic {key}")),
    }
    Ok(())
}

fn dia_symbol(c: [f64; 2], segs: &mut Vec<Seg>) {
    let r = 0.36 * H;
    poly(&circle_pts(c, r, 32, 0.0, 2.0 * std::f64::consts::PI), segs);
    let l = r * 1.35;
    let a = std::f64::consts::PI / 3.0;
    segs.push([c[0] - l * js::cos(a), c[1] - l * js::sin(a), c[0] + l * js::cos(a), c[1] + l * js::sin(a)]);
}
const DIA_W: f64 = 0.9 * H;

fn mod_symbol(letter: &str, c: [f64; 2], segs: &mut Vec<Seg>) {
    let r = 0.48 * H;
    poly(&circle_pts(c, r, 32, 0.0, 2.0 * std::f64::consts::PI), segs);
    let u = r * 0.5;
    let [x, y] = c;
    if letter == "M" {
        poly(&[[x - u, y - u], [x - u, y + u], [x, y - 0.1 * u], [x + u, y + u], [x + u, y - u]], segs);
    } else {
        poly(&[[x - 0.6 * u, y + u], [x - 0.6 * u, y - u], [x + 0.75 * u, y - u]], segs);
    }
}
const MOD_W: f64 = 1.05 * H;

fn box_of(strokes: &[(f64, Vec<Seg>)], fills: &[Fill], texts: &[Text]) -> [f64; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |x: f64, y: f64| {
        x0 = js::min(x0, x);
        x1 = js::max(x1, x);
        y0 = js::min(y0, y);
        y1 = js::max(y1, y);
    };
    for s in strokes {
        for g in &s.1 {
            add(g[0], g[1]);
            add(g[2], g[3]);
        }
    }
    for f in fills {
        for p in f {
            add(p[0], p[1]);
        }
    }
    for tx in texts {
        let w = text_w(&tx.text, tx.size);
        let dx = if tx.anchor == "middle" {
            -w / 2.0
        } else if tx.anchor == "end" {
            -w
        } else {
            0.0
        };
        add(tx.x + dx, tx.y - 0.25 * tx.size);
        add(tx.x + dx + w, tx.y + 0.75 * tx.size);
    }
    [x0, y0, x1, y1]
}
fn nearest_on_box(b: [f64; 4], p: [f64; 2]) -> [f64; 2] {
    [js::min(b[2], js::max(b[0], p[0])), js::min(b[3], js::max(b[1], p[1]))]
}
fn inside(b: [f64; 4], p: [f64; 2]) -> bool {
    p[0] > b[0] && p[0] < b[2] && p[1] > b[1] && p[1] < b[3]
}

/// `annotationGeometry(a, anchor)`'s result.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// (width, segments): the frame stroke, then the leader stroke
    pub strokes: Vec<(f64, Vec<Seg>)>,
    pub fills: Vec<Fill>,
    pub texts: Vec<Text>,
    pub boxx: [f64; 4],
    pub parts: Option<Vec<Value>>,
    pub frame_box: Option<[f64; 4]>,
    pub triangle: Option<Fill>,
    pub circle: Option<([f64; 2], f64)>,
}
impl Geometry {
    pub fn to_json(&self) -> Value {
        let segs = |s: &[Seg]| Value::Array(s.iter().map(|g| jv::nums(g)).collect());
        let fill = |f: &Fill| Value::Array(f.iter().map(|p| jv::pt2(*p)).collect());
        let mut o = jv::obj(vec![
            ("strokes", Value::Array(self.strokes.iter().map(|(w, s)| jv::obj(vec![("width", jv::num(*w)), ("segs", segs(s))])).collect())),
            ("fills", Value::Array(self.fills.iter().map(|f| jv::obj(vec![("pts", fill(f))])).collect())),
            ("texts", Value::Array(self.texts.iter().map(Text::to_json).collect())),
            ("box", jv::nums(&self.boxx)),
        ]);
        if let Some(p) = &self.parts {
            jv::set(&mut o, "parts", Value::Array(p.clone()));
        }
        if let Some(b) = self.frame_box {
            jv::set(&mut o, "frameBox", jv::nums(&b));
        }
        if let Some(tr) = &self.triangle {
            jv::set(&mut o, "triangle", fill(tr));
        }
        if let Some((c, r)) = self.circle {
            jv::set(&mut o, "circle", jv::obj(vec![("c", jv::pt2(c)), ("r", jv::num(r))]));
        }
        o
    }
}

/// `annotationGeometry(a, anchor)`.
pub fn annotation_geometry(a: &Value, anchor: [f64; 2]) -> Result<Geometry, String> {
    let h = H;
    let mut frame: Vec<Seg> = Vec::new();
    let mut leader: Vec<Seg> = Vec::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut texts: Vec<Text> = Vec::new();
    let off = jv::arr(a.get("off"));
    let offx = off.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
    let offy = off.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let p = [anchor[0] + offx, anchor[1] + offy];
    let mut g = Geometry::default();
    let rect = |segs: &mut Vec<Seg>, x0: f64, y0: f64, x1: f64, y1: f64| {
        segs.push([x0, y0, x1, y0]);
        segs.push([x1, y0, x1, y1]);
        segs.push([x1, y1, x0, y1]);
        segs.push([x0, y1, x0, y0]);
    };
    let leader_to = |leader: &mut Vec<Seg>, fills: &mut Vec<Fill>, from: [f64; 2]| {
        if js::hypot2(anchor[0] - from[0], anchor[1] - from[1]) > ARROW_L * 1.2 {
            leader.push([from[0], from[1], anchor[0], anchor[1]]);
            arrow_head(fills, anchor, from);
        }
    };
    match jv::s(a, "kind") {
        "fcf" => {
            let hh = 2.0 * h;
            let (y0, y1, ty) = (p[1] - hh / 2.0, p[1] + hh / 2.0, p[1] - 0.35 * h);
            let mut parts: Vec<Value> = Vec::new();
            let mut x = p[0];
            parts.push(jv::obj(vec![("kind", "char".into()), ("box", jv::nums(&[x, y0, x + hh, y1]))]));
            char_symbol(jv::s(a, "char"), [x + hh / 2.0, p[1]], &mut frame, &mut fills)?;
            x += hh;
            let tol_text = num4(jv::n(a, "tol"));
            let dia = jv::truthy(a.get("dia"));
            let md = js::str_of(a.get("mod"));
            let mut w = PAD + if dia { DIA_W } else { 0.0 } + text_w(&tol_text, h) + if !md.is_empty() { 0.6 + MOD_W } else { 0.0 } + PAD;
            w = js::max(w, hh);
            let mut cx = x + PAD;
            if dia {
                dia_symbol([cx + DIA_W / 2.0, p[1]], &mut frame);
                cx += DIA_W;
            }
            texts.push(Text { x: cx, y: ty, size: h, text: tol_text.clone(), anchor: "start" });
            cx += text_w(&tol_text, h);
            if !md.is_empty() {
                mod_symbol(&md, [cx + 0.6 + MOD_W / 2.0, p[1]], &mut frame);
            }
            parts.push(jv::obj(vec![("kind", "tol".into()), ("box", jv::nums(&[x, y0, x + w, y1]))]));
            x += w;
            for d in jv::arr(a.get("datums")) {
                let letter = js::str_of(d.get("letter"));
                let dm = js::str_of(d.get("mod"));
                let has_m = jv::truthy(d.get("mod"));
                let dw = js::max(hh, PAD + text_w(&letter, h) + if has_m { 0.6 + MOD_W } else { 0.0 } + PAD);
                let inner = text_w(&letter, h) + if has_m { 0.6 + MOD_W } else { 0.0 };
                let sx = x + (dw - inner) / 2.0;
                texts.push(Text { x: sx, y: ty, size: h, text: letter.clone(), anchor: "start" });
                if has_m {
                    mod_symbol(&dm, [sx + text_w(&letter, h) + 0.6 + MOD_W / 2.0, p[1]], &mut frame);
                }
                parts.push(jv::obj(vec![("kind", "datum".into()), ("letter", letter.into()), ("box", jv::nums(&[x, y0, x + dw, y1]))]));
                x += dw;
            }
            rect(&mut frame, p[0], y0, x, y1);
            for pt in parts.iter().skip(1) {
                let bx = jv::arr(pt.get("box")).first().and_then(Value::as_f64).unwrap_or(f64::NAN);
                frame.push([bx, y0, bx, y1]);
            }
            let fb = [p[0], y0, x, y1];
            if !inside(fb, anchor) {
                leader_to(&mut leader, &mut fills, nearest_on_box(fb, anchor));
            }
            g.parts = Some(parts);
            g.frame_box = Some(fb);
        }
        "datum" => {
            let s = 2.0 * h;
            let fb = [p[0] - s / 2.0, p[1] - s / 2.0, p[0] + s / 2.0, p[1] + s / 2.0];
            rect(&mut frame, fb[0], fb[1], fb[2], fb[3]);
            texts.push(Text { x: p[0], y: p[1] - 0.35 * h, size: h, text: js::str_of(a.get("letter")), anchor: "middle" });
            let (dx, dy) = (p[0] - anchor[0], p[1] - anchor[1]);
            let vertical = dx.abs() <= 2.0 * dy.abs();
            let ux = if vertical { 0.0 } else { js::sign(dx) };
            let uy = if vertical { if dy < 0.0 { -1.0 } else { 1.0 } } else { 0.0 };
            let side = 3.0;
            let th = side * 3.0f64.sqrt() / 2.0;
            let apex = [anchor[0] + ux * th, anchor[1] + uy * th];
            let tri = vec![
                [anchor[0] - uy * side / 2.0, anchor[1] + ux * side / 2.0],
                [anchor[0] + uy * side / 2.0, anchor[1] - ux * side / 2.0],
                apex,
            ];
            fills.push(tri.clone());
            let start = nearest_on_box(fb, apex);
            if !inside(fb, apex) && js::hypot2(start[0] - apex[0], start[1] - apex[1]) > 1e-9 {
                leader.push([start[0], start[1], apex[0], apex[1]]);
            }
            g.frame_box = Some(fb);
            g.triangle = Some(tri);
        }
        "finish" => {
            let (h1, h2) = (1.4 * h, 3.0 * h);
            let k = 1.0 / js::tan(std::f64::consts::PI / 3.0);
            let tip = p;
            let s_top = [tip[0] - h1 * k, tip[1] + h1];
            let l_top = [tip[0] + h2 * k, tip[1] + h2];
            frame.push([tip[0], tip[1], s_top[0], s_top[1]]);
            frame.push([tip[0], tip[1], l_top[0], l_top[1]]);
            let process = jv::s(a, "process");
            if process == "removal" {
                frame.push([s_top[0], s_top[1], tip[0] + h1 * k, tip[1] + h1]);
            }
            if process == "noRemoval" {
                let rc = h1 / 3.0;
                poly(&circle_pts([tip[0], tip[1] + 2.0 * rc], rc, 32, 0.0, 2.0 * std::f64::consts::PI), &mut frame);
                g.circle = Some(([tip[0], tip[1] + 2.0 * rc], rc));
            }
            let ra_text = format!("Ra {}", num4(jv::n(a, "ra")));
            let note = js::str_of(a.get("note"));
            let has_note = jv::truthy(a.get("note"));
            let ext = js::max(text_w(&ra_text, h), if has_note { text_w(&note, 2.5) } else { 0.0 }) + 2.0;
            frame.push([l_top[0], l_top[1], l_top[0] + ext, l_top[1]]);
            texts.push(Text { x: l_top[0] + 1.0, y: l_top[1] - 1.0 - 0.75 * h, size: h, text: ra_text, anchor: "start" });
            if has_note {
                texts.push(Text { x: l_top[0] + 1.0, y: l_top[1] + 1.0, size: 2.5, text: note, anchor: "start" });
            }
            if js::hypot2(offx, offy) > 1e-9 {
                leader_to(&mut leader, &mut fills, tip);
            }
        }
        _ => return Err(t("model.gdt.unknown_kind", &[])),
    }
    g.strokes = vec![(FRAME, frame), (LEADER, leader)];
    g.boxx = box_of(&g.strokes, &fills, &texts);
    g.fills = fills;
    g.texts = texts;
    Ok(g)
}
