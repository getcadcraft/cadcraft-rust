//! The parity job's copy / paste / swap steps (`cadcraft-cli bimdraw`; forks/tools/bmclipdiff.mjs runs the same steps on
//! the frozen JS: ifc-edit.js placeLike / swapItem / removeElements / snapshot / pasteElements, ifc-place.js
//! placements / info, clipboard.js). Elements are named by a pick: an express ID, `{placed: i}` (the i-th of
//! `placements()`), `{cls: "IFCDOOR", i}` (the i-th line of that class), `{level: i}`, `{wall: i}`, `{opening: i}`.

use std::collections::HashMap;
use std::sync::Arc;

use cc_io_ifc::model::Id;
use serde_json::{Map, Value as J, json};

use crate::api::{JsErr, R};
use crate::draw::clip::{self, Box3, ClipMeta, ElRow, ElementsCtx, PlacementCtx};
use crate::draw::editor::Editor;
use crate::draw::ops::Snapshot;
use crate::draw::place::{PasteOpts, PlaceOpts, PlacementRow, Placer, Snap};
use crate::js::jn;
use crate::wapi::P;

fn num(v: Option<&J>) -> Option<f64> {
    v.and_then(J::as_f64)
}
fn s(v: Option<&J>) -> String {
    v.and_then(J::as_str).unwrap_or("").to_string()
}

/// A property value as JSON.
pub fn pj(v: &P) -> J {
    match v {
        P::Undef => J::Null,
        P::Null => J::Null,
        P::Bool(b) => J::Bool(*b),
        P::Num(n) => jn(*n),
        P::Str(t) => J::String(t.lossy().into_owned()),
        P::Arr(a) => J::Array(a.iter().map(pj).collect()),
        P::Obj => J::String("[object Object]".into()),
    }
}

/// A `placements()` row as the harness compares it.
pub fn row_json(p: &PlacementRow) -> J {
    let mut eq = Map::new();
    for (k, v) in &p.equipment {
        eq.insert(k.clone(), pj(v));
    }
    json!({
        "id": jn(p.id), "name": p.name, "cls": p.cls, "storey": p.storey.map_or(J::Null, jn), "typeId": p.type_id.map_or(J::Null, jn),
        "x": jn(p.x), "y": jn(p.y), "rot": jn(p.rot), "itemNumber": p.item_number, "quantity": pj(&p.quantity), "notes": p.notes,
        "library": p.library, "uuid": p.uuid, "version": pj(&p.version), "sha256": p.sha256, "license": p.license,
        "attribution": p.attribution, "sourceFile": p.source_file, "equipment": J::Object(eq),
    })
}

fn pick(ed: &mut Editor, sn: &Snapshot, v: Option<&J>) -> R<Id> {
    let v = v.ok_or_else(|| JsErr("no pick".into()))?;
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }
    let i = |k: &str| v.get(k).and_then(J::as_u64).and_then(|x| usize::try_from(x).ok());
    if let Some(k) = i("placed") {
        let rows = Placer::new(&mut ed.api)?.placements()?;
        return rows.get(k).map(|r| r.id).ok_or_else(|| JsErr("no such placement".into()));
    }
    if let Some(cls) = v.get("cls").and_then(J::as_str) {
        let k = i("i").unwrap_or(0);
        return ed.api.ids(&cls.to_ascii_uppercase(), false).get(k).copied().ok_or_else(|| JsErr("no such element".into()));
    }
    if let Some(k) = i("level") {
        return sn.levels.get(k).map(|l| l.id).ok_or_else(|| JsErr("no level".into()));
    }
    if let Some(k) = i("wall") {
        return sn.walls.get(k).map(|w| w.id).ok_or_else(|| JsErr("no wall".into()));
    }
    if let Some(k) = i("opening") {
        return sn.openings.get(k).map(|w| w.id).ok_or_else(|| JsErr("no opening".into()));
    }
    Err(JsErr("bad pick".into()))
}

fn picks(ed: &mut Editor, sn: &Snapshot, v: Option<&J>) -> R<Vec<Id>> {
    let mut out = Vec::new();
    for p in v.and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        out.push(pick(ed, sn, Some(p))?);
    }
    Ok(out)
}

fn item_of(o: &Map<String, J>) -> R<J> {
    match (o.get("item"), o.get("itemFile").and_then(J::as_str)) {
        (Some(i), _) => Ok(i.clone()),
        (None, Some(f)) => std::fs::read(f).ok().and_then(|b| serde_json::from_slice::<J>(&b).ok()).ok_or_else(|| JsErr("no item".into())),
        _ => Err(JsErr("no item".into())),
    }
}

fn box_of(v: Option<&J>) -> Option<Box3> {
    let b = v?.as_object()?;
    let g = |k: &str| b.get(k).and_then(J::as_f64);
    Some(Box3 { x0: g("x0")?, y0: g("y0")?, z0: g("z0")?, x1: g("x1")?, y1: g("y1")?, z1: g("z1")? })
}

/// One copy / paste / swap step.
pub fn step(ed: &mut Editor, sn: &Snapshot, snaps: &mut HashMap<String, Arc<Snap>>, op: &str, o: &Map<String, J>, now: f64) -> R<J> {
    let ms = if now.is_finite() { now.rem_euclid(1000.0).floor() } else { 0.0 };
    let now_iso = format!("{}.{:0>3}Z", crate::draw::editor::iso_seconds(now), crate::js::num_str(ms));
    match op {
        "placements" => {
            let rows = Placer::new(&mut ed.api)?.placements()?;
            Ok(J::Array(rows.iter().map(row_json).collect()))
        }
        "info" => {
            let ss = Placer::new(&mut ed.api)?.info()?;
            Ok(J::Array(ss.iter().map(|s| json!({ "id": jn(s.id), "name": s.name, "elevation": jn(s.elevation) })).collect()))
        }
        "placeLike" => {
            let src = pick(ed, sn, o.get("src"))?;
            let storey =
                o.get("level").and_then(J::as_u64).and_then(|i| usize::try_from(i).ok()).and_then(|i| sn.levels.get(i)).map(|l| l.id);
            let opts = PlaceOpts {
                storey: storey.unwrap_or(-1.0),
                x: num(o.get("x")).unwrap_or(0.0),
                y: num(o.get("y")).unwrap_or(0.0),
                rot: num(o.get("rot")).unwrap_or(0.0),
                item_number: s(o.get("itemNumber")),
                quantity: num(o.get("quantity")).unwrap_or(1.0),
                notes: s(o.get("notes")),
                ..PlaceOpts::default()
            };
            let id = ed.place_like(src, opts, &s(o.get("label")))?;
            Ok(json!({ "ok": true, "id": jn(id) }))
        }
        "swapItem" => {
            let target = pick(ed, sn, o.get("target"))?;
            let item = item_of(o)?;
            let opts = PlaceOpts {
                library: s(o.get("library")),
                sha256: s(o.get("sha256")),
                source_file: s(o.get("sourceFile")),
                ..PlaceOpts::default()
            };
            let old = o.get("oldName").and_then(J::as_str);
            let what = o.get("what").and_then(J::as_str).unwrap_or("swap");
            let r = ed.swap_item(target, Arc::new(item), opts, old, what)?;
            Ok(json!({
                "ok": true, "typeCreated": r.type_created, "classDiffers": r.class_differs,
                "classChanged": r.class_changed.map_or(J::Null, |(a, b)| json!([a, b])),
            }))
        }
        "removeAny" => {
            let list = picks(ed, sn, o.get("picks"))?;
            let r = ed.remove_elements(&list, &s(o.get("label")))?;
            Ok(json!({
                "removed": r.removed.iter().map(|x| jn(*x)).collect::<Vec<_>>(),
                "refused": r.refused.iter().map(|(id, why)| json!([jn(*id), why])).collect::<Vec<_>>(),
            }))
        }
        "snapshotEls" => {
            let list = picks(ed, sn, o.get("picks"))?;
            let snap = ed.snapshot(&list)?;
            let out = json!({
                "top": snap.top.iter().map(|x| jn(*x)).collect::<Vec<_>>(),
                "storey": snap.storey.map_or(J::Null, jn),
                "n": snap.elements.len(),
                "els": snap.elements.iter().map(|e| json!([jn(e.id), e.top, e.psets.len(), e.parts.len(), e.voids.len(), e.fills.is_some(), e.placement_pset.as_ref().map_or(J::Null, |p| J::String(p.item_number.clone()))])).collect::<Vec<_>>(),
                "frame": snap.frame.map_or(J::Null, |f| json!({ "rot": jn(f.rot), "x": jn(f.x), "y": jn(f.y), "box": [jn(f.bx[0]), jn(f.bx[1]), jn(f.bx[2]), jn(f.bx[3]), jn(f.bx[4]), jn(f.bx[5])] })),
            });
            snaps.insert(s(o.get("name")), Arc::new(snap));
            Ok(out)
        }
        "pasteEls" => {
            let snap = snaps.get(&s(o.get("snap"))).cloned().ok_or_else(|| JsErr("no snapshot".into()))?;
            let storey = match o.get("level").and_then(J::as_u64).and_then(|i| usize::try_from(i).ok()) {
                Some(i) => sn.levels.get(i).map(|l| l.id),
                None => None,
            };
            let about = o.get("about").and_then(J::as_array).and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]));
            let opts = PasteOpts {
                storey,
                dx: num(o.get("dx")).unwrap_or(0.0),
                dy: num(o.get("dy")).unwrap_or(0.0),
                rot: num(o.get("rot")).unwrap_or(0.0),
                about,
                keep_numbers: o.get("keepNumbers").and_then(J::as_bool).unwrap_or(false),
            };
            let r = ed.paste_elements(snap, opts, &s(o.get("label")))?;
            Ok(json!({ "ids": r.ids.iter().map(|x| jn(*x)).collect::<Vec<_>>(), "all": r.all.len() }))
        }
        // bimcraft.js copyPicked for ONE placed item: clipboard.js placementClip with the box the page gives it
        "clipPlacement" => {
            let k = o.get("placed").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
            let mut pl = Placer::new(&mut ed.api)?;
            let rows = pl.placements()?;
            let p = rows.get(k).ok_or_else(|| JsErr("no such placement".into()))?;
            let st = pl.info()?.into_iter().find(|x| Some(x.id) == p.storey);
            let item = o.get("item").filter(|i| !i.is_null()).cloned().map(Arc::new);
            let meta = o.get("meta").and_then(J::as_object).map(|m| ClipMeta {
                library: s(m.get("library")),
                sha256: s(m.get("sha256")),
                source_file: s(m.get("sourceFile")),
            });
            let bx = match &item {
                Some(it) => Some(clip::box_from_item(it)),
                None => clip::box_from_equipment(p, pl.w.unit_mm),
            };
            let c = clip::placement_clip(
                p,
                &PlacementCtx {
                    model: s(o.get("model")),
                    file: s(o.get("file")),
                    storey_name: st.as_ref().map(|x| x.name.clone()).unwrap_or_default(),
                    storey_elevation: st.as_ref().map(|x| x.elevation),
                    item,
                    meta,
                    bx,
                    now: now_iso,
                },
            );
            let ghost = clip::ghost_item(&c);
            let beside = clip::beside_spot(&c);
            Ok(json!({ "clip": c.json(), "ghost": ghost, "beside": [jn(beside.0), jn(beside.1), jn(beside.2)] }))
        }
        // copyElements' clip from a snapshot's frame (the elements' names and classes from their lines)
        "clipElements" => {
            let snap = snaps.get(&s(o.get("snap"))).cloned().ok_or_else(|| JsErr("no snapshot".into()))?;
            let list = picks(ed, sn, o.get("picks"))?;
            let mut rows = Vec::new();
            for id in list {
                let l = ed.api.must(ed.now(id))?;
                rows.push(ElRow { id, name: l.a("Name").text_or_empty(), cls: l.class.to_string(), guid: l.a("GlobalId").text_or_empty() });
            }
            let info = Placer::new(&mut ed.api)?.info()?;
            let st = info.iter().find(|x| Some(x.id) == snap.storey);
            let f = snap.frame;
            let c = clip::elements_clip(
                &rows,
                &ElementsCtx {
                    model: s(o.get("model")),
                    file: s(o.get("file")),
                    now: now_iso,
                    storey: snap.storey,
                    storey_name: st.map(|x| x.name.clone()).unwrap_or_default(),
                    storey_elevation: st.map(|x| x.elevation),
                    x: f.map_or(0.0, |f| f.x),
                    y: f.map_or(0.0, |f| f.y),
                    rot: f.map_or(0.0, |f| f.rot),
                    bx: f.map(|f| Box3 { x0: f.bx[0], y0: f.bx[1], z0: f.bx[2], x1: f.bx[3], y1: f.bx[4], z1: f.bx[5] }),
                    cut: o.get("cut").and_then(J::as_bool).unwrap_or(false),
                },
            );
            let at_x = num(o.get("x")).unwrap_or(0.0);
            let at_y = num(o.get("y")).unwrap_or(0.0);
            let at_rot = num(o.get("rot")).unwrap_or(0.0);
            let (dx, dy, rot, about) = clip::paste_move(&c, at_x, at_y, at_rot);
            let beside = clip::beside_spot(&c);
            let ss: Vec<_> = info.clone();
            let ps = clip::pick_storey(&c, &ss, o.get("same").and_then(J::as_bool).unwrap_or(true));
            Ok(json!({
                "clip": c.json(),
                "move": { "dx": jn(dx), "dy": jn(dy), "rot": jn(rot), "about": [jn(about[0]), jn(about[1])] },
                "beside": [jn(beside.0), jn(beside.1), jn(beside.2)],
                "storey": ps.map_or(J::Null, |(id, how)| json!({ "id": jn(id), "how": how })),
            }))
        }
        // the editor's copy / paste / swap / delete on this model, each undone again, best of 3 (ms; bmclipdiff.mjs --bench)
        "clipBench" => {
            let list = picks(ed, sn, o.get("picks"))?;
            let placed = pick(ed, sn, o.get("placed"))?;
            let item = Arc::new(item_of(o)?);
            let storey = sn.levels.first().map_or(-1.0, |l| l.id);
            let mut best = [f64::INFINITY; 5];
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let snap = Arc::new(ed.snapshot(&list)?);
                best[0] = best[0].min(t.elapsed().as_secs_f64() * 1000.0);
                let t = std::time::Instant::now();
                ed.paste_elements(snap, PasteOpts { dx: 5000.0, rot: 90.0, ..PasteOpts::default() }, "")?;
                best[1] = best[1].min(t.elapsed().as_secs_f64() * 1000.0);
                ed.undo()?;
                let t = std::time::Instant::now();
                ed.remove_elements(&list, "")?;
                best[2] = best[2].min(t.elapsed().as_secs_f64() * 1000.0);
                ed.undo()?;
                let t = std::time::Instant::now();
                ed.place_like(
                    placed,
                    PlaceOpts { storey, x: 1000.0, y: 1000.0, item_number: "9".into(), quantity: 1.0, ..PlaceOpts::default() },
                    "",
                )?;
                best[3] = best[3].min(t.elapsed().as_secs_f64() * 1000.0);
                ed.undo()?;
                let t = std::time::Instant::now();
                ed.swap_item(placed, item.clone(), PlaceOpts { sha256: "ab".repeat(32), ..PlaceOpts::default() }, None, "swap")?;
                best[4] = best[4].min(t.elapsed().as_secs_f64() * 1000.0);
                ed.undo()?;
            }
            Ok(json!({ "snapshot": best[0], "paste": best[1], "remove": best[2], "placeLike": best[3], "swap": best[4] }))
        }
        // clipboard.js's pure helpers on given inputs
        "clipFns" => {
            let item = o.get("item").cloned().unwrap_or(J::Null);
            let b = clip::box_from_item(&item);
            Ok(json!({ "boxFromItem": b.json(), "box": box_of(o.get("box")).map_or(J::Null, |b| b.json()) }))
        }
        other => Err(JsErr(format!("unknown step {other}"))),
    }
}
