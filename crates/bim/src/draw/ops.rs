//! model-ops.js — what the building tools do to the model, as changes for the editor: each op writes through one
//! ModelWriter (one journal, one undo step), names storeys and elements by GlobalId, and fixes the GlobalIds it makes
//! the first time it runs, so a redo makes the same elements again. Every op ends by bringing the walls it may have
//! changed up to date (joins, heights that follow a level, quantities) and what follows them (rooms, floors, roofs).

use std::collections::HashMap;

use cc_io_ifc::model::Id;

use crate::api::R;
use crate::draw::levels::{Level, LevelChange, add_level, read_levels, set_level};
use crate::draw::m::{V2, dist};
use crate::draw::openings::{NewOpening, Opening, OpeningChange, add_opening, delete_openings, read_openings, update_opening};
use crate::draw::slabs::{
    Derived, DerivedChange, NewRoof, StoreyLoops, add_ceiling, add_floor, add_roof, delete_derived, read_derived, storey_loops,
    sync_derived, update_derived,
};
use crate::draw::walls::{NewWall, Wall, WallChange, add_wall, delete_walls, read_walls, regen_walls, update_wall};
use crate::draw::writer::{Journal, W, key_err};
use crate::wapi::Api;

/// What an op returns.
#[derive(Clone, Debug, Default)]
pub struct OpResult {
    pub journal: Journal,
    pub id: Option<Id>,
    pub ids: Vec<Id>,
    /// GlobalId → id of what it made (insertion order)
    pub made: Vec<(String, Id)>,
}

/// One wall of a drawWalls op.
#[derive(Clone, Debug, PartialEq)]
pub struct SegSpec {
    pub a: V2,
    pub b: V2,
    pub type_key: Option<String>,
    pub ext_side: Option<String>,
    pub guid: Option<String>,
}

/// A building-tool change (its GlobalIds are fixed into it the first time it runs).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    DrawWalls {
        storey: String,
        segs: Vec<SegSpec>,
        type_key: String,
        base_offset: f64,
        top_level: String,
        top_offset: f64,
        height: f64,
        ext_side: String,
    },
    ChangeWall {
        guid: String,
        changes: WallChange,
        carry_joined: bool,
    },
    ChangeWalls {
        guids: Vec<String>,
        changes: WallChange,
    },
    PlaceOpening(NewOpening),
    ChangeOpening {
        guid: String,
        changes: OpeningChange,
    },
    ChangeOpenings {
        guids: Vec<String>,
        changes: OpeningChange,
    },
    RemoveOpenings {
        guids: Vec<String>,
    },
    RemoveElements {
        walls: Vec<String>,
        openings: Vec<String>,
        derived: Vec<String>,
    },
    PlaceFloor {
        storey: String,
        seed: V2,
        mode: String,
        kind: String,
        thickness: f64,
        offset: f64,
        guid: Option<String>,
    },
    PlaceCeiling {
        storey: String,
        seed: V2,
        mode: String,
        height: f64,
        thickness: f64,
        guid: Option<String>,
    },
    PlaceRoof {
        storey: String,
        spec: NewRoof,
    },
    ChangeDerived {
        guids: Vec<String>,
        changes: DerivedChange,
    },
    RemoveWalls {
        guids: Vec<String>,
    },
    NewLevel {
        name: String,
        elevation: f64,
        height: f64,
        guid: Option<String>,
    },
    ChangeLevel {
        guid: String,
        changes: LevelChange,
    },
    /// several ops as one change; `keep_first`: the result is the first's (chainClose), else the last's (mergeR)
    Seq {
        ops: Vec<Op>,
        keep_first: bool,
    },
}

fn storey_id(w: &mut W<'_>, guid: &str) -> R<Id> {
    w.by_guid(guid)?.ok_or_else(|| key_err("model.err.nolevel"))
}

/// After doors / windows changed in these walls: the walls (RC: what follows them is not ported; models with
/// residential elements are refused by the caller).
fn after_openings(w: &mut W<'_>, guids: &[String]) -> R<()> {
    let walls = read_walls(w, None)?;
    for g in guids {
        if let Some(x) = walls.iter().find(|y| y.guid == *g)
            && let Some(st) = x.storey
        {
            regen_walls(w, st, Some(std::slice::from_ref(g)))?;
        }
    }
    Ok(())
}

/// Every storey's walls made again (after a level change), and what follows them.
pub fn regen_all(w: &mut W<'_>) -> R<usize> {
    let mut n = 0usize;
    for l in read_levels(w)? {
        n = n.saturating_add(regen_walls(w, l.id, None)?);
        sync_derived(w, l.id)?;
    }
    Ok(n)
}

/// After walls changed on these storeys: joins, then rooms / floors / ceilings / roofs.
fn settle(w: &mut W<'_>, storeys: &[Option<Id>], force: Option<&[String]>) -> R<()> {
    let mut seen: Vec<Id> = Vec::new();
    for s in storeys.iter().flatten() {
        if seen.contains(s) {
            continue;
        }
        seen.push(*s);
        regen_walls(w, *s, force)?;
        sync_derived(w, *s)?;
    }
    Ok(())
}

/// Doors and windows of walls that moved or changed keep their place in the world (or fit themselves in again).
fn after_wall_move(w: &mut W<'_>, before: &[Wall], ops0: &[Opening], guids: &[String], rethick: bool) -> R<()> {
    let after = read_walls(w, None)?;
    for g in guids {
        let (Some(a), Some(b)) = (before.iter().find(|x| x.guid == *g), after.iter().find(|x| x.guid == *g)) else { continue };
        let mine: Vec<&Opening> = ops0.iter().filter(|o| o.wall_guid == *g).collect();
        if mine.is_empty() {
            continue;
        }
        let ua = [(a.b[0] - a.a[0]) / a.len, (a.b[1] - a.a[1]) / a.len];
        let ub = [(b.b[0] - b.a[0]) / b.len, (b.b[1] - b.a[1]) / b.len];
        let same_line = (ua[0] * ub[1] - ua[1] * ub[0]).abs() < 1e-9 && ua[0] * ub[0] + ua[1] * ub[1] > 0.0;
        let shift = if same_line { (b.a[0] - a.a[0]) * ua[0] + (b.a[1] - a.a[1]) * ua[1] } else { 0.0 };
        let moved = shift.abs() > 1e-6 || (a.len - b.len).abs() > 1e-6 || (a.t - b.t).abs() > 1e-6 || rethick;
        if !moved {
            continue;
        }
        for o in mine {
            update_opening(w, &o.guid, &OpeningChange { offset: Some(o.offset - shift), ..OpeningChange::default() })?;
        }
    }
    Ok(())
}

impl Op {
    /// Run the op on the model: one journal (an error undoes what it wrote).
    pub fn run(&mut self, api: &mut Api) -> R<OpResult> {
        if let Op::Seq { ops, keep_first } = self {
            let mut acc: Option<OpResult> = None;
            for op in ops.iter_mut() {
                let r = op.run(api)?;
                acc = Some(match acc {
                    None => r,
                    Some(a) => {
                        let j = Journal::merge(a.journal.clone(), r.journal.clone());
                        if *keep_first { OpResult { journal: j, ..a } } else { OpResult { journal: j, ..r } }
                    }
                });
            }
            return Ok(acc.unwrap_or_default());
        }
        let mut w = W::new(api)?;
        let ((id, ids, made), journal) = w.run(|w| self.body(w))?;
        Ok(OpResult { journal, id, ids, made })
    }

    #[allow(clippy::type_complexity)]
    fn body(&mut self, w: &mut W<'_>) -> R<(Option<Id>, Vec<Id>, Vec<(String, Id)>)> {
        match self {
            Op::DrawWalls { storey, segs, type_key, base_offset, top_level, top_offset, height, ext_side } => {
                for s in segs.iter_mut() {
                    if s.guid.as_deref().is_none_or(str::is_empty) {
                        s.guid = w.guid().val_str();
                    }
                }
                let st = storey_id(w, storey)?;
                let mut made: Vec<(String, Id)> = Vec::new();
                for s in segs.iter() {
                    let spec = NewWall {
                        storey: st,
                        a: s.a,
                        b: s.b,
                        type_key: s.type_key.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| type_key.clone()),
                        base_offset: *base_offset,
                        top_level: top_level.clone(),
                        top_offset: *top_offset,
                        height: *height,
                        ext_side: s.ext_side.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| ext_side.clone()),
                        guid: s.guid.clone(),
                    };
                    let (id, g) = add_wall(w, &spec)?;
                    match made.iter_mut().find(|x| x.0 == g) {
                        Some(x) => x.1 = id,
                        None => made.push((g, id)),
                    }
                }
                settle(w, &[Some(st)], None)?;
                let ids: Vec<Id> = made.iter().map(|x| x.1).collect();
                Ok((ids.last().copied(), ids, made))
            }
            Op::ChangeWall { guid, changes, carry_joined } => {
                let before = read_walls(w, None)?;
                let wall = before.iter().find(|x| x.guid == *guid).cloned().ok_or_else(|| key_err("model.err.nowall"))?;
                let ops0 = read_openings(w, None, &before)?;
                let guids = update_wall(w, guid, changes, *carry_joined)?;
                after_wall_move(w, &before, &ops0, &guids, changes.type_key.as_deref().is_some_and(|k| !k.is_empty()))?;
                settle(w, &[wall.storey], Some(&guids))?;
                let id = w.by_guid(guid)?;
                Ok((id, id.into_iter().collect(), Vec::new()))
            }
            Op::ChangeWalls { guids, changes } => {
                let all = read_walls(w, None)?;
                let ops0 = read_openings(w, None, &all)?;
                let mut storeys: Vec<Option<Id>> = Vec::new();
                for g in guids.iter() {
                    let Some(wall) = all.iter().find(|x| x.guid == *g) else { continue };
                    update_wall(w, g, changes, false)?;
                    if !storeys.contains(&wall.storey) {
                        storeys.push(wall.storey);
                    }
                }
                after_wall_move(w, &all, &ops0, guids, changes.type_key.as_deref().is_some_and(|k| !k.is_empty()))?;
                settle(w, &storeys, Some(guids))?;
                let mut ids = Vec::new();
                for g in guids.iter() {
                    if let Some(id) = w.by_guid(g)? {
                        ids.push(id);
                    }
                }
                Ok((ids.first().copied(), ids, Vec::new()))
            }
            Op::PlaceOpening(s) => {
                if s.guid.as_deref().is_none_or(str::is_empty) {
                    s.guid = w.guid().val_str();
                    s.opening_guid = w.guid().val_str();
                }
                let (id, g, og) = add_opening(w, s)?;
                after_openings(w, std::slice::from_ref(&s.wall))?;
                let oid = w.by_guid(&og)?;
                let mut made = vec![(g, id)];
                if let Some(o) = oid {
                    made.push((og, o));
                }
                Ok((Some(id), vec![id], made))
            }
            Op::ChangeOpening { guid, changes } => {
                let walls = update_opening(w, guid, changes)?;
                after_openings(w, &walls)?;
                let id = w.by_guid(guid)?;
                Ok((id, id.into_iter().collect(), Vec::new()))
            }
            Op::ChangeOpenings { guids, changes } => {
                let mut hosts: Vec<String> = Vec::new();
                for g in guids.iter() {
                    for h in update_opening(w, g, changes)? {
                        if !hosts.contains(&h) {
                            hosts.push(h);
                        }
                    }
                }
                after_openings(w, &hosts)?;
                let mut ids = Vec::new();
                for g in guids.iter() {
                    if let Some(id) = w.by_guid(g)? {
                        ids.push(id);
                    }
                }
                Ok((ids.first().copied(), ids, Vec::new()))
            }
            Op::RemoveOpenings { guids } => {
                let walls = delete_openings(w, guids)?;
                after_openings(w, &walls)?;
                Ok((None, Vec::new(), Vec::new()))
            }
            Op::RemoveElements { walls, openings, derived } => {
                let mut storeys: Vec<Option<Id>> = Vec::new();
                let mut hosts: Vec<String> = Vec::new();
                if !derived.is_empty() {
                    delete_derived(w, derived)?;
                }
                if !openings.is_empty() {
                    for g in delete_openings(w, openings)? {
                        if !hosts.contains(&g) {
                            hosts.push(g);
                        }
                    }
                }
                if !walls.is_empty() {
                    for s in delete_walls(w, walls)? {
                        if !storeys.contains(&Some(s)) {
                            storeys.push(Some(s));
                        }
                    }
                }
                let all = read_walls(w, None)?;
                for g in &hosts {
                    if let Some(x) = all.iter().find(|y| y.guid == *g)
                        && !storeys.contains(&x.storey)
                    {
                        storeys.push(x.storey);
                    }
                }
                settle(w, &storeys, Some(&hosts))?;
                Ok((None, Vec::new(), Vec::new()))
            }
            Op::PlaceFloor { storey, seed, mode, kind, thickness, offset, guid } => {
                if guid.as_deref().is_none_or(str::is_empty) {
                    *guid = w.guid().val_str();
                }
                let st = storey_id(w, storey)?;
                let (id, g) = add_floor(w, st, *seed, mode, kind, *thickness, *offset, guid.as_deref())?;
                Ok((Some(id), vec![id], vec![(g, id)]))
            }
            Op::PlaceCeiling { storey, seed, mode, height, thickness, guid } => {
                if guid.as_deref().is_none_or(str::is_empty) {
                    *guid = w.guid().val_str();
                }
                let st = storey_id(w, storey)?;
                let (id, g) = add_ceiling(w, st, *seed, mode, *height, *thickness, guid.as_deref())?;
                Ok((Some(id), vec![id], vec![(g, id)]))
            }
            Op::PlaceRoof { storey, spec } => {
                if spec.guid.as_deref().is_none_or(str::is_empty) {
                    spec.guid = w.guid().val_str();
                }
                let st = storey_id(w, storey)?;
                let mut s = spec.clone();
                s.storey = st;
                let (id, g) = add_roof(w, &s)?;
                Ok((Some(id), vec![id], vec![(g, id)]))
            }
            Op::ChangeDerived { guids, changes } => {
                let mut storeys: Vec<Id> = Vec::new();
                for g in guids.iter() {
                    let (_, st) = update_derived(w, g, changes)?;
                    if let Some(s) = st
                        && !storeys.contains(&s)
                    {
                        storeys.push(s);
                    }
                }
                for st in storeys {
                    sync_derived(w, st)?;
                }
                let mut ids = Vec::new();
                for g in guids.iter() {
                    if let Some(id) = w.by_guid(g)? {
                        ids.push(id);
                    }
                }
                Ok((ids.first().copied(), ids, Vec::new()))
            }
            Op::RemoveWalls { guids } => {
                let st = delete_walls(w, guids)?;
                let s: Vec<Option<Id>> = st.into_iter().map(Some).collect();
                settle(w, &s, None)?;
                Ok((None, Vec::new(), Vec::new()))
            }
            Op::NewLevel { name, elevation, height, guid } => {
                if guid.as_deref().is_none_or(str::is_empty) {
                    *guid = w.guid().val_str();
                }
                let (id, g) = add_level(w, name, *elevation, *height, guid.as_deref())?;
                regen_all(w)?;
                Ok((Some(id), vec![id], vec![(g, id)]))
            }
            Op::ChangeLevel { guid, changes } => {
                let moved = set_level(w, guid, changes)?;
                regen_all(w)?;
                Ok((w.by_guid(guid)?, moved, Vec::new()))
            }
            Op::Seq { .. } => Ok((None, Vec::new(), Vec::new())),
        }
    }
}

/// The levels, walls, doors / windows and derived elements now (model-ops.js `snapshotModel`).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub levels: Vec<Level>,
    pub walls: Vec<Wall>,
    pub openings: Vec<Opening>,
    pub derived: Derived,
    pub unit_mm: f64,
    pub v4: bool,
}

pub fn snapshot(api: &mut Api) -> R<Snapshot> {
    let mut w = W::new(api)?;
    let walls = read_walls(&mut w, None)?;
    let levels = read_levels(&mut w)?;
    let openings = read_openings(&mut w, None, &walls)?;
    let derived = read_derived(&mut w)?;
    Ok(Snapshot { levels, walls, openings, derived, unit_mm: w.unit_mm, v4: w.v4 })
}

/// The rooms and building outlines of a level, from its walls now (`loopsOf`).
pub fn loops_of(api: &mut Api, storey: Id) -> R<StoreyLoops> {
    let mut w = W::new(api)?;
    storey_loops(&mut w, storey)
}

/// Wall endpoints within `tol` (for callers that join chains).
pub fn near(a: V2, b: V2, tol: f64) -> bool {
    dist(a, b) <= tol
}

/// The made map as a lookup.
pub fn made_map(r: &OpResult) -> HashMap<String, Id> {
    r.made.iter().cloned().collect()
}
