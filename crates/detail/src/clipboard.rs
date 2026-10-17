//! clipboard.js — copy and paste: `copyClip` (members whole with their custom profiles, footings, stairs, the
//! connections whose members all come along), `readClip` (the stored clipboard is untrusted: format, version and base
//! checked, lists capped, then everything through `readModel`), `pasteInto` (inside a model change: new ids,
//! connections re-pointed, every point moved), `clipSegments` (the ghost), `overlaps`.

use crate::i18n::{F, t, t0};
use crate::js::{Res, arr, jnum, num, parse, round, s, string_or_empty, stringify, upper};
use crate::model::{Tx, read_model, strict_eq};
use crate::state::members_of_v;
use crate::transform::stair_outline;
use serde_json::{Map, Value, json};

pub const CLIP_FORMAT: &str = "detailcraft-clip";
pub const CLIP_VERSION: f64 = 1.0;
pub const CLIP_KEY: &str = "detailcraft.clipboard";

fn max_of(k: &str) -> usize {
    match k {
        "members" | "connections" => 20000,
        "footings" => 5000,
        "stairs" => 2000,
        _ => 500,
    }
}
fn copy(v: Vec<Value>) -> Value {
    // JSON.parse(JSON.stringify(o)): numbers and keys as the text gives them back
    parse(&stringify(&Value::Array(v))).unwrap_or(Value::Array(vec![]))
}
fn add(p: Option<&Value>, o: [f64; 3]) -> Value {
    let a = arr(p);
    json!([jnum(num(a.first()) + o[0]), jnum(num(a.get(1)) + o[1]), jnum(num(a.get(2)) + o[2])])
}
fn has(ids: &[Value], v: Option<&Value>) -> bool {
    ids.iter().any(|x| strict_eq(Some(x), v))
}

/// `copyClip(state, refs, { connections })` → `{ clip, counts }` or None (nothing to copy). `source` (the time and a
/// random tag in the JS) is the caller's.
pub fn copy_clip(state: &Value, refs: &[Value], connections: bool, source: &str) -> Option<Value> {
    let want = |kind: &str| -> Vec<Value> {
        refs.iter()
            .filter(|r| crate::js::truthy(Some(r)) && r.get("kind").and_then(Value::as_str) == Some(kind))
            .map(|r| r.get("id").cloned().unwrap_or(Value::Null))
            .collect()
    };
    let (m_ids, f_ids, c_ids, s_ids) = (want("member"), want("footing"), want("conn"), want("stair"));
    let members: Vec<Value> = arr(state.get("members")).iter().filter(|m| has(&m_ids, m.get("id"))).cloned().collect();
    let footings: Vec<Value> = arr(state.get("footings")).iter().filter(|f| has(&f_ids, f.get("id"))).cloned().collect();
    let stairs: Vec<Value> = arr(state.get("stairs")).iter().filter(|x| has(&s_ids, x.get("id"))).cloned().collect();
    if members.is_empty() && footings.is_empty() && stairs.is_empty() {
        return None;
    }
    let in_set = |c: &Value| members_of_v(c).iter().all(|id| has(&m_ids, *id));
    let touches = |c: &Value| members_of_v(c).iter().any(|id| has(&m_ids, *id)) || has(&c_ids, c.get("id"));
    let all_conns = arr(state.get("connections"));
    let take: Vec<bool> = all_conns.iter().map(|c| connections && in_set(c)).collect();
    let conns: Vec<Value> = all_conns.iter().zip(&take).filter(|(_, k)| **k).map(|(c, _)| c.clone()).collect();
    let left_out = all_conns.iter().zip(&take).filter(|(c, k)| touches(c) && !**k).count();
    let mut used: Vec<String> = members.iter().map(|m| upper(&crate::js::string_of(m.get("profile")))).collect();
    used.extend(stairs.iter().map(|x| upper(&crate::js::string_of(x.get("stringer")))));
    let profiles: Vec<Value> = arr(state.get("profiles")).iter().filter(|p| used.contains(&upper(s(p.get("name"))))).cloned().collect();
    let mut pts: Vec<[f64; 3]> = Vec::new();
    for m in &members {
        pts.push(crate::js::p3_of(m.get("start")));
        pts.push(crate::js::p3_of(m.get("end")));
    }
    for f in &footings {
        pts.push([num(f.get("x")), num(f.get("y")), num(f.get("top"))]);
    }
    for x in &stairs {
        pts.push([num(x.get("x")), num(x.get("y")), num(x.get("z"))]);
    }
    let base: Vec<Value> = (0..3).map(|k| jnum(crate::js::min_of(pts.iter().map(|p| crate::js::g3(*p, k))))).collect();
    let counts = json!({ "members": members.len(), "connections": conns.len(), "footings": footings.len(), "stairs": stairs.len(), "leftOut": left_out });
    let mut clip = Map::new();
    clip.insert("format".into(), Value::from(CLIP_FORMAT));
    clip.insert("version".into(), Value::from(1));
    clip.insert("source".into(), Value::from(source));
    clip.insert("from".into(), state.get("name").cloned().unwrap_or(Value::Null));
    clip.insert("base".into(), Value::Array(base));
    clip.insert("members".into(), copy(members));
    clip.insert("connections".into(), copy(conns));
    clip.insert("footings".into(), copy(footings));
    clip.insert("stairs".into(), copy(stairs));
    clip.insert("profiles".into(), copy(profiles));
    Some(json!({ "clip": Value::Object(clip), "counts": counts }))
}

/// `readClip(obj)` → the checked clip, or the refusal.
pub fn read_clip(obj: &Value) -> Res<Value> {
    let parsed;
    let j: &Value = match obj {
        Value::String(text) => {
            parsed = parse(text).map_err(|_| t0("clip.err.not-parts"))?;
            &parsed
        }
        v => v,
    };
    if !(j.is_object() || j.is_array()) || j.get("format").and_then(Value::as_str) != Some(CLIP_FORMAT) {
        return Err(t0("clip.err.not-parts"));
    }
    let ver = match j.get("version") {
        Some(Value::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    if !crate::js::is_integer(ver) || ver < 1.0 {
        return Err(t0("clip.err.no-version"));
    }
    if ver > CLIP_VERSION {
        return Err(t0("clip.err.newer"));
    }
    let b = match j.get("base") {
        Some(Value::Array(b)) if b.len() == 3 => b,
        _ => return Err(t0("clip.err.no-base")),
    };
    let base: Vec<f64> = b.iter().map(|x| num(Some(x))).collect();
    if base.iter().any(|n| !n.is_finite() || n.abs() > 1e7) {
        return Err(t0("clip.err.base"));
    }
    let list = |k: &str| -> Res<Value> {
        match j.get(k) {
            None => Ok(json!([])),
            Some(Value::Array(a)) => {
                if a.len() > max_of(k) {
                    return Err(t("clip.err.too-many", &[("what", F::from(k))]));
                }
                Ok(Value::Array(a.clone()))
            }
            Some(_) => Err(t("clip.err.damaged", &[("what", F::from(k))])),
        }
    };
    let input = json!({
        "format": "detailcraft", "version": 1, "name": "Clipboard", "grids": { "x": [], "y": [] }, "levels": [{ "name": "Clipboard", "z": 0 }],
        "members": list("members")?, "connections": list("connections")?, "footings": list("footings")?, "stairs": list("stairs")?, "profiles": list("profiles")?, "seq": 1,
    });
    let (state, warnings) = read_model(&input)?;
    let empty = |k: &str| arr(state.get(k)).is_empty();
    if empty("members") && empty("footings") && empty("stairs") {
        return Err(t0("clip.err.empty"));
    }
    let source: String = {
        let raw = string_or_empty(j.get("source"));
        let kept: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        crate::js::slice16(&kept, 40)
    };
    let from = crate::js::slice16(&crate::js::strip_controls(&string_or_empty(j.get("from"))), 80);
    let g = |k: &str| state.get(k).cloned().unwrap_or(json!([]));
    Ok(json!({
        "format": CLIP_FORMAT, "version": 1, "source": source, "from": from,
        "base": base.iter().map(|x| jnum(*x)).collect::<Vec<_>>(),
        "members": g("members"), "connections": g("connections"), "footings": g("footings"), "stairs": g("stairs"), "profiles": g("profiles"),
        "warnings": warnings,
    }))
}

/// `pasteInto(tx, clip, offset)` → `{ refs, profilesKept }`.
pub fn paste_into(tx: &mut Tx, clip: &Value, offset: Option<&Value>) -> Res<Value> {
    let default = json!([0, 0, 0]);
    let off_v: Vec<f64> = arr(Some(offset.unwrap_or(&default))).iter().map(|x| num(Some(x))).collect();
    if off_v.len() != 3 || off_v.iter().any(|n| !n.is_finite()) {
        return Err(t0("clip.err.offset"));
    }
    let off = [off_v.first().copied().unwrap_or(0.0), off_v.get(1).copied().unwrap_or(0.0), off_v.get(2).copied().unwrap_or(0.0)];
    let mut kept = 0usize;
    for p in arr(clip.get("profiles")) {
        let name = upper(s(p.get("name")));
        if arr(tx.s.get("profiles")).iter().any(|x| upper(s(x.get("name"))) == name) {
            kept = kept.saturating_add(1);
        } else {
            tx.add_profile(Some(p))?;
        }
    }
    let mut ids: Vec<(Value, Value)> = Vec::new();
    let mut refs: Vec<Value> = Vec::new();
    let id_of = |ids: &Vec<(Value, Value)>, k: Option<&Value>| -> Option<Value> {
        ids.iter().rev().find(|(a, _)| strict_eq(Some(a), k)).map(|(_, b)| b.clone())
    };
    for m in arr(clip.get("members")) {
        let mut mm = crate::js::spread(Some(m), None);
        mm.insert("start".into(), add(m.get("start"), off));
        mm.insert("end".into(), add(m.get("end"), off));
        let id = tx.add_member(Some(&Value::Object(mm)))?;
        for h in arr(m.get("holes")) {
            tx.add_hole(Some(&id), Some(h))?;
        }
        for c in arr(m.get("cuts")) {
            tx.add_cut(Some(&id), Some(c))?;
        }
        ids.push((m.get("id").cloned().unwrap_or(Value::Null), id.clone()));
        refs.push(json!({ "kind": "member", "id": id }));
    }
    for c in arr(clip.get("connections")) {
        if !members_of_v(c).iter().all(|m| id_of(&ids, *m).is_some()) {
            continue;
        }
        let ty = c.get("type").and_then(Value::as_str).unwrap_or("");
        let mut o = Map::new();
        let put = |o: &mut Map<String, Value>, k: &str, v: Option<Value>| {
            if let Some(v) = v {
                o.insert(k.into(), v);
            }
        };
        match ty {
            "splice" => {
                o.insert("type".into(), Value::from("splice"));
                put(&mut o, "a", id_of(&ids, c.get("a")));
                put(&mut o, "aEnd", c.get("aEnd").cloned());
                put(&mut o, "b", id_of(&ids, c.get("b")));
            }
            "gusset" => {
                o.insert("type".into(), Value::from("gusset"));
                put(&mut o, "brace", id_of(&ids, c.get("brace")));
                put(&mut o, "end", c.get("end").cloned());
                put(&mut o, "support", id_of(&ids, c.get("support")));
            }
            _ => {
                put(&mut o, "type", c.get("type").cloned());
                let beam = if crate::js::truthy(c.get("beam")) { id_of(&ids, c.get("beam")) } else { None };
                put(&mut o, "beam", beam);
                put(&mut o, "end", c.get("end").cloned());
                put(&mut o, "column", id_of(&ids, c.get("column")));
            }
        }
        put(&mut o, "params", c.get("params").cloned());
        let id = tx.add_connection(Some(&Value::Object(o)))?;
        refs.push(json!({ "kind": "conn", "id": id }));
    }
    for f in arr(clip.get("footings")) {
        let mut rest = crate::js::spread(Some(f), None);
        rest.shift_remove("id");
        rest.insert("x".into(), jnum(num(f.get("x")) + off[0]));
        rest.insert("y".into(), jnum(num(f.get("y")) + off[1]));
        rest.insert("top".into(), jnum(num(f.get("top")) + off[2]));
        let id = tx.add_footing(Some(&Value::Object(rest)))?;
        refs.push(json!({ "kind": "footing", "id": id }));
    }
    for x in arr(clip.get("stairs")) {
        let mut rest = crate::js::spread(Some(x), None);
        rest.shift_remove("id");
        rest.insert("x".into(), jnum(num(x.get("x")) + off[0]));
        rest.insert("y".into(), jnum(num(x.get("y")) + off[1]));
        rest.insert("z".into(), jnum(num(x.get("z")) + off[2]));
        let id = tx.add_stair(Some(&Value::Object(rest)))?;
        refs.push(json!({ "kind": "stair", "id": id }));
    }
    Ok(json!({ "refs": refs, "profilesKept": kept }))
}

/// `clipSegments(clip)` → segments relative to the base.
pub fn clip_segments(clip: &Value) -> Vec<[[f64; 3]; 2]> {
    let bb = arr(clip.get("base"));
    let b = [num(bb.first()), num(bb.get(1)), num(bb.get(2))];
    let rel = |p: [f64; 3]| [p[0] - b[0], p[1] - b[1], p[2] - b[2]];
    let pt3 = |v: Option<&Value>| {
        let a = arr(v);
        [num(a.first()), num(a.get(1)), num(a.get(2))]
    };
    let mut segs: Vec<[[f64; 3]; 2]> =
        arr(clip.get("members")).iter().map(|m| [rel(pt3(m.get("start"))), rel(pt3(m.get("end")))]).collect();
    for f in arr(clip.get("footings")) {
        let a = crate::js::num_or0(f.get("rotation")) * std::f64::consts::PI / 180.0;
        let (c, s_) = (crate::js::cos(a), crate::js::sin(a));
        let g = |k: &str| num(f.get(k));
        let pt = |u: f64, v: f64| rel([g("x") + u * c - v * s_, g("y") + u * s_ + v * c, g("top")]);
        let (l, bw) = (g("L"), g("B"));
        let q = [pt(-l / 2.0, -bw / 2.0), pt(l / 2.0, -bw / 2.0), pt(l / 2.0, bw / 2.0), pt(-l / 2.0, bw / 2.0)];
        for i in 0..4 {
            segs.push([crate::js::g3(q, i), crate::js::g3(q, (i + 1) % 4)]);
        }
    }
    for x in arr(clip.get("stairs")) {
        segs.extend(stair_outline(x).into_iter().map(|[a, c]| [rel(a), rel(c)]));
    }
    segs
}

/// `overlaps(state, clip, offset)`.
pub fn overlaps(state: &Value, clip: &Value, offset: Option<&Value>) -> usize {
    let o = {
        let a = arr(offset);
        [num(a.first()), num(a.get(1)), num(a.get(2))]
    };
    let key = |m: &Value, o: [f64; 3]| -> String {
        let (st, en) = (arr(m.get("start")), arr(m.get("end")));
        let vals: Vec<String> = (0..3)
            .map(|i| num(st.get(i)) + crate::js::g3(o, i))
            .chain((0..3).map(|i| num(en.get(i)) + crate::js::g3(o, i)))
            .map(|v| crate::js::num_to_string(round(v)))
            .collect();
        format!("{}|{}", crate::js::string_of(m.get("profile")), vals.join(","))
    };
    let have: Vec<String> = arr(state.get("members")).iter().map(|m| key(m, [0.0; 3])).collect();
    arr(clip.get("members")).iter().filter(|m| have.contains(&key(m, o))).count()
}
