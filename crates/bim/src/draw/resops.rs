//! The residential tools' own changes (0.5 part 4, track RC; res-ui.js's Build menu), one op each as the JS writes them:
//! res-foundation.js `placeFoundation` / `changeFoundation` / `removeFoundations`, res-counter.js `placeCountertop` /
//! `changeCountertop` / `fitCountertop` / `removeCountertops`, res-stair.js `placeStair` / `changeStair` /
//! `removeStairs`, res-railing.js (drawn railings), res-roofplane.js (manual roof planes), res-sheets.js's section lines,
//! res-elec.js `placeDevice` / `changeDevice` / `linkLight` / `removeDevices`, res-framing.js `frameHosts` /
//! `changeFraming` / `removeFraming` / `keepPiece`, and the lumber list.
//!
//! An op is named by the JS function and takes its arguments as JSON (`ResOp { name, args }`), read the way the JS reads
//! them (`Number.isFinite`, defaults spread under the spec); GlobalIds are fixed into the arguments the first time it
//! runs, so a redo makes the same elements again.

use cc_io_ifc::model::Id;
use serde_json::{Value as J, json};

use crate::api::R;
use crate::draw::levels::read_levels;
use crate::draw::m::{self, V2};
use crate::draw::tr::{t, t0};
use crate::draw::walls::read_walls;
use crate::draw::writer::{QRow, Row, W, key_err, round_d, round6, row};
use crate::util::{apply4, inv4, mul4};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;
const D2R: f64 = std::f64::consts::PI / 180.0;

/// A residential op: the JS function's name and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct ResOp {
    pub name: String,
    pub args: Vec<J>,
}

type Out = (Option<Id>, Vec<Id>, Vec<(String, Id)>);

fn fail(key: &str) -> crate::api::JsErr {
    key_err(key)
}
/// `Number.isFinite(o[k])`: a JSON number only.
fn nf(o: &J, k: &str) -> Option<f64> {
    o.get(k).and_then(J::as_f64).filter(|v| v.is_finite())
}
fn st(o: &J, k: &str) -> Option<String> {
    o.get(k).and_then(J::as_str).map(str::to_string)
}
fn pt(v: Option<&J>) -> Option<V2> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}
fn pts(v: Option<&J>) -> Option<Vec<V2>> {
    let a = v?.as_array()?;
    a.iter().map(|q| pt(Some(q))).collect()
}
fn guids(v: Option<&J>) -> Vec<String> {
    v.and_then(J::as_array).map(|a| a.iter().filter_map(J::as_str).map(str::to_string).collect()).unwrap_or_default()
}
/// The spec's GlobalId, fixed into it the first time.
fn fix_guid(w: &mut W<'_>, spec: &mut J) -> String {
    if let Some(g) = spec.get("guid").and_then(J::as_str).filter(|g| !g.is_empty()) {
        return g.to_string();
    }
    let g = w.guid().val_str().unwrap_or_default();
    if let Some(o) = spec.as_object_mut() {
        o.insert("guid".into(), J::String(g.clone()));
    }
    g
}
fn storey_by_guid(w: &mut W<'_>, guid: &str) -> R<Id> {
    w.by_guid(guid)?.ok_or_else(|| fail("model.err.nolevel"))
}
/// `w.H(w.placeAt(storey.ObjectPlacement.value, 0, 0, 0, 0))`
fn place_on(w: &mut W<'_>, storey: Id) -> R<JV> {
    let s = w.must(storey)?;
    let lp = s.a("ObjectPlacement").href();
    Ok(JV::H(w.place_at(lp, 0.0, 0.0, 0.0, 0.0)?))
}
fn len_row(w: &W<'_>, k: &str, v: f64) -> Row {
    row(k, "IFCLENGTHMEASURE", P::Num(round6(w.u(v))))
}
/// JS truthiness of a JSON value.
fn truthy(v: &J) -> bool {
    match v {
        J::Null => false,
        J::Bool(b) => *b,
        J::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        J::String(s) => !s.is_empty(),
        _ => true,
    }
}
/// `String(v)`
fn js_string(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Number(n) => crate::js::num_str(n.as_f64().unwrap_or(f64::NAN)),
        J::Bool(b) => b.to_string(),
        J::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
fn arg(args: &[J], i: usize) -> J {
    args.get(i).cloned().unwrap_or(J::Null)
}

impl ResOp {
    pub fn new(name: &str, args: Vec<J>) -> ResOp {
        ResOp { name: name.to_string(), args }
    }

    /// Run inside the op's journal.
    pub fn body(&mut self, w: &mut W<'_>) -> R<Out> {
        let a1 = arg(&self.args, 1);
        let gl = guids(self.args.first());
        let g0 = self.args.first().and_then(J::as_str).unwrap_or("").to_string();
        match self.name.as_str() {
            "placeFoundation" => place_foundation(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeFoundation" => change_foundation(w, &g0, &a1),
            "removeFoundations" => remove(w, gl, |w| {
                Ok(crate::draw::resfound::read_foundations(w)?.into_iter().map(|f| (f.guid, [vec![f.id], f.parts].concat())).collect())
            }),
            "placeCountertop" => place_countertop(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeCountertop" => change_countertop(w, &g0, &a1),
            "fitCountertop" => change_countertop(w, &g0, &json!({})),
            "removeCountertops" => {
                remove(w, gl, |w| Ok(crate::draw::rescounter::read_countertops(w)?.into_iter().map(|c| (c.guid, vec![c.id])).collect()))
            }
            "placeStair" => place_stair(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeStair" => change_stair(w, &g0, &a1),
            "removeStairs" => remove_stairs(w, &gl),
            "placeRailing" => place_railing(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeRailing" => change_railing(w, &g0, &a1),
            "removeRailings" => remove(w, gl, |w| Ok(read_railings(w)?.into_iter().map(|r| (r.guid, vec![r.id])).collect())),
            "placeRoofPlane" => place_roof_plane(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeRoofPlane" => change_roof_plane(w, &g0, &a1),
            "removeRoofPlanes" => {
                remove(w, gl, |w| Ok(read_roof_planes(w)?.into_iter().map(|r| (r.guid, [vec![r.id], r.parts].concat())).collect()))
            }
            "placeSectionLine" => place_section(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeSectionLine" => change_section(w, &g0, &a1),
            "removeSectionLines" => remove(w, gl, |w| Ok(read_sections(w)?.into_iter().map(|r| (r.guid, vec![r.id])).collect())),
            "placeDevice" => place_device(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeDevice" => change_device(w, &g0, &a1),
            "linkLight" => {
                let light = a1.as_str().unwrap_or("").to_string();
                let sw = crate::draw::reselec::read_devices(w)?.into_iter().find(|x| x.guid == g0).ok_or_else(|| fail("model.err.gone"))?;
                let controls: Vec<J> = if sw.controls.contains(&light) {
                    sw.controls.iter().filter(|g| **g != light).map(|g| J::String(g.clone())).collect()
                } else {
                    sw.controls.iter().cloned().chain(std::iter::once(light)).map(J::String).collect()
                };
                change_device(w, &g0, &json!({ "controls": controls }))
            }
            "removeDevices" => remove_devices(w, &gl),
            "frameHosts" => frame_hosts(w, self.args.get_mut(0).ok_or_else(|| fail("model.err.gone"))?),
            "changeFraming" => change_framing(w, &g0, &a1),
            "removeFraming" => remove(w, gl, |w| {
                Ok(crate::draw::resframe::read_framing(w)?.into_iter().map(|f| (f.guid, [vec![f.id], f.parts].concat())).collect())
            }),
            "keepPiece" => keep_piece(w, &g0),
            other => Err(crate::api::JsErr(format!("unknown residential op {other}"))),
        }
    }
}

/// The remove* ops: what of `list` the guids name goes (dropProduct of each one's lines).
fn remove(w: &mut W<'_>, gl: Vec<String>, list: impl FnOnce(&mut W<'_>) -> R<Vec<(String, Vec<Id>)>>) -> R<Out> {
    let all = list(w)?;
    let ids: Vec<Id> = all.into_iter().filter(|(g, _)| gl.contains(g)).flat_map(|(_, v)| v).collect();
    if !ids.is_empty() {
        w.drop_product(&ids)?;
    }
    Ok((None, Vec::new(), Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ foundations
use crate::draw::resfound::{FIELDS, FOUND_PSET, FOUNDATION_KINDS, Foundation, complete, foundation_geometry, foundation_params, loads_on};

fn found_rows(w: &W<'_>, stored: &[Option<f64>; 9]) -> Vec<Row> {
    FIELDS
        .iter()
        .zip(stored.iter())
        .map(|((f, _), v)| row(f, "IFCLENGTHMEASURE", P::Num(round6(w.u(v.filter(|x| *x > 0.0).unwrap_or(0.0))))))
        .collect()
}

fn place_foundation(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let kind = st(spec, "kind").unwrap_or_else(|| "slab".into());
    if !FOUNDATION_KINDS.contains(&kind.as_str()) {
        return Err(fail("res.err.fdkind"));
    }
    let guid = fix_guid(w, spec);
    let storey = storey_by_guid(w, &st(spec, "level").unwrap_or_default())?;
    let l = crate::draw::resfound::storey_outlines(w, Some(storey))?;
    let seed = pt(spec.get("seed")).unwrap_or([f64::NAN, f64::NAN]);
    let o = crate::draw::rooms::loop_at(&l.outlines, seed).ok_or_else(|| fail("model.err.nobuilding2"))?;
    let mut want = [None; 9];
    for (i, (_, k)) in FIELDS.iter().enumerate() {
        if let Some(v) = nf(spec, k).filter(|v| *v > 0.0)
            && let Some(s) = want.get_mut(i)
        {
            *s = Some(v);
        }
    }
    let (stories, heavy) = loads_on(w, storey, Some(&l.walls))?;
    if !complete(&kind, &foundation_geometry(&o.poly, &foundation_params(&kind, &want, stories, heavy), None)) {
        return Err(fail("res.err.fdsize"));
    }
    let nm = w.l(&t0(&format!("res.fd.kind.{kind}")));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let ot = w.l("Foundation");
    let gj = w.gid_jv(&guid);
    let id = w.make("IFCELEMENTASSEMBLY", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s("SITE"), JV::s("USERDEFINED")])?;
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    let mut rows = vec![
        row("Kind", "IFCLABEL", P::s(&kind)),
        len_row(w, "SeedX", seed[0]),
        len_row(w, "SeedY", seed[1]),
        row("ShapeKey", "IFCIDENTIFIER", P::s("")),
    ];
    rows.extend(found_rows(w, &want));
    w.upsert_pset(id, FOUND_PSET, &rows)?;
    let f = Foundation { id, guid: guid.clone(), storey: Some(storey), seed, kind, stored: want, key: String::new(), parts: Vec::new() };
    crate::draw::resfound::write_foundation(w, &f, Some(&l))?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_foundation(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let f = crate::draw::resfound::read_foundations(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut rows = vec![row("ShapeKey", "IFCIDENTIFIER", P::s(""))];
    let mut stored = f.stored;
    let mut kind = f.kind.clone();
    if let Some(k) = st(ch, "kind").filter(|k| !k.is_empty() && *k != f.kind) {
        if !FOUNDATION_KINDS.contains(&k.as_str()) {
            return Err(fail("res.err.fdkind"));
        }
        kind = k;
        rows.push(row("Kind", "IFCLABEL", P::s(&kind)));
        rows.push(row("Bottom", "IFCLENGTHMEASURE", P::Num(0.0)));
        rows.push(row("StemDrop", "IFCLENGTHMEASURE", P::Num(0.0)));
        if let Some(s) = stored.get_mut(6) {
            *s = Some(0.0);
        }
        if let Some(s) = stored.get_mut(7) {
            *s = Some(0.0);
        }
        let mut l = w.touch(f.id)?;
        let nm = w.l(&t0(&format!("res.fd.kind.{kind}")));
        l.set("Name", nm);
        w.write(&mut l)?;
    }
    for (i, (field, k)) in FIELDS.iter().enumerate() {
        match ch.get(*k) {
            None | Some(J::Null) => continue,
            Some(v) => {
                let x = v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| fail("model.err.length"))?;
                if let Some(s) = stored.get_mut(i) {
                    *s = Some(x);
                }
                rows.push(len_row(w, field, x));
            }
        }
    }
    let l = crate::draw::resfound::storey_outlines(w, f.storey)?;
    if let Some(o) = crate::draw::rooms::loop_at(&l.outlines, f.seed)
        && let Some(stid) = f.storey
    {
        let (stories, heavy) = loads_on(w, stid, Some(&l.walls))?;
        if !complete(&kind, &foundation_geometry(&o.poly, &foundation_params(&kind, &stored, stories, heavy), None)) {
            return Err(fail("res.err.fdsize"));
        }
    }
    w.upsert_pset(f.id, FOUND_PSET, &rows)?;
    let nf2 = Foundation { kind, stored, key: String::new(), ..f.clone() };
    crate::draw::resfound::write_foundation(w, &nf2, Some(&l))?;
    Ok((Some(f.id), vec![f.id], Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ countertops
use crate::draw::rescounter::{COUNTER_PSET, Countertop, counter_cabinets, counter_runs, point_in, read_countertops, run_key};

fn place_countertop(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let overhang = nf(spec, "overhang").unwrap_or(IN);
    let thickness = nf(spec, "thickness").unwrap_or(1.5 * IN);
    let guid = fix_guid(w, spec);
    let level = st(spec, "level").unwrap_or_default();
    let storey = w.by_guid(&level)?.ok_or_else(|| fail("model.err.nolevel"))?;
    let seed = pt(spec.get("seed")).unwrap_or([f64::NAN, f64::NAN]);
    let cabs = counter_cabinets(w, storey)?;
    let runs = counter_runs(&cabs, overhang);
    let run = runs.into_iter().find(|r| point_in(seed, &r.poly)).ok_or_else(|| fail("res.err.nocabinet"))?;
    let nm = w.l(&t0("res.counter"));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let ot = w.l("Countertop");
    let gj = w.gid_jv(&guid);
    let id = if w.v4 {
        w.make("IFCFURNITURE", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s("USERDEFINED")])?
    } else {
        w.make("IFCFURNISHINGELEMENT", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null])?
    };
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    let mat = w.material(&t0("res.mat.counter"))?;
    w.associate_material(id, mat)?;
    let rows = vec![
        row("Level", "IFCIDENTIFIER", P::s(&level)),
        len_row(w, "Overhang", overhang),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(thickness)))),
    ];
    w.upsert_pset(id, COUNTER_PSET, &rows)?;
    let c = Countertop { id, guid: guid.clone(), storey: Some(storey), seed, overhang, thickness, key: String::new() };
    crate::draw::rescounter::write_countertop(w, &c, &run)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_countertop(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let c = read_countertops(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut nx = c.clone();
    let mut rows = Vec::new();
    if let Some(v) = nf(ch, "overhang").filter(|v| *v >= 0.0) {
        nx.overhang = v;
        rows.push(len_row(w, "Overhang", v));
    }
    if let Some(v) = ch.get("thickness").and_then(J::as_f64).filter(|v| *v > 0.0) {
        nx.thickness = v;
        rows.push(row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(v)))));
    }
    let storey = nx.storey.ok_or_else(|| fail("res.err.nocabinet"))?;
    let cabs = counter_cabinets(w, storey)?;
    let run = counter_runs(&cabs, nx.overhang).into_iter().find(|r| point_in(nx.seed, &r.poly)).ok_or_else(|| fail("res.err.nocabinet"))?;
    if !rows.is_empty() {
        w.upsert_pset(c.id, COUNTER_PSET, &rows)?;
    }
    crate::draw::rescounter::write_countertop(w, &nx, &run)?;
    let _ = run_key;
    Ok((Some(c.id), vec![c.id], Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ stairs
use crate::draw::resstair::{STAIR_KINDS, STAIR_PSET, Stair, ifc_type, read_stairs, stair_height, stair_layout};

fn stair_rows(w: &W<'_>, s: &Stair) -> Vec<Row> {
    vec![
        row("Kind", "IFCLABEL", P::s(&s.kind)),
        row("Turn", "IFCLABEL", P::s(&s.turn)),
        len_row(w, "StartX", s.start[0]),
        len_row(w, "StartY", s.start[1]),
        row("Angle", "IFCREAL", P::Num(round6(s.angle))),
        len_row(w, "Width", s.width),
        len_row(w, "Height", s.fixed_height),
        len_row(w, "RiserMax", s.riser_max),
        len_row(w, "Tread", s.tread),
        row("Split", "IFCREAL", P::Num(round_d(s.split, 4))),
        len_row(w, "Gap", s.gap),
        row("Railings", "IFCLABEL", P::s(&s.railings)),
        len_row(w, "RailHeight", s.rail_height),
        row("Well", "IFCBOOLEAN", P::Bool(s.well)),
        row("TopLevel", "IFCIDENTIFIER", P::s(&s.top_level)),
    ]
}
fn stair_check(s: &Stair) -> R<()> {
    if !STAIR_KINDS.contains(&s.kind.as_str()) {
        return Err(fail("res.err.stairkind"));
    }
    if !(s.width >= 500.0 && s.width <= 5000.0) {
        return Err(fail("res.err.stairwidth"));
    }
    if !(s.riser_max >= 100.0 && s.riser_max <= 300.0) || !(s.tread >= 150.0 && s.tread <= 600.0) {
        return Err(fail("res.err.stairsize"));
    }
    if !(s.split > 0.0 && s.split < 1.0) {
        return Err(fail("res.err.stairsize"));
    }
    Ok(())
}
/// `{ ...STAIR_DEFAULTS, angle: 0, fixedHeight: 0, topLevel: '', ...spec }` over a stair (strings, numbers, a boolean).
fn stair_over(mut s: Stair, o: &J, all: bool) -> Stair {
    let str_of = |k: &str| o.get(k).and_then(J::as_str).map(str::to_string);
    for (k, slot) in [("kind", &mut s.kind), ("turn", &mut s.turn), ("railings", &mut s.railings)] {
        if let Some(v) = str_of(k).filter(|v| all || !v.is_empty()) {
            *slot = v;
        }
    }
    if all && let Some(v) = str_of("topLevel") {
        s.top_level = v;
    }
    let num = |k: &str| if all { o.get(k).and_then(J::as_f64) } else { nf(o, k) };
    for (k, slot) in [
        ("width", &mut s.width),
        ("riserMax", &mut s.riser_max),
        ("tread", &mut s.tread),
        ("split", &mut s.split),
        ("gap", &mut s.gap),
        ("railHeight", &mut s.rail_height),
        ("angle", &mut s.angle),
    ] {
        if let Some(v) = num(k) {
            *slot = v;
        }
    }
    if all && let Some(v) = o.get("fixedHeight").and_then(J::as_f64) {
        s.fixed_height = v;
    }
    if let Some(p) = pt(o.get("start")) {
        s.start = p;
    }
    if let Some(b) = o.get("well").and_then(J::as_bool) {
        s.well = b;
    }
    s
}
pub fn stair_defaults() -> Stair {
    Stair {
        id: 0.0,
        guid: String::new(),
        storey: None,
        top_level: String::new(),
        kind: "straight".into(),
        turn: "left".into(),
        start: [0.0, 0.0],
        angle: 0.0,
        width: 36.0 * IN,
        fixed_height: 0.0,
        riser_max: 7.75 * IN,
        tread: 10.0 * IN,
        split: 0.5,
        gap: 2.0 * IN,
        railings: "both".into(),
        rail_height: 36.0 * IN,
        well: true,
        key: String::new(),
        parts: Vec::new(),
    }
}

fn place_stair(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let mut s = stair_over(stair_defaults(), spec, true);
    stair_check(&s)?;
    let guid = fix_guid(w, spec);
    let storey = w.by_guid(&st(spec, "level").unwrap_or_default())?.ok_or_else(|| fail("model.err.nolevel"))?;
    s.storey = Some(storey);
    s.guid = guid.clone();
    let levels = read_levels(w)?;
    let (h, _) = stair_height(&s, &levels);
    stair_layout(&s, h).map_err(fail)?;
    let nm = w.l(&t0("res.stair"));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let gj = w.gid_jv(&guid);
    let pdef = if w.v4 { ifc_type(&s.kind) } else { "NOTDEFINED" };
    let id = w.make("IFCSTAIR", vec![gj, oh, nm.clone(), JV::Null, nm, pl, JV::Null, JV::Null, JV::s(pdef)])?;
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    let rows = stair_rows(w, &s);
    w.upsert_pset(id, STAIR_PSET, &rows)?;
    s.id = id;
    crate::draw::resstair::write_stair(w, &s, &levels)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_stair(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let st0 = read_stairs(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let nx = stair_over(st0.clone(), ch, false);
    stair_check(&nx)?;
    let levels = read_levels(w)?;
    let (h, _) = stair_height(&nx, &levels);
    stair_layout(&nx, h).map_err(fail)?;
    let rows = stair_rows(w, &nx);
    w.upsert_pset(st0.id, STAIR_PSET, &rows)?;
    let nx = Stair { key: String::new(), ..nx };
    crate::draw::resstair::write_stair(w, &nx, &levels)?;
    Ok((Some(st0.id), vec![st0.id], Vec::new()))
}

fn remove_stairs(w: &mut W<'_>, gl: &[String]) -> R<Out> {
    let list: Vec<Stair> = read_stairs(w)?.into_iter().filter(|x| gl.contains(&x.guid)).collect();
    let mut wells = Vec::new();
    for x in &list {
        wells.extend(crate::draw::resstair::wells_of(w, &x.guid)?);
    }
    if !list.is_empty() {
        let mut ids: Vec<Id> = list.iter().flat_map(|x| std::iter::once(x.id).chain(x.parts.iter().copied())).collect();
        ids.extend(wells);
        w.drop_product(&ids)?;
    }
    Ok((None, Vec::new(), Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ railings
pub const RAIL_PSET: &str = "CadCraft_Railing";
const RAIL_HEIGHT: f64 = 36.0 * IN;

/// A drawn railing (`readRailings`).
#[derive(Clone, Debug)]
pub struct Railing {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub level: String,
    pub points: Vec<V2>,
    pub height: f64,
    pub base: f64,
    pub kind: String,
}

/// `Points` text "x,y;x,y" (file units) → plan mm.
fn read_points(text: &str, um: f64) -> Vec<V2> {
    text.split(';')
        .filter_map(|q| {
            let v: Vec<f64> = q.split(',').map(crate::js::str_num).collect();
            match v.as_slice() {
                [x, y] if x.is_finite() && y.is_finite() => Some([x * um, y * um]),
                _ => None,
            }
        })
        .take(100_000)
        .collect()
}
fn points_text(w: &W<'_>, p: &[V2]) -> String {
    p.iter()
        .map(|q| format!("{},{}", crate::js::num_str(round_d(w.u(q[0]), 3)), crate::js::num_str(round_d(w.u(q[1]), 3))))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn read_railings(w: &mut W<'_>) -> R<Vec<Railing>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCRAILING") {
        let Some(ps) = w.read_pset(id, RAIL_PSET)? else { continue };
        let l = w.must(id)?;
        let s = |k: &str| crate::draw::res::s_or(&ps, k);
        let numv = |k: &str| ps.get(k).map_or(f64::NAN, P::number);
        let base = match ps.get("Base") {
            Some(v) if v.truthy() => v.number(),
            _ => 0.0,
        };
        out.push(Railing {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&cc_io_ifc::model::id_key(id)).copied(),
            level: s("Level"),
            points: read_points(&s("Points"), um),
            height: numv("Height") * um,
            base: base * um,
            kind: if s("Kind") == "handrail" { "handrail".into() } else { "guard".into() },
        });
    }
    Ok(out)
}

fn write_railing(w: &mut W<'_>, r: &Railing) -> R<()> {
    let path: Vec<[f64; 3]> = r.points.iter().map(|p| [p[0], p[1], r.base]).collect();
    let parts = crate::draw::resstair::railing_parts(&path, r.height);
    let items = crate::draw::resstair::railing_items(w, &parts)?;
    let sty = w.surface_style(&t0("res.style.rail"), [0.95, 0.95, 0.93], 0.0)?;
    for it in &items {
        w.style_item(*it, sty)?;
    }
    let rep = w.rep("body", "Body", "SweptSolid", &items)?;
    w.set_shape(r.id, &[rep])?;
    let rows = vec![
        row("Points", "IFCTEXT", P::s(&points_text(w, &r.points))),
        row("Height", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(r.height)))),
        len_row(w, "Base", r.base),
        row("Kind", "IFCLABEL", P::s(&r.kind)),
    ];
    w.upsert_pset(r.id, RAIL_PSET, &rows)?;
    let q = vec![
        len_row(w, "Length", parts.length),
        row("Balusters", "IFCINTEGER", P::Num(parts.balusters.len() as f64)),
        row("Posts", "IFCINTEGER", P::Num(parts.posts.len() as f64)),
    ];
    w.upsert_pset(r.id, "CadCraft_RailingQuantities", &q)?;
    Ok(())
}

fn rail_name(kind: &str) -> String {
    t0(if kind == "handrail" { "res.rail.handrail" } else { "res.rail.guard" })
}

fn place_railing(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let points = pts(spec.get("points")).unwrap_or_default();
    let height = match spec.get("height") {
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => RAIL_HEIGHT,
    };
    let base = match spec.get("base") {
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => 0.0,
    };
    let kind = st(spec, "kind").unwrap_or_else(|| "guard".into());
    if points.len() < 2 {
        return Err(fail("res.err.rail"));
    }
    if !(height >= 300.0) {
        return Err(fail("model.err.length"));
    }
    let guid = fix_guid(w, spec);
    let level = st(spec, "level").unwrap_or_default();
    let storey = w.by_guid(&level)?.ok_or_else(|| fail("model.err.nolevel"))?;
    let nm = w.l(&rail_name(&kind));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let gj = w.gid_jv(&guid);
    let pdef = if w.v4 { if kind == "handrail" { "HANDRAIL" } else { "GUARDRAIL" } } else { "NOTDEFINED" };
    let id = w.make("IFCRAILING", vec![gj, oh, nm.clone(), JV::Null, nm, pl, JV::Null, JV::Null, JV::s(pdef)])?;
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    w.upsert_pset(id, RAIL_PSET, &[row("Level", "IFCIDENTIFIER", P::s(&level))])?;
    let r = Railing { id, guid: guid.clone(), storey: Some(storey), level, points, height, base, kind };
    write_railing(w, &r)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_railing(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let r = read_railings(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut nx = r.clone();
    match ch.get("height") {
        Some(v) if v.as_f64().is_some_and(|h| h >= 300.0) => nx.height = v.as_f64().unwrap_or(nx.height),
        Some(_) => return Err(fail("model.err.length")),
        None => {}
    }
    if let Some(b) = nf(ch, "base") {
        nx.base = b;
    }
    if let Some(p) = pts(ch.get("points")).filter(|p| p.len() >= 2) {
        nx.points = p;
    }
    if let Some(k) = st(ch, "kind").filter(|k| k == "guard" || k == "handrail") {
        nx.kind = k.clone();
        if w.v4 {
            let mut l = w.touch(r.id)?;
            l.set("PredefinedType", JV::e(if k == "handrail" { "HANDRAIL" } else { "GUARDRAIL" }));
            let nm = w.l(&rail_name(&k));
            l.set("Name", nm);
            w.write(&mut l)?;
        }
    }
    write_railing(w, &nx)?;
    Ok((Some(r.id), vec![r.id], Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ roof planes
pub const PLANE_PSET: &str = "CadCraft_RoofPlane";

/// A manual roof plane (`readRoofPlanes`).
#[derive(Clone, Debug)]
pub struct RoofPlane {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub level: String,
    pub points: Vec<V2>,
    pub pitch: f64,
    pub base: f64,
    pub thickness: f64,
    pub parts: Vec<Id>,
}

/// `planeFrame(points)` → (o, u, n): the eave's frame, n inward (to the left of u).
pub fn plane_frame(p: &[V2]) -> (V2, V2, V2) {
    let p0 = p.first().copied().unwrap_or([0.0, 0.0]);
    let p1 = p.get(1).copied().unwrap_or(p0);
    let l = m::hypot(p1[0] - p0[0], p1[1] - p0[1]);
    let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
    let u = [(p1[0] - p0[0]) / l, (p1[1] - p0[1]) / l];
    let n = [-u[1], u[0]];
    let side: f64 = p.iter().map(|q| (q[0] - p0[0]) * n[0] + (q[1] - p0[1]) * n[1]).fold(0.0, |s, v| s + v);
    if side < 0.0 { (p0, [-u[0], -u[1]], [-n[0], -n[1]]) } else { (p0, u, n) }
}
/// `planeZ(rp, p)`
pub fn plane_z(rp: &RoofPlane, p: V2) -> f64 {
    let (o, _, n) = plane_frame(&rp.points);
    rp.base + m::tan(rp.pitch * D2R) * ((p[0] - o[0]) * n[0] + (p[1] - o[1]) * n[1])
}
fn area2(p: &[V2]) -> f64 {
    let mut s = 0.0;
    for (i, a) in p.iter().enumerate() {
        let b = p.get(m::nx(i, p.len())).copied().unwrap_or(*a);
        s += a[0] * b[1] - a[1] * b[0];
    }
    s / 2.0
}

pub fn read_roof_planes(w: &mut W<'_>) -> R<Vec<RoofPlane>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCROOF") {
        let Some(ps) = w.read_pset(id, PLANE_PSET)? else { continue };
        let l = w.must(id)?;
        let numv = |k: &str| ps.get(k).map_or(f64::NAN, P::number);
        out.push(RoofPlane {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&cc_io_ifc::model::id_key(id)).copied(),
            level: crate::draw::res::s_or(&ps, "Level"),
            points: read_points(&crate::draw::res::s_or(&ps, "Points"), um),
            pitch: numv("PitchDegrees"),
            base: numv("Base") * um,
            thickness: numv("Thickness") * um,
            parts: crate::draw::res::parts_of(w, id)?,
        });
    }
    Ok(out)
}

fn plane_check(points: &[V2], pitch: f64) -> R<()> {
    if points.len() < 3 || area2(points).abs() < 1e4 {
        return Err(fail("res.err.plane"));
    }
    if !(0.0..80.0).contains(&pitch) {
        return Err(fail("model.err.pitch"));
    }
    Ok(())
}
/// `wallsTop(w, storey)`: the tallest drawn wall's top on a level, else its height.
pub fn walls_top(w: &mut W<'_>, storey: Id) -> R<f64> {
    let levels = read_levels(w)?;
    let top = read_walls(w, Some(storey))?.iter().fold(0.0, |mx, x| m::max(mx, x.base_offset + x.height));
    if top != 0.0 && !top.is_nan() {
        return Ok(top);
    }
    let h = levels.iter().find(|l| l.id == storey).map_or(0.0, |l| l.height);
    Ok(if h != 0.0 && !h.is_nan() { h } else { 2438.4 })
}

fn write_plane(w: &mut W<'_>, rp: &RoofPlane) -> R<Id> {
    let levels = read_levels(w)?;
    let lvl = levels.iter().find(|l| l.guid.as_deref() == Some(rp.level.as_str())).ok_or_else(|| fail("model.err.nolevel"))?.clone();
    let storey = rp.storey.ok_or_else(|| fail("model.err.nolevel"))?;
    let hw = w.storey_world(storey)?;
    let lw = w.storey_world(lvl.id)?;
    let mm = mul4(&inv4(&hw).ok_or_else(|| fail("model.err.nolevel"))?, &lw);
    let pf = |x: f64, y: f64, z: f64| apply4(&mm, x, y, z);
    let df = |x: f64, y: f64, z: f64| {
        let a = apply4(&mm, x, y, z);
        let o = apply4(&mm, 0.0, 0.0, 0.0);
        [a[0] - o[0], a[1] - o[1], a[2] - o[2]]
    };
    if !rp.parts.is_empty() {
        w.drop_product(&rp.parts)?;
    }
    let (o, u, n) = plane_frame(&rp.points);
    let (cos, sin) = (m::cos(rp.pitch * D2R), m::sin(rp.pitch * D2R));
    let x = df(u[0], u[1], 0.0);
    let y = df(n[0] * cos, n[1] * cos, sin);
    let z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let prof: Vec<V2> = rp
        .points
        .iter()
        .map(|p| [(p[0] - o[0]) * u[0] + (p[1] - o[1]) * u[1], ((p[0] - o[0]) * n[0] + (p[1] - o[1]) * n[1]) / cos])
        .collect();
    let nm = w.l(&t0("model.roof.slab"));
    let pl = place_on(w, storey)?;
    let g = w.guid();
    let oh = w.owner_history();
    let sid = w.make("IFCSLAB", vec![g, oh, nm.clone(), JV::Null, nm, pl, JV::Null, JV::Null, JV::s("ROOF")])?;
    let pr = w.profile(&prof, None)?;
    let fr = w.axis_frame(pf(o[0], o[1], rp.base), z, x)?;
    let solid = w.extrude_at(pr, fr, rp.thickness / cos, [0.0, sin, cos])?;
    let sty = w.surface_style(&t0("model.style.roofing"), [0.42, 0.33, 0.3], 0.0)?;
    w.style_item(solid, sty)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[solid])?;
    w.set_shape(sid, &[rep])?;
    let mut rel = None;
    for rid in w.ids("IFCRELAGGREGATES") {
        if w.must(rid)?.a("RelatingObject").href() == Some(rp.id) {
            rel = Some(rid);
            break;
        }
    }
    match rel {
        Some(r) => {
            let mut l = w.touch(r)?;
            l.set("RelatedObjects", JV::L(vec![JV::H(sid)]));
            w.write(&mut l)?;
        }
        None => {
            let g = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(rp.id), JV::L(vec![JV::H(sid)])])?;
        }
    }
    let proj = area2(&rp.points).abs();
    let q: Vec<QRow> =
        vec![("GrossArea".into(), "area", proj / cos), ("NetArea".into(), "area", proj / cos), ("ProjectedArea".into(), "area", proj)];
    w.upsert_qto(rp.id, "Qto_RoofBaseQuantities", &q)?;
    let rows = vec![
        row("Points", "IFCTEXT", P::s(&points_text(w, &rp.points))),
        row("PitchDegrees", "IFCREAL", P::Num(round_d(rp.pitch, 4))),
        len_row(w, "Base", rp.base),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(rp.thickness)))),
    ];
    w.upsert_pset(rp.id, PLANE_PSET, &rows)?;
    Ok(sid)
}

fn place_roof_plane(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let points = pts(spec.get("points")).unwrap_or_default();
    let pitch = spec.get("pitch").and_then(J::as_f64).unwrap_or(f64::NAN);
    let thickness = match spec.get("thickness") {
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => 8.0 * IN,
    };
    plane_check(&points, pitch)?;
    let guid = fix_guid(w, spec);
    let level = st(spec, "level").unwrap_or_default();
    let levels = read_levels(w)?;
    let i = levels.iter().position(|l| l.guid.as_deref() == Some(level.as_str())).ok_or_else(|| fail("model.err.nolevel"))?;
    let host = levels.get(i.saturating_add(1)).or_else(|| levels.get(i)).ok_or_else(|| fail("model.err.nolevel"))?.clone();
    let here = levels.get(i).map(|l| l.id).unwrap_or(host.id);
    let base = match nf(spec, "base") {
        Some(b) => b,
        None => walls_top(w, here)?,
    };
    let nm = w.l(&t0("res.roofplane"));
    let pl = place_on(w, host.id)?;
    let oh = w.owner_history();
    let ot = w.l("Roof plane");
    let gj = w.gid_jv(&guid);
    let pdef = if w.v4 { "FREEFORM" } else { "NOTDEFINED" };
    let id = w.make("IFCROOF", vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s(pdef)])?;
    w.note_guid(&guid, id);
    w.contain(id, host.id)?;
    w.upsert_pset(id, PLANE_PSET, &[row("Level", "IFCIDENTIFIER", P::s(&level))])?;
    let rp = RoofPlane { id, guid: guid.clone(), storey: Some(host.id), level, points, pitch, base, thickness, parts: Vec::new() };
    write_plane(w, &rp)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_roof_plane(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let rp = read_roof_planes(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut nx = rp.clone();
    if let Some(p) = ch.get("points").and_then(J::as_array) {
        nx.points = p.iter().filter_map(|q| pt(Some(q))).collect();
    }
    if let Some(v) = nf(ch, "pitch") {
        nx.pitch = v;
    }
    if let Some(v) = nf(ch, "base") {
        nx.base = v;
    }
    if let Some(v) = ch.get("thickness").and_then(J::as_f64).filter(|v| *v > 0.0) {
        nx.thickness = v;
    }
    plane_check(&nx.points, nx.pitch)?;
    write_plane(w, &nx)?;
    Ok((Some(rp.id), vec![rp.id], Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ section lines
pub const SECTION_PSET: &str = "CadCraft_SectionLine";

/// A section line (`readSectionLines`).
#[derive(Clone, Debug)]
pub struct Section {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub level: String,
    pub name: String,
    pub a: V2,
    pub b: V2,
    pub look: String,
    pub depth: f64,
}

pub fn read_sections(w: &mut W<'_>) -> R<Vec<Section>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for id in w.ids("IFCANNOTATION") {
        let Some(ps) = w.read_pset(id, SECTION_PSET)? else { continue };
        let l = w.must(id)?;
        let numv = |k: &str| ps.get(k).map_or(f64::NAN, P::number);
        let s = |k: &str| crate::draw::res::s_or(&ps, k);
        let depth = match ps.get("Depth") {
            Some(v) if v.truthy() => v.number(),
            _ => 0.0,
        };
        out.push(Section {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&cc_io_ifc::model::id_key(id)).copied(),
            level: s("Level"),
            name: {
                let n = s("Name");
                if n.is_empty() { "A".into() } else { n }
            },
            a: [numv("AX") * um, numv("AY") * um],
            b: [numv("BX") * um, numv("BY") * um],
            look: if s("Look") == "right" { "right".into() } else { "left".into() },
            depth: depth * um,
        });
    }
    Ok(out)
}
/// `sectionDir(sl)` → (o, the way it looks, along it)
pub fn section_dir(sl: &Section) -> (V2, V2, V2) {
    let d = [sl.b[0] - sl.a[0], sl.b[1] - sl.a[1]];
    let l = m::hypot(d[0], d[1]);
    let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
    let u = [d[0] / l, d[1] / l];
    (sl.a, if sl.look == "right" { [u[1], -u[0]] } else { [-u[1], u[0]] }, u)
}

fn place_section(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let a = pt(spec.get("a")).unwrap_or([f64::NAN, f64::NAN]);
    let b = pt(spec.get("b")).unwrap_or([f64::NAN, f64::NAN]);
    let look = st(spec, "look").unwrap_or_else(|| "left".into());
    let depth = match spec.get("depth") {
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => 0.0,
    };
    if m::hypot(b[0] - a[0], b[1] - a[1]) < 100.0 {
        return Err(fail("res.err.sectionshort"));
    }
    let guid = fix_guid(w, spec);
    let level = st(spec, "level").unwrap_or_default();
    let storey = w.by_guid(&level)?.ok_or_else(|| fail("model.err.nolevel"))?;
    let name = match st(spec, "name").filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => {
            let have: Vec<String> = read_sections(w)?.into_iter().map(|x| x.name).collect();
            let n = "ABCDEFGHJKLMNPQRSTUVWXYZ".chars().map(String::from).find(|c| !have.contains(c)).unwrap_or_else(|| "S".into());
            if let Some(o) = spec.as_object_mut() {
                o.insert("name".into(), J::String(n.clone()));
            }
            n
        }
    };
    let nm = w.l(&t("res.sec.name", &[("name", &name)]));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let ot = w.l("Section line");
    let gj = w.gid_jv(&guid);
    let mut args = vec![gj, oh, nm, JV::Null, ot, pl, JV::Null];
    if w.x3 {
        args.push(JV::s("USERDEFINED"));
    }
    let id = w.make("IFCANNOTATION", args)?;
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    let rows = vec![
        row("Level", "IFCIDENTIFIER", P::s(&level)),
        row("Name", "IFCLABEL", P::s(&name)),
        len_row(w, "AX", a[0]),
        len_row(w, "AY", a[1]),
        len_row(w, "BX", b[0]),
        len_row(w, "BY", b[1]),
        row("Look", "IFCLABEL", P::s(&look)),
        len_row(w, "Depth", depth),
    ];
    w.upsert_pset(id, SECTION_PSET, &rows)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_section(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let sl = read_sections(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut rows = Vec::new();
    if let Some(n) = st(ch, "name").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        let n = crate::js::slice16(&n, 8);
        rows.push(row("Name", "IFCLABEL", P::s(&n)));
        let mut l = w.touch(sl.id)?;
        let nm = w.l(&t("res.sec.name", &[("name", &n)]));
        l.set("Name", nm);
        w.write(&mut l)?;
    }
    if let Some(lk) = st(ch, "look").filter(|l| l == "left" || l == "right") {
        rows.push(row("Look", "IFCLABEL", P::s(&lk)));
    }
    if let Some(d) = nf(ch, "depth").filter(|d| *d >= 0.0) {
        rows.push(len_row(w, "Depth", d));
    }
    if let Some(a) = ch.get("a").filter(|v| v.is_array()) {
        let a = pt(Some(a)).unwrap_or([f64::NAN, f64::NAN]);
        rows.push(len_row(w, "AX", a[0]));
        rows.push(len_row(w, "AY", a[1]));
    }
    if let Some(b) = ch.get("b").filter(|v| v.is_array()) {
        let b = pt(Some(b)).unwrap_or([f64::NAN, f64::NAN]);
        rows.push(len_row(w, "BX", b[0]));
        rows.push(len_row(w, "BY", b[1]));
    }
    w.upsert_pset(sl.id, SECTION_PSET, &rows)?;
    Ok((Some(sl.id), vec![sl.id], Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ electrical
use crate::draw::reselec::{Device, ELEC_PSET, read_devices};

/// (kind, IFC4 class, IFC4 type, IFC2X3 class, prefix, height ftin, height mm, load W)
pub type DeviceKind = (&'static str, &'static str, &'static str, &'static str, &'static str, f64, f64, f64);

/// res-elec.js DEVICES (the placing half): kind, IFC4 class and type, IFC2X3 class, tag prefix, wall height (ftin, mm),
/// the symbol's planning load (W).
pub const DEVICE_KINDS: [DeviceKind; 16] = [
    ("socket", "IFCOUTLET", "POWEROUTLET", "IFCFLOWTERMINAL", "R", 16.0 * IN, 300.0, 200.0),
    ("socket.double", "IFCOUTLET", "POWEROUTLET", "IFCFLOWTERMINAL", "R", 16.0 * IN, 300.0, 400.0),
    ("socket.earth", "IFCOUTLET", "POWEROUTLET", "IFCFLOWTERMINAL", "R", 16.0 * IN, 300.0, 200.0),
    ("socket.3ph", "IFCOUTLET", "POWEROUTLET", "IFCFLOWTERMINAL", "R", 16.0 * IN, 300.0, 3000.0),
    ("switch", "IFCSWITCHINGDEVICE", "TOGGLESWITCH", "IFCFLOWCONTROLLER", "S", 48.0 * IN, 1100.0, 0.0),
    ("switch.2way", "IFCSWITCHINGDEVICE", "TOGGLESWITCH", "IFCFLOWCONTROLLER", "S", 48.0 * IN, 1100.0, 0.0),
    ("switch.dimmer", "IFCSWITCHINGDEVICE", "DIMMERSWITCH", "IFCFLOWCONTROLLER", "S", 48.0 * IN, 1100.0, 0.0),
    ("pushbutton", "IFCSWITCHINGDEVICE", "MOMENTARYSWITCH", "IFCFLOWCONTROLLER", "S", 48.0 * IN, 1100.0, 0.0),
    ("sensor", "IFCSENSOR", "MOVEMENTSENSOR", "IFCFLOWCONTROLLER", "M", 0.0, 0.0, 2.0),
    ("luminaire", "IFCLIGHTFIXTURE", "POINTSOURCE", "IFCFLOWTERMINAL", "L", 0.0, 0.0, 60.0),
    ("downlight", "IFCLIGHTFIXTURE", "POINTSOURCE", "IFCFLOWTERMINAL", "L", 0.0, 0.0, 10.0),
    ("luminaire.linear", "IFCLIGHTFIXTURE", "POINTSOURCE", "IFCFLOWTERMINAL", "L", 0.0, 0.0, 36.0),
    ("luminaire.emergency", "IFCLIGHTFIXTURE", "SECURITYLIGHTING", "IFCFLOWTERMINAL", "L", 0.0, 0.0, 5.0),
    ("junction", "IFCJUNCTIONBOX", "POWER", "IFCFLOWFITTING", "J", 0.0, 0.0, 0.0),
    ("appliance", "IFCOUTLET", "POWEROUTLET", "IFCFLOWTERMINAL", "R", 16.0 * IN, 300.0, 2000.0),
    ("board", "IFCELECTRICDISTRIBUTIONBOARD", "DISTRIBUTIONBOARD", "IFCFLOWCONTROLLER", "P", 60.0 * IN, 1500.0, 0.0),
];
/// res-elec.js `isSwitch(kind)`
pub fn is_switch(kind: &str) -> bool {
    kind.starts_with("switch") || kind.starts_with("pushbutton") || kind.starts_with("sensor")
}
fn ceiling_of(w: &mut W<'_>, storey: Id) -> R<f64> {
    let ws = read_walls(w, Some(storey))?;
    if !ws.is_empty() {
        let mut hs: Vec<f64> = ws.iter().map(|x| x.height).collect();
        hs.sort_by(|a, b| crate::js::num_cmp(*a, *b));
        return Ok(hs.get(hs.len() / 2).copied().unwrap_or(0.0));
    }
    let h = read_levels(w)?.into_iter().find(|l| l.id == storey).map_or(0.0, |l| l.height);
    Ok(m::max(2000.0, (if h != 0.0 && !h.is_nan() { h } else { 2700.0 }) - 300.0))
}
fn next_tag(devs: &[Device], prefix: &str) -> String {
    let mut i = 1u64;
    loop {
        let t = format!("{prefix}{i}");
        if !devs.iter().any(|d| d.tag == t) || i > 1_000_000 {
            return t;
        }
        i = i.saturating_add(1);
    }
}

fn place_device(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let kind = st(spec, "kind").unwrap_or_default();
    let dk = DEVICE_KINDS.iter().find(|d| d.0 == kind).copied().ok_or_else(|| fail("res.err.elkind"))?;
    let wall_mount = crate::draw::reselec::device(&kind).is_some_and(|d| d.1);
    // res-elec.js copies the spec inside the op: a redo makes the device with a new GlobalId
    let guid = match spec.get("guid").and_then(J::as_str).filter(|g| !g.is_empty()) {
        Some(g) => g.to_string(),
        None => w.guid().val_str().unwrap_or_default(),
    };
    let level = st(spec, "level").unwrap_or_default();
    let storey = w.by_guid(&level)?.ok_or_else(|| fail("model.err.nolevel"))?;
    let walls = read_walls(w, Some(storey))?;
    let wall = st(spec, "wall").unwrap_or_default();
    if wall_mount && !(!wall.is_empty() && walls.iter().any(|x| x.guid == wall)) {
        return Err(fail("res.err.elwall"));
    }
    let height = match nf(spec, "height").filter(|h| *h > 0.0) {
        Some(h) => h,
        None => {
            if wall_mount {
                if st(spec, "units").as_deref() == Some("mm") { dk.6 } else { dk.5 }
            } else {
                ceiling_of(w, storey)?
            }
        }
    };
    let devs = read_devices(w)?;
    let tag = match spec.get("tag").filter(|v| truthy(v)) {
        Some(v) => crate::js::slice16(&js_string(v), 20),
        None => next_tag(&devs, dk.4),
    };
    let nm = w.l(&t0(&format!("res.el.kind.{kind}")));
    let pl = place_on(w, storey)?;
    let oh = w.owner_history();
    let ot = w.l(&kind);
    let gj = w.gid_jv(&guid);
    let tg = w.t("IFCIDENTIFIER", P::s(&tag));
    let id = if w.v4 {
        w.make(dk.1, vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, tg, JV::s(dk.2)])?
    } else {
        w.make(dk.3, vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, tg])?
    };
    w.note_guid(&guid, id);
    w.contain(id, storey)?;
    let load = nf(spec, "load").unwrap_or(dk.7);
    let side = if spec.get("side").and_then(J::as_f64).is_some_and(|s| s < 0.0) { -1.0 } else { 1.0 };
    let offset = match spec.get("offset").and_then(J::as_f64) {
        Some(o) if o != 0.0 && !o.is_nan() => o,
        _ => 0.0,
    };
    let circuit = match spec.get("circuit") {
        None | Some(J::Null) => String::new(),
        Some(v) => js_string(v),
    };
    let rows = vec![
        row("Level", "IFCIDENTIFIER", P::s(&level)),
        row("Kind", "IFCLABEL", P::s(&kind)),
        row("Circuit", "IFCLABEL", P::s(&circuit)),
        row("Load", "IFCPOWERMEASURE", P::Num(round_d(load, 3))),
        len_row(w, "MountingHeight", height),
        row("Wall", "IFCIDENTIFIER", P::s(&wall)),
        row("Side", "IFCINTEGER", P::Num(side)),
        len_row(w, "Offset", offset),
        row("Controls", "IFCTEXT", P::s("")),
    ];
    w.upsert_pset(id, ELEC_PSET, &rows)?;
    let angle = match spec.get("angle").and_then(J::as_f64) {
        Some(a) if a != 0.0 && !a.is_nan() => a,
        _ => 0.0,
    };
    let d = Device {
        id,
        guid: guid.clone(),
        storey: Some(storey),
        level,
        circuit,
        load,
        controls: Vec::new(),
        kind,
        tag,
        height,
        wall,
        side,
        offset,
        at: pt(spec.get("at")).unwrap_or([0.0, 0.0]),
        angle,
    };
    crate::draw::reselec::write_device(w, &d, &walls)?;
    Ok((Some(id), vec![id], vec![(guid, id)]))
}

fn change_device(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let all = read_devices(w)?;
    let d = all.iter().find(|x| x.guid == guid).cloned().ok_or_else(|| fail("model.err.gone"))?;
    let mut rows = Vec::new();
    let mut nx = d.clone();
    if let Some(tg) = ch.get("tag").and_then(J::as_str).map(str::trim).filter(|t| !t.is_empty()) {
        let tag = crate::js::slice16(tg, 20);
        if all.iter().any(|x| x.guid != guid && x.tag == tag) {
            return Err(fail("res.err.eltag"));
        }
        let mut l = w.touch(d.id)?;
        let tv = w.t("IFCIDENTIFIER", P::s(&tag));
        l.set("Tag", tv);
        w.write(&mut l)?;
    }
    if let Some(c) = ch.get("circuit").and_then(J::as_str) {
        rows.push(row("Circuit", "IFCLABEL", P::s(&crate::js::slice16(c.trim(), 20))));
    }
    if let Some(v) = nf(ch, "load").filter(|v| *v >= 0.0) {
        rows.push(row("Load", "IFCPOWERMEASURE", P::Num(round_d(v, 3))));
    }
    if let Some(v) = nf(ch, "height").filter(|v| *v > 0.0) {
        nx.height = v;
        rows.push(len_row(w, "MountingHeight", v));
    }
    if let Some(list) = ch.get("controls").and_then(J::as_array) {
        if !is_switch(&d.kind) {
            return Err(fail("res.err.elnotswitch"));
        }
        let lights: Vec<&str> = all.iter().filter(|x| crate::draw::reselec::is_light(&x.kind)).map(|x| x.guid.as_str()).collect();
        let mut seen: Vec<String> = Vec::new();
        for g in list.iter().map(js_string) {
            if !seen.contains(&g) {
                seen.push(g);
            }
        }
        let keep: Vec<String> = seen.into_iter().filter(|g| lights.contains(&g.as_str())).collect();
        rows.push(row("Controls", "IFCTEXT", P::s(&keep.join(","))));
    }
    if !rows.is_empty() {
        w.upsert_pset(d.id, ELEC_PSET, &rows)?;
    }
    if nx.height != d.height {
        let walls = read_walls(w, d.storey)?;
        crate::draw::reselec::write_device(w, &nx, &walls)?;
    }
    Ok((Some(d.id), vec![d.id], Vec::new()))
}

fn remove_devices(w: &mut W<'_>, gl: &[String]) -> R<Out> {
    let all = read_devices(w)?;
    for s in &all {
        if !gl.contains(&s.guid) && s.controls.iter().any(|g| gl.contains(g)) {
            let keep: Vec<String> = s.controls.iter().filter(|g| !gl.contains(g)).cloned().collect();
            w.upsert_pset(s.id, ELEC_PSET, &[row("Controls", "IFCTEXT", P::s(&keep.join(",")))])?;
        }
    }
    let ids: Vec<Id> = all.iter().filter(|x| gl.contains(&x.guid)).map(|x| x.id).collect();
    if !ids.is_empty() {
        w.drop_product(&ids)?;
    }
    Ok((None, Vec::new(), Vec::new()))
}

// ------------------------------------------------------------------------------------------------------------ framing
use crate::draw::resframe::{Cache, FRAME_PSET, Framing, Host, LUMBER_PSET, Wrote, host_of, read_framing, write_framing_out};

fn frame_hosts(w: &mut W<'_>, spec: &mut J) -> R<Out> {
    let spacing = spec.get("spacing").map_or(16.0 * IN, |v| v.as_f64().unwrap_or(f64::NAN));
    let corners = st(spec, "corners").unwrap_or_else(|| "three-stud".into());
    let blocking = spec.get("blocking").is_none_or(truthy);
    let roof_mode = st(spec, "roofMode").unwrap_or_else(|| "stick".into());
    let hosts = guids(spec.get("hosts"));
    let mut cache = Cache::default();
    let have = read_framing(w)?;
    let mut made = Vec::new();
    let mut ids_out = Vec::new();
    let mut n = 0usize;
    for g in &hosts {
        let Some((h, storey)) = host_of(w, g, &mut cache)? else { continue };
        let Some(storey) = storey else { continue };
        let kind = match h {
            Host::Wall(_) => "wall",
            Host::Floor(_) => "floor",
            Host::Roof(_) => "roof",
        };
        let old = have.iter().find(|x| x.host == *g).cloned();
        let id = match &old {
            Some(o) => o.id,
            None => {
                let fixed = spec.get("guids").and_then(|m| m.get(g.as_str())).and_then(J::as_str).map(str::to_string);
                let gid = match fixed {
                    Some(x) => x,
                    None => {
                        let x = w.guid().val_str().unwrap_or_default();
                        if let Some(o) = spec.as_object_mut() {
                            let m = o.entry("guids").or_insert_with(|| json!({}));
                            if let Some(mo) = m.as_object_mut() {
                                mo.insert(g.clone(), J::String(x.clone()));
                            }
                        }
                        x
                    }
                };
                let nm = w.l(&t0(&format!("res.fr.of.{kind}")));
                let pl = place_on(w, storey)?;
                let oh = w.owner_history();
                let ot = w.l("Framing");
                let gj = w.gid_jv(&gid);
                let id = w.make(
                    "IFCELEMENTASSEMBLY",
                    vec![gj, oh, nm, JV::Null, ot, pl, JV::Null, JV::Null, JV::s("SITE"), JV::s("USERDEFINED")],
                )?;
                w.note_guid(&gid, id);
                w.contain(id, storey)?;
                made.push((gid, id));
                id
            }
        };
        let rows = vec![
            row("Host", "IFCIDENTIFIER", P::s(g)),
            row("Kind", "IFCLABEL", P::s(kind)),
            row("Spacing", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(spacing)))),
            row("Corners", "IFCLABEL", P::s(&corners)),
            row("Blocking", "IFCBOOLEAN", P::Bool(blocking)),
            row("RoofMode", "IFCLABEL", P::s(&roof_mode)),
            row("ShapeKey", "IFCIDENTIFIER", P::s("")),
        ];
        w.upsert_pset(id, FRAME_PSET, &rows)?;
        let fr = match old {
            Some(o) => Framing { spacing, corners: corners.clone(), blocking, roof_mode: roof_mode.clone(), key: String::new(), ..o },
            None => Framing {
                id,
                guid: made.last().map(|x| x.0.clone()).unwrap_or_default(),
                kind: kind.into(),
                storey: Some(storey),
                host: g.clone(),
                spacing,
                corners: corners.clone(),
                blocking,
                roof_mode: roof_mode.clone(),
                key: String::new(),
                parts: Vec::new(),
            },
        };
        let r = write_framing_out(w, &fr, &mut cache)?;
        if let Wrote::Error(e) = r
            && hosts.len() == 1
        {
            return Err(fail(e));
        }
        if r == Wrote::Written {
            n = n.saturating_add(1);
        }
        ids_out.push(id);
    }
    if n == 0 && ids_out.is_empty() {
        return Err(fail("res.err.noframe"));
    }
    Ok((ids_out.first().copied(), ids_out, made))
}

fn change_framing(w: &mut W<'_>, guid: &str, ch: &J) -> R<Out> {
    let fr = read_framing(w)?.into_iter().find(|x| x.guid == guid).ok_or_else(|| fail("model.err.gone"))?;
    let mut nx = Framing { key: String::new(), ..fr.clone() };
    if let Some(v) = ch.get("spacing").and_then(J::as_f64).filter(|v| *v > 100.0) {
        nx.spacing = v;
    }
    if let Some(v) = st(ch, "corners").filter(|v| !v.is_empty()) {
        nx.corners = v;
    }
    if let Some(v) = ch.get("blocking").and_then(J::as_bool) {
        nx.blocking = v;
    }
    if let Some(v) = st(ch, "roofMode").filter(|v| !v.is_empty()) {
        nx.roof_mode = v;
    }
    let rows = vec![
        row("Spacing", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(nx.spacing)))),
        row("Corners", "IFCLABEL", P::s(&nx.corners)),
        row("Blocking", "IFCBOOLEAN", P::Bool(nx.blocking)),
        row("RoofMode", "IFCLABEL", P::s(&nx.roof_mode)),
    ];
    w.upsert_pset(fr.id, FRAME_PSET, &rows)?;
    if let Wrote::Error(e) = write_framing_out(w, &nx, &mut Cache::default())? {
        return Err(fail(e));
    }
    Ok((Some(fr.id), vec![fr.id], Vec::new()))
}

fn keep_piece(w: &mut W<'_>, guid: &str) -> R<Out> {
    let mut id = w.by_guid(guid)?;
    if id.is_none() {
        let mut list = w.ids("IFCMEMBER");
        list.extend(w.ids("IFCBEAM"));
        for x in list {
            if w.guid_of(x)?.as_deref() == Some(guid) {
                id = Some(x);
                break;
            }
        }
    }
    let id = id.ok_or_else(|| fail("model.err.gone"))?;
    w.upsert_pset(id, LUMBER_PSET, &[row("Generated", "IFCBOOLEAN", P::Bool(false)), row("Check", "IFCLABEL", P::s(""))])?;
    Ok((Some(id), vec![id], Vec::new()))
}

/// A row of the lumber list (`lumberList`): size, its label, stock length, pieces, board feet.
#[derive(Clone, Debug, PartialEq)]
pub struct LumberRow {
    pub size: String,
    pub label: String,
    pub stock: f64,
    pub count: f64,
    pub bf: f64,
}

/// `lumberList(w, { metric })` → (rows, pieces, board feet)
pub fn lumber_list(w: &mut W<'_>, metric: bool) -> R<(Vec<LumberRow>, f64, f64)> {
    const FT: f64 = 304.8;
    let mut rows: Vec<LumberRow> = Vec::new();
    let (mut pieces, mut bf) = (0.0, 0.0);
    for fr in read_framing(w)? {
        for pid in fr.parts {
            let Some(ps) = w.read_pset(pid, LUMBER_PSET)? else { continue };
            let len = ps.get("Length").map_or(f64::NAN, P::number) * w.unit_mm;
            let sz = crate::draw::res::s_or(&ps, "Size");
            let base = sz.split(' ').next().unwrap_or("").to_string();
            let plies =
                sz.split_once("× ").and_then(|(_, r)| r.split(|c: char| !c.is_ascii_digit()).next()).map_or(1.0, crate::js::str_num);
            let plies = if plies.is_finite() { plies } else { 1.0 };
            let stock =
                if metric { m::max(2400.0, (len / 600.0).ceil() * 600.0) } else { m::max(8.0 * FT, (len / (2.0 * FT)).ceil() * 2.0 * FT) };
            let b = crate::draw::resframe::board_feet(&base, stock) * plies;
            let label = if metric { metric_name(&base).unwrap_or(&base).to_string() } else { base.clone() };
            match rows.iter_mut().find(|r| r.size == base && r.stock == stock) {
                Some(r) => {
                    r.count += plies;
                    r.bf += b;
                }
                None => rows.push(LumberRow { size: base.clone(), label, stock, count: plies, bf: b }),
            }
            pieces += plies;
            bf += b;
        }
    }
    let idx = |s: &str| crate::draw::resframe::SIZES.iter().position(|x| *x == s).map_or(-1.0, |i| i as f64);
    rows.sort_by(|a, b| crate::js::num_cmp(idx(&a.size) - idx(&b.size), 0.0).then(crate::js::num_cmp(a.stock - b.stock, 0.0)));
    Ok((rows, pieces, bf))
}
/// LUMBER[size].metric
pub fn metric_name(size: &str) -> Option<&'static str> {
    Some(match size {
        "2x4" => "38 × 89",
        "2x6" => "38 × 140",
        "2x8" => "38 × 184",
        "2x10" => "38 × 235",
        "2x12" => "38 × 286",
        _ => return None,
    })
}
