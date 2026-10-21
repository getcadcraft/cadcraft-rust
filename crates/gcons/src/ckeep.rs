//! ckeep.js — constraints that come through MIRROR and TRIM, inside the command's change (one undo step with the
//! geometry): MIRROR copies the constraints among the sources onto the copies with signs and sides worked out again
//! (dimensional ones get new names, as a paste does); TRIM moves each constraint on the cut object to the piece that
//! still has what it holds and drops the ones that held the part that went or a length the cut changed.

use cc_cad::entity::Entity;
use cc_cad::js;
use cc_cad::{Doc, DocError};
use cc_cad_geom::jsmath::JsMath;
use serde_json::json;

use crate::con::{Con, Ref};
use crate::gcons::{
    MakeOpts, clip_constraints, cons, cons_values, drop_dim_params, is_dim, make_constraint, model_get, params_of, paste_constraints,
};
use crate::tr::Tr;

/// `reSign(doc, c)`: the constraint with its signs, sides and fixed values taken from the geometry as it is now.
fn re_sign(doc: &Doc, tr: &Tr, c: &Con) -> Option<Con> {
    let n = make_constraint(
        doc,
        tr,
        &c.typ,
        &c.refs,
        &MakeOpts {
            id: Some(c.id.clone()),
            name: c.name().map(str::to_string),
            value: is_dim(c).then_some(c.value_num()),
            model: false,
            loc: None,
        },
    )
    .ok()?;
    let mut out = c.clone();
    for k in ["sign", "kind", "pair", "value", "fixed"] {
        if k == "value" && is_dim(c) {
            continue;
        }
        match n.get(k) {
            Some(v) => out.set(k, v.clone()),
            None => {
                if k != "value" {
                    out.remove(k);
                }
            }
        }
    }
    if c.typ == "angle"
        && let Some(d) = c.dirs()
    {
        let line = |i: usize| {
            c.refs
                .get(i)
                .and_then(|r| model_get(doc, r.e))
                .and_then(|e| if let cc_cad::entity::Geo::Line(l) = e.geo { Some(l) } else { None })
        };
        if let (Some(l1), Some(l2)) = (line(0), line(1)) {
            let u = [(l1.x2 - l1.x1) * d[0], (l1.y2 - l1.y1) * d[0]];
            let v = [(l2.x2 - l2.x1) * d[1], (l2.y2 - l2.y1) * d[1]];
            out.set("sign", json!(if (u[0] * v[1] - u[1] * v[0]).jatan2(u[0] * v[0] + u[1] * v[1]) >= 0.0 { 1 } else { -1 }));
            if let Some(dv) = c.get("dirs").cloned() {
                out.set("dirs", dv);
            }
        }
    }
    Some(out)
}

/// The value to pass `value` with when re-signing (JS passes `c.value` whatever the type).
fn _unused() {}

/// `resignAmong(doc, ids)`: after an in-place change (MIRROR erasing its source).
pub fn resign_among(doc: &mut Doc, tr: &Tr, ids: &[u64]) -> Result<usize, DocError> {
    if !doc.in_model() || doc.constraints.is_empty() {
        return Ok(0);
    }
    let mut n = 0usize;
    let cs: Vec<Con> = cons(doc)
        .iter()
        .cloned()
        .map(|c| {
            if !c.refs.iter().all(|r| ids.contains(&r.e)) {
                return c;
            }
            match re_sign(doc, tr, &c) {
                Some(k) => {
                    n = n.saturating_add(1);
                    k
                }
                None => c,
            }
        })
        .collect();
    if n > 0 {
        let p = doc.params.clone();
        doc.set_parametric(cons_values(&cs), p)?;
    }
    Ok(n)
}

/// `mirrorConstraints(doc, pairs)`: pairs (source id, copy id).
pub fn mirror_constraints(doc: &mut Doc, tr: &Tr, pairs: &[(u64, u64)]) -> Result<usize, DocError> {
    if !doc.in_model() || doc.constraints.is_empty() || pairs.is_empty() {
        return Ok(0);
    }
    let index: Vec<(u64, usize)> = pairs.iter().enumerate().map(|(i, (s, _))| (*s, i)).collect();
    let list = clip_constraints(doc, &index);
    if list.is_empty() {
        return Ok(0);
    }
    let before: Vec<String> = cons(doc).iter().map(|c| c.id.clone()).collect();
    let ids: Vec<Option<u64>> = pairs.iter().map(|(_, c)| Some(*c)).collect();
    let n = paste_constraints(doc, &list, &ids, 0.0, 0.0)?;
    if n == 0 {
        return Ok(0);
    }
    let cs: Vec<Con> =
        cons(doc).iter().cloned().map(|c| if before.contains(&c.id) { c } else { re_sign(doc, tr, &c).unwrap_or(c) }).collect();
    let p = doc.params.clone();
    doc.set_parametric(cons_values(&cs), p)?;
    Ok(n)
}

/// A piece of a cut object: its id, and the original's parameters 0…1 it spans.
pub struct Piece {
    pub id: u64,
    pub t0: f64,
    pub t1: f64,
}

/// `refWhat(type, k)`
fn ref_what(typ: &str, k: &str) -> Option<&'static str> {
    match typ {
        "line" => match k {
            "p1" => Some("start"),
            "p2" => Some("end"),
            "mid" => Some("mid"),
            "L" => Some("curve"),
            _ => None,
        },
        "arc" => match k {
            "s" => Some("start"),
            "e" => Some("end"),
            "mid" => Some("mid"),
            "c" | "R" => Some("circle"),
            _ => None,
        },
        "circle" => match k {
            "c" | "R" => Some("circle"),
            _ => None,
        },
        _ => None,
    }
}

/// `trimConstraints(doc, oldE, pieces)` → (kept, dropped)
pub fn trim_constraints(doc: &mut Doc, tr: &Tr, old: &Entity, pieces: &[Piece]) -> Result<(usize, usize), DocError> {
    if doc.constraints.is_empty() {
        return Ok((0, 0));
    }
    let Some(id) = old.id else { return Ok((0, 0)) };
    let all = cons(doc);
    let mine: Vec<Con> = all.iter().filter(|c| c.refs.iter().any(|r| r.e == id)).cloned().collect();
    if mine.is_empty() {
        return Ok((0, 0));
    }
    let others: Vec<Con> = all.iter().filter(|c| !c.refs.iter().any(|r| r.e == id)).cloned().collect();
    let with_start = pieces.iter().find(|p| p.t0 <= 1e-9);
    let with_end = pieces.iter().find(|p| p.t1 >= 1.0 - 1e-9);
    let typ = old.typ();
    let mut keep: Vec<(Con, bool)> = Vec::new();
    let mut dropped: Vec<Con> = Vec::new();
    for c in &mine {
        if typ == "poly" || pieces.is_empty() {
            dropped.push(c.clone());
            continue;
        }
        let mut ok = true;
        let mut on_all = false;
        let refs: Vec<Ref> = c
            .refs
            .iter()
            .map(|r| {
                if r.e != id {
                    return r.clone();
                }
                let what = ref_what(typ, &r.k);
                if what == Some("start")
                    && let Some(p) = with_start
                {
                    return Ref { e: p.id, k: r.k.clone() };
                }
                if what == Some("end")
                    && let Some(p) = with_end
                {
                    return Ref { e: p.id, k: r.k.clone() };
                }
                if what == Some("curve") || what == Some("circle") {
                    if c.typ == "equal" || (what == Some("circle") && c.typ == "angle") {
                        ok = false;
                        return r.clone();
                    }
                    if c.typ == "fix" {
                        ok = false;
                        return r.clone();
                    }
                    on_all = matches!(c.typ.as_str(), "horizontal" | "vertical" | "parallel" | "perpendicular")
                        && c.refs.iter().filter(|q| q.e == id).count() == 1;
                    return Ref { e: pieces.first().map_or(id, |p| p.id), k: r.k.clone() };
                }
                ok = false;
                r.clone()
            })
            .collect();
        if !ok {
            dropped.push(c.clone());
            continue;
        }
        if c.typ == "angle" && c.refs.len() == 1 {
            dropped.push(c.clone());
            continue;
        }
        let mut k = c.clone();
        k.set_refs(refs.clone());
        keep.push((k, true));
        if on_all {
            for p in pieces.iter().skip(1) {
                let mut cp = c.clone();
                cp.set("id", serde_json::Value::Null);
                cp.id = String::new();
                cp.set_refs(
                    refs.iter()
                        .zip(c.refs.iter())
                        .map(|(r, orig)| if orig.e == id { Ref { e: p.id, k: r.k.clone() } } else { r.clone() })
                        .collect(),
                );
                keep.push((cp, false));
            }
        }
    }
    let mut ids: Vec<String> = others
        .iter()
        .map(|c| c.id.clone())
        .chain(keep.iter().filter(|(_, has)| *has).map(|(c, _)| c.id.clone()))
        .filter(|s| !s.is_empty())
        .collect();
    let mut n = 1u64;
    let mut out: Vec<Con> = Vec::new();
    let keep_len = keep.len();
    for (mut c, has_id) in keep {
        if !has_id {
            while ids.contains(&format!("k{n}")) {
                n = n.saturating_add(1);
            }
            let f = format!("k{n}");
            ids.push(f.clone());
            c.set_id(&f);
        }
        if let Some(k) = re_sign(doc, tr, &c) {
            out.push(k);
        }
    }
    let lost = keep_len.saturating_sub(out.len());
    let p = drop_dim_params(&params_of(doc), &dropped);
    let mut all_out = others;
    all_out.extend(out.iter().cloned());
    doc.set_parametric(cons_values(&all_out), p.to_json())?;
    Ok((out.len(), dropped.len().saturating_add(lost)))
}

#[allow(dead_code)]
fn _js(_: f64) -> f64 {
    js::round(0.0)
}
