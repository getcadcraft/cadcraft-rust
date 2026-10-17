//! params-part.js — PartCraft's adapter for the parametric engine: every number of a part an equation may drive
//! (`"<featureId>.<field>"` from the field table, `"<sketchFeatureId>.<constraintId>"` for sketch dimensions), the
//! re-solve after a change (a sketch that cannot solve gets its dimensions back), target names, the parameter report and
//! the rollback helpers.
//!
//! The field table is static here (the JS registers fields at module load: params-part.js, bodies.js, sheetmetal.js,
//! rib.js, threads.js); the order of the fields of one type is the JS registration order.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::omap::OMap;
use crate::params::{self, Adapter, Params, Problem, Target};
use crate::sketch;
use crate::solver;
use serde_json::Value;
use std::collections::HashSet;

const POS: f64 = 1e-6;
const MAXL: f64 = 1e6;

/// How a field's label is made.
#[derive(Clone, Copy, Debug)]
pub enum Label {
    /// `lbl(key, …)` in PARAM_FIELDS: `tr(key)`.
    Key(&'static str),
    /// registered with a getter: `String(tr(key) ?? field).slice(0, 60)`.
    Reg(&'static str),
    /// registered with a plain label: `String(label).slice(0, 60)`.
    Plain(&'static str),
    /// `model.bodies.field.scale_axis` with the axis letter.
    Axis(&'static str),
}

/// One drivable field of a feature type.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub ty: &'static str,
    pub field: &'static str,
    pub label: Label,
    pub kind: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub integer: bool,
    pub when: Option<fn(&Value) -> bool>,
}

const fn fld(
    ty: &'static str,
    field: &'static str,
    label: Label,
    kind: &'static str,
    min: f64,
    max: f64,
    when: Option<fn(&Value) -> bool>,
) -> Field {
    Field { ty, field, label, kind, min: Some(min), max: Some(max), integer: matches!(kind.as_bytes(), b"count"), when }
}

fn end_blind_mid(f: &Value) -> bool {
    f.get("end").is_none() || jv::is_str(f.get("end"), "blind") || jv::is_str(f.get("end"), "mid")
}
fn not_through(f: &Value) -> bool {
    !jv::truthy(f.get("through"))
}
fn kind_cbore(f: &Value) -> bool {
    jv::is_str(f.get("kind"), "cbore")
}
fn two(f: &Value) -> bool {
    jv::truthy(f.get("two"))
}
fn end_offset(f: &Value) -> bool {
    jv::is_str(f.get("end"), "offset")
}
fn kind_csink(f: &Value) -> bool {
    jv::is_str(f.get("kind"), "csink")
}
fn kind_tapped(f: &Value) -> bool {
    jv::is_str(f.get("kind"), "tapped")
}
fn mode_two(f: &Value) -> bool {
    jv::is_str(f.get("mode"), "two")
}
fn mode_angle(f: &Value) -> bool {
    jv::is_str(f.get("mode"), "angle")
}
fn mode_offset(f: &Value) -> bool {
    jv::is_str(f.get("mode"), "offset")
}
fn mode_translate(f: &Value) -> bool {
    jv::is_str(f.get("mode"), "translate")
}
fn mode_rotate(f: &Value) -> bool {
    jv::is_str(f.get("mode"), "rotate")
}
fn uniform(f: &Value) -> bool {
    jv::truthy(f.get("uniform"))
}
fn not_uniform(f: &Value) -> bool {
    !jv::truthy(f.get("uniform"))
}
fn spacing_distance(f: &Value) -> bool {
    jv::is_str(f.get("spacing"), "distance")
}
fn relief_rect(f: &Value) -> bool {
    jv::is_str(f.get("relief"), "rect")
}

use Label::{Axis, Key, Plain, Reg};

/// The field table (`PARAM_FIELDS` after every module's `registerParamField`).
pub const FIELDS: [Field; 44] = [
    fld("extrude", "depth", Key("model.params_part.field.depth"), "length", POS, MAXL, Some(end_blind_mid)),
    fld("extrude", "depth2", Reg("model.params_part.field.second_depth"), "length", POS, MAXL, Some(two)),
    fld("extrude", "taper", Reg("model.params_part.field.taper_angle"), "angle", -89.0, 89.0, None),
    fld("extrude", "offset", Reg("model.params_part.field.offset"), "length", -MAXL, MAXL, Some(end_offset)),
    fld("revolve", "angle", Key("model.params_part.field.angle"), "angle", 0.001, 360.0, None),
    fld("hole", "d", Key("model.params_part.field.diameter"), "length", POS, MAXL, None),
    fld("hole", "depth", Key("model.params_part.field.depth"), "length", POS, MAXL, Some(not_through)),
    fld("hole", "cboreD", Key("model.params_part.field.counterbore_diameter"), "length", POS, MAXL, Some(kind_cbore)),
    fld("hole", "cboreDepth", Key("model.params_part.field.counterbore_depth"), "length", POS, MAXL, Some(kind_cbore)),
    fld("hole", "csinkD", Reg("model.params_part.field.countersink_diameter"), "length", POS, MAXL, Some(kind_csink)),
    fld("hole", "threadDepth", Reg("model.params_part.field.thread_depth"), "length", POS, MAXL, Some(kind_tapped)),
    fld("lpattern", "count", Key("model.params_part.field.count"), "count", 1.0, 1000.0, None),
    fld("lpattern", "spacing", Key("model.params_part.field.spacing"), "length", -MAXL, MAXL, None),
    fld("cpattern", "count", Key("model.params_part.field.count"), "count", 1.0, 1000.0, None),
    fld("cpattern", "angle", Key("model.params_part.field.angle"), "angle", 0.001, 360.0, None),
    fld("fillet", "r", Key("model.params_part.field.radius"), "length", POS, 1e5, None),
    fld("fillet", "r2", Reg("model.params_part.field.end_radius"), "length", POS, 1e5, None),
    fld("chamfer", "d", Key("model.params_part.field.distance"), "length", POS, 1e5, None),
    fld("chamfer", "d2", Reg("model.params_part.field.second_distance"), "length", POS, 1e5, Some(mode_two)),
    fld("chamfer", "angle", Reg("model.params_part.field.angle"), "angle", 0.001, 89.999, Some(mode_angle)),
    fld("shell", "t", Key("model.params_part.field.wall_thickness"), "length", POS, 1e5, None),
    fld("plane", "d", Reg("model.params_part.field.distance"), "length", -1e9, 1e9, Some(mode_offset)),
    fld("plane", "angle", Reg("model.params_part.field.angle"), "angle", -360.0, 360.0, Some(mode_angle)),
    fld("draft", "angle", Reg("model.params_part.field.angle"), "angle", -89.0, 89.0, None),
    fld("sweep", "scale", Reg("model.params_part.field.end_scale"), "number", 1e-3, 1e3, None),
    fld("movebody", "dx", Plain("ΔX"), "length", -1e6, 1e6, Some(mode_translate)),
    fld("movebody", "dy", Plain("ΔY"), "length", -1e6, 1e6, Some(mode_translate)),
    fld("movebody", "dz", Plain("ΔZ"), "length", -1e6, 1e6, Some(mode_translate)),
    fld("movebody", "angle", Reg("model.params_part.field.angle"), "angle", -3600.0, 3600.0, Some(mode_rotate)),
    fld("scalebody", "s", Reg("model.bodies.field.scale"), "number", 1e-6, 1e6, Some(uniform)),
    fld("scalebody", "sx", Axis("X"), "number", 1e-6, 1e6, Some(not_uniform)),
    fld("scalebody", "sy", Axis("Y"), "number", 1e-6, 1e6, Some(not_uniform)),
    fld("scalebody", "sz", Axis("Z"), "number", 1e-6, 1e6, Some(not_uniform)),
    fld("pathpattern", "count", Reg("model.params_part.field.count"), "count", 1.0, 1000.0, None),
    fld("pathpattern", "d", Reg("model.params_part.field.spacing"), "length", 1e-6, 1e6, Some(spacing_distance)),
    fld("sheetbase", "t", Reg("sheet.param.t"), "length", 0.01, 100.0, None),
    fld("sheetbase", "r", Reg("sheet.param.r"), "length", 0.0, 1000.0, None),
    fld("sheetbase", "k", Reg("sheet.param.k"), "number", 0.0, 0.5, None),
    fld("sheetbase", "depth", Reg("sheet.param.depth"), "length", 0.01, 1e5, None),
    fld("sheetflange", "angle", Reg("sheet.param.angle"), "angle", 1.0, 180.0, None),
    fld("sheetflange", "length", Reg("sheet.param.length"), "length", 0.01, 1e5, None),
    fld("sheetflange", "reliefWidth", Reg("sheet.param.relief_width"), "length", 0.01, 100.0, Some(relief_rect)),
    fld("rib", "t", Reg("rib.param.t"), "length", 1e-4, 1e5, None),
    fld("thread", "length", Reg("thread.param.length"), "length", 1e-3, 1e5, None),
];
/// The thread clearance (the 45th field; kept apart so the table above stays a plain array literal).
pub const THREAD_CLEARANCE: Field = fld("thread", "clearance", Reg("thread.param.clearance"), "length", 0.0, 5.0, None);

/// The fields of a feature type, in registration order.
pub fn fields_of(ty: &str) -> Vec<&'static Field> {
    let mut v: Vec<&'static Field> = FIELDS.iter().filter(|f| f.ty == ty).collect();
    if ty == "thread" {
        v.push(&THREAD_CLEARANCE);
    }
    v
}
fn field_spec(ty: &str, field: &str) -> Option<&'static Field> {
    fields_of(ty).into_iter().find(|f| f.field == field)
}

/// A field's label now.
pub fn label(f: &Field) -> String {
    match f.label {
        Key(k) => t(k, &[]),
        Reg(k) => js::slice16(&t(k, &[]), 60),
        Plain(s) => js::slice16(s, 60),
        Axis(a) => js::slice16(&t("model.bodies.field.scale_axis", &[("axis", F::from(a))]), 60),
    }
}

/// `fieldInUse(f, field)`.
pub fn field_in_use(f: &Value, field: &str) -> bool {
    field_spec(jv::s(f, "type"), field).is_some_and(|s| s.when.is_none_or(|w| w(f)))
}

fn dim_label(ty: &str) -> String {
    js::lower(&sketch::label_of(ty).filter(|s| !s.is_empty()).unwrap_or_else(|| ty.to_string()))
}
fn dims_of(sk: &Value) -> Vec<&Value> {
    sketch::constraints(sk).iter().filter(|c| sketch::is_dim(jv::s(c, "type"))).collect()
}

/// `targets(part)`.
pub fn targets(part: &Value) -> OMap<Target> {
    let mut out = OMap::new();
    for f in jv::arr(part.get("features")) {
        let ty = jv::s(f, "type");
        let fid = js::str_of(f.get("id"));
        let fname = js::str_of(f.get("name"));
        for s in fields_of(ty) {
            if jv::finite(f.get(s.field)).is_none() {
                continue;
            }
            let key = format!("{fid}.{}", s.field);
            out.set(
                &key,
                Target {
                    key: key.clone(),
                    label: format!("{fname} {}", label(s)),
                    kind: s.kind,
                    min: s.min,
                    max: s.max,
                    integer: s.integer,
                    feature: fid.clone(),
                    field: Some(s.field.to_string()),
                    constraint: None,
                    dim: None,
                },
            );
        }
        if ty == "sketch"
            && let Some(sk) = f.get("sketch").filter(|v| jv::truthy(Some(v)))
        {
            for (i, c) in dims_of(sk).iter().enumerate() {
                let cid = js::str_of(c.get("id"));
                let key = format!("{fid}.{cid}");
                let cty = jv::s(c, "type");
                let ang = cty == "angle";
                let min = if ang {
                    0.0
                } else if cty == "radius" || cty == "diameter" {
                    POS
                } else {
                    0.0
                };
                let dn = format!("D{}", i.saturating_add(1));
                out.set(
                    &key,
                    Target {
                        key: key.clone(),
                        label: format!("{fname} {dn} ({})", dim_label(cty)),
                        kind: if ang { "angle" } else { "length" },
                        min: Some(min),
                        max: Some(if ang { 180.0 } else { MAXL }),
                        integer: false,
                        feature: fid.clone(),
                        field: None,
                        constraint: Some(cid),
                        dim: Some(dn),
                    },
                );
            }
        }
    }
    out
}

fn feature_index(part: &Value, id: &str) -> Option<usize> {
    jv::arr(part.get("features")).iter().position(|f| jv::is_str(f.get("id"), id))
}

/// The value a target names now.
pub fn target_value(part: &Value, tg: &Target) -> f64 {
    let Some(f) = feature_index(part, &tg.feature).and_then(|i| jv::arr(part.get("features")).get(i)) else { return f64::NAN };
    if let Some(field) = &tg.field {
        return jv::n(f, field);
    }
    let cid = tg.constraint.as_deref().unwrap_or("");
    let sk = f.get("sketch").unwrap_or(&Value::Null);
    sketch::constraints(sk).iter().find(|c| jv::is_str(c.get("id"), cid)).map_or(f64::NAN, |c| jv::n(c, "value"))
}

fn set_target(part: &mut Value, tg: &Target, v: f64) {
    let Some(i) = feature_index(part, &tg.feature) else { return };
    let Some(f) = part.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.get_mut(i)) else { return };
    if let Some(field) = &tg.field {
        jv::set(f, field, jv::num(v));
        return;
    }
    let cid = tg.constraint.clone().unwrap_or_default();
    if let Some(c) = f
        .get_mut("sketch")
        .and_then(|s| s.get_mut("constraints"))
        .and_then(Value::as_array_mut)
        .and_then(|cs| cs.iter_mut().find(|c| jv::is_str(c.get("id"), &cid)))
    {
        jv::set(c, "value", jv::num(v));
    }
}

/// `partAdapter`, with the journal of first values the JS keeps in a WeakMap per part.
#[derive(Default)]
pub struct PartAdapter {
    journal: OMap<f64>,
}

impl Adapter for PartAdapter {
    fn targets(&self, doc: &Value) -> OMap<Target> {
        targets(doc)
    }
    fn get(&self, doc: &Value, tg: &Target) -> f64 {
        target_value(doc, tg)
    }
    fn set(&mut self, doc: &mut Value, tg: &Target, v: f64) {
        let old = target_value(doc, tg);
        if !self.journal.has(&tg.key) {
            self.journal.set(&tg.key, old);
        }
        set_target(doc, tg, v);
    }
    fn after_apply(&mut self, doc: &mut Value, changed: &[String]) -> Vec<Problem> {
        let j = std::mem::take(&mut self.journal);
        let mut errors = Vec::new();
        let mut by_sketch: Vec<(usize, Vec<String>)> = Vec::new();
        for key in changed {
            let (fid, sub) = split_key(key);
            let Some(i) = feature_index(doc, &fid) else { continue };
            let is_sketch = jv::arr(doc.get("features")).get(i).is_some_and(|f| jv::s(f, "type") == "sketch");
            if !is_sketch {
                continue;
            }
            match by_sketch.iter_mut().find(|(k, _)| *k == i) {
                Some(e) => e.1.push(sub),
                None => by_sketch.push((i, vec![sub])),
            }
        }
        for (i, cids) in by_sketch {
            let Some(f) = doc.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.get_mut(i)) else { continue };
            let fid = js::str_of(f.get("id"));
            let fname = js::str_of(f.get("name"));
            let Some(sk) = f.get_mut("sketch") else { continue };
            let r = solver::solve_sketch(sk, None, None);
            if r.ok {
                continue;
            }
            let mut names = Vec::new();
            for cid in &cids {
                let old = j.get(&format!("{fid}.{cid}")).copied();
                let idx = sketch::constraints(sk).iter().position(|k| jv::is_str(k.get("id"), cid));
                if let (Some(ci), Some(old)) = (idx, old) {
                    let (dl, cur) = {
                        let c = sketch::constraints(sk).get(ci).unwrap_or(&Value::Null);
                        let di = dims_of(sk).iter().position(|d| std::ptr::eq(*d, c)).map_or(0, |d| d.saturating_add(1));
                        (format!("{fname} D{di}"), jv::n(c, "value"))
                    };
                    names.push(format!("{dl} = {}", js::num_str(js::prec_num(cur, 12))));
                    if let Some(c) = sk.get_mut("constraints").and_then(Value::as_array_mut).and_then(|a| a.get_mut(ci)) {
                        jv::set(c, "value", jv::num(old));
                    }
                }
            }
            solver::solve_sketch(sk, None, None);
            errors.push(Problem {
                at: fname.clone(),
                message: t(
                    "model.params_part.cannot_solve",
                    &[("name", F::S(fname.clone())), ("values", F::S(names.join(", "))), ("n", F::from(names.len()))],
                ),
            });
        }
        errors
    }
}

/// `splitKey(key)`.
pub fn split_key(key: &str) -> (String, String) {
    match key.find('.') {
        Some(i) => (key.get(..i).unwrap_or("").to_string(), key.get(i.saturating_add(1)..).unwrap_or("").to_string()),
        None => (key.to_string(), String::new()),
    }
}

/// `paramTargetKeys(part)`.
pub fn param_target_keys(part: &Value) -> HashSet<String> {
    targets(part).keys().map(str::to_string).collect()
}

/// `readPartParams(raw, part)`: None when empty.
pub fn read_part_params(raw: &Value, part: &Value) -> Result<Option<Params>, String> {
    let p = params::read_params(raw, Some(&param_target_keys(part)))?;
    Ok(if params::has_params(Some(&p)) { Some(p) } else { None })
}

/// `part.params` as typed params (empty when absent).
pub fn params_of(part: &Value) -> Params {
    part.get("params").filter(|v| v.is_object()).map(Params::from_json).unwrap_or_default()
}
/// Write params back into the part (`part.params` keeps its place; an empty table stays until [`tidy_params`]).
pub fn put_params(part: &mut Value, p: &Params) {
    jv::set(part, "params", p.to_json());
}
/// `tidyParams(part)`.
pub fn tidy_params(part: &mut Value) {
    if part.get("params").is_some() && !params::has_params(Some(&params_of(part))) {
        jv::del(part, "params");
    }
}

/// `applyPartParams(part, opts)`.
pub fn apply_part_params(part: &mut Value, overrides: Option<&OMap<f64>>) -> params::Applied {
    let p = params_of(part);
    if !params::has_params(Some(&p)) || !part.get("params").is_some_and(Value::is_object) {
        return params::Applied { ok: true, ..params::Applied::default() };
    }
    let mut ad = PartAdapter::default();
    params::apply_params(part, &p, &mut ad, overrides)
}

/// `resolveTarget(part, text)`.
pub fn resolve_target(part: &Value, text: &str) -> Option<String> {
    let s = js::trim(text).to_string();
    let all = targets(part);
    if all.has(&s) {
        return Some(s);
    }
    // /^(.+?)[ .]([A-Za-z][A-Za-z0-9 ]*)$/
    let chars: Vec<char> = s.chars().collect();
    let field_ok =
        |rest: &[char]| rest.first().is_some_and(char::is_ascii_alphabetic) && rest.iter().all(|c| c.is_ascii_alphanumeric() || *c == ' ');
    let mut split = None;
    for i in 1..chars.len() {
        let head = chars.get(..i).unwrap_or(&[]);
        if head.iter().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')) {
            break;
        }
        if matches!(chars.get(i), Some(' ' | '.')) && field_ok(chars.get(i.saturating_add(1)..).unwrap_or(&[])) {
            split = Some(i);
            break;
        }
    }
    let i = split?;
    let fname: String = chars.get(..i).unwrap_or(&[]).iter().collect();
    let field: String = chars.get(i.saturating_add(1)..).unwrap_or(&[]).iter().collect();
    let fs: Vec<&Value> =
        jv::arr(part.get("features")).iter().filter(|f| jv::is_str(f.get("name"), &fname) || jv::is_str(f.get("id"), &fname)).collect();
    if fs.len() != 1 {
        return None;
    }
    let f = fs.first()?;
    let fid = js::str_of(f.get("id"));
    let ty = jv::s(f, "type");
    for (key, tg) in all.iter() {
        if tg.feature != fid {
            continue;
        }
        if let Some(fl) = &tg.field
            && (*fl == field || field_spec(ty, fl).is_some_and(|s| js::lower(&label(s)) == js::lower(&field)))
        {
            return Some(key.to_string());
        }
        if let Some(d) = &tg.dim
            && js::lower(d) == js::lower(&field)
        {
            return Some(key.to_string());
        }
    }
    None
}

/// `targetName(part, key)`.
pub fn target_name(part: &Value, key: &str) -> String {
    let all = targets(part);
    let Some(tg) = all.get(key) else { return key.to_string() };
    let fname = feature_index(part, &tg.feature)
        .and_then(|i| jv::arr(part.get("features")).get(i))
        .map(|f| js::str_of(f.get("name")))
        .unwrap_or_default();
    format!("{fname}.{}", tg.field.clone().or_else(|| tg.dim.clone()).unwrap_or_default())
}

/// `pruneDeleted(partBefore, partAfter)` → labels of the dropped links.
pub fn prune_deleted(before: &Value, after: &mut Value) -> Vec<String> {
    if !after.get("params").is_some_and(|p| jv::truthy(Some(p))) {
        return Vec::new();
    }
    let keep = targets(after);
    let labels = targets(before);
    let mut p = params_of(after);
    let gone: Vec<String> = p.links.iter().filter(|l| !keep.has(&l.target)).map(|l| l.target.clone()).collect();
    p.links.retain(|l| keep.has(&l.target));
    put_params(after, &p);
    tidy_params(after);
    gone.iter().map(|g| labels.get(g).map(|x| x.label.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| g.clone())).collect()
}

/// `paramsReport(part)`.
pub fn params_report(part: &Value) -> Value {
    let p = params_of(part);
    let ev = params::evaluate_vars(&p, None);
    let vars: Vec<Value> = p
        .vars
        .iter()
        .map(|v| {
            jv::obj(vec![
                ("name", v.name.as_str().into()),
                ("expr", v.expr.as_str().into()),
                ("note", v.note.clone().filter(|s| !s.is_empty()).unwrap_or_default().into()),
                ("value", ev.values.get(&v.name).map_or(Value::Null, |x| jv::num(*x))),
                ("error", ev.errors.get(&v.name).map_or(Value::Null, |x| x.as_str().into())),
            ])
        })
        .collect();
    let ad = PartAdapter::default();
    let links: Vec<Value> = params::describe_links(part, &p, &ad)
        .into_iter()
        .map(|mut l| {
            let tg = jv::s(&l, "target").to_string();
            jv::set(&mut l, "name", target_name(part, &tg).into());
            l
        })
        .collect();
    jv::obj(vec![("vars", Value::Array(vars)), ("links", Value::Array(links))])
}

// ---------------------------------------------------------------- rollback

fn built(part: &Value) -> f64 {
    let n = js::f(jv::arr(part.get("features")).len());
    match part.get("rollback") {
        None | Some(Value::Null) => n,
        Some(v) => js::min(v.as_f64().unwrap_or(f64::NAN), n),
    }
}

/// `{ ok, from, to, message }`.
pub fn roll_json(ok: bool, from: Option<f64>, to: Option<f64>, message: &str) -> Value {
    let mut o = jv::obj(vec![("ok", ok.into())]);
    if let Some(f) = from {
        jv::set(&mut o, "from", jv::num(f));
    }
    if let Some(t) = to {
        jv::set(&mut o, "to", jv::num(t));
    }
    jv::set(&mut o, "message", message.into());
    o
}

/// `stepRollback(part, delta)`.
pub fn step_rollback(part: &mut Value, delta: f64) -> Value {
    let from = built(part);
    let n = js::f(jv::arr(part.get("features")).len());
    let to = js::max(0.0, js::min(n, from + delta));
    if to == from {
        let msg = if delta < 0.0 {
            if n != 0.0 { t("model.params_part.rollback.at_top", &[]) } else { t("model.params_part.rollback.none", &[]) }
        } else {
            t("model.params_part.rollback.at_end", &[])
        };
        return roll_json(false, Some(from), Some(to), &msg);
    }
    jv::set(part, "rollback", if to >= n { Value::Null } else { jv::num(to) });
    roll_json(true, Some(from), Some(to), &rollback_words(part))
}

/// `rollTo(part, id)`.
pub fn roll_to(part: &mut Value, id: Option<&str>) -> Value {
    let Some(id) = id else {
        let from = built(part);
        jv::set(part, "rollback", Value::Null);
        let n = js::f(jv::arr(part.get("features")).len());
        return roll_json(true, Some(from), Some(n), &rollback_words(part));
    };
    let Some(i) = feature_index(part, id) else { return roll_json(false, None, None, &t("model.params_part.no_such_feature", &[])) };
    let from = built(part);
    jv::set(part, "rollback", jv::unum(i));
    roll_json(true, Some(from), Some(js::f(i)), &rollback_words(part))
}

/// `rollbackWords(part)`.
pub fn rollback_words(part: &Value) -> String {
    let b = built(part);
    let feats = jv::arr(part.get("features"));
    let n = js::f(feats.len());
    if b >= n {
        return t("model.params_part.rollback.all", &[("n", F::N(n))]);
    }
    let name = |i: f64| js::idx(i).and_then(|i| feats.get(i)).map(|f| js::str_of(f.get("name"))).unwrap_or_else(|| "undefined".into());
    if b == 0.0 {
        return t("model.params_part.rollback.nothing", &[("name", F::S(name(0.0)))]);
    }
    t("model.params_part.rollback.after", &[("name", F::S(name(b - 1.0))), ("b", F::N(b)), ("n", F::N(n))])
}
