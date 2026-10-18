//! model-levels.js — the building's levels (IfcBuildingStorey): read with their heights, added, renamed, moved.

use cc_io_ifc::model::Id;

use crate::api::{JsErr, R};
use crate::draw::writer::{W, key_err, row};
use crate::wapi::{JV, P};

pub const LEVEL_PSET: &str = "CadCraft_Level";
pub const DEFAULT_HEIGHT: f64 = 3000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub id: Id,
    pub guid: Option<String>,
    pub name: String,
    /// world Z of the storey (mm)
    pub elevation: f64,
    pub height: f64,
    /// it has the CadCraft_Level pset
    pub own: bool,
}

/// `val(a) || val(b) || fallback` as text.
pub fn name_or(a: &JV, b: &JV, fallback: &str) -> String {
    let x = a.text_or_empty();
    if !x.is_empty() {
        return x;
    }
    let y = b.text_or_empty();
    if !y.is_empty() { y } else { fallback.to_string() }
}

/// `readLevels(w)`
pub fn read_levels(w: &mut W<'_>) -> R<Vec<Level>> {
    let mut out: Vec<(Level, Option<f64>)> = Vec::new();
    for id in w.ids("IFCBUILDINGSTOREY") {
        let s = w.must(id)?;
        let m = w.storey_world(id)?;
        let ps = w.read_pset(id, LEVEL_PSET)?;
        let h = ps.as_ref().and_then(|p| p.get("Height")).map_or(f64::NAN, P::number);
        let height = if ps.is_some() && h > 0.0 { Some(h * w.unit_mm) } else { None };
        out.push((
            Level {
                id,
                guid: s.a("GlobalId").val_str(),
                name: name_or(s.a("Name"), s.a("LongName"), &format!("#{}", crate::js::num_str(id))),
                elevation: m[14],
                height: 0.0,
                own: ps.is_some(),
            },
            height,
        ));
    }
    out.sort_by(|a, b| {
        let d = a.0.elevation - b.0.elevation;
        let d = if d != 0.0 && !d.is_nan() { d } else { a.0.id - b.0.id };
        crate::js::sign_ord(d)
    });
    let mut levels: Vec<Level> = Vec::with_capacity(out.len());
    let n = out.len();
    for i in 0..n {
        let Some((l, h)) = out.get(i).cloned() else { continue };
        let mut l = l;
        l.height = match h {
            Some(h) if h > 0.0 => h,
            _ => match out.get(i.wrapping_add(1)) {
                Some(next) => crate::draw::m::max(1000.0, next.0.elevation - l.elevation),
                None => match levels.last() {
                    Some(p) if p.height != 0.0 && !p.height.is_nan() => p.height,
                    _ => DEFAULT_HEIGHT,
                },
            },
        };
        levels.push(l);
    }
    Ok(levels)
}

/// The next level up.
pub fn level_above(levels: &[Level], id: Id) -> Option<&Level> {
    let i = levels.iter().position(|l| l.id == id)?;
    levels.get(i.wrapping_add(1))
}

/// The building the storeys hang under (the parent of the first storey, else the first IfcBuilding).
fn building_of(w: &mut W<'_>) -> R<Option<(Id, Option<Id>)>> {
    for rid in w.ids("IFCRELAGGREGATES") {
        let r = w.must(rid)?;
        let Some(parent) = r.a("RelatingObject").href() else { continue };
        if w.line(parent)?.is_some_and(|l| l.class.eq_ignore_ascii_case("IfcBuilding")) {
            return Ok(Some((parent, Some(rid))));
        }
    }
    Ok(w.ids("IFCBUILDING").first().map(|&b| (b, None)))
}

/// `addLevel(w, { name, elevation, height, guid })` → (id, guid)
pub fn add_level(w: &mut W<'_>, name: &str, elevation: f64, height: f64, guid: Option<&str>) -> R<(Id, String)> {
    let (bid, brel) = building_of(w)?.ok_or_else(|| key_err("model.err.nobuilding"))?;
    let bl = w.must(bid)?;
    let blp = bl.get("ObjectPlacement").filter(|v| v.truthy()).and_then(JV::href);
    let bz = match blp {
        Some(lp) => w.rel_world_z(lp)?,
        None => 0.0,
    };
    let lp = w.place_at(blp, 0.0, 0.0, elevation - bz, 0.0)?;
    let g = w.gid(guid);
    let oh = w.owner_history();
    let nm = w.l(name);
    let id = w.make(
        "IFCBUILDINGSTOREY",
        vec![w.gid_jv(&g), oh, nm, JV::Null, JV::Null, JV::H(lp), JV::Null, JV::Null, JV::s("ELEMENT"), JV::Num(w.u(elevation))],
    )?;
    w.note_guid(&g, id);
    match brel {
        Some(rel) => {
            let mut r = w.touch(rel)?;
            let mut list = match r.a("RelatedObjects") {
                JV::L(v) => v.clone(),
                _ => Vec::new(),
            };
            list.push(JV::H(id));
            r.set("RelatedObjects", JV::L(list));
            w.write(&mut r)?;
        }
        None => {
            let gg = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![gg, oh, JV::Null, JV::Null, JV::H(bid), JV::L(vec![JV::H(id)])])?;
        }
    }
    w.upsert_pset(id, LEVEL_PSET, &[row("Height", "IFCPOSITIVELENGTHMEASURE", P::Num(w.u(height)))])?;
    Ok((id, g))
}

/// What a level change asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelChange {
    pub name: Option<String>,
    pub elevation: Option<f64>,
    pub height: Option<f64>,
    /// default true
    pub shift_above: Option<bool>,
}

/// `setLevel(w, guid, changes)` → the storeys moved.
pub fn set_level(w: &mut W<'_>, guid: &str, ch: &LevelChange) -> R<Vec<Id>> {
    let levels = read_levels(w)?;
    let i = levels.iter().position(|l| l.guid.as_deref() == Some(guid)).ok_or_else(|| key_err("model.err.nolevel"))?;
    let l = levels.get(i).cloned().ok_or_else(|| key_err("model.err.nolevel"))?;
    if let Some(name) = &ch.name {
        let t = crate::js::trim(name);
        if !t.is_empty() && t != l.name {
            let mut s = w.touch(l.id)?;
            s.set("Name", w.l(t));
            w.write(&mut s)?;
        }
    }
    let mut dz = 0.0;
    if let Some(e) = ch.elevation.filter(|e| e.is_finite())
        && (e - l.elevation).abs() > 0.01
    {
        dz = e - l.elevation;
    }
    if let Some(h) = ch.height.filter(|h| h.is_finite() && *h > 0.0)
        && (!l.own || (h - l.height).abs() > 0.01)
    {
        w.upsert_pset(l.id, LEVEL_PSET, &[row("Height", "IFCPOSITIVELENGTHMEASURE", P::Num(w.u(h)))])?;
    }
    let mut moves: Vec<(Id, f64)> = Vec::new();
    if dz != 0.0 {
        moves.push((l.id, dz));
    }
    if ch.shift_above.unwrap_or(true) {
        let dh = ch.height.filter(|h| h.is_finite() && *h > 0.0).map_or(0.0, |h| h - l.height);
        let up = dz + dh;
        if up.abs() > 0.01 {
            for a in levels.iter().skip(i.wrapping_add(1)) {
                match moves.iter_mut().find(|(id, _)| *id == a.id) {
                    Some(m) => m.1 += up,
                    None => moves.push((a.id, up)),
                }
            }
        }
    }
    for &(id, d) in &moves {
        move_storey(w, id, d)?;
    }
    Ok(moves.into_iter().map(|(id, _)| id).collect())
}

/// Raise a storey by dz mm: its placement's Z and its Elevation attribute.
fn move_storey(w: &mut W<'_>, id: Id, dz: f64) -> R<()> {
    let s = w.must(id)?;
    if let Some(lp) = s.a("ObjectPlacement").href() {
        let pl = w.must(lp)?;
        let ax = match pl.get("RelativePlacement").filter(|v| v.truthy()).and_then(JV::href) {
            Some(a) => w.line(a)?,
            None => None,
        };
        let nums = |v: &JV| -> Vec<f64> {
            match v {
                JV::L(l) => l
                    .iter()
                    .map(|x| {
                        let n = x.val().number();
                        if n != 0.0 && !n.is_nan() { n } else { 0.0 }
                    })
                    .collect(),
                _ => Vec::new(),
            }
        };
        let loc = match ax.as_ref().and_then(|a| a.get("Location")).filter(|v| v.truthy()).and_then(JV::href) {
            Some(l) => w.line(l)?,
            None => None,
        };
        let c = loc.map(|l| nums(l.a("Coordinates"))).unwrap_or_default();
        let dir_of = |w: &W<'_>, name: &str, dflt: Vec<f64>| -> R<Vec<f64>> {
            match ax.as_ref().and_then(|a| a.get(name)).filter(|v| v.truthy()).and_then(JV::href) {
                Some(d) => Ok(w.line(d)?.map(|l| nums(l.a("DirectionRatios"))).unwrap_or_default()),
                None => Ok(dflt),
            }
        };
        let rf = dir_of(w, "RefDirection", vec![1.0, 0.0])?;
        let az = dir_of(w, "Axis", vec![0.0, 0.0, 1.0])?;
        let az2 = az.get(2).copied().unwrap_or(f64::NAN);
        if (az2 - 1.0).abs() > 1e-6 {
            return Err(key_err("model.err.tilted"));
        }
        let c0 = c.first().copied().unwrap_or(0.0);
        let c1 = c.get(1).copied().unwrap_or(0.0);
        let c2 = c.get(2).copied().unwrap_or(0.0);
        let p = w.point3(c0, c1, c2 + w.u(dz))?;
        let zd = w.dir3(0.0, 0.0, 1.0)?;
        let xd = w.dir3(rf.first().copied().unwrap_or(1.0), rf.get(1).copied().unwrap_or(0.0), 0.0)?;
        let a = w.make("IFCAXIS2PLACEMENT3D", vec![JV::H(p), JV::H(zd), JV::H(xd)])?;
        let mut t = w.touch(lp)?;
        t.set("RelativePlacement", JV::H(a));
        w.write(&mut t)?;
    }
    let mut st = w.touch(id)?;
    let e = st.a("Elevation").val().number();
    let e = if e != 0.0 && !e.is_nan() { e } else { 0.0 };
    st.set("Elevation", JV::Num(e + w.u(dz)));
    w.write(&mut st)?;
    Ok(())
}

/// A message for an error the JS throws with a `key`.
pub fn err_key(e: &JsErr) -> &str {
    &e.0
}
