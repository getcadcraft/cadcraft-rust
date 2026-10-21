//! Script ops of the lead's file modules (LandXML …); twins of forks/tools/cvdiff/ops-io.mjs.

use crate::landxml::{self, WAlignment, WNetwork, WStruct, WSurface};
use cc_civil::alignment::Alignment;
use cc_civil::js::{J, arr, numf};
use cc_civil::profile::Profile;
use cc_civil::script::{Call, Registry, keep};
use cc_civil::tin::Surface;
use serde_json::Value;

type R = Option<Result<Value, String>>;

/// A text argument: a string, or `{ ref: name }` = a text an earlier call kept.
pub fn text_arg(reg: &Registry, v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(o) => o.get("ref").and_then(Value::as_str).and_then(|n| reg.get::<String>(n)).cloned().unwrap_or_default(),
        None => String::new(),
    }
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "lx.write" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let units = o.get("units").and_then(Value::as_str).unwrap_or("ft").to_string();
            let name = o.get("name").and_then(Value::as_str).unwrap_or("CivilCraft project").to_string();
            let points = cc_civil::script_ops::read_pts(o.get("points"));
            let surfaces: Vec<WSurface> = arr(o.get("surfaces"))
                .iter()
                .map(|s| WSurface {
                    name: cc_civil::js::string_or_empty(s.get("name")),
                    desc: cc_civil::js::string_or_empty(s.get("desc")),
                    surface: s.get("surface").and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n)),
                    breaklines: arr(s.get("breaklines")).iter().map(cc_civil::script_ops::read_breakline).collect(),
                    boundary: s.get("boundary").filter(|b| b.is_array()).map(|b| cc_civil::geo::read_p2s(Some(b))),
                })
                .collect();
            let alignments: Vec<WAlignment> = arr(o.get("alignments"))
                .iter()
                .map(|a| WAlignment {
                    name: cc_civil::js::string_or_empty(a.get("name")),
                    alignment: a.get("alignment").and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n)),
                    eg: a.get("eg").filter(|e| e.is_object()).map(|e| {
                        let pts = arr(e.get("pts"))
                            .iter()
                            .map(|p| cc_civil::profile::StaZ {
                                sta: numf(p.get("sta")),
                                z: p.get("z").filter(|z| !z.is_null()).map(|z| numf(Some(z))),
                            })
                            .collect();
                        (cc_civil::js::string_or_empty(e.get("name")), pts)
                    }),
                    profile: a.get("profile").filter(|e| e.is_object()).and_then(|e| {
                        e.get("profile")
                            .and_then(Value::as_str)
                            .and_then(|n| reg.get::<Profile>(n))
                            .map(|p| (cc_civil::js::string_or_empty(e.get("name")), p))
                    }),
                })
                .collect();
            let networks: Vec<WNetwork> = arr(o.get("networks"))
                .iter()
                .map(|n| {
                    let net = cc_civil::pipes::read_network(n);
                    let structures = arr(n.get("structures"))
                        .iter()
                        .map(|s| WStruct {
                            id: cc_civil::js::string_or_empty(s.get("id")),
                            name: cc_civil::js::string_or_empty(s.get("name")),
                            x: numf(s.get("x")),
                            y: numf(s.get("y")),
                            dia: numf(s.get("dia")),
                            rim_shown: s.get("rimShown").filter(|v| !v.is_null()).map(|v| numf(Some(v))),
                            sump: s.get("sump").filter(|v| !v.is_null()).map(|v| numf(Some(v))),
                        })
                        .collect();
                    WNetwork { name: net.name, ty: net.ty, structures, pipes: net.pipes }
                })
                .collect();
            // `tz` (minutes east of UTC): the zone of the JS's local date / time fields; the runner's without it
            let zone = o.get("tz").map_or(reg.zone, |v| cc_clock::Zone::from_minutes(Some(numf(Some(v)))));
            let date = zone.local_ms(o.get("date").map_or(0.0, |d| numf(Some(d))));
            let crs = o.get("crs").filter(|c| !c.is_null());
            let text = landxml::write_land_xml(&name, &units, &points, &surfaces, &alignments, &networks, date, crs);
            let j = text.j();
            keep(reg, c, text);
            Some(Ok(j))
        }
        "lx.read" => Some(landxml::read_land_xml(&text_arg(reg, c.raw(0)))),
        "lx.scaleImport" => Some(landxml::read_land_xml(&text_arg(reg, c.raw(0))).map(|d| landxml::scale_import(d, c.num(1)))),
        "lx.num" => Some(Ok(landxml::num(c.num(0), cc_civil::js::usize_of(c.num(1)).unwrap_or(6)).j())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// scene, DXF, DWG, PDF, sheets

use crate::plot::{self, Ext, Scale, Sheet};
use crate::scene::{self, SceneAlignment, SceneCorridor, ScenePad, SceneState, Show};
use cc_civil::geo::read_p2;

/// base64 (btoa).
pub fn b64(b: &[u8]) -> String {
    cc_civil::project::b64(b)
}
/// FNV-1a 32 of bytes as 8 hex digits (the harness's fingerprint of image streams).
pub fn sha16(b: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for &x in b {
        h ^= u32::from(x);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

fn scene_state<'a>(reg: &'a Registry, o: &Value) -> SceneState<'a> {
    let surf = o.get("surface").and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n));
    let contours = o.get("contours").filter(|c| c.is_object()).map(|c| {
        let interval = numf(c.get("interval"));
        let major = c.get("major").map_or(f64::NAN, |m| numf(Some(m)));
        let list = surf
            .map(|s| s.contours(if interval != 0.0 && !interval.is_nan() { interval } else { 1.0 }, 0.0, 2000.0).unwrap_or_default())
            .unwrap_or_default();
        (interval, major, list)
    });
    let breaklines = arr(o.get("breaklines")).iter().map(|b| arr(b.get("pts")).to_vec()).collect();
    let alignments = arr(o.get("alignments"))
        .iter()
        .map(|a| SceneAlignment {
            name: cc_civil::js::string_or_empty(a.get("name")),
            alignment: a.get("alignment").and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n)),
            pis: arr(a.get("pis")).to_vec(),
        })
        .collect();
    let pads = arr(o.get("pads"))
        .iter()
        .map(|p| {
            let (daylight, rays) = match surf {
                Some(s) if p.get("grade") == Some(&Value::Bool(true)) => {
                    let pad = cc_civil::grading::Pad {
                        pts: cc_civil::geo::read_pzs(p.get("pts")),
                        z: numf(p.get("z")),
                        cut: numf(p.get("cut")),
                        fill: numf(p.get("fill")),
                        ..cc_civil::grading::Pad::default()
                    };
                    let dl = cc_civil::grading::daylight_pad(s, &pad);
                    (dl.daylight.iter().map(J::j).collect(), dl.rays.iter().map(|r| (r.x0, r.y0, r.x, r.y, r.d)).collect())
                }
                _ => (Vec::new(), Vec::new()),
            };
            ScenePad {
                name: cc_civil::js::string_or_empty(p.get("name")),
                pts: arr(p.get("pts")).to_vec(),
                z: numf(p.get("z")),
                daylight,
                rays,
            }
        })
        .collect();
    let corridors = arr(o.get("corridors"))
        .iter()
        .filter_map(|c| {
            let s = surf?;
            let al = c.get("alignment").and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n));
            let pr = c.get("profile").and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n));
            let asm = cc_civil::corridor::read_assembly(c.get("asm").unwrap_or(&Value::Null), "ft");
            let cp = cc_civil::corridor::corridor_patch(s, al, pr, &asm);
            Some(SceneCorridor {
                lines: cp.info.lines.iter().map(|(k, v)| (k.clone(), v.iter().map(J::j).collect())).collect(),
                sections: cp.info.sections.iter().map(|sec| (sec.l.iter().map(J::j).collect(), sec.r.iter().map(J::j).collect())).collect(),
            })
        })
        .collect();
    let network = o.get("network").filter(|n| n.is_object()).map(|n| {
        let net = cc_civil::pipes::read_network(n);
        let units = o.get("units").and_then(Value::as_str).unwrap_or("ft");
        let check =
            if n.get("check") == Some(&Value::Bool(false)) { None } else { Some(cc_civil::pipes::analyze_network(&net, surf, units)) };
        (net, check)
    });
    SceneState {
        units: o.get("units").and_then(Value::as_str).unwrap_or("").to_string(),
        points: cc_civil::script_ops::read_pts(o.get("points")),
        surface: surf,
        contours,
        breaklines,
        alignments,
        pads,
        corridors,
        network,
    }
}
fn show_of(v: Option<&Value>) -> Show {
    let b = |k: &str| v.and_then(|o| o.get(k)).and_then(Value::as_bool);
    Show {
        triangles: b("triangles"),
        contours: b("contours"),
        labels: b("labels"),
        breaklines: b("breaklines"),
        outline: b("outline"),
        points: b("points"),
        point_labels: b("pointLabels"),
        slopes: b("slopes"),
        alignments: b("alignments"),
        pads: b("pads"),
        pipes: b("pipes"),
        point_label_text: v.and_then(|o| o.get("pointLabelText")).and_then(Value::as_str).map(str::to_string),
    }
}
fn ext_of(v: Option<&Value>) -> Ext {
    let g = |k: &str| numf(v.and_then(|o| o.get(k)));
    Ext { min_x: g("minX"), min_y: g("minY"), max_x: g("maxX"), max_y: g("maxY") }
}
fn scale_of(v: Option<&Value>) -> Scale {
    Scale { label: cc_civil::js::string_or_empty(v.and_then(|s| s.get("label"))), mm_per_unit: numf(v.and_then(|s| s.get("mmPerUnit"))) }
}
/// The harness's normal form of a sheet (missing parts filled the way the JS shapes mean them).
pub fn sheet_norm(s: &Sheet) -> Value {
    let views: Vec<&plot::View> = match &s.views {
        Some(v) => v.iter().collect(),
        None => vec![&s.single],
    };
    let vs: Vec<Value> = views
        .iter()
        .map(|v| {
            serde_json::json!({
                "clip": { "x0": v.clip.0.j(), "y0": v.clip.1.j(), "x1": v.clip.2.j(), "y1": v.clip.3.j() },
                "strokes": v.strokes.j(), "texts": v.texts.j(),
                "points": v.points.iter().map(|(c, x, y)| serde_json::json!({ "color": c, "x": x.j(), "y": y.j() })).collect::<Vec<_>>(),
                "fills": v.fills.iter().map(|f| serde_json::json!({ "color": f.color, "pts": f.pts.iter().map(|(x, y)| serde_json::json!({ "x": x.j(), "y": y.j() })).collect::<Vec<_>>() })).collect::<Vec<_>>(),
                "images": v.images.len(),
            })
        })
        .collect();
    let mut m = serde_json::Map::new();
    m.insert("w".into(), s.w.j());
    m.insert("h".into(), s.h.j());
    m.insert("name".into(), s.name.j());
    m.insert("views".into(), Value::Array(vs));
    m.insert("chrome".into(), s.chrome.j());
    m.insert("chromeTexts".into(), s.chrome_texts.j());
    for (k, v) in &s.extra {
        m.insert(k.clone(), v.clone());
    }
    Value::Object(m)
}
fn pdf_result(bytes: &[u8], mut report: Value) -> Value {
    let (view, delta) = plot::pdf_view(bytes);
    if let Some(o) = report.as_object_mut()
        && let Some(n) = o.get("size").and_then(Value::as_i64)
    {
        o.insert("size".into(), Value::from(n.saturating_add(delta)));
    }
    serde_json::json!({ "view": view, "report": report })
}

pub fn handle2(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "scene.layers" => Some(Ok(scene::layers_json())),
        "scene.plan" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let st = scene_state(reg, &o);
            let mm = c.arg(1).and_then(|v| v.get("mmPerUnit")).map_or(1.0, |v| numf(Some(v)));
            let items = scene::plan_scene(&st, mm, &show_of(c.arg(1).and_then(|v| v.get("show"))));
            let j = Value::Array(items.clone());
            keep(reg, c, items);
            Some(Ok(j))
        }
        "scene.labelsAlong" => {
            let pts: Vec<(f64, f64)> = arr(c.arg(0)).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect();
            let v: Vec<Value> = scene::labels_along(&pts, c.num(1), c.num(2))
                .into_iter()
                .map(|(x, y, r)| serde_json::json!({ "x": x.j(), "y": y.j(), "rot": r.j() }))
                .collect();
            Some(Ok(Value::Array(v)))
        }
        "dxf.fromScene" => {
            let Some(items) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Vec<Value>>(n)) else {
                return Some(Err("no scene".into()));
            };
            let ents = scene::to_dxf_entities(items, c.num(1));
            let (text, report) = crate::dxf::write_dxf(c.str(2), &crate::dxf::scene_layers(), &ents);
            Some(Ok(serde_json::json!({ "text": text, "report": report })))
        }
        "dxf.write" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let layers: Vec<(String, Value)> = arr(o.get("layers"))
                .iter()
                .map(|l| (cc_civil::js::string_of(l.get("name")), l.get("color").cloned().unwrap_or(Value::Null)))
                .collect();
            let (text, report) =
                crate::dxf::write_dxf(o.get("units").and_then(Value::as_str).unwrap_or("ft"), &layers, arr(o.get("entities")));
            Some(Ok(serde_json::json!({ "text": text, "report": report })))
        }
        "dwg.write" | "dwg.fromScene" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let (layers, entities): (Vec<Value>, Vec<Value>) = if c.op == "dwg.fromScene" {
                let Some(items) = o.get("scene").and_then(Value::as_str).and_then(|n| reg.get::<Vec<Value>>(n)) else {
                    return Some(Err("no scene".into()));
                };
                let ls = scene::LAYERS.iter().map(|(n, _, a, _)| serde_json::json!({ "name": n, "color": a })).collect();
                (ls, scene::to_dxf_entities(items, numf(o.get("mmPerUnit"))))
            } else {
                (arr(o.get("layers")).to_vec(), arr(o.get("entities")).to_vec())
            };
            let images: Vec<crate::dwg_write::Image> = arr(o.get("images"))
                .iter()
                .map(|im| crate::dwg_write::Image {
                    file_name: cc_civil::js::string_or_empty(im.get("fileName")),
                    px_w: numf(im.get("pxW")),
                    px_h: numf(im.get("pxH")),
                    bbox: im
                        .get("box")
                        .filter(|b| b.is_object())
                        .map(|b| (numf(b.get("minX")), numf(b.get("minY")), numf(b.get("maxX")), numf(b.get("maxY")))),
                    layer: im.get("layer").and_then(Value::as_str).map(str::to_string),
                })
                .collect();
            let units = o.get("units").and_then(Value::as_str);
            let version = o.get("version").and_then(Value::as_str).unwrap_or("AC1015");
            match crate::dwg_write::write_dwg(units, &layers, &entities, numf(o.get("created")), version, &images, true) {
                Ok((bytes, report)) => Some(Ok(serde_json::json!({ "bytes": { "$base64": b64(&bytes) }, "report": report }))),
                Err(e) => Some(Err(e)),
            }
        }
        "plot.paperBy" => {
            let p = plot::paper_by(c.str(0));
            Some(Ok(serde_json::json!({ "key": p.key, "name": plot::paper_name(&p), "w": p.w.j(), "h": p.h.j() })))
        }
        "plot.scalesFor" => Some(Ok(plot::scales_for(c.str(0)).j())),
        "plot.fitScale" => {
            let p = plot::paper_by(c.str(2));
            Some(Ok(plot::fit_scale(c.str(0), &ext_of(c.arg(1)), &p, c.arg(3).and_then(Value::as_bool).unwrap_or(true)).j()))
        }
        "plot.sheetWindow" => {
            let o = c.arg(1);
            let e = plot::sheet_window(
                &ext_of(c.arg(0)),
                o.and_then(|o| o.get("paper")).and_then(Value::as_str).unwrap_or("tabloid"),
                o.and_then(|o| o.get("landscape")).and_then(Value::as_bool).unwrap_or(true),
                &scale_of(o.and_then(|o| o.get("scale"))),
            );
            Some(Ok(serde_json::json!({ "minX": e.min_x.j(), "maxX": e.max_x.j(), "minY": e.min_y.j(), "maxY": e.max_y.j() })))
        }
        "plot.planSheet" => {
            let Some(items) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Vec<Value>>(n)).cloned() else {
                return Some(Err("no scene".into()));
            };
            let o = c.arg(1).cloned().unwrap_or(Value::Null);
            let notes: Vec<String> = arr(o.get("notes")).iter().map(|x| cc_civil::js::string_of(Some(x))).collect();
            let hidden: Vec<String> = arr(o.get("hidden")).iter().map(|x| cc_civil::js::string_of(Some(x))).collect();
            let images: Vec<plot::SceneImage> = arr(o.get("images"))
                .iter()
                .map(|im| plot::SceneImage {
                    jpeg: cc_civil::project::unb64(im.get("jpeg").and_then(Value::as_str).unwrap_or("")).unwrap_or_default(),
                    px_w: numf(im.get("pxW")),
                    px_h: numf(im.get("pxH")),
                    bbox: ext_of(im.get("box")),
                })
                .collect();
            let sheet = plot::plan_sheet(
                &items,
                &ext_of(o.get("extents")),
                o.get("paper").and_then(Value::as_str).unwrap_or("tabloid"),
                o.get("landscape").and_then(Value::as_bool).unwrap_or(true),
                &scale_of(o.get("scale")),
                o.get("title").and_then(Value::as_str),
                o.get("units").and_then(Value::as_str).unwrap_or("ft"),
                &notes,
                &hidden,
                &images,
                plot::now_ms(),
            );
            let j = sheet_norm(&sheet);
            keep(reg, c, vec![sheet]);
            Some(Ok(j))
        }
        "plot.toPdf" | "plot.toPdfPages" => {
            let Some(sheets) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Vec<Sheet>>(n)) else {
                return Some(Err("no sheets".into()));
            };
            let o = c.arg(1);
            let mono = o.and_then(|o| o.get("mono")).and_then(Value::as_bool).unwrap_or(false);
            let (bytes, report) = if c.op == "plot.toPdf" {
                match sheets.first() {
                    Some(s) => plot::to_pdf(s, mono, plot::now_ms()),
                    None => return Some(Err("no sheet".into())),
                }
            } else {
                plot::to_pdf_pages(sheets, mono, o.and_then(|o| o.get("title")).and_then(Value::as_str).unwrap_or(""), plot::now_ms())
            };
            Some(Ok(pdf_result(&bytes, report)))
        }
        "plot.winAnsi" => {
            let (s, w, m) = plot::win_ansi(&cc_civil::js::string_or_empty(c.arg(0)));
            Some(Ok(serde_json::json!({ "str": s, "width": w.j(), "missing": m })))
        }
        "plot.arcBeziers" => Some(Ok(plot::arc_beziers(c.num(0), c.num(1), c.num(2), c.num(3), c.num(4)).j())),
        "font.advance" => Some(Ok(f64::from(crate::font::advance(&cc_civil::js::string_or_empty(c.arg(0)))).j())),
        "sheets.plan" => {
            let Some(al) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n)) else {
                return Some(Err("no alignment".into()));
            };
            let o = c.arg(1);
            let g = |k: &str| o.and_then(|o| o.get(k));
            let paper = g("paper").and_then(Value::as_str).unwrap_or("tabloid");
            let landscape = g("landscape").and_then(Value::as_bool).unwrap_or(true);
            let f = |k: &str| g(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            let (length, st) = crate::sheets::sheet_plan(
                al,
                paper,
                landscape,
                &scale_of(g("scale")),
                f("from"),
                f("to"),
                g("units").and_then(Value::as_str).unwrap_or("ft"),
            );
            Some(Ok(
                serde_json::json!({ "length": length.j(), "stations": st.iter().map(|(a, b)| serde_json::json!([a.j(), b.j()])).collect::<Vec<_>>() }),
            ))
        }
        "sheets.pp" => {
            let Some(items) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Vec<Value>>(n)).cloned() else {
                return Some(Err("no scene".into()));
            };
            let o = c.arg(1).cloned().unwrap_or(Value::Null);
            let Some(al) = o.get("alignment").and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n)) else {
                return Some(Err("no alignment".into()));
            };
            let eg: Vec<cc_civil::profile::StaZ> = arr(o.get("eg"))
                .iter()
                .map(|p| cc_civil::profile::StaZ { sta: numf(p.get("sta")), z: p.get("z").filter(|z| !z.is_null()).map(|z| numf(Some(z))) })
                .collect();
            let pr = o.get("profile").and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n));
            let g = |k: &str, d: f64| o.get(k).map_or(d, |v| numf(Some(v)));
            let (sheets, vex, notes, (length, stations)) = crate::sheets::plan_profile_sheets(
                &items,
                al,
                &eg,
                pr,
                o.get("paper").and_then(Value::as_str).unwrap_or("tabloid"),
                o.get("landscape").and_then(Value::as_bool).unwrap_or(true),
                &scale_of(o.get("scale")),
                g("vex", 10.0),
                o.get("units").and_then(Value::as_str).unwrap_or("ft"),
                o.get("title").and_then(Value::as_str).unwrap_or(""),
                o.get("name").and_then(Value::as_str).unwrap_or(""),
                g("from", f64::NAN),
                g("to", f64::NAN),
                plot::now_ms(),
            );
            let j = serde_json::json!({
                "sheets": sheets.iter().map(sheet_norm).collect::<Vec<_>>(), "vex": vex.j(), "notes": notes,
                "plan": { "length": length.j(), "stations": stations.iter().map(|(a, b)| serde_json::json!([a.j(), b.j()])).collect::<Vec<_>>() },
            });
            keep(reg, c, sheets);
            Some(Ok(j))
        }
        "sheets.sections" => {
            let Some(set) = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<cc_civil::sections::SectionSet>(n)) else {
                return Some(Err("no sections".into()));
            };
            let o = c.arg(1).cloned().unwrap_or(Value::Null);
            let g = |k: &str, d: f64| o.get(k).map_or(d, |v| numf(Some(v)));
            let (sheets, cell, per_page, vex, notes) = crate::sheets::section_sheets(
                set,
                o.get("paper").and_then(Value::as_str).unwrap_or("tabloid"),
                o.get("landscape").and_then(Value::as_bool).unwrap_or(true),
                &scale_of(o.get("scale")),
                g("vex", 5.0),
                o.get("units").and_then(Value::as_str).unwrap_or("ft"),
                o.get("title").and_then(Value::as_str).unwrap_or(""),
                o.get("name").and_then(Value::as_str).unwrap_or(""),
                g("from", f64::NEG_INFINITY),
                g("to", f64::INFINITY),
                plot::now_ms(),
            );
            let j = serde_json::json!({
                "sheets": sheets.iter().map(sheet_norm).collect::<Vec<_>>(),
                "cell": cell.map(|(w, h, co, ro)| serde_json::json!({ "w": w.j(), "h": h.j(), "cols": co.j(), "rows": ro.j() })),
                "perPage": per_page.j(), "vex": vex.j(), "notes": notes,
            });
            keep(reg, c, sheets);
            Some(Ok(j))
        }
        "sheets.staking" => {
            let rows: Vec<cc_civil::staking::Row> = arr(c.arg(0)).iter().map(cc_civil::staking::read_row).collect();
            let o = c.arg(1).cloned().unwrap_or(Value::Null);
            let (sheets, per_page) = crate::sheets::staking_sheets(
                &rows,
                o.get("paper").and_then(Value::as_str).unwrap_or("letter"),
                o.get("landscape").and_then(Value::as_bool).unwrap_or(true),
                o.get("units").and_then(Value::as_str).unwrap_or("ft"),
                o.get("title").and_then(Value::as_str).unwrap_or(""),
                o.get("name").and_then(Value::as_str).unwrap_or(""),
                plot::now_ms(),
            );
            let j = serde_json::json!({ "sheets": sheets.iter().map(sheet_norm).collect::<Vec<_>>(), "perPage": per_page.j() });
            keep(reg, c, sheets);
            Some(Ok(j))
        }
        "xml.parse" => Some(crate::xml::parse_xml(&cc_civil::js::string_or_empty(c.arg(0)), 256).map(|e| e.j()).map_err(|e| e.0)),
        "xml.esc" => Some(Ok(crate::xml::esc(&cc_civil::js::string_or_empty(c.arg(0))).j())),
        _ => {
            let _ = read_p2;
            None
        }
    }
}
