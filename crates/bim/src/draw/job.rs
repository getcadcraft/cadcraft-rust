//! A parity job for the building tools and the IFC editor (`cadcraft-cli bimdraw <job.json>`; forks/tools/bmuidiff.mjs
//! runs the same steps on the frozen JS): open a file or make a new building, then steps — the tools' changes as the
//! page makes them (model-ui.js), undo / redo, attribute and property edits, snapshots, saves. Elements are named by
//! their index in the snapshot taken just before the step (levels by elevation, walls / doors in reading order), so both
//! sides resolve the same GlobalIds. Results: one JSON value per step.

use std::sync::Arc;

use serde_json::{Map, Value as J, json};

use crate::draw::editor::{Change, Editor, PropRef, iso_seconds};
use crate::draw::levels::LevelChange;
use crate::draw::m::{self, V2};
use crate::draw::newmodel::{NewBuilding, NewLevel, new_building};
use crate::draw::openings::{NewOpening, OpeningChange};
use crate::draw::ops::{Op, SegSpec, Snapshot, loops_of, snapshot};
use crate::draw::roof::{format_edges, preset_edges, roof_defaults};
use crate::draw::rooms::loop_at;
use crate::draw::slabs::{DerivedChange, NewRoof};
use crate::draw::tr::t0;
use crate::draw::walls::{WallChange, type_name, type_thickness, wall_type};
use crate::js::jn;
use crate::wapi::Rand;

fn num(v: Option<&J>) -> Option<f64> {
    v.and_then(J::as_f64)
}
fn s(v: Option<&J>) -> Option<String> {
    v.and_then(J::as_str).map(str::to_string)
}
fn pt(v: Option<&J>) -> Option<V2> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}
fn err(e: &str) -> J {
    json!({ "$error": e })
}
fn p2(p: V2) -> J {
    J::Array(vec![jn(p[0]), jn(p[1])])
}

/// The snapshot as the harness compares it.
pub fn snapshot_json(sn: &Snapshot) -> J {
    json!({
        "levels": sn.levels.iter().map(|l| json!({ "name": l.name, "elevation": jn(l.elevation), "height": jn(l.height), "own": l.own })).collect::<Vec<_>>(),
        "walls": sn.walls.iter().map(|w| json!({ "a": p2(w.a), "b": p2(w.b), "len": jn(w.len), "t": jn(w.t), "height": jn(w.height), "typeKey": w.type_key, "extSide": w.ext_side, "baseOffset": jn(w.base_offset), "topOffset": jn(w.top_offset), "name": w.name })).collect::<Vec<_>>(),
        "openings": sn.openings.iter().map(|o| json!({ "kind": o.kind(), "typeKey": o.type_key, "offset": jn(o.offset), "width": jn(o.width), "height": jn(o.height), "sill": jn(o.sill), "hand": o.hand, "face": o.face, "mark": o.mark })).collect::<Vec<_>>(),
        "spaces": sn.derived.spaces.iter().map(|x| json!({ "number": x.number, "name": x.name, "seed": p2(x.seed) })).collect::<Vec<_>>(),
        "floors": sn.derived.floors.iter().map(|x| json!({ "mode": x.mode, "kind": x.kind, "thickness": jn(x.thickness), "offset": jn(x.offset), "seed": p2(x.seed) })).collect::<Vec<_>>(),
        "ceilings": sn.derived.ceilings.iter().map(|x| json!({ "mode": x.mode, "height": jn(x.height), "thickness": jn(x.thickness), "seed": p2(x.seed) })).collect::<Vec<_>>(),
        "roofs": sn.derived.roofs.iter().map(|x| json!({ "pitch": jn(x.pitch), "overhang": jn(x.overhang), "thickness": jn(x.thickness), "edges": x.edges, "parts": x.parts.len(), "seed": p2(x.seed) })).collect::<Vec<_>>(),
    })
}

/// One property set's rows: (set name, [(property name, its edit ref when it is a single value)]).
pub type PropSet = (String, Vec<(String, Option<PropRef>)>);

fn pts_json(p: &[V2]) -> J {
    J::Array(p.iter().map(|q| p2(*q)).collect())
}
fn segs_json(s: &[[f64; 4]]) -> J {
    J::Array(s.iter().map(|q| J::Array(q.iter().map(|v| jn(*v)).collect())).collect())
}
fn flat_json(s: &[f64]) -> J {
    J::Array(s.iter().map(|v| jn(*v)).collect())
}

/// The plan scene as the harness compares it.
pub fn scene_json(sc: &crate::draw::plan::Scene) -> J {
    json!({
        "walls": sc.walls.iter().map(|w| json!({ "key": jn(w.key), "guid": w.guid, "poly": pts_json(&w.poly), "pieces": w.pieces.iter().map(|p| pts_json(p)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "symbols": sc.symbols.iter().map(|s| json!({
            "key": jn(s.key), "guid": s.guid, "lines": segs_json(&s.sym.lines), "thin": segs_json(&s.sym.thin), "dashed": segs_json(&s.sym.dashed), "poly": pts_json(&s.sym.poly), "roof": s.roof,
            "arcs": s.sym.arcs.iter().map(|a| json!({ "c": p2(a.c), "r": jn(a.r), "a0": jn(a.a0), "a1": jn(a.a1) })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "rooms": sc.rooms.iter().map(|r| json!({ "key": jn(r.key), "poly": pts_json(&r.poly), "at": p2(r.at), "label": r.label })).collect::<Vec<_>>(),
        "dims": sc.dims.iter().map(|d| json!({ "a": p2(d.a), "b": p2(d.b), "text": d.text, "offPx": d.off_px.map(jn), "offMm": d.off_mm.map(jn), "key": d.key.map(jn) })).collect::<Vec<_>>(),
        "cut": sc.cut.iter().map(|c| json!({ "key": jn(c.key), "segs": flat_json(&c.segs), "loops": c.loops.iter().map(|l| flat_json(l)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "below": sc.below.iter().map(|c| json!({ "key": jn(c.key), "segs": flat_json(&c.segs) })).collect::<Vec<_>>(),
        "wallEdges": sc.wall_edges.as_ref().map(|e| e.iter().map(|p| pts_json(p)).collect::<Vec<_>>()),
    })
}

/// The plan scene of a level from the saved bytes (the page cuts the rebuilt model; keyBase 0).
pub fn plan_scene(
    ed: &mut Editor,
    sn: &Snapshot,
    bytes: &[u8],
    level: f64,
    units: &str,
    dims: bool,
) -> Result<crate::draw::plan::Scene, String> {
    use crate::draw::plan::{CUT, cut_scene, make_scene, own_keys, plan_cut, viewer_to_plan};
    let f = cc_io_ifc::model::IfcFile::open(bytes).map_err(|e| e.0)?;
    f.parse_all();
    let model = crate::model::load(&f, &cc_io_ifc::geo::bool_ops::ManifoldOps, 0.0).map_err(|e| e.0)?;
    let sw = {
        let mut w = crate::draw::writer::W::new(&mut ed.api).map_err(|e| e.0)?;
        w.storey_world(level).map_err(|e| e.0)?
    };
    let mm = viewer_to_plan(&model.coord, &sw).ok_or("singular")?;
    let own = own_keys(sn, 0.0);
    let c = plan_cut(&model, &mm, level, CUT, &own, 50.0, true);
    let (cut, below) = cut_scene(&c);
    let loops = loops_of(&mut ed.api, level).map_err(|e| e.0)?;
    let mut other = |st: f64| loops_of(&mut ed.api, st).ok();
    Ok(make_scene(sn, level, &loops.loops, &cut, &below, units, dims, 0.0, &mut other))
}

/// The editable property rows of an element (ifc-model.js `properties()` sets, with their `edit` refs), by set name.
pub fn prop_refs(ed: &Editor, id: f64) -> Vec<PropSet> {
    let f = &ed.api.f;
    let mut out: Vec<PropSet> = Vec::new();
    let mut sets: Vec<(f64, bool)> = Vec::new();
    if let Ok(rels) = f.inverse("IFCRELDEFINESBYPROPERTIES", "RelatedObjects") {
        for &r in rels.get(&cc_io_ifc::model::id_key(id)).map(Vec::as_slice).unwrap_or(&[]) {
            match f.attr(r, "RelatingPropertyDefinition").ok().flatten() {
                Some(cc_io_ifc::value::Value::Ref(p)) => sets.push((*p, false)),
                Some(cc_io_ifc::value::Value::List(l)) => {
                    sets.extend(l.iter().filter_map(cc_io_ifc::value::Value::as_ref_id).map(|p| (p, false)))
                }
                _ => {}
            }
        }
    }
    if let Ok(rels) = f.inverse("IFCRELDEFINESBYTYPE", "RelatedObjects")
        && let Some(&r) = rels.get(&cc_io_ifc::model::id_key(id)).and_then(|v| v.first())
        && let Some(t) = f.attr(r, "RelatingType").ok().flatten().and_then(cc_io_ifc::value::Value::as_ref_id)
        && let Ok(Some(tl)) = ed.api.line(t)
    {
        for h in tl.a("HasPropertySets").hrefs() {
            if f.has(h) {
                sets.push((h, true));
            }
        }
    }
    for (sid, from_type) in sets {
        let Ok(Some(sl)) = ed.api.line(sid) else { continue };
        let name = sl.a("Name").text_or_empty();
        let set_name = if name.is_empty() { "Properties".to_string() } else { name };
        let mut rows = Vec::new();
        for h in sl.a("HasProperties").hrefs() {
            let Ok(Some(p)) = ed.api.line(h) else { continue };
            let nv = p.a("NominalValue");
            let single = !matches!(nv, crate::wapi::JV::Null | crate::wapi::JV::L(_)) && p.get("NominalValue").is_some();
            let pname = p.a("Name").val();
            let pname = if matches!(pname, crate::wapi::P::Null) { String::new() } else { pname.string() };
            rows.push((
                pname.clone(),
                single.then(|| PropRef {
                    pset_id: sid,
                    prop_id: h,
                    from_type,
                    set_name: set_name.clone(),
                    prop_name: pname,
                    value_type: nv.tname().map(str::to_string),
                }),
            ));
        }
        for h in sl.a("Quantities").hrefs() {
            let Ok(Some(q)) = ed.api.line(h) else { continue };
            rows.push((q.a("Name").val().string(), None));
        }
        if !rows.is_empty() {
            out.push((format!("{set_name}{}", if from_type { " (type)" } else { "" }), rows));
        }
    }
    out.sort_by(|a, b| crate::js::locale_cmp_plain(&a.0, &b.0));
    out
}

fn level_guid(sn: &Snapshot, v: Option<&J>) -> Option<String> {
    let i = usize::try_from(v?.as_u64()?).ok()?;
    sn.levels.get(i)?.guid.clone()
}
fn wall_guid(sn: &Snapshot, v: Option<&J>) -> Option<String> {
    let i = usize::try_from(v?.as_u64()?).ok()?;
    Some(sn.walls.get(i)?.guid.clone())
}
fn opening_guid(sn: &Snapshot, v: Option<&J>) -> Option<String> {
    let i = usize::try_from(v?.as_u64()?).ok()?;
    Some(sn.openings.get(i)?.guid.clone())
}
fn derived_guid(sn: &Snapshot, v: &J) -> Option<String> {
    let o = v.as_object()?;
    let ix = |k: &str| o.get(k).and_then(J::as_u64).and_then(|x| usize::try_from(x).ok());
    if let Some(i) = ix("space") {
        return sn.derived.spaces.get(i).map(|x| x.guid.clone());
    }
    if let Some(i) = ix("floor") {
        return sn.derived.floors.get(i).map(|x| x.guid.clone());
    }
    if let Some(i) = ix("ceiling") {
        return sn.derived.ceilings.get(i).map(|x| x.guid.clone());
    }
    if let Some(i) = ix("roof") {
        return sn.derived.roofs.get(i).map(|x| x.guid.clone());
    }
    None
}

fn wall_change(sn: &Snapshot, c: &Map<String, J>) -> WallChange {
    WallChange {
        a: pt(c.get("a")),
        b: pt(c.get("b")),
        type_key: s(c.get("typeKey")),
        base_offset: num(c.get("baseOffset")),
        top_level: match c.get("topLevel") {
            Some(J::String(x)) => Some(x.clone()),
            Some(J::Number(_)) => Some(level_guid(sn, c.get("topLevel")).unwrap_or_default()),
            _ => None,
        },
        top_offset: num(c.get("topOffset")),
        height: num(c.get("height")),
        ext_side: s(c.get("extSide")),
    }
}

/// The result of a change as the harness compares it.
fn change_json(r: &crate::draw::ops::OpResult) -> J {
    json!({ "ok": true, "ids": r.ids.iter().map(|x| jn(*x)).collect::<Vec<_>>(), "made": r.made.len() })
}

/// The job runner's state.
pub struct Run {
    pub ed: Option<Editor>,
    rand: Option<Rand>,
    now: f64,
}

impl Run {
    pub fn new(seed: u32, now: f64) -> Run {
        Run { ed: None, rand: Some(Rand::new(seed)), now }
    }

    fn model(&mut self, op: Op, label: &str, verb: &str) -> J {
        let Some(ed) = self.ed.as_mut() else { return err("no model") };
        match ed.model_change(op, label, verb) {
            Ok(r) => change_json(&r),
            Err(e) => err(&e.0),
        }
    }

    /// One step → its result.
    pub fn step(&mut self, st: &J) -> J {
        let op = st.get("op").and_then(J::as_str).unwrap_or("");
        if op == "open" || op == "new" {
            let bytes: Vec<u8> = if op == "open" {
                match std::fs::read(st.get("file").and_then(J::as_str).unwrap_or("")) {
                    Ok(b) => b,
                    Err(e) => return err(&e.to_string()),
                }
            } else {
                let levels: Vec<NewLevel> = st
                    .get("levels")
                    .and_then(J::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|l| NewLevel {
                                name: s(l.get("name")).unwrap_or_default(),
                                elevation: num(l.get("elevation")).unwrap_or(0.0),
                                height: num(l.get("height")).unwrap_or(0.0),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let nb = NewBuilding {
                    name: s(st.get("name")).unwrap_or_default(),
                    site_name: t0("new.site"),
                    levels,
                    author: String::new(),
                    file_name: s(st.get("fileName")).unwrap_or_else(|| "building.ifc".into()),
                    now_ms: self.now,
                    version: String::new(),
                };
                let Some(r) = self.rand.as_mut() else { return err("no seed") };
                match new_building(&nb, r) {
                    Ok(t) => t.into_bytes(),
                    Err(k) => return err(k),
                }
            };
            let rand = self.rand.take().unwrap_or_else(|| Rand::new(0));
            match Editor::open(Arc::from(bytes.into_boxed_slice()), 0) {
                Ok(mut e) => {
                    e.api.rand = rand;
                    e.is_new = op == "new";
                    self.ed = Some(e);
                    return json!({ "ok": true });
                }
                Err(e) => return err(&e.0),
            }
        }
        let Some(ed) = self.ed.as_mut() else { return err("no model") };
        let sn = match snapshot(&mut ed.api) {
            Ok(x) => x,
            Err(e) => return err(&e.0),
        };
        let o = st.as_object().cloned().unwrap_or_default();
        let ch = |k: &str| o.get(k).and_then(J::as_object).cloned().unwrap_or_default();
        match op {
            "snapshot" => snapshot_json(&sn),
            "save" => {
                let ts = iso_seconds(self.now);
                match ed.save(&ts) {
                    Ok(b) => {
                        if let Some(out) = s(o.get("out")) {
                            let _ = std::fs::write(out, &b);
                        }
                        json!({ "bytes": b.len(), "digest": crate::digest::bytes(&b) })
                    }
                    Err(e) => err(&e.0),
                }
            }
            "undo" => match ed.undo() {
                Ok(Some(c)) => json!({ "undid": if c.label().is_empty() { J::Null } else { J::String(c.label().into()) } }),
                Ok(None) => json!({ "undid": J::Null }),
                Err(e) => err(&e.0),
            },
            "redo" => match ed.redo() {
                Ok(Some(c)) => json!({ "redid": if c.label().is_empty() { J::Null } else { J::String(c.label().into()) } }),
                Ok(None) => json!({ "redid": J::Null }),
                Err(e) => err(&e.0),
            },
            "setAttribute" => {
                let id = num(o.get("id")).unwrap_or(0.0);
                let v = s(o.get("value"));
                match ed.set_attribute(id, &s(o.get("attr")).unwrap_or_default(), v.as_deref(), true) {
                    Ok(b) => json!({ "changed": b }),
                    Err(e) => err(&e.0),
                }
            }
            "setProperty" => {
                let id = num(o.get("id")).unwrap_or(0.0);
                let (set, prop) = (s(o.get("set")).unwrap_or_default(), s(o.get("prop")).unwrap_or_default());
                let refs = prop_refs(ed, id);
                let hit = refs.iter().find(|x| x.0 == set).and_then(|x| x.1.iter().find(|r| r.0 == prop)).and_then(|r| r.1.clone());
                let Some(r) = hit else { return err("no such property") };
                match ed.set_property(id, &r, &s(o.get("value")).unwrap_or_default(), true) {
                    Ok(b) => json!({ "changed": b }),
                    Err(e) => err(&e.0),
                }
            }
            "drawWalls" => {
                let Some(storey) = level_guid(&sn, o.get("level")) else { return err("no level") };
                let segs = o
                    .get("segs")
                    .and_then(J::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| {
                                Some(SegSpec { a: pt(x.get("a"))?, b: pt(x.get("b"))?, type_key: None, ext_side: None, guid: None })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let top = level_guid(&sn, o.get("topLevel")).unwrap_or_default();
                let opx = Op::DrawWalls {
                    storey,
                    segs,
                    type_key: s(o.get("typeKey")).unwrap_or_else(|| "ext-2x6".into()),
                    base_offset: num(o.get("baseOffset")).unwrap_or(0.0),
                    top_level: top,
                    top_offset: num(o.get("topOffset")).unwrap_or(0.0),
                    height: num(o.get("height")).unwrap_or(0.0),
                    ext_side: s(o.get("extSide")).unwrap_or_else(|| "left".into()),
                };
                self.model(opx, "walls", "Drew")
            }
            "chain" => self.chain(&sn, &o),
            "changeWall" => {
                let g = wall_guid(&sn, o.get("wall")).unwrap_or_default();
                let c = wall_change(&sn, &ch("changes"));
                self.model(Op::ChangeWall { guid: g, changes: c, carry_joined: true }, &t0("model.wall"), &t0("model.verb.changed"))
            }
            "changeWalls" => {
                let gs: Vec<String> = o
                    .get("walls")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| wall_guid(&sn, Some(x))).collect())
                    .unwrap_or_default();
                let c = wall_change(&sn, &ch("changes"));
                self.model(Op::ChangeWalls { guids: gs, changes: c }, &t0("model.wall"), &t0("model.verb.changed"))
            }
            "removeWalls" => {
                let gs: Vec<String> = o
                    .get("walls")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| wall_guid(&sn, Some(x))).collect())
                    .unwrap_or_default();
                self.model(Op::RemoveWalls { guids: gs }, "", "")
            }
            "placeOpening" => {
                let g = wall_guid(&sn, o.get("wall")).unwrap_or_default();
                let spec = NewOpening {
                    wall: g,
                    type_key: s(o.get("typeKey")).unwrap_or_default(),
                    offset: num(o.get("offset")),
                    width: num(o.get("width")).unwrap_or(0.0),
                    height: num(o.get("height")).unwrap_or(0.0),
                    sill: num(o.get("sill")),
                    hand: s(o.get("hand")).unwrap_or_else(|| "a".into()),
                    face: s(o.get("face")).unwrap_or_else(|| "left".into()),
                    guid: None,
                    opening_guid: None,
                };
                self.model(Op::PlaceOpening(spec), "", &t0("model.verb.placed"))
            }
            "changeOpening" | "changeOpenings" => {
                let c = ch("changes");
                let oc = OpeningChange {
                    offset: num(c.get("offset")),
                    width: num(c.get("width")),
                    height: num(c.get("height")),
                    sill: num(c.get("sill")),
                    type_key: s(c.get("typeKey")),
                    hand: s(c.get("hand")),
                    face: s(c.get("face")),
                    wall: wall_guid(&sn, c.get("wall")),
                };
                if op == "changeOpening" {
                    let g = opening_guid(&sn, o.get("opening")).unwrap_or_default();
                    self.model(Op::ChangeOpening { guid: g, changes: oc }, "", &t0("model.verb.changed"))
                } else {
                    let gs: Vec<String> = o
                        .get("openings")
                        .and_then(J::as_array)
                        .map(|a| a.iter().filter_map(|x| opening_guid(&sn, Some(x))).collect())
                        .unwrap_or_default();
                    self.model(Op::ChangeOpenings { guids: gs, changes: oc }, "", &t0("model.verb.changed"))
                }
            }
            "removeElements" => {
                let walls: Vec<String> = o
                    .get("walls")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| wall_guid(&sn, Some(x))).collect())
                    .unwrap_or_default();
                let openings: Vec<String> = o
                    .get("openings")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| opening_guid(&sn, Some(x))).collect())
                    .unwrap_or_default();
                let derived: Vec<String> = o
                    .get("derived")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| derived_guid(&sn, x)).collect())
                    .unwrap_or_default();
                self.model(Op::RemoveElements { walls, openings, derived }, "", &t0("model.verb.deleted"))
            }
            "placeFloor" => {
                let Some(storey) = level_guid(&sn, o.get("level")) else { return err("no level") };
                let kind = s(o.get("kind")).unwrap_or_else(|| "wood".into());
                let opx = Op::PlaceFloor {
                    storey,
                    seed: pt(o.get("seed")).unwrap_or([0.0, 0.0]),
                    mode: s(o.get("mode")).unwrap_or_else(|| "room".into()),
                    thickness: num(o.get("thickness"))
                        .unwrap_or_else(|| crate::draw::slabs::FLOOR_KINDS.iter().find(|k| k.0 == kind).map_or(0.0, |k| k.1)),
                    kind,
                    offset: num(o.get("offset")).unwrap_or(0.0),
                    guid: None,
                };
                self.model(opx, "", &t0("model.verb.placed"))
            }
            "placeCeiling" => {
                let Some(storey) = level_guid(&sn, o.get("level")) else { return err("no level") };
                let opx = Op::PlaceCeiling {
                    storey,
                    seed: pt(o.get("seed")).unwrap_or([0.0, 0.0]),
                    mode: s(o.get("mode")).unwrap_or_else(|| "room".into()),
                    height: num(o.get("height")).unwrap_or(2438.4),
                    thickness: num(o.get("thickness")).unwrap_or(12.7),
                    guid: None,
                };
                self.model(opx, "", &t0("model.verb.placed"))
            }
            "placeRoof" => {
                let Some(li) = o.get("level").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()) else { return err("no level") };
                let Some(lv) = sn.levels.get(li).cloned() else { return err("no level") };
                let seed = pt(o.get("seed")).unwrap_or([0.0, 0.0]);
                let loops = match loops_of(&mut ed.api, lv.id) {
                    Ok(l) => l,
                    Err(e) => return err(&e.0),
                };
                let Some(outline) = loop_at(&loops.loops.outlines, seed) else { return err("model.err.nobuilding2") };
                let walls: Vec<_> = sn.walls.iter().filter(|w| w.storey == Some(lv.id)).cloned().collect();
                let ends = s(o.get("ends")).unwrap_or_else(|| "hip".into());
                let Some(edges) = preset_edges(&outline.poly, &walls, &ends, Some(seed)).map(|mm| format_edges(&mm)) else {
                    return err("model.err.gable4");
                };
                let d = roof_defaults();
                let fascia = o.get("fascia").and_then(J::as_bool).unwrap_or(true);
                let spec = NewRoof {
                    seed,
                    pitch: num(o.get("pitch")).unwrap_or_else(|| m::atan(6.0 / 12.0) * 180.0 / std::f64::consts::PI),
                    overhang: num(o.get("overhang")).unwrap_or(304.8),
                    edges,
                    thickness: d.thickness,
                    fascia: if fascia { d.fascia } else { 0.0 },
                    soffit: fascia,
                    ..NewRoof::default()
                };
                self.model(Op::PlaceRoof { storey: lv.guid.clone().unwrap_or_default(), spec }, &t0("model.roof"), &t0("model.verb.placed"))
            }
            "changeDerived" => {
                let gs: Vec<String> = o
                    .get("targets")
                    .and_then(J::as_array)
                    .map(|a| a.iter().filter_map(|x| derived_guid(&sn, x)).collect())
                    .unwrap_or_default();
                let c = ch("changes");
                let dc = DerivedChange {
                    thickness: num(c.get("thickness")),
                    offset: num(c.get("offset")),
                    kind: s(c.get("kind")),
                    height: num(c.get("height")),
                    pitch: num(c.get("pitch")),
                    overhang: num(c.get("overhang")),
                    gables: c.get("gables").and_then(J::as_array).map(|a| a.iter().filter_map(J::as_f64).collect()),
                    name: s(c.get("name")),
                    number: s(c.get("number")),
                    edges: s(c.get("edges")),
                    rake: num(c.get("rake")),
                    plate: num(c.get("plate")),
                    fascia: num(c.get("fascia")),
                    soffit: c.get("soffit").and_then(J::as_bool),
                };
                self.model(Op::ChangeDerived { guids: gs, changes: dc }, "", &t0("model.verb.changed"))
            }
            "newLevel" => {
                let opx = Op::NewLevel {
                    name: s(o.get("name")).unwrap_or_default(),
                    elevation: num(o.get("elevation")).unwrap_or(0.0),
                    height: num(o.get("height")).unwrap_or(3000.0),
                    guid: None,
                };
                self.model(opx, "", &t0("model.verb.added"))
            }
            "changeLevel" => {
                let Some(g) = level_guid(&sn, o.get("level")) else { return err("no level") };
                let c = ch("changes");
                let lc = LevelChange {
                    name: s(c.get("name")),
                    elevation: num(c.get("elevation")),
                    height: num(c.get("height")),
                    shift_above: c.get("shiftAbove").and_then(J::as_bool),
                };
                self.model(Op::ChangeLevel { guid: g, changes: lc }, "", &t0("model.verb.changed"))
            }
            "plan" => {
                let Some(li) = o.get("level").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()) else { return err("no level") };
                let Some(lv) = sn.levels.get(li).cloned() else { return err("no level") };
                let units = s(o.get("units")).unwrap_or_else(|| "ftin".to_string());
                let ts = iso_seconds(self.now);
                let bytes = match ed.save(&ts) {
                    Ok(b) => b,
                    Err(e) => return err(&e.0),
                };
                let scene = match plan_scene(ed, &sn, &bytes, lv.id, &units, o.get("dims").and_then(J::as_bool).unwrap_or(true)) {
                    Ok(x) => x,
                    Err(e) => return err(&e),
                };
                let base = s(o.get("base")).unwrap_or_else(|| "building".into());
                let when = iso_seconds(self.now).get(..10).unwrap_or("").to_string();
                let lines = vec![
                    base,
                    crate::draw::tr::t("model.print.cut", &[("h", &crate::draw::units::fmt_len(crate::draw::plan::CUT, &units))]),
                    when,
                ];
                let title = crate::draw::tr::t("model.print.titleof", &[("level", &lv.name)]);
                let mut print = scene.clone();
                for d in &mut print.dims {
                    d.off_px = None;
                }
                let pdf = crate::draw::pdf::plan_pdf(&print, &title, &lines, if units == "mm" { "a4" } else { "letter" }, &units, None);
                if let Some(out) = s(o.get("out")) {
                    let _ = std::fs::write(out, &pdf.bytes);
                }
                json!({ "scene": scene_json(&scene), "pdf": { "bytes": pdf.bytes.len(), "digest": crate::digest::bytes(&pdf.bytes), "scale": pdf.label, "missing": pdf.missing } })
            }
            "schedule" => {
                let units = s(o.get("units")).unwrap_or_else(|| "ftin".to_string());
                let mut loops = std::collections::HashMap::new();
                for l in &sn.levels {
                    if sn.derived.spaces.iter().any(|x| x.storey == Some(l.id))
                        && let Ok(lp) = loops_of(&mut ed.api, l.id)
                    {
                        loops.insert(crate::draw::plan::key_u64(l.id), lp.loops);
                    }
                }
                let all = crate::draw::schedule::schedules(&sn, &loops, &units);
                let which = s(o.get("which")).unwrap_or_else(|| "doors".into());
                let Some(sc) = all.get(&which) else { return err("no such schedule") };
                let mut csv = "\u{feff}".to_string();
                csv.push_str(&crate::draw::schedule::schedule_csv(sc));
                if let Some(out) = s(o.get("out")) {
                    let _ = std::fs::write(out, csv.as_bytes());
                }
                json!({ "head": sc.head, "rows": sc.rows, "total": sc.total, "nums": sc.nums.iter().map(|r| r.iter().map(|v| jn(*v)).collect::<Vec<_>>()).collect::<Vec<_>>(), "csv": crate::digest::bytes(csv.as_bytes()) })
            }
            "bench" => {
                // timings for forks/tools/bmbench.mjs (not compared): the model loaded from the edited bytes (what
                // the 3D view and the plan read), the plan of a level, an attribute change and Save
                let li = o.get("level").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
                let level = sn.levels.get(li).map(|l| l.id);
                let ts = iso_seconds(self.now);
                let t0 = std::time::Instant::now();
                let bytes = match ed.save(&ts) {
                    Ok(b) => b,
                    Err(e) => return err(&e.0),
                };
                let save0 = t0.elapsed().as_secs_f64() * 1000.0;
                let t1 = std::time::Instant::now();
                let f = match cc_io_ifc::model::IfcFile::open(&bytes) {
                    Ok(f) => f,
                    Err(e) => return err(&e.0),
                };
                f.parse_all();
                let model = match crate::model::load(&f, &cc_io_ifc::geo::bool_ops::ManifoldOps, 0.0) {
                    Ok(m) => m,
                    Err(e) => return err(&e.0),
                };
                let load_ms = t1.elapsed().as_secs_f64() * 1000.0;
                let t2 = std::time::Instant::now();
                let view = cc_io_ifc::view::view(&bytes, &cc_io_ifc::geo::bool_ops::ManifoldOps).map(|v| v.pieces.len()).unwrap_or(0);
                let view_ms = t2.elapsed().as_secs_f64() * 1000.0;
                let mut plan_ms = f64::NAN;
                let mut cut = 0usize;
                if let Some(level) = level {
                    let t3 = std::time::Instant::now();
                    let sw = {
                        let mut w = match crate::draw::writer::W::new(&mut ed.api) {
                            Ok(w) => w,
                            Err(e) => return err(&e.0),
                        };
                        w.storey_world(level).unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
                    };
                    if let Some(mm) = crate::draw::plan::viewer_to_plan(&model.coord, &sw) {
                        let own = crate::draw::plan::own_keys(&sn, 0.0);
                        let c = crate::draw::plan::plan_cut(&model, &mm, level, crate::draw::plan::CUT, &own, 50.0, true);
                        let (cu, be) = crate::draw::plan::cut_scene(&c);
                        cut = cu.len().saturating_add(be.len());
                        if let Ok(loops) = loops_of(&mut ed.api, level) {
                            let api = &mut ed.api;
                            let mut other = |st: f64| loops_of(api, st).ok();
                            let _ = crate::draw::plan::make_scene(&sn, level, &loops.loops, &cu, &be, "mm", true, 0.0, &mut other);
                        }
                    }
                    plan_ms = t3.elapsed().as_secs_f64() * 1000.0;
                }
                let id = num(o.get("id")).unwrap_or(0.0);
                let t4 = std::time::Instant::now();
                let _ = ed.set_attribute(id, "Name", Some("Bench"), true);
                let saved = ed.save(&ts).map(|b| b.len()).unwrap_or(0);
                let edit_save_ms = t4.elapsed().as_secs_f64() * 1000.0;
                json!({ "save0_ms": save0, "load_ms": load_ms, "view_ms": view_ms, "view_pieces": view, "plan_ms": jn(plan_ms), "plan_items": cut, "edit_save_ms": edit_save_ms, "bytes": saved })
            }
            "changes" => J::Array(
                ed.changes
                    .iter()
                    .map(|c| match c {
                        Change::Model { label, verb, .. } => json!({ "kind": "model", "label": label, "verb": verb }),
                        Change::Attr { attr, .. } => json!({ "kind": "attr", "attr": attr }),
                        Change::Prop { r, .. } => json!({ "kind": "prop", "prop": r.prop_name }),
                    })
                    .collect(),
            ),
            other => err(&format!("unknown step {other}")),
        }
    }

    /// model-ui.js `wallPoint` for each point of a chain (one change per wall; the closing wall turns every wall of the
    /// chain's outside to the outside of the loop, in the same change).
    fn chain(&mut self, sn: &Snapshot, o: &Map<String, J>) -> J {
        let Some(li) = o.get("level").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()) else { return err("no level") };
        let Some(lv) = sn.levels.get(li).cloned() else { return err("no level") };
        let above = sn.levels.get(li.wrapping_add(1)).cloned();
        let type_key = s(o.get("typeKey")).unwrap_or_else(|| "ext-2x6".into());
        let Some(ty) = wall_type(&type_key) else { return err("model.err.walltype") };
        let top_mode = s(o.get("top")).unwrap_or_else(|| "level".into());
        let fixed_h = num(o.get("height")).unwrap_or(2438.4);
        let pts_in: Vec<V2> =
            o.get("points").and_then(J::as_array).map(|a| a.iter().filter_map(|x| pt(Some(x))).collect()).unwrap_or_default();
        let close = o.get("close").and_then(J::as_bool).unwrap_or(false);
        let mut pts: Vec<V2> = Vec::new();
        let mut guids: Vec<String> = Vec::new();
        let mut results = Vec::new();
        let mut all: Vec<V2> = pts_in.clone();
        if close && let Some(f) = pts_in.first() {
            all.push(*f);
        }
        for (k, q) in all.iter().enumerate() {
            if k == 0 {
                pts.push(*q);
                continue;
            }
            let Some(&a) = pts.last() else { continue };
            if m::hypot(q[0] - a[0], q[1] - a[1]) < 1.0 {
                continue;
            }
            let closing = pts.len() >= 3 && pts.first().is_some_and(|f| m::dist(*f, *q) <= 1.0);
            let b = if closing { pts.first().copied().unwrap_or(*q) } else { *q };
            let (top_level, height) = if top_mode == "level" && above.is_some() {
                (above.as_ref().and_then(|x| x.guid.clone()).unwrap_or_default(), 0.0)
            } else {
                (String::new(), if top_mode == "fixed" { fixed_h } else { lv.height })
            };
            let side = if closing { if crate::draw::tools::loop_area(&pts) > 0.0 { "right" } else { "left" } } else { "left" };
            let flips = if closing { guids.clone() } else { Vec::new() };
            pts.push(b);
            let run = Op::DrawWalls {
                storey: lv.guid.clone().unwrap_or_default(),
                segs: vec![SegSpec { a, b, type_key: None, ext_side: None, guid: None }],
                type_key: type_key.clone(),
                base_offset: 0.0,
                top_level,
                top_offset: 0.0,
                height,
                ext_side: side.into(),
            };
            let opx = if closing && !flips.is_empty() {
                Op::Seq {
                    ops: vec![
                        run,
                        Op::ChangeWalls {
                            guids: flips.clone(),
                            changes: WallChange { ext_side: Some(side.into()), ..WallChange::default() },
                        },
                    ],
                    keep_first: true,
                }
            } else {
                run
            };
            let _ = type_thickness(&ty);
            let Some(ed) = self.ed.as_mut() else { return err("no model") };
            match ed.model_change(opx, &type_name(&ty), &t0("model.verb.drew")) {
                Ok(r) => {
                    for (g, _) in &r.made {
                        guids.push(g.clone());
                    }
                    results.push(change_json(&r));
                }
                Err(e) => {
                    pts.pop();
                    results.push(err(&e.0));
                }
            }
            if closing {
                break;
            }
        }
        J::Array(results)
    }
}

/// Run a whole job → `{ results: [...] }`.
pub fn run(job: &J) -> J {
    let seed = job.get("seed").and_then(J::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(1);
    let now = job.get("now").and_then(J::as_f64).unwrap_or(0.0);
    let mut r = Run::new(seed, now);
    let steps = job.get("steps").and_then(J::as_array).cloned().unwrap_or_default();
    let results: Vec<J> = steps.iter().map(|s| r.step(s)).collect();
    json!({ "results": results })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new building, a closed room of four walls, undo and redo: the saved files are the ones the frozen 0.5 JS saves
    /// for the same steps (digests from forks/tools/bmuidiff.mjs, job room-undo-redo).
    #[test]
    fn a_room_saves_as_the_js_saves_it() {
        let levels =
            json!([{ "name": "Level 1", "elevation": 0, "height": 3000 }, { "name": "Level 2", "elevation": 3000, "height": 3000 }]);
        let job = json!({
            "seed": 7, "now": 1_791_979_200_000_u64,
            "steps": [
                { "op": "new", "name": "Parity house", "levels": levels },
                { "op": "chain", "level": 0, "points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "close": true, "typeKey": "ext-2x6", "top": "level" },
                { "op": "snapshot" }, { "op": "save" },
                { "op": "undo" }, { "op": "snapshot" }, { "op": "redo" }, { "op": "snapshot" }, { "op": "save" },
            ]
        });
        let out = run(&job);
        let r = out.get("results").and_then(J::as_array).cloned().unwrap_or_default();
        assert_eq!(r.get(3).and_then(|x| x.get("digest")).and_then(|d| d.get("h")).and_then(J::as_str), Some("23d7694fcec1803a"), "{out}");
        assert_eq!(r.get(8).and_then(|x| x.get("digest")).and_then(|d| d.get("h")).and_then(J::as_str), Some("a854754ae3fb66e3"));
        let walls = r.get(2).and_then(|x| x.get("walls")).and_then(J::as_array).map_or(0, Vec::len);
        assert_eq!(walls, 4);
        assert_eq!(r.get(5).and_then(|x| x.get("walls")).and_then(J::as_array).map_or(9, Vec::len), 3, "undo takes back the closing wall");
    }

    /// Hostile steps never panic: unknown ops, missing references, NaN and huge numbers.
    #[test]
    fn hostile_steps_are_refused() {
        let job = json!({
            "seed": 1, "now": 0,
            "steps": [
                { "op": "drawWalls", "level": 0 },
                { "op": "new", "name": "x", "levels": [{ "name": "L", "elevation": 0, "height": 10 }] },
                { "op": "new", "name": "x", "levels": [{ "name": "L", "elevation": 0, "height": 3000 }] },
                { "op": "drawWalls", "level": 5, "segs": [{ "a": [0, 0], "b": [1, 1] }] },
                { "op": "drawWalls", "level": 0, "segs": [{ "a": [0, 0], "b": [1e300, -1e300] }], "typeKey": "int-100" },
                { "op": "placeOpening", "wall": 7, "typeKey": "door-single" },
                { "op": "placeRoof", "level": 0, "seed": [0, 0] },
                { "op": "setProperty", "id": 1e18, "set": "x", "prop": "y", "value": "z" },
                { "op": "setAttribute", "id": -3, "attr": "Name", "value": "z" },
                { "op": "nonsense" },
                { "op": "undo" }, { "op": "undo" }, { "op": "redo" }, { "op": "save" }, { "op": "plan", "level": 0 },
            ]
        });
        let out = run(&job);
        assert_eq!(out.get("results").and_then(J::as_array).map(Vec::len), Some(15));
    }
}
