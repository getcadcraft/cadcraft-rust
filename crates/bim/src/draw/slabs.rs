//! model-slabs.js — rooms, floors, ceilings and roofs over drawn walls. Each remembers a point inside what it covers
//! (its seed) and is made again from whatever loop holds that point after the walls change (`syncDerived`). A short
//! hash of what a shape was made from (ShapeKey) leaves an unchanged one alone.

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::levels::{Level, read_levels};
use crate::draw::m::{self, V2};
use crate::draw::roof::{EdgeKind, Roof, RoofSet, edge_kinds, roof_defaults, roof_geometry, write_roof_parts};
use crate::draw::rooms::{Loops, Outline, Room, area as loop_area, label_point, loop_at, loop_index, perim, plan_loops};
use crate::draw::tr::{t, t0};
use crate::draw::walls::{Wall, ps_num, ps_num0, ps_str, read_walls, wall_outlines};
use crate::draw::writer::{QRow, Row, W, key_err, round6, row};
use crate::wapi::{JV, P};

pub const ROOM_PSET: &str = "CadCraft_Room";
pub const FLOOR_PSET: &str = "CadCraft_Floor";
pub const CEILING_PSET: &str = "CadCraft_Ceiling";
pub const ROOF_PSET: &str = "CadCraft_Roof";
const IN: f64 = 25.4;

/// FLOOR_KINDS: key, thickness, material.
pub const FLOOR_KINDS: [(&str, f64, &str); 2] = [("wood", 12.0 * IN, "floor-wood"), ("concrete", 6.0 * IN, "concrete")];

/// A level's walls and the rooms / outlines they make.
#[derive(Clone, Debug, Default)]
pub struct StoreyLoops {
    pub walls: Vec<Wall>,
    pub loops: Loops,
}

/// `storeyLoops(w, storey)`
pub fn storey_loops(w: &mut W<'_>, storey: Id) -> R<StoreyLoops> {
    let walls = read_walls(w, Some(storey))?;
    let segs: Vec<_> = walls.iter().map(Wall::seg).collect();
    let polys: Vec<Vec<V2>> = wall_outlines(&segs, 1.0).into_iter().map(|(_, p)| p).collect();
    Ok(StoreyLoops { loops: plan_loops(&polys), walls })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub seed: V2,
    pub number: String,
    pub name: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Floor {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub seed: V2,
    pub mode: String,
    pub kind: String,
    pub thickness: f64,
    pub offset: f64,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ceiling {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub seed: V2,
    pub mode: String,
    pub height: f64,
    pub thickness: f64,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Derived {
    pub spaces: Vec<Space>,
    pub floors: Vec<Floor>,
    pub ceilings: Vec<Ceiling>,
    pub roofs: Vec<Roof>,
}

fn agg_parents(w: &W<'_>) -> R<std::collections::HashMap<u64, Option<Id>>> {
    let mut m = std::collections::HashMap::new();
    for rid in w.ids("IFCRELAGGREGATES") {
        let r = w.must(rid)?;
        let to = r.a("RelatingObject").href();
        for h in r.a("RelatedObjects").hrefs() {
            m.insert(id_key(h), to);
        }
    }
    Ok(m)
}

fn parts_of(w: &W<'_>, id: Id) -> R<Vec<Id>> {
    for rid in w.ids("IFCRELAGGREGATES") {
        let r = w.must(rid)?;
        if r.a("RelatingObject").href() == Some(id) {
            return Ok(r.a("RelatedObjects").hrefs());
        }
    }
    Ok(Vec::new())
}

/// `readDerived(w)`
pub fn read_derived(w: &mut W<'_>) -> R<Derived> {
    let so = w.storey_of()?;
    let agg = agg_parents(w)?;
    let um = w.unit_mm;
    let seed_of = |ps: &std::collections::HashMap<String, P>| [ps_num(ps, "SeedX") * um, ps_num(ps, "SeedY") * um];
    let mut out = Derived::default();
    for id in w.ids("IFCSPACE") {
        let Some(ps) = w.read_pset(id, ROOM_PSET)? else { continue };
        let l = w.must(id)?;
        out.spaces.push(Space {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: agg.get(&id_key(id)).copied().flatten(),
            seed: seed_of(&ps),
            number: l.a("Name").text_or_empty(),
            name: l.a("LongName").text_or_empty(),
            key: ps_str(&ps, "ShapeKey"),
        });
    }
    for id in w.ids("IFCSLAB") {
        let Some(ps) = w.read_pset(id, FLOOR_PSET)? else { continue };
        let l = w.must(id)?;
        let kind = ps_str(&ps, "Kind");
        out.floors.push(Floor {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&id_key(id)).copied(),
            seed: seed_of(&ps),
            mode: if ps.get("Mode").and_then(P::as_str).as_deref() == Some("building") { "building".into() } else { "room".into() },
            kind: if kind.is_empty() { "wood".into() } else { kind },
            thickness: ps_num(&ps, "Thickness") * um,
            offset: ps_num0(&ps, "Offset") * um,
            key: ps_str(&ps, "ShapeKey"),
        });
    }
    for id in w.ids("IFCCOVERING") {
        let Some(ps) = w.read_pset(id, CEILING_PSET)? else { continue };
        let l = w.must(id)?;
        out.ceilings.push(Ceiling {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&id_key(id)).copied(),
            seed: seed_of(&ps),
            mode: if ps.get("Mode").and_then(P::as_str).as_deref() == Some("building") { "building".into() } else { "room".into() },
            height: ps_num(&ps, "Height") * um,
            thickness: ps_num(&ps, "Thickness") * um,
            key: ps_str(&ps, "ShapeKey"),
        });
    }
    for id in w.ids("IFCROOF") {
        let Some(ps) = w.read_pset(id, ROOF_PSET)? else { continue };
        let l = w.must(id)?;
        let present = |k: &str| ps.get(k).is_some_and(|v| !matches!(v, P::Null | P::Undef));
        let soffit = match ps.get("Soffit") {
            Some(P::Bool(true)) => true,
            Some(v) => {
                let s = if matches!(v, P::Null | P::Undef) { String::new() } else { v.string() };
                let lo = s.to_ascii_lowercase();
                matches!(lo.as_str(), "t" | ".t" | "t." | ".t." | "true")
            }
            None => false,
        };
        out.roofs.push(Roof {
            id,
            guid: l.a("GlobalId").val_str().unwrap_or_default(),
            storey: so.get(&id_key(id)).copied(),
            walls: ps_str(&ps, "WallsLevel"),
            seed: seed_of(&ps),
            pitch: ps_num(&ps, "PitchDegrees"),
            overhang: ps_num(&ps, "Overhang") * um,
            thickness: ps_num(&ps, "Thickness") * um,
            gables: ps_str(&ps, "Gables").split(',').filter(|x| !x.is_empty()).map(crate::js::str_num).collect(),
            key: ps_str(&ps, "ShapeKey"),
            parts: parts_of(w, id)?,
            edges: ps_str(&ps, "Edges"),
            rake: if present("Rake") { Some(ps_num(&ps, "Rake") * um) } else { None },
            plate: ps_num0(&ps, "PlateHeight") * um,
            fascia: if present("FasciaHeight") { ps_num(&ps, "FasciaHeight") * um } else { 0.0 },
            soffit,
        });
    }
    Ok(out)
}

/// A value of a shape key's parts (JSON.stringify with numbers rounded to 0.01).
#[derive(Clone, Debug)]
pub enum K {
    N(f64),
    S(String),
    B(bool),
    Null,
    A(Vec<K>),
}

pub fn kpts(p: &[V2]) -> K {
    K::A(p.iter().map(|q| K::A(vec![K::N(q[0]), K::N(q[1])])).collect())
}
pub fn kloops(h: &[Vec<V2>]) -> K {
    K::A(h.iter().map(|x| kpts(x)).collect())
}

fn kjson(k: &K, out: &mut String) {
    match k {
        K::N(x) => {
            let r = m::round(x * 100.0) / 100.0;
            if r.is_finite() {
                out.push_str(&crate::js::num_str(if r == 0.0 { 0.0 } else { r }));
            } else {
                out.push_str("null");
            }
        }
        K::S(s) => {
            let j = serde_json::to_string(s).unwrap_or_default();
            out.push_str(&j);
        }
        K::B(b) => out.push_str(if *b { "true" } else { "false" }),
        K::Null => out.push_str("null"),
        K::A(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                kjson(x, out);
            }
            out.push(']');
        }
    }
}

/// `shapeKey(...parts)`: FNV-1a over the JSON text's UTF-16 units, base 36, then its length in base 36.
pub fn shape_key(parts: Vec<K>) -> String {
    let mut s = String::new();
    kjson(&K::A(parts), &mut s);
    let mut h: u32 = 2_166_136_261;
    let mut n = 0u32;
    for u in s.encode_utf16() {
        h ^= u32::from(u);
        h = h.wrapping_mul(16_777_619);
        n = n.wrapping_add(1);
    }
    format!("{}{}", base36(u64::from(h)), base36(u64::from(n)))
}

fn base36(mut x: u64) -> String {
    const D: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if x == 0 {
        return "0".into();
    }
    let mut v = Vec::new();
    while x > 0 {
        v.push(D.get(usize::try_from(x % 36).unwrap_or(0)).copied().unwrap_or(b'0'));
        x /= 36;
    }
    v.reverse();
    String::from_utf8(v).unwrap_or_default()
}

/// A level and the one above it.
pub struct Lv {
    pub level: Level,
    pub above: Option<Level>,
    pub index: usize,
}
pub fn level_of(levels: &[Level], storey: Id) -> Option<Lv> {
    let i = levels.iter().position(|l| l.id == storey)?;
    Some(Lv { level: levels.get(i)?.clone(), above: levels.get(i.wrapping_add(1)).cloned(), index: i })
}

fn space_h(lv: &Lv) -> f64 {
    match &lv.above {
        Some(a) => a.elevation - lv.level.elevation,
        None => lv.level.height,
    }
}

fn seed_rows(w: &W<'_>, sd: V2, key: &str) -> Vec<Row> {
    vec![
        row("SeedX", "IFCLENGTHMEASURE", P::Num(round6(w.u(sd[0])))),
        row("SeedY", "IFCLENGTHMEASURE", P::Num(round6(w.u(sd[1])))),
        row("ShapeKey", "IFCIDENTIFIER", P::s(key)),
    ]
}

fn write_space(w: &mut W<'_>, id: Id, room: &Room, lv: &Lv) -> R<()> {
    let h = space_h(lv);
    let key = shape_key(vec![kpts(&room.poly), kloops(&room.holes), K::N(h)]);
    let pr = w.profile_voids(&room.poly, &room.holes, None)?;
    let ex = w.extrude(pr, 0.0, h)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[ex])?;
    w.set_shape(id, &[rep])?;
    let a = room.area;
    let q: Vec<QRow> = vec![
        ("Height".into(), "length", h),
        ("NetPerimeter".into(), "length", room.perimeter),
        ("GrossPerimeter".into(), "length", perim(&room.poly)),
        ("NetFloorArea".into(), "area", a),
        ("GrossFloorArea".into(), "area", loop_area(&room.poly).abs()),
        ("NetVolume".into(), "volume", a * h),
    ];
    w.upsert_qto(id, "Qto_SpaceBaseQuantities", &q)?;
    let sd = label_point(&room.poly);
    let rows = seed_rows(w, sd, &key);
    w.upsert_pset(id, ROOM_PSET, &rows)?;
    Ok(())
}

fn add_space(w: &mut W<'_>, storey: Id, room: &Room, lv: &Lv, number: &str, name: &str) -> R<Id> {
    let st = w.must(storey)?;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let lp = w.place_at(Some(stp), 0.0, 0.0, 0.0, 0.0)?;
    let g = w.guid();
    let oh = w.owner_history();
    let mut args = vec![g, oh, w.l(number), JV::Null, JV::Null, JV::H(lp), JV::Null, w.l(name), JV::s("ELEMENT")];
    if w.v4 {
        args.extend([JV::s("SPACE"), JV::Null]);
    } else {
        args.extend([JV::s("INTERNAL"), JV::Null]);
    }
    let id = w.make("IFCSPACE", args)?;
    let mut rel = None;
    for rid in w.ids("IFCRELAGGREGATES") {
        if w.must(rid)?.a("RelatingObject").href() == Some(storey) {
            rel = Some(rid);
            break;
        }
    }
    match rel {
        Some(rid) => {
            let mut r = w.touch(rid)?;
            let mut list = match r.a("RelatedObjects") {
                JV::L(v) => v.clone(),
                _ => Vec::new(),
            };
            list.push(JV::H(id));
            r.set("RelatedObjects", JV::L(list));
            w.write(&mut r)?;
        }
        None => {
            let g = w.guid();
            let oh = w.owner_history();
            w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(storey), JV::L(vec![JV::H(id)])])?;
        }
    }
    w.upsert_pset(
        id,
        "Pset_SpaceCommon",
        &[row("Reference", "IFCIDENTIFIER", P::s(number)), row("IsExternal", "IFCBOOLEAN", P::Bool(false))],
    )?;
    write_space(w, id, room, lv)?;
    Ok(id)
}

/// A loop as model-slabs reads it (poly + holes).
pub struct Lp<'a> {
    pub poly: &'a [V2],
    pub holes: &'a [Vec<V2>],
}
impl<'a> From<&'a Room> for Lp<'a> {
    fn from(r: &'a Room) -> Self {
        Lp { poly: &r.poly, holes: &r.holes }
    }
}
impl<'a> From<&'a Outline> for Lp<'a> {
    fn from(o: &'a Outline) -> Self {
        Lp { poly: &o.poly, holes: &[] }
    }
}

fn holes_area(holes: &[Vec<V2>]) -> f64 {
    holes.iter().fold(0.0, |s, h| s + loop_area(h).abs())
}

fn write_floor(w: &mut W<'_>, id: Id, thickness: f64, offset: f64, cur_key: &str, lp: &Lp<'_>) -> R<bool> {
    let key = shape_key(vec![kpts(lp.poly), kloops(lp.holes), K::N(thickness), K::N(offset)]);
    if cur_key == key {
        return Ok(false);
    }
    let pr = w.profile_voids(lp.poly, lp.holes, None)?;
    let ex = w.extrude(pr, offset - thickness, thickness)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[ex])?;
    w.set_shape(id, &[rep])?;
    let a = loop_area(lp.poly).abs() - holes_area(lp.holes);
    let q: Vec<QRow> = vec![
        ("Width".into(), "length", thickness),
        ("Perimeter".into(), "length", perim(lp.poly)),
        ("GrossArea".into(), "area", loop_area(lp.poly).abs()),
        ("NetArea".into(), "area", a),
        ("NetVolume".into(), "volume", a * thickness),
    ];
    w.upsert_qto(id, "Qto_SlabBaseQuantities", &q)?;
    let sd = label_point(lp.poly);
    let rows = seed_rows(w, sd, &key);
    w.upsert_pset(id, FLOOR_PSET, &rows)?;
    Ok(true)
}

fn write_ceiling(w: &mut W<'_>, id: Id, height: f64, thickness: f64, cur_key: &str, lp: &Lp<'_>) -> R<bool> {
    let key = shape_key(vec![kpts(lp.poly), kloops(lp.holes), K::N(height), K::N(thickness)]);
    if cur_key == key {
        return Ok(false);
    }
    let pr = w.profile_voids(lp.poly, lp.holes, None)?;
    let ex = w.extrude(pr, height, thickness)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[ex])?;
    w.set_shape(id, &[rep])?;
    let a = loop_area(lp.poly).abs() - holes_area(lp.holes);
    let q: Vec<QRow> =
        vec![("Width".into(), "length", thickness), ("GrossArea".into(), "area", loop_area(lp.poly).abs()), ("NetArea".into(), "area", a)];
    w.upsert_qto(id, "Qto_CoveringBaseQuantities", &q)?;
    let sd = label_point(lp.poly);
    let rows = seed_rows(w, sd, &key);
    w.upsert_pset(id, CEILING_PSET, &rows)?;
    Ok(true)
}

/// The room (or building outline) at a seed, or the refusal's key.
fn pick_loop(w: &mut W<'_>, storey: Id, seed: V2, mode: &str) -> R<(Vec<V2>, Vec<Vec<V2>>)> {
    let l = storey_loops(w, storey)?;
    if mode == "building" {
        loop_at(&l.loops.outlines, seed).map(|o| (o.poly.clone(), Vec::new())).ok_or_else(|| key_err("model.err.nobuilding2"))
    } else {
        loop_at(&l.loops.rooms, seed).map(|r| (r.poly.clone(), r.holes.clone())).ok_or_else(|| key_err("model.err.noroom"))
    }
}

/// `addFloor(w, { storey, seed, mode, kind, thickness, offset, guid })` → (id, guid)
#[allow(clippy::too_many_arguments)]
pub fn add_floor(
    w: &mut W<'_>,
    storey: Id,
    seed: V2,
    mode: &str,
    kind: &str,
    thickness: f64,
    offset: f64,
    guid: Option<&str>,
) -> R<(Id, String)> {
    let k = FLOOR_KINDS.iter().find(|x| x.0 == kind).copied().unwrap_or(FLOOR_KINDS[0]);
    let (poly, holes) = pick_loop(w, storey, seed, mode)?;
    let st = w.must(storey)?;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let lp = w.place_at(Some(stp), 0.0, 0.0, 0.0, 0.0)?;
    let g = w.gid(guid);
    let name = w.l(&t0(&format!("model.floor.{}", k.0)));
    let oh = w.owner_history();
    let id = w.make("IFCSLAB", vec![w.gid_jv(&g), oh, name.clone(), JV::Null, name, JV::H(lp), JV::Null, JV::Null, JV::s("FLOOR")])?;
    w.note_guid(&g, id);
    w.contain(id, storey)?;
    let mat = w.material(&t0(&format!("model.mat.{}", k.2)))?;
    w.associate_material(id, mat)?;
    let tt = if thickness > 0.0 { thickness } else { k.1 };
    let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
    let rows = vec![
        row("Mode", "IFCLABEL", P::s(mode)),
        row("Kind", "IFCLABEL", P::s(k.0)),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", u(w, tt)),
        row("Offset", "IFCLENGTHMEASURE", u(w, offset)),
        row("SeedX", "IFCLENGTHMEASURE", u(w, seed[0])),
        row("SeedY", "IFCLENGTHMEASURE", u(w, seed[1])),
        row("ShapeKey", "IFCIDENTIFIER", P::s("")),
    ];
    w.upsert_pset(id, FLOOR_PSET, &rows)?;
    w.upsert_pset(
        id,
        "Pset_SlabCommon",
        &[row("IsExternal", "IFCBOOLEAN", P::Bool(false)), row("LoadBearing", "IFCBOOLEAN", P::Bool(true))],
    )?;
    write_floor(w, id, tt, offset, "", &Lp { poly: &poly, holes: &holes })?;
    Ok((id, g))
}

/// `addCeiling(w, { storey, seed, mode, height, thickness, guid })` → (id, guid)
pub fn add_ceiling(w: &mut W<'_>, storey: Id, seed: V2, mode: &str, height: f64, thickness: f64, guid: Option<&str>) -> R<(Id, String)> {
    let (poly, holes) = pick_loop(w, storey, seed, mode)?;
    let st = w.must(storey)?;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let lp = w.place_at(Some(stp), 0.0, 0.0, 0.0, 0.0)?;
    let g = w.gid(guid);
    let name = w.l(&t0("model.ceiling"));
    let oh = w.owner_history();
    let id =
        w.make("IFCCOVERING", vec![w.gid_jv(&g), oh, name.clone(), JV::Null, name, JV::H(lp), JV::Null, JV::Null, JV::s("CEILING")])?;
    w.note_guid(&g, id);
    w.contain(id, storey)?;
    let mat = w.material(&t0("model.mat.gypsum"))?;
    w.associate_material(id, mat)?;
    let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
    let rows = vec![
        row("Mode", "IFCLABEL", P::s(mode)),
        row("Height", "IFCLENGTHMEASURE", u(w, height)),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", u(w, thickness)),
        row("SeedX", "IFCLENGTHMEASURE", u(w, seed[0])),
        row("SeedY", "IFCLENGTHMEASURE", u(w, seed[1])),
        row("ShapeKey", "IFCIDENTIFIER", P::s("")),
    ];
    w.upsert_pset(id, CEILING_PSET, &rows)?;
    write_ceiling(w, id, height, thickness, "", &Lp { poly: &poly, holes: &holes })?;
    Ok((id, g))
}

/// What a new roof is (`placeRoof` spec).
#[derive(Clone, Debug, PartialEq)]
pub struct NewRoof {
    pub storey: Id,
    pub seed: V2,
    pub pitch: f64,
    pub overhang: f64,
    pub gables: Vec<f64>,
    pub thickness: f64,
    pub guid: Option<String>,
    pub edges: String,
    pub rake: Option<f64>,
    pub plate: f64,
    pub fascia: f64,
    pub soffit: bool,
}

impl Default for NewRoof {
    fn default() -> Self {
        NewRoof {
            storey: 0.0,
            seed: [0.0, 0.0],
            pitch: 26.565,
            overhang: 12.0 * IN,
            gables: Vec::new(),
            thickness: 8.0 * IN,
            guid: None,
            edges: String::new(),
            rake: None,
            plate: 0.0,
            fascia: roof_defaults().fascia,
            soffit: true,
        }
    }
}

/// `addRoof(w, spec)` → (id, guid)
pub fn add_roof(w: &mut W<'_>, s: &NewRoof) -> R<(Id, String)> {
    let l = storey_loops(w, s.storey)?;
    let outline = loop_at(&l.loops.outlines, s.seed).cloned().ok_or_else(|| key_err("model.err.nobuilding2"))?;
    let set = RoofSet { pitch: s.pitch, overhang: s.overhang, rake: s.rake, gables: s.gables.clone(), edges: s.edges.clone() };
    let kinds: Vec<EdgeKind> = edge_kinds(&outline.poly, &l.walls, &set);
    if let Err(e) = roof_geometry(&outline.poly, &kinds, s.thickness, 0.0, s.fascia, roof_defaults().fascia_t, s.soffit) {
        return Err(key_err(e));
    }
    let levels = read_levels(w)?;
    let lv = level_of(&levels, s.storey).ok_or_else(|| crate::api::JsErr("TypeError: level".into()))?;
    let host = lv.above.clone().unwrap_or_else(|| lv.level.clone());
    let st = w.must(host.id)?;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let lp = w.place_at(Some(stp), 0.0, 0.0, 0.0, 0.0)?;
    let g = w.gid(s.guid.as_deref());
    let name = w.l(&t0("model.roof"));
    let oh = w.owner_history();
    let pt = if w.v4 { if !s.gables.is_empty() || !s.edges.is_empty() { "GABLE_ROOF" } else { "HIP_ROOF" } } else { "NOTDEFINED" };
    let id = w.make("IFCROOF", vec![w.gid_jv(&g), oh, name.clone(), JV::Null, name, JV::H(lp), JV::Null, JV::Null, JV::s(pt)])?;
    w.note_guid(&g, id);
    w.contain(id, host.id)?;
    let walls_guid = lv.level.guid.clone().unwrap_or_default();
    let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
    let rows = vec![
        row("WallsLevel", "IFCIDENTIFIER", P::s(&walls_guid)),
        row("PitchDegrees", "IFCREAL", P::Num(s.pitch)),
        row("Overhang", "IFCLENGTHMEASURE", u(w, s.overhang)),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", u(w, s.thickness)),
        row("Gables", "IFCLABEL", P::s(&s.gables.iter().map(|g| crate::js::num_str(*g)).collect::<Vec<_>>().join(","))),
        row("SeedX", "IFCLENGTHMEASURE", u(w, s.seed[0])),
        row("SeedY", "IFCLENGTHMEASURE", u(w, s.seed[1])),
        row("ShapeKey", "IFCIDENTIFIER", P::s("")),
        row("Edges", "IFCTEXT", if s.edges.is_empty() { P::Null } else { P::s(&s.edges) }),
        row("Rake", "IFCLENGTHMEASURE", s.rake.map_or(P::Null, |r| u(w, r))),
        row("PlateHeight", "IFCLENGTHMEASURE", u(w, s.plate)),
        row("FasciaHeight", "IFCLENGTHMEASURE", u(w, s.fascia)),
        row("Soffit", "IFCBOOLEAN", P::Bool(s.soffit)),
    ];
    w.upsert_pset(id, ROOF_PSET, &rows)?;
    let r = Roof {
        id,
        guid: g.clone(),
        storey: Some(host.id),
        walls: walls_guid,
        seed: s.seed,
        pitch: s.pitch,
        overhang: s.overhang,
        thickness: s.thickness,
        gables: s.gables.clone(),
        key: String::new(),
        parts: Vec::new(),
        edges: s.edges.clone(),
        rake: s.rake,
        plate: s.plate,
        fascia: s.fascia,
        soffit: s.soffit,
    };
    write_roof(w, &r, Some(&levels), Some(&l), Some(&outline))?;
    Ok((id, g))
}

/// Make a roof's parts again from the walls' outline under it.
fn write_roof(w: &mut W<'_>, r: &Roof, levels: Option<&[Level]>, l: Option<&StoreyLoops>, outline: Option<&Outline>) -> R<bool> {
    let levels = match levels {
        Some(x) => x.to_vec(),
        None => read_levels(w)?,
    };
    let Some(walls_lv) = levels.iter().find(|x| x.guid.as_deref() == Some(r.walls.as_str())).cloned() else { return Ok(false) };
    let l = match l {
        Some(x) => x.clone(),
        None => storey_loops(w, walls_lv.id)?,
    };
    let outline = match outline {
        Some(o) => o.clone(),
        None => match loop_at(&l.loops.outlines, r.seed) {
            Some(o) => o.clone(),
            None => return Ok(false),
        },
    };
    let top0 = l.walls.iter().fold(0.0, |mx, x| m::max(mx, x.base_offset + x.height));
    let top = if top0 != 0.0 && !top0.is_nan() { top0 } else { walls_lv.height };
    let key = shape_key(vec![
        kpts(&outline.poly),
        K::N(r.pitch),
        K::N(r.overhang),
        K::A(r.gables.iter().map(|g| K::N(*g)).collect()),
        K::N(r.thickness),
        K::N(top),
        K::N(walls_lv.elevation),
        K::S(r.edges.clone()),
        r.rake.map_or(K::Null, K::N),
        K::N(if r.plate != 0.0 && !r.plate.is_nan() { r.plate } else { 0.0 }),
        K::N(if r.fascia != 0.0 && !r.fascia.is_nan() { r.fascia } else { 0.0 }),
        K::B(r.soffit),
        K::A(
            l.walls
                .iter()
                .map(|x| {
                    K::A(vec![
                        K::S(x.guid.clone()),
                        K::A(vec![K::N(x.a[0]), K::N(x.a[1])]),
                        K::A(vec![K::N(x.b[0]), K::N(x.b[1])]),
                        K::N(x.t),
                        K::N(x.base_offset + x.height),
                    ])
                })
                .collect(),
        ),
    ]);
    if r.key == key && !r.parts.is_empty() {
        return Ok(false);
    }
    if write_roof_parts(w, r, walls_lv.id, &l.walls, &outline, top)?.is_none() {
        return Ok(false);
    }
    let sd = label_point(&outline.poly);
    let rows = seed_rows(w, sd, &key);
    w.upsert_pset(r.id, ROOF_PSET, &rows)?;
    Ok(true)
}

/// What syncDerived did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sync {
    pub made: usize,
    pub changed: usize,
    pub dropped: usize,
}

fn room_number(level_index: usize, n: usize) -> String {
    crate::js::num_str(((level_index as f64) + 1.0) * 100.0 + n as f64)
}

/// `syncDerived(w, storey)`: a level's rooms, floors, ceilings (and the roofs over its walls) brought up to date.
pub fn sync_derived(w: &mut W<'_>, storey: Id) -> R<Sync> {
    let levels = read_levels(w)?;
    let Some(lv) = level_of(&levels, storey) else { return Ok(Sync::default()) };
    let l = storey_loops(w, storey)?;
    let d = read_derived(w)?;
    let mut out = Sync::default();
    let mut taken: Vec<usize> = Vec::new();
    let mut gone: Vec<Id> = Vec::new();
    for sp in d.spaces.iter().filter(|x| x.storey == Some(storey)) {
        let li = loop_index(&l.loops.rooms, sp.seed);
        match li {
            Some(i) if !taken.contains(&i) => {
                taken.push(i);
                let Some(room) = l.loops.rooms.get(i) else { continue };
                let h = space_h(&lv);
                if sp.key != shape_key(vec![kpts(&room.poly), kloops(&room.holes), K::N(h)]) {
                    write_space(w, sp.id, room, &lv)?;
                    out.changed = out.changed.saturating_add(1);
                }
            }
            _ => gone.push(sp.id),
        }
    }
    if !gone.is_empty() {
        w.drop_product(&gone)?;
        out.dropped = out.dropped.saturating_add(gone.len());
    }
    let mut used: Vec<String> =
        d.spaces.iter().filter(|x| x.storey == Some(storey) && !gone.contains(&x.id)).map(|x| x.number.clone()).collect();
    let mut n = 1usize;
    for (i, room) in l.loops.rooms.iter().enumerate() {
        if taken.contains(&i) {
            continue;
        }
        while used.contains(&room_number(lv.index, n)) {
            n = n.saturating_add(1);
        }
        let num = room_number(lv.index, n);
        used.push(num.clone());
        let name = t("model.room.name", &[("n", &n.to_string())]);
        add_space(w, storey, room, &lv, &num, &name)?;
        out.made = out.made.saturating_add(1);
    }
    for f in d.floors.iter().filter(|x| x.storey == Some(storey)) {
        let hit: Option<Lp<'_>> = if f.mode == "building" {
            loop_at(&l.loops.outlines, f.seed).map(Lp::from)
        } else {
            loop_at(&l.loops.rooms, f.seed).map(Lp::from)
        };
        if let Some(lp) = hit
            && write_floor(w, f.id, f.thickness, f.offset, &f.key, &lp)?
        {
            out.changed = out.changed.saturating_add(1);
        }
    }
    for c in d.ceilings.iter().filter(|x| x.storey == Some(storey)) {
        let hit: Option<Lp<'_>> = if c.mode == "building" {
            loop_at(&l.loops.outlines, c.seed).map(Lp::from)
        } else {
            loop_at(&l.loops.rooms, c.seed).map(Lp::from)
        };
        if let Some(lp) = hit
            && write_ceiling(w, c.id, c.height, c.thickness, &c.key, &lp)?
        {
            out.changed = out.changed.saturating_add(1);
        }
    }
    let lg = lv.level.guid.clone().unwrap_or_default();
    for r in d.roofs.iter().filter(|x| x.walls == lg) {
        if write_roof(w, r, Some(&levels), Some(&l), None)? {
            out.changed = out.changed.saturating_add(1);
        }
    }
    Ok(out)
}

/// What a change of a room, floor, ceiling or roof asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DerivedChange {
    pub thickness: Option<f64>,
    pub offset: Option<f64>,
    pub kind: Option<String>,
    pub height: Option<f64>,
    pub pitch: Option<f64>,
    pub overhang: Option<f64>,
    pub gables: Option<Vec<f64>>,
    pub name: Option<String>,
    pub number: Option<String>,
    pub edges: Option<String>,
    pub rake: Option<f64>,
    pub plate: Option<f64>,
    pub fascia: Option<f64>,
    pub soffit: Option<bool>,
}

/// `updateDerived(w, guid, changes)` → (kind, the storey to sync)
pub fn update_derived(w: &mut W<'_>, guid: &str, ch: &DerivedChange) -> R<(&'static str, Option<Id>)> {
    let d = read_derived(w)?;
    let fin = |v: Option<f64>| v.filter(|x| x.is_finite());
    if let Some(x) = d.spaces.iter().find(|x| x.guid == guid) {
        let mut l = w.touch(x.id)?;
        if let Some(n) = ch.number.as_deref().map(crate::js::trim).filter(|s| !s.is_empty()) {
            l.set("Name", w.l(n));
        }
        if let Some(n) = ch.name.as_deref().map(crate::js::trim).filter(|s| !s.is_empty()) {
            l.set("LongName", w.l(n));
        }
        w.write(&mut l)?;
        if let Some(n) = &ch.number {
            w.upsert_pset(x.id, "Pset_SpaceCommon", &[row("Reference", "IFCIDENTIFIER", P::s(crate::js::trim(n)))])?;
        }
        return Ok(("space", x.storey));
    }
    let len = |w: &W<'_>, k: &str, v: f64| row(k, "IFCLENGTHMEASURE", P::Num(round6(w.u(v))));
    let plen = |w: &W<'_>, k: &str, v: f64| row(k, "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(v))));
    let mut rows: Vec<Row> = vec![row("ShapeKey", "IFCIDENTIFIER", P::s(""))];
    if let Some(x) = d.floors.iter().find(|x| x.guid == guid) {
        if let Some(t) = ch.thickness.filter(|t| *t > 0.0) {
            rows.push(plen(w, "Thickness", t));
        }
        if let Some(o) = fin(ch.offset) {
            rows.push(len(w, "Offset", o));
        }
        if let Some(k) = ch.kind.as_ref().filter(|k| FLOOR_KINDS.iter().any(|f| f.0 == k.as_str())) {
            rows.push(row("Kind", "IFCLABEL", P::s(k)));
            let nm = w.l(&t0(&format!("model.floor.{k}")));
            let mut l = w.touch(x.id)?;
            l.set("Name", nm.clone());
            l.set("ObjectType", nm);
            w.write(&mut l)?;
        }
        w.upsert_pset(x.id, FLOOR_PSET, &rows)?;
        return Ok(("floor", x.storey));
    }
    if let Some(x) = d.ceilings.iter().find(|x| x.guid == guid) {
        if let Some(t) = ch.thickness.filter(|t| *t > 0.0) {
            rows.push(plen(w, "Thickness", t));
        }
        if let Some(h) = fin(ch.height) {
            rows.push(len(w, "Height", h));
        }
        w.upsert_pset(x.id, CEILING_PSET, &rows)?;
        return Ok(("ceiling", x.storey));
    }
    let Some(x) = d.roofs.iter().find(|x| x.guid == guid).cloned() else { return Err(key_err("model.err.gone")) };
    if let Some(t) = ch.thickness.filter(|t| *t > 0.0) {
        rows.push(plen(w, "Thickness", t));
    }
    if let Some(o) = fin(ch.overhang).filter(|o| *o >= 0.0) {
        rows.push(len(w, "Overhang", o));
    }
    if let Some(p) = fin(ch.pitch) {
        if !(p > 1.0 && p < 80.0) {
            return Err(key_err("model.err.pitch"));
        }
        rows.push(row("PitchDegrees", "IFCREAL", P::Num(p)));
    }
    if let Some(g) = &ch.gables {
        rows.push(row("Gables", "IFCLABEL", P::s(&g.iter().map(|v| crate::js::num_str(*v)).collect::<Vec<_>>().join(","))));
        if w.v4 {
            let mut l = w.touch(x.id)?;
            l.set("PredefinedType", JV::s(if g.is_empty() { "HIP_ROOF" } else { "GABLE_ROOF" }));
            w.write(&mut l)?;
        }
    }
    if let Some(e) = &ch.edges {
        rows.push(row("Edges", "IFCTEXT", P::s(if e.is_empty() { " " } else { e })));
    }
    if let Some(v) = fin(ch.rake).filter(|v| *v >= 0.0) {
        rows.push(len(w, "Rake", v));
    }
    if let Some(v) = fin(ch.plate).filter(|v| *v >= 0.0) {
        rows.push(len(w, "PlateHeight", v));
    }
    if let Some(v) = fin(ch.fascia).filter(|v| *v >= 0.0) {
        rows.push(len(w, "FasciaHeight", v));
    }
    if let Some(b) = ch.soffit {
        rows.push(row("Soffit", "IFCBOOLEAN", P::Bool(b)));
    }
    let levels = read_levels(w)?;
    if let Some(wl) = levels.iter().find(|l| l.guid.as_deref() == Some(x.walls.as_str())).cloned() {
        let sl = storey_loops(w, wl.id)?;
        if let Some(o) = loop_at(&sl.loops.outlines, x.seed) {
            let set = RoofSet {
                pitch: ch.pitch.unwrap_or(x.pitch),
                overhang: ch.overhang.unwrap_or(x.overhang),
                gables: ch.gables.clone().unwrap_or_else(|| x.gables.clone()),
                edges: ch.edges.clone().unwrap_or_else(|| x.edges.clone()),
                rake: if ch.rake.is_some() { ch.rake } else { x.rake },
            };
            let kinds = edge_kinds(&o.poly, &sl.walls, &set);
            let th = match ch.thickness {
                Some(t) if t > 0.0 => t,
                _ => x.thickness,
            };
            if let Err(e) = roof_geometry(
                &o.poly,
                &kinds,
                th,
                0.0,
                ch.fascia.unwrap_or(x.fascia),
                roof_defaults().fascia_t,
                ch.soffit.unwrap_or(x.soffit),
            ) {
                return Err(key_err(e));
            }
        }
    }
    w.upsert_pset(x.id, ROOF_PSET, &rows)?;
    Ok(("roof", levels.iter().find(|l| l.guid.as_deref() == Some(x.walls.as_str())).map(|l| l.id)))
}

/// `deleteDerived(w, guids)`
pub fn delete_derived(w: &mut W<'_>, guids: &[String]) -> R<()> {
    let d = read_derived(w)?;
    let mut list = Vec::new();
    for x in &d.spaces {
        if guids.contains(&x.guid) {
            list.push(x.id);
        }
    }
    for x in &d.floors {
        if guids.contains(&x.guid) {
            list.push(x.id);
        }
    }
    for x in &d.ceilings {
        if guids.contains(&x.guid) {
            list.push(x.id);
        }
    }
    for x in &d.roofs {
        if guids.contains(&x.guid) {
            list.push(x.id);
            list.extend(x.parts.iter().copied());
        }
    }
    if !list.is_empty() {
        w.drop_product(&list)?;
    }
    Ok(())
}
