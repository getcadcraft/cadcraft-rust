//! params.js — the parametric engine: global variables + equations (links), resolved forward (evaluate everything,
//! write changed values through an adapter) and backward (solve the driving variable behind an edited value through
//! the whole chain). Cycles are reported, never looped; the dependency walk is iterative.

use crate::expr::{self, ExprError};
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::omap::OMap;
use serde_json::Value;
use std::collections::HashSet;

pub const LIMIT_VARS: usize = 500;
pub const LIMIT_LINKS: usize = 4000;
pub const LIMIT_EXPR: usize = 1000;
pub const LIMIT_NOTE: usize = 200;

/// `{ name, expr, note? }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub name: String,
    pub expr: String,
    pub note: Option<String>,
}
/// `{ target, expr }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub target: String,
    pub expr: String,
}
/// `{ vars, links }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    pub vars: Vec<Var>,
    pub links: Vec<Link>,
}

impl Params {
    pub fn to_json(&self) -> Value {
        let vars = self
            .vars
            .iter()
            .map(|v| {
                let mut o = jv::obj(vec![("name", v.name.as_str().into()), ("expr", v.expr.as_str().into())]);
                if let Some(n) = &v.note {
                    jv::set(&mut o, "note", n.as_str().into());
                }
                o
            })
            .collect();
        let links =
            self.links.iter().map(|l| jv::obj(vec![("target", l.target.as_str().into()), ("expr", l.expr.as_str().into())])).collect();
        jv::obj(vec![("vars", Value::Array(vars)), ("links", Value::Array(links))])
    }
    /// A params value already in memory (a part's `params`): read as it is, no checks (readParams checks files).
    pub fn from_json(v: &Value) -> Params {
        let vars = jv::arr(v.get("vars"))
            .iter()
            .map(|x| Var {
                name: js::str_of(x.get("name")),
                expr: js::str_of(x.get("expr")),
                note: x.get("note").filter(|n| !n.is_null()).map(|n| js::str_of(Some(n))),
            })
            .collect();
        let links =
            jv::arr(v.get("links")).iter().map(|x| Link { target: js::str_of(x.get("target")), expr: js::str_of(x.get("expr")) }).collect();
        Params { vars, links }
    }
    pub fn var(&self, name: &str) -> Option<&Var> {
        self.vars.iter().find(|v| v.name == name)
    }
    fn var_index(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }
    pub fn link(&self, key: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.target == key)
    }
}

/// `hasParams(p)`.
pub fn has_params(p: Option<&Params>) -> bool {
    p.is_some_and(|p| !p.vars.is_empty() || !p.links.is_empty())
}
/// `isDriving(v)`: its expression is a plain number.
pub fn is_driving(v: &Var) -> bool {
    expr::literal(&v.expr).is_some()
}

/// `TARGET_RE` = /^[A-Za-z0-9_.:@#-]{1,120}$/.
pub fn target_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.:@#-".contains(&c))
}

/// `nameProblem(name, p, self)`: why a name cannot be a variable (`me` = the variable's own index), or None.
pub fn name_problem(name: Option<&Value>, p: Option<&Params>, me: Option<usize>) -> Option<String> {
    let Some(name) = name.and_then(Value::as_str).filter(|n| expr::name_ok(n)) else {
        return Some(t("param.name_starts_letter_has", &[]));
    };
    name_problem_str(name, p, me)
}
fn name_problem_str(name: &str, p: Option<&Params>, me: Option<usize>) -> Option<String> {
    if !expr::name_ok(name) {
        return Some(t("param.name_starts_letter_has", &[]));
    }
    if expr::reserved(name) || expr::reserved(&js::lower(name)) {
        return Some(t("param.function_constant_unit_name", &[("name", F::from(name))]));
    }
    if matches!(name, "__proto__" | "constructor" | "prototype") {
        return Some(t("param.is_reserved", &[("name", F::from(name))]));
    }
    if let Some(p) = p
        && p.vars.iter().enumerate().any(|(i, v)| v.name == name && Some(i) != me)
    {
        return Some(t("param.there_already_parameter_called", &[("name", F::from(name))]));
    }
    None
}

/// `readParams(raw, { targetKeys })`: untrusted params from a file → a clean copy, or the message.
pub fn read_params(raw: &Value, target_keys: Option<&HashSet<String>>) -> Result<Params, String> {
    if !raw.is_object() {
        return Err(t("param.parameters_file_bad", &[]));
    }
    let vars_v = raw.get("vars").filter(|v| !v.is_null());
    let links_v = raw.get("links").filter(|v| !v.is_null());
    let empty = Vec::new();
    let vars = match vars_v {
        None => &empty,
        Some(Value::Array(a)) if a.len() <= LIMIT_VARS => a,
        _ => return Err(t("param.parameter_list_file_bad", &[])),
    };
    let links = match links_v {
        None => &empty,
        Some(Value::Array(a)) if a.len() <= LIMIT_LINKS => a,
        _ => return Err(t("param.equation_list_file_bad", &[])),
    };
    let mut out = Params::default();
    for v in vars {
        if !jv::truthy(Some(v)) || !jv::is_obj_like(Some(v)) {
            return Err(t("param.parameter_file_bad", &[]));
        }
        if let Some(bad) = name_problem(v.get("name"), Some(&out), None) {
            return Err(t("param.parameter", &[("name", F::S(js::text(v.get("name"), 40))), ("bad", F::S(bad))]));
        }
        let name = jv::s(v, "name").to_string();
        let ok = match v.get("expr") {
            Some(Value::String(e)) => js::len16(e) <= LIMIT_EXPR && expr::parses(e),
            _ => false,
        };
        if !ok {
            return Err(t("param.parameter_value_not_valid", &[("name", F::S(name))]));
        }
        let note = if jv::truthy(v.get("note")) { Some(js::text(v.get("note"), LIMIT_NOTE)) } else { None };
        out.vars.push(Var { name, expr: jv::s(v, "expr").to_string(), note });
    }
    let mut seen = HashSet::new();
    for l in links {
        let target = match l.get("target") {
            Some(Value::String(s)) if jv::truthy(Some(l)) && target_ok(s) && !seen.contains(s) => s.clone(),
            _ => return Err(t("param.equation_file_bad", &[])),
        };
        if let Some(keys) = target_keys
            && !keys.contains(&target)
        {
            return Err(t("param.equation_drives_which_file", &[("target", F::S(js::slice16(&js::strip_ctrl(&target), 60)))]));
        }
        let ok = match l.get("expr") {
            Some(Value::String(e)) => js::len16(e) <= LIMIT_EXPR && expr::parses(e),
            _ => false,
        };
        if !ok {
            return Err(t("param.equation_not_valid_expression", &[("target", F::S(target))]));
        }
        seen.insert(target.clone());
        out.links.push(Link { target, expr: jv::s(l, "expr").to_string() });
    }
    Ok(out)
}

// ---------------------------------------------------------------- forward

/// `evaluateVars(p, overrides)`'s result.
#[derive(Clone, Debug, Default)]
pub struct Evaluated {
    pub values: OMap<f64>,
    pub errors: OMap<String>,
    pub order: Vec<String>,
    pub uses: OMap<Vec<String>>,
}

impl Evaluated {
    pub fn to_json(&self) -> Value {
        let mut values = jv::Obj::new();
        for (k, v) in self.values.iter() {
            values.insert(k.to_string(), jv::num(*v));
        }
        let mut errors = jv::Obj::new();
        for (k, v) in self.errors.iter() {
            errors.insert(k.to_string(), v.as_str().into());
        }
        let mut uses = jv::Obj::new();
        for (k, v) in self.uses.iter() {
            uses.insert(k.to_string(), jv::strs(v));
        }
        jv::obj(vec![
            ("values", Value::Object(values)),
            ("errors", Value::Object(errors)),
            ("order", jv::strs(&self.order)),
            ("uses", Value::Object(uses)),
        ])
    }
}

/// `evaluateVars(p, overrides)`.
pub fn evaluate_vars(p: &Params, overrides: Option<&OMap<f64>>) -> Evaluated {
    let mut names: Vec<String> = Vec::new();
    {
        let mut seen = HashSet::new();
        for v in &p.vars {
            if seen.insert(v.name.clone()) {
                names.push(v.name.clone());
            }
        }
    }
    let mut parsed: OMap<expr::Ast> = OMap::new();
    let mut uses: OMap<Vec<String>> = OMap::new();
    let mut errors: OMap<String> = OMap::new();
    for v in &p.vars {
        if overrides.is_some_and(|o| o.has(&v.name)) {
            uses.set(&v.name, Vec::new());
            continue;
        }
        match expr::parse(&v.expr, Some(&names)) {
            Err(e) => {
                errors.set(&v.name, e.message);
                uses.set(&v.name, Vec::new());
            }
            Ok(r) => {
                parsed.set(&v.name, r.ast);
                uses.set(&v.name, r.uses);
            }
        }
    }
    // dependency order (DFS); a cycle marks every variable on it
    let mut order: Vec<String> = Vec::new();
    let mut state: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let empty: Vec<String> = Vec::new();
    for v in &p.vars {
        let start = &v.name;
        match state.get(start) {
            Some(2) => continue,
            Some(1) => {
                mark_cycle(&stack, start, &mut errors);
                continue;
            }
            _ => {}
        }
        state.insert(start.clone(), 1);
        stack.push(start.clone());
        let mut frames: Vec<(String, usize)> = vec![(start.clone(), 0)];
        while let Some((n, k)) = frames.last().cloned() {
            let us = uses.get(&n).unwrap_or(&empty);
            if let Some(d) = us.get(k).cloned() {
                if let Some(fr) = frames.last_mut() {
                    fr.1 = k.saturating_add(1);
                }
                match state.get(&d) {
                    Some(2) => {}
                    Some(1) => mark_cycle(&stack, &d, &mut errors),
                    _ => {
                        state.insert(d.clone(), 1);
                        stack.push(d.clone());
                        frames.push((d, 0));
                    }
                }
            } else {
                frames.pop();
                stack.pop();
                state.insert(n.clone(), 2);
                order.push(n);
            }
        }
    }
    let mut values: OMap<f64> = OMap::new();
    for n in &order {
        if let Some(o) = overrides
            && let Some(v) = o.get(n)
        {
            values.set(n, *v);
            continue;
        }
        if errors.has(n) {
            continue;
        }
        let missing: Vec<String> = uses.get(n).unwrap_or(&empty).iter().filter(|d| !values.has(d)).cloned().collect();
        if !missing.is_empty() {
            errors.set(n, t("param.uses_which_no_value", &[("v", F::S(missing.join(", "))), ("n", F::from(missing.len()))]));
            continue;
        }
        let r = match parsed.get(n) {
            Some(ast) => expr::evaluate(ast, &values),
            None => Err(ExprError { message: t("expr.bad_expression", &[]), pos: None }),
        };
        match r {
            Ok(v) => values.set(n, v),
            Err(e) => errors.set(n, e.message),
        }
    }
    Evaluated { values, errors, order, uses }
}

fn mark_cycle(stack: &[String], n: &str, errors: &mut OMap<String>) {
    let at = stack.iter().position(|s| s == n).unwrap_or(0);
    let lp = stack.get(at..).unwrap_or(&[]);
    let msg = format!("{} {}.", lp.join(", "), if lp.len() == 1 { t("param.uses_itself", &[]) } else { t("param.use_each_other", &[]) });
    for m in lp {
        errors.set(m, msg.clone());
    }
}

/// A number a document lets equations drive (params-part.js targets).
#[derive(Clone, Debug)]
pub struct Target {
    pub key: String,
    pub label: String,
    pub kind: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub integer: bool,
    pub feature: String,
    pub field: Option<String>,
    pub constraint: Option<String>,
    pub dim: Option<String>,
}
impl Target {
    pub fn to_json(&self, value: f64) -> Value {
        let mut o = jv::obj(vec![
            ("label", self.label.as_str().into()),
            ("kind", self.kind.into()),
            ("integer", self.integer.into()),
            ("feature", self.feature.as_str().into()),
            ("value", jv::num(value)),
        ]);
        if let Some(x) = self.min {
            jv::set(&mut o, "min", jv::num(x));
        }
        if let Some(x) = self.max {
            jv::set(&mut o, "max", jv::num(x));
        }
        if let Some(x) = &self.field {
            jv::set(&mut o, "field", x.as_str().into());
        }
        if let Some(x) = &self.constraint {
            jv::set(&mut o, "constraint", x.as_str().into());
        }
        if let Some(x) = &self.dim {
            jv::set(&mut o, "dim", x.as_str().into());
        }
        o
    }
}

/// `{ where, message }`.
#[derive(Clone, Debug)]
pub struct Problem {
    pub at: String,
    pub message: String,
}
impl Problem {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![("where", self.at.as_str().into()), ("message", self.message.as_str().into())])
    }
}

/// The adapter the engine reads and writes a document through.
pub trait Adapter {
    fn targets(&self, doc: &Value) -> OMap<Target>;
    fn get(&self, doc: &Value, t: &Target) -> f64;
    fn set(&mut self, doc: &mut Value, t: &Target, v: f64);
    fn after_apply(&mut self, doc: &mut Value, changed: &[String]) -> Vec<Problem>;
}

/// `valueProblem(t, v)`.
pub fn value_problem(tg: &Target, v: f64) -> Option<String> {
    if !v.is_finite() {
        return Some(t("param.not_number", &[]));
    }
    if (tg.integer || tg.kind == "count") && (v - js::round(v)).abs() > 1e-9 {
        return Some(t("param.has_whole_number_came", &[("label", F::S(tg.label.clone())), ("v", F::S(expr::format_number(v)))]));
    }
    if let Some(mn) = tg.min
        && v < mn - 1e-12
    {
        return Some(t(
            "param.has_least_came_out",
            &[("label", F::S(tg.label.clone())), ("min", F::S(expr::format_number(mn))), ("v", F::S(expr::format_number(v)))],
        ));
    }
    if let Some(mx) = tg.max
        && v > mx + 1e-12
    {
        return Some(t(
            "param.can_most_came_out",
            &[("label", F::S(tg.label.clone())), ("max", F::S(expr::format_number(mx))), ("v", F::S(expr::format_number(v)))],
        ));
    }
    None
}

/// `applyParams`'s result.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub ok: bool,
    pub values: OMap<f64>,
    pub errors: Vec<Problem>,
    pub changed: Vec<String>,
}
impl Applied {
    pub fn to_json(&self) -> Value {
        let mut values = jv::Obj::new();
        for (k, v) in self.values.iter() {
            values.insert(k.to_string(), jv::num(*v));
        }
        jv::obj(vec![
            ("ok", self.ok.into()),
            ("values", Value::Object(values)),
            ("errors", Value::Array(self.errors.iter().map(Problem::to_json).collect())),
            ("changed", jv::strs(&self.changed)),
        ])
    }
}

/// `applyParams(doc, p, adapter, { overrides })`.
pub fn apply_params(doc: &mut Value, p: &Params, adapter: &mut dyn Adapter, overrides: Option<&OMap<f64>>) -> Applied {
    let ev = evaluate_vars(p, overrides);
    let mut errors: Vec<Problem> = ev.errors.iter().map(|(k, m)| Problem { at: k.to_string(), message: m.clone() }).collect();
    let targets = adapter.targets(doc);
    let mut changed = Vec::new();
    let mut known: Vec<String> = ev.values.key_vec();
    known.extend(ev.errors.key_vec());
    for l in &p.links {
        let Some(tg) = targets.get(&l.target) else {
            errors.push(Problem { at: l.target.clone(), message: t("param.equation_drives_something_gone", &[]) });
            continue;
        };
        let r = match expr::parse(&l.expr, Some(&known)) {
            Ok(r) => r,
            Err(e) => {
                errors.push(Problem { at: l.target.clone(), message: e.message });
                continue;
            }
        };
        let bad: Vec<String> = r.uses.iter().filter(|n| !ev.values.has(n)).cloned().collect();
        if !bad.is_empty() {
            errors.push(Problem {
                at: l.target.clone(),
                message: t("param.uses_which_no_value", &[("v", F::S(bad.join(", "))), ("n", F::from(bad.len()))]),
            });
            continue;
        }
        let mut v = match expr::evaluate(&r.ast, &ev.values) {
            Ok(v) => v,
            Err(e) => {
                errors.push(Problem { at: l.target.clone(), message: e.message });
                continue;
            }
        };
        if tg.integer || tg.kind == "count" {
            v = js::round(v);
        }
        if let Some(vp) = value_problem(tg, v) {
            errors.push(Problem { at: l.target.clone(), message: vp });
            continue;
        }
        let cur = adapter.get(doc, tg);
        if !((cur - v).abs() <= 1e-12 * js::max(1.0, v.abs())) {
            adapter.set(doc, tg, v);
            changed.push(l.target.clone());
        }
    }
    errors.extend(adapter.after_apply(doc, &changed));
    Applied { ok: errors.is_empty(), values: ev.values, errors, changed }
}

/// `describeLinks(doc, p, adapter)` rows.
pub fn describe_links(doc: &Value, p: &Params, adapter: &dyn Adapter) -> Vec<Value> {
    let ev = evaluate_vars(p, None);
    let targets = adapter.targets(doc);
    let names = ev.values.key_vec();
    p.links
        .iter()
        .map(|l| {
            let tg = targets.get(&l.target);
            let r = expr::parse(&l.expr, Some(&names)).and_then(|r| expr::evaluate(&r.ast, &ev.values));
            let mut o = jv::obj(vec![
                ("target", l.target.as_str().into()),
                ("expr", l.expr.as_str().into()),
                ("label", tg.map_or(l.target.as_str(), |x| x.label.as_str()).into()),
            ]);
            if let Some(x) = tg {
                jv::set(&mut o, "kind", x.kind.into());
            }
            jv::set(&mut o, "value", tg.map_or(Value::Null, |x| jv::num(adapter.get(doc, x))));
            jv::set(&mut o, "result", r.as_ref().map_or(Value::Null, |v| jv::num(*v)));
            let err = match (tg, &r) {
                (None, _) => Value::String(t("param.equation_drives_something_gone", &[])),
                (Some(_), Ok(_)) => Value::Null,
                (Some(_), Err(e)) => Value::String(e.message.clone()),
            };
            jv::set(&mut o, "error", err);
            o
        })
        .collect()
}

// ---------------------------------------------------------------- backward

/// `drivingBehind(p, src)`.
pub fn driving_behind(p: &Params, src: &str) -> Vec<String> {
    let Ok(r) = expr::parse(src, None) else { return Vec::new() };
    let ev = evaluate_vars(p, None);
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let empty: Vec<String> = Vec::new();
    // walk(n) for each use, depth first, children in order
    for start in &r.uses {
        let mut frames: Vec<(String, usize, bool)> = vec![(start.clone(), 0, false)];
        while let Some((n, k, entered)) = frames.last().cloned() {
            if !entered {
                if !seen.insert(n.clone()) {
                    frames.pop();
                    continue;
                }
                let Some(v) = p.var(&n) else {
                    frames.pop();
                    continue;
                };
                if is_driving(v) {
                    if !out.contains(&n) {
                        out.push(n.clone());
                    }
                    frames.pop();
                    continue;
                }
                if let Some(fr) = frames.last_mut() {
                    fr.2 = true;
                }
            }
            let us = ev.uses.get(&n).unwrap_or(&empty);
            match us.get(k) {
                Some(d) => {
                    if let Some(fr) = frames.last_mut() {
                        fr.1 = k.saturating_add(1);
                    }
                    frames.push((d.clone(), 0, false));
                }
                None => {
                    frames.pop();
                }
            }
        }
    }
    out
}

/// `{ ok, var, from, to }` | `{ ok: false, error, choices? }` | `{ ok, direct }` | `{ ok, link, from, to }`.
#[derive(Clone, Debug)]
pub enum Drive {
    Var { var: String, from: f64, to: f64 },
    Direct,
    Link { link: String, from: String, to: String },
    Set { var: String, from: String, to: String },
    Fail { error: String, choices: Option<Vec<String>> },
}
impl Drive {
    pub fn to_json(&self) -> Value {
        match self {
            Drive::Var { var, from, to } => {
                jv::obj(vec![("ok", true.into()), ("var", var.as_str().into()), ("from", jv::num(*from)), ("to", jv::num(*to))])
            }
            Drive::Direct => jv::obj(vec![("ok", true.into()), ("direct", true.into())]),
            Drive::Link { link, from, to } => jv::obj(vec![
                ("ok", true.into()),
                ("link", link.as_str().into()),
                ("from", from.as_str().into()),
                ("to", to.as_str().into()),
            ]),
            Drive::Set { var, from, to } => {
                jv::obj(vec![("ok", true.into()), ("var", var.as_str().into()), ("from", from.as_str().into()), ("to", to.as_str().into())])
            }
            Drive::Fail { error, choices } => {
                let mut o = jv::obj(vec![("ok", false.into()), ("error", error.as_str().into())]);
                if let Some(c) = choices {
                    jv::set(&mut o, "choices", jv::strs(c));
                }
                o
            }
        }
    }
    pub fn ok(&self) -> bool {
        !matches!(self, Drive::Fail { .. })
    }
}

fn solve_back(p: &Params, src: &str, value: f64, prefer: Option<&str>, label: &str) -> Drive {
    let behind = driving_behind(p, src);
    if behind.is_empty() {
        return Drive::Fail {
            error: t("param.fixed_expression_change_equation", &[("label", F::from(label)), ("src", F::from(src))]),
            choices: Some(Vec::new()),
        };
    }
    let name = match prefer {
        Some(n) if !n.is_empty() => {
            if !behind.iter().any(|b| b == n) {
                return Drive::Fail {
                    error: t("param.does_not_depend", &[("label", F::from(label)), ("name", F::from(n))]),
                    choices: Some(behind),
                };
            }
            n.to_string()
        }
        _ => {
            if behind.len() > 1 {
                return Drive::Fail {
                    error: t("param.depends_pick_which_one", &[("label", F::from(label)), ("v", F::S(behind.join(", ")))]),
                    choices: Some(behind),
                };
            }
            behind.first().cloned().unwrap_or_default()
        }
    };
    let Ok(parsed) = expr::parse(src, None) else {
        return Drive::Fail { error: t("expr.bad_expression", &[]), choices: None };
    };
    let base = evaluate_vars(p, None);
    let from = base.values.get(&name).copied().unwrap_or(f64::NAN);
    let chain = |x: f64| -> Result<f64, ExprError> {
        let mut o = OMap::new();
        o.set(&name, x);
        let ev = evaluate_vars(p, Some(&o));
        expr::evaluate(&parsed.ast, &ev.values)
    };
    match solve_chain(&chain, from, value) {
        Some(to) => Drive::Var { var: name, from, to },
        None => Drive::Fail {
            error: t(
                "param.cannot_changing",
                &[("label", F::from(label)), ("value", F::S(expr::format_number(value))), ("name", F::S(name.clone()))],
            ),
            choices: Some(behind),
        },
    }
}

fn solve_chain(chain: &dyn Fn(f64) -> Result<f64, ExprError>, x0: f64, value: f64) -> Option<f64> {
    let f = |x: f64| chain(x).map_or(f64::NAN, |y| y - value);
    let scale = js::max(js::max(1.0, value.abs()), x0.abs());
    let close = |y: f64| y.is_finite() && y.abs() <= 1e-10 * scale;
    let mut a = x0;
    let mut fa = f(a);
    if close(fa) {
        return Some(a);
    }
    let mut b = if x0 == 0.0 { 1e-3 } else { x0 * (1.0 + 1e-4) };
    let mut fb = f(b);
    let mut k: u32 = 0;
    while k < 80 && fa.is_finite() && fb.is_finite() && fb != fa {
        let c = b - fb * (b - a) / (fb - fa);
        let fc = f(c);
        if close(fc) {
            return Some(nice(c, &f, &close));
        }
        a = b;
        fa = fb;
        b = c;
        fb = fc;
        k = k.saturating_add(1);
    }
    let g0 = f(x0);
    let mut step = js::max(1e-6, x0.abs() * 1e-3);
    while step < 1e9 {
        for s in [x0 + step, x0 - step] {
            let gs = f(s);
            if close(gs) {
                return Some(nice(s, &f, &close));
            }
            if g0.is_finite() && gs.is_finite() && js::sign(gs) != js::sign(g0) {
                let (mut lo, mut hi, mut glo) = (x0, s, g0);
                for _ in 0..200 {
                    let mid = (lo + hi) / 2.0;
                    let gm = f(mid);
                    if close(gm) || (hi - lo).abs() <= 1e-15 * js::max(1.0, mid.abs()) {
                        return Some(nice(mid, &f, &close));
                    }
                    if js::sign(gm) == js::sign(glo) {
                        lo = mid;
                        glo = gm;
                    } else {
                        hi = mid;
                    }
                }
            }
        }
        step *= 2.0;
    }
    None
}

fn nice(x: f64, f: &dyn Fn(f64) -> f64, close: &dyn Fn(f64) -> bool) -> f64 {
    for d in 0..=10 {
        let p = js::pow(10.0, f64::from(d));
        let r = js::round(x * p) / p;
        if close(f(r)) {
            return r;
        }
    }
    let r = js::prec_num(x, 12);
    if close(f(r)) { r } else { x }
}

/// `driveBack(p, adapter, doc, key, value, { prefer })` (mutates p).
pub fn drive_back(p: &mut Params, adapter: &dyn Adapter, doc: &Value, key: &str, value: f64, prefer: Option<&str>) -> Drive {
    let Some(li) = p.links.iter().position(|l| l.target == key) else { return Drive::Direct };
    let targets = adapter.targets(doc);
    let tg = targets.get(key);
    let label = tg.map_or_else(|| key.to_string(), |x| x.label.clone());
    if let Some(tg) = tg
        && let Some(vp) = value_problem(tg, value)
    {
        return Drive::Fail { error: vp, choices: None };
    }
    let le = p.links.get(li).map(|l| l.expr.clone()).unwrap_or_default();
    let parsed = expr::parse(&le, None);
    if expr::literal(&le).is_some() || parsed.as_ref().map_or(true, |r| r.uses.is_empty()) {
        let to = expr::format_number(value);
        if let Some(l) = p.links.get_mut(li) {
            l.expr = to.clone();
        }
        return Drive::Link { link: key.to_string(), from: le, to };
    }
    let r = solve_back(p, &le, value, prefer, &label);
    if let Drive::Var { var, to, .. } = &r
        && let Some(i) = p.var_index(var)
        && let Some(v) = p.vars.get_mut(i)
    {
        v.expr = expr::format_number(*to);
    }
    r
}

/// `driveVar(p, name, value, { prefer })` (mutates p).
pub fn drive_var(p: &mut Params, name: &str, value: f64, prefer: Option<&str>) -> Drive {
    let Some(i) = p.var_index(name) else {
        return Drive::Fail { error: t("param.there_no_parameter_called", &[("name", F::from(name))]), choices: None };
    };
    let Some(v) = p.vars.get(i).cloned() else { return Drive::Direct };
    if is_driving(&v) {
        let to = expr::format_number(value);
        if let Some(w) = p.vars.get_mut(i) {
            w.expr = to.clone();
        }
        return Drive::Set { var: name.to_string(), from: v.expr, to };
    }
    let r = solve_back(p, &v.expr, value, prefer, name);
    if let Drive::Var { var, to, .. } = &r
        && let Some(j) = p.var_index(var)
        && let Some(w) = p.vars.get_mut(j)
    {
        w.expr = expr::format_number(*to);
    }
    r
}

/// `{ ok }` | `{ ok: false, error, cycle? }`.
#[derive(Clone, Debug)]
pub struct Edit {
    pub ok: bool,
    pub error: Option<String>,
    pub cycle: bool,
    pub value: Option<f64>,
    pub uses: Option<(Vec<String>, Vec<String>)>,
}
impl Edit {
    fn ok() -> Self {
        Edit { ok: true, error: None, cycle: false, value: None, uses: None }
    }
    fn fail(e: String) -> Self {
        Edit { ok: false, error: Some(e), cycle: false, value: None, uses: None }
    }
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![("ok", self.ok.into())]);
        if let Some(e) = &self.error {
            jv::set(&mut o, "error", e.as_str().into());
        }
        if self.cycle {
            jv::set(&mut o, "cycle", true.into());
        }
        if let Some(v) = self.value {
            jv::set(&mut o, "value", jv::num(v));
        }
        if let Some((vars, links)) = &self.uses {
            jv::set(&mut o, "uses", jv::obj(vec![("vars", jv::strs(vars)), ("links", jv::strs(links))]));
        }
        o
    }
}

/// `setVar(p, name, expr, { note, rename })`; `note`: None = not given, Some(None) = given empty (deleted).
pub fn set_var(p: &mut Params, name: &str, src: &str, note: Option<Option<&Value>>, rename: Option<&str>) -> Edit {
    if let Err(e) = expr::parse(src, None) {
        return Edit::fail(e.message);
    }
    let before = p.clone();
    let mut vi = p.var_index(rename.unwrap_or(name));
    if let Some(rn) = rename
        && rn != name
    {
        let Some(i) = vi else {
            return Edit::fail(t("param.there_no_parameter_called_2", &[("rename", F::from(rn))]));
        };
        if let Some(bad) = name_problem_str(name, Some(p), Some(i)) {
            return Edit::fail(bad);
        }
        for w in &mut p.vars {
            w.expr = expr::rename_in(&w.expr, rn, name);
        }
        for l in &mut p.links {
            l.expr = expr::rename_in(&l.expr, rn, name);
        }
        if let Some(v) = p.vars.get_mut(i) {
            v.name = name.to_string();
        }
    }
    if vi.is_none() {
        if let Some(bad) = name_problem_str(name, Some(p), None) {
            return Edit::fail(bad);
        }
        if p.vars.len() >= LIMIT_VARS {
            return Edit::fail(t("param.part_holds_parameters_most", &[("vars", F::from(LIMIT_VARS))]));
        }
        p.vars.push(Var { name: name.to_string(), expr: src.to_string(), note: None });
        vi = Some(p.vars.len().saturating_sub(1));
    }
    if let Some(v) = vi.and_then(|i| p.vars.get_mut(i)) {
        v.expr = src.to_string();
        if let Some(n) = note {
            if jv::truthy(n) {
                v.note = Some(js::text(n, LIMIT_NOTE));
            } else {
                v.note = None;
            }
        }
    }
    let ev = evaluate_vars(p, None);
    if let Some(e) = ev.errors.get(name)
        && (e.contains("each other") || e.contains("itself"))
    {
        let error = e.clone();
        *p = before;
        return Edit { ok: false, error: Some(error), cycle: true, value: None, uses: None };
    }
    Edit::ok()
}

/// `usesOf(p, name)` → (vars, link targets).
pub fn uses_of(p: &Params, name: &str) -> (Vec<String>, Vec<String>) {
    let uses = |src: &str| expr::parse(src, None).map(|r| r.uses.iter().any(|u| u == name)).unwrap_or(false);
    let vars = p.vars.iter().filter(|v| v.name != name && uses(&v.expr)).map(|v| v.name.clone()).collect();
    let links = p.links.iter().filter(|l| uses(&l.expr)).map(|l| l.target.clone()).collect();
    (vars, links)
}

/// `removeVar(p, name, { inline })`.
pub fn remove_var(p: &mut Params, name: &str, inline: bool) -> Edit {
    let Some(i) = p.var_index(name) else {
        return Edit::fail(t("param.there_no_parameter_called", &[("name", F::from(name))]));
    };
    let (uv, ul) = uses_of(p, name);
    if (!uv.is_empty() || !ul.is_empty()) && !inline {
        let mut all = uv.clone();
        all.extend(ul.iter().cloned());
        return Edit {
            ok: false,
            error: Some(t("param.is_used_by", &[("name", F::from(name)), ("v", F::S(all.join(", ")))])),
            cycle: false,
            value: None,
            uses: Some((uv, ul)),
        };
    }
    if inline {
        let val = evaluate_vars(p, None).values.get(name).copied();
        let Some(val) = val else {
            return Edit::fail(t("param.has_no_value_write", &[("name", F::from(name))]));
        };
        let lit = format!("({})", expr::format_number(val));
        for (j, w) in p.vars.iter_mut().enumerate() {
            if j != i {
                w.expr = expr::rename_in(&w.expr, name, &lit);
            }
        }
        for l in &mut p.links {
            l.expr = expr::rename_in(&l.expr, name, &lit);
        }
    }
    if i < p.vars.len() {
        p.vars.remove(i);
    }
    Edit::ok()
}

/// `setLink(p, adapter, doc, key, expr)`.
pub fn set_link(p: &mut Params, adapter: &dyn Adapter, doc: &Value, key: &str, src: &str) -> Edit {
    let targets = adapter.targets(doc);
    let Some(tg) = targets.get(key) else { return Edit::fail(t("param.value_cannot_driven_equation", &[])) };
    let ev = evaluate_vars(p, None);
    let names = ev.values.key_vec();
    let r = match expr::parse(src, Some(&names)).and_then(|r| expr::evaluate(&r.ast, &ev.values)) {
        Ok(v) => v,
        Err(e) => return Edit::fail(e.message),
    };
    let checked = if tg.integer || tg.kind == "count" { js::round(r) } else { r };
    if let Some(vp) = value_problem(tg, checked) {
        return Edit::fail(vp);
    }
    if let Some(l) = p.links.iter_mut().find(|l| l.target == key) {
        l.expr = src.to_string();
    } else {
        if p.links.len() >= LIMIT_LINKS {
            return Edit::fail(t("param.too_many_equations", &[]));
        }
        p.links.push(Link { target: key.to_string(), expr: src.to_string() });
    }
    Edit { ok: true, error: None, cycle: false, value: Some(r), uses: None }
}

/// `removeLink(p, key)`.
pub fn remove_link(p: &mut Params, key: &str) -> bool {
    match p.links.iter().position(|l| l.target == key) {
        Some(i) => {
            p.links.remove(i);
            true
        }
        None => false,
    }
}

/// `pruneLinks(p, adapter, doc)` → the dropped keys.
pub fn prune_links(p: &mut Params, adapter: &dyn Adapter, doc: &Value) -> Vec<String> {
    let keys = adapter.targets(doc);
    let gone: Vec<String> = p.links.iter().filter(|l| !keys.has(&l.target)).map(|l| l.target.clone()).collect();
    p.links.retain(|l| keys.has(&l.target));
    gone
}

// ---------------------------------------------------------------- design tables

/// One design-table row: its name and the parameters it overrides.
#[derive(Clone, Debug)]
pub struct TableRow {
    pub name: String,
    pub overrides: OMap<f64>,
}

/// `readDesignTable(csv, p)`: Ok(Ok((rows, columns))) | Ok(Err(error)) (`{ ok: false }`) | Err(thrown message).
pub fn read_design_table(csv: &str, p: &Params) -> Result<Result<(Vec<TableRow>, Vec<String>), String>, String> {
    let body = csv.strip_prefix('\u{feff}').unwrap_or(csv);
    let lines: Vec<&str> = split_lines(body).into_iter().filter(|l| !js::trim(l).is_empty()).collect();
    if lines.len() < 2 {
        return Ok(Err(t("param.design_table_needs_header", &[])));
    }
    if lines.len() > 10001 {
        return Ok(Err(t("param.design_table_has_10", &[])));
    }
    let cells: Vec<Vec<String>> = lines.iter().map(|l| split_csv(l)).collect();
    let head: Vec<String> = cells.first().map(|h| h.iter().map(|x| js::trim(x).to_string()).collect()).unwrap_or_default();
    let h0 = head.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let named = matches!(h0.as_str(), "name" | "config" | "configuration") && head.first().is_some_and(|s| s.is_ascii());
    let cols: Vec<String> = if named { head.iter().skip(1).cloned().collect() } else { head.clone() };
    for c in &cols {
        if p.var(c).is_none() {
            return Ok(Err(t("param.design_table_has_column", &[("c", F::S(js::slice16(&js::strip_ctrl(c), 40)))])));
        }
    }
    let mut rows = Vec::new();
    for (r, row) in cells.iter().enumerate().skip(1) {
        let rr = F::from(r);
        let name = if named {
            let n = js::trim(&js::slice16(&js::strip_ctrl(row.first().map_or("", String::as_str)), 60)).to_string();
            if n.is_empty() { t("param.row", &[("r", rr.clone())]) } else { n }
        } else {
            t("param.row", &[("r", rr.clone())])
        };
        let mut overrides = OMap::new();
        for (j, c) in cols.iter().enumerate() {
            let at = j.saturating_add(usize::from(named));
            let cell = js::trim(row.get(at).map_or("", String::as_str)).to_string();
            if cell.is_empty() {
                continue;
            }
            let Some(v) = expr::literal(&cell) else {
                return Err(t(
                    "param.design_table_row_not",
                    &[("r", rr), ("c", F::S(c.clone())), ("cell", F::S(js::slice16(&js::strip_ctrl(&cell), 30)))],
                ));
            };
            overrides.set(c, v);
        }
        rows.push(TableRow { name, overrides });
    }
    Ok(Ok((rows, cols)))
}

/// `s.split(/\r?\n/)`.
pub fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if q {
            if c == '"' && chars.get(i.saturating_add(1)) == Some(&'"') {
                cur.push('"');
                i = i.saturating_add(1);
            } else if c == '"' {
                q = false;
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            q = true;
        } else if c == ',' || c == ';' || c == '\t' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        i = i.saturating_add(1);
    }
    out.push(cur);
    out
}

/// `applyOverrides(p, overrides)`.
pub fn apply_overrides(p: &mut Params, overrides: &OMap<f64>) -> Result<(), String> {
    for (n, v) in overrides.iter() {
        let Some(w) = p.vars.iter_mut().find(|w| w.name == n) else {
            return Err(t("param.there_no_parameter_called_3", &[("n", F::from(n))]));
        };
        w.expr = expr::format_number(*v);
    }
    Ok(())
}
