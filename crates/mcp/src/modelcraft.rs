//! ModelCraft for assistants — the app's 0.5 `agents.js` (describeModel, boxPolys, cylinderPolys, addSolid,
//! moveInstance, eraseInstances, lengthOf) on cc-m3d's model. Each edit is one `Assistant: …` undo step; solids come in
//! as groups, so they never merge with what the person drew.

use cc_m3d::model::{Model, place};
use cc_m3d::solids::solid_info;
use cc_m3d::vec::IDENT;
use serde_json::{Value, json};

const PREFIX: &str = "Assistant: ";

fn r6(v: f64) -> Value {
    if v.is_finite() { cc_cad::num(cc_cad::js::round(v * 1e6) / 1e6) } else { Value::Null }
}

fn units(model: &Model) -> String {
    if model.units.is_empty() { "m".into() } else { model.units.clone() }
}

/// agents.js `lengthOf(v, unit)`: a number in the display unit, or text the value box takes → metres.
pub fn length_of(v: &Value, unit: &str) -> Result<f64, String> {
    let text = match v {
        Value::Null => String::new(),
        other => cc_cad::js::to_js_string(other),
    };
    match cc_m3d_tools::units::parse_length(&text, unit) {
        Some(m) if m.is_finite() => Ok(m),
        _ => Err(format!("\"{}\" is not a length.", if v.is_null() { "undefined".into() } else { text })),
    }
}

/// The six faces of a w × d × h box from its lowest corner at the origin, outward.
pub fn box_polys(w: f64, d: f64, h: f64) -> Vec<Value> {
    vec![
        json!([[0, 0, 0], [0, d, 0], [w, d, 0], [w, 0, 0]]),
        json!([[0, 0, h], [w, 0, h], [w, d, h], [0, d, h]]),
        json!([[0, 0, 0], [w, 0, 0], [w, 0, h], [0, 0, h]]),
        json!([[w, d, 0], [0, d, 0], [0, d, h], [w, d, h]]),
        json!([[0, d, 0], [0, 0, 0], [0, 0, h], [0, d, h]]),
        json!([[w, 0, 0], [w, d, 0], [w, d, h], [w, 0, h]]),
    ]
}

/// A cylinder of radius r and height h standing on the origin, `sides` facets.
pub fn cylinder_polys(r: f64, h: f64, sides: usize) -> Vec<Value> {
    let sides = sides.clamp(3, 96);
    let ring = |z: f64| -> Vec<[f64; 3]> {
        (0..sides)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let a = (2.0 * std::f64::consts::PI * i as f64) / sides as f64;
                [r * a.cos(), r * a.sin(), z]
            })
            .collect()
    };
    let (lo, hi) = (ring(0.0), ring(h));
    let mut polys = vec![json!(lo.iter().rev().collect::<Vec<_>>()), json!(hi)];
    for i in 0..sides {
        let j = if i.saturating_add(1) >= sides { 0 } else { i.saturating_add(1) };
        if let (Some(a), Some(b), Some(c), Some(d)) = (lo.get(i), lo.get(j), hi.get(j), hi.get(i)) {
            polys.push(json!([a, b, c, d]));
        }
    }
    polys
}

fn translate(p: [f64; 3]) -> [f64; 16] {
    let mut m = IDENT;
    m[12] = p[0];
    m[13] = p[1];
    m[14] = p[2];
    m
}

/// agents.js `addSolid` inside one undo step → {instance, def, closed, volume, undo}.
pub fn add_solid(model: &mut Model, polys: &[Value], name: &str, at: [f64; 3], what: &str) -> Result<Value, String> {
    let label = format!("{PREFIX}{what}");
    let r = model
        .change(&label, |m| {
            let id = cc_m3d::mesh::mesh_def(m, polys, Some(name), "group", &[]);
            let info = m.def(&id).map(solid_info);
            let iid = m.root_mut().map_or(0.0, |root| place(root, &id, &translate(at)));
            Ok((iid, id, info))
        })
        .map_err(|e| e.to_string())?;
    let (iid, id, info) = r;
    Ok(json!({
        "instance": cc_cad::num(iid), "def": id, "closed": info.as_ref().is_some_and(|i| i.closed),
        "volume": info.and_then(|i| i.volume).map_or(Value::Null, cc_cad::num), "undo": label,
    }))
}

/// agents.js `moveInstance` inside one undo step.
pub fn move_instance(model: &mut Model, iid: f64, by: [f64; 3]) -> Result<(), String> {
    model
        .change(&format!("{PREFIX}move"), |m| {
            let root = m.root_mut().ok_or(cc_m3d::error::Error::Msg("no model".into()))?;
            let Some(inst) = root.insts.get_mut(iid) else {
                return Err(cc_m3d::error::Error::Msg(format!(
                    "There is no group or component {} in the model (model_describe lists them).",
                    cc_cad::js::num_to_string(iid)
                )));
            };
            inst.m[12] += by[0];
            inst.m[13] += by[1];
            inst.m[14] += by[2];
            Ok(())
        })
        .map_err(|e| e.to_string())
}

/// agents.js `eraseInstances` inside one undo step.
pub fn erase_instances(model: &mut Model, ids: &[f64]) -> Result<(), String> {
    model
        .change(&format!("{PREFIX}erase"), |m| {
            let root = m.root_mut().ok_or(cc_m3d::error::Error::Msg("no model".into()))?;
            let bad: Vec<String> = ids.iter().filter(|id| !root.insts.has(**id)).map(|id| cc_cad::js::num_to_string(*id)).collect();
            if !bad.is_empty() {
                return Err(cc_m3d::error::Error::Msg(format!("No groups or components {} in the model.", bad.join(", "))));
            }
            for id in ids {
                root.insts.delete(*id);
            }
            Ok(())
        })
        .map_err(|e| e.to_string())
}

/// The assistant's undo (only `Assistant:` steps unless `any`).
pub fn undo(model: &mut Model, steps: u64, any: bool) -> Value {
    let mut undone = Vec::new();
    for _ in 0..steps {
        let Some(top) = model.undo_stack.last() else { break };
        if !any && !top.label.starts_with(PREFIX) {
            break;
        }
        undone.push(model.undo().ok().flatten().map_or(Value::Null, Value::String));
    }
    json!({"undone": undone})
}

/// agents.js `describeModel(model)`.
pub fn describe_model(model: &Model) -> Value {
    let unit = units(model);
    let l = |m: f64| cc_m3d::units::format_length(m, &unit);
    let bb = model.bbox("root", &IDENT);
    let root = model.root();
    let ri = root.map(solid_info);
    let mut counts: Vec<(String, u64)> = Vec::new();
    if let Some(r) = root {
        for inst in r.insts.values() {
            match counts.iter_mut().find(|(d, _)| *d == inst.def) {
                Some((_, n)) => *n = n.saturating_add(1),
                None => counts.push((inst.def.clone(), 1)),
            }
        }
    }
    let instances: Vec<Value> = root
        .map(|r| {
            r.insts
                .iter()
                .take(1000)
                .map(|(id, inst)| {
                    let d = model.def(&inst.def);
                    let si = d.map(solid_info);
                    let name = inst.name.clone().filter(|n| !n.is_empty()).or_else(|| d.map(|d| d.name.clone())).unwrap_or_default();
                    json!({
                        "id": cc_cad::num(id), "kind": d.map(|d| d.kind.clone()), "name": name, "def": inst.def,
                        "at": [r6(inst.m[12]), r6(inst.m[13]), r6(inst.m[14])], "faces": si.as_ref().map_or(0.0, |s| s.faces),
                        "solid": si.as_ref().is_some_and(|s| s.closed), "volumeM3": si.and_then(|s| s.volume).map_or(Value::Null, r6),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let components: Vec<Value> = model
        .defs
        .values()
        .filter(|d| d.kind == "component")
        .map(|d| json!({"def": d.id, "name": d.name, "copies": counts.iter().find(|(k, _)| *k == d.id).map_or(0, |(_, n)| *n)}))
        .collect();
    json!({
        "units": unit,
        "extents": bb.map(|b| json!({"min": b.min.map(r6), "max": b.max.map(r6), "size": [l(b.max[0] - b.min[0]), l(b.max[1] - b.min[1]), l(b.max[2] - b.min[2])]})),
        "loose": {"faces": ri.as_ref().map_or(0.0, |s| s.faces), "edges": ri.as_ref().map_or(0.0, |s| s.edges)},
        "instances": instances,
        "components": components,
        "scenes": model.scenes.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
    })
}

/// `app_info` of ModelCraft's window.
pub fn app_info(model: &Model, allowed: &[String]) -> Value {
    json!({"app": "modelcraft", "title": "ModelCraft", "allowed": allowed, "units": units(model), "instances": model.root().map_or(0, |r| r.insts.len()), "unsaved": model.dirty})
}

/// A point the assistant gave ([x, y] or [x, y, z], each a length) → metres.
pub fn point_of(v: Option<&Value>, unit: &str) -> Result<[f64; 3], String> {
    let Some(a) = v.and_then(Value::as_array) else { return Ok([0.0, 0.0, 0.0]) };
    let get = |i: usize| a.get(i).map_or(Ok(0.0), |x| length_of(x, unit));
    Ok([get(0)?, get(1)?, if a.len() > 2 { get(2)? } else { 0.0 }])
}

/// One of ModelCraft's live tools (agents.js `installAgents`). `view_screenshot` is the window's (it needs the GPU).
pub fn run(model: &mut Model, name: &str, a: &Value, allowed: &[String]) -> Result<Value, String> {
    let unit = units(model);
    match name {
        "app_info" => Ok(app_info(model, allowed)),
        "model_describe" => Ok(describe_model(model)),
        "box_add" => {
            let size = a.get("size").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut s = [0.0; 3];
            for (i, v) in size.iter().take(3).enumerate() {
                if let Some(slot) = s.get_mut(i) {
                    *slot = length_of(v, &unit)?;
                }
            }
            let [w, d, h] = s;
            if !(w > 0.0 && d > 0.0 && h > 0.0) {
                return Err("A box needs a width, depth and height above zero.".into());
            }
            let at = point_of(a.get("at"), &unit)?;
            let nm = a.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or("Box");
            add_solid(model, &box_polys(w, d, h), nm, at, "box")
        }
        "cylinder_add" => {
            let r = length_of(a.get("radius").unwrap_or(&Value::Null), &unit)?;
            let h = length_of(a.get("height").unwrap_or(&Value::Null), &unit)?;
            if !(r > 0.0 && h > 0.0) {
                return Err("A cylinder needs a radius and height above zero.".into());
            }
            let at = point_of(a.get("center"), &unit)?;
            let sides = usize::try_from(a.get("sides").and_then(Value::as_u64).unwrap_or(24)).unwrap_or(24);
            let nm = a.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or("Cylinder");
            add_solid(model, &cylinder_polys(r, h, sides), nm, at, "cylinder")
        }
        "instance_move" => {
            #[allow(clippy::cast_precision_loss)]
            let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as f64;
            let by = point_of(a.get("by"), &unit)?;
            move_instance(model, id, by)?;
            Ok(json!({"moved": cc_cad::num(id), "undo": format!("{PREFIX}move")}))
        }
        "instances_erase" => {
            #[allow(clippy::cast_precision_loss)]
            let ids: Vec<f64> = a
                .get("ids")
                .and_then(Value::as_array)
                .map(|v| v.iter().filter_map(Value::as_u64).map(|x| x as f64).collect())
                .unwrap_or_default();
            erase_instances(model, &ids)?;
            Ok(json!({"erased": ids.len(), "undo": format!("{PREFIX}erase")}))
        }
        "undo" => {
            Ok(undo(model, a.get("steps").and_then(Value::as_u64).unwrap_or(1), a.get("any").and_then(Value::as_bool).unwrap_or(false)))
        }
        other => Err(format!("{other} is not one of ModelCraft's tools.")),
    }
}
