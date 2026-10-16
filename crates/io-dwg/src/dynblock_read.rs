//! Dynamic blocks (port of lib/dwg/dynblock-read.js, 0.5 DB.2): what a definition's parameters are and what each
//! reference is set to. Not in the ODA spec: every layout here is a `Spec gap:` found by experiment on AutoCAD's own
//! files (docs/DWG-DYNAMIC-BLOCKS.md).
//!
//! `read_dynamic(model)` runs after the model is built and works on the model only — the objects kept raw
//! (passthrough `raw`), the dictionaries, XRECORDs, extension dictionaries and xdata. It adds, validated (file data is
//! untrusted: counts, sizes and handles checked):
//!
//!   block.dynamic = { graph, params: [{ node, kind, handle, name, label?, description?, point?, grip?, all?,
//!                     states? [{ name, entities, elements }] }], actions: [{ node, kind, handle, name }] }
//!   block.dynamicOf = definition block record (handle)                     on a reference's copy
//!   insert.dyn = { def, values: [{ node, kind, xrecord, data, …decoded }], state? }   on a reference
//!
//! Where the JS throws outside its own `try` (a TypeError on malformed model values), this returns `Err` at the same
//! point, keeping the changes made before it (read.rs ignores the error as read.js does).

use std::collections::{HashMap, HashSet};

use crate::bits::{BitReader, Codepage, Res, err};
use crate::obj;
use crate::objects::string_stream_start;
use crate::util::{to_u32, to_usize, unb64};
use crate::value::{Obj, V};

const LIMIT_NODES: i32 = 10000;
const LIMIT_LIST: i32 = 100_000;
const LIMIT_STATES: i32 = 1000;
const LIMIT_NAME: usize = 255;

/// JS `String(v)` for the values a handle can be.
fn js_str(v: &V) -> String {
    match v {
        V::Obj(_) => "[object Object]".into(),
        V::Arr(a) => a
            .iter()
            .map(|x| match x {
                V::Null => String::new(),
                o => js_str(o),
            })
            .collect::<Vec<_>>()
            .join(","),
        V::Bytes(_) => String::new(),
        o => crate::read::js_string(o),
    }
}

/// `hexKey(h)`: '' for undefined/null, else upper-case without leading zeros (one character kept).
fn hex_key(v: Option<&V>) -> String {
    let s = match v {
        None | Some(V::Null) => return String::new(),
        Some(x) => js_str(x).to_uppercase(),
    };
    // /^0+(?=.)/ — `.` does not match a line terminator
    let zeros = s.chars().take_while(|&c| c == '0').count();
    if zeros == 0 {
        return s;
    }
    let next = s.chars().nth(zeros);
    let strip = match next {
        Some(c) if !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') => zeros,
        _ => zeros.saturating_sub(1),
    };
    s.chars().skip(strip).collect()
}

/// `cleanName(s)`: control characters removed, at most 255 UTF-16 units.
fn clean_name(s: &str) -> String {
    let mut out = String::new();
    let mut units = 0usize;
    for c in s.chars().filter(|&c| u32::from(c) >= 0x20) {
        let n = c.len_utf16();
        if units.saturating_add(n) > LIMIT_NAME {
            // JS slice would keep a lone high surrogate here; a Rust string cannot hold one (dropped)
            break;
        }
        units = units.saturating_add(n);
        out.push(c);
    }
    out
}

fn count(n: i32, max: i32) -> Res<i32> {
    if !(0..=max).contains(&n) {
        return err(format!("count {n}"));
    }
    Ok(n)
}

/// A reader on a raw object's data bits; R2007+ strings come from the string stream at the end (ODA spec 20.1).
fn with_reader<T>(raw: &Obj, f: impl FnOnce(&mut BitReader) -> Res<T>) -> Res<T> {
    let buf = unb64(raw.str("data").unwrap_or(""));
    let ver = match raw.get("ver") {
        Some(V::Num(v)) if *v != 0.0 && !v.is_nan() => to_u32(*v),
        _ => 15,
    };
    let n = match raw.get("n") {
        Some(V::Num(v)) => to_usize(*v),
        _ => buf.len().saturating_mul(8),
    };
    let mut r = BitReader::range(&buf, ver, Codepage::latin1(), 0, n);
    if ver >= 21 {
        match string_stream_start(&buf, n, 0) {
            Some((s, e)) => {
                r.set_str(s, e);
                r.end = s;
            }
            None => {
                r.set_str(n, n);
                r.end = n.saturating_sub(1);
            }
        }
    }
    f(&mut r)
}

fn raw_of(o: &Obj) -> Option<&Obj> {
    o.obj("raw")
}

/// Spec gap: the element header every dynamic-block object starts with — BL −1, BL release (27 = the AutoCAD 2007
/// format), BL maintenance, BL a 32-bit value (meaning unknown), and its name as the first string.
fn header(r: &mut BitReader) -> Res<String> {
    r.bl()?;
    r.bl()?;
    r.bl()?;
    r.bl()?;
    Ok(clean_name(&r.tv()?))
}

/// Spec gap: an element's name (the first string) without decoding the rest.
fn element_name(o: &Obj) -> String {
    match raw_of(o) {
        Some(raw) => with_reader(raw, header).unwrap_or_default(),
        None => String::new(),
    }
}

/// Spec gap: ACAD_EVALUATION_GRAPH — BL, BL, BL node count, then per node BL index into the graph's handle list, BL
/// flags, BL node id, BL ×4 edge indices; the edges follow (not read). Same bits in AutoCAD 2000–2018.
/// → [(node, handle)]
fn graph_nodes(o: &Obj) -> Res<Vec<(i32, String)>> {
    let Some(raw) = raw_of(o) else { return Ok(Vec::new()) };
    let empty = Vec::new();
    let refs = raw.arr("h").unwrap_or(&empty);
    with_reader(raw, |r| {
        let mut out = Vec::new();
        r.bl()?;
        r.bl()?;
        let n = count(r.bl()?, LIMIT_NODES)?;
        for _ in 0..n {
            let idx = r.bl()?;
            r.bl()?;
            let node = r.bl()?;
            r.bl()?;
            r.bl()?;
            r.bl()?;
            r.bl()?;
            let h = usize::try_from(idx).ok().and_then(|i| refs.get(i)).filter(|e| e.truthy()).and_then(|e| match e {
                V::Arr(a) => a.get(1),
                _ => None,
            });
            if let Some(h) = h
                && node >= 0
                && h.truthy()
            {
                out.push((node, hex_key(Some(h))));
            }
        }
        Ok(out)
    })
}

/// `([, x]) => …` destructuring: arrays give their second item; strings their second character; anything else is a
/// TypeError in JS.
fn second(v: &V) -> Res<Option<V>> {
    match v {
        V::Arr(a) => Ok(a.get(1).cloned()),
        V::Str(s) => Ok(s.chars().nth(1).map(|c| V::Str(c.to_string()))),
        _ => err("not iterable"),
    }
}
/// `([c]) => c === k` destructuring (a string's first character is never a number).
fn code_is(v: &V, k: f64) -> Res<bool> {
    match v {
        V::Arr(a) => Ok(matches!(a.first(), Some(V::Num(c)) if *c == k)),
        V::Str(_) => Ok(false),
        _ => err("not iterable"),
    }
}

/// Spec gap: BLOCKVISIBILITYPARAMETER — header · B B (show properties, chain actions) · BS ×4 · BD3 grip point · BS BS ·
/// BL node id of its grip · TV label · TV description · B B · BL n + n handles (every entity of the block) · BL states,
/// each: TV name · BL a + a handles (entities shown) · BL b + b handles (parameters and grips shown). Exact (no bit,
/// handle or string left) on every visibility parameter of the samples.
fn visibility_of(o: &Obj) -> Res<Obj> {
    let Some(raw) = raw_of(o) else { return err("raw") };
    let mut refs: Vec<String> = Vec::new();
    if let Some(h) = raw.get("h").filter(|h| h.truthy()) {
        match h {
            V::Arr(a) => {
                for x in a {
                    refs.push(hex_key(second(x)?.as_ref()));
                }
            }
            _ => return err("not an array"),
        }
    }
    with_reader(raw, |r| {
        let mut k = 0usize;
        let mut take = |n: i32| -> Res<Vec<V>> {
            let n = usize::try_from(n).unwrap_or(0);
            let e = k.saturating_add(n);
            let Some(l) = refs.get(k..e) else { return err("handles") };
            k = e;
            Ok(l.iter().map(|s| V::Str(s.clone())).collect())
        };
        let name = header(r)?;
        r.b()?;
        r.b()?;
        r.bs()?;
        r.bs()?;
        r.bs()?;
        r.bs()?;
        let point = r.bd3_raw()?;
        r.bs()?;
        r.bs()?;
        let grip = r.bl()?;
        let label = clean_name(&r.tv()?);
        let description = clean_name(&r.tv()?);
        r.b()?;
        r.b()?;
        let all = take(count(r.bl()?, LIMIT_LIST)?)?;
        let mut states: Vec<V> = Vec::new();
        let ns = count(r.bl()?, LIMIT_STATES)?;
        for _ in 0..ns {
            let sname = clean_name(&r.tv()?);
            let entities = take(count(r.bl()?, LIMIT_LIST)?)?;
            let elements = take(count(r.bl()?, LIMIT_LIST)?)?;
            states.push(obj! { "name" => sname, "entities" => entities, "elements" => elements }.into());
        }
        if r.bits_left() != 0 || k != refs.len() {
            return err("layout");
        }
        if !point.iter().all(|v| v.is_finite()) {
            return err("point");
        }
        let [x, y, z] = point;
        Ok(obj! {
            "name" => name, "label" => label, "description" => description, "point" => crate::value::p3(x, y, z),
            "grip" => grip, "all" => all, "states" => states,
        })
    })
}

fn finite_num(o: &Obj, k: &str) -> Option<f64> {
    match o.get(k) {
        Some(V::Num(n)) if n.is_finite() => Some(*n),
        _ => None,
    }
}

/// A reference's values (an XRECORD's groups) for a parameter kind → the decoded fields. Unknown shapes stay raw.
fn value_of(kind: &str, data: Option<&V>) -> Res<Obj> {
    let empty = Vec::new();
    let list = match data {
        Some(V::Arr(a)) => a,
        _ => &empty,
    };
    // later AutoCAD writes a header (1071, 1071, 70, 70) before the values (docs/DWG-DYNAMIC-BLOCKS.md)
    let first_code = |i: usize| -> Res<Option<f64>> {
        match list.get(i) {
            Some(V::Null) | None => err("null"),
            Some(V::Arr(a)) => Ok(match a.first() {
                Some(V::Num(c)) => Some(*c),
                _ => None,
            }),
            Some(_) => Ok(None),
        }
    };
    let hdr = list.len() >= 4
        && first_code(0)? == Some(1071.0)
        && first_code(1)? == Some(1071.0)
        && first_code(2)? == Some(70.0)
        && first_code(3)? == Some(70.0);
    let v: &[V] = if hdr { list.get(4..).unwrap_or_default() } else { list };
    let mut pts: Vec<(Obj, f64, f64)> = Vec::new();
    for item in v {
        if code_is(item, 10.0)?
            && let Some(V::Obj(p)) = second(item)?
            && let (Some(x), Some(y)) = (finite_num(&p, "x"), finite_num(&p, "y"))
        {
            pts.push((p, x, y));
        }
    }
    let find = |k: f64| -> Res<Option<&V>> {
        for item in v {
            if code_is(item, k)? {
                return Ok(Some(item));
            }
        }
        Ok(None)
    };
    let mut out = Obj::new();
    match kind {
        "visibility" => {
            if let Some(s) = find(1.0)?
                && let Some(V::Str(st)) = second(s)?
            {
                out.set("state", clean_name(&st));
            }
            if let Some((p, _, _)) = pts.first() {
                out.set("point", p.clone());
            }
        }
        "linear" | "alignment" => {
            if let (Some((p1, x1, y1)), Some((p2, x2, y2))) = (pts.first(), pts.get(1)) {
                out.set("p1", p1.clone());
                out.set("p2", p2.clone());
                if kind == "linear" {
                    out.set("distance", (x2 - x1).hypot(y2 - y1));
                } else {
                    out.set("angle", (y2 - y1).atan2(x2 - x1));
                }
            }
        }
        "point" | "basepoint" => {
            if let Some((p, _, _)) = pts.first() {
                out.set("point", p.clone());
            }
        }
        "flip" => {
            if let (Some((p1, _, _)), Some((p2, _, _))) = (pts.first(), pts.get(1)) {
                out.set("p1", p1.clone());
                out.set("p2", p2.clone());
            }
            if let Some(f) = find(70.0)? {
                let flipped = matches!(second(f)?, Some(V::Num(n)) if n == 1.0);
                out.set("flipped", flipped);
            }
        }
        _ => {}
    }
    Ok(out)
}

/// JS `a === b` for model values (absent = undefined).
fn js_eq(a: Option<&V>, b: Option<&V>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(V::Num(x)), Some(V::Num(y))) => x == y,
        (Some(V::Str(x)), Some(V::Str(y))) => x == y,
        (Some(V::Bool(x)), Some(V::Bool(y))) => x == y,
        (Some(V::Null), Some(V::Null)) => true,
        _ => false,
    }
}

/// The visibility state a block shows: its entities line up with the definition's (a copy holds them all, in order, the
/// hidden ones flagged invisible); the state whose list is exactly the shown ones. → name | null
fn state_shown(vis: &Obj, def: Option<&Obj>, blk: Option<&Obj>) -> Res<Option<V>> {
    let (Some(blk), Some(def)) = (blk, def) else { return Ok(None) };
    let len = |o: &Obj| match o.get("entities").filter(|e| e.truthy()) {
        Some(V::Arr(a)) => a.len(),
        _ => 0,
    };
    if len(blk) != len(def) {
        return Ok(None);
    }
    let Some(be) = blk.arr("entities") else { return err("entities") };
    let de = def.arr("entities");
    let mut shown: HashSet<String> = HashSet::new();
    for (i, x) in be.iter().enumerate() {
        let inv = match x {
            V::Obj(o) => o.truthy("invisible"),
            V::Null => return err("null"),
            _ => false,
        };
        if !inv {
            let h = de.and_then(|d| d.get(i)).and_then(V::as_obj).and_then(|o| o.get("handle"));
            shown.insert(hex_key(h));
        }
    }
    let empty = Vec::new();
    for s in vis.arr("states").unwrap_or(&empty) {
        let Some(s) = s.as_obj() else { continue };
        let ents = s.arr("entities").unwrap_or(&empty);
        if ents.len() == shown.len() && ents.iter().all(|h| shown.contains(&hex_key(Some(h)))) {
            return Ok(s.get("name").cloned());
        }
    }
    Ok(None)
}

/// JS `Number(key)` for dictionary keys.
fn js_number(key: &str) -> f64 {
    let t = key.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).map(|n| crate::util::i64_to_f64(i64::try_from(n).unwrap_or(i64::MAX))).unwrap_or(f64::NAN);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Object.entries(o)` order: array-index keys ascending first, then the others in insertion order.
fn js_entries(o: &Obj) -> Vec<(&str, &V)> {
    let index = |k: &str| -> Option<u32> {
        if k == "0" {
            return Some(0);
        }
        if k.starts_with('0') || k.is_empty() || !k.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        k.parse::<u32>().ok().filter(|&n| n != u32::MAX)
    };
    let mut ints: Vec<(u32, &str, &V)> = Vec::new();
    let mut rest: Vec<(&str, &V)> = Vec::new();
    for (k, v) in o.iter() {
        match index(k) {
            Some(n) => ints.push((n, k, v)),
            None => rest.push((k, v)),
        }
    }
    ints.sort_by_key(|&(n, _, _)| n);
    ints.into_iter().map(|(_, k, v)| (k, v)).chain(rest).collect()
}

fn is_param(t: &str) -> bool {
    t.starts_with("BLOCK") && t.ends_with("PARAMETER") && t.len() >= 14 && !t.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}
fn is_action(t: &str) -> bool {
    t.starts_with("BLOCK") && t.ends_with("ACTION") && t.len() >= 11 && !t.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}
fn param_kind(t: &str) -> &'static str {
    match t {
        "BLOCKVISIBILITYPARAMETER" => "visibility",
        "BLOCKLINEARPARAMETER" => "linear",
        "BLOCKPOLARPARAMETER" => "polar",
        "BLOCKXYPARAMETER" => "xy",
        "BLOCKROTATIONPARAMETER" => "rotation",
        "BLOCKALIGNMENTPARAMETER" => "alignment",
        "BLOCKFLIPPARAMETER" => "flip",
        "BLOCKPOINTPARAMETER" => "point",
        "BLOCKBASEPOINTPARAMETER" => "basepoint",
        "BLOCKLOOKUPPARAMETER" => "lookup",
        _ => "other",
    }
}
fn action_kind(t: &str) -> &'static str {
    match t {
        "BLOCKMOVEACTION" => "move",
        "BLOCKSTRETCHACTION" => "stretch",
        "BLOCKSCALEACTION" => "scale",
        "BLOCKROTATEACTION" => "rotate",
        "BLOCKFLIPACTION" => "flip",
        "BLOCKARRAYACTION" => "array",
        "BLOCKPOLARSTRETCHACTION" => "polarstretch",
        "BLOCKLOOKUPACTION" => "lookup",
        _ => "other",
    }
}

/// Add block.dynamic, block.dynamicOf and insert.dyn to a model (see the header).
pub fn read_dynamic(model: &mut Obj) -> Res<()> {
    let mut blocks = match model.get_mut("blocks") {
        Some(slot @ V::Arr(_)) => std::mem::take(slot),
        _ => V::Arr(Vec::new()),
    };
    let res = match &mut blocks {
        V::Arr(list) => dynamic_pass(model, list),
        _ => Ok((0, 0, 0)),
    };
    if let Some(slot) = model.get_mut("blocks")
        && matches!(slot, V::Null)
    {
        *slot = blocks;
    }
    let (n, failed, refs) = res?;
    if (n != 0 || failed != 0)
        && let Some(rep) = model.obj_mut("report")
    {
        rep.set("dynamicBlocks", n);
        rep.set("dynamicReferences", refs);
        if failed != 0 {
            rep.set("dynamicFailed", failed);
        }
    }
    Ok(())
}

/// One definition found dynamic: its block's index and its parameters.
struct Def {
    block: usize,
    params: Vec<Obj>,
}

fn dynamic_pass(model: &Obj, blocks: &mut [V]) -> Res<(usize, usize, usize)> {
    let empty = Vec::new();
    let mut objs: HashMap<String, &Obj> = HashMap::new();
    for o in model.arr("objects").unwrap_or(&empty) {
        match o {
            V::Obj(o) => {
                objs.insert(hex_key(o.get("handle")), o);
            }
            V::Null => return err("null object"),
            _ => {
                objs.insert(String::new(), &EMPTY);
            }
        }
    }
    let dict = |h: Option<&V>| -> Option<&Obj> { objs.get(&hex_key(h)).and_then(|o| o.obj("entries")) };
    let mut recs: HashMap<String, &Obj> = HashMap::new();
    if let Some(list) = model.obj("tables").and_then(|t| t.arr("blockRecords")) {
        for r in list {
            match r {
                V::Obj(r) => {
                    recs.insert(hex_key(r.get("handle")), r);
                }
                _ => return err("block record"),
            }
        }
    }
    let mut defs: HashMap<String, Def> = HashMap::new();
    let (mut n, mut failed) = (0usize, 0usize);
    for (bi, bv) in blocks.iter_mut().enumerate() {
        let V::Obj(b) = bv else { return err("block") };
        let Some(rr) = recs.get(&hex_key(b.get("record"))) else { continue };
        if let Some(V::Arr(btag)) = rr.obj("xdata").and_then(|x| x.get("AcDbBlockRepBTag")) {
            let mut tag = None;
            for t in btag {
                if code_is(t, 1005.0)? {
                    tag = Some(t);
                    break;
                }
            }
            if let Some(t) = tag {
                let k = hex_key(second(t)?.as_ref());
                if recs.contains_key(&k) {
                    b.set("dynamicOf", k);
                }
            }
        }
        let g = if rr.truthy("extDict") {
            let e = dict(rr.get("extDict")).and_then(|d| d.get("ACAD_ENHANCEDBLOCK"));
            objs.get(&hex_key(e)).copied()
        } else {
            None
        };
        let Some(g) = g else { continue };
        if g.str("type") != Some("ACAD_EVALUATION_GRAPH") || !g.truthy("raw") {
            continue;
        }
        let built = (|| -> Res<(Vec<Obj>, Vec<V>)> {
            let mut params: Vec<Obj> = Vec::new();
            let mut actions: Vec<V> = Vec::new();
            for (node, handle) in graph_nodes(g)? {
                let Some(o) = objs.get(&handle) else { continue };
                let t = o.str("type").unwrap_or("undefined");
                if is_param(t) {
                    let kind = param_kind(t);
                    let mut p = obj! { "node" => node, "kind" => kind, "handle" => handle.clone(), "name" => element_name(o) };
                    if kind == "visibility" {
                        match visibility_of(o) {
                            Ok(v) => p.assign(v),
                            Err(_) => p.set("undecoded", true),
                        }
                    }
                    params.push(p);
                } else if is_action(t) {
                    actions.push(
                        obj! { "node" => node, "kind" => action_kind(t), "handle" => handle.clone(), "name" => element_name(o) }.into(),
                    );
                }
            }
            Ok((params, actions))
        })();
        match built {
            Ok((params, actions)) => {
                let pv: Vec<V> = params.iter().cloned().map(V::Obj).collect();
                b.set("dynamic", obj! { "graph" => hex_key(g.get("handle")), "params" => pv, "actions" => actions });
                defs.insert(hex_key(b.get("record")), Def { block: bi, params });
                n = n.saturating_add(1);
            }
            Err(_) => failed = failed.saturating_add(1),
        }
    }
    // references: INSERT xdict → AcDbBlockRepresentation → AcDbRepData (→ definition) + AppDataCache → ACAD_ENHANCEDBLOCKDATA
    let mut updates: Vec<(usize, usize, Obj)> = Vec::new();
    let res = references(blocks, &objs, &defs, &mut updates);
    let refs = updates.len();
    for (bi, ei, d) in updates {
        if let Some(V::Obj(b)) = blocks.get_mut(bi)
            && let Some(V::Arr(es)) = b.get_mut("entities")
            && let Some(V::Obj(e)) = es.get_mut(ei)
        {
            e.set("dyn", d);
        }
    }
    res?;
    Ok((n, failed, refs))
}

static EMPTY: Obj = Obj(Vec::new());

fn references(blocks: &[V], objs: &HashMap<String, &Obj>, defs: &HashMap<String, Def>, updates: &mut Vec<(usize, usize, Obj)>) -> Res<()> {
    let dict = |h: Option<&V>| -> Option<&Obj> { objs.get(&hex_key(h)).and_then(|o| o.obj("entries")) };
    let by_name = |name: Option<&V>| -> Option<&Obj> { blocks.iter().find_map(|x| x.as_obj().filter(|x| js_eq(x.get("name"), name))) };
    for (bi, bv) in blocks.iter().enumerate() {
        let Some(b) = bv.as_obj() else { continue };
        let Some(ents) = b.get("entities").filter(|e| e.truthy()) else { continue };
        let V::Arr(ents) = ents else { return err("entities") };
        for (ei, ev) in ents.iter().enumerate() {
            let Some(e) = ev.as_obj() else {
                if matches!(ev, V::Null) {
                    return err("null entity");
                }
                continue;
            };
            if e.str("type") != Some("INSERT") {
                continue;
            }
            let rep = if e.truthy("extDict") { dict(dict(e.get("extDict")).and_then(|d| d.get("AcDbBlockRepresentation"))) } else { None };
            let rget = |k: &str| rep.and_then(|r| r.get(k));
            let rd = objs.get(&hex_key(rget("AcDbRepData")));
            let mut def = String::new();
            if let Some(raw) = rd.and_then(|o| o.get("raw")).filter(|r| r.truthy())
                && let Some(h) = raw.as_obj().and_then(|r| r.get("h")).filter(|h| h.truthy())
                && let Some(h0) = h.as_arr().and_then(|a| a.first()).filter(|x| x.truthy())
            {
                def = hex_key(h0.as_arr().and_then(|a| a.get(1)));
            }
            if !defs.contains_key(&def) {
                // a reference straight to the definition (no representation data)
                let own = by_name(e.get("block"));
                def = match own {
                    Some(o) if defs.contains_key(&hex_key(o.get("record"))) => hex_key(o.get("record")),
                    _ => String::new(),
                };
            }
            let Some(d) = defs.get(&def) else { continue };
            let mut values: Vec<Obj> = Vec::new();
            let cache = dict(rget("AppDataCache").and_then(|x| dict(Some(x))).and_then(|c| c.get("ACAD_ENHANCEDBLOCKDATA")));
            if let Some(cache) = cache {
                for (key, xh) in js_entries(cache) {
                    let node = js_number(key);
                    let Some(x) = objs.get(&hex_key(Some(xh))) else { continue };
                    if !(node.is_finite() && node.trunc() == node) || x.str("type") != Some("XRECORD") {
                        continue;
                    }
                    // grips and actions keep values too: left as they are in the file
                    let Some(p) = d.params.iter().rev().find(|p| p.num("node") == Some(node)) else { continue };
                    let kind = p.str("kind").unwrap_or("");
                    let data = x.get("data").filter(|v| v.truthy()).cloned().unwrap_or(V::Arr(Vec::new()));
                    let mut v = obj! { "node" => node, "kind" => kind, "xrecord" => hex_key(Some(xh)), "data" => data };
                    v.assign(value_of(kind, x.get("data"))?);
                    values.push(v);
                }
            }
            let vis = d.params.iter().find(|p| p.str("kind") == Some("visibility") && p.truthy("states"));
            let vv = vis.and_then(|vis| values.iter().find(|v| v.get("node") == vis.get("node") && vis.get("node").is_some()));
            let vstate = vv.and_then(|v| v.get("state")).cloned();
            let shown = match (vis, &vstate) {
                (Some(vis), None) => state_shown(vis, blocks.get(d.block).and_then(V::as_obj), by_name(e.get("block")))?,
                _ => None,
            };
            let mut dy = obj! { "def" => def.clone(), "values" => values.into_iter().map(V::Obj).collect::<Vec<V>>() };
            if let Some(s) = vstate {
                dy.set("state", s);
            } else if let Some(s) = shown.filter(V::truthy) {
                dy.set("state", s);
            }
            updates.push((bi, ei, dy));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_keys() {
        assert_eq!(hex_key(Some(&V::Str("00ab".into()))), "AB");
        assert_eq!(hex_key(Some(&V::Str("000".into()))), "0");
        assert_eq!(hex_key(None), "");
        assert_eq!(hex_key(Some(&V::Num(10.0))), "10");
    }
    #[test]
    fn names_and_numbers() {
        assert_eq!(clean_name("a\u{1}b\tc"), "abc");
        assert_eq!(clean_name(&"x".repeat(300)).len(), 255);
        assert_eq!(js_number(" 12 "), 12.0);
        assert_eq!(js_number(""), 0.0);
        assert!(js_number("ab").is_nan());
        let o = obj! { "b" => 1, "10" => 2, "2" => 3, "01" => 4 };
        let k: Vec<&str> = js_entries(&o).into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, vec!["2", "10", "b", "01"]);
    }
}
