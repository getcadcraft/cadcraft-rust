//! clipboard.js — what Copy, Paste and Duplicate carry in BimCraft (owner's brief, 2026-10-11):
//!   - a placed library item (an element with CadCraft_Placement) is copied as a *placement clip*: which item it is
//!     (uuid@version or sha:<sha256>), its place, storey, item number, quantity, notes, the box the ghost shows and —
//!     when this window still has it — the library item itself. Paste places the same item again (`placeLike` in the
//!     model it came from; `placeItem` with the kept item in any other model).
//!   - any other element, or several, is copied as an *elements clip*: which elements, where (an anchor and the box
//!     round them, for the ghost) and whether it was a Cut. The geometry is not in the clip: the window keeps the
//!     editor's snapshot in memory, so these paste only into the model they came from.
//!
//! The fork keeps the clip in the window's memory only (one window: 0.5's localStorage copy for other tabs has no
//! counterpart), so nothing is read back from storage.

use cc_io_ifc::model::Id;
use serde_json::{Map, Value as J, json};

use crate::draw::m;
use crate::draw::place::{PlacementRow, StoreyInfo};
use crate::draw::tr::t0;
use crate::js::{jn, slice16};
use crate::wapi::P;

pub const CLIP_FORMAT: &str = "bimcraft-clipboard";
pub const CLIP_VERSION: u32 = 1;
/// paste in place beside the original: this much clear space between them
pub const GAP_MM: f64 = 100.0;
const MAX_NAME: usize = 255;
const MAX_FILE: usize = 255;
const MAX_NUMBER: usize = 40;
const MAX_NOTES: usize = 200;
const MAX_MODEL: usize = 64;
const MAX_CLS: usize = 64;
const MAX_GUID: usize = 64;
const MAX_LIBRARY: usize = 40;
const MAX_IDS: usize = 50_000;

/// An item-local box (mm; back-centre-bottom origin, front −Y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub x0: f64,
    pub y0: f64,
    pub z0: f64,
    pub x1: f64,
    pub y1: f64,
    pub z1: f64,
}

impl Box3 {
    pub fn json(&self) -> J {
        json!({ "x0": jn(self.x0), "y0": jn(self.y0), "z0": jn(self.z0), "x1": jn(self.x1), "y1": jn(self.y1), "z1": jn(self.z1) })
    }
}

/// Where a placement clip's item came from (`meta`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipMeta {
    pub library: String,
    pub sha256: String,
    pub source_file: String,
}

/// A clip (clipboard.js: kinds "placement" and "elements").
#[derive(Clone, Debug)]
pub struct Clip {
    pub kind: &'static str,
    pub copied_at: String,
    pub model: String,
    pub file: String,
    pub id: Id,
    pub name: String,
    // placement
    pub key: String,
    pub item_number: String,
    pub quantity: f64,
    pub notes: String,
    pub item: Option<std::sync::Arc<J>>,
    pub meta: Option<ClipMeta>,
    // elements
    pub ids: Vec<Id>,
    pub count: usize,
    pub cls: String,
    pub guid: String,
    pub cut: bool,
    // where
    pub storey: Option<Id>,
    pub storey_name: String,
    pub storey_elevation: Option<f64>,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub bx: Option<Box3>,
}

fn cut(s: &str, n: usize) -> String {
    slice16(s, n)
}

/// `Number.isInteger(v) && v > 0 ? v : null`
fn pos_int(v: Option<Id>) -> Option<Id> {
    v.filter(|x| x.is_finite() && x.fract() == 0.0 && *x > 0.0)
}

/// `libKey(p)`: "uuid@version" or "sha:<sha256>".
pub fn lib_key(p: &PlacementRow) -> String {
    if p.uuid.is_empty() {
        format!("sha:{}", p.sha256)
    } else {
        let v = match &p.version {
            P::Null => "null".to_string(),
            P::Undef => "undefined".to_string(),
            other => other.string(),
        };
        format!("{}@{v}", p.uuid)
    }
}

/// What a placement clip is made with besides the row (`ctx`).
#[derive(Clone, Debug, Default)]
pub struct PlacementCtx {
    pub model: String,
    pub file: String,
    pub storey_name: String,
    pub storey_elevation: Option<f64>,
    pub item: Option<std::sync::Arc<J>>,
    pub meta: Option<ClipMeta>,
    pub bx: Option<Box3>,
    pub now: String,
}

/// `placementClip(p, ctx)`
pub fn placement_clip(p: &PlacementRow, ctx: &PlacementCtx) -> Clip {
    let name = match p.equip("Name") {
        Some(v) if v.truthy() => v.string(),
        _ if !p.name.is_empty() => p.name.clone(),
        _ => t0("clip.name.libitem"),
    };
    let q = p.quantity.number();
    let q = m::round(if q != 0.0 && !q.is_nan() { q } else { 1.0 });
    Clip {
        kind: "placement",
        copied_at: ctx.now.clone(),
        model: cut(&ctx.model, MAX_MODEL),
        file: cut(&ctx.file, MAX_FILE),
        id: p.id,
        name: cut(&name, MAX_NAME),
        key: lib_key(p),
        item_number: cut(&p.item_number, MAX_NUMBER),
        quantity: m::max(1.0, q),
        notes: cut(&p.notes, MAX_NOTES),
        item: ctx.item.clone(),
        meta: ctx.meta.as_ref().map(|mm| ClipMeta {
            library: cut(&mm.library, MAX_LIBRARY),
            sha256: cut(&mm.sha256, 64),
            source_file: cut(&mm.source_file, MAX_FILE),
        }),
        ids: Vec::new(),
        count: 1,
        cls: String::new(),
        guid: String::new(),
        cut: false,
        storey: pos_int(p.storey),
        storey_name: cut(&ctx.storey_name, MAX_NAME),
        storey_elevation: ctx.storey_elevation.filter(|e| e.is_finite()),
        x: p.x,
        y: p.y,
        rot: p.rot,
        bx: ctx.bx,
    }
}

/// One element of an elements clip: id, name, class, GlobalId.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElRow {
    pub id: Id,
    pub name: String,
    pub cls: String,
    pub guid: String,
}

/// What an elements clip is made with besides the list (`ctx`).
#[derive(Clone, Debug, Default)]
pub struct ElementsCtx {
    pub model: String,
    pub file: String,
    pub now: String,
    pub storey: Option<Id>,
    pub storey_name: String,
    pub storey_elevation: Option<f64>,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub bx: Option<Box3>,
    pub cut: bool,
}

/// `elementsClip(list, ctx)`
pub fn elements_clip(list: &[ElRow], ctx: &ElementsCtx) -> Clip {
    let first = list.first().cloned().unwrap_or_default();
    let rot = if ctx.rot.is_finite() { ((ctx.rot % 360.0) + 360.0) % 360.0 } else { 0.0 };
    Clip {
        kind: "elements",
        copied_at: ctx.now.clone(),
        model: cut(&ctx.model, MAX_MODEL),
        file: cut(&ctx.file, MAX_FILE),
        id: first.id,
        name: cut(&if first.name.is_empty() { t0("clip.unnamed") } else { first.name.clone() }, MAX_NAME),
        key: String::new(),
        item_number: String::new(),
        quantity: 1.0,
        notes: String::new(),
        item: None,
        meta: None,
        ids: list.iter().map(|e| e.id).take(MAX_IDS).collect(),
        count: list.len(),
        cls: cut(&first.cls, MAX_CLS),
        guid: cut(&first.guid, MAX_GUID),
        cut: ctx.cut,
        storey: pos_int(ctx.storey),
        storey_name: cut(&ctx.storey_name, MAX_NAME),
        storey_elevation: ctx.storey_elevation.filter(|e| e.is_finite()),
        x: ctx.x,
        y: ctx.y,
        rot,
        bx: ctx.bx,
    }
}

impl Clip {
    /// The clip as clipboard.js writes it (key order as the JS object).
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("format".into(), J::from(CLIP_FORMAT));
        o.insert("version".into(), J::from(CLIP_VERSION));
        o.insert("kind".into(), J::from(self.kind));
        o.insert("copiedAt".into(), J::from(self.copied_at.clone()));
        o.insert("model".into(), J::from(self.model.clone()));
        o.insert("file".into(), J::from(self.file.clone()));
        o.insert("id".into(), jn(self.id));
        let storey = self.storey.map_or(J::Null, jn);
        let elev = self.storey_elevation.map_or(J::Null, jn);
        let bx = self.bx.map_or(J::Null, |b| b.json());
        if self.kind == "placement" {
            o.insert("key".into(), J::from(self.key.clone()));
            o.insert("name".into(), J::from(self.name.clone()));
            o.insert("itemNumber".into(), J::from(self.item_number.clone()));
            o.insert("quantity".into(), jn(self.quantity));
            o.insert("notes".into(), J::from(self.notes.clone()));
            o.insert("storey".into(), storey);
            o.insert("storeyName".into(), J::from(self.storey_name.clone()));
            o.insert("storeyElevation".into(), elev);
            o.insert("x".into(), jn(self.x));
            o.insert("y".into(), jn(self.y));
            o.insert("rot".into(), jn(self.rot));
            o.insert("box".into(), bx);
            o.insert("item".into(), self.item.as_deref().cloned().unwrap_or(J::Null));
            o.insert(
                "meta".into(),
                self.meta
                    .as_ref()
                    .map_or(J::Null, |mm| json!({ "library": mm.library, "sha256": mm.sha256, "sourceFile": mm.source_file })),
            );
        } else {
            o.insert("ids".into(), J::Array(self.ids.iter().map(|x| jn(*x)).collect()));
            o.insert("count".into(), J::from(self.count));
            o.insert("name".into(), J::from(self.name.clone()));
            o.insert("cls".into(), J::from(self.cls.clone()));
            o.insert("guid".into(), J::from(self.guid.clone()));
            o.insert("storey".into(), storey);
            o.insert("storeyName".into(), J::from(self.storey_name.clone()));
            o.insert("storeyElevation".into(), elev);
            o.insert("x".into(), jn(self.x));
            o.insert("y".into(), jn(self.y));
            o.insert("rot".into(), jn(self.rot));
            o.insert("box".into(), bx);
            o.insert("cut".into(), J::Bool(self.cut));
        }
        J::Object(o)
    }
}

/// `pasteMove(clip, at)` → (dx, dy, rot, about): the snapshot turned BY the difference, the anchor onto at.x, at.y.
pub fn paste_move(clip: &Clip, x: f64, y: f64, at_rot: f64) -> (f64, f64, f64, [f64; 2]) {
    let n = |v: f64| if v != 0.0 && !v.is_nan() { v } else { 0.0 };
    let mut rot = n(at_rot) - n(clip.rot);
    rot = ((rot % 360.0) + 360.0) % 360.0;
    if (rot - 360.0).abs() < 1e-9 {
        rot = 0.0;
    }
    (x - clip.x, y - clip.y, rot, [clip.x, clip.y])
}

fn num(v: Option<&J>) -> f64 {
    match v {
        Some(J::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(J::String(s)) => crate::js::str_num(s),
        Some(J::Null) => 0.0,
        Some(J::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

/// `boxFromItem(item)`: its body's box, else its plan 900 mm tall.
pub fn box_from_item(item: &J) -> Box3 {
    let b = item.get("body").filter(|b| b.is_object());
    let ty = b.and_then(|b| b.get("type")).and_then(J::as_str);
    if let Some(b) = b
        && ty == Some("box")
    {
        let (x, y, w, d, h) = (num(b.get("x")), num(b.get("y")), num(b.get("w")), num(b.get("d")), num(b.get("h")));
        let z = {
            let z = num(b.get("z"));
            if b.get("z").is_some() && z != 0.0 && !z.is_nan() { z } else { 0.0 }
        };
        return Box3 { x0: x, y0: y, z0: z, x1: x + w, y1: y + d, z1: z + h };
    }
    if let Some(b) = b
        && ty == Some("mesh")
        && let Some(pp) = b.get("positions").and_then(J::as_array).filter(|a| a.len() >= 3)
    {
        let mut o = Box3 {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            z0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y1: f64::NEG_INFINITY,
            z1: f64::NEG_INFINITY,
        };
        for q in pp.as_chunks::<3>().0 {
            let [x, y, z] = [num(Some(&q[0])), num(Some(&q[1])), num(Some(&q[2]))];
            o.x0 = m::min(o.x0, x);
            o.x1 = m::max(o.x1, x);
            o.y0 = m::min(o.y0, y);
            o.y1 = m::max(o.y1, y);
            o.z0 = m::min(o.z0, z);
            o.z1 = m::max(o.z1, z);
        }
        if o.x1 > o.x0 && o.y1 > o.y0 {
            return o;
        }
    }
    let pb = crate::libitem::plan_bounds(item.get("plan")).unwrap_or([-300.0, -300.0, 300.0, 0.0]);
    Box3 { x0: pb[0], y0: pb[1], z0: 0.0, x1: m::max(pb[2], pb[0] + 1.0), y1: m::max(pb[3], pb[1] + 1.0), z1: 900.0 }
}

/// `boxFromEquipment(eq, unitMm)`: from CadCraft_Equipment Width / Depth / Height (file units); None without both.
pub fn box_from_equipment(p: &PlacementRow, unit_mm: f64) -> Option<Box3> {
    let g = |k: &str| p.equip(k).map_or(f64::NAN, P::number) * unit_mm;
    let (w, d, h) = (g("Width"), g("Depth"), g("Height"));
    if !(w > 0.0 && d > 0.0) {
        return None;
    }
    Some(Box3 { x0: -w / 2.0, y0: -d, z0: 0.0, x1: w / 2.0, y1: 0.0, z1: if h > 0.0 { h } else { 900.0 } })
}

/// `ghostItem(clip)`: the ghost and wall snap's stand-in while pasting (a box body).
pub fn ghost_item(clip: &Clip) -> J {
    if let Some(it) = &clip.item {
        return (**it).clone();
    }
    let b = clip.bx.unwrap_or(Box3 { x0: -300.0, y0: -600.0, z0: 0.0, x1: 300.0, y1: 0.0, z1: 900.0 });
    json!({ "name": clip.name, "plan": null, "body": { "type": "box", "x": jn(b.x0), "y": jn(b.y0), "z": jn(b.z0), "w": jn(b.x1 - b.x0), "d": jn(b.y1 - b.y0), "h": jn(m::max(1.0, b.z1 - b.z0)) } })
}

/// `besideSpot(clip)`: one item width plus GAP_MM along the item's own X axis → (x, y, rot).
pub fn beside_spot(clip: &Clip) -> (f64, f64, f64) {
    let (x0, x1) = clip.bx.map_or((-300.0, 300.0), |b| (b.x0, b.x1));
    let step = (x1 - x0) + GAP_MM;
    let r = (if clip.rot != 0.0 && !clip.rot.is_nan() { clip.rot } else { 0.0 }) * std::f64::consts::PI / 180.0;
    (clip.x + step * m::cos(r), clip.y + step * m::sin(r), clip.rot)
}

/// `pickStorey(clip, storeys, sameModel)` → (id, how: same | name | elevation | first).
pub fn pick_storey(clip: &Clip, storeys: &[StoreyInfo], same_model: bool) -> Option<(Id, &'static str)> {
    let first = storeys.first()?;
    if same_model
        && let Some(st) = clip.storey
        && storeys.iter().any(|s| s.id == st)
    {
        return Some((st, "same"));
    }
    let name = clip.storey_name.trim().to_lowercase();
    if !name.is_empty()
        && let Some(s) = storeys.iter().find(|s| s.name.trim().to_lowercase() == name)
    {
        return Some((s.id, "name"));
    }
    if let Some(e) = clip.storey_elevation.filter(|e| e.is_finite()) {
        let mut v: Vec<&StoreyInfo> = storeys.iter().collect();
        v.sort_by(|a, b| crate::js::num_cmp((a.elevation - e).abs() - (b.elevation - e).abs(), 0.0));
        if let Some(near) = v.first() {
            return Some((near.id, "elevation"));
        }
    }
    Some((first.id, "first"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_at(rot: f64) -> Clip {
        elements_clip(
            &[ElRow { id: 7.0, name: "Wall".into(), cls: "IfcWall".into(), guid: "g".into() }],
            &ElementsCtx {
                x: 1000.0,
                y: 2000.0,
                rot,
                bx: Some(Box3 { x0: -500.0, y0: -100.0, z0: 0.0, x1: 500.0, y1: 100.0, z1: 2700.0 }),
                ..ElementsCtx::default()
            },
        )
    }

    #[test]
    fn paste_turns_by_the_difference() {
        let c = clip_at(-90.0);
        assert_eq!(c.rot, 270.0);
        let (dx, dy, rot, about) = paste_move(&c, 1500.0, 2000.0, 270.0);
        assert_eq!((dx, dy, rot, about), (500.0, 0.0, 0.0, [1000.0, 2000.0]));
        let (_, _, rot, _) = paste_move(&c, 0.0, 0.0, 0.0);
        assert_eq!(rot, 90.0);
    }

    #[test]
    fn beside_goes_one_width_and_the_gap_along_x() {
        let (x, y, _) = beside_spot(&clip_at(0.0));
        assert_eq!((x, y), (2100.0, 2000.0));
    }

    #[test]
    fn storeys_by_name_then_height() {
        let ss = vec![
            StoreyInfo { id: 1.0, name: "Ground".into(), elevation: 0.0 },
            StoreyInfo { id: 2.0, name: "Upper".into(), elevation: 3000.0 },
        ];
        let mut c = clip_at(0.0);
        c.storey = Some(9.0);
        c.storey_name = " upper ".into();
        assert_eq!(pick_storey(&c, &ss, true), Some((2.0, "name")));
        c.storey_name.clear();
        c.storey_elevation = Some(2600.0);
        assert_eq!(pick_storey(&c, &ss, false), Some((2.0, "elevation")));
        c.storey_elevation = None;
        assert_eq!(pick_storey(&c, &ss, false), Some((1.0, "first")));
        assert_eq!(pick_storey(&c, &[], false), None);
    }

    #[test]
    fn boxes_from_a_hostile_item_never_panic() {
        for item in
            [json!(null), json!({"body": {"type": "mesh", "positions": [1, "x"]}}), json!({"body": {"type": "box"}}), json!({"plan": 5})]
        {
            let _ = box_from_item(&item);
        }
        let b = box_from_item(&json!({"body": {"type": "box", "x": -300, "y": -600, "w": 600, "d": 600, "h": 900}}));
        assert_eq!(b, Box3 { x0: -300.0, y0: -600.0, z0: 0.0, x1: 300.0, y1: 0.0, z1: 900.0 });
    }
}
