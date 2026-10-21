//! CadCraft's own clipboard (clipboard.js, with blockcopy.js's `planCopy(…, {refs})` / `applyCopy(…, 'keep')`):
//! COPYCLIP, CUTCLIP, COPYBASE, PASTECLIP, PASTEORIG.
//!
//! * [`make_clip`]: the objects (no ids, no readers' private `_` fields), every block they reach (nested first) and
//!   their layers' settings; viewports stay on their sheet (`skipped`).
//! * [`Clipboard`]: this engine's copy plus the host's shared store (the browser keeps it in localStorage so another
//!   tab pastes; the Rust window keeps one store for every drawing it opens). What comes back from the store is
//!   untrusted and goes through [`read_clip`] like a file.
//! * [`base_point`], [`paste_preview`], [`paste_clip`]: where the copy hangs from, what follows the cursor, and the
//!   paste itself (an existing block of the same name keeps this drawing's definition; anonymous ones get a free
//!   name; layers the copy uses are added with the settings they had).
//!
//! Constraints among the copied objects ride along (gcons.js `clipConstraints` / `pasteConstraints`, cc-gcons): refs by
//! index into the clip's objects; dimensional ones get new names when pasted.

use std::sync::atomic::{AtomicU64, Ordering};

use cc_cad::Doc;
use cc_cad::doc::{Block, block_ref};
use cc_cad::entity::{Entity, Geo, Obj, Poly, Vertex, num, pt};
use cc_cad::js;
use cc_cad::omap::OMap;
use cc_cad_geom::geom::{Blocks, Bounds, K, TAU, m_translate, norm_angle, prims};
use cc_cad_geom::pick::{empty_bounds, entity_bounds, grow_pt, valid_bounds};
use cc_cad_geom::tx::tx_entity;
use cc_geom::P2;
use serde_json::{Map, Value, json};

use crate::lang::Lang;

pub const CLIP_FORMAT: &str = "cadcraft-clipboard";
pub const CLIP_VERSION: i64 = 1;

/// Limits for what comes back from the shared store (clipboard.js `CLIP_LIMITS`).
pub const LIMIT_CHARS: usize = 20_000_000;
pub const LIMIT_ENTITIES: usize = 200_000;
pub const LIMIT_BLOCKS: usize = 20_000;
pub const LIMIT_LAYERS: usize = 10_000;
pub const LIMIT_NODES: usize = 4_000_000;
pub const LIMIT_COORD: f64 = 1e12;
pub const LIMIT_TEXT: usize = 65_536;
pub const LIMIT_DEPTH: usize = 6;

const TYPES: [&str; 13] =
    ["line", "circle", "arc", "poly", "ellipse", "text", "point", "insert", "hatch", "dim", "mleader", "xline", "ray"];
const BAD_KEYS: [&str; 5] = ["constructor", "prototype", "toString", "valueOf", "hasOwnProperty"];

/// The clipboard's contents.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: String,
    pub time: f64,
    pub units: String,
    /// 'model' | 'paper'
    pub space: String,
    pub from: String,
    pub entities: Vec<Entity>,
    pub blocks: Vec<Block>,
    /// `{name, color, on, frozen, locked, plot}`
    pub layers: Vec<Value>,
    /// the COPYBASE point
    pub base: Option<P2>,
    /// viewports left on their sheet
    pub skipped: usize,
    /// constraints among the copied objects (gcons.js `clipConstraints`: refs by index into `entities`)
    pub constraints: Vec<Value>,
}

impl Clip {
    /// The JSON the shared store keeps (clipboard.js's clip object).
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("format".into(), CLIP_FORMAT.into());
        m.insert("version".into(), CLIP_VERSION.into());
        m.insert("id".into(), self.id.clone().into());
        m.insert("time".into(), num(self.time));
        m.insert("units".into(), self.units.clone().into());
        m.insert("space".into(), self.space.clone().into());
        m.insert("from".into(), self.from.clone().into());
        m.insert("entities".into(), Value::Array(self.entities.iter().map(Entity::to_value).collect()));
        m.insert("blocks".into(), Value::Array(self.blocks.iter().map(block_value).collect()));
        m.insert("layers".into(), Value::Array(self.layers.clone()));
        if let Some(b) = self.base {
            m.insert("base".into(), pt(b));
        }
        if self.skipped > 0 {
            m.insert("skipped".into(), self.skipped.into());
        }
        if !self.constraints.is_empty() {
            m.insert("constraints".into(), Value::Array(self.constraints.clone()));
        }
        Value::Object(m)
    }
    fn block_map(&self) -> Blocks {
        let mut m: Blocks = OMap::new();
        for b in &self.blocks {
            m.insert(b.name.clone(), b.clone());
        }
        m
    }
}

fn block_value(b: &Block) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), b.name.clone().into());
    m.insert("base".into(), pt(b.base));
    m.insert("entities".into(), Value::Array(b.entities.iter().map(Entity::to_value).collect()));
    if let Some(a) = b.attdefs.as_ref().filter(|a| !a.is_empty()) {
        m.insert("attdefs".into(), Value::Array(a.clone()));
    }
    Value::Object(m)
}

/// Blocks the entities use directly (inserts and dimensions drawn from a block), in first-use order.
fn refs_of(list: &[Entity]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in list {
        if matches!(e.geo, Geo::Insert(_) | Geo::Dim(_))
            && let Some(b) = block_ref(e).filter(|b| !b.is_empty())
            && !out.iter().any(|x| x == b)
        {
            out.push(b.to_string());
        }
    }
    out
}

/// A plain copy without the id, a view label's link (`field`) and the readers' private `_` fields.
fn plain(e: &Entity) -> Entity {
    let mut c = e.clone();
    c.id = None;
    c.extra.remove("id");
    c.extra.remove("field");
    let private: Vec<String> = c.extra.keys().filter(|k| k.starts_with('_')).cloned().collect();
    for k in private {
        c.extra.remove(&k);
    }
    c
}

/// Set the block an insert / dimension (or any object with a `block` key) names.
fn set_block(e: &mut Entity, name: &str) {
    match &mut e.geo {
        Geo::Insert(i) => i.block = name.to_string(),
        Geo::Dim(d) => d.block = Some(name.to_string()),
        _ => {
            e.extra.insert("block".into(), Value::String(name.to_string()));
        }
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// The clipboard contents for these objects, or None (nothing to copy). `base` = a picked base point (COPYBASE).
pub fn make_clip(doc: &Doc, ids: &[u64], base: Option<P2>) -> Option<Clip> {
    let mut ents: Vec<Entity> = Vec::new();
    let mut ent_ids: Vec<u64> = Vec::new();
    let mut skipped = 0usize;
    for id in ids {
        let Some(e) = doc.get(*id) else { continue };
        if matches!(e.geo, Geo::Viewport(_)) {
            skipped = skipped.saturating_add(1);
            continue;
        }
        ents.push(plain(e));
        ent_ids.push(*id);
    }
    if ents.is_empty() {
        return None;
    }
    // every block reachable from them, nested ones first
    let mut blocks: Vec<Block> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    fn visit(doc: &Doc, name: &str, depth: u32, seen: &mut Vec<String>, blocks: &mut Vec<Block>) {
        if depth > 32 || seen.iter().any(|s| s == name) {
            return;
        }
        seen.push(name.to_string());
        let Some(b) = doc.blocks.get(name) else { return };
        for r in refs_of(&b.entities) {
            visit(doc, &r, depth.saturating_add(1), seen, blocks);
        }
        let attdefs = b.attdefs.clone().filter(|a| !a.is_empty());
        blocks.push(Block {
            name: b.name.clone(),
            base: P2::new(js::or(Some(b.base.x), 0.0), js::or(Some(b.base.y), 0.0)),
            entities: b.entities.iter().map(plain).collect(),
            attdefs,
            xref: None,
            dyn_: None,
        });
    }
    for r in refs_of(&ents) {
        visit(doc, &r, 0, &mut seen, &mut blocks);
    }
    // drop references to blocks the drawing does not have (a reader may leave one): they draw nothing anyway
    let have: Vec<String> = blocks.iter().map(|b| b.name.clone()).collect();
    let has = |n: &str| have.iter().any(|h| h == n);
    let keep = |e: &Entity| !matches!(&e.geo, Geo::Insert(i) if !has(&i.block));
    let clean = |e: Entity| -> Entity {
        if let Geo::Dim(d) = &e.geo
            && let Some(b) = d.block.as_deref()
            && !b.is_empty()
            && !has(b)
        {
            let mut c = e.clone();
            if let Geo::Dim(dd) = &mut c.geo {
                dd.block = None;
            }
            return c;
        }
        e
    };
    let top_ids: Vec<u64> = ents.iter().zip(&ent_ids).filter(|(e, _)| keep(e)).map(|(_, id)| *id).collect();
    let top: Vec<Entity> = ents.into_iter().filter(|e| keep(e)).map(clean).collect();
    for b in &mut blocks {
        let list = std::mem::take(&mut b.entities);
        b.entities = list.into_iter().filter(|e| keep(e)).map(clean).collect();
    }
    if top.is_empty() {
        return None;
    }
    let lay = |e: &Entity| -> String { e.layer.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "0".into()) };
    let mut used: Vec<String> = vec!["0".into()];
    let mut use_l = |n: String| {
        if !used.contains(&n) {
            used.push(n);
        }
    };
    for e in &top {
        use_l(lay(e));
    }
    for b in &blocks {
        for e in &b.entities {
            use_l(lay(e));
        }
        for d in b.attdefs.iter().flatten() {
            use_l(d.get("layer").filter(|v| js::truthy(Some(v))).map_or_else(|| "0".into(), js::to_js_string));
        }
    }
    let layers: Vec<Value> = used
        .iter()
        .filter_map(|n| doc.layers.get(n.as_str()))
        .map(|l| json!({ "name": l.name, "color": l.color.to_value(), "on": l.on, "frozen": l.frozen, "locked": l.locked, "plot": l.plot }))
        .collect();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    #[allow(clippy::cast_precision_loss)]
    let time = seq as f64;
    Some(Clip {
        id: format!("cc{seq:08}"),
        time,
        units: doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default(),
        space: if doc.in_model() { "model".into() } else { "paper".into() },
        from: doc.meta.get("name").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default(),
        entities: top,
        blocks,
        layers,
        base,
        skipped,
        // constraints whose objects are all in the copy (gcons.js; refs by index into `entities`)
        constraints: cc_gcons::gcons::clip_constraints(doc, &top_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect::<Vec<_>>()),
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Validation (what comes back from the shared store is untrusted)

struct Checker<'a> {
    nodes: usize,
    lang: &'a Lang,
}

type V<T> = Result<T, String>;

fn key_ok(k: &str) -> bool {
    let mut cs = k.chars();
    let Some(first) = cs.next() else { return false };
    first.is_ascii_alphabetic() && k.len() <= 32 && cs.all(|c| c.is_ascii_alphanumeric() || c == '_') && !BAD_KEYS.contains(&k)
}
fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}
fn has_control(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 0x20)
}
/// NAME_RE `^[^<>/\\":;?*|=`\u0000-\u001f]{1,255}$`
fn name_ok(n: &str) -> bool {
    let l = len16(n);
    (1..=255).contains(&l) && !n.chars().any(|c| "<>/\\\":;?*|=`".contains(c) || (c as u32) < 0x20)
}

impl Checker<'_> {
    fn t(&self, k: &str, vars: &[(&str, String)]) -> String {
        self.lang.t(k, vars)
    }
    fn num(&self, v: Option<&Value>, what: &str) -> V<f64> {
        match v.and_then(Value::as_f64) {
            Some(x) if x.is_finite() && x.abs() <= LIMIT_COORD => Ok(x),
            _ => Err(self.t("clip.bad.number", &[("what", what.into())])),
        }
    }
    fn pt(&self, v: Option<&Value>, what: &str) -> V<Value> {
        let Some(o) = v.and_then(Value::as_object) else { return Err(self.t("clip.bad.point", &[("what", what.into())])) };
        let x = self.num(o.get("x"), what)?;
        let y = self.num(o.get("y"), what)?;
        Ok(json!({ "x": num(x), "y": num(y) }))
    }
    fn value(&mut self, v: &Value, depth: usize, what: &str) -> V<Value> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > LIMIT_NODES {
            return Err(self.t("clip.clipboard_holds_too_much", &[]));
        }
        match v {
            Value::Null | Value::Bool(_) => Ok(v.clone()),
            Value::Number(_) => Ok(num(self.num(Some(v), what)?)),
            Value::String(s) => {
                if len16(s) > LIMIT_TEXT {
                    return Err(self.t("clip.bad.long", &[("what", what.into())]));
                }
                Ok(v.clone())
            }
            _ if depth > LIMIT_DEPTH => Err(self.t("clip.bad.deep", &[("what", what.into())])),
            Value::Array(a) => {
                let mut out = Vec::with_capacity(a.len().min(1024));
                for x in a {
                    out.push(self.value(x, depth.saturating_add(1), what)?);
                }
                Ok(Value::Array(out))
            }
            Value::Object(o) => {
                let mut out = Map::new();
                for (k, x) in o {
                    if !key_ok(k) {
                        return Err(self.t("clip.bad.field", &[("what", what.into()), ("field", js::slice16(k, 40))]));
                    }
                    out.insert(k.clone(), self.value(x, depth.saturating_add(1), &format!("{what}.{k}"))?);
                }
                Ok(Value::Object(out))
            }
        }
    }
    fn color(&self, c: Option<&Value>) -> V<Value> {
        match c {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(Value::Number(n)) if n.as_f64().is_some_and(|x| x.fract() == 0.0 && (0.0..=256.0).contains(&x)) => {
                Ok(Value::Number(n.clone()))
            }
            Some(Value::String(s)) if s.len() == 7 && s.starts_with('#') && s.chars().skip(1).all(|c| c.is_ascii_hexdigit()) => {
                Ok(Value::String(s.clone()))
            }
            _ => Err(self.t("clip.colour_not_one_cadcraft", &[])),
        }
    }
    fn layer_name(&self, n: Option<&Value>) -> V<String> {
        match n {
            None | Some(Value::Null) => Ok("0".into()),
            Some(Value::String(s)) if s.is_empty() => Ok("0".into()),
            Some(Value::String(s)) if len16(s) <= 255 && !has_control(s) => Ok(s.clone()),
            _ => Err(self.t("clip.layer_name_not_usable", &[])),
        }
    }
    fn tagged(&mut self, v: &Value, what: &str) -> V<Value> {
        let Some(o) = v.as_object() else { return Err(self.t("clip.bad.attrs", &[("what", what.into())])) };
        let mut out = Map::new();
        for (k, x) in o {
            if k.is_empty() || len16(k) > 255 || k == "__proto__" || has_control(k) {
                return Err(self.t("clip.bad.tag", &[("what", what.into())]));
            }
            out.insert(k.clone(), self.value(x, 2, what)?);
        }
        Ok(Value::Object(out))
    }
    /// One entity → a clean copy (fields re-read one by one), or an error.
    fn entity(&mut self, e: &Value, names: &[String], wher: &str) -> V<Value> {
        let Some(o) = e.as_object() else { return Err(self.t("clip.bad.object", &[("where", wher.into())])) };
        let typ = o.get("type").and_then(Value::as_str).unwrap_or("");
        if !TYPES.contains(&typ) {
            let shown = o.get("type").map_or_else(|| "undefined".into(), js::to_js_string);
            return Err(self.t("clip.bad.type", &[("where", wher.into()), ("type", js::slice16(&shown, 20))]));
        }
        let what = self.t("clip.where.in", &[("type", typ.into()), ("where", wher.into())]);
        let mut out = Map::new();
        for (k, x) in o {
            if ["id", "type", "layer", "color"].contains(&k.as_str()) {
                continue;
            }
            if !key_ok(k) {
                return Err(self.t("clip.bad.field.entity", &[("what", what.clone()), ("field", js::slice16(k, 40))]));
            }
            let w = format!("{what} {k}");
            let v = if k == "attrs" || k == "attloc" { self.tagged(x, &w)? } else { self.value(x, 1, &w)? };
            out.insert(k.clone(), v);
        }
        out.insert("type".into(), typ.into());
        out.insert("layer".into(), self.layer_name(o.get("layer"))?.into());
        out.insert("color".into(), self.color(o.get("color"))?);
        let need = |c: &Self, out: &Map<String, Value>, keys: &[&str]| -> V<()> {
            for k in keys {
                c.num(out.get(*k), &format!("{what} {k}"))?;
            }
            Ok(())
        };
        let f = |out: &Map<String, Value>, k: &str| out.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        match typ {
            "line" => need(self, &out, &["x1", "y1", "x2", "y2"])?,
            "circle" => {
                need(self, &out, &["cx", "cy", "r"])?;
                if !(f(&out, "r") > 0.0) {
                    return Err(self.t("clip.circle_has_no_radius", &[]));
                }
            }
            "arc" => {
                need(self, &out, &["cx", "cy", "r", "a0", "a1"])?;
                if !(f(&out, "r") > 0.0) {
                    return Err(self.t("clip.arc_has_no_radius", &[]));
                }
            }
            "point" => need(self, &out, &["x", "y"])?,
            "xline" | "ray" => {
                need(self, &out, &["x", "y", "dx", "dy"])?;
                let (dx, dy) = (f(&out, "dx"), f(&out, "dy"));
                let l = dx.hypot(dy);
                if !(l > 1e-12) {
                    return Err(self.t("clip.bad.direction", &[("what", what.clone())]));
                }
                out.insert("dx".into(), num(dx / l));
                out.insert("dy".into(), num(dy / l));
            }
            "ellipse" => need(self, &out, &["cx", "cy", "mx", "my", "ratio", "t0", "t1"])?,
            "text" => {
                need(self, &out, &["x", "y", "h"])?;
                if !out.get("s").is_some_and(Value::is_string) {
                    return Err(self.t("clip.text_has_no_text", &[]));
                }
                if let Some(r) = out.get("rot") {
                    self.num(Some(r), &format!("{what} rot"))?;
                }
            }
            "poly" => {
                let pts = out.get("pts").and_then(Value::as_array).filter(|a| a.len() >= 2).cloned();
                let Some(pts) = pts else { return Err(self.t("clip.polyline_has_fewer_than", &[])) };
                let mut clean = Vec::with_capacity(pts.len());
                for p in &pts {
                    let mut q = self.pt(Some(p), &format!("{what} point"))?;
                    let b = match p.get("b") {
                        None | Some(Value::Null) => 0.0,
                        Some(b) => self.num(Some(b), &format!("{what} bulge"))?,
                    };
                    if let Some(m) = q.as_object_mut() {
                        m.insert("b".into(), num(b));
                    }
                    clean.push(q);
                }
                out.insert("pts".into(), Value::Array(clean));
                let closed = js::truthy(out.get("closed"));
                out.insert("closed".into(), closed.into());
            }
            "mleader" => {
                let pts = out.get("pts").and_then(Value::as_array).filter(|a| a.len() >= 2).cloned();
                let Some(pts) = pts else { return Err(self.t("clip.mld.pts", &[])) };
                let mut clean = Vec::with_capacity(pts.len());
                for p in &pts {
                    clean.push(self.pt(Some(p), &format!("{what} point"))?);
                }
                out.insert("pts".into(), Value::Array(clean));
                let dir = if out.get("dir").and_then(Value::as_f64) == Some(-1.0) { -1 } else { 1 };
                out.insert("dir".into(), dir.into());
                if out.get("s").is_some_and(|s| !s.is_string()) {
                    return Err(self.t("clip.mld.text", &[]));
                }
            }
            "hatch" => {
                let loops = out.get("loops").and_then(Value::as_array).filter(|a| !a.is_empty()).cloned();
                let Some(loops) = loops else { return Err(self.t("clip.hatch_has_no_boundary", &[])) };
                for l in &loops {
                    let Some(a) = l.as_array().filter(|a| a.len() >= 6 && a.len() % 2 == 0) else {
                        return Err(self.t("clip.hatch_boundary_not_list", &[]));
                    };
                    for v in a {
                        self.num(Some(v), &format!("{what} boundary"))?;
                    }
                }
            }
            "insert" => {
                need(self, &out, &["x", "y"])?;
                for k in ["sx", "sy", "rot"] {
                    if let Some(v) = out.get(k) {
                        self.num(Some(v), &format!("{what} {k}"))?;
                    }
                }
                if !out.get("block").and_then(Value::as_str).is_some_and(|b| names.iter().any(|n| n == b)) {
                    return Err(self.t("clip.block_reference_names_block", &[]));
                }
                if let Some(a) = out.get("attrs") {
                    let Some(o) = a.as_object() else { return Err(self.t("clip.block_attributes_not_list", &[])) };
                    if o.values().any(|v| !v.is_string()) {
                        return Err(self.t("clip.block_attribute_value_not", &[]));
                    }
                }
            }
            "dim" => {
                if let Some(b) = out.get("block")
                    && !b.as_str().is_some_and(|b| names.iter().any(|n| n == b))
                {
                    return Err(self.t("clip.dimension_names_block_clipboard", &[]));
                }
                for k in ["p1", "p2", "c", "loc"] {
                    if let Some(v) = out.get(k).cloned() {
                        let p = self.pt(Some(&v), &format!("{what} {k}"))?;
                        out.insert(k.into(), p);
                    }
                }
                let kind = out.get("kind").and_then(Value::as_str).unwrap_or("");
                if !js::truthy(out.get("block")) && !["linear", "aligned", "angular", "radius", "diameter"].contains(&kind) {
                    return Err(self.t("clip.dimension_has_no_kind", &[]));
                }
                if js::truthy(out.get("kind")) && !(out.contains_key("p1") && out.contains_key("loc")) {
                    return Err(self.t("clip.dimension_missing_points", &[]));
                }
            }
            _ => {}
        }
        Ok(Value::Object(out))
    }
}

/// Validate clipboard contents (JSON text). → the clip, or why not.
pub fn read_clip(input: &str, lang: &Lang) -> Result<Clip, String> {
    let bad = |k: &str| lang.s(k);
    if input.len() > LIMIT_CHARS {
        return Err(bad("clip.clipboard_holds_too_much"));
    }
    let v: Value = serde_json::from_str(input).map_err(|_| bad("clip.clipboard_not_readable"))?;
    let Some(o) = v.as_object() else { return Err(bad("clip.clipboard_not_readable")) };
    if o.get("format").and_then(Value::as_str) != Some(CLIP_FORMAT) {
        return Err(bad("clip.clipboard_does_not_hold"));
    }
    if o.get("version").and_then(Value::as_f64) != Some(1.0) {
        return Err(bad("clip.clipboard_was_written_another"));
    }
    let mut c = Checker { nodes: 0, lang };
    let Some(ents_in) = o.get("entities").and_then(Value::as_array).filter(|a| !a.is_empty()) else {
        return Err(bad("clip.clipboard_holds_no_objects"));
    };
    let empty = Vec::new();
    let blocks_in = match o.get("blocks") {
        None => &empty,
        Some(Value::Array(a)) if a.len() <= LIMIT_BLOCKS => a,
        _ => return Err(bad("clip.clipboard_s_blocks_not")),
    };
    let layers_in = match o.get("layers") {
        None => &empty,
        Some(Value::Array(a)) if a.len() <= LIMIT_LAYERS => a,
        _ => return Err(bad("clip.clipboard_s_layers_not")),
    };
    let mut count = ents_in.len();
    for b in blocks_in {
        count = count.saturating_add(b.get("entities").and_then(Value::as_array).map_or(0, Vec::len));
    }
    if count > LIMIT_ENTITIES {
        return Err(bad("clip.clipboard_holds_too_many"));
    }
    // block names first, so references can be checked
    let mut names: Vec<String> = Vec::new();
    for b in blocks_in {
        let n = b.get("name").and_then(Value::as_str).unwrap_or("");
        let core = n.strip_prefix('*').unwrap_or(n);
        if !b.is_object() || b.get("name").is_none_or(|v| !v.is_string()) || !name_ok(core) {
            return Err(bad("clip.block_name_clipboard_not"));
        }
        if names.iter().any(|x| x == n) {
            return Err(lang.t("clip.bad.twoblocks", &[("name", n.into())]));
        }
        if !b.get("entities").is_some_and(Value::is_array) {
            return Err(lang.t("clip.bad.block.empty", &[("name", n.into())]));
        }
        names.push(n.to_string());
    }
    let mut blocks: Vec<Block> = Vec::new();
    for b in blocks_in {
        let n = b.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let base = match b.get("base") {
            None => json!({ "x": 0, "y": 0 }),
            Some(p) => c.pt(Some(p), &lang.t("clip.where.block.base", &[("name", n.clone())]))?,
        };
        let wher = lang.t("clip.where.block", &[("name", n.clone())]);
        let mut ents = Vec::new();
        for e in b.get("entities").and_then(Value::as_array).into_iter().flatten() {
            ents.push(Entity::from_value(&c.entity(e, &names, &wher)?));
        }
        let mut attdefs = None;
        if let Some(a) = b.get("attdefs") {
            let Some(list) = a.as_array().filter(|l| l.len() <= 1000) else {
                return Err(lang.t("clip.bad.block.attrs", &[("name", n.clone())]));
            };
            let wa = lang.t("clip.where.attr", &[("name", n.clone())]);
            let mut out = Vec::new();
            for d in list {
                let mut cd = c.value(d, 1, &wa)?;
                let tag_ok = cd.get("tag").and_then(Value::as_str).is_some_and(|t| !t.is_empty() && len16(t) <= 255);
                if !cd.is_object() || !tag_ok {
                    return Err(lang.t("clip.bad.attr.tag", &[("name", n.clone())]));
                }
                for k in ["x", "y", "h"] {
                    c.num(cd.get(k), &wa)?;
                }
                let layer = c.layer_name(cd.get("layer"))?;
                let color = c.color(cd.get("color"))?;
                if let Some(m) = cd.as_object_mut() {
                    m.insert("layer".into(), layer.into());
                    m.insert("color".into(), color);
                }
                out.push(cd);
            }
            attdefs = Some(out);
        }
        let bp = P2::new(base.get("x").and_then(Value::as_f64).unwrap_or(0.0), base.get("y").and_then(Value::as_f64).unwrap_or(0.0));
        blocks.push(Block { name: n, base: bp, entities: ents.into(), attdefs: attdefs.filter(|a| !a.is_empty()), xref: None, dyn_: None });
    }
    // no block may contain itself, however deep
    let mut state: Vec<(String, u8)> = Vec::new();
    fn looped(name: &str, blocks: &[Block], state: &mut Vec<(String, u8)>, depth: usize) -> bool {
        match state.iter().find(|(n, _)| n == name).map(|(_, s)| *s) {
            Some(2) => return false,
            Some(1) => return true,
            _ => {}
        }
        if depth > LIMIT_BLOCKS {
            return true;
        }
        state.push((name.to_string(), 1));
        let refs = blocks.iter().find(|b| b.name == name).map(|b| refs_of(&b.entities)).unwrap_or_default();
        for r in refs {
            if looped(&r, blocks, state, depth.saturating_add(1)) {
                return true;
            }
        }
        if let Some(s) = state.iter_mut().find(|(n, _)| n == name) {
            s.1 = 2;
        }
        false
    }
    for b in &blocks {
        if looped(&b.name, &blocks, &mut state, 0) {
            return Err(bad("clip.block_clipboard_contains_itself"));
        }
    }
    let wher = lang.s("clip.the_clipboard");
    let mut entities = Vec::with_capacity(ents_in.len());
    for e in ents_in {
        entities.push(Entity::from_value(&c.entity(e, &names, &wher)?));
    }
    let mut layers: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for l in layers_in {
        if !l.is_object() {
            return Err(bad("clip.layer_clipboard_not_readable"));
        }
        let name = c.layer_name(l.get("name"))?;
        if seen.contains(&name) {
            continue;
        }
        seen.push(name.clone());
        let color = match c.color(l.get("color"))? {
            Value::Null => Value::from(7),
            v => v,
        };
        layers.push(json!({
            "name": name, "color": color,
            "on": l.get("on") != Some(&Value::Bool(false)), "frozen": l.get("frozen") == Some(&Value::Bool(true)),
            "locked": l.get("locked") == Some(&Value::Bool(true)), "plot": l.get("plot") != Some(&Value::Bool(false)),
        }));
    }
    let s = |k: &str, max: usize| o.get(k).and_then(Value::as_str).map(|v| js::slice16(v, max)).unwrap_or_default();
    let base = match o.get("base") {
        None => None,
        Some(b) => {
            let p = c.pt(Some(b), &lang.s("clip.base_point"))?;
            Some(P2::new(p.get("x").and_then(Value::as_f64).unwrap_or(0.0), p.get("y").and_then(Value::as_f64).unwrap_or(0.0)))
        }
    };
    let skipped = o.get("skipped").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    let constraints = match o.get("constraints") {
        None => Vec::new(),
        Some(list) => cc_gcons::gcons::read_clip_constraints(Some(list), entities.len())
            .map_err(|()| lang.s("clip.constraints_clipboard_not_readable"))?,
    };
    Ok(Clip {
        id: s("id", 64),
        time: js::finite(o.get("time")).unwrap_or(0.0),
        units: s("units", 40),
        space: if o.get("space").and_then(Value::as_str) == Some("paper") { "paper".into() } else { "model".into() },
        from: s("from", 255),
        entities,
        blocks,
        layers,
        base,
        skipped,
        constraints,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// The clipboard: this engine's copy + the host's shared store

/// What `get` found.
#[derive(Debug, Default)]
pub struct Got {
    pub clip: Option<Clip>,
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// What a host's shared store holds: text from outside (another tab, the system clipboard: untrusted, read through
/// [`read_clip`]) or a copy made in this process (another drawing of the same window: used as it is).
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Shared {
    Text(String),
    Clip(Clip),
}

#[derive(Debug, Default)]
pub struct Clipboard {
    pub mem: Option<Clip>,
}

impl Clipboard {
    /// Keep a copy; `store` offers it to the host's shared store → { shared }.
    pub fn put(&mut self, clip: Clip, store: impl FnOnce(&Clip) -> bool) -> bool {
        let shared = store(&clip);
        self.mem = Some(clip);
        shared
    }
    /// The newest copy: this engine's, or the store's.
    pub fn get(&self, raw: Option<Shared>, lang: &Lang) -> Got {
        let parsed = match raw {
            Some(Shared::Text(t)) if !t.is_empty() => Some(read_clip(&t, lang)),
            Some(Shared::Clip(c)) => Some(Ok(c)),
            _ => None,
        };
        if let Some(r) = parsed {
            match r {
                Ok(c) => {
                    if self.mem.as_ref().is_none_or(|m| c.id == m.id || c.time >= m.time) {
                        return Got { clip: Some(c), ..Got::default() };
                    }
                }
                Err(e) => {
                    if self.mem.is_none() {
                        return Got { error: Some(e), ..Got::default() };
                    }
                    return Got { clip: self.mem.clone(), warning: Some(e), ..Got::default() };
                }
            }
        }
        Got { clip: self.mem.clone(), ..Got::default() }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Pasting

fn ent_xy(e: &Entity) -> Option<(f64, f64)> {
    match &e.geo {
        Geo::Point(p) => Some((p.x, p.y)),
        Geo::Text(t) => Some((t.x, t.y)),
        Geo::Insert(i) => Some((i.x, i.y)),
        Geo::Xline(i) | Geo::Ray(i) => Some((i.x, i.y)),
        Geo::Viewport(v) => Some((v.x, v.y)),
        _ => Some((e.num("x")?, e.num("y")?)),
    }
}

/// Extents of the copy (inserts measured with the copy's own block definitions).
pub fn clip_extents(clip: &Clip) -> Option<Bounds> {
    let blocks = clip.block_map();
    let mut b = empty_bounds();
    for e in &clip.entities {
        let eb = entity_bounds(e, Some(&blocks));
        if valid_bounds(&eb) {
            grow_pt(&mut b, eb.min_x, eb.min_y);
            grow_pt(&mut b, eb.max_x, eb.max_y);
        } else if let Some((x, y)) = ent_xy(e).filter(|(x, y)| x.is_finite() && y.is_finite()) {
            grow_pt(&mut b, x, y);
        }
    }
    valid_bounds(&b).then_some(b)
}

/// The point the copy hangs from while it follows the cursor ('lower-left' | 'center').
pub fn base_point(clip: &Clip, mode: Option<&str>) -> P2 {
    if let Some(b) = clip.base {
        return b;
    }
    let Some(b) = clip_extents(clip) else { return P2::new(0.0, 0.0) };
    if mode == Some("center") { P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0) } else { P2::new(b.min_x, b.min_y) }
}

/// What to draw while pasting, at the copy's own place: block references and block-drawn dimensions become the
/// lines and arcs of the copy's own definitions. Past `max` pieces the rest shows as its extents.
pub fn paste_preview(clip: &Clip, max: usize) -> Vec<Entity> {
    let blocks = clip.block_map();
    let mut out = Vec::new();
    let mut n = 0usize;
    for e in &clip.entities {
        if n > max {
            break;
        }
        let drawn = matches!(e.geo, Geo::Insert(_)) || matches!(&e.geo, Geo::Dim(d) if d.block.as_deref().is_some_and(|b| !b.is_empty()));
        if drawn {
            for p in prims(e, Some(&blocks)) {
                n = n.saturating_add(1);
                if n > max {
                    break;
                }
                let g = if p.k == K::L {
                    cc_cad_geom::geom::line_ent(p.x1, p.y1, p.x2, p.y2)
                } else if p.full || p.a1 - p.a0 >= TAU - 1e-9 {
                    Geo::Circle(cc_cad::entity::Circle { cx: p.cx, cy: p.cy, r: p.r })
                } else {
                    cc_cad_geom::geom::arc_ent(p.cx, p.cy, p.r, norm_angle(p.a0), norm_angle(p.a1))
                };
                out.push(Entity::new(g));
            }
        } else {
            out.push(e.clone());
            n = n.saturating_add(1);
        }
    }
    if n > max
        && let Some(b) = clip_extents(clip)
    {
        let v = |x: f64, y: f64| Vertex { x, y, b: None, extra: Obj::new() };
        out.push(Entity::new(Geo::Poly(Poly {
            pts: vec![v(b.min_x, b.min_y), v(b.max_x, b.min_y), v(b.max_x, b.max_y), v(b.min_x, b.max_y)],
            closed: Some(true),
        })));
    }
    out
}

/// What a paste did.
#[derive(Debug, Default)]
pub struct Pasted {
    pub ids: Vec<u64>,
    /// old → new block names
    pub renamed: Vec<(String, String)>,
    /// layers added
    pub layers: Vec<String>,
    pub note: String,
    /// constraints that came along
    pub constraints: usize,
}

/// Add the copy to doc's open space, moved by dx, dy (inside a doc change).
pub fn paste_clip(doc: &mut Doc, clip: &Clip, dx: f64, dy: f64, lang: &Lang) -> Result<Pasted, cc_cad::DocError> {
    // blockcopy.js planCopy(doc, src, { refs: true }): every block the entities reach, nested first
    let needed: Vec<String> = {
        let mut needed = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        fn visit(name: &str, depth: u32, clip: &Clip, seen: &mut Vec<String>, needed: &mut Vec<String>) {
            if depth > 32 || seen.iter().any(|s| s == name) {
                return;
            }
            seen.push(name.to_string());
            let Some(b) = clip.blocks.iter().find(|b| b.name == name) else { return };
            for r in refs_of(&b.entities) {
                visit(&r, depth.saturating_add(1), clip, seen, needed);
            }
            needed.push(name.to_string());
        }
        for r in refs_of(&clip.entities) {
            visit(&r, 0, clip, &mut seen, &mut needed);
        }
        needed
    };
    // applyCopy(doc, src, plan, 'keep')
    let mut renamed: Vec<(String, String)> = Vec::new();
    let mut keep: Vec<String> = Vec::new();
    let taken = |doc: &Doc, renamed: &[(String, String)], n: &str| doc.blocks.contains_key(n) || renamed.iter().any(|(_, b)| b == n);
    for n in &needed {
        if doc.blocks.contains_key(n.as_str()) && !n.starts_with('*') {
            keep.push(n.clone());
            continue;
        }
        let to = if !taken(doc, &renamed, n) {
            n.clone()
        } else if n.starts_with('*') {
            let letters: String = n.chars().skip(1).take_while(char::is_ascii_alphabetic).collect();
            let p = format!("*{letters}");
            let mut k = 1u64;
            while taken(doc, &renamed, &format!("{p}{k}")) {
                k = k.saturating_add(1);
            }
            format!("{p}{k}")
        } else {
            let mut k = 2u64;
            while taken(doc, &renamed, &format!("{n} ({k})")) {
                k = k.saturating_add(1);
            }
            format!("{n} ({k})")
        };
        if &to != n {
            renamed.push((n.clone(), to));
        }
    }
    let rename_of = |n: &str| renamed.iter().find(|(a, _)| a == n).map(|(_, b)| b.clone());
    let mut used_layers: Vec<String> = Vec::new();
    for n in &needed {
        if keep.contains(n) {
            continue;
        }
        let Some(b) = clip.blocks.iter().find(|b| &b.name == n) else { continue };
        let ents: Vec<Entity> = b
            .entities
            .iter()
            .map(|e| {
                let mut c = e.clone();
                if let Some(to) = block_ref(e).filter(|x| !x.is_empty()).and_then(rename_of) {
                    set_block(&mut c, &to);
                }
                c
            })
            .collect();
        for e in &ents {
            if let Some(l) = e.layer.as_ref().filter(|l| !l.is_empty())
                && !used_layers.contains(l)
            {
                used_layers.push(l.clone());
            }
        }
        for d in b.attdefs.iter().flatten() {
            let l = d.get("layer").filter(|v| js::truthy(Some(v))).map_or_else(|| "0".into(), js::to_js_string);
            if !used_layers.contains(&l) {
                used_layers.push(l);
            }
        }
        let name = rename_of(n).unwrap_or_else(|| n.clone());
        doc.define_block(&Block { name, base: b.base, entities: ents.into(), attdefs: b.attdefs.clone(), xref: None, dyn_: None })?;
    }
    let src_layer = |n: &str| clip.layers.iter().find(|l| l.get("name").and_then(Value::as_str) == Some(n)).cloned();
    let mut added: Vec<String> = Vec::new();
    for l in used_layers {
        if !doc.layers.contains_key(l.as_str()) {
            doc.add_layer(&src_layer(&l).unwrap_or_else(|| json!({ "name": l })));
            added.push(l);
        }
    }
    // layers the top-level objects use
    for e in &clip.entities {
        let name = e.layer.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "0".into());
        if !doc.layers.contains_key(name.as_str()) {
            doc.add_layer(&src_layer(&name).unwrap_or_else(|| json!({ "name": name })));
            added.push(name);
        }
    }
    let m = m_translate(dx, dy);
    let moved = dx.abs() > 0.0 || dy.abs() > 0.0;
    let mut ids = Vec::with_capacity(clip.entities.len());
    for e0 in &clip.entities {
        let mut e = e0.clone();
        if let Some(to) = block_ref(e0).filter(|x| !x.is_empty()).and_then(rename_of) {
            set_block(&mut e, &to);
        }
        let mut t = if moved { tx_entity(&e, &m) } else { e };
        t.id = None;
        if let Some(id) = doc.add(t)?.id {
            ids.push(id);
        }
    }
    let a = clip.units.as_str();
    let b = doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default();
    let note = if !a.is_empty() && !b.is_empty() && a != b {
        lang.t("clip.units_differ", &[("from", a.into()), ("to", b)])
    } else {
        String::new()
    };
    // constraints among the copied objects come along (model space; dimensional ones get new names)
    let ids_opt: Vec<Option<u64>> = ids.iter().map(|i| Some(*i)).collect();
    let constraints = if clip.constraints.is_empty() {
        0
    } else {
        cc_gcons::gcons::paste_constraints(doc, &clip.constraints, &ids_opt, if moved { dx } else { 0.0 }, if moved { dy } else { 0.0 })?
    };
    Ok(Pasted { ids, renamed, layers: added, note, constraints })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang() -> Lang {
        Lang::new(&Value::Null)
    }
    fn doc_with(v: Value) -> Doc {
        let mut d = Doc::new();
        d.load(&v);
        d
    }

    #[test]
    fn copy_paste_round_trip_with_blocks_and_layers() {
        let d = doc_with(json!({
            "layers": [{ "name": "A", "color": 1 }],
            "entities": [
                { "type": "line", "layer": "A", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
                { "type": "insert", "layer": "0", "block": "B", "x": 5, "y": 5 },
                { "type": "insert", "layer": "0", "block": "Missing", "x": 1, "y": 1 },
            ],
            "blocks": [{ "name": "B", "base": { "x": 0, "y": 0 }, "entities": [{ "type": "circle", "layer": "C", "cx": 0, "cy": 0, "r": 1 }] }],
        }));
        let ids: Vec<u64> = d.entities().filter_map(|e| e.id).collect();
        let clip = make_clip(&d, &ids, None).expect("something to copy");
        assert_eq!(clip.entities.len(), 2, "the reference to a missing block is dropped");
        assert_eq!(clip.blocks.len(), 1);
        assert_eq!(base_point(&clip, None), P2::new(0.0, 0.0));
        // through the shared store and back
        let back = read_clip(&clip.to_value().to_string(), &lang()).expect("readable");
        assert_eq!(back.entities.len(), 2);
        let mut target = Doc::new();
        let r = target.change("PASTECLIP", |t| paste_clip(t, &back, 100.0, 0.0, &lang())).expect("pasted");
        assert_eq!(r.ids.len(), 2);
        assert!(target.blocks.contains_key("B") && target.layers.contains_key("A") && target.layers.contains_key("C"));
        // a clash keeps this drawing's block
        let r2 = target.change("PASTECLIP", |t| paste_clip(t, &back, 0.0, 0.0, &lang())).expect("pasted");
        assert!(r2.renamed.is_empty());
    }

    #[test]
    fn hostile_store_contents_are_refused() {
        let l = lang();
        for bad in [
            "not json",
            "[]",
            r#"{"format":"x","version":1,"entities":[{}]}"#,
            r#"{"format":"cadcraft-clipboard","version":2,"entities":[{}]}"#,
            r#"{"format":"cadcraft-clipboard","version":1,"entities":[]}"#,
            r#"{"format":"cadcraft-clipboard","version":1,"entities":[{"type":"line","x1":0,"y1":0,"x2":1e300,"y2":0}]}"#,
            r#"{"format":"cadcraft-clipboard","version":1,"entities":[{"type":"insert","x":0,"y":0,"block":"B"}],"blocks":[{"name":"B","entities":[{"type":"insert","x":0,"y":0,"block":"B"}]}]}"#,
            r#"{"format":"cadcraft-clipboard","version":1,"entities":[{"type":"line","x1":0,"y1":0,"x2":1,"y2":0,"__proto__":1}]}"#,
            r#"{"format":"cadcraft-clipboard","version":1,"entities":[{"type":"circle","cx":0,"cy":0,"r":0}]}"#,
        ] {
            assert!(read_clip(bad, &l).is_err(), "{bad}");
        }
        // deep nesting stops at the depth limit instead of recursing
        let mut deep = String::from("1");
        for _ in 0..5000 {
            deep = format!("[{deep}]");
        }
        let s = format!(r#"{{"format":"cadcraft-clipboard","version":1,"entities":[{{"type":"point","x":0,"y":0,"q":{deep}}}]}}"#);
        assert!(read_clip(&s, &l).is_err());
    }
}
