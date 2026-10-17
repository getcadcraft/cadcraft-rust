//! civil-agent.js — CivilCraft's live tools for AI assistants and scripts (docs/AGENTS-SPEC.md §5.2, §6.3): the tool
//! definitions (names, groups, schemas: the same an assistant set up for 0.5 sees) and their `run`. The dispatcher
//! (permissions, schema checks) is the host's; `run` gets arguments that already passed the schema.
//!
//! Tools (project units, easting x / northing y; stations as numbers): `site_describe`, `points_list`,
//! `surface_elevation`, `alignment_station` (read) and `points_select` (edit: the selection only).

use serde_json::{Value, json};

use crate::js::{arr, numf, truthy};
use crate::points::{Pt, group_key};
use crate::project::Project;

pub const LIMIT: usize = 1000;

/// A tool definition: `{name, group, title, description, inputSchema?}`.
pub struct ToolDef {
    pub name: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Option<Value>,
}

fn filters() -> Value {
    json!({
        "group": { "type": "string", "description": "Point group: the first word of the description, upper case (as the Points panel lists them)." },
        "description": { "type": "string", "description": "Text the description contains (case does not matter)." },
        "within": {
            "type": "object", "description": "A box in project coordinates.",
            "properties": { "minX": { "type": "number" }, "minY": { "type": "number" }, "maxX": { "type": "number" }, "maxY": { "type": "number" } },
            "required": ["minX", "minY", "maxX", "maxY"], "additionalProperties": false,
        },
    })
}
fn xy() -> Value {
    json!({ "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } }, "required": ["x", "y"], "additionalProperties": false })
}
fn with_filters(extra: Value) -> Value {
    let mut m = filters().as_object().cloned().unwrap_or_default();
    if let Value::Object(e) = extra {
        m.extend(e);
    }
    Value::Object(m)
}

/// `civilTools(app)` definitions, in order.
pub fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "site_describe",
            group: "read",
            title: "Describe the site",
            description: "The open CivilCraft project: name, units, coordinate system, survey points and their groups, alignments, surfaces, parcels and pipes.",
            input_schema: None,
        },
        ToolDef {
            name: "points_list",
            group: "read",
            title: "List survey points",
            description: "Survey points of the open site (number, easting x, northing y, elevation z, description), filtered and paged.",
            input_schema: Some(json!({
                "type": "object",
                "properties": with_filters(json!({ "offset": { "type": "integer", "minimum": 0 }, "limit": { "type": "integer", "minimum": 1, "maximum": LIMIT } })),
                "additionalProperties": false,
            })),
        },
        ToolDef {
            name: "surface_elevation",
            group: "read",
            title: "Surface elevations",
            description: "Elevation of the existing ground (eg) or the graded surface at given points; null where a point is off the surface.",
            input_schema: Some(json!({
                "type": "object",
                "properties": { "points": { "type": "array", "items": xy(), "minItems": 1, "maxItems": LIMIT }, "surface": { "type": "string", "enum": ["eg", "graded"] } },
                "required": ["points"], "additionalProperties": false,
            })),
        },
        ToolDef {
            name: "alignment_station",
            group: "read",
            title: "Station and offset",
            description: "Station and offset (+ = right) of points along an alignment, or the coordinates of stations on it (with the ground elevation there).",
            input_schema: Some(json!({
                "type": "object", "properties": {
                    "alignment": { "type": "string", "description": "Its name or id; may be left out when the site has one alignment." },
                    "points": { "type": "array", "items": xy(), "maxItems": LIMIT },
                    "stations": { "type": "array", "items": { "type": "number" }, "maxItems": LIMIT },
                }, "additionalProperties": false,
            })),
        },
        ToolDef {
            name: "points_select",
            group: "edit",
            title: "Select survey points",
            description: "Select survey points on the plan (by number or by the points_list filters), to show them to the person or before they copy or delete them. Changes no point.",
            input_schema: Some(json!({
                "type": "object",
                "properties": with_filters(json!({ "numbers": { "type": "array", "items": { "type": "string" }, "maxItems": 100_000 }, "mode": { "type": "string", "enum": ["replace", "add", "remove"] } })),
                "additionalProperties": false,
            })),
        },
    ]
}

/// The definitions as JSON (`[{name, group, title, description, inputSchema?}]`).
pub fn tools_json() -> Value {
    Value::Array(
        tools()
            .into_iter()
            .map(|t| {
                let mut m = serde_json::Map::new();
                m.insert("name".into(), json!(t.name));
                m.insert("group".into(), json!(t.group));
                m.insert("title".into(), json!(t.title));
                m.insert("description".into(), json!(t.description));
                if let Some(s) = t.input_schema {
                    m.insert("inputSchema".into(), s);
                }
                Value::Object(m)
            })
            .collect(),
    )
}

/// `r6(v)`: rounded to 1e-6, null when missing or not finite.
fn r6(v: Option<f64>) -> Value {
    match v {
        Some(v) if v.is_finite() => crate::js::jnum(crate::js::round(v * 1e6) / 1e6),
        _ => Value::Null,
    }
}

/// `filterPoints(points, a)`.
pub fn filter_points<'a>(points: &'a [Pt], a: &Value) -> Vec<&'a Pt> {
    let g = a.get("group").filter(|v| truthy(Some(v))).map(|v| crate::js::upper(crate::js::trim(&crate::js::to_js_string(v))));
    let d = a.get("description").filter(|v| truthy(Some(v))).map(|v| crate::js::lower(&crate::js::to_js_string(v)));
    let w = a
        .get("within")
        .filter(|v| truthy(Some(v)))
        .map(|w| (numf(w.get("minX")), numf(w.get("minY")), numf(w.get("maxX")), numf(w.get("maxY"))));
    points
        .iter()
        .filter(|p| {
            g.as_ref().is_none_or(|g| group_key(&p.d) == *g)
                && d.as_ref().is_none_or(|d| crate::js::lower(&p.d).contains(d.as_str()))
                && w.is_none_or(|w| p.x >= w.0 && p.x <= w.2 && p.y >= w.1 && p.y <= w.3)
        })
        .collect()
}

fn find_al(p: &Project, r: Option<&Value>) -> Result<Value, String> {
    let all = arr(p.state.get("alignments"));
    let names = || all.iter().map(|a| crate::js::string_of(a.get("name"))).collect::<Vec<_>>().join(", ");
    let r = r.filter(|v| !v.is_null() && v.as_str() != Some(""));
    let Some(r) = r else {
        if all.len() == 1 {
            return all.first().cloned().ok_or_default();
        }
        return Err(if all.is_empty() { "The site has no alignment.".into() } else { format!("Say which alignment: {}.", names()) });
    };
    let rs = crate::js::to_js_string(r);
    let found = all
        .iter()
        .find(|x| x.get("id") == Some(r))
        .or_else(|| all.iter().find(|x| crate::js::lower(&crate::js::string_of(x.get("name"))) == crate::js::lower(&rs)));
    match found {
        Some(a) => Ok(a.clone()),
        None => {
            let n = names();
            Err(format!("No alignment \"{rs}\". There are: {}.", if n.is_empty() { "none".to_string() } else { n }))
        }
    }
}

trait OkOrDefault {
    fn ok_or_default(self) -> Result<Value, String>;
}
impl OkOrDefault for Option<Value> {
    fn ok_or_default(self) -> Result<Value, String> {
        self.ok_or_else(|| "The site has no alignment.".to_string())
    }
}

fn stats_json(s: Option<&crate::tin::Surface>) -> Value {
    match s {
        None => Value::Null,
        Some(s) => {
            let x = s.stats();
            json!({ "points": x.points, "triangles": x.triangles, "minZ": r6(Some(x.min_z)), "maxZ": r6(Some(x.max_z)), "area2d": r6(Some(x.area2d)) })
        }
    }
}

/// Run one tool. `select(numbers, mode)` → how many points are selected after it.
pub fn run(name: &str, p: &mut Project, a: &Value, select: &mut dyn FnMut(&[String], &str) -> usize) -> Result<Value, String> {
    match name {
        "site_describe" => {
            let mut groups: Vec<(String, usize)> = Vec::new();
            for pt in &p.points {
                let k = group_key(&pt.d);
                match groups.iter_mut().find(|(g, _)| *g == k) {
                    Some(e) => e.1 = e.1.saturating_add(1),
                    None => groups.push((k, 1)),
                }
            }
            crate::js::sort_by(&mut groups, |x, y| crate::js::locale_compare(&x.0, &y.0));
            let eg = p.eg();
            let g = p.graded();
            let s = p.state.clone();
            let crs = s.get("crs").filter(|c| truthy(Some(c))).map(|c| {
                let or_null = |v: Option<&Value>| v.filter(|x| truthy(Some(x))).cloned().unwrap_or(Value::Null);
                json!({ "name": or_null(c.get("name")), "code": or_null(c.get("code")) })
            });
            let als: Vec<Value> = arr(s.get("alignments"))
                .iter()
                .map(|a| {
                    let al = p.alignment(a);
                    json!({ "id": a.get("id").cloned().unwrap_or(Value::Null), "name": a.get("name").cloned().unwrap_or(Value::Null), "ok": al.ok(), "start": r6(Some(al.start)), "end": r6(Some(al.end)), "length": r6(Some(al.length)) })
                })
                .collect();
            let net = s.get("network").filter(|n| truthy(Some(n))).map(|n| {
                json!({ "name": n.get("name").cloned().unwrap_or(Value::Null), "structures": arr(n.get("structures")).len(), "pipes": arr(n.get("pipes")).len() })
            });
            Ok(json!({
                "name": s.get("name").cloned().unwrap_or(Value::Null),
                "units": s.get("units").cloned().unwrap_or(Value::Null),
                "crs": crs,
                "points": {
                    "count": p.points.len(),
                    "withElevation": p.points.iter().filter(|q| q.z.is_some()).count(),
                    "groups": groups.iter().map(|(g, c)| json!({ "group": g, "count": c })).collect::<Vec<_>>(),
                },
                "alignments": als,
                "surfaces": { "existingGround": stats_json(eg.surface.as_ref()), "graded": stats_json(g.surface.as_deref()) },
                "pads": arr(s.get("pads")).len(),
                "parcels": arr(s.get("parcels")).len(),
                "network": net,
            }))
        }
        "points_list" => {
            let pts = filter_points(&p.points, a);
            let off = a.get("offset").filter(|v| truthy(Some(v))).map_or(0.0, |v| numf(Some(v)));
            let lim = a.get("limit").filter(|v| truthy(Some(v))).map_or(100.0, |v| numf(Some(v)));
            let o = crate::js::usize_of(off.max(0.0)).unwrap_or(0);
            let l = crate::js::usize_of(lim.max(0.0)).unwrap_or(0);
            let page: Vec<Value> = pts
                .iter()
                .skip(o)
                .take(l)
                .map(|q| json!({ "n": q.n, "x": crate::js::jnum(q.x), "y": crate::js::jnum(q.y), "z": q.z.map_or(Value::Null, crate::js::jnum), "d": q.d }))
                .collect();
            Ok(json!({ "total": pts.len(), "offset": crate::js::jnum(off), "points": page }))
        }
        "surface_elevation" => {
            let graded = a.get("surface").and_then(Value::as_str) == Some("graded");
            let eg = p.eg();
            let g = p.graded();
            let sf = if graded { g.surface.as_deref() } else { eg.surface.as_ref() };
            let Some(sf) = sf else {
                return Err(if graded {
                    "The site has no graded surface (no pads or grading yet).".into()
                } else {
                    "The site has no existing-ground surface (no points with elevations).".into()
                });
            };
            let pts: Vec<Value> = arr(a.get("points"))
                .iter()
                .map(|q| {
                    let (x, y) = (numf(q.get("x")), numf(q.get("y")));
                    json!({ "x": q.get("x").cloned().unwrap_or(Value::Null), "y": q.get("y").cloned().unwrap_or(Value::Null), "z": r6(sf.z_at(x, y)) })
                })
                .collect();
            let name = a.get("surface").filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!("eg"));
            Ok(json!({ "surface": name, "points": pts }))
        }
        "alignment_station" => {
            let r = find_al(p, a.get("alignment"))?;
            let al = p.alignment(&r);
            let rname = crate::js::string_of(r.get("name"));
            if !al.ok() {
                let why = al.errors.first().cloned().unwrap_or_else(|| "it needs at least two PIs".into());
                return Err(format!("Alignment \"{rname}\" cannot be built: {why}."));
            }
            let pts = arr(a.get("points"));
            let stas = arr(a.get("stations"));
            if pts.is_empty() && stas.is_empty() {
                return Err("Give points (for their stations) or stations (for their coordinates).".into());
            }
            let eg = p.eg();
            let mut out = serde_json::Map::new();
            out.insert("alignment".into(), r.get("name").cloned().unwrap_or(Value::Null));
            out.insert("start".into(), r6(Some(al.start)));
            out.insert("end".into(), r6(Some(al.end)));
            if !pts.is_empty() {
                let v: Vec<Value> = pts
                    .iter()
                    .map(|q| {
                        let so = al.station_offset(numf(q.get("x")), numf(q.get("y")));
                        json!({
                            "x": q.get("x").cloned().unwrap_or(Value::Null), "y": q.get("y").cloned().unwrap_or(Value::Null),
                            "station": r6(so.as_ref().map(|s| s.sta)), "offset": r6(so.as_ref().map(|s| s.off)), "outside": so.is_some_and(|s| s.outside),
                        })
                    })
                    .collect();
                out.insert("points".into(), Value::Array(v));
            }
            if !stas.is_empty() {
                let v: Vec<Value> = stas
                    .iter()
                    .map(|sv| {
                        let sta = numf(Some(sv));
                        if sta < al.start - 1e-9 || sta > al.end + 1e-9 {
                            return json!({ "station": sv, "outside": true });
                        }
                        let q = al.point_at(sta);
                        let dir = q.as_ref().map(|q| ((90.0 - q.dir * 180.0 / std::f64::consts::PI) % 360.0 + 360.0) % 360.0);
                        json!({
                            "station": sv, "x": r6(q.as_ref().map(|q| q.x)), "y": r6(q.as_ref().map(|q| q.y)), "direction": r6(dir),
                            "ground": match (eg.surface.as_ref(), q.as_ref()) { (Some(s), Some(q)) => r6(s.z_at(q.x, q.y)), (Some(_), None) => Value::Null, _ => Value::Null },
                        })
                    })
                    .collect();
                out.insert("stations".into(), Value::Array(v));
            }
            Ok(Value::Object(out))
        }
        "points_select" => {
            let nums: Vec<String> = if let Some(ns) = a.get("numbers").filter(|v| truthy(Some(v))) {
                let ns: Vec<String> = arr(Some(ns)).iter().map(crate::js::to_js_string).collect();
                let have: std::collections::HashSet<&str> = p.points.iter().map(|q| q.n.as_str()).collect();
                let bad: Vec<&String> = ns.iter().filter(|n| !have.contains(n.as_str())).collect();
                if !bad.is_empty() {
                    let list: Vec<&str> = bad.iter().take(20).map(|s| s.as_str()).collect();
                    return Err(format!(
                        "Unknown point numbers: {}{}. Nothing selected.",
                        list.join(", "),
                        if bad.len() > 20 { " …" } else { "" }
                    ));
                }
                let mut seen = std::collections::HashSet::new();
                ns.into_iter().filter(|n| seen.insert(n.clone())).collect()
            } else if ["group", "description", "within"].iter().any(|k| a.get(*k).is_some_and(|v| truthy(Some(v)))) {
                filter_points(&p.points, a).iter().map(|q| q.n.clone()).collect()
            } else {
                return Err("Say which points: numbers, or group / description / within.".into());
            };
            let mode = a.get("mode").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or("replace");
            let selected = select(&nums, mode);
            Ok(json!({ "matched": nums.len(), "selected": selected }))
        }
        _ => Err(format!("No tool {name}.")),
    }
}
