//! The model's file form (model.js `toJSON`, `load`, `defToJSON`, `defFromJSON`, `cleanLight`, `cleanText`,
//! `cleanStoreys`, `cleanScene`). `load` reads untrusted JSON: every count is capped as the JS caps it, definitions
//! that would contain themselves are refused (checked without recursion), and what is malformed is either refused
//! with the JS's message or left out exactly where the JS leaves it out.

#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::rc::Rc;

use serde_json::{Map, Value};

use crate::def::*;
use crate::error::{Error, Res, msg, msg_with};
use crate::i18n::t;
use crate::js;
use crate::json::{self, Out};
use crate::model::{Dim, End, Guide, Light, Model, RadC, Scene, SceneVis, Section, SectionView, Storey, Tag};
use crate::omap::{FxMap, IdMap, OMap};
use crate::ops::{SPLINE_SEGS, is_curve_id};
use crate::paint::{is_color, is_paint};
use crate::vec::*;

pub const FORMAT: &str = "modelcraft";
pub const VERSION: f64 = 1.0;
pub const UNIT_KEYS: [&str; 5] = ["ft-in", "in", "m", "cm", "mm"];
pub const STYLE_MODES: [&str; 4] = ["shaded", "hidden", "wire", "xray"];
pub const LIGHT_KINDS: [&str; 3] = ["point", "spot", "area"];

// ---------------------------------------------------------------------------------------------------------------
// defToJSON

/// `JSON.stringify(defToJSON(d))` appended to `o`.
pub fn def_json<D: DefView>(d: &D, o: &mut Out) {
    o.raw("{\"id\":");
    o.str(d.id());
    o.raw(",\"kind\":");
    o.str(d.kind());
    o.raw(",\"name\":");
    o.str(d.name());
    o.raw(",\"next\":");
    o.num(d.next());
    if let Some(s) = d.starter() {
        o.raw(",\"starter\":");
        o.str(s);
    }
    if let Some(dy) = d.dyn_info() {
        o.raw(",\"dyn\":{\"src\":");
        o.str(&dy.src);
        o.raw(",\"size\":");
        o.nums(&dy.size);
        o.ch('}');
    }
    o.raw(",\"verts\":[");
    for (i, (id, p)) in d.verts().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.ch('[');
        o.num(id);
        o.ch(',');
        o.num(p[0]);
        o.ch(',');
        o.num(p[1]);
        o.ch(',');
        o.num(p[2]);
        o.ch(']');
    }
    o.raw("],\"edges\":[");
    for (i, (id, e)) in d.edges().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.ch('[');
        o.num(id);
        o.ch(',');
        o.num(e.a);
        o.ch(',');
        o.num(e.b);
        if e.hidden {
            o.raw(if e.soft { ",1," } else { ",0," });
            match &e.curve {
                Some(c) => o.str(c),
                None => o.ch('0'),
            }
            o.raw(",1");
        } else if let Some(c) = &e.curve {
            o.raw(if e.soft { ",1," } else { ",0," });
            o.str(c);
        } else if e.soft {
            o.raw(",1");
        }
        o.ch(']');
    }
    o.raw("],\"faces\":[");
    for (i, (id, f)) in d.faces().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.raw("{\"id\":");
        o.num(id);
        o.raw(",\"loops\":[");
        for (k, l) in f.loops.iter().enumerate() {
            if k > 0 {
                o.ch(',');
            }
            o.nums(l);
        }
        o.ch(']');
        if let Some(c) = &f.color {
            o.raw(",\"color\":");
            o.str(c);
        }
        if f.hidden {
            o.raw(",\"hidden\":true");
        }
        o.ch('}');
    }
    o.raw("],\"insts\":[");
    for (i, (id, inst)) in d.insts().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        inst_json(id, inst, o);
    }
    o.ch(']');
    if d.has_splines() {
        o.raw(",\"splines\":[");
        for (i, (id, sp)) in d.splines().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"id\":");
            o.str(id);
            o.raw(",\"pts\":[");
            for (k, p) in sp.pts.iter().enumerate() {
                if k > 0 {
                    o.ch(',');
                }
                o.p3(p);
            }
            o.raw("],\"closed\":");
            o.bool(sp.closed);
            o.raw(",\"segs\":");
            o.num(sp.segs);
            o.ch('}');
        }
        o.ch(']');
    }
    o.ch('}');
}

fn inst_json(id: f64, i: &Inst, o: &mut Out) {
    o.raw("{\"id\":");
    o.num(id);
    o.raw(",\"def\":");
    o.str(&i.def);
    o.raw(",\"m\":");
    o.nums(&i.m);
    if let Some(n) = i.name.as_deref().filter(|n| !n.is_empty()) {
        o.raw(",\"name\":");
        o.str(n);
    }
    if let Some(c) = i.color.as_deref().filter(|c| !c.is_empty()) {
        o.raw(",\"color\":");
        o.str(c);
    }
    if i.hidden {
        o.raw(",\"hidden\":true");
    }
    if let Some(t) = i.tag.as_deref().filter(|t| !t.is_empty()) {
        o.raw(",\"tag\":");
        o.str(t);
    }
    if let Some(r) = &i.rule {
        o.raw(",\"rule\":{");
        for (n, k) in r.keys().into_iter().enumerate() {
            if n > 0 {
                o.ch(',');
            }
            o.key(k);
            match k {
                "x" | "y" | "z" => o.str(
                    match k {
                        "x" => r.x.as_deref(),
                        "y" => r.y.as_deref(),
                        _ => r.z.as_deref(),
                    }
                    .unwrap_or(""),
                ),
                _ => o.num(
                    match k {
                        "sx" => r.sx,
                        "sy" => r.sy,
                        _ => r.sz,
                    }
                    .unwrap_or(f64::NAN),
                ),
            }
        }
        o.ch('}');
    }
    o.ch('}');
}

/// `JSON.stringify(defToJSON(d))`.
pub fn def_string<D: DefView>(d: &D) -> String {
    let mut o = Out::new(false);
    def_json(d, &mut o);
    o.s
}

/// A JS number as a JSON value (a non-finite number becomes null: the checks defFromJSON makes treat both alike).
#[inline]
pub fn jn(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}
fn jp3(p: &P3) -> Value {
    Value::Array(vec![jn(p[0]), jn(p[1]), jn(p[2])])
}

/// `defToJSON(d)` as a live object (makeUnique, copyClip, revertPreview hand it to defFromJSON without a string).
pub fn def_value(d: &Def) -> Value {
    def_value_with(d, jn)
}
/// `JSON.parse(JSON.stringify(defToJSON(d)))`: as def_value, with −0 read back as 0.
pub fn def_value_text<D: DefView>(d: &D) -> Value {
    def_value_with(d, |x| jn(if x == 0.0 { 0.0 } else { x }))
}
fn def_value_with<D: DefView>(d: &D, jn: impl Fn(f64) -> Value + Copy) -> Value {
    let jp3 = |p: &P3| Value::Array(vec![jn(p[0]), jn(p[1]), jn(p[2])]);
    let mut m = Map::new();
    m.insert("id".into(), Value::String(d.id().to_string()));
    m.insert("kind".into(), Value::String(d.kind().to_string()));
    m.insert("name".into(), Value::String(d.name().to_string()));
    m.insert("next".into(), jn(d.next()));
    if let Some(s) = d.starter() {
        m.insert("starter".into(), Value::String(s.clone()));
    }
    if let Some(dy) = d.dyn_info() {
        let mut o = Map::new();
        o.insert("src".into(), Value::String(dy.src.clone()));
        o.insert("size".into(), Value::Array(dy.size.iter().map(|&x| jn(x)).collect()));
        m.insert("dyn".into(), Value::Object(o));
    }
    m.insert("verts".into(), Value::Array(d.verts().map(|(id, p)| Value::Array(vec![jn(id), jn(p[0]), jn(p[1]), jn(p[2])])).collect()));
    m.insert(
        "edges".into(),
        Value::Array(
            d.edges()
                .map(|(id, e)| {
                    let mut a = vec![jn(id), jn(e.a), jn(e.b)];
                    let soft = jn(if e.soft { 1.0 } else { 0.0 });
                    if e.hidden {
                        a.push(soft);
                        a.push(e.curve.clone().map_or(jn(0.0), Value::String));
                        a.push(jn(1.0));
                    } else if let Some(c) = &e.curve {
                        a.push(soft);
                        a.push(Value::String(c.clone()));
                    } else if e.soft {
                        a.push(jn(1.0));
                    }
                    Value::Array(a)
                })
                .collect(),
        ),
    );
    m.insert(
        "faces".into(),
        Value::Array(
            d.faces()
                .map(|(id, f)| {
                    let mut o = Map::new();
                    o.insert("id".into(), jn(id));
                    o.insert(
                        "loops".into(),
                        Value::Array(f.loops.iter().map(|l| Value::Array(l.iter().map(|&v| jn(v)).collect())).collect()),
                    );
                    if let Some(c) = &f.color {
                        o.insert("color".into(), Value::String(c.clone()));
                    }
                    if f.hidden {
                        o.insert("hidden".into(), Value::Bool(true));
                    }
                    Value::Object(o)
                })
                .collect(),
        ),
    );
    m.insert("insts".into(), Value::Array(d.insts().map(|(id, i)| inst_value_with(id, i, jn)).collect()));
    if d.has_splines() {
        m.insert(
            "splines".into(),
            Value::Array(
                d.splines()
                    .map(|(id, sp)| {
                        let mut o = Map::new();
                        o.insert("id".into(), Value::String(id.clone()));
                        o.insert("pts".into(), Value::Array(sp.pts.iter().map(jp3).collect()));
                        o.insert("closed".into(), Value::Bool(sp.closed));
                        o.insert("segs".into(), jn(sp.segs));
                        Value::Object(o)
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(m)
}

pub fn inst_value(id: f64, i: &Inst) -> Value {
    inst_value_with(id, i, jn)
}
fn inst_value_with(id: f64, i: &Inst, jn: impl Fn(f64) -> Value) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), jn(id));
    o.insert("def".into(), Value::String(i.def.clone()));
    o.insert("m".into(), Value::Array(i.m.iter().map(|&x| jn(x)).collect()));
    if let Some(n) = i.name.as_deref().filter(|n| !n.is_empty()) {
        o.insert("name".into(), Value::String(n.to_string()));
    }
    if let Some(c) = i.color.as_deref().filter(|c| !c.is_empty()) {
        o.insert("color".into(), Value::String(c.to_string()));
    }
    if i.hidden {
        o.insert("hidden".into(), Value::Bool(true));
    }
    if let Some(t) = i.tag.as_deref().filter(|t| !t.is_empty()) {
        o.insert("tag".into(), Value::String(t.to_string()));
    }
    if let Some(r) = &i.rule {
        let mut ro = Map::new();
        for k in r.keys() {
            let v = match k {
                "x" => r.x.clone().map(Value::String),
                "y" => r.y.clone().map(Value::String),
                "z" => r.z.clone().map(Value::String),
                "sx" => r.sx.map(&jn),
                "sy" => r.sy.map(&jn),
                _ => r.sz.map(&jn),
            };
            if let Some(v) = v {
                ro.insert(k.into(), v);
            }
        }
        o.insert("rule".into(), Value::Object(ro));
    }
    Value::Object(o)
}

// ---------------------------------------------------------------------------------------------------------------
// defFromJSON

/// How a check that fails in defFromJSON throws: `fail` of load (the message wrapped in m3d.model.file_bad) or the
/// default (the message alone).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    File,
    Plain,
}
impl Fail {
    pub fn err(self, key: &str) -> Error {
        match self {
            Fail::Plain => msg(key),
            Fail::File => msg_with("m3d.model.file_bad", &[("why", &t(key, &[]))]),
        }
    }
}

/// /^d\d{1,9}$/
pub fn is_def_id(s: &str) -> bool {
    s.strip_prefix('d').is_some_and(|n| (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}
fn int_id(v: Option<&Value>) -> Option<f64> {
    json::int(v).filter(|&x| x > 0.0 && x < 1e9)
}
/// A key of `d.verts` given as a JSON value (`d.verts.has(x)`).
fn vert_key(d: &Def, v: Option<&Value>) -> Option<f64> {
    json::num(v).filter(|&x| d.verts.has(x))
}
const RULE_WORDS: [&str; 7] = ["auto", "stretch", "start", "end", "move", "scale", "repeat"];

/// `defFromJSON(d, j, fail)`.
pub fn def_from_json(d: &mut Def, j: &Value, fail: Fail) -> Res<()> {
    if let Some(s) = json::str(j.get("starter"))
        && (1..=32).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        d.starter = Some(s.to_string());
    }
    if let Some(dy) = j.get("dyn")
        && json::truthy(Some(dy))
        && let Some(src) = json::str(dy.get("src"))
        && is_def_id(src)
        && let Some(size) = dy.get("size").and_then(Value::as_array)
        && size.len() == 3
        && size.iter().all(|x| json::finite(Some(x)).is_some_and(|x| x >= 0.0))
    {
        d.dyn_ = Some(Dyn { src: src.to_string(), size: size.iter().filter_map(Value::as_f64).collect() });
    }
    let mut max = 0.0f64;
    for v in j.get("verts").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
        let Some(a) = v.as_array().filter(|a| a.len() == 4) else { return Err(fail.err("m3d.model.a_vertex_is_wrong")) };
        let (Some(id), Some(x), Some(y), Some(z)) =
            (int_id(a.first()), json::is_num(a.get(1)), json::is_num(a.get(2)), json::is_num(a.get(3)))
        else {
            return Err(fail.err("m3d.model.a_vertex_is_wrong"));
        };
        d.verts.set(id, [x, y, z]);
        max = js::max(max, id);
    }
    for e in j.get("edges").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
        let Some(a) = e.as_array() else { return Err(fail.err("m3d.model.an_edge_is_wrong")) };
        let (Some(id), Some(va), Some(vb)) = (int_id(a.first()), vert_key(d, a.get(1)), vert_key(d, a.get(2))) else {
            return Err(fail.err("m3d.model.an_edge_is_wrong"));
        };
        if va == vb {
            return Err(fail.err("m3d.model.an_edge_is_wrong"));
        }
        if edge_between(d, va, vb).is_some() {
            continue;
        }
        let curve = json::str(a.get(4)).filter(|c| is_curve_id(c)).map(str::to_string);
        let ed = Edge { a: va, b: vb, soft: json::truthy(a.get(3)), curve, hidden: json::num(a.get(5)) == Some(1.0) };
        d.edges.set(id, ed);
        ek(d).insert(key(va, vb), id);
        max = js::max(max, id);
    }
    for f in j.get("faces").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
        let fid = if json::truthy(Some(f)) { int_id(f.get("id")) } else { None };
        let (Some(fid), Some(loops)) = (fid, f.get("loops").and_then(Value::as_array).filter(|l| !l.is_empty())) else {
            return Err(fail.err("m3d.model.a_face_is_wrong"));
        };
        let mut ls: Vec<Vec<f64>> = Vec::with_capacity(loops.len());
        for l in loops {
            let arr = l.as_array().map_or(&[][..], |a| a.as_slice());
            if arr.len() < 3 {
                return Err(fail.err("m3d.model.a_face_is_wrong"));
            }
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                let Some(x) = vert_key(d, Some(v)) else { return Err(fail.err("m3d.model.a_face_is_wrong")) };
                out.push(x);
            }
            ls.push(out);
        }
        max = js::max(max, fid);
        d.next = js::max(d.next, max + 1.0);
        let color = json::str(f.get("color")).filter(|c| is_paint(c)).map(str::to_ascii_lowercase);
        f_add(d, ls, color, Some(fid), json::is_true(f.get("hidden")));
    }
    for i in j.get("insts").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
        let ok = json::truthy(Some(i));
        let id = if ok { int_id(i.get("id")) } else { None };
        let def = if ok { json::str(i.get("def")) } else { None };
        let m = i.get("m").and_then(Value::as_array).filter(|m| m.len() == 16);
        let (Some(id), Some(def), Some(m)) = (id, def, m) else { return Err(fail.err("m3d.model.an_instance_is_wrong")) };
        let mut mm = [0.0f64; 16];
        for (k, x) in m.iter().enumerate() {
            let Some(v) = json::is_num(Some(x)) else { return Err(fail.err("m3d.model.an_instance_is_wrong")) };
            if let Some(slot) = mm.get_mut(k) {
                *slot = v;
            }
        }
        let mut ins = Inst::new(def.to_string(), mm);
        if json::truthy(i.get("name")) {
            ins.name = Some(json::clean(i.get("name"), 120));
        }
        if let Some(c) = json::str(i.get("color")).filter(|c| is_paint(c)) {
            ins.color = Some(c.to_ascii_lowercase());
        }
        ins.hidden = json::is_true(i.get("hidden"));
        if let Some(tg) = json::str(i.get("tag")) {
            let c = js::clean_text(tg, 80);
            if !c.is_empty() {
                ins.tag = Some(c);
            }
        }
        if let Some(r) = i.get("rule").filter(|r| r.is_object() || r.is_array()) {
            let word = |k: &str| json::str(r.get(k)).filter(|w| RULE_WORDS.contains(w)).map(str::to_string);
            let scale = |k: &str| json::finite(r.get(k)).filter(|&x| x > 0.0 && x < 1e6);
            let rule = Rule { x: word("x"), y: word("y"), z: word("z"), sx: scale("sx"), sy: scale("sy"), sz: scale("sz"), order: None };
            if rule != Rule::default() {
                ins.rule = Some(rule);
            }
        }
        d.insts.set(id, ins);
        max = js::max(max, id);
    }
    for sp in j.get("splines").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice()) {
        let Some(id) = json::str(sp.get("id")).filter(|s| is_curve_id(s)) else { continue };
        let Some(pts) = sp.get("pts").and_then(Value::as_array).filter(|p| (2..=10000).contains(&p.len())) else { continue };
        let pp: Option<Vec<P3>> = pts.iter().map(|p| json::is_pt(Some(p))).collect();
        let Some(pp) = pp else { continue };
        let segs = json::int(sp.get("segs")).filter(|&s| (1.0..=64.0).contains(&s)).unwrap_or(SPLINE_SEGS);
        d.splines.set(id.to_string(), Spline { pts: pp, closed: json::truthy(sp.get("closed")), segs });
    }
    let jnext = json::int(j.get("next")).unwrap_or(1.0);
    d.next = js::max(js::max(jnext, max + 1.0), d.next);
    Ok(())
}

/// `defOf(dj, fail)`: a new definition from its file form.
pub fn def_of(dj: &Value, fail: Fail) -> Res<Def> {
    let id = json::str(dj.get("id")).unwrap_or("");
    let kind = if id == "root" {
        "root"
    } else if json::str(dj.get("kind")) == Some("component") {
        "component"
    } else {
        "group"
    };
    let mut name = json::clean(dj.get("name"), 120);
    if name.is_empty() {
        name = match kind {
            "root" => "Model",
            "component" => "Component",
            _ => "Group",
        }
        .to_string();
    }
    let mut d = Def::new(id, kind, &name);
    def_from_json(&mut d, dj, fail)?;
    Ok(d)
}

// ---------------------------------------------------------------------------------------------------------------
// clean*

/// `cleanLight(o)` → a fresh light | None.
pub fn clean_light(o: &Value) -> Option<Light> {
    if !o.is_object() && !o.is_array() {
        return None;
    }
    let kind = json::str(o.get("kind")).filter(|k| LIGHT_KINDS.contains(k))?;
    let p = json::is_pt(o.get("p"))?;
    let clamp = |x: Option<&Value>, lo: f64, hi: f64, d: f64| json::is_num(x).map_or(d, |x| js::min(hi, js::max(lo, x)));
    let dir = json::is_pt(o.get("dir")).filter(|d| len(*d) > 1e-9).map_or([0.0, 0.0, -1.0], norm);
    let size = match o.get("size").and_then(Value::as_array).filter(|s| s.len() == 2) {
        Some(s) => [clamp(s.first(), 0.02, 20.0, 0.6), clamp(s.get(1), 0.02, 20.0, 0.6)],
        None if kind == "area" => [0.6, 0.6],
        None => [0.06, 0.06],
    };
    Some(Light {
        kind: kind.to_string(),
        p,
        dir,
        color: json::str(o.get("color")).filter(|c| is_color(c)).map_or_else(|| "#ffe7c4".to_string(), str::to_ascii_lowercase),
        lumens: js::round(clamp(o.get("lumens"), 1.0, 200000.0, 800.0)),
        angle: js::round(clamp(o.get("angle"), 5.0, 170.0, 60.0)),
        size,
        on: json::not_false(o.get("on")),
        name: json::clean(o.get("name"), 80),
    })
}

/// A light as the object `cleanLight` made (for `{...L, ...patch}`).
pub fn light_value(l: &Light) -> Value {
    let mut o = Map::new();
    o.insert("kind".into(), Value::String(l.kind.clone()));
    o.insert("p".into(), jp3(&l.p));
    o.insert("dir".into(), jp3(&l.dir));
    o.insert("color".into(), Value::String(l.color.clone()));
    o.insert("lumens".into(), jn(l.lumens));
    o.insert("angle".into(), jn(l.angle));
    o.insert("size".into(), Value::Array(vec![jn(l.size[0]), jn(l.size[1])]));
    o.insert("on".into(), Value::Bool(l.on));
    o.insert("name".into(), Value::String(l.name.clone()));
    Value::Object(o)
}

/// `Number(x)` (ToNumber) for a JSON value (absent = NaN).
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => str_to_number(s),
        Some(Value::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [x] => str_to_number(&to_js_string(x)),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}
fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js::num_str(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { to_js_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}
/// StringToNumber: trimmed; '' → 0; decimal literals, Infinity, 0x / 0o / 0b integers; else NaN.
pub fn str_to_number(s: &str) -> f64 {
    let t = js::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| -> f64 {
        if p.is_empty() || !p.chars().all(|c| c.is_digit(r)) {
            return f64::NAN;
        }
        p.chars().fold(0.0, |acc, c| acc * f64::from(r) + f64::from(c.to_digit(r).unwrap_or(0)))
    };
    let lower = t.get(..2).map(str::to_ascii_lowercase);
    match lower.as_deref() {
        Some("0x") => return radix(t.get(2..).unwrap_or(""), 16),
        Some("0o") => return radix(t.get(2..).unwrap_or(""), 8),
        Some("0b") => return radix(t.get(2..).unwrap_or(""), 2),
        _ => {}
    }
    let (sgn, body) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, t.get(1..).unwrap_or("")),
        Some(b'+') => (1.0, t.get(1..).unwrap_or("")),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sgn * f64::INFINITY;
    }
    let b = body.as_bytes();
    let mut i = 0usize;
    let mut digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            digits = digits.saturating_add(1);
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let start = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j == start {
            return f64::NAN;
        }
        i = j;
    }
    if i != b.len() {
        return f64::NAN;
    }
    let mut lit = String::from(body);
    if lit.starts_with('.') {
        lit.insert(0, '0');
    }
    lit = lit.replace(".e", ".0e").replace(".E", ".0E");
    if lit.ends_with('.') {
        lit.push('0');
    }
    sgn * lit.parse::<f64>().unwrap_or(f64::NAN)
}

/// `cleanStoreys(list)`: named, finite elevations, sorted, no two at one elevation.
pub fn clean_storeys(list: Option<&Value>) -> Vec<Storey> {
    let mut out: Vec<Storey> = Vec::new();
    for s in list.and_then(Value::as_array).map_or(&[][..], |a| a.get(..200).unwrap_or(a)) {
        let name = json::clean(s.get("name"), 80);
        let e = to_number(s.get("elevation"));
        if name.is_empty() || !e.is_finite() || e.abs() > 1e6 || out.iter().any(|o| (o.elevation - e).abs() < 1e-6) {
            continue;
        }
        out.push(Storey { name, elevation: e });
    }
    js::sort_by(&mut out, |a, b| a.elevation - b.elevation);
    out
}

/// `cleanScene(s)`: a scene from a file, every field checked.
pub fn clean_scene(s: &Value) -> Option<Scene> {
    if !s.is_object() && !s.is_array() {
        return None;
    }
    let empty = Value::Object(Map::new());
    let pick = |v: Option<&Value>| -> Value { v.filter(|x| json::truthy(Some(x))).cloned().unwrap_or_else(|| empty.clone()) };
    let c = pick(s.get("cam"));
    let eye = json::is_pt(c.get("eye"))?;
    let target = json::is_pt(c.get("target"))?;
    if dist(eye, target) < 1e-9 {
        return None;
    }
    let st = pick(s.get("style"));
    let sun = pick(st.get("sun"));
    let num = |x: Option<&Value>, lo: f64, hi: f64, d: f64| json::finite(x).map_or(d, |x| js::min(hi, js::max(lo, x)));
    let vis = s.get("vis").filter(|v| v.is_object() || v.is_array()).map(|v| {
        let paths: Vec<Vec<f64>> = v
            .get("insts")
            .and_then(Value::as_array)
            .map_or(&[][..], |a| a.get(..20000).unwrap_or(a))
            .iter()
            .filter_map(|p| {
                let a = p.as_array()?;
                if a.is_empty() || a.len() > 32 {
                    return None;
                }
                a.iter().map(|x| json::int(Some(x))).collect::<Option<Vec<f64>>>()
            })
            .collect();
        let tags: Vec<String> = v
            .get("tags")
            .and_then(Value::as_array)
            .map_or(&[][..], |a| a.get(..1000).unwrap_or(a))
            .iter()
            .map(|x| json::clean(Some(x), 80))
            .filter(|x| !x.is_empty())
            .collect();
        SceneVis { insts: paths, tags, section: json::int(v.get("section")), cuts: json::not_false(v.get("cuts")) }
    });
    let mut name = json::clean(s.get("name"), 80);
    if name.is_empty() {
        name = "Scene".into();
    }
    let mode = json::str(st.get("mode")).filter(|m| STYLE_MODES.contains(m)).unwrap_or("shaded").to_string();
    Some(Scene {
        vis,
        name,
        eye,
        target,
        fov: num(c.get("fov"), 10.0, 120.0, 35.0),
        persp: json::not_false(c.get("persp")),
        height: num(c.get("height"), 1e-6, 1e7, 10.0),
        mode,
        edges: json::not_false(st.get("edges")),
        shadows: json::is_true(st.get("shadows")),
        month: js::round(num(sun.get("month"), 1.0, 12.0, 6.0)),
        day: js::round(num(sun.get("day"), 1.0, 31.0, 21.0)),
        hour: num(sun.get("hour"), 0.0, 23.99, 15.0),
        lat: num(sun.get("lat"), -90.0, 90.0, 40.0),
        lon: num(sun.get("lon"), -180.0, 180.0, -105.0),
        tz: num(sun.get("tz"), -14.0, 14.0, -7.0),
    })
}

/// A scene as JSON.
pub fn scene_json(s: &Scene, o: &mut Out) {
    o.ch('{');
    if let Some(v) = &s.vis {
        o.raw("\"vis\":{\"insts\":[");
        for (i, p) in v.insts.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.nums(p);
        }
        o.raw("],\"tags\":");
        o.strs(&v.tags);
        o.raw(",\"section\":");
        match v.section {
            Some(x) => o.num(x),
            None => o.raw("null"),
        }
        o.raw(",\"cuts\":");
        o.bool(v.cuts);
        o.raw("},");
    }
    o.raw("\"name\":");
    o.str(&s.name);
    o.raw(",\"cam\":{\"eye\":");
    o.p3(&s.eye);
    o.raw(",\"target\":");
    o.p3(&s.target);
    o.raw(",\"fov\":");
    o.num(s.fov);
    o.raw(",\"persp\":");
    o.bool(s.persp);
    o.raw(",\"height\":");
    o.num(s.height);
    o.raw("},\"style\":{\"mode\":");
    o.str(&s.mode);
    o.raw(",\"edges\":");
    o.bool(s.edges);
    o.raw(",\"shadows\":");
    o.bool(s.shadows);
    o.raw(",\"sun\":{\"month\":");
    o.num(s.month);
    o.raw(",\"day\":");
    o.num(s.day);
    o.raw(",\"hour\":");
    o.num(s.hour);
    o.raw(",\"lat\":");
    o.num(s.lat);
    o.raw(",\"lon\":");
    o.num(s.lon);
    o.raw(",\"tz\":");
    o.num(s.tz);
    o.raw("}}}");
}

pub fn light_json(id: f64, l: &Light, o: &mut Out) {
    o.raw("{\"id\":");
    o.num(id);
    o.raw(",\"kind\":");
    o.str(&l.kind);
    o.raw(",\"p\":");
    o.p3(&l.p);
    o.raw(",\"dir\":");
    o.p3(&l.dir);
    o.raw(",\"color\":");
    o.str(&l.color);
    o.raw(",\"lumens\":");
    o.num(l.lumens);
    o.raw(",\"angle\":");
    o.num(l.angle);
    o.raw(",\"size\":");
    o.nums(&l.size);
    o.raw(",\"on\":");
    o.bool(l.on);
    o.raw(",\"name\":");
    o.str(&l.name);
    o.ch('}');
}

// ---------------------------------------------------------------------------------------------------------------
// toJSON / load

impl Model {
    /// `JSON.stringify(model.toJSON(noDefs))` appended to `o` (canon: non-finite numbers as the kit's markers).
    /// Like the JS, it brings every dimension end up to date on the way (endAt / radialAt).
    pub fn to_json_out(&mut self, o: &mut Out, no_defs: bool) {
        o.raw("{\"format\":\"modelcraft\",\"version\":1,\"units\":");
        o.str(&self.units);
        o.raw(",\"nextId\":");
        o.num(self.next_id);
        o.raw(",\"defs\":");
        if no_defs {
            o.ch('0');
        } else {
            o.ch('[');
            for (i, d) in self.defs.values().enumerate() {
                if i > 0 {
                    o.ch(',');
                }
                def_json(d, o);
            }
            o.ch(']');
        }
        o.raw(",\"guides\":[");
        for (i, (id, g)) in self.guides.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"id\":");
            o.num(id);
            match g {
                Guide::Line { p, d } => {
                    o.raw(",\"kind\":\"line\",\"p\":");
                    o.p3(p);
                    o.raw(",\"d\":");
                    o.p3(d);
                }
                Guide::Point { p } => {
                    o.raw(",\"kind\":\"point\",\"p\":");
                    o.p3(p);
                }
            }
            o.ch('}');
        }
        o.ch(']');
        if !self.lights.is_empty() {
            o.raw(",\"lights\":[");
            for (i, (id, l)) in self.lights.iter().enumerate() {
                if i > 0 {
                    o.ch(',');
                }
                light_json(id, l, o);
            }
            o.ch(']');
        }
        if !self.sections.is_empty() {
            o.raw(",\"sections\":[");
            for (i, (id, s)) in self.sections.iter().enumerate() {
                if i > 0 {
                    o.ch(',');
                }
                o.raw("{\"id\":");
                o.num(id);
                o.raw(",\"p\":");
                o.p3(&s.p);
                o.raw(",\"n\":");
                o.p3(&s.n);
                o.raw(",\"name\":");
                o.str(&s.name);
                o.ch('}');
            }
            o.raw("],\"activeSection\":");
            match self.active_section {
                Some(x) => o.num(x),
                None => o.raw("null"),
            }
        }
        if !(self.section_view.planes && self.section_view.cuts) {
            o.raw(",\"sectionView\":{\"planes\":");
            o.bool(self.section_view.planes);
            o.raw(",\"cuts\":");
            o.bool(self.section_view.cuts);
            o.ch('}');
        }
        if let Some(a) = &self.axes_m {
            o.raw(",\"axes\":");
            o.nums(a);
        }
        if !self.storeys.is_empty() {
            o.raw(",\"storeys\":[");
            for (i, s) in self.storeys.iter().enumerate() {
                if i > 0 {
                    o.ch(',');
                }
                o.raw("{\"name\":");
                o.str(&s.name);
                o.raw(",\"elevation\":");
                o.num(s.elevation);
                o.ch('}');
            }
            o.ch(']');
        }
        o.raw(",\"dims\":[");
        let ids = self.dims.key_vec();
        for (i, id) in ids.into_iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            self.dim_json(id, o);
        }
        o.raw("],\"scenes\":[");
        for (i, s) in self.scenes.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            scene_json(s, o);
        }
        o.ch(']');
        if !self.tags.is_empty() {
            o.raw(",\"tags\":[");
            for (i, g) in self.tags.iter().enumerate() {
                if i > 0 {
                    o.ch(',');
                }
                o.raw("{\"name\":");
                o.str(&g.name);
                if g.hidden {
                    o.raw(",\"hidden\":true");
                }
                o.ch('}');
            }
            o.ch(']');
        }
        o.ch('}');
    }

    fn dim_json(&mut self, id: f64, o: &mut Out) {
        let Some(mut dm) = self.dims.get(id).cloned() else { return };
        let end = |me: &mut Model, e: &mut End, o: &mut Out| {
            let p = me.end_at(e);
            e.p = p;
            o.raw("{\"p\":");
            o.p3(&p);
            if let Some((path, v)) = &e.at {
                o.raw(",\"path\":");
                o.nums(path);
                o.raw(",\"v\":");
                o.num(*v);
            }
            o.ch('}');
        };
        o.raw("{\"id\":");
        o.num(id);
        match &mut dm {
            Dim::Ang { o: eo, a, b, off } => {
                o.raw(",\"kind\":\"ang\",\"o\":");
                end(self, eo, o);
                o.raw(",\"a\":");
                end(self, a, o);
                o.raw(",\"b\":");
                end(self, b, o);
                o.raw(",\"off\":");
                o.p3(off);
            }
            Dim::Rad { kind, c, off } => {
                let g = self.radial_at(c);
                o.raw(",\"kind\":");
                o.str(kind);
                o.raw(",\"c\":{\"p\":");
                o.p3(&g.as_ref().map_or([0.0, 0.0, 0.0], |g| g.c));
                o.raw(",\"r\":");
                o.num(g.as_ref().map_or(0.0, |g| g.r));
                o.raw(",\"n\":");
                o.p3(&g.as_ref().map_or([0.0, 0.0, 1.0], |g| g.n.unwrap_or([f64::NAN; 3])));
                if let Some((path, curve)) = &c.at {
                    o.raw(",\"path\":");
                    o.nums(path);
                    o.raw(",\"curve\":");
                    o.str(curve);
                }
                o.raw("},\"off\":");
                o.p3(off);
            }
            Dim::Lin { a, b, off, mv } => {
                o.raw(",\"a\":");
                end(self, a, o);
                o.raw(",\"b\":");
                end(self, b, o);
                o.raw(",\"off\":");
                o.p3(off);
                o.raw(",\"move\":");
                o.str(mv);
            }
        }
        o.ch('}');
        self.dims.set(id, dm);
    }

    /// `JSON.stringify(model.toJSON(noDefs))`.
    pub fn to_json_string(&mut self, no_defs: bool) -> String {
        let mut o = Out::new(false);
        self.to_json_out(&mut o, no_defs);
        o.s
    }

    /// `load(j)`: open a model from its file form (untrusted). Fails with the JS's messages.
    pub fn load(&mut self, j: &Value, keep_history: bool) -> Res<()> {
        self.load_with(j, keep_history, None)
    }

    /// `load(j, {keepHistory, defs})`: `made` (restore from a Snap) = the definitions in order, each either a new one or
    /// the current definition of that id kept as it is.
    pub(crate) fn load_with(&mut self, j: &Value, keep_history: bool, made: Option<Vec<(String, Option<Def>)>>) -> Res<()> {
        let fail = |k: &str| Err(Fail::File.err(k));
        let restoring = made.is_some();
        if !j.is_object() || json::str(j.get("format")) != Some(FORMAT) {
            return fail("m3d.model.the_format_mark_is_missing");
        }
        let Some(version) = json::int(j.get("version")).filter(|&v| v >= 1.0) else { return fail("m3d.model.the_version_is_missing") };
        if version > VERSION {
            return Err(msg_with("m3d.model.file_newer", &[("v", &js::num_str(version))]));
        }
        let jdefs = j.get("defs").and_then(Value::as_array);
        if made.is_none() && jdefs.is_none_or(|a| a.is_empty()) {
            return fail("m3d.model.it_has_no_model");
        }
        // the definitions: new ones made here, or (restore) kept ones still in self.defs
        let mut fresh: OMap<String, Option<Def>> = OMap::new();
        match made {
            Some(list) => {
                for (id, d) in list {
                    fresh.set(id, d);
                }
            }
            None => {
                for dj in jdefs.map_or(&[][..], |a| a.get(..100_000).unwrap_or(a)) {
                    let id = if json::truthy(Some(dj)) { json::str(dj.get("id")) } else { None };
                    let Some(id) = id.filter(|s| *s == "root" || is_def_id(s)) else { return fail("m3d.model.a_definition_id_is_wrong") };
                    if fresh.has(id) {
                        return fail("m3d.model.a_definition_appears_twice");
                    }
                    let d = def_of(dj, Fail::File)?;
                    fresh.set(d.id.clone(), Some(d));
                }
            }
        }
        if !fresh.has("root") {
            return fail("m3d.model.it_has_no_root");
        }
        {
            let get = |id: &str| -> Option<&Def> {
                match fresh.get(id) {
                    Some(Some(d)) => Some(d),
                    Some(None) => self.defs.get(id),
                    None => None,
                }
            };
            for id in fresh.keys() {
                let Some(d) = get(id) else { continue };
                if d.insts.values().any(|i| !fresh.has(&i.def)) {
                    return fail("m3d.model.an_instance_points_at_a_missing");
                }
            }
            // no definition may contain itself (an explicit stack: definitions nest without limit in a file)
            let mut state: FxMap<String, u8> = FxMap::default();
            for start in fresh.keys() {
                if state.contains_key(start) {
                    continue;
                }
                let mut stack: Vec<(String, Vec<String>, usize)> = Vec::new();
                let kids = |id: &str| get(id).map(|d| d.insts.values().map(|i| i.def.clone()).collect()).unwrap_or_default();
                state.insert(start.clone(), 1);
                stack.push((start.clone(), kids(start), 0));
                while let Some((_, list, k)) = stack.last_mut() {
                    let Some(next) = list.get(*k).cloned() else {
                        if let Some((id, _, _)) = stack.pop() {
                            state.insert(id, 2);
                        }
                        continue;
                    };
                    *k = k.saturating_add(1);
                    match state.get(&next) {
                        Some(1) => return fail("m3d.model.a_group_contains_itself"),
                        Some(_) => {}
                        None => {
                            state.insert(next.clone(), 1);
                            let kl = kids(&next);
                            stack.push((next, kl, 0));
                        }
                    }
                }
            }
        }
        let mut guides: IdMap<Guide> = IdMap::new();
        for g in j.get("guides").and_then(Value::as_array).map_or(&[][..], |a| a.get(..10_000).unwrap_or(a)) {
            let (Some(id), Some(p)) = (json::int(g.get("id")), json::is_pt(g.get("p"))) else { continue };
            match json::str(g.get("kind")) {
                Some("line") => {
                    if let Some(d) = json::is_pt(g.get("d")).filter(|d| len(*d) > 1e-9) {
                        guides.set(id, Guide::Line { p, d: norm(d) });
                    }
                }
                Some("point") => guides.set(id, Guide::Point { p }),
                _ => {}
            }
        }
        let dim_end = |e: Option<&Value>| -> Option<End> {
            let e = e?;
            let p = json::is_pt(e.get("p"))?;
            let mut out = End { p, at: None };
            if let (Some(path), Some(v)) = (e.get("path").and_then(Value::as_array), json::int(e.get("v")))
                && path.len() <= 32
                && let Some(path) = path.iter().map(|x| json::int(Some(x))).collect::<Option<Vec<f64>>>()
            {
                out.at = Some((path, v));
            }
            Some(out)
        };
        let mut dims: IdMap<Dim> = IdMap::new();
        for dm in j.get("dims").and_then(Value::as_array).map_or(&[][..], |a| a.get(..10_000).unwrap_or(a)) {
            let Some(id) = json::int(dm.get("id")) else { continue };
            if guides.has(id) {
                continue;
            }
            let Some(off) = json::is_pt(dm.get("off")) else { continue };
            match json::str(dm.get("kind")) {
                Some("ang") => {
                    if let (Some(o), Some(a), Some(b)) = (dim_end(dm.get("o")), dim_end(dm.get("a")), dim_end(dm.get("b"))) {
                        dims.set(id, Dim::Ang { o, a, b, off });
                    }
                    continue;
                }
                Some(k @ ("rad" | "dia")) => {
                    let c = dm.get("c");
                    let Some(c) = c.filter(|c| json::truthy(Some(c))) else { continue };
                    let (Some(p), Some(n)) = (json::is_pt(c.get("p")), json::is_pt(c.get("n"))) else { continue };
                    let Some(r) = json::finite(c.get("r")).filter(|&r| r >= 0.0) else { continue };
                    let mut rc = RadC { p: Some(p), r, n: Some(n), at: None };
                    if let (Some(path), Some(curve)) = (c.get("path").and_then(Value::as_array), json::str(c.get("curve")))
                        && path.len() <= 32
                        && is_curve_id(curve)
                        && let Some(path) = path.iter().map(|x| json::int(Some(x))).collect::<Option<Vec<f64>>>()
                    {
                        rc.at = Some((path, curve.to_string()));
                    }
                    dims.set(id, Dim::Rad { kind: k.to_string(), c: rc, off });
                    continue;
                }
                _ => {}
            }
            if let (Some(a), Some(b)) = (dim_end(dm.get("a")), dim_end(dm.get("b"))) {
                let mv = json::str(dm.get("move")).filter(|m| ["a", "b", "both"].contains(m)).unwrap_or("b").to_string();
                dims.set(id, Dim::Lin { a, b, off, mv });
            }
        }
        let scenes: Vec<Scene> = j
            .get("scenes")
            .and_then(Value::as_array)
            .map_or(&[][..], |a| a.get(..500).unwrap_or(a))
            .iter()
            .filter_map(clean_scene)
            .collect();
        let mut max_id = 0.0f64;
        for id in fresh.keys() {
            if id != "root" {
                max_id = js::max(max_id, str_to_number(id.get(1..).unwrap_or("")));
            }
        }
        for id in guides.keys() {
            max_id = js::max(max_id, id);
        }
        for id in dims.keys() {
            max_id = js::max(max_id, id);
        }
        let mut lights: IdMap<Light> = IdMap::new();
        for lo in j.get("lights").and_then(Value::as_array).map_or(&[][..], |a| a.get(..1000).unwrap_or(a)) {
            let Some(id) = json::int(lo.get("id")) else { continue };
            if id < 1.0 || guides.has(id) || dims.has(id) || lights.has(id) {
                continue;
            }
            if let Some(l) = clean_light(lo) {
                lights.set(id, l);
                max_id = js::max(max_id, id);
            }
        }
        let mut sections: IdMap<Section> = IdMap::new();
        for s in j.get("sections").and_then(Value::as_array).map_or(&[][..], |a| a.get(..1000).unwrap_or(a)) {
            let Some(id) = json::int(s.get("id")) else { continue };
            if guides.has(id) || dims.has(id) || lights.has(id) || sections.has(id) {
                continue;
            }
            let (Some(p), Some(n)) = (json::is_pt(s.get("p")), json::is_pt(s.get("n"))) else { continue };
            if !(len(n) > 1e-9) {
                continue;
            }
            let mut name = json::clean(s.get("name"), 80);
            if name.is_empty() {
                name = t("m3d.section.name", &[("n", &js::num_str(sections.len() as f64 + 1.0))]);
            }
            sections.set(id, Section { p, n: norm(n), name });
            max_id = js::max(max_id, id);
        }
        let active = json::num(j.get("activeSection")).filter(|&x| sections.has(x));
        self.sections = sections;
        self.active_section = active;
        let sv = j.get("sectionView");
        self.section_view = SectionView {
            planes: json::not_false(sv.and_then(|v| v.get("planes"))),
            cuts: json::not_false(sv.and_then(|v| v.get("cuts"))),
        };
        self.storeys = clean_storeys(j.get("storeys"));
        self.axes_m = None;
        if let Some(a) = j.get("axes").and_then(Value::as_array).filter(|a| a.len() == 16) {
            let nums: Option<Vec<f64>> = a.iter().map(|x| json::is_num(Some(x))).collect();
            if let Some(a) = nums {
                let g = |i: usize| a.get(i).copied().unwrap_or(0.0);
                let (x, y, z) = ([g(0), g(1), g(2)], [g(4), g(5), g(6)], [g(8), g(9), g(10)]);
                // only a proper frame: three square unit directions, right-handed
                if [x, y, z].iter().all(|v| (len(*v) - 1.0).abs() < 1e-6) && dot(x, y).abs() < 1e-6 && dot(cross(x, y), z) > 0.999 {
                    let mut m = [0.0; 16];
                    for (k, v) in a.iter().enumerate() {
                        if let Some(s) = m.get_mut(k) {
                            *s = *v;
                        }
                    }
                    self.axes_m = Some(m);
                }
            }
        }
        // the definitions: kept ones move over from the old table
        let mut defs: OMap<String, Def> = OMap::new();
        let mut old = std::mem::take(&mut self.defs);
        for (id, d) in fresh.iter_mut() {
            let d = match d.take() {
                Some(d) => d,
                None => match old.delete(id.as_str()) {
                    Some(d) => d,
                    None => continue,
                },
            };
            defs.set(id.clone(), d);
        }
        self.defs = defs;
        // an undo / redo frees the definitions it replaced later (a few kept at most)
        if restoring && !old.is_empty() {
            if self.trash.0.len() >= 4 {
                self.trash.0.clear();
            }
            self.trash.0.push(old);
        }
        self.guides = guides;
        self.lights = lights;
        self.dims = dims;
        self.scenes = scenes;
        self.tags = Vec::new();
        for g in j.get("tags").and_then(Value::as_array).map_or(&[][..], |a| a.get(..1000).unwrap_or(a)) {
            let nm = json::clean(g.get("name"), 80);
            if !nm.is_empty() && self.tag(&nm).is_none() {
                self.tags.push(Tag { name: nm, hidden: json::is_true(g.get("hidden")) });
            }
        }
        self.ensure_tags();
        self.units = json::str(j.get("units")).filter(|u| UNIT_KEYS.contains(u)).unwrap_or("ft-in").to_string();
        self.next_id = js::max(json::int(j.get("nextId")).unwrap_or(1.0), max_id + 1.0);
        if !keep_history {
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.dirty = false;
            self.set_base(None);
            self.uid = crate::model::new_uid();
        }
        // given definitions are new objects or untouched ones
        if restoring {
            self.bump_safe();
        } else {
            self.bump_version();
        }
        Ok(())
    }
}

/// The parts of a Snap: a definition's JSON string.
pub type Part = (String, Rc<str>);
