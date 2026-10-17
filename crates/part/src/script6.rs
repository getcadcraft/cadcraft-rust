//! M12 ops for the call-script runner (mirrors `forks/tools/pcdiff/ops-kernel.mjs`): the mesh kernel itself, and the
//! argument placeholders that make every generator build its meshes with this side's own kernel:
//! `{"$k": <part>, "get": "<what>", "upTo": n?}` (rebuild the part here, take `<what>`) and `{"$call": "<op>", "args":
//! [...]}` (another op's result). One rebuild per distinct part per script, as on the JS side, so both number their
//! meshes the same way.

use crate::jv;
use crate::kernel::{self, Regen};
use crate::script::{Handler, num_arg};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Placeholders nest at most this deep (a `$call` inside a `$call` …). Plain JSON nesting in between does not count:
/// serde_json's parser already caps that at 128.
const MAX_DEPTH: u32 = 16;

struct Built {
    regen: Regen,
    part: Value,
}

thread_local! {
    static REGENS: RefCell<HashMap<String, Rc<Built>>> = RefCell::new(HashMap::new());
}

/// Forget this script's rebuilds (and the kernel's offset cache): a fresh JS module.
pub fn reset() {
    REGENS.with(|r| r.borrow_mut().clear());
    crate::kernel_edge::clear_offset_cache();
}

fn up_to(v: Option<&Value>) -> Option<f64> {
    v.filter(|x| !x.is_null()).map(|x| num_arg(Some(x)))
}

fn regen(part: &Value, upto: Option<f64>) -> Rc<Built> {
    let key = jv::stringify(&Value::Array(vec![part.clone(), upto.map_or(Value::Null, jv::num)]));
    if let Some(b) = REGENS.with(|r| r.borrow().get(&key).cloned()) {
        return b;
    }
    let regen = kernel::regenerate(part, upto);
    let mut p = part.clone();
    if let Some(feats) = p.get_mut("features").and_then(Value::as_array_mut) {
        for f in feats.iter_mut() {
            let id = jv::s(f, "id").to_string();
            if let Some(sk) = regen.solved.get(&id) {
                jv::set(f, "sketch", sk.clone());
            }
        }
    }
    let b = Rc::new(Built { regen, part: p });
    REGENS.with(|r| r.borrow_mut().insert(key, Rc::clone(&b)));
    b
}

fn sub(o: &Value, keys: &[&str]) -> Value {
    let mut out = jv::Obj::new();
    for k in keys {
        if let Some(x) = o.get(*k) {
            out.insert((*k).to_string(), x.clone());
        }
    }
    Value::Object(out)
}

/// What a `$k` placeholder takes from a rebuild.
fn pick(b: &Built, get: &str) -> Result<Value, String> {
    let r = &b.regen;
    let m = r.mesh.as_ref();
    Ok(match get {
        "regen" => {
            let mut j = r.to_json();
            jv::set(&mut j, "part", b.part.clone());
            j
        }
        "mesh" => m.map_or(Value::Null, |m| sub(m, &["positions", "indices"])),
        "meshTri" => m.map_or(Value::Null, |m| sub(m, &["positions", "indices", "triFace"])),
        "meshFull" => m.map_or(Value::Null, |m| sub(m, &["positions", "indices", "triFace", "edgeSegs", "segEdge"])),
        "faces" => r.faces.clone().unwrap_or(Value::Null),
        "edges" => r.edges.clone().unwrap_or(Value::Null),
        "stats" => r.stats.clone().unwrap_or(Value::Null),
        "named" => r.faces.is_some().into(),
        "threads" => r.to_json().get("threads").cloned().unwrap_or(Value::Null),
        "build" => sub(&r.to_json(), &["results", "candidates", "axes", "holeAxes", "refGeom"]),
        "part" => b.part.clone(),
        "capMesh" => r.last_named.as_ref().map_or(Value::Null, |c| {
            jv::obj(vec![
                ("positions", jv::nums(&c.positions)),
                ("indices", Value::Array(c.indices.iter().map(|&x| Value::from(x)).collect())),
            ])
        }),
        "capTriTool" => r.last_named.as_ref().map_or(Value::Null, |c| Value::Array(c.tri_tool.iter().map(|&x| Value::from(x)).collect())),
        "capSurfs" => r.last_named.as_ref().map_or(Value::Null, |c| Value::Array(c.lists.clone())),
        "capSeg" => r.last_named.as_ref().map_or(Value::Null, |c| jv::num(c.segments)),
        "drawG" => {
            let mut th = jv::Obj::new();
            for (k, x) in r.threads.iter() {
                th.insert(k.to_string(), jv::obj(vec![("callout", x.get("callout").cloned().unwrap_or(Value::Null))]));
            }
            jv::obj(vec![
                ("named", r.faces.is_some().into()),
                ("faces", r.faces.clone().unwrap_or(Value::Null)),
                ("threads", Value::Object(th)),
            ])
        }
        "ifcG" => jv::obj(vec![
            ("mesh", m.map_or(Value::Null, |m| sub(m, &["positions", "indices"]))),
            ("stats", r.stats.clone().unwrap_or(Value::Null)),
        ]),
        "measureB" => {
            let mut mm = m.map_or(Value::Null, |m| sub(m, &["positions", "indices", "triFace", "edgeSegs", "segEdge"]));
            if mm.is_null() {
                mm = Value::Null;
            }
            jv::obj(vec![
                ("exact", false.into()),
                ("named", r.faces.is_some().into()),
                ("edges", r.edges.clone().unwrap_or(Value::Array(Vec::new()))),
                ("faces", r.faces.clone().unwrap_or(Value::Array(Vec::new()))),
                ("stats", r.stats.clone().unwrap_or(Value::Null)),
                ("mesh", mm),
            ])
        }
        "asmBuild" => {
            if r.results.is_empty() && r.mesh.is_none() {
                return Ok(jv::obj(vec![("mesh", Value::Null)]));
            }
            let mut o = sub(&r.to_json(), &["results", "candidates", "axes", "holeAxes", "refGeom"]);
            jv::set(&mut o, "mesh", m.map_or(Value::Null, |m| sub(m, &["positions", "indices"])));
            jv::set(&mut o, "threads", r.to_json().get("threads").cloned().unwrap_or(Value::Null));
            o
        }
        _ => return Err(format!("unknown $k get {get}")),
    })
}

/// Replaces every placeholder (depth first, left to right).
pub fn resolve(v: &Value, handlers: &[Handler], depth: u32) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err("placeholders nested too deep".into());
    }
    match v {
        Value::Array(a) => a.iter().map(|x| resolve(x, handlers, depth)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        Value::Object(o) => {
            if let Some(p) = o.get("$k") {
                let part = resolve(p, handlers, depth.saturating_add(1))?;
                let b = regen(&part, up_to(o.get("upTo")));
                return pick(&b, jv::as_str(o.get("get")).unwrap_or(""));
            }
            if let Some(op) = o.get("$call") {
                let args = resolve(o.get("args").unwrap_or(&Value::Array(Vec::new())), handlers, depth.saturating_add(1))?;
                let op = jv::as_str(Some(op)).unwrap_or("");
                let a = jv::arr(Some(&args));
                for h in handlers {
                    if let Some(r) = h(op, a) {
                        return r;
                    }
                }
                return Err(format!("unknown op {op}"));
            }
            let mut out = jv::Obj::new();
            for (k, x) in o {
                out.insert(k.clone(), resolve(x, handlers, depth)?);
            }
            Ok(Value::Object(out))
        }
        _ => Ok(v.clone()),
    }
}

fn dmesh(v: &Value) -> crate::drawing::DMesh {
    #[allow(clippy::cast_possible_truncation)]
    let positions = jv::arr(v.get("positions")).iter().map(|x| f64::from(num_arg(Some(x)) as f32)).collect();
    let indices = jv::arr(v.get("indices")).iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX)).collect();
    crate::drawing::DMesh { positions, indices }
}

fn m4(v: &Value) -> crate::vec::M4 {
    let a = jv::arr(Some(v));
    let mut m = [f64::NAN; 16];
    for (i, x) in m.iter_mut().enumerate() {
        *x = num_arg(a.get(i));
    }
    m
}

/// `o[k]` (an array index for a number, a key for a string), undefined → None.
fn get_key(o: &Value, k: &Value) -> Option<Value> {
    match (o, k) {
        (Value::Array(a), Value::Number(n)) => n.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| a.get(i)).cloned(),
        (Value::Object(m), _) => m.get(&crate::js::str_of(Some(k))).cloned(),
        _ => None,
    }
}

/// The ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let arg = |i: usize| a.get(i).unwrap_or(&Value::Null);
    Some(match op {
        "kernel.regenerate" => pick(&regen(arg(0), up_to(a.get(1))), "regen"),
        "draw.meshViewsSec" => {
            let m = dmesh(arg(0));
            let iso = !matches!(a.get(1), Some(Value::Bool(false)));
            let sec = arg(2);
            let section = sec
                .is_object()
                .then(|| (jv::as_str(sec.get("of")).unwrap_or(""), sec.get("at").filter(|x| x.is_number()).and_then(Value::as_f64)));
            crate::kernel_section::mesh_views_section(&m, iso, section).map(|views| {
                let mut o = jv::Obj::new();
                for (k, vw) in views {
                    o.insert(k, vw.to_json());
                }
                jv::obj(vec![("views", Value::Object(o)), ("exact", false.into())])
            })
        }
        "asm.interference" => {
            let (_, solids) = crate::kernel_asm::builds_from_json(arg(1));
            let world: Vec<(String, crate::vec::M4)> =
                arg(2).as_object().map(|o| o.iter().map(|(k, m)| (k.clone(), m4(m))).collect()).unwrap_or_default();
            let o = arg(3);
            let tol = o.get("tol").map_or(1e-6, |x| num_arg(Some(x)));
            let touch = jv::truthy(o.get("touch"));
            let touch_tol = o.get("touchTol").map_or(1e-3, |x| num_arg(Some(x)));
            crate::kernel_asm::interference(arg(0), &solids, &world, tol, touch, touch_tol).map(Value::Array)
        }
        "asm.motionCheck" => {
            let (builds, solids) = crate::kernel_asm::builds_from_json(arg(1));
            let steps = a.get(5).filter(|x| !x.is_null()).map_or(12.0, |x| num_arg(Some(x)));
            let key = a.get(6).and_then(Value::as_str);
            crate::kernel_asm::motion_check(
                arg(0),
                &builds,
                &solids,
                &crate::js::str_of(a.get(2)),
                num_arg(a.get(3)),
                num_arg(a.get(4)),
                steps,
                key,
            )
            .map(Value::Array)
        }
        "util.set" => {
            let mut o = arg(0).clone();
            if !o.is_object() {
                o = Value::Object(jv::Obj::new());
            }
            jv::set(&mut o, &crate::js::str_of(a.get(1)), arg(2).clone());
            Ok(o)
        }
        "util.get" => Ok(get_key(arg(0), arg(1)).unwrap_or(Value::Null)),
        "util.path" => {
            let mut cur = arg(0).clone();
            for k in jv::arr(a.get(1)) {
                cur = get_key(&cur, k).unwrap_or(Value::Null);
            }
            Ok(cur)
        }
        "util.replaceAt" => {
            let swaps = arg(1);
            Ok(Value::Array(
                jv::arr(a.first())
                    .iter()
                    .enumerate()
                    .map(|(i, x)| swaps.get(i.to_string()).cloned().unwrap_or_else(|| x.clone()))
                    .collect(),
            ))
        }
        // JSON round trip: a serde Value already is one
        "util.json" => Ok(arg(0).clone()),
        "util.add3" => {
            let (x, y) = (jv::arr(a.first()), jv::arr(a.get(1)));
            Ok(Value::Array((0..3).map(|i| jv::num(num_arg(x.get(i)) + num_arg(y.get(i)))).collect()))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
        handle(op, a)
    }

    #[test]
    fn placeholders_resolve_and_nest_boundedly() {
        let hs: Vec<Handler> = vec![h];
        let v = serde_json::json!({ "x": [[[{ "$call": "util.get", "args": [{ "a": 5 }, "a"] }]]] });
        assert_eq!(resolve(&v, &hs, 0), Ok(serde_json::json!({ "x": [[[5]]] })));
        let mut deep = serde_json::json!(1);
        for _ in 0..40 {
            deep = serde_json::json!({ "$call": "util.json", "args": [deep] });
        }
        assert!(resolve(&deep, &hs, 0).is_err());
        let bad = serde_json::json!({ "$call": "nope", "args": [] });
        assert!(resolve(&bad, &hs, 0).is_err());
    }

    #[test]
    fn util_ops_follow_js() {
        let r = handle("util.path", &[serde_json::json!({ "a": [{ "b": 2 }] }), serde_json::json!(["a", 0, "b"])]);
        assert_eq!(r, Some(Ok(serde_json::json!(2))));
        let r = handle("util.path", &[serde_json::json!({ "a": 1 }), serde_json::json!(["z", "q"])]);
        assert_eq!(r, Some(Ok(Value::Null)));
        let r = handle("util.replaceAt", &[serde_json::json!([1, 2, 3]), serde_json::json!({ "1": -1 })]);
        assert_eq!(r, Some(Ok(serde_json::json!([1, -1, 3]))));
        let r = handle("util.add3", &[serde_json::json!([1, 2, 3]), serde_json::json!([0.5, 0.5, 0.5])]);
        assert_eq!(r, Some(Ok(serde_json::json!([1.5, 2.5, 3.5]))));
    }
}
