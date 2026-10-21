//! params-doc.js — CadCraft's adapter for the parametric engine (cc-part params.js) and every parameter / constraint
//! edit as one undo step. Targets: each dimensional constraint, key "c:<id>" (label "d1 (horizontal distance)", kind
//! length | angle, at least 0, a radius / diameter / angle above 0, an angle between lines at most 180°, an arc's sweep
//! below 360°). Every edit works on a draft (copies of the constraints and parameters), resolves it forward, solves
//! the touched geometry (the first thing a dimension measures from held when it can be) and only then writes it all in
//! one change: a solve that fails changes nothing and says why.

use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad::{Doc, DocError};
use cc_part::expr;
use cc_part::omap::OMap;
use cc_part::params::{self, Adapter, Drive, Params, Problem, Target};
use serde_json::{Value, json};

use crate::con::{Con, Ref};
use crate::gcons::{
    DIMENSIONAL, MakeOpts, cons, cons_values, describe_constraint, dim_kind, drop_dim_params, fmt_value, is_dim, label_of, make_constraint,
    model_get, params_of, solve_constraints, status, status_text, target_key, type_label,
};
use crate::gsolve::{Pins, analyze, components_idx, fields_of, ref_kind};
use crate::tr::Tr;

/// What an edit gives back: `{ ok, error?, choices?, message?, c? }`.
#[derive(Clone, Debug, Default)]
pub struct Res {
    pub ok: bool,
    pub error: String,
    pub choices: Vec<String>,
    pub message: Option<String>,
    pub uses: bool,
    /// the constraint added (addConstraint)
    pub c: Option<Con>,
}
fn fail(e: String) -> Res {
    Res { ok: false, error: e, ..Res::default() }
}

/// The adapter over a draft (params-doc.js `docAdapter`): the draft's constraints, parsed once; `doc` is not read.
struct CadAdapter<'a> {
    tr: &'a Tr,
    cs: Vec<Con>,
    /// ids of the arcs among the constraints' objects (an arc's sweep may go to 360°)
    arcs: Vec<u64>,
    changed: Vec<String>,
}

impl<'a> CadAdapter<'a> {
    fn new(doc: &Doc, tr: &'a Tr, cs: &[Con]) -> Self {
        Self::owning(doc, tr, cs.to_vec())
    }
    fn owning(doc: &Doc, tr: &'a Tr, cs: Vec<Con>) -> Self {
        let model = doc.space_map(cc_cad::doc::MODEL);
        let arcs: Vec<u64> = cs
            .iter()
            .filter(|c| c.typ == "angle" && c.refs.len() == 1)
            .flat_map(|c| c.refs.iter().map(|r| r.e))
            .filter(|id| model.and_then(|m| m.get(id)).is_some_and(|e| matches!(e.geo, Geo::Arc(_))))
            .collect();
        CadAdapter { tr, cs, arcs, changed: Vec::new() }
    }
    fn index_of(&self, t: &Target) -> Option<usize> {
        let id = t.constraint.as_deref()?;
        self.cs.iter().position(|c| c.id == id)
    }
}

impl Adapter for CadAdapter<'_> {
    fn targets(&self, _doc: &Value) -> OMap<Target> {
        let mut m = OMap::new();
        for c in &self.cs {
            if !is_dim(c) {
                continue;
            }
            let arc = c.typ == "angle" && c.refs.len() == 1 && c.refs.first().is_some_and(|r| self.arcs.contains(&r.e));
            let min = if c.typ == "radius" || c.typ == "diameter" || c.typ == "angle" { 1e-9 } else { 0.0 };
            let max = if c.typ == "angle" { Some(if arc { 360.0 - 1e-9 } else { 180.0 }) } else { None };
            let label = format!("{} ({})", c.name().unwrap_or_default(), type_label(self.tr, &c.typ).to_lowercase());
            m.set(
                &target_key(c),
                Target {
                    key: target_key(c),
                    label,
                    kind: dim_kind(&c.typ),
                    min: Some(min),
                    max,
                    integer: false,
                    feature: String::new(),
                    field: None,
                    constraint: Some(c.id.clone()),
                    dim: None,
                },
            );
        }
        m
    }
    fn get(&self, _doc: &Value, t: &Target) -> f64 {
        self.index_of(t).and_then(|i| self.cs.get(i)).map_or(f64::NAN, Con::value_num)
    }
    fn set(&mut self, _doc: &mut Value, t: &Target, v: f64) {
        if let Some(i) = self.index_of(t)
            && let Some(c) = self.cs.get_mut(i)
        {
            c.set("value", cc_cad::entity::num(v));
        }
    }
    fn after_apply(&mut self, _doc: &mut Value, changed: &[String]) -> Vec<Problem> {
        self.changed = changed.to_vec();
        Vec::new()
    }
}

/// `softPins(doc, cs)`: the first point each dimension measures from (AutoCAD moves the second).
fn soft_pins(doc: &Doc, cs: &[Con]) -> Pins {
    let mut pins = Pins::new();
    for c in cs {
        let Some(r) = c.refs.first() else { continue };
        let Some(e) = model_get(doc, r.e) else { continue };
        let mut add = |fs: &[&str]| {
            for f in fs {
                pins.insert((r.e, (*f).to_string()));
            }
        };
        match ref_kind(&e, &r.k) {
            Some('P') => match &e.geo {
                Geo::Line(_) => {
                    if r.k == "p1" {
                        add(&["x1", "y1"]);
                    } else if r.k == "p2" {
                        add(&["x2", "y2"]);
                    }
                }
                Geo::Poly(_) => {
                    let i = r.k.get(1..).unwrap_or("");
                    let (x, y) = (format!("{i}x"), format!("{i}y"));
                    add(&[x.as_str(), y.as_str()]);
                }
                Geo::Point(_) => add(&["x", "y"]),
                Geo::Circle(_) | Geo::Arc(_) if r.k == "c" => add(&["cx", "cy"]),
                _ => {}
            },
            Some('R') => {
                if !matches!(e.geo, Geo::Poly(_)) {
                    add(&["cx", "cy"]);
                }
            }
            Some('L') => {
                if matches!(e.geo, Geo::Line(_)) {
                    add(&["x1", "y1"]);
                }
            }
            _ => {}
        }
    }
    pins
}

/// `commit(doc, label, constraints, params, { touch, added, pinned })`
#[allow(clippy::too_many_arguments)]
pub fn commit(
    doc: &mut Doc,
    tr: &Tr,
    label: &str,
    cs: Vec<Con>,
    prm: &Params,
    touch: &[u64],
    added: &[String],
    pinned: Option<Pins>,
) -> Result<Res, DocError> {
    let mut ad = CadAdapter::owning(doc, tr, cs);
    let mut dummy = Value::Null;
    let ap = params::apply_params(&mut dummy, prm, &mut ad, None);
    let mut dcs = std::mem::take(&mut ad.cs);
    let param_errors: Vec<&Problem> =
        ap.errors.iter().filter(|e| !e.at.starts_with("c:") || dcs.iter().any(|c| target_key(c) == e.at)).collect();
    if !param_errors.is_empty() {
        let text = param_errors
            .iter()
            .take(3)
            .map(|e| {
                let n =
                    dcs.iter().find(|k| target_key(k) == e.at).and_then(|c| c.name().map(str::to_string)).unwrap_or_else(|| e.at.clone());
                format!("{n}: {}", e.message)
            })
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(fail(text));
    }
    let changed_cs: Vec<Con> = dcs.iter().filter(|c| ad.changed.contains(&target_key(c))).cloned().collect();
    let mut only: Vec<u64> = touch.to_vec();
    for c in &changed_cs {
        for r in c.refs.iter() {
            if !only.contains(&r.e) {
                only.push(r.e);
            }
        }
    }
    let mut updates: Vec<(u64, Entity)> = Vec::new();
    if !only.is_empty() {
        let pins = pinned.unwrap_or_else(|| soft_pins(doc, &changed_cs));
        let get = |id: u64| model_get(doc, id);
        let r = solve_constraints(&get, &dcs, if pins.is_empty() { None } else { Some(&pins) }, Some(&only), false);
        if !r.ok {
            let mut seen: Vec<String> = Vec::new();
            for id in &r.conflicts {
                if !seen.contains(id) {
                    seen.push(id.clone());
                }
            }
            let names: Vec<String> =
                seen.iter().filter_map(|id| dcs.iter().find(|c| &c.id == id)).take(3).map(|c| describe_constraint(doc, tr, c)).collect();
            return Ok(fail(tr.t("params.conflicts", &[("names", names.join("; "))])));
        }
        updates = r.updates;
        if !added.is_empty() {
            let up = updates.clone();
            let get2 = |id: u64| up.iter().find(|(i, _)| *i == id).map(|(_, e)| e.clone()).or_else(|| model_get(doc, id));
            for gi in components_idx(&dcs) {
                if !gi.iter().filter_map(|i| dcs.get(*i)).any(|c| added.contains(&c.id)) {
                    continue;
                }
                let g: Vec<Con> = gi.into_iter().filter_map(|i| dcs.get(i).cloned()).collect();
                let a = analyze(&g, &get2);
                let bad: Vec<&String> = added.iter().filter(|id| a.redundant.contains(id)).collect();
                if let Some(first) = bad.first()
                    && let Some(c) = dcs.iter().find(|k| &&k.id == first)
                {
                    let mut msg = tr.t("params.redundant", &[("what", describe_constraint(doc, tr, c))]);
                    if is_dim(c) {
                        msg.push(' ');
                        msg.push_str(&tr.s("params.use_plain_dimension_dimlinear"));
                    }
                    return Ok(fail(msg));
                }
            }
        }
    }
    doc.solving = true;
    let r = doc.change(label, |d| {
        d.set_parametric(std::mem::take(&mut dcs), prm.to_json())?;
        for (id, e) in &updates {
            d.update_in(*id, e.clone(), cc_cad::doc::MODEL)?;
        }
        Ok(())
    });
    doc.solving = false;
    r?;
    Ok(Res { ok: true, ..Res::default() })
}

/// `parseDimInput(text)`: "d2=width/2" → (Some("d2"), "width/2"); "40" → (None, "40").
pub fn parse_dim_input(text: &str) -> (Option<String>, String) {
    let s = js::trim(text);
    let first = s.chars().next();
    if first.is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        let name: String = s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let rest = js::trim_start(s.get(name.len()..).unwrap_or(""));
        if let Some(after) = rest.strip_prefix('=')
            && !after.starts_with('=')
        {
            return (Some(name), js::trim(after).to_string());
        }
    }
    (None, s.to_string())
}

fn round6(v: f64) -> f64 {
    js::round(v * 1e6) / 1e6
}

/// `addConstraint(doc, type, refs, { loc, expr, name, pinFirst, pinEntity })`
pub struct AddOpts {
    pub loc: Option<cc_geom::P2>,
    pub expr: Option<String>,
    pub name: Option<String>,
    pub pin_first: bool,
    pub pin_entity: Option<u64>,
}
impl Default for AddOpts {
    fn default() -> Self {
        AddOpts { loc: None, expr: None, name: None, pin_first: true, pin_entity: None }
    }
}

pub fn add_constraint(doc: &mut Doc, tr: &Tr, typ: &str, refs: &[Ref], o: &AddOpts) -> Result<Res, DocError> {
    let c = match make_constraint(doc, tr, typ, refs, &MakeOpts { loc: o.loc, name: o.name.clone(), ..MakeOpts::default() }) {
        Ok(c) => c,
        Err(e) => return Ok(fail(e)),
    };
    let mut prm = params_of(doc);
    if is_dim(&c) {
        if let Some(n) = &o.name
            && let Some(bad) = params::name_problem(Some(&json!(n)), Some(&prm), None)
        {
            return Ok(fail(bad));
        }
        let e = match &o.expr {
            Some(x) if !x.is_empty() => x.clone(),
            _ => expr::format_number(round6(c.value_num())),
        };
        let names: Vec<String> = prm.vars.iter().map(|v| v.name.clone()).collect();
        if let Err(err) = expr::parse(&e, Some(&names)) {
            return Ok(fail(err.message));
        }
        let name = c.name().unwrap_or_default().to_string();
        prm.vars.push(params::Var { name: name.clone(), expr: e, note: None });
        prm.links.push(params::Link { target: target_key(&c), expr: name });
    }
    let mut cs = cons(doc).to_vec();
    cs.push(c.clone());
    let pin_id = o.pin_entity.or_else(|| refs.first().map(|r| r.e));
    let mut pinned = Pins::new();
    if let Some(pid) = pin_id
        && let Some(first) = model_get(doc, pid)
        && o.pin_first
        && !is_dim(&c)
        && refs.len() > 1
        && refs.iter().any(|r| r.e != pid)
    {
        for f in fields_of(&first) {
            pinned.insert((pid, f));
        }
    }
    if typ == "symmetric"
        && o.pin_first
        && let (Some(r0), Some(r1), Some(r2)) = (refs.first(), refs.get(1), refs.get(2))
        && r2.e != r0.e
        && r2.e != r1.e
        && let Some(axis) = model_get(doc, r2.e)
    {
        for f in fields_of(&axis) {
            pinned.insert((r2.e, f));
        }
    }
    let touch: Vec<u64> = refs.iter().map(|r| r.e).collect();
    let label = type_label(tr, typ).to_uppercase();
    let mut res =
        commit(doc, tr, &label, cs, &prm, &touch, std::slice::from_ref(&c.id), if pinned.is_empty() { None } else { Some(pinned) })?;
    if res.ok {
        res.c = cons(doc).iter().find(|k| k.id == c.id).cloned();
    }
    Ok(res)
}

/// `setDimInput(doc, cid, text)`
pub fn set_dim_input(doc: &mut Doc, tr: &Tr, cid: &str, text: &str) -> Result<Res, DocError> {
    let all = cons(doc);
    let Some(c) = all.iter().find(|k| k.id == cid && is_dim(k)).cloned() else { return Ok(fail(tr.s("params.constraint_gone"))) };
    let (name, ex) = parse_dim_input(text);
    if ex.is_empty() {
        return Ok(fail(tr.s("params.type_value_expression_like")));
    }
    let mut prm = params_of(doc);
    let mut cs = all.to_vec();
    let cname = c.name().unwrap_or_default().to_string();
    let mut my_name = cname.clone();
    if let Some(n) = &name
        && n != &cname
    {
        let cur = prm.var(&cname).map_or_else(|| expr::format_number(c.value_num()), |v| v.expr.clone());
        let r = params::set_var(&mut prm, n, &cur, None, Some(&cname));
        if !r.ok {
            return Ok(fail(r.error.unwrap_or_default()));
        }
        if let Some(me) = cs.iter_mut().find(|k| k.id == cid) {
            me.set("name", json!(n));
        }
        my_name = n.clone();
    }
    let lit = expr::literal(&ex);
    if let Some(lit) = lit
        && prm.var(&my_name).is_some_and(|v| expr::literal(&v.expr).is_none())
    {
        return drive_constraint_in(doc, tr, cs.to_vec(), prm, cid, lit, None);
    }
    let r = params::set_var(&mut prm, &my_name, &ex, None, None);
    if !r.ok {
        return Ok(fail(r.error.unwrap_or_default()));
    }
    commit(doc, tr, "CONSTRAINT VALUE", cs, &prm, &[], &[], None)
}

/// `driveConstraint(doc, cid, value, { prefer })`
pub fn drive_constraint(doc: &mut Doc, tr: &Tr, cid: &str, value: f64, prefer: Option<&str>) -> Result<Res, DocError> {
    let cs = cons(doc);
    let prm = params_of(doc);
    drive_constraint_in(doc, tr, cs.to_vec(), prm, cid, value, prefer)
}
fn drive_constraint_in(
    doc: &mut Doc,
    tr: &Tr,
    cs: Vec<Con>,
    mut prm: Params,
    cid: &str,
    value: f64,
    prefer: Option<&str>,
) -> Result<Res, DocError> {
    let Some(c) = cs.iter().find(|k| k.id == cid && is_dim(k)).cloned() else { return Ok(fail(tr.s("params.constraint_gone"))) };
    let ad = CadAdapter::new(doc, tr, &cs);
    let r = params::drive_back(&mut prm, &ad, &Value::Null, &target_key(&c), value, prefer);
    let name = c.name().unwrap_or_default().to_string();
    let var = match &r {
        Drive::Fail { error, choices } => {
            return Ok(Res { ok: false, error: error.clone(), choices: choices.clone().unwrap_or_default(), ..Res::default() });
        }
        Drive::Direct => {
            if let Some(v) = prm.vars.iter_mut().find(|v| v.name == name) {
                v.expr = expr::format_number(value);
            }
            None
        }
        Drive::Var { var, to, .. } => Some((var.clone(), *to)),
        _ => None,
    };
    let mut res = commit(doc, tr, "CONSTRAINT VALUE", cs, &prm, &[], &[], None)?;
    if res.ok
        && let Some((v, to)) = var
        && v != name
    {
        res.message = Some(tr.t("params.is_now", &[("name", v), ("value", expr::format_number(to))]));
    }
    Ok(res)
}

/// `setParameter(doc, name, expr, { note, rename })`
pub fn set_parameter(doc: &mut Doc, tr: &Tr, name: &str, ex: &str, rename: Option<&str>) -> Result<Res, DocError> {
    let mut prm = params_of(doc);
    let mut cs = cons(doc).to_vec();
    let r = params::set_var(&mut prm, name, js::trim(ex), None, rename);
    if !r.ok {
        return Ok(fail(r.error.unwrap_or_default()));
    }
    let renamed = rename.is_some_and(|rn| rn != name);
    if let Some(rn) = rename.filter(|rn| *rn != name) {
        for c in cs.iter_mut() {
            if is_dim(c) && c.name() == Some(rn) {
                c.set("name", json!(name));
            }
        }
    }
    commit(doc, tr, if renamed { "RENAME PARAMETER" } else { "PARAMETER" }, cs, &prm, &[], &[], None)
}

/// `driveParameter(doc, name, value, { prefer })`
pub fn drive_parameter(doc: &mut Doc, tr: &Tr, name: &str, value: f64, prefer: Option<&str>) -> Result<Res, DocError> {
    let mut prm = params_of(doc);
    let r = params::drive_var(&mut prm, name, value, prefer);
    let var = match &r {
        Drive::Fail { error, choices } => {
            return Ok(Res { ok: false, error: error.clone(), choices: choices.clone().unwrap_or_default(), ..Res::default() });
        }
        Drive::Var { var, to, .. } => Some((var.clone(), *to)),
        _ => None,
    };
    let cs = cons(doc);
    let mut res = commit(doc, tr, "PARAMETER", cs.to_vec(), &prm, &[], &[], None)?;
    if res.ok
        && let Some((v, to)) = var
        && v != name
    {
        res.message = Some(tr.t("params.is_now", &[("name", v), ("value", expr::format_number(to))]));
    }
    Ok(res)
}

/// `removeParameter(doc, name, { inline })`
pub fn remove_parameter(doc: &mut Doc, tr: &Tr, name: &str, inline: bool) -> Result<Res, DocError> {
    if cons(doc).iter().any(|k| is_dim(k) && k.name() == Some(name)) {
        return Ok(fail(tr.t("params.is_dimensional", &[("name", name.to_string())])));
    }
    let mut prm = params_of(doc);
    let r = params::remove_var(&mut prm, name, inline);
    if !r.ok {
        return Ok(Res { ok: false, error: r.error.unwrap_or_default(), uses: r.uses.is_some(), ..Res::default() });
    }
    let cs = cons(doc);
    commit(doc, tr, "DELETE PARAMETER", cs.to_vec(), &prm, &[], &[], None)
}

/// `removeConstraints(doc, ids, label)` → how many went.
pub fn remove_constraints(doc: &mut Doc, ids: &[String], label: &str) -> Result<usize, DocError> {
    let all = cons(doc);
    let gone: Vec<Con> = all.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
    if gone.is_empty() {
        return Ok(0);
    }
    let prm = drop_dim_params(&params_of(doc), &gone);
    let keep: Vec<Con> = all.iter().filter(|c| !ids.contains(&c.id)).cloned().collect();
    doc.solving = true;
    let r = doc.change(label, |d| d.set_parametric(cons_values(&keep), prm.to_json()));
    doc.solving = false;
    r?;
    Ok(gone.len())
}

/// `solveAll(doc, label)`
pub fn solve_all(doc: &mut Doc, tr: &Tr, label: &str) -> Result<Res, DocError> {
    let cs = cons(doc);
    let touch: Vec<u64> = cs.iter().flat_map(|c| c.refs.iter().map(|r| r.e)).collect();
    let prm = params_of(doc);
    commit(doc, tr, label, cs.to_vec(), &prm, &touch, &[], None)
}

/// One row of `describe(doc)`.
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub expr: String,
    pub note: String,
    pub value: Option<f64>,
    pub error: Option<String>,
    pub dim: bool,
    pub constraint: Option<Con>,
    pub type_label: String,
    pub driving: bool,
    pub used_by: Vec<String>,
    pub label: Option<String>,
    pub shown: Option<String>,
}

/// `describe(doc)` → (rows, status line)
pub fn describe(doc: &Doc, tr: &Tr) -> (Vec<Row>, String) {
    let p = params_of(doc);
    let ev = params::evaluate_vars(&p, None);
    let all = cons(doc);
    let rows = p
        .vars
        .iter()
        .map(|v| {
            let c = all.iter().find(|k| is_dim(k) && k.name() == Some(v.name.as_str())).cloned();
            let (uv, ul) = params::uses_of(&p, &v.name);
            let mut used_by = uv;
            let tk = c.as_ref().map(target_key);
            used_by.extend(ul.into_iter().filter(|t| tk.as_ref().is_none_or(|k| k != t)));
            Row {
                name: v.name.clone(),
                expr: v.expr.clone(),
                note: v.note.clone().unwrap_or_default(),
                value: ev.values.get(&v.name).copied(),
                error: ev.errors.get(&v.name).cloned(),
                dim: c.is_some(),
                type_label: c.as_ref().map_or_else(|| tr.s("params.user_parameter"), |c| type_label(tr, &c.typ)),
                driving: expr::literal(&v.expr).is_some(),
                used_by,
                label: c.as_ref().map(|c| label_of(c, Some(&p))),
                shown: c.as_ref().map(|c| fmt_value(c, c.value_num())),
                constraint: c,
            }
        })
        .collect();
    let _ = status(doc);
    (rows, status_text(doc, tr))
}

#[allow(dead_code)]
fn _keep(_: &[(&str, &str, &str, &[&str], &str)]) {
    let _ = DIMENSIONAL;
}
