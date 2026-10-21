//! blockcopy.js: bring block definitions from another drawing into this one (INSERT, option File): one of its blocks,
//! or its whole model as one block named after the file. Nested blocks come along; a named block that is already here
//! is kept ('keep') or the incoming one gets a new name ('rename': "Name (2)"); anonymous blocks always get a free
//! name. Layers the copied objects use are added with the other drawing's settings. `scale` turns the other drawing's
//! units into this one's (1 when either has none).

use cc_cad::Doc;
use cc_cad::doc::Block;
use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use serde_json::{Value, json};

use crate::cmd::xref::{XSource, block_name_from_file, mm_per_unit};
use crate::lang::Lang;

/// What `planCopy` returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// the block to place (None: only the references, PASTECLIP's case, not used here)
    pub root: Option<String>,
    pub whole: bool,
    /// every block reachable from the root, nested ones first
    pub needed: Vec<String>,
    /// named blocks this drawing has already
    pub clashes: Vec<String>,
    pub scale: f64,
    pub note: String,
    /// the whole drawing's base point (its INSBASE)
    pub base: Value,
}

/// What to copy.
pub enum Which<'a> {
    Block(&'a str),
    Whole(&'a str),
}

/// `e.block` when truthy (any object: an insert or dimension drawn from a block, or one kept as given).
fn block_of(e: &Entity) -> Option<String> {
    match &e.geo {
        Geo::Insert(i) => Some(i.block.clone()),
        Geo::Dim(d) => d.block.clone(),
        _ => e.extra.get("block").filter(|b| js::truthy(Some(b))).map(js::to_js_string),
    }
    .filter(|b| !b.is_empty())
}

fn set_block_of(e: &mut Entity, to: &str) {
    match &mut e.geo {
        Geo::Insert(i) => to.clone_into(&mut i.block),
        Geo::Dim(d) => d.block = Some(to.to_string()),
        _ => {
            e.extra.insert("block".into(), Value::String(to.to_string()));
        }
    }
}

/// `refsOf(entities)`: the blocks inserts and dimensions use directly, in first-use order.
fn refs_of(entities: &[Entity]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in entities {
        if !matches!(e.typ(), "insert" | "dim") {
            continue;
        }
        if let Some(b) = block_of(e)
            && !out.contains(&b)
        {
            out.push(b);
        }
    }
    out
}

/// `new Map(src.blocks.map((b) => [b.name, b]))`: the last block of a name wins.
fn src_block<'a>(src: &'a XSource, name: &str) -> Option<&'a crate::cmd::xref::XBlock> {
    src.blocks.iter().rev().find(|b| b.name == name)
}

/// `planCopy(doc, src, { block } | { whole })`.
pub fn plan_copy(doc: &Doc, src: &XSource, which: Which<'_>, lang: &Lang) -> Result<Plan, String> {
    let (root, root_refs, whole) = match which {
        Which::Whole(file) => (block_name_from_file(file), refs_of(&src.entities), true),
        Which::Block(b) => {
            if src_block(src, b).is_none() {
                return Err(lang.t("blockcopy.noblock", &[("name", b.to_string())]));
            }
            (b.to_string(), vec![b.to_string()], false)
        }
    };
    let mut needed: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    fn visit(src: &XSource, name: &str, depth: u32, seen: &mut Vec<String>, needed: &mut Vec<String>) {
        if depth > 32 || seen.iter().any(|s| s == name) {
            return;
        }
        seen.push(name.to_string());
        let Some(b) = src_block(src, name) else { return };
        for r in refs_of(&b.entities) {
            visit(src, &r, depth.saturating_add(1), seen, needed);
        }
        needed.push(name.to_string());
    }
    for r in &root_refs {
        visit(src, r, 0, &mut seen, &mut needed);
    }
    let mut clashes: Vec<String> = needed.iter().filter(|n| !n.starts_with('*') && doc.blocks.contains_key(n.as_str())).cloned().collect();
    if whole && doc.blocks.contains_key(root.as_str()) {
        clashes.push(root.clone());
    }
    let su = src.meta.get("units");
    let du = doc.meta.get("units").cloned();
    let (a, b) = (mm_per_unit(su).filter(|x| *x != 0.0), mm_per_unit(du.as_ref()).filter(|x| *x != 0.0));
    let scale = match (a, b) {
        (Some(a), Some(b)) => a / b,
        _ => 1.0,
    };
    let note = match (a, b) {
        (Some(a), Some(b)) if a != b => lang.t(
            "blockcopy.units",
            &[
                ("from", su.map(js::to_js_string).unwrap_or_default()),
                ("to", du.as_ref().map(js::to_js_string).unwrap_or_default()),
                ("scale", js::num_to_string(js::str_to_number(&js::to_exponential(scale, 5)))),
            ],
        ),
        _ => String::new(),
    };
    let base = if whole {
        src.meta.get("insbase").filter(|v| js::truthy(Some(v))).cloned().unwrap_or_else(|| json!({ "x": 0, "y": 0 }))
    } else {
        Value::Null
    };
    Ok(Plan { root: Some(root), whole, needed, clashes, scale, note, base })
}

/// What `applyCopy` returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Copied {
    /// the block to place
    pub name: Option<String>,
    pub copied: usize,
    /// old → new names
    pub renamed: Vec<(String, String)>,
    /// layers added
    pub layers: Vec<String>,
}

/// `applyCopy(doc, src, plan, onClash)` inside a change (`keep`: a clash keeps this drawing's definition). The copied
/// objects are moved out of `src` (the JS shares them; here the other drawing is not used afterwards).
pub fn apply_copy(doc: &mut Doc, src: &mut XSource, plan: &Plan, keep_existing: bool) -> Result<Copied, cc_cad::DocError> {
    let mut renamed: Vec<(String, String)> = Vec::new();
    let mut keep: Vec<String> = Vec::new();
    let taken = |doc: &Doc, renamed: &[(String, String)], n: &str| doc.blocks.contains_key(n) || renamed.iter().any(|(_, b)| b == n);
    let free = |doc: &Doc, renamed: &[(String, String)], name: &str| -> String {
        if !taken(doc, renamed, name) {
            return name.to_string();
        }
        if name.starts_with('*') {
            let letters: String = name.chars().skip(1).take_while(char::is_ascii_alphabetic).collect();
            let p = format!("*{letters}");
            let mut k = 1u64;
            while taken(doc, renamed, &format!("{p}{k}")) {
                k = k.saturating_add(1);
            }
            return format!("{p}{k}");
        }
        let mut k = 2u64;
        while taken(doc, renamed, &format!("{name} ({k})")) {
            k = k.saturating_add(1);
        }
        format!("{name} ({k})")
    };
    for n in &plan.needed {
        if doc.blocks.contains_key(n.as_str()) && !n.starts_with('*') && keep_existing {
            keep.push(n.clone());
            continue;
        }
        let to = free(doc, &renamed, n);
        if &to != n {
            renamed.push((n.clone(), to));
        }
    }
    let rename_of = |n: &str| renamed.iter().find(|(a, _)| a == n).map(|(_, b)| b.clone());
    let rename = |mut e: Entity| -> Entity {
        if let Some(to) = block_of(&e).and_then(|b| rename_of(&b)) {
            set_block_of(&mut e, &to);
        }
        e
    };
    let mut layers: Vec<String> = Vec::new();
    let use_layer = |layers: &mut Vec<String>, l: String| {
        if !layers.contains(&l) {
            layers.push(l);
        }
    };
    let mut copied = 0usize;
    for n in &plan.needed {
        if keep.contains(n) {
            continue;
        }
        // `new Map(src.blocks.map(…))`: the last block of a name
        let Some(b) = src.blocks.iter_mut().rev().find(|b| b.name == *n) else { continue };
        let ents: Vec<Entity> = std::mem::take(&mut b.entities).into_iter().map(rename).collect();
        for e in &ents {
            if let Some(l) = e.layer.clone().filter(|l| !l.is_empty()) {
                use_layer(&mut layers, l);
            }
        }
        for d in b.attdefs.iter().flatten() {
            use_layer(&mut layers, d.get("layer").filter(|v| js::truthy(Some(v))).map_or_else(|| "0".into(), js::to_js_string));
        }
        let mut blk = Block::from_value(&json!({ "name": rename_of(n).unwrap_or_else(|| n.clone()), "base": b.base }));
        blk.entities = ents.into();
        blk.attdefs = b.attdefs.take().filter(|a| !a.is_empty());
        doc.define_block_owned(blk)?;
        copied = copied.saturating_add(1);
    }
    let mut name = plan.root.clone();
    if plan.whole {
        let root = plan.root.clone().unwrap_or_default();
        if !(doc.blocks.contains_key(root.as_str()) && keep_existing) {
            let to = free(doc, &renamed, &root);
            let ents: Vec<Entity> = std::mem::take(&mut src.entities).into_iter().filter(|e| e.typ() != "viewport").map(rename).collect();
            for e in &ents {
                if let Some(l) = e.layer.clone().filter(|l| !l.is_empty()) {
                    use_layer(&mut layers, l);
                }
            }
            let mut blk = Block::from_value(&json!({ "name": to, "base": plan.base }));
            blk.entities = ents.into();
            doc.define_block_owned(blk)?;
            copied = copied.saturating_add(1);
            name = Some(to);
        }
    } else if let Some(n) = &name {
        name = Some(rename_of(n).unwrap_or_else(|| n.clone()));
    }
    let src_layer = |n: &str| src.layers.iter().rev().find(|l| l.get("name").map(js::to_js_string).as_deref() == Some(n)).cloned();
    let mut added: Vec<String> = Vec::new();
    for l in layers {
        if !doc.layers.contains_key(l.as_str()) {
            doc.add_layer(&src_layer(&l).unwrap_or_else(|| json!({ "name": l })));
            added.push(l);
        }
    }
    Ok(Copied { name, copied, renamed, layers: added })
}
