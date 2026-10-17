//! marks.js — part marks (the same piece of steel, in any way it can be turned over that maps its section onto
//! itself, within a tolerance), assembly marks (a main part plus the plates welded to it, at their places), the bill
//! of materials tables, bolt lists and footing marks; numbered per phase when asked.

use crate::build::{Built, Part};
use crate::geom::{Local, to_local};
use crate::i18n::t0;
use crate::js::{Obj, fixed_num, locale_compare, locale_compare_num, num_to_string, or, round, stringify, to_fixed, upper, utf16_cmp};
use crate::state::Cut;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;

fn part_prefix(kind: &str) -> &str {
    match kind {
        "beam" => "b",
        "column" => "c",
        "brace" => "br",
        "stringer" => "st",
        "plate" => "p",
        _ => "undefined",
    }
}
fn asm_prefix(kind: &str) -> &str {
    match kind {
        "beam" => "B",
        "column" => "C",
        "brace" => "BR",
        "stringer" => "ST",
        "plate" => "P",
        _ => "undefined",
    }
}
fn type_order(kind: &str) -> f64 {
    match kind {
        "column" => 0.0,
        "beam" => 1.0,
        "brace" => 2.0,
        "stringer" => 3.0,
        "plate" => 4.0,
        _ => f64::NAN,
    }
}

fn variants_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "C" => &["id", "ru"],
        "L" | "CHS" => &["id"],
        _ => &["id", "rv", "ru", "rx"],
    }
}
fn flips3(v: &str) -> Option<[bool; 3]> {
    match v {
        "rv" => Some([true, true, false]),
        "ru" => Some([true, false, true]),
        "rx" => Some([false, true, true]),
        _ => None,
    }
}

/// `r(v, tol)`.
pub fn rq(v: f64, tol: f64) -> f64 {
    let n = round(v / tol) * tol;
    fixed_num(if n == 0.0 { 0.0 } else { n }, 3)
}

/// A signature item: text or number, compared as the JS `<` / `>` do.
#[derive(Clone, Debug)]
enum It {
    S(String),
    N(f64),
}
fn it_json(x: &It) -> Value {
    match x {
        It::S(s) => Value::from(s.clone()),
        It::N(n) => Value::from(*n),
    }
}
/// `cmpArr(a, b)`.
fn cmp_arr(a: &[It], b: &[It]) -> Ordering {
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i), b.get(i));
        let lt = match (x, y) {
            (Some(It::S(p)), Some(It::S(q))) => utf16_cmp(p, q),
            (Some(It::N(p)), Some(It::N(q))) => {
                if p < q {
                    Ordering::Less
                } else if p > q {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            // undefined < x and x < undefined are both false
            _ => Ordering::Equal,
        };
        if lt != Ordering::Equal {
            return lt;
        }
    }
    Ordering::Equal
}
fn arr_json(rows: &[Vec<It>]) -> Value {
    Value::Array(rows.iter().map(|r| Value::Array(r.iter().map(it_json).collect())).collect())
}

/// A hole after turning the part over by variant v.
fn turn_hole(face: &str, x: f64, y: f64, v: &str, l: f64) -> (String, f64, f64) {
    let Some([rev, fu, fv]) = flips3(v) else { return (face.to_string(), x, y) };
    let (mut face, mut x, mut y) = (face.to_string(), x, y);
    if rev {
        x = l - x;
    }
    if face == "web" {
        if fv {
            y = -y;
        }
    } else if face == "plate" {
        if fu {
            y = -y;
        }
    } else {
        if fu {
            y = -y;
        }
        if fv {
            face = if face == "top" { "bottom".into() } else { "top".into() };
        }
    }
    (face, x, y)
}
fn turn_cut(c: &Cut, v: &str) -> (String, String) {
    let flips = match v {
        "rv" => Some([true, false]),
        "ru" => Some([true, true]),
        "rx" => Some([false, true]),
        _ => None,
    };
    let Some([fe, fs]) = flips else { return (c.end.clone(), c.side.clone()) };
    let end = if fe { if c.end == "start" { "end".into() } else { "start".into() } } else { c.end.clone() };
    let side = if fs { if c.side == "top" { "bottom".into() } else { "top".into() } } else { c.side.clone() };
    (end, side)
}
fn turn_point(p: Local, v: &str, l: f64) -> Local {
    let Some([a, b, c]) = flips3(v) else { return p };
    Local { x: if a { l - p.x } else { p.x }, u: if b { -p.u } else { p.u }, v: if c { -p.v } else { p.v } }
}

/// The part's own signature in variant v.
fn part_sig(p: &Part, v: &str, tol: f64) -> String {
    let l = p.length;
    let mut holes: Vec<Vec<It>> = p
        .holes
        .iter()
        .map(|h| {
            let (face, x, y) = turn_hole(&h.face, h.x, h.y, v, l);
            vec![It::S(face), It::N(rq(x, tol)), It::N(rq(y, tol)), It::N(rq(h.d, 0.1))]
        })
        .collect();
    holes.sort_by(|a, b| cmp_arr(a, b));
    let mut cuts: Vec<Vec<It>> = p
        .cuts
        .iter()
        .map(|c| {
            let (end, side) = turn_cut(c, v);
            vec![It::S(c.kind.clone()), It::S(end), It::S(side), It::N(rq(c.length, tol)), It::N(rq(c.depth, tol))]
        })
        .collect();
    cuts.sort_by(|a, b| cmp_arr(a, b));
    let kind = if p.kind == "plate" { "plate".to_string() } else { p.kind.clone() };
    stringify(&Value::Array(vec![
        Value::from(kind),
        Value::from(upper(&p.profile_name)),
        Value::from(upper(&p.material)),
        Value::from(rq(l, tol)),
        arr_json(&holes),
        arr_json(&cuts),
    ]))
}

/// A part-mark row.
#[derive(Clone, Debug)]
pub struct PartRow {
    pub mark: String,
    pub kind: String,
    pub profile: String,
    pub material: String,
    pub length: f64,
    pub count: usize,
    pub unit_weight: f64,
    pub ids: Vec<String>,
    pub phase: f64,
}
/// An assembly-mark row.
#[derive(Clone, Debug)]
pub struct AsmRow {
    pub mark: String,
    pub main: String,
    pub count: usize,
    pub weight: f64,
    pub ids: Vec<String>,
    pub members: Vec<String>,
    pub phase: f64,
}
/// `markParts(built, opts)`.
#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub part_mark: Vec<(String, String)>,
    pub asm_mark: Vec<(String, String)>,
    pub asm_of: Vec<(String, String)>,
    pub parts: Vec<PartRow>,
    pub assemblies: Vec<AsmRow>,
    pub phased: bool,
    pm: HashMap<String, usize>,
    am: HashMap<String, usize>,
    ao: HashMap<String, usize>,
}
fn map_set(list: &mut Vec<(String, String)>, idx: &mut HashMap<String, usize>, k: &str, v: String) {
    match idx.get(k) {
        Some(&i) => {
            if let Some(slot) = list.get_mut(i) {
                slot.1 = v;
            }
        }
        None => {
            idx.insert(k.to_string(), list.len());
            list.push((k.to_string(), v));
        }
    }
}
impl Marks {
    pub fn part_mark(&self, id: &str) -> Option<&str> {
        self.pm.get(id).and_then(|&i| self.part_mark.get(i)).map(|x| x.1.as_str())
    }
    pub fn asm_mark(&self, id: &str) -> Option<&str> {
        self.am.get(id).and_then(|&i| self.asm_mark.get(i)).map(|x| x.1.as_str())
    }
    pub fn asm_of(&self, id: &str) -> Option<&str> {
        self.ao.get(id).and_then(|&i| self.asm_of.get(i)).map(|x| x.1.as_str())
    }
}

struct Grp<'a> {
    sig: String,
    first: &'a Part,
    ids: Vec<String>,
    mark: String,
}

pub fn mark_parts(built: &Built, tol: f64, per_phase: bool) -> Marks {
    let parts = &built.parts;
    let phased = per_phase && {
        let mut seen: Vec<f64> = Vec::new();
        for p in parts {
            let ph = or(p.phase, 1.0);
            if !seen.contains(&ph) {
                seen.push(ph);
            }
        }
        seen.len() > 1
    };
    let ph = |p: &Part| if phased { or(p.phase, 1.0) } else { 0.0 };
    let pre0 = |p: &Part| if phased { format!("{}-", num_to_string(or(p.phase, 1.0))) } else { String::new() };
    let mut own_sig: Vec<String> = Vec::with_capacity(parts.len());
    let mut groups: Vec<Grp> = Vec::new();
    let mut gidx: HashMap<String, usize> = HashMap::new();
    for p in parts {
        let mut best: Option<String> = None;
        for v in variants_for(&p.profile.kind) {
            let s = part_sig(p, v, tol);
            if best.as_ref().is_none_or(|b| utf16_cmp(&s, b) == Ordering::Less) {
                best = Some(s);
            }
        }
        let best = best.unwrap_or_default();
        let sig = if phased { format!("{}|{}", num_to_string(or(p.phase, 1.0)), best) } else { best.clone() };
        own_sig.push(best);
        match gidx.get(&sig) {
            Some(&i) => {
                if let Some(g) = groups.get_mut(i) {
                    g.ids.push(p.id.clone());
                }
            }
            None => {
                gidx.insert(sig.clone(), groups.len());
                groups.push(Grp { sig, first: p, ids: vec![p.id.clone()], mark: String::new() });
            }
        }
    }
    groups.sort_by(|a, b| {
        let mut c = ph(a.first) - ph(b.first);
        if c == 0.0 || c.is_nan() {
            c = type_order(&a.first.kind) - type_order(&b.first.kind);
        }
        if c == 0.0 || c.is_nan() {
            c = locale_compare_num(&a.first.profile_name, &b.first.profile_name);
        }
        if c == 0.0 || c.is_nan() {
            c = b.first.length - a.first.length;
        }
        if c == 0.0 || c.is_nan() {
            c = if utf16_cmp(&a.sig, &b.sig) == Ordering::Less { -1.0 } else { 1.0 };
        }
        crate::js::ord(c)
    });
    let mut counters: HashMap<String, f64> = HashMap::new();
    let mut out = Marks { phased, ..Marks::default() };
    for g in &mut groups {
        let pre = format!("{}{}", pre0(g.first), part_prefix(&g.first.kind));
        let n = counters.get(&pre).copied().unwrap_or(0.0) + 1.0;
        counters.insert(pre.clone(), n);
        g.mark = format!("{pre}{}", num_to_string(n));
        for id in &g.ids {
            map_set(&mut out.part_mark, &mut out.pm, id, g.mark.clone());
        }
        out.parts.push(PartRow {
            mark: g.mark.clone(),
            kind: g.first.kind.clone(),
            profile: g.first.profile_name.clone(),
            material: g.first.material.clone(),
            length: g.first.length,
            count: g.ids.len(),
            unit_weight: g.first.weight,
            ids: g.ids.clone(),
            phase: or(g.first.phase, 1.0),
        });
    }

    // assemblies: every part not attached to another is a main part
    let mut attached: Vec<(String, Vec<&Part>)> = Vec::new();
    let mut at_idx: HashMap<String, usize> = HashMap::new();
    for p in parts {
        let main = match &p.attach_to {
            Some(a) if built.by_id.contains_key(a) => a.clone(),
            _ => p.id.clone(),
        };
        map_set(&mut out.asm_of, &mut out.ao, &p.id, main.clone());
        if main != p.id {
            match at_idx.get(&main) {
                Some(&i) => {
                    if let Some(x) = attached.get_mut(i) {
                        x.1.push(p);
                    }
                }
                None => {
                    at_idx.insert(main.clone(), attached.len());
                    attached.push((main, vec![p]));
                }
            }
        }
    }
    let subs_of = |id: &str| -> &[&Part] { at_idx.get(id).and_then(|&i| attached.get(i)).map_or(&[][..], |x| x.1.as_slice()) };
    let mut asm_sig: Vec<(String, String)> = Vec::new();
    let mut as_idx: HashMap<String, usize> = HashMap::new();
    for (pi, p) in parts.iter().enumerate() {
        if out.asm_of(&p.id) != Some(p.id.as_str()) {
            continue;
        }
        let subs = subs_of(&p.id);
        let mut best: Option<String> = None;
        for v in variants_for(&p.profile.kind) {
            let own = part_sig(p, v, tol);
            if Some(&own) != own_sig.get(pi) {
                continue;
            }
            let mut at: Vec<Vec<It>> = subs
                .iter()
                .map(|s| {
                    let mid = [(s.start[0] + s.end[0]) / 2.0, (s.start[1] + s.end[1]) / 2.0, (s.start[2] + s.end[2]) / 2.0];
                    let c = to_local(&p.frame, mid, p.voff);
                    let tp = turn_point(c, v, p.length);
                    let mk = out.part_mark(&s.id).map(|m| It::S(m.to_string()));
                    vec![mk.unwrap_or(It::N(f64::NAN)), It::N(rq(tp.x, tol)), It::N(rq(tp.u, tol)), It::N(rq(tp.v, tol))]
                })
                .collect();
            at.sort_by(|a, b| cmp_arr(a, b));
            let s = stringify(&Value::Array(vec![out.part_mark(&p.id).map_or(Value::Null, Value::from), arr_json(&at)]));
            if best.as_ref().is_none_or(|b| utf16_cmp(&s, b) == Ordering::Less) {
                best = Some(s);
            }
        }
        map_set(&mut asm_sig, &mut as_idx, &p.id, best.unwrap_or_else(|| "null".into()));
    }
    struct AGrp<'a> {
        sig: String,
        ids: Vec<String>,
        main: &'a Part,
    }
    let mut agroups: Vec<AGrp> = Vec::new();
    let mut ag_idx: HashMap<String, usize> = HashMap::new();
    for (id, s) in &asm_sig {
        match ag_idx.get(s) {
            Some(&i) => {
                if let Some(g) = agroups.get_mut(i) {
                    g.ids.push(id.clone());
                }
            }
            None => {
                let Some(main) = built.get(id) else { continue };
                ag_idx.insert(s.clone(), agroups.len());
                agroups.push(AGrp { sig: s.clone(), ids: vec![id.clone()], main });
            }
        }
    }
    let pmk = |id: &str| out.part_mark(id).unwrap_or("").to_string();
    agroups.sort_by(|a, b| {
        let mut c = ph(a.main) - ph(b.main);
        if c == 0.0 || c.is_nan() {
            c = type_order(&a.main.kind) - type_order(&b.main.kind);
        }
        if c == 0.0 || c.is_nan() {
            c = locale_compare_num(&pmk(&a.main.id), &pmk(&b.main.id));
        }
        if c == 0.0 || c.is_nan() {
            c = if utf16_cmp(&a.sig, &b.sig) == Ordering::Less { -1.0 } else { 1.0 };
        }
        crate::js::ord(c)
    });
    let mut acount: HashMap<String, f64> = HashMap::new();
    let mut asm_rows = Vec::new();
    let mut asm_marks: Vec<(String, String)> = Vec::new();
    for g in &agroups {
        let pre = format!("{}{}", pre0(g.main), asm_prefix(&g.main.kind));
        let n = acount.get(&pre).copied().unwrap_or(0.0) + 1.0;
        acount.insert(pre.clone(), n);
        let mark = format!("{pre}{}", num_to_string(n));
        for id in &g.ids {
            asm_marks.push((id.clone(), mark.clone()));
        }
        let subs = subs_of(&g.main.id);
        let mut members = vec![pmk(&g.main.id)];
        members.extend(subs.iter().map(|s| pmk(&s.id)));
        asm_rows.push(AsmRow {
            mark,
            main: pmk(&g.main.id),
            count: g.ids.len(),
            weight: g.main.weight + subs.iter().fold(0.0, |s, x| s + x.weight),
            ids: g.ids.clone(),
            members,
            phase: or(g.main.phase, 1.0),
        });
    }
    for (id, m) in asm_marks {
        map_set(&mut out.asm_mark, &mut out.am, &id, m);
    }
    out.assemblies = asm_rows;
    out
}

/// A bolt-list row.
#[derive(Clone, Debug)]
pub struct BoltRow {
    pub kind: String,
    pub d: f64,
    pub length: f64,
    pub grade: String,
    pub count: usize,
}
/// `boltList(bolts)`.
pub fn bolt_list(bolts: &[crate::connections::Bolt]) -> Vec<BoltRow> {
    let mut rows: Vec<BoltRow> = Vec::new();
    let mut idx: HashMap<String, usize> = HashMap::new();
    for b in bolts {
        let k = format!("{}|{}|{}|{}", b.kind, num_to_string(b.d), num_to_string(b.length), b.grade);
        match idx.get(&k) {
            Some(&i) => {
                if let Some(r) = rows.get_mut(i) {
                    r.count = r.count.saturating_add(1);
                }
            }
            None => {
                idx.insert(k, rows.len());
                rows.push(BoltRow { kind: b.kind.clone(), d: b.d, length: b.length, grade: b.grade.clone(), count: 1 });
            }
        }
    }
    rows.sort_by(|a, b| {
        let mut c = locale_compare(&a.kind, &b.kind);
        if c == 0.0 || c.is_nan() {
            c = a.d - b.d;
        }
        if c == 0.0 || c.is_nan() {
            c = a.length - b.length;
        }
        crate::js::ord(c)
    });
    rows
}

/// `bomTables(marks, bolts)`.
pub struct Bom {
    pub parts: Vec<Vec<String>>,
    pub assemblies: Vec<Vec<String>>,
    pub bolts: Vec<Vec<String>>,
    pub total_weight: f64,
}
pub fn bom_tables(marks: &Marks, bolts: &[crate::connections::Bolt]) -> Bom {
    let kg = |v: f64| to_fixed(v, 1);
    let col = |k: &str| t0(&format!("marks.col.{k}"));
    let mut parts = vec![
        ["part-mark", "type", "profile", "material", "length", "count", "unit-weight", "total-weight"]
            .iter()
            .map(|k| col(k))
            .collect::<Vec<_>>(),
    ];
    for p in &marks.parts {
        parts.push(vec![
            p.mark.clone(),
            p.kind.clone(),
            p.profile.clone(),
            p.material.clone(),
            num_to_string(round(p.length)),
            p.count.to_string(),
            kg(p.unit_weight),
            kg(p.unit_weight * p.count as f64),
        ]);
    }
    let tw = marks.parts.iter().fold(0.0, |s, p| s + p.unit_weight * p.count as f64);
    let tc = marks.parts.iter().fold(0usize, |s, p| s.saturating_add(p.count));
    parts.push(vec![t0("marks.total"), String::new(), String::new(), String::new(), String::new(), tc.to_string(), String::new(), kg(tw)]);
    let mut asm =
        vec![["assembly-mark", "main-part", "parts", "count", "unit-weight", "total-weight"].iter().map(|k| col(k)).collect::<Vec<_>>()];
    for a in &marks.assemblies {
        asm.push(vec![
            a.mark.clone(),
            a.main.clone(),
            a.members.join(" + "),
            a.count.to_string(),
            kg(a.weight),
            kg(a.weight * a.count as f64),
        ]);
    }
    let mut bl = vec![["fastener", "diameter", "length", "grade", "count"].iter().map(|k| col(k)).collect::<Vec<_>>()];
    for b in bolt_list(bolts) {
        bl.push(vec![
            if b.kind == "ANCHORBOLT" { t0("marks.anchor-rod") } else { t0("marks.bolt") },
            num_to_string(b.d),
            num_to_string(b.length),
            b.grade.clone(),
            b.count.to_string(),
        ]);
    }
    Bom { parts, assemblies: asm, bolts: bl, total_weight: tw }
}

/// `markFootings(footings, {perPhase})` (stored footings) → [(id, mark)] in the order marks were given.
pub fn mark_footings(footings: &[Value], per_phase: bool) -> Vec<(String, String)> {
    let phase = |f: &Value| or(crate::js::f(f.get("phase")), 1.0);
    let phased = per_phase && {
        let mut seen: Vec<f64> = Vec::new();
        for f in footings {
            let p = phase(f);
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
        seen.len() > 1
    };
    let mut list: Vec<&Value> = footings.iter().collect();
    if phased {
        list.sort_by(|a, b| crate::js::ord(phase(a) - phase(b)));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut oidx: HashMap<String, usize> = HashMap::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut count: HashMap<String, f64> = HashMap::new();
    for f in list {
        let pre = if phased { format!("{}-", num_to_string(phase(f))) } else { String::new() };
        let g = |k: &str| f.get(k).cloned().unwrap_or(Value::Null);
        let k = format!("{pre}{}", stringify(&Value::Array(vec![g("L"), g("B"), g("H"), g("concrete"), g("mat"), g("starters")])));
        if !seen.contains_key(&k) {
            let n = count.get(&pre).copied().unwrap_or(0.0) + 1.0;
            count.insert(pre.clone(), n);
            seen.insert(k.clone(), format!("{pre}F{}", num_to_string(n)));
        }
        let id = crate::js::string_of(f.get("id"));
        map_set(&mut out, &mut oidx, &id, seen.get(&k).cloned().unwrap_or_default());
    }
    out
}

/// The JSON of marks (Maps as objects, as the parity kit's canon writes them).
pub fn marks_json(m: &Marks) -> Value {
    let map = |l: &[(String, String)]| {
        let mut o = serde_json::Map::new();
        for (k, v) in l {
            o.insert(k.clone(), Value::from(v.clone()));
        }
        Value::Object(o)
    };
    Obj::new()
        .set("partMark", map(&m.part_mark))
        .set("asmMark", map(&m.asm_mark))
        .set("asmOf", map(&m.asm_of))
        .set(
            "parts",
            Value::Array(
                m.parts
                    .iter()
                    .map(|p| {
                        Obj::new()
                            .set("mark", p.mark.as_str())
                            .set("type", p.kind.as_str())
                            .set("profile", p.profile.as_str())
                            .set("material", p.material.as_str())
                            .set("length", p.length)
                            .set("count", p.count)
                            .set("unitWeight", p.unit_weight)
                            .set("ids", p.ids.clone())
                            .set("phase", p.phase)
                            .done()
                    })
                    .collect(),
            ),
        )
        .set(
            "assemblies",
            Value::Array(
                m.assemblies
                    .iter()
                    .map(|a| {
                        Obj::new()
                            .set("mark", a.mark.as_str())
                            .set("main", a.main.as_str())
                            .set("count", a.count)
                            .set("weight", a.weight)
                            .set("ids", a.ids.clone())
                            .set("members", a.members.clone())
                            .set("phase", a.phase)
                            .done()
                    })
                    .collect(),
            ),
        )
        .set("phased", m.phased)
        .done()
}
