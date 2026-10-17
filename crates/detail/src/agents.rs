//! agents.js — DetailCraft's live tools for AI assistants and scripts (0.4 part 2 X.12; docs/AGENTS-SPEC.md §5.2, §6.3):
//! the tool definitions (names, groups, titles, descriptions, input schemas: the same an assistant set up for 0.5 sees)
//! and their `run` over the open window ([`AgentApp`]). The dispatcher (permissions, schema checks) is the host's: `run`
//! gets arguments that already passed the schema. Units: millimetres; points [x, y, z], z up.

use crate::build::{Built, build};
use crate::clash::clash_check;
use crate::connections::{find_joints, free_brace_ends, free_splices};
use crate::js::{Res, arr, jnum, num, round, s, string_of, truthy, upper};
use crate::marks::{Marks, mark_footings, mark_parts};
use crate::model::Model;
use crate::state;
use serde_json::{Map, Value, json};

pub const LIMIT: usize = 1000;

/// A tool definition.
pub struct ToolDef {
    pub name: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Option<Value>,
}
impl ToolDef {
    pub fn j(&self) -> Value {
        let mut o = Map::new();
        o.insert("name".into(), Value::from(self.name));
        o.insert("group".into(), Value::from(self.group));
        o.insert("title".into(), Value::from(self.title));
        o.insert("description".into(), Value::from(self.description));
        if let Some(sc) = &self.input_schema {
            o.insert("inputSchema".into(), sc.clone());
        }
        Value::Object(o)
    }
}

/// The window the tools act on: the open model, the selection, and what Save / an export did (the host's part).
pub struct AgentApp {
    pub model: Model,
    pub selected: Vec<Value>,
    pub saves: usize,
    pub exported: Vec<String>,
    cache: Option<(u64, Built, Marks, Vec<(String, String)>)>,
}
impl AgentApp {
    pub fn new(model: Model) -> AgentApp {
        AgentApp { model, selected: vec![], saves: 0, exported: vec![], cache: None }
    }
    /// The build and marks of the current model (made again after any change, as the page's cache does).
    fn ensure(&mut self) -> Res<()> {
        if self.cache.as_ref().is_none_or(|c| c.0 != self.model.version) {
            let b = build(&self.model.state)?;
            let k = mark_parts(&b, 1.0, false);
            let fm = mark_footings(arr(self.model.state.get("footings")), false);
            self.cache = Some((self.model.version, b, k, fm));
        }
        Ok(())
    }
    fn cached(&self) -> Res<&(u64, Built, Marks, Vec<(String, String)>)> {
        self.cache.as_ref().ok_or_else(|| "no build".to_string())
    }
}

fn r1(v: f64) -> Value {
    if v.is_finite() { jnum(round(v * 10.0) / 10.0) } else { Value::Null }
}
fn obj(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}
fn pt() -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 })
}
fn ref_schema() -> Value {
    obj(
        json!({ "kind": { "type": "string", "enum": ["member", "conn", "footing", "stair"] }, "id": { "type": "string" } }),
        &["kind", "id"],
    )
}
/// `COUNT(list, key)` for string keys.
fn count(keys: impl IntoIterator<Item = String>) -> Vec<Value> {
    let mut m: Vec<(String, usize)> = Vec::new();
    for k in keys {
        match m.iter_mut().find(|x| x.0 == k) {
            Some(x) => x.1 = x.1.saturating_add(1),
            None => m.push((k, 1)),
        }
    }
    m.into_iter().map(|(k, n)| json!({ "name": k, "count": n })).collect()
}

/// `modelSummary(model, built, marks)`.
pub fn model_summary(model: &Model, built: &Built, marks: &Marks) -> Value {
    let st = &model.state;
    let len = |k: &str| arr(st.get(k)).len();
    json!({
        "name": st.get("name"), "unsaved": model.dirty(), "members": len("members"), "connections": len("connections"), "footings": len("footings"),
        "stairs": len("stairs"), "looseBolts": len("bolts"), "parts": built.parts.len(), "partMarks": marks.parts.len(), "assemblies": marks.assemblies.len(),
        "bolts": built.bolts.len(), "steelKg": r1(built.parts.iter().fold(0.0, |t, p| t + p.weight)), "problems": built.problems.len(),
    })
}

/// `describeModel(model)`.
pub fn describe_model(model: &Model) -> Value {
    let st = &model.state;
    let mut by_type: Vec<(String, Vec<&Value>)> = Vec::new();
    for m in arr(st.get("members")) {
        let ty = string_of(m.get("type"));
        match by_type.iter_mut().find(|x| x.0 == ty) {
            Some(x) => x.1.push(m),
            None => by_type.push((ty, vec![m])),
        }
    }
    let grids = st.get("grids");
    json!({
        "name": st.get("name"), "units": "mm",
        "grids": {
            "numbered": arr(grids.and_then(|g| g.get("x"))).iter().map(|g| json!({ "label": g.get("label"), "x": g.get("at") })).collect::<Vec<_>>(),
            "lettered": arr(grids.and_then(|g| g.get("y"))).iter().map(|g| json!({ "label": g.get("label"), "y": g.get("at") })).collect::<Vec<_>>(),
        },
        "levels": arr(st.get("levels")).iter().map(|l| json!({ "name": l.get("name"), "z": l.get("z") })).collect::<Vec<_>>(),
        "phases": arr(st.get("phases")).iter().map(|p| json!({ "n": p.get("n"), "name": p.get("name") })).collect::<Vec<_>>(),
        "members": by_type.iter().map(|(ty, list)| json!({ "type": ty, "count": list.len(), "profiles": count(list.iter().map(|m| string_of(m.get("profile")))) })).collect::<Vec<_>>(),
        "connections": count(arr(st.get("connections")).iter().map(|c| string_of(c.get("type")))).into_iter().map(|x| json!({ "type": x.get("name"), "count": x.get("count") })).collect::<Vec<_>>(),
        "footings": arr(st.get("footings")).len(),
        "stairs": arr(st.get("stairs")).iter().map(|x| json!({ "id": x.get("id"), "foot": [x.get("x"), x.get("y"), x.get("z")], "rise": x.get("rise"), "width": x.get("width"), "direction": x.get("dir") })).collect::<Vec<_>>(),
    })
}

/// `partsRows(built, marks, a)`.
pub fn parts_rows(built: &Built, marks: &Marks, a: &Value) -> Value {
    let g = |k: &str| a.get(k).filter(|v| truthy(Some(v)));
    let rows: Vec<&crate::marks::PartRow> = marks
        .parts
        .iter()
        .filter(|p| {
            g("type").is_none_or(|t| t.as_str() == Some(p.kind.as_str()))
                && g("profile").is_none_or(|v| upper(&p.profile) == upper(&string_of(Some(v))))
                && g("mark").is_none_or(|v| p.mark.to_lowercase() == string_of(Some(v)).to_lowercase())
                && g("phase").is_none_or(|v| {
                    let ph = p.ids.first().and_then(|id| built.get(id)).map_or(1.0, |x| crate::js::or(x.phase, 1.0));
                    v.as_f64() == Some(ph)
                })
        })
        .collect();
    let offset = g("offset").map_or(0.0, |v| num(Some(v)));
    let limit = g("limit").map_or(100.0, |v| num(Some(v)));
    let start = cc_civil::js::usize_of(offset.floor()).unwrap_or(0);
    let take = cc_civil::js::usize_of(limit.floor()).unwrap_or(0);
    let parts: Vec<Value> = rows
        .iter()
        .skip(start)
        .take(take)
        .map(|p| {
            let part = p.ids.first().and_then(|id| built.get(id));
            let w = part.map_or(f64::NAN, |x| x.weight);
            let mut asm: Vec<String> = Vec::new();
            for id in &p.ids {
                if let Some(m) = marks.asm_of(id).and_then(|a| marks.asm_mark(a))
                    && !m.is_empty() && !asm.iter().any(|x| x == m) {
                        asm.push(m.to_string());
                    }
            }
            json!({
                "mark": p.mark, "type": p.kind, "profile": p.profile, "material": p.material, "length": r1(p.length), "count": p.count,
                "weightEachKg": r1(w), "weightKg": r1(crate::js::or(part.map_or(0.0, |x| x.weight), 0.0) * p.count as f64),
                "phase": part.map_or(1.0, |x| crate::js::or(x.phase, 1.0)), "assemblies": asm, "ids": p.ids.iter().take(50).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "total": rows.len(), "offset": jnum(offset), "parts": parts })
}

/// `detailTools(app)`: the definitions, in order.
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "app_info",
            group: "read",
            title: "About this window",
            description: "DetailCraft's window: the model open (name, unsaved changes), how many members, connections, footings, stairs, parts, marks and bolts it has, its steel weight, how many problems, and what the person is in the middle of.",
            input_schema: None,
        },
        ToolDef {
            name: "model_describe",
            group: "read",
            title: "Describe the model",
            description: "The open DetailCraft model in millimetres: grids (numbered lines at x, lettered at y), levels (z), phases, members by type and profile, connections by type, footings and stairs.",
            input_schema: None,
        },
        ToolDef {
            name: "parts_list",
            group: "read",
            title: "List part marks",
            description: "The bill of materials by part mark: type, profile, material, cut length (mm), count, weight each and in all (kg, gross), phase, assembly marks and the part ids. Filters: type, profile, mark, phase.",
            input_schema: Some(obj(
                json!({ "type": { "type": "string", "enum": ["beam", "column", "brace", "plate", "stringer"] }, "profile": { "type": "string" }, "mark": { "type": "string" }, "phase": { "type": "integer", "minimum": 1 },
                    "offset": { "type": "integer", "minimum": 0 }, "limit": { "type": "integer", "minimum": 1, "maximum": LIMIT } }),
                &[],
            )),
        },
        ToolDef {
            name: "problems_list",
            group: "read",
            title: "List problems",
            description: "What the Problems tab lists: connections that cannot be built or need a look, holes off their parts, footings without room for their bars, stair checks. Each with the member, connection, footing or stair it is about.",
            input_schema: None,
        },
        ToolDef {
            name: "clash_check",
            group: "read",
            title: "Check clashes",
            description: "The clash check: steel parts and footings that reach into each other by more than the tolerance (mm), leaving out what a connection means to meet. Optionally only some phases.",
            input_schema: Some(obj(
                json!({ "tolerance": { "type": "number", "minimum": 0, "maximum": 50 }, "phases": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "maxItems": 999 } }),
                &[],
            )),
        },
        ToolDef {
            name: "selection_get",
            group: "read",
            title: "What is selected",
            description: "What the person has selected in DetailCraft, each described (member: type, profile, mark, ends; connection: type and members; footing; stair).",
            input_schema: None,
        },
        ToolDef {
            name: "selection_set",
            group: "edit",
            title: "Select",
            description: "Select members, connections, footings or stairs to show them to the person: by refs ({kind, id}) or by part marks (every member with that mark). Changes nothing in the model.",
            input_schema: Some(obj(
                json!({ "refs": { "type": "array", "items": ref_schema(), "maxItems": 100000 }, "marks": { "type": "array", "items": { "type": "string" }, "maxItems": 1000 }, "mode": { "type": "string", "enum": ["replace", "add", "remove"] } }),
                &[],
            )),
        },
        ToolDef {
            name: "member_add",
            group: "edit",
            title: "Add a member",
            description: "Add a beam, column, brace or plate between two points (mm). Profile by name (catalog, e.g. W16x26, IPE300, HSS4x4x1/4, or PL20x200 for a plate); position top|middle|bottom puts the reference line on the top of steel, the middle or the bottom; rotation in degrees about its axis. One undo step.",
            input_schema: Some(obj(
                json!({ "type": { "type": "string", "enum": ["beam", "column", "brace", "plate"] }, "start": pt(), "end": pt(), "profile": { "type": "string" }, "material": { "type": "string" },
                    "position": { "type": "string", "enum": ["top", "middle", "bottom"] }, "rotation": { "type": "number" }, "name": { "type": "string", "maxLength": 40 }, "phase": { "type": "integer", "minimum": 1 } }),
                &["type", "start", "end"],
            )),
        },
        ToolDef {
            name: "connection_add",
            group: "edit",
            title: "Add connections",
            description: "Connect where the given members meet, as the Connect buttons do: endplate or sheartab (at every free end of the beams that meets a column), baseplate (under each column), splice (where the pieces meet another of their profile end to end), gusset (at every brace end on a column or beam). Default parameters; one undo step. Returns the new connection ids and any problems they report.",
            input_schema: Some(obj(
                json!({ "type": { "type": "string", "enum": ["endplate", "sheartab", "baseplate", "splice", "gusset"] }, "members": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 1000 } }),
                &["type", "members"],
            )),
        },
        ToolDef {
            name: "undo",
            group: "edit",
            title: "Undo",
            description: "Undo the assistant's last steps (labelled \"Assistant: …\"); steps the person made are only undone with any: true.",
            input_schema: Some(obj(
                json!({ "steps": { "type": "integer", "minimum": 1, "maximum": 50 }, "any": { "type": "boolean" } }),
                &[],
            )),
        },
        ToolDef {
            name: "model_save",
            group: "save",
            title: "Save the model",
            description: "DetailCraft's own Save: the model as .detailcraft.json (desktop: in place; browser: downloaded).",
            input_schema: None,
        },
        ToolDef {
            name: "model_export",
            group: "export",
            title: "Export",
            description: "One of DetailCraft's own exports, downloaded as the Output buttons do: ifc (IFC4), nc (DSTV NC files, zipped), bom (CSV), bending (bending schedule CSV), kiss (KISS file), cutlist (cut list CSV).",
            input_schema: Some(obj(
                json!({ "format": { "type": "string", "enum": ["ifc", "nc", "bom", "bending", "kiss", "cutlist"] } }),
                &["format"],
            )),
        },
    ]
}

fn ref_ok(st: &Value, r: &Value) -> bool {
    let id = r.get("id");
    let key = match r.get("kind").and_then(Value::as_str) {
        Some("member") => "members",
        Some("conn") => "connections",
        Some("footing") => "footings",
        _ => "stairs",
    };
    arr(st.get(key)).iter().any(|x| crate::model::strict_eq(x.get("id"), id))
}
fn find<'a>(st: &'a Value, key: &str, id: Option<&Value>) -> Option<&'a Value> {
    arr(st.get(key)).iter().find(|x| crate::model::strict_eq(x.get("id"), id))
}

impl AgentApp {
    fn describe_ref(&mut self, r: &Value) -> Res<Value> {
        let st = self.model.state.clone();
        let (_, _, mk, fm) = {
            self.ensure()?;
            self.cached()?
        };
        let mut o = r.as_object().cloned().unwrap_or_default();
        let id = r.get("id");
        let ids = id.and_then(Value::as_str).unwrap_or("");
        match r.get("kind").and_then(Value::as_str) {
            Some("member") => {
                let m = find(&st, "members", id).cloned().unwrap_or(Value::Null);
                o.insert("type".into(), m.get("type").cloned().unwrap_or(Value::Null));
                o.insert("profile".into(), m.get("profile").cloned().unwrap_or(Value::Null));
                o.insert("mark".into(), mk.part_mark(ids).filter(|x| !x.is_empty()).map_or(Value::Null, Value::from));
                o.insert("start".into(), m.get("start").cloned().unwrap_or(Value::Null));
                o.insert("end".into(), m.get("end").cloned().unwrap_or(Value::Null));
            }
            Some("conn") => {
                let c = find(&st, "connections", id).cloned().unwrap_or(Value::Null);
                o.insert("type".into(), c.get("type").cloned().unwrap_or(Value::Null));
                let ms: Vec<Value> = ["beam", "column", "a", "b", "brace", "support"]
                    .iter()
                    .filter_map(|k| c.get(*k))
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .collect();
                o.insert("members".into(), Value::Array(ms));
            }
            Some("footing") => {
                let f = find(&st, "footings", id).cloned().unwrap_or(Value::Null);
                let mark = fm.iter().find(|(k, _)| k == ids).map(|(_, v)| v.clone()).filter(|x| !x.is_empty());
                o.insert("mark".into(), mark.map_or(Value::Null, Value::from));
                o.insert("center".into(), json!([f.get("x"), f.get("y"), f.get("top")]));
                o.insert("size".into(), json!([f.get("L"), f.get("B"), f.get("H")]));
            }
            _ => {
                let x = find(&st, "stairs", id).cloned().unwrap_or(Value::Null);
                o.insert("foot".into(), json!([x.get("x"), x.get("y"), x.get("z")]));
                o.insert("rise".into(), x.get("rise").cloned().unwrap_or(Value::Null));
                o.insert("width".into(), x.get("width").cloned().unwrap_or(Value::Null));
            }
        }
        Ok(Value::Object(o))
    }
    fn by_mark(&mut self, mark: &Value) -> Res<Vec<Value>> {
        let want = string_of(Some(mark)).to_lowercase();
        let st_members = arr(self.model.state.get("members")).to_vec();
        let (_, _, mk, _) = {
            self.ensure()?;
            self.cached()?
        };
        let p = mk
            .parts
            .iter()
            .find(|x| x.mark.to_lowercase() == want)
            .ok_or_else(|| format!("No part mark \"{}\".", string_of(Some(mark))))?;
        Ok(p.ids
            .iter()
            .filter(|id| st_members.iter().any(|m| s(m.get("id")) == id.as_str()))
            .map(|id| json!({ "kind": "member", "id": id }))
            .collect())
    }

    /// Runs one tool with arguments that passed its schema → its result, or the error the dispatcher reports.
    pub fn run(&mut self, name: &str, a: &Value) -> Res<Value> {
        match name {
            "app_info" => {
                let (_, b, k, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                let sum = model_summary(&self.model, b, k);
                let mut o = json!({ "app": "detailcraft", "title": "DetailCraft", "busy": null }).as_object().cloned().unwrap_or_default();
                if let Some(m) = sum.as_object() {
                    o.extend(m.clone());
                }
                Ok(Value::Object(o))
            }
            "model_describe" => Ok(describe_model(&self.model)),
            "parts_list" => {
                let (_, b, k, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                Ok(parts_rows(b, k, a))
            }
            "problems_list" => {
                let (_, b, _, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                let ps: Vec<Value> = b
                    .problems
                    .iter()
                    .map(|p| {
                        let about = match &p.r#ref {
                            Some(r) if !r.is_empty() => {
                                let kind = match r.chars().next() {
                                    Some('m') => "member",
                                    Some('j') => "conn",
                                    Some('f') => "footing",
                                    Some('s') => "stair",
                                    _ => "other",
                                };
                                json!({ "kind": kind, "id": r })
                            }
                            _ => Value::Null,
                        };
                        json!({ "text": p.text, "hard": p.hard == Some(true), "about": about })
                    })
                    .collect();
                Ok(json!({ "problems": ps }))
            }
            "clash_check" => {
                let st = self.model.state.clone();
                let tol = a.get("tolerance").filter(|v| !v.is_null()).map_or(1.0, |v| num(Some(v)));
                let phases: Option<Vec<f64>> = a.get("phases").and_then(Value::as_array).map(|p| p.iter().map(|x| num(Some(x))).collect());
                let (_, b, mk, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                let r = clash_check(&st, b, tol, phases.as_deref());
                let side = |r: &Value, part: &str| {
                    let mut o = r.as_object().cloned().unwrap_or_default();
                    o.insert("part".into(), Value::from(mk.part_mark(part).filter(|x| !x.is_empty()).unwrap_or(part)));
                    Value::Object(o)
                };
                let cl: Vec<Value> = r
                    .0
                    .iter()
                    .take(LIMIT)
                    .map(|c| json!({ "a": side(&c.a, &c.a_part), "b": side(&c.b, &c.b_part), "depth": jnum(c.depth), "at": crate::js::p3v(c.at), "kind": if c.concrete { "concrete" } else { "steel" } }))
                    .collect();
                Ok(json!({ "checked": r.2, "clashes": cl, "total": r.0.len() }))
            }
            "selection_get" => {
                let st = self.model.state.clone();
                let sel: Vec<Value> = self.selected.iter().take(LIMIT).filter(|r| ref_ok(&st, r)).cloned().collect();
                let mut out = Vec::new();
                for r in &sel {
                    out.push(self.describe_ref(r)?);
                }
                Ok(json!({ "selected": out }))
            }
            "selection_set" => {
                let st = self.model.state.clone();
                let mut refs: Vec<Value> = Vec::new();
                let has_refs = a.get("refs").is_some_and(|v| truthy(Some(v)));
                let has_marks = a.get("marks").is_some_and(|v| truthy(Some(v)));
                if has_refs {
                    let given = arr(a.get("refs"));
                    let bad: Vec<&Value> = given.iter().filter(|r| !ref_ok(&st, r)).collect();
                    if !bad.is_empty() {
                        let list: Vec<String> =
                            bad.iter().take(10).map(|r| format!("{} {}", string_of(r.get("kind")), string_of(r.get("id")))).collect();
                        return Err(format!("Unknown: {}. Nothing selected.", list.join(", ")));
                    }
                    refs = given.to_vec();
                }
                if has_marks {
                    for mk in arr(a.get("marks")) {
                        let more = self.by_mark(mk)?;
                        refs.extend(more);
                    }
                }
                if !has_refs && !has_marks {
                    return Err("Say what to select: refs or marks.".into());
                }
                let key = |r: &Value| format!("{}:{}", string_of(r.get("kind")), string_of(r.get("id")));
                let next = match a.get("mode").and_then(Value::as_str) {
                    Some("add") => {
                        let mut v = self.selected.clone();
                        v.extend(refs);
                        v
                    }
                    Some("remove") => self.selected.iter().filter(|r| !refs.iter().any(|q| key(q) == key(r))).cloned().collect(),
                    _ => refs,
                };
                self.selected = next;
                Ok(json!({ "selected": self.selected.len() }))
            }
            "member_add" => {
                let label = format!("Assistant: add {}", string_of(a.get("type")));
                let input = a.clone();
                let id = self.model.change(&label, |tx| tx.add_member(Some(&input)))?;
                let ids = id.as_str().unwrap_or("").to_string();
                let (_, _, k, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                let mark = k.part_mark(&ids).filter(|x| !x.is_empty()).map_or(Value::Null, Value::from);
                Ok(json!({ "id": id, "mark": mark }))
            }
            "connection_add" => {
                let st = self.model.state.clone();
                let ty = s(a.get("type")).to_string();
                let members: Vec<String> = arr(a.get("members")).iter().map(|v| string_of(Some(v))).collect();
                let ms = state::members(&st);
                let cs = state::conns(&st);
                let bad: Vec<&String> = members.iter().filter(|id| !ms.iter().any(|m| m.id == **id)).collect();
                if !bad.is_empty() {
                    return Err(format!("Unknown members: {}.", bad.iter().take(10).map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
                }
                let todo: Vec<Value> = if ty == "endplate" || ty == "sheartab" {
                    let taken: Vec<String> = cs
                        .iter()
                        .filter(|c| c.is_beam_end())
                        .map(|c| format!("{}:{}", c.beam.as_deref().unwrap_or("undefined"), c.end))
                        .collect();
                    find_joints(&ms)
                        .into_iter()
                        .filter(|j| {
                            members.contains(&j.beam)
                                && !taken.contains(&format!("{}:{}", j.beam, j.end))
                                && (ty == "sheartab" || j.face != "skew")
                        })
                        .map(|j| json!({ "type": ty, "beam": j.beam, "end": j.end, "column": j.column }))
                        .collect()
                } else if ty == "baseplate" {
                    members
                        .iter()
                        .filter(|id| {
                            ms.iter().find(|m| m.id == **id).is_some_and(|m| m.kind == "column")
                                && !cs.iter().any(|c| c.kind == "baseplate" && c.column.as_deref() == Some(id.as_str()))
                        })
                        .map(|id| json!({ "type": "baseplate", "column": id }))
                        .collect()
                } else if ty == "splice" {
                    free_splices(&ms, &cs, Some(&members))
                        .into_iter()
                        .map(|j| json!({ "type": "splice", "a": j.a, "aEnd": j.a_end, "b": j.b }))
                        .collect()
                } else {
                    free_brace_ends(&ms, &cs, Some(&members))
                        .into_iter()
                        .map(|j| json!({ "type": "gusset", "brace": j.brace, "end": j.end, "support": j.support }))
                        .collect()
                };
                if todo.is_empty() {
                    return Err(format!("Nowhere to add a {ty} on those members (no free joint of that kind)."));
                }
                let label = format!("Assistant: add {ty}{}", if todo.len() > 1 { "s" } else { "" });
                let ids = self.model.change(&label, |tx| {
                    let mut out = Vec::new();
                    for c in &todo {
                        out.push(tx.add_connection(Some(c))?);
                    }
                    Ok(out)
                })?;
                let (_, b, _, _) = {
                    self.ensure()?;
                    self.cached()?
                };
                let probs: Vec<Value> = b
                    .problems
                    .iter()
                    .filter(|p| p.r#ref.as_ref().is_some_and(|r| ids.iter().any(|i| i.as_str() == Some(r.as_str()))))
                    .map(|p| json!({ "connection": p.r#ref, "text": p.text, "hard": p.hard == Some(true) }))
                    .collect();
                Ok(json!({ "added": ids, "problems": probs }))
            }
            "undo" => {
                let steps = a.get("steps").filter(|v| truthy(Some(v))).map_or(1.0, |v| num(Some(v)));
                let any = truthy(a.get("any"));
                let mut undone: Vec<Value> = Vec::new();
                let mut i = 0.0;
                while i < steps && i < 1000.0 {
                    let Some(top) = self.model.undo_stack.last() else { break };
                    if !any && !top.label.starts_with("Assistant: ") {
                        if undone.is_empty() {
                            return Err(format!("The last step is the person's (\"{}\"): pass any: true to undo it.", top.label));
                        }
                        break;
                    }
                    undone.push(self.model.undo().map_or(Value::Null, Value::from));
                    i += 1.0;
                }
                Ok(json!({ "undone": undone }))
            }
            "model_save" => {
                self.saves = self.saves.saturating_add(1);
                self.model.mark_saved();
                Ok(json!({ "saved": self.model.state.get("name"), "unsaved": self.model.dirty() }))
            }
            "model_export" => {
                let f = string_of(a.get("format"));
                self.exported.push(f.clone());
                Ok(json!({ "exported": f }))
            }
            _ => Err(format!("unknown tool {name}")),
        }
    }
}
