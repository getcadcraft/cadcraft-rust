//! stdparts.js — parametric part templates for mechanical standard parts: fully constrained sketches whose dimensions
//! are driven by parameters, one configuration per size of the table (shapes_parts.rs), the chosen size active.

use crate::js;
use crate::jv;
use crate::params_part;
use crate::part;
use crate::shapes_parts as sp;
use crate::sketch::{self as s, At, ORIGIN};
use crate::sketch_tools as st;
use serde_json::Value;

/// `TEMPLATES`: (id, params).
pub const TEMPLATES: [(&str, &[&str]); 9] = [
    ("iso4017", &["d", "s", "k", "l"]),
    ("iso4014", &["d", "s", "k", "l"]),
    ("asme-hex-bolt", &["d", "s", "k", "l"]),
    ("iso4032", &["d", "s", "height"]),
    ("asme-hex-nut", &["d", "s", "height"]),
    ("iso7089", &["d1", "d2", "h"]),
    ("iso2338", &["d", "l"]),
    ("iso15", &["d", "D", "B"]),
    ("din6885", &["b", "h", "l"]),
];

/// `TEMPLATES` as JSON.
pub fn templates_json() -> Value {
    Value::Array(
        TEMPLATES
            .iter()
            .filter_map(|(id, params)| {
                let f = sp::find_family(id)?;
                Some(jv::obj(vec![
                    ("id", (*id).into()),
                    ("name", f.name.into()),
                    ("standard", f.standard.into()),
                    ("title", sp::standard(f.standard).map_or("", |x| x.0).into()),
                    ("params", jv::strs(params)),
                ]))
            })
            .collect(),
    )
}

fn num_text(v: f64) -> String {
    js::num_str(js::fixed_num(v, 4))
}

/// One configuration row: (name, size, values in the template's parameter order).
#[derive(Clone)]
struct Row {
    name: String,
    size: String,
    values: Vec<(&'static str, f64)>,
}

/// The configuration rows of a template, built once per id (they come from constant tables).
fn rows(id: &str) -> &'static [Row] {
    static CACHE: std::sync::OnceLock<Vec<(&'static str, Vec<Row>)>> = std::sync::OnceLock::new();
    let all = CACHE.get_or_init(|| TEMPLATES.iter().map(|(k, _)| (*k, build_rows(k))).collect());
    all.iter().find(|(k, _)| *k == id).map_or(&[], |(_, v)| v.as_slice())
}
fn build_rows(id: &str) -> Vec<Row> {
    sp::part_sizes(id)
        .iter()
        .map(|z| {
            let size = js::str_of(z.get("size"));
            let made = sp::make_part(id, &Value::String(size.clone()), &sp::part_options(id, &size));
            let p = made.get("part").cloned().unwrap_or(Value::Null);
            let x = p.get("dims").cloned().unwrap_or(Value::Null);
            let g = |k: &str| jv::as_num(x.get(k)).unwrap_or(f64::NAN);
            let values: Vec<(&'static str, f64)> = match id {
                "iso4017" | "iso4014" | "asme-hex-bolt" => vec![("d", g("d")), ("s", g("s")), ("k", g("k")), ("l", g("l"))],
                "iso4032" | "asme-hex-nut" => vec![("d", g("d")), ("s", g("s")), ("height", g("m"))],
                "iso7089" => vec![("d1", g("d1")), ("d2", g("d2")), ("h", g("h"))],
                "iso2338" => vec![("d", g("d")), ("l", g("l"))],
                "iso15" => vec![("d", g("d")), ("D", g("D")), ("B", g("B"))],
                "din6885" => vec![("b", g("b")), ("h", g("h")), ("l", g("l"))],
                _ => Vec::new(),
            };
            let values = values.into_iter().map(|(k, v)| (k, js::fixed_num(v, 4))).collect();
            Row { name: js::str_of(p.get("sizeText")), size, values }
        })
        .collect()
}

/// `templateSizes(id)` → [{ size, name }].
pub fn template_sizes(id: &str) -> Result<Value, String> {
    if !TEMPLATES.iter().any(|(k, _)| *k == id) && !sp::part_sizes(id).is_empty() {
        // rows(): `{ … }[id]` is undefined for a family with no template, and Object.keys(undefined) throws
        return Err("Cannot convert undefined or null to object".into());
    }
    Ok(Value::Array(rows(id).iter().map(|r| jv::obj(vec![("size", r.size.as_str().into()), ("name", r.name.as_str().into())])).collect()))
}

fn hexagon(sk: &mut Value, sz: f64) -> Result<Value, String> {
    let e = sz * 2.0 / js::sqrt(3.0);
    let poly = st::add_polygon(sk, [0.0, 0.0], [e / 2.0, 0.0], 6.0)?;
    let circ = js::str_of(poly.get("circle"));
    let lines: Vec<String> = jv::arr(poly.get("lines")).iter().map(|x| js::str_of(Some(x))).collect();
    let c = s::entities(sk).iter().find(|x| jv::is_str(x.get("id"), &circ)).map(|x| js::str_of(x.get("c"))).unwrap_or_default();
    s::add_constraint(sk, "coincident", &[c, ORIGIN.to_string()], None, None)?;
    let l1 = lines.get(1).cloned().unwrap_or_default();
    let l4 = lines.get(4).cloned().unwrap_or_default();
    s::add_constraint(sk, "horizontal", std::slice::from_ref(&l1), None, None)?;
    s::add_constraint(sk, "distance", &[l1, l4], Some(&jv::num(sz)), None)
}
fn round_c(sk: &mut Value, d: f64) -> Result<Value, String> {
    let c = s::add_circle(sk, &At::Id(ORIGIN.to_string()), d / 2.0, false)?;
    s::add_constraint(sk, "diameter", &[js::str_of(c.get("id"))], Some(&jv::num(d)), None)
}

/// `makeTemplate(id, size)`.
pub fn make_template(id: &str, size: Option<&str>) -> Result<Value, String> {
    let Some((_, params)) = TEMPLATES.iter().find(|(k, _)| *k == id) else { return Err(format!("No template for {id}")) };
    let fam = sp::find_family(id).ok_or_else(|| format!("No template for {id}"))?;
    let all = rows(id);
    let row = all.iter().find(|r| Some(r.size.as_str()) == size).or_else(|| all.first()).ok_or_else(|| format!("No template for {id}"))?;
    let v = |k: &str| row.values.iter().find(|(n, _)| *n == k).map_or(f64::NAN, |(_, x)| *x);
    let mut pt = part::new_part(&format!("{} {}", fam.name, fam.standard));
    let mut links: Vec<Value> = Vec::new();
    let top = jv::obj(vec![("kind", "base".into()), ("name", "top".into())]);
    let add_sketch = |pt: &mut Value, sk: Value| -> Result<String, String> {
        let f = part::add_feature(pt, "sketch", &jv::obj(vec![("plane", top.clone()), ("sketch", sk)]))?;
        Ok(js::str_of(f.get("id")))
    };
    let extrude = |pt: &mut Value, links: &mut Vec<Value>, skid: &str, depth: f64, expr: &str, reverse: bool| -> Result<(), String> {
        let mut props = jv::obj(vec![("sketch", skid.into()), ("depth", jv::num(depth))]);
        if reverse {
            jv::set(&mut props, "reverse", true.into());
        }
        let f = part::add_feature(pt, "extrude", &props)?;
        links.push(jv::obj(vec![("target", format!("{}.depth", js::str_of(f.get("id"))).into()), ("expr", expr.into())]));
        Ok(())
    };
    let dim = |links: &mut Vec<Value>, skid: &str, c: &Value, expr: &str| {
        links.push(jv::obj(vec![("target", format!("{skid}.{}", js::str_of(c.get("id"))).into()), ("expr", expr.into())]));
    };
    match id {
        "iso4017" | "iso4014" | "asme-hex-bolt" => {
            let mut sk = s::new_sketch();
            let cs = hexagon(&mut sk, v("s"))?;
            let head = add_sketch(&mut pt, sk)?;
            dim(&mut links, &head, &cs, "s");
            extrude(&mut pt, &mut links, &head, v("k"), "k", false)?;
            let mut sk2 = s::new_sketch();
            let cd = round_c(&mut sk2, v("d"))?;
            let shank = add_sketch(&mut pt, sk2)?;
            dim(&mut links, &shank, &cd, "d");
            extrude(&mut pt, &mut links, &shank, v("l"), "l", true)?;
        }
        "iso4032" | "asme-hex-nut" => {
            let mut sk = s::new_sketch();
            let cs = hexagon(&mut sk, v("s"))?;
            let cd = round_c(&mut sk, v("d"))?;
            let id2 = add_sketch(&mut pt, sk)?;
            dim(&mut links, &id2, &cs, "s");
            dim(&mut links, &id2, &cd, "d");
            extrude(&mut pt, &mut links, &id2, v("height"), "height", false)?;
        }
        "iso7089" => {
            let mut sk = s::new_sketch();
            let c2 = round_c(&mut sk, v("d2"))?;
            let c1 = round_c(&mut sk, v("d1"))?;
            let id2 = add_sketch(&mut pt, sk)?;
            dim(&mut links, &id2, &c2, "d2");
            dim(&mut links, &id2, &c1, "d1");
            extrude(&mut pt, &mut links, &id2, v("h"), "h", false)?;
        }
        "iso2338" => {
            let mut sk = s::new_sketch();
            let cd = round_c(&mut sk, v("d"))?;
            let id2 = add_sketch(&mut pt, sk)?;
            dim(&mut links, &id2, &cd, "d");
            extrude(&mut pt, &mut links, &id2, v("l"), "l", false)?;
        }
        "iso15" => {
            let mut sk = s::new_sketch();
            let c_big = round_c(&mut sk, v("D"))?;
            let c_small = round_c(&mut sk, v("d"))?;
            let id2 = add_sketch(&mut pt, sk)?;
            dim(&mut links, &id2, &c_big, "D");
            dim(&mut links, &id2, &c_small, "d");
            extrude(&mut pt, &mut links, &id2, v("B"), "B", false)?;
        }
        _ => {
            // din6885
            let mut sk = s::new_sketch();
            let (b, l) = (v("b"), v("l"));
            let half = (l - b) / 2.0;
            let slot = st::add_slot(&mut sk, [-half, 0.0], [half, 0.0], b)?;
            let centres: Vec<String> = jv::arr(slot.get("centres")).iter().map(|x| js::str_of(Some(x))).collect();
            let arcs: Vec<String> = jv::arr(slot.get("arcs")).iter().map(|x| js::str_of(Some(x))).collect();
            let ca = centres.first().cloned().unwrap_or_default();
            let cb = centres.get(1).cloned().unwrap_or_default();
            s::add_constraint(&mut sk, "horizontal", &[ca.clone(), cb.clone()], None, None)?;
            let ax = s::add_line(&mut sk, &At::Id(ORIGIN.to_string()), &At::Xy([0.0, 1.0]), true)?;
            let axid = js::str_of(ax.get("id"));
            s::add_constraint(&mut sk, "vertical", std::slice::from_ref(&axid), None, None)?;
            s::add_constraint(&mut sk, "fix", &[js::str_of(ax.get("p2"))], None, None)?;
            s::add_constraint(&mut sk, "symmetric", &[ca.clone(), cb.clone(), axid], None, None)?;
            let clen = s::add_constraint(&mut sk, "distance", &[ca, cb], Some(&jv::num(l - b)), None)?;
            let cr = s::add_constraint(&mut sk, "radius", &[arcs.first().cloned().unwrap_or_default()], Some(&jv::num(b / 2.0)), None)?;
            let id2 = add_sketch(&mut pt, sk)?;
            dim(&mut links, &id2, &clen, "l - b");
            dim(&mut links, &id2, &cr, "b / 2");
            extrude(&mut pt, &mut links, &id2, v("h"), "h", false)?;
        }
    }
    jv::set(&mut pt, "material", jv::obj(vec![("name", "Steel".into()), ("density", jv::num(7.85))]));
    let vars: Vec<Value> = params.iter().map(|nm| jv::obj(vec![("name", (*nm).into()), ("expr", num_text(v(nm)).into())])).collect();
    jv::set(&mut pt, "params", jv::obj(vec![("vars", Value::Array(vars)), ("links", Value::Array(links))]));
    let cfg_rows: Vec<Value> = all
        .iter()
        .map(|r| {
            let mut vals = jv::Obj::new();
            for (k, x) in &r.values {
                vals.insert((*k).to_string(), jv::num(*x));
            }
            jv::obj(vec![("name", r.name.as_str().into()), ("values", Value::Object(vals))])
        })
        .collect();
    jv::set(&mut pt, "configs", jv::obj(vec![("rows", Value::Array(cfg_rows)), ("active", row.name.as_str().into())]));
    let res = params_part::apply_part_params(&mut pt, None);
    if !res.ok {
        let msg = res.errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; ");
        return Err(format!("The template does not solve: {msg}"));
    }
    Ok(pt)
}
