//! res-counter.js (0.5 part 4, RC.4), the half that follows the model: base cabinets from the library
//! (`CC-COUNTER-READY yes`, an L footprint from `CC-FOOTPRINT "L <leg>"`), the runs they make with the front overhang,
//! countertops read back and fitted again when their run changed (`syncCountertops`).

use std::collections::HashMap;

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::m::{self, V2, at, hypot};
use crate::draw::place::{PSET_EQUIPMENT, PSET_PLACEMENT};
use crate::draw::res::{num, s_or};
use crate::draw::rooms::plan_loops_snap;
use crate::draw::slabs::{K, kpts, shape_key_scaled};
use crate::draw::tr::t0;
use crate::draw::writer::{D2R, W, inv4j, round6, row};
use crate::util::apply4;
use crate::wapi::{JV, P};

pub const COUNTER_PSET: &str = "CadCraft_Countertop";

/// A library placement as IfcPlacer `placements()` lists it (what the countertops need of it).
#[derive(Clone, Debug)]
pub struct Placement {
    pub id: Id,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub item_number: String,
    pub equipment: HashMap<String, P>,
}

/// IfcPlacer `placements()`: every element with CadCraft_Placement, by item number (natural order).
pub fn placements(w: &mut W<'_>) -> R<Vec<Placement>> {
    // the pset index: product → [pset] in the order the relations list them
    let mut order: Vec<Id> = Vec::new();
    let mut index: HashMap<u64, Vec<Id>> = HashMap::new();
    for rid in w.ids("IFCRELDEFINESBYPROPERTIES") {
        let r = w.must(rid)?;
        let Some(ps) = r.a("RelatingPropertyDefinition").href() else { continue };
        if ps == 0.0 {
            continue;
        }
        for h in r.a("RelatedObjects").hrefs() {
            let e = index.entry(id_key(h)).or_insert_with(|| {
                order.push(h);
                Vec::new()
            });
            e.push(ps);
        }
    }
    let mut out = Vec::new();
    for id in order {
        let mut sets: HashMap<String, HashMap<String, P>> = HashMap::new();
        for &ps in index.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]) {
            let l = w.must(ps)?;
            let name = l.a("Name").text_or_empty();
            let mut props = HashMap::new();
            for h in l.a("HasProperties").hrefs() {
                if let Some(p) = w.line(h)?
                    && p.get("Name").is_some_and(JV::truthy)
                {
                    props.insert(p.a("Name").val().string(), p.a("NominalValue").val());
                }
            }
            sets.insert(name, props);
        }
        let Some(pl) = sets.get(PSET_PLACEMENT) else { continue };
        let item_number = match pl.get("ItemNumber") {
            Some(v) if !matches!(v, P::Undef | P::Null) => v.string(),
            _ => String::new(),
        };
        let equipment = sets.get(PSET_EQUIPMENT).cloned().unwrap_or_default();
        let line = w.must(id)?;
        let (mut x, mut y, mut rot) = (0.0, 0.0, 0.0);
        if let Some(lp) = line.a("ObjectPlacement").href() {
            let wm = w.api.world(lp)?;
            x = wm[12] * w.unit_mm;
            y = wm[13] * w.unit_mm;
            rot = m::round(m::atan2(wm[1], wm[0]) / D2R * 1000.0) / 1000.0;
        }
        out.push(Placement { id, x, y, rot, item_number, equipment });
    }
    out.sort_by(|a, b| crate::js::locale_cmp(&a.item_number, &b.item_number));
    Ok(out)
}

/// A base cabinet ready for a countertop (`counterCabinets`).
#[derive(Clone, Debug)]
pub struct Cabinet {
    pub id: Id,
    pub top: f64,
    pub local: Vec<V2>,
    pub fronts: Vec<(usize, usize)>,
    px: f64,
    py: f64,
    c: f64,
    s: f64,
    mi: [f64; 16],
}
impl Cabinet {
    fn to_plan(&self, x: f64, y: f64) -> V2 {
        let xx = self.px + x * self.c - y * self.s;
        let yy = self.py + x * self.s + y * self.c;
        let q = apply4(&self.mi, xx, yy, 0.0);
        [q[0], q[1]]
    }
}

/// `/^L\s+([\d.]+)/` → the leg's text
fn l_leg(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('L')?;
    let t = rest.trim_start_matches(|c: char| c.is_whitespace());
    if t.len() == rest.len() {
        return None;
    }
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    if end == 0 { None } else { t.get(..end) }
}

pub fn counter_cabinets(w: &mut W<'_>, storey: Id) -> R<Vec<Cabinet>> {
    let mut out = Vec::new();
    let st = w.must(storey)?;
    if !st.a("ObjectPlacement").truthy() {
        return Ok(out);
    }
    let mi = inv4j(&w.storey_world(storey)?)?;
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for p in placements(w)? {
        if so.get(&id_key(p.id)).copied() != Some(storey) {
            continue;
        }
        let src = w.read_pset(p.id, "CadCraft_SourceAttributes")?.unwrap_or_default();
        if s_or(&src, "CC-COUNTER-READY").to_lowercase() != "yes" {
            continue;
        }
        let eq = &p.equipment;
        let (wid, dep, top) = (num(eq, "Width") * um, num(eq, "Depth") * um, num(eq, "Height") * um);
        if !(wid > 0.0 && dep > 0.0 && top > 0.0) {
            continue;
        }
        let r = p.rot * std::f64::consts::PI / 180.0;
        let (c, s) = (m::cos(r), m::sin(r));
        let fp = s_or(&src, "CC-FOOTPRINT");
        let (local, fronts) = match l_leg(&fp) {
            Some(leg) => {
                let l = crate::js::str_num(leg);
                let h = wid / 2.0;
                (vec![[-h, -dep], [-h + l, -dep], [-h + l, -l], [h, -l], [h, 0.0], [-h, 0.0]], vec![(1, 2), (2, 3)])
            }
            None => (vec![[-wid / 2.0, -dep], [wid / 2.0, -dep], [wid / 2.0, 0.0], [-wid / 2.0, 0.0]], vec![(0, 1)]),
        };
        out.push(Cabinet { id: p.id, top, local, fronts, px: p.x, py: p.y, c, s, mi });
    }
    Ok(out)
}

/// A run of cabinets under one countertop.
#[derive(Clone, Debug)]
pub struct Run {
    pub poly: Vec<V2>,
    pub area: f64,
    pub perimeter: f64,
    pub cabinets: Vec<Id>,
    pub top: f64,
}

fn centroid_of(p: &[V2]) -> V2 {
    let n = p.len() as f64;
    [p.iter().fold(0.0, |s, q| s + q[0]) / n, p.iter().fold(0.0, |s, q| s + q[1]) / n]
}
/// `pointIn(pt, poly)`: even-odd, the boundary counting as inside.
fn point_in(pt: V2, poly: &[V2]) -> bool {
    if m::in_poly(pt[0], pt[1], poly) {
        return true;
    }
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (at(poly, j), at(poly, i));
        let d = hypot(b[0] - a[0], b[1] - a[1]);
        let d = if d != 0.0 && !d.is_nan() { d } else { 1.0 };
        if ((b[0] - a[0]) * (pt[1] - a[1]) - (b[1] - a[1]) * (pt[0] - a[0])).abs() / d < 1.0
            && m::min(a[0], b[0]) - 1.0 <= pt[0]
            && pt[0] <= m::max(a[0], b[0]) + 1.0
            && m::min(a[1], b[1]) - 1.0 <= pt[1]
            && pt[1] <= m::max(a[1], b[1]) + 1.0
        {
            return true;
        }
        j = i;
    }
    false
}

/// `counterRuns(cabinets, { overhang })`
pub fn counter_runs(cabinets: &[Cabinet], overhang: f64) -> Vec<Run> {
    let polys: Vec<Vec<V2>> = cabinets
        .iter()
        .map(|cb| {
            let mut q = cb.local.clone();
            for &(i, j) in &cb.fronts {
                let (a, b) = (at(&cb.local, i), at(&cb.local, j));
                let l = hypot(b[0] - a[0], b[1] - a[1]);
                let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
                let n = [(b[1] - a[1]) / l, -(b[0] - a[0]) / l];
                for k in [i, j] {
                    if let Some(p) = q.get_mut(k) {
                        p[0] += n[0] * overhang;
                        p[1] += n[1] * overhang;
                    }
                }
            }
            q.iter().map(|p| cb.to_plan(p[0], p[1])).collect()
        })
        .collect();
    if polys.is_empty() {
        return Vec::new();
    }
    let l = plan_loops_snap(&polys, 0.5);
    l.outlines
        .iter()
        .map(|o| {
            let inside: Vec<&Cabinet> = cabinets
                .iter()
                .zip(polys.iter())
                .filter(|(_, pp)| pp.iter().any(|p| point_in(*p, &o.poly)) || point_in(centroid_of(pp), &o.poly))
                .map(|(cb, _)| cb)
                .collect();
            Run {
                poly: o.poly.clone(),
                area: o.area.abs(),
                perimeter: o.perimeter,
                cabinets: inside.iter().map(|cb| cb.id).collect(),
                top: m::max_of(inside.iter().map(|cb| cb.top).chain(std::iter::once(0.0))),
            }
        })
        .collect()
}

/// A countertop read back (`readCountertops`).
#[derive(Clone, Debug)]
pub struct Countertop {
    pub id: Id,
    pub storey: Option<Id>,
    pub seed: V2,
    pub overhang: f64,
    pub thickness: f64,
    pub key: String,
}

pub fn read_countertops(w: &mut W<'_>) -> R<Vec<Countertop>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let um = w.unit_mm;
    for cls in ["IFCFURNITURE", "IFCFURNISHINGELEMENT"] {
        for id in w.ids(cls) {
            let Some(ps) = w.read_pset(id, COUNTER_PSET)? else { continue };
            out.push(Countertop {
                id,
                storey: so.get(&id_key(id)).copied(),
                seed: [num(&ps, "SeedX") * um, num(&ps, "SeedY") * um],
                overhang: num(&ps, "Overhang") * um,
                thickness: num(&ps, "Thickness") * um,
                key: s_or(&ps, "ShapeKey"),
            });
        }
    }
    Ok(out)
}

fn run_key(run: &Run, thickness: f64) -> String {
    shape_key_scaled(vec![kpts(&run.poly), K::N(run.top), K::N(thickness)], 10.0)
}

/// `writeCountertop(w, c, run)`
fn write_countertop(w: &mut W<'_>, c: &Countertop, run: &Run) -> R<()> {
    let key = run_key(run, c.thickness);
    let pr = w.profile(&run.poly, None)?;
    let solid = w.extrude(pr, run.top, c.thickness)?;
    let st = w.surface_style(&t0("res.style.counter"), [0.86, 0.85, 0.8], 0.0)?;
    w.style_item(solid, st)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[solid])?;
    w.set_shape(c.id, &[rep])?;
    // the front edge (the overhang side) for the takeoff
    let mut front = 0.0;
    if let Some(storey) = c.storey {
        for cb in counter_cabinets(w, storey)?.iter().filter(|cb| run.cabinets.contains(&cb.id)) {
            for &(i, j) in &cb.fronts {
                let (a, b) = (at(&cb.local, i), at(&cb.local, j));
                front += hypot(b[0] - a[0], b[1] - a[1]);
            }
        }
    }
    w.upsert_pset(
        c.id,
        "CadCraft_CountertopQuantities",
        &[
            row("Area", "IFCAREAMEASURE", P::Num(round6(run.area / 1e6))),
            row("Perimeter", "IFCLENGTHMEASURE", P::Num(round6(w.u(run.perimeter)))),
            row("FrontEdge", "IFCLENGTHMEASURE", P::Num(round6(w.u(front)))),
        ],
    )?;
    w.upsert_pset(
        c.id,
        COUNTER_PSET,
        &[
            row("SeedX", "IFCLENGTHMEASURE", P::Num(round6(w.u(c.seed[0])))),
            row("SeedY", "IFCLENGTHMEASURE", P::Num(round6(w.u(c.seed[1])))),
            row("ShapeKey", "IFCIDENTIFIER", P::s(&key)),
            row("Cabinets", "IFCINTEGER", P::Num(run.cabinets.len() as f64)),
        ],
    )?;
    Ok(())
}

/// `syncCountertops(w, storey)`: every countertop of a level whose cabinets changed fitted again (a run that is gone
/// leaves its countertop as it was).
pub fn sync_countertops(w: &mut W<'_>, storey: Id) -> R<usize> {
    let all: Vec<Countertop> = read_countertops(w)?.into_iter().filter(|c| c.storey == Some(storey)).collect();
    if all.is_empty() {
        return Ok(0);
    }
    let cabinets = counter_cabinets(w, storey)?;
    let mut n = 0usize;
    for c in &all {
        let runs = counter_runs(&cabinets, c.overhang);
        let Some(run) = runs.iter().find(|r| point_in(c.seed, &r.poly)) else { continue };
        if run_key(run, c.thickness) != c.key {
            write_countertop(w, c, run)?;
            n = n.saturating_add(1);
        }
    }
    Ok(n)
}
