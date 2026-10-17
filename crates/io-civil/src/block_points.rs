//! block-points.js — survey points from block references in a DWG or DXF (0.5 WL.19): every top-level model-space
//! INSERT / MINSERT becomes a candidate point at its insertion point (OCS applied, project units), with its attributes.

use crate::cad_linework::{ocs, unit_scale};
use cc_civil::js::{G, J, JsMath, int32, locale_compare_num, max, min, num_to_string, slice16, str_to_number, trim};
use cc_io_dwg::{Obj, V};
use serde_json::{Map, Value};

pub const MAX_REFS: usize = 200_000;

/// `str(v, n)`: control characters → spaces, trimmed, at most n UTF-16 units.
pub fn clean(v: &str, n: usize) -> String {
    slice16(trim(&cc_civil::js::strip_controls(v, " ")), n)
}
fn vstr(v: Option<&V>) -> String {
    match v {
        None | Some(V::Null) => String::new(),
        Some(V::Str(s)) => s.clone(),
        Some(V::Num(n)) => num_to_string(*n),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::Arr(_)) => String::new(),
        Some(_) => "[object Object]".into(),
    }
}
fn num(v: Option<&V>) -> f64 {
    match v {
        Some(V::Num(n)) => *n,
        _ => f64::NAN,
    }
}
fn or0(v: Option<&V>) -> f64 {
    let x = match v {
        Some(V::Num(n)) => *n,
        Some(V::Bool(true)) => 1.0,
        _ => 0.0,
    };
    if x.is_nan() { 0.0 } else { x }
}

/// One block reference.
#[derive(Clone, Debug)]
pub struct BlockRef {
    pub block: String,
    pub layer: String,
    pub handle: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub attrs: Vec<(String, String)>,
}
impl J for BlockRef {
    fn j(&self) -> Value {
        let mut a = Map::new();
        for (k, v) in &self.attrs {
            a.insert(k.clone(), Value::String(v.clone()));
        }
        cc_civil::jo! { "block" => self.block, "layer" => self.layer, "handle" => self.handle, "x" => self.x, "y" => self.y, "z" => self.z, "attrs" => Value::Object(a) }
    }
}
/// blockRefs' result.
#[derive(Clone, Debug)]
pub struct Refs {
    pub refs: Vec<BlockRef>,
    pub blocks: Vec<(String, usize, Vec<String>, Vec<String>)>,
    pub layers: Vec<(String, usize)>,
    pub scale: f64,
    pub notes: Vec<String>,
}
impl J for Refs {
    fn j(&self) -> Value {
        let blocks: Vec<Value> = self
            .blocks
            .iter()
            .map(|(n, c, l, t)| cc_civil::jo! { "name" => n, "count" => *c, "layers" => l.clone(), "tags" => t.clone() })
            .collect();
        let layers: Vec<Value> = self.layers.iter().map(|(n, c)| cc_civil::jo! { "name" => n, "count" => *c }).collect();
        cc_civil::jo! { "refs" => self.refs, "blocks" => blocks, "layers" => layers, "scale" => self.scale, "notes" => self.notes }
    }
}

/// JS default `sort()` on strings: UTF-16 code unit order.
fn sort_utf16(v: &mut [String]) {
    v.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

/// `blockRefs(model, { projectUnit, scale })`.
pub fn block_refs(model: &Obj, project_unit: &str, scale: Option<f64>) -> Refs {
    let mut notes = Vec::new();
    let us = unit_scale(model.get("header").and_then(V::as_obj), project_unit);
    let k = match scale {
        Some(s) if s.is_finite() && s > 0.0 => s,
        _ => us.scale,
    };
    if scale.is_none()
        && let Some(n) = us.note
    {
        notes.push(n);
    }
    let mut by_name: std::collections::HashMap<String, &Obj> = std::collections::HashMap::new();
    for b in model.get("blocks").and_then(V::as_arr).map_or(&[][..], Vec::as_slice) {
        if let Some(o) = b.as_obj() {
            let n = match o.get("name") {
                None => "UNDEFINED".into(),
                v => vstr(v).to_uppercase(),
            };
            by_name.insert(n, o);
        }
    }
    let ms = by_name.get("*MODEL_SPACE").copied();
    let mut refs: Vec<BlockRef> = Vec::new();
    let ents = ms.and_then(|m| m.get("entities")).and_then(V::as_arr).map_or(&[][..], Vec::as_slice);
    'outer: for ev in ents {
        let Some(e) = ev.as_obj() else { continue };
        let ty = e.get("type").and_then(V::as_str).unwrap_or("");
        if (ty != "INSERT" && ty != "MINSERT") || e.get("invisible").is_some_and(V::truthy) {
            continue;
        }
        let bkey = if e.get("block").is_some_and(V::truthy) { vstr(e.get("block")).to_uppercase() } else { String::new() };
        let b = by_name.get(&bkey).copied();
        if let Some(b) = b
            && (b.get("xrefPath").is_some_and(V::truthy) || crate::cad_linework::flag(num(b.get("flags")), 4))
        {
            continue;
        }
        let raw_name = match b.and_then(|b| b.get("trueName")).filter(|v| v.truthy()) {
            Some(v) => vstr(Some(v)),
            None => {
                if e.get("block").is_some_and(V::truthy) {
                    vstr(e.get("block"))
                } else {
                    "?".into()
                }
            }
        };
        let name = clean(&raw_name, 255);
        let ins = e.get("insert").filter(|v| v.truthy()).and_then(V::as_obj);
        let o = ocs(e.get("extrusion")).unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let (ix, iy, iz) = match ins {
            Some(i) => (or0(i.get("x")), or0(i.get("y")), or0(i.get("z"))),
            None => (0.0, 0.0, 0.0),
        };
        let w = [o[0] * ix + o[1] * iy + o[2] * iz, o[4] * ix + o[5] * iy + o[6] * iz, o[8] * ix + o[9] * iy + o[10] * iz];
        let mut attrs: Vec<(String, String)> = Vec::new();
        for a in e.get("attribs").and_then(V::as_arr).map_or(&[][..], Vec::as_slice) {
            let Some(ao) = a.as_obj() else { continue };
            let tag = clean(&vstr(ao.get("tag")), 255).to_uppercase();
            if !tag.is_empty() && !attrs.iter().any(|(t, _)| *t == tag) {
                let val = match ao.get("value") {
                    None | Some(V::Null) => vstr(ao.get("text")),
                    v => vstr(v),
                };
                attrs.push((tag, clean(&val, 500)));
            }
        }
        let cnt = |key: &str| -> usize {
            if ty != "MINSERT" {
                return 1;
            }
            let v = int32(num(e.get(key)));
            let v = if v != 0.0 { v } else { 1.0 };
            cc_civil::js::usize_of(max(1.0, min(1000.0, v))).unwrap_or(1)
        };
        let (cols, rows) = (cnt("cols"), cnt("rows"));
        let r = or0(e.get("rotation"));
        let (c, s) = (r.jcos(), r.jsin());
        let layer = clean(
            &match e.get("layer") {
                None | Some(V::Null) => "0".into(),
                v => vstr(v),
            },
            255,
        );
        let handle = clean(&vstr(e.get("handle")), 32);
        for ci in 0..cols {
            for ri in 0..rows {
                let dx = ci as f64 * or0(e.get("colSpacing"));
                let dy = ri as f64 * or0(e.get("rowSpacing"));
                let ox = c * dx - s * dy;
                let oy = s * dx + c * dy;
                let p = [w[0] + o[0] * ox + o[1] * oy, w[1] + o[4] * ox + o[5] * oy, w[2] + o[8] * ox + o[9] * oy].map(|v| v * k);
                if !p.iter().all(|v| v.is_finite()) {
                    continue;
                }
                let h = if cols.saturating_mul(rows) > 1 {
                    format!("{handle}#{}", ci.saturating_mul(rows).saturating_add(ri).saturating_add(1))
                } else {
                    handle.clone()
                };
                refs.push(BlockRef {
                    block: name.clone(),
                    layer: layer.clone(),
                    handle: h,
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    attrs: attrs.clone(),
                });
                if refs.len() >= MAX_REFS {
                    break; // the JS leaves only the row loop: the next column still adds its references
                }
            }
        }
        if refs.len() >= MAX_REFS {
            notes.push("max".into());
            break 'outer;
        }
    }
    let mut blocks: Vec<(String, usize, Vec<String>, Vec<String>)> = Vec::new();
    let mut layers: Vec<(String, usize)> = Vec::new();
    for r in &refs {
        let i = match blocks.iter().position(|b| b.0 == r.block) {
            Some(i) => i,
            None => {
                blocks.push((r.block.clone(), 0, Vec::new(), Vec::new()));
                blocks.len().saturating_sub(1)
            }
        };
        if let Some(b) = blocks.get_mut(i) {
            b.1 = b.1.saturating_add(1);
            if !b.2.contains(&r.layer) {
                b.2.push(r.layer.clone());
            }
            for (t, _) in &r.attrs {
                if !b.3.contains(t) {
                    b.3.push(t.clone());
                }
            }
        }
        match layers.iter_mut().find(|(n, _)| *n == r.layer) {
            Some(l) => l.1 = l.1.saturating_add(1),
            None => layers.push((r.layer.clone(), 1)),
        }
    }
    for b in &mut blocks {
        sort_utf16(&mut b.2);
        sort_utf16(&mut b.3);
    }
    let order = |ac: usize, an: &str, bc: usize, bn: &str| {
        let d = bc as f64 - ac as f64;
        if d != 0.0 { d } else { locale_compare_num(an, bn) }
    };
    cc_civil::js::sort_by(&mut blocks, |a, b| order(a.1, &a.0, b.1, &b.0));
    cc_civil::js::sort_by(&mut layers, |a, b| order(a.1, &a.0, b.1, &b.0));
    Refs { refs, blocks, layers, scale: k, notes }
}

/// `attrNumber(text)`: the first number in attribute text ("EL 105.32'", "105,32").
pub fn attr_number(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    while i < n {
        let mut j = i;
        if matches!(b.get(j), Some(b'-' | b'+')) {
            j = j.saturating_add(1);
        }
        let ds = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > ds {
            // (?:[.,]\d+)?
            if matches!(b.get(j), Some(b'.' | b',')) && b.get(j.saturating_add(1)).is_some_and(u8::is_ascii_digit) {
                j = j.saturating_add(1);
                while b.get(j).is_some_and(u8::is_ascii_digit) {
                    j = j.saturating_add(1);
                }
            }
            // (?:e[-+]?\d+)?
            if matches!(b.get(j), Some(b'e' | b'E')) {
                let mut k = j.saturating_add(1);
                if matches!(b.get(k), Some(b'-' | b'+')) {
                    k = k.saturating_add(1);
                }
                let es = k;
                while b.get(k).is_some_and(u8::is_ascii_digit) {
                    k = k.saturating_add(1);
                }
                if k > es {
                    j = k;
                }
            }
            let m = text.get(i..j).unwrap_or("").replacen(',', ".", 1);
            let v = str_to_number(&m);
            return if v.is_finite() { Some(v) } else { None };
        }
        i = i.saturating_add(1);
    }
    None
}

/// A point from blocks.
#[derive(Clone, Debug)]
pub struct BlkPoint {
    pub n: String,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub d: String,
    pub blk: (String, String, String),
}
impl J for BlkPoint {
    fn j(&self) -> Value {
        cc_civil::jo! {
            "n" => self.n, "x" => self.x, "y" => self.y, "z" => self.z, "d" => self.d,
            "blk" => cc_civil::jo! { "name" => self.blk.0, "layer" => self.blk.1, "handle" => self.blk.2 },
        }
    }
}

/// pointsFromBlocks' options.
pub struct FromBlocks<'a> {
    pub blocks: &'a [String],
    pub layers: &'a [String],
    pub desc: &'a str,
    pub z: &'a str,
    pub number: &'a str,
    pub existing: &'a [String],
    pub start: Option<f64>,
}

/// `pointsFromBlocks(refs, opts)` → (points, noZ, noNumber, renumbered).
pub fn points_from_blocks(refs: &[BlockRef], o: &FromBlocks) -> (Vec<BlkPoint>, usize, usize, usize) {
    let chosen = refs.iter().filter(|r| o.blocks.contains(&r.block) || o.layers.contains(&r.layer));
    let tag_of = |how: &str| how.strip_prefix("attr:").map(str::to_uppercase);
    let (d_tag, z_tag, n_tag) = (tag_of(o.desc), tag_of(o.z), tag_of(o.number));
    let mut taken: std::collections::HashSet<String> = o.existing.iter().cloned().collect();
    let mut highest: f64 = o
        .existing
        .iter()
        .fold(0.0, |m, n| if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) { max(m, str_to_number(n)) } else { m });
    if let Some(s) = o.start
        && s.is_finite()
        && s > 0.0
    {
        highest = max(0.0, s.floor() - 1.0);
    }
    let (mut no_z, mut no_number, mut renumbered) = (0usize, 0usize, 0usize);
    let mut points = Vec::new();
    let attr = |r: &BlockRef, t: &str| r.attrs.iter().find(|(a, _)| a == t).map(|(_, v)| v.clone());
    for r in chosen {
        let mut n = match &n_tag {
            Some(t) => clean(&attr(r, t).unwrap_or_default(), 40),
            None => String::new(),
        };
        if n_tag.is_some() && n.is_empty() {
            no_number = no_number.saturating_add(1);
        }
        if n.is_empty() || taken.contains(&n) {
            if !n.is_empty() {
                renumbered = renumbered.saturating_add(1);
            }
            loop {
                highest += 1.0;
                if !taken.contains(&num_to_string(highest)) {
                    break;
                }
            }
            n = num_to_string(highest);
        }
        taken.insert(n.clone());
        let mut zz: Option<f64> = None;
        if o.z == "insert" {
            zz = Some(r.z);
        } else if let Some(t) = &z_tag {
            zz = attr_number(&attr(r, t).unwrap_or_default());
            if zz.is_none() {
                no_z = no_z.saturating_add(1);
            }
        }
        let mut d = if o.desc == "layer" {
            r.layer.clone()
        } else if let Some(t) = &d_tag {
            clean(&attr(r, t).unwrap_or_default(), 200)
        } else {
            r.block.clone()
        };
        if d.is_empty() {
            d = clean(&r.block, 200);
        }
        points.push(BlkPoint {
            n,
            x: r.x,
            y: r.y,
            z: zz.filter(|z| z.is_finite() && z.abs() < 1e6),
            d: clean(&d, 200),
            blk: (clean(&r.block, 255), clean(&r.layer, 255), clean(&r.handle, 40)),
        });
    }
    (points, no_z, no_number, renumbered)
}

/// `cleanBlk(raw)` — a point's `blk` from a project file (untrusted).
pub fn clean_blk(raw: &Value) -> Option<Value> {
    let o = raw.as_object()?;
    let name = o.get("name")?.as_str()?;
    let s = |k: &str, n: usize| clean(&cc_civil::js::string_or_empty(o.get(k)), n);
    Some(cc_civil::jo! { "name" => clean(name, 255), "layer" => s("layer", 255), "handle" => s("handle", 40) })
}

impl cc_civil::js::Und for BlockRef {
    fn und() -> Self {
        BlockRef {
            block: String::new(),
            layer: String::new(),
            handle: String::new(),
            x: f64::NAN,
            y: f64::NAN,
            z: f64::NAN,
            attrs: Vec::new(),
        }
    }
}
/// `G` kept in scope for later use.
pub fn first_ref(r: &[BlockRef]) -> BlockRef {
    r.g(0)
}
