//! dyn-blocks.js, the reading side (0.5 DB.2): a dynamic block keeps every file entity (hidden ones too) in
//! `block.dyn`; a reference keeps its values in `insert.dyn`; `doc.dynSrc` keeps the file objects the dynamic data
//! needs (raw, for dwg-save.js).

use std::collections::{HashMap, HashSet, VecDeque};

use cc_io_dwg::value::{Obj, V};

use crate::jsv::{self, Ov};

const MAX_STATES: usize = 1000;
const MAX_LIST: usize = 100_000;

/// `hexKey(h)`: upper case, leading zeros dropped ('' for none).
pub fn hex_key(h: Ov) -> String {
    if jsv::nullish(h) {
        return String::new();
    }
    let s = jsv::upper(&jsv::to_str(h));
    let t = s.trim_start_matches('0');
    if t.is_empty() && !s.is_empty() { "0".to_string() } else { t.to_string() }
}
/// `cleanName(s)`: control characters dropped, at most 255 characters.
pub fn clean_name(s: Ov) -> String {
    let t: String = jsv::str_nn(s).chars().filter(|c| u32::from(*c) > 0x1f).collect();
    crate::text::slice16(&t, 0, 255)
}

/// What `dynBlockFromDwg` makes.
pub struct DynBlock {
    pub entities: Vec<V>,
    pub attdefs: Vec<V>,
    pub dyn_: Obj,
}

/// `dynBlockFromDwg(b, { convert, attdefOf, defName, record })`: a file block that is dynamic → its entities (the
/// shown ones), attribute definitions and `dyn`; None for a plain block.
pub fn dyn_block_from_dwg(
    b: &Obj,
    convert: &mut dyn FnMut(&Obj) -> Option<V>,
    attdef_of: &dyn Fn(&Obj) -> Option<Obj>,
    def_name: Option<String>,
    record: Option<&Obj>,
) -> Option<DynBlock> {
    if !jsv::truthy(b.get("dynamic")) && !jsv::truthy(b.get("dynamicOf")) {
        return None;
    }
    let list = jsv::arr(b.get("entities"));
    let list = list.get(..list.len().min(MAX_LIST)).unwrap_or(&[]);
    let mut all: Vec<Vec<V>> = Vec::new();
    let mut h: Vec<String> = Vec::new();
    let mut shown: Vec<bool> = Vec::new();
    let mut attdefs: Vec<V> = Vec::new();
    let mut ad: Vec<usize> = Vec::new();
    for e in list {
        let Some(eo) = jsv::obj(Some(e)) else {
            h.push(String::new());
            shown.push(true);
            all.push(Vec::new());
            continue;
        };
        h.push(hex_key(eo.get("handle")));
        shown.push(!jsv::truthy(eo.get("invisible")));
        if jsv::s(eo.get("type")) == Some("ATTDEF") {
            if let Some(d) = attdef_of(eo) {
                attdefs.push(V::Obj(d));
                ad.push(all.len());
            }
            all.push(Vec::new());
            continue;
        }
        let mut copy = eo.clone();
        copy.set("invisible", false);
        let c = convert(&copy);
        all.push(match c {
            Some(c) => jsv::flat1(c).into_iter().filter(V::truthy).collect(),
            None => Vec::new(),
        });
    }
    let mut d = Obj::new();
    d.set("src", hex_key(b.get("record")));
    d.set("h", V::Arr(h.iter().map(|x| V::Str(x.clone())).collect()));
    d.set("shown", V::Arr(shown.iter().map(|x| V::Bool(*x)).collect()));
    d.set("all", V::Arr(all.iter().map(|x| V::Arr(x.clone())).collect()));
    d.set("ad", V::Arr(ad.iter().map(|x| V::from(*x)).collect()));
    if let Some(r) = record {
        if jsv::truthy(r.get("extDict")) {
            d.set("x", hex_key(r.get("extDict")));
        }
        if let Some(xd) = r.get("xdata").filter(|x| matches!(x, V::Obj(_) | V::Arr(_) | V::Bytes(_))) {
            d.set("xd", xd.clone());
        }
    }
    if let Some(dynamic) = b.get("dynamic").filter(|x| x.truthy()) {
        d.set("def", true);
        let params = jsv::arr(jsv::get(Some(dynamic), "params"));
        d.set(
            "params",
            V::Arr(
                params
                    .iter()
                    .map(|p| {
                        let g = |k: &str| jsv::get(Some(p), k);
                        let mut o = Obj::new();
                        o.set_opt("node", g("node").cloned());
                        o.set_opt("kind", g("kind").cloned());
                        o.set("name", clean_name(g("name")));
                        V::Obj(o)
                    })
                    .collect(),
            ),
        );
        if let Some(v) = params
            .iter()
            .find(|p| jsv::s(jsv::get(Some(p), "kind")) == Some("visibility") && matches!(jsv::get(Some(p), "states"), Some(V::Arr(_))))
        {
            let g = |k: &str| jsv::get(Some(v), k);
            let mut at: HashMap<&str, usize> = HashMap::new();
            for (i, x) in h.iter().enumerate() {
                at.insert(x.as_str(), i);
            }
            let mut vis = Obj::new();
            vis.set_opt("node", g("node").cloned());
            vis.set("name", clean_name(g("name")));
            vis.set("label", clean_name(g("label")));
            if jsv::truthy(g("handle")) {
                vis.set("param", hex_key(g("handle")));
            }
            let pt = g("point");
            let point = match (jsv::fin(jsv::get(pt, "x")), jsv::fin(jsv::get(pt, "y"))) {
                (Some(x), Some(y)) if jsv::truthy(pt) => jsv::p2(x, y),
                _ => V::Null,
            };
            vis.set("point", point);
            let states = jsv::arr(g("states"));
            vis.set(
                "states",
                V::Arr(
                    states
                        .iter()
                        .take(MAX_STATES)
                        .map(|s| {
                            let show: Vec<V> = jsv::arr(jsv::get(Some(s), "entities"))
                                .iter()
                                .filter_map(|x| at.get(hex_key(Some(x)).as_str()).map(|i| V::from(*i)))
                                .collect();
                            let mut o = Obj::new();
                            o.set("name", clean_name(jsv::get(Some(s), "name")));
                            o.set("show", V::Arr(show));
                            V::Obj(o)
                        })
                        .collect(),
                ),
            );
            d.set("vis", vis);
        }
    }
    if jsv::truthy(b.get("dynamicOf"))
        && let Some(n) = def_name.filter(|n| !n.is_empty())
    {
        d.set("of", n);
    }
    let entities: Vec<V> = all.iter().zip(shown.iter()).filter(|(_, s)| **s).flat_map(|(a, _)| a.iter().cloned()).collect();
    Some(DynBlock { entities, attdefs, dyn_: d })
}

/// `dynRefFromDwg(e)`: a reference's `dyn` from cc-dwg's INSERT → CadCraft's insert.dyn.
pub fn dyn_ref_from_dwg(e: &Obj) -> Option<V> {
    let d = e.get("dyn").filter(|x| x.truthy())?;
    let values: Vec<V> = jsv::arr(jsv::get(Some(d), "values"))
        .iter()
        .take(1000)
        .map(|v| match v {
            V::Obj(o) => {
                let mut c = o.clone();
                c.remove("data");
                V::Obj(c)
            }
            _ => V::Obj(Obj::new()),
        })
        .collect();
    let mut o = Obj::new();
    if let Some(st) = jsv::s(jsv::get(Some(d), "state")) {
        o.set("state", clean_name(Some(&V::from(st))));
    }
    o.set("values", V::Arr(values));
    o.set("src", hex_key(e.get("handle")));
    if jsv::truthy(e.get("extDict")) {
        o.set("x", hex_key(e.get("extDict")));
    }
    Some(V::Obj(o))
}

/// `dynObjects(db)`: each dynamic definition's / copy's / reference's extension dictionary tree with everything owned
/// by something in it.
fn dyn_objects(db: &Obj) -> Vec<V> {
    let objs = jsv::arr(db.get("objects"));
    let mut by_h: HashMap<String, &V> = HashMap::new();
    let mut kids: HashMap<String, Vec<&V>> = HashMap::new();
    for o in objs {
        by_h.insert(hex_key(jsv::get(Some(o), "handle")), o);
        let k = hex_key(jsv::get(Some(o), "owner"));
        if !k.is_empty() {
            kids.entry(k).or_default().push(o);
        }
    }
    let tables = db.get("tables");
    let mut recs: HashMap<String, &V> = HashMap::new();
    for r in jsv::arr(jsv::get(tables, "blockRecords")) {
        recs.insert(hex_key(jsv::get(Some(r), "handle")), r);
    }
    let mut roots: Vec<String> = Vec::new();
    for b in jsv::arr(db.get("blocks")) {
        let g = |k: &str| jsv::get(Some(b), k);
        if jsv::truthy(g("dynamic")) || jsv::truthy(g("dynamicOf")) {
            let rr = recs.get(&hex_key(g("record")));
            if let Some(x) = rr.and_then(|r| jsv::get(Some(r), "extDict")).filter(|x| x.truthy()) {
                roots.push(hex_key(Some(x)));
            }
        }
        for e in jsv::arr(g("entities")) {
            if jsv::truthy(jsv::get(Some(e), "dyn")) && jsv::truthy(jsv::get(Some(e), "extDict")) {
                roots.push(hex_key(jsv::get(Some(e), "extDict")));
            }
        }
    }
    let mut keep: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = roots.into_iter().collect();
    while let Some(k) = queue.pop_front() {
        if k.is_empty() || seen.contains(&k) || !by_h.contains_key(&k) {
            continue;
        }
        seen.insert(k.clone());
        keep.push(k.clone());
        let Some(o) = by_h.get(&k).copied() else { continue };
        if let Some(x) = jsv::get(Some(o), "extDict").filter(|x| x.truthy()) {
            queue.push_back(hex_key(Some(x)));
        }
        if let Some(V::Obj(en)) = jsv::get(Some(o), "entries") {
            for key in jsv::keys_js(en) {
                queue.push_back(hex_key(en.get(key)));
            }
        }
        for c in kids.get(&k).map(Vec::as_slice).unwrap_or(&[]) {
            queue.push_back(hex_key(jsv::get(Some(c), "handle")));
        }
    }
    keep.iter().filter_map(|k| by_h.get(k).map(|o| (*o).clone())).collect()
}

/// `/^(BLOCK\w*|ACAD_EVALUATION_GRAPH|ACDB_BLOCKREPRESENTATION_DATA|ACDB_DYNAMICBLOCK\w*)$/`
fn dyn_class(s: &str) -> bool {
    let word = |r: &str| r.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
    s.strip_prefix("BLOCK").is_some_and(word)
        || s == "ACAD_EVALUATION_GRAPH"
        || s == "ACDB_BLOCKREPRESENTATION_DATA"
        || s.strip_prefix("ACDB_DYNAMICBLOCK").is_some_and(word)
}

/// `dynSource(db)`: doc.dynSrc — the objects, the classes they use, the version and maintenance they came in.
pub fn dyn_source(db: &Obj) -> V {
    let objects = dyn_objects(db);
    let nums: Vec<f64> =
        objects.iter().filter_map(|o| jsv::fin(jsv::get(Some(o), "typeNum"))).filter(|n| n.fract() == 0.0 && *n >= 500.0).collect();
    let classes: Vec<V> = jsv::arr(db.get("classes"))
        .iter()
        .filter(|c| {
            let num = jsv::get(Some(c), "num");
            let hit = match num {
                Some(V::Num(n)) => nums.contains(n),
                _ => false,
            };
            hit || dyn_class(&jsv::str_or_empty(jsv::get(Some(c), "dxfName")))
        })
        .map(|c| match c {
            V::Obj(o) => V::Obj(o.clone()),
            _ => V::Obj(Obj::new()),
        })
        .collect();
    let records: Vec<V> = jsv::arr(db.get("blocks"))
        .iter()
        .filter(|b| jsv::truthy(jsv::get(Some(b), "dynamic")) || jsv::truthy(jsv::get(Some(b), "dynamicOf")))
        .map(|b| V::Str(hex_key(jsv::get(Some(b), "record"))))
        .collect();
    let mut o = Obj::new();
    o.set("version", jsv::str_or_empty(db.get("version")));
    if let Some(m) = jsv::fin(db.get("maintenance")) {
        o.set("maintenance", m);
    }
    o.set("objects", V::Arr(objects));
    o.set("classes", V::Arr(classes));
    o.set("records", V::Arr(records));
    V::Obj(o)
}

// ------------------------------------------------------------------------------------------------ saving (dynSave)

/// `parseInt(h, 16)` (NaN when no hex digits).
fn parse_hex(h: &str) -> f64 {
    let t = cc_cad::js::trim_start(h);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let mut v = 0.0f64;
    let mut any = false;
    for c in t.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = v * 16.0 + f64::from(d);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}
/// `(n).toString(16).toUpperCase()` for a non-negative integer double.
fn hex_of(n: f64) -> String {
    if !(n.is_finite() && n >= 0.0) {
        return cc_cad::js::num_to_string(n).to_uppercase();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = n.trunc() as u128;
    format!("{u:X}")
}

/// `dynEntities(b)`: a dynamic block's file entities as they are now ([[entities]…] per file entity), or None when the
/// block no longer lines up.
pub fn dyn_entities(b: &cc_cad::Block) -> Option<Vec<Vec<serde_json::Value>>> {
    let d = b.dyn_.as_ref()?;
    let all = d.get("all")?.as_array()?;
    let shown = d.get("shown")?.as_array()?;
    if all.len() != shown.len() {
        return None;
    }
    let ad = d.get("ad").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
    if b.attdefs.as_ref().map_or(0, Vec::len) != ad {
        return None;
    }
    let have: Vec<serde_json::Value> = b.entities.iter().map(cc_cad::Entity::to_value).collect();
    let mut k = 0usize;
    let mut out = Vec::with_capacity(all.len());
    for (i, parts) in all.iter().enumerate() {
        let parts = parts.as_array().cloned().unwrap_or_default();
        if !cc_cad::js::truthy(shown.get(i)) {
            out.push(parts);
            continue;
        }
        let end = k.saturating_add(parts.len()).min(have.len());
        let now: Vec<serde_json::Value> = have.get(k..end).map(<[serde_json::Value]>::to_vec).unwrap_or_default();
        k = k.saturating_add(parts.len());
        let same = now.len() == parts.len()
            && now.iter().zip(parts.iter()).all(|(x, p)| x.is_object() && x.get("type").is_some() && x.get("type") == p.get("type"));
        if !same {
            return None;
        }
        out.push(now);
    }
    (k == have.len()).then_some(out)
}

/// What dwg-save.js needs to keep dynamic blocks dynamic (`dynSave(doc)`).
pub struct DynSave {
    /// block name → dyn (with `all` as it is now), in block order
    pub blocks: Vec<(String, Obj)>,
    pub classes: Vec<V>,
    pub version: String,
    pub maintenance: Option<f64>,
    pub dropped: f64,
    src_objects: Vec<V>,
    src_objs: HashMap<String, V>,
    replaced: Vec<(String, V)>,
    extra: Vec<V>,
    used: HashSet<String>,
    rec_for: Vec<(String, String)>,
    fake: f64,
    ver_num: f64,
    rep_class: Option<Obj>,
    active: bool,
}

impl DynSave {
    pub fn get(&self, name: &str) -> Option<&Obj> {
        self.blocks.iter().find(|(n, _)| n == name).map(|(_, d)| d)
    }
    /// `record(name)`: the record handle to write a block under.
    pub fn record(&self, name: &str) -> Option<String> {
        self.rec_for.iter().find(|(n, _)| n == name).map(|(_, h)| h.clone()).filter(|h| !h.is_empty())
    }
    fn new_h(&mut self) -> String {
        let h = hex_of(self.fake);
        self.fake += 1.0;
        h
    }
    fn replace(&mut self, k: String, v: V) {
        match self.replaced.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => self.replaced.push((k, v)),
        }
    }
    /// `objects`: the source objects (replaced ones swapped in) + what was made.
    pub fn objects(&self) -> Vec<V> {
        let mut out: Vec<V> = self
            .src_objects
            .iter()
            .map(|o| {
                let k = hex_key(jsv::get(Some(o), "handle"));
                self.replaced.iter().find(|(x, _)| *x == k).map_or_else(|| o.clone(), |(_, v)| v.clone())
            })
            .collect();
        out.extend(self.extra.iter().cloned());
        out
    }

    fn clone_tree(&mut self, root: &V, owner: &str, state: Option<&V>, vis_node: Option<&V>) -> Option<V> {
        let mut map: Vec<(String, String)> = Vec::new();
        let mut stack: Vec<(V, u32)> = vec![(root.clone(), 0)];
        // depth-first, as the JS recursion visits (pre-order; children in Object.values order)
        fn walk(s: &mut DynSave, h: &V, map: &mut Vec<(String, String)>, depth: u32) {
            if depth > 10_000 {
                return;
            }
            let Some(o) = s.src_objs.get(&hex_key(Some(h))).cloned() else { return };
            let raw = jsv::to_str(jsv::get(Some(&o), "handle"));
            if map.iter().any(|(k, _)| *k == raw) {
                return;
            }
            let nh = s.new_h();
            let hk = hex_key(jsv::get(Some(&o), "handle"));
            if map.iter().any(|(k, _)| *k == hk) {
                // the JS keys by hexKey but asks by the raw handle: a second visit re-maps it (kept as the JS does)
                if let Some(slot) = map.iter_mut().find(|(k, _)| *k == hk) {
                    slot.1 = nh;
                }
            } else {
                map.push((hk, nh));
            }
            if let Some(V::Obj(en)) = jsv::get(Some(&o), "entries") {
                for k in jsv::keys_js(en) {
                    if let Some(v) = en.get(k) {
                        walk(s, v, map, depth.saturating_add(1));
                    }
                }
            }
        }
        stack.clear();
        walk(self, root, &mut map, 0);
        let root_k = hex_key(Some(root));
        let lookup = |map: &Vec<(String, String)>, k: &str| map.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        for (old, nh) in map.clone() {
            let Some(o) = self.src_objs.get(&old).cloned() else { continue };
            let ow = if old == root_k {
                owner.to_string()
            } else {
                lookup(&map, &hex_key(jsv::get(Some(&o), "owner"))).unwrap_or_else(|| owner.to_string())
            };
            if let Some(en) = jsv::get(Some(&o), "entries").filter(|e| e.truthy()) {
                let mut entries = Obj::new();
                if let V::Obj(eo) = en {
                    for k in jsv::keys_js(eo) {
                        let v = eo.get(k).cloned().unwrap_or(V::Null);
                        let mapped = lookup(&map, &hex_key(Some(&v))).map_or(v, V::Str);
                        entries.set(k.to_string(), mapped);
                    }
                }
                self.extra.push(dict_obj(&nh, &ow, entries));
            } else if jsv::s(jsv::get(Some(&o), "type")) == Some("XRECORD") {
                let parent = self.src_objs.get(&hex_key(jsv::get(Some(&o), "owner")));
                let key = parent.and_then(|p| match jsv::get(Some(p), "entries") {
                    Some(V::Obj(pe)) => jsv::keys_js(pe).into_iter().find(|k| hex_key(pe.get(k)) == old).map(str::to_string),
                    _ => None,
                });
                let data = jsv::arr(jsv::get(Some(&o), "data"));
                let d = match state {
                    Some(st) if key.as_deref() == Some(jsv::to_str(vis_node).as_str()) => state_data(data, st),
                    _ => data.to_vec(),
                };
                self.extra.push(xrec_obj(&nh, &ow, d));
            } else {
                let mut c = jsv::obj(Some(&o)).cloned().unwrap_or_default();
                c.set("handle", nh.clone());
                c.set("owner", ow.clone());
                c.set("reactors", V::Arr(vec![V::Str(ow)]));
                self.extra.push(V::Obj(c));
            }
        }
        lookup(&map, &root_k).map(V::Str)
    }

    /// `ref(insert)`: a dynamic reference's handle and extension dictionary (adding what it needs), or None.
    pub fn reference(&mut self, block: &str, d: &V, handle_src: Option<&V>) -> Option<(String, Option<V>)> {
        if !self.active {
            return None;
        }
        let _ = handle_src;
        let blk = self.get(block)?.clone();
        let def = if jsv::truthy(blk.get("def")) { blk.clone() } else { self.get(&jsv::to_str(blk.get("of")))?.clone() };
        let vis = def.get("vis").filter(|v| v.truthy()).cloned();
        let v = vis.as_ref().and_then(|vis| {
            jsv::arr(jsv::get(Some(d), "values"))
                .iter()
                .find(|x| crate::props::strict_eq(jsv::get(Some(x), "node"), jsv::get(Some(vis), "node")))
                .cloned()
        });
        let state: Option<V> = match v.as_ref().and_then(|v| jsv::get(Some(v), "state")) {
            Some(V::Str(s)) => Some(V::Str(s.clone())),
            _ => jsv::get(Some(d), "state").cloned(),
        };
        let src = jsv::get(Some(d), "src").filter(|s| s.truthy()).map(|s| jsv::to_str(Some(s)));
        let first = src.as_ref().is_some_and(|s| !self.used.contains(s));
        let handle = if first { src.clone().unwrap_or_default() } else { self.new_h() };
        self.used.insert(handle.clone());
        let dx = jsv::get(Some(d), "x").filter(|x| x.truthy()).cloned();
        if let (Some(x), true) = (&dx, first) {
            let xo = v
                .as_ref()
                .and_then(|v| jsv::get(Some(v), "xrecord"))
                .filter(|x| x.truthy())
                .and_then(|h| self.src_objs.get(&hex_key(Some(h))).cloned());
            if let (Some(xo), Some(V::Str(st))) = (xo, state.as_ref()) {
                let data = jsv::arr(jsv::get(Some(&xo), "data"));
                if data.iter().any(|g| {
                    jsv::num(jsv::at(Some(g), 0)) == Some(1.0) && !crate::props::strict_eq(jsv::at(Some(g), 1), Some(&V::Str(st.clone())))
                }) {
                    let rec = xrec_obj(
                        &jsv::to_str(jsv::get(Some(&xo), "handle")),
                        &jsv::to_str(jsv::get(Some(&xo), "owner")),
                        state_data(data, &V::Str(st.clone())),
                    );
                    let rec = match rec {
                        V::Obj(mut o) => {
                            o.set_opt("handle", jsv::get(Some(&xo), "handle").cloned());
                            o.set_opt("owner", jsv::get(Some(&xo), "owner").cloned());
                            o.set("reactors", V::Arr(vec![jsv::get(Some(&xo), "owner").cloned().unwrap_or(V::Null)]));
                            V::Obj(o)
                        }
                        other => other,
                    };
                    self.replace(hex_key(jsv::get(Some(&xo), "handle")), rec);
                }
            }
            return Some((handle, Some(x.clone())));
        }
        if let Some(x) = dx {
            let node = vis.as_ref().and_then(|v| jsv::get(Some(v), "node")).cloned();
            let st = state.clone();
            let root = self.clone_tree(&x, &handle, st.as_ref(), node.as_ref());
            return Some((handle, root));
        }
        let rep = self.rep_class.clone()?;
        if jsv::truthy(blk.get("def")) || self.ver_num == 0.0 {
            return None;
        }
        let (xd, repd, rd, adc, ebd) = (self.new_h(), self.new_h(), self.new_h(), self.new_h(), self.new_h());
        self.extra.push(dict_obj(&xd, &handle, cc_io_dwg::obj! { "AcDbBlockRepresentation" => repd.clone() }));
        self.extra.push(dict_obj(&repd, &xd, cc_io_dwg::obj! { "AcDbRepData" => rd.clone(), "AppDataCache" => adc.clone() }));
        let ver = self.ver_num;
        let raw = cc_io_dwg::obj! {
            "ver" => ver,
            "n" => if ver >= 21.0 { 11.0 } else { 10.0 },
            "data" => "QEA=",
            "h" => V::Arr(vec![V::Arr(vec![V::Num(5.0), def.get("src").cloned().unwrap_or(V::Null)])])
        };
        let mut ro = Obj::new();
        ro.set("handle", rd.clone());
        ro.set("owner", repd.clone());
        ro.set("reactors", V::Arr(vec![V::Str(repd.clone())]));
        ro.set("type", "ACDB_BLOCKREPRESENTATION_DATA");
        ro.set("undecoded", true);
        ro.set_opt("typeNum", rep.get("num").cloned());
        ro.set_opt("className", rep.get("cppName").cloned());
        ro.set("raw", raw);
        self.extra.push(V::Obj(ro));
        self.extra.push(dict_obj(&adc, &repd, cc_io_dwg::obj! { "ACAD_ENHANCEDBLOCKDATA" => ebd.clone() }));
        let mut entries = Obj::new();
        if let (Some(vis), Some(V::Str(st))) = (vis.as_ref(), state.as_ref()) {
            let xr = self.new_h();
            entries.set(jsv::to_str(jsv::get(Some(vis), "node")), xr.clone());
            let pt = v
                .as_ref()
                .and_then(|v| jsv::get(Some(v), "point"))
                .filter(|p| p.truthy())
                .or_else(|| jsv::get(Some(vis), "point").filter(|p| p.truthy()))
                .cloned()
                .unwrap_or_else(|| jsv::p2(0.0, 0.0));
            let p3 = V::Obj(cc_io_dwg::obj! {
                "x" => jsv::get(Some(&pt), "x").cloned().unwrap_or(V::Null),
                "y" => jsv::get(Some(&pt), "y").cloned().unwrap_or(V::Null),
                "z" => 0.0
            });
            self.extra.push(xrec_obj(&xr, &ebd, vec![V::Arr(vec![V::Num(10.0), p3]), V::Arr(vec![V::Num(1.0), V::Str(st.clone())])]));
        }
        self.extra.push(dict_obj(&ebd, &adc, entries));
        Some((handle, Some(V::Str(xd))))
    }
}

fn dict_obj(handle: &str, owner: &str, entries: Obj) -> V {
    V::Obj(cc_io_dwg::obj! {
        "handle" => handle, "owner" => owner, "reactors" => V::Arr(vec![V::from(owner)]), "type" => "DICTIONARY",
        "entries" => entries, "hardOwner" => 1.0, "cloning" => 1.0
    })
}
fn xrec_obj(handle: &str, owner: &str, data: Vec<V>) -> V {
    V::Obj(cc_io_dwg::obj! {
        "handle" => handle, "owner" => owner, "reactors" => V::Arr(vec![V::from(owner)]), "type" => "XRECORD",
        "data" => V::Arr(data), "cloning" => 1.0
    })
}
/// `stateData(data, state)`: group 1 → the state.
fn state_data(data: &[V], state: &V) -> Vec<V> {
    data.iter()
        .map(|g| {
            if jsv::num(jsv::at(Some(g), 0)) == Some(1.0) {
                V::Arr(vec![V::Num(1.0), state.clone()])
            } else {
                match g {
                    V::Arr(a) => V::Arr(vec![a.first().cloned().unwrap_or(V::Null), a.get(1).cloned().unwrap_or(V::Null)]),
                    other => other.clone(),
                }
            }
        })
        .collect()
}

/// `editedVisibility(d, srcObjs)`: a definition's BLOCKVISIBILITYPARAMETER with its states as edited, or None.
fn edited_visibility(d: &Obj, src_objs: &HashMap<String, V>, order: &[V]) -> Option<V> {
    let vis = d.get("vis").filter(|v| v.truthy())?;
    let states = jsv::arr(jsv::get(Some(vis), "states"));
    let hs = match d.get("h") {
        Some(V::Arr(a)) => a,
        _ => return None,
    };
    if states.is_empty() || !matches!(jsv::get(Some(vis), "states"), Some(V::Arr(_))) {
        return None;
    }
    let param = jsv::get(Some(vis), "param").filter(|p| p.truthy());
    let mut o = param.and_then(|p| src_objs.get(&hex_key(Some(p))).cloned());
    if o.is_none() {
        let mine: HashSet<String> = hs.iter().filter(|h| h.truthy()).map(|h| jsv::to_str(Some(h))).collect();
        o = order
            .iter()
            .find(|x| {
                if jsv::s(jsv::get(Some(x), "type")) != Some("BLOCKVISIBILITYPARAMETER") {
                    return false;
                }
                let Some(raw) = jsv::obj(jsv::get(Some(x), "raw")) else { return false };
                match cc_io_dwg::w_dynblock::visibility_fields(raw) {
                    Ok(f) => {
                        let all: Vec<String> = f.all.iter().map(|(_, h)| hex_key(Some(&V::Str(h.clone())))).collect();
                        all.len() == mine.len() && all.iter().all(|h| mine.contains(h))
                    }
                    Err(_) => false,
                }
            })
            .cloned();
    }
    let o = o?;
    if jsv::s(jsv::get(Some(&o), "type")) != Some("BLOCKVISIBILITYPARAMETER") {
        return None;
    }
    let raw = jsv::obj(jsv::get(Some(&o), "raw"))?;
    let st: Vec<V> = states
        .iter()
        .map(|s| {
            let name = jsv::get(Some(s), "name").cloned().unwrap_or(V::Null);
            let from = jsv::get(Some(s), "src").filter(|x| !matches!(x, V::Null)).cloned().unwrap_or_else(|| name.clone());
            let ents: Vec<V> = jsv::arr(jsv::get(Some(s), "show"))
                .iter()
                .filter_map(|i| jsv::num(Some(i)).and_then(|i| hs.get(jsv::f2us(i))).filter(|h| h.truthy()).cloned())
                .collect();
            V::Obj(cc_io_dwg::obj! { "name" => name, "from" => from, "entities" => V::Arr(ents) })
        })
        .collect();
    let new_raw = cc_io_dwg::w_dynblock::rewrite_visibility(raw, &V::Arr(st)).ok()?;
    let mut c = jsv::obj(Some(&o)).cloned().unwrap_or_default();
    c.set("raw", new_raw);
    Some(V::Obj(c))
}

/// `dynSave(doc)`
pub fn dyn_save(doc: &cc_cad::Doc) -> DynSave {
    let src = doc.dyn_src.as_ref().map(jsv::from_json);
    let mut out = DynSave {
        blocks: Vec::new(),
        classes: Vec::new(),
        version: jsv::str_or_empty(jsv::get(src.as_ref(), "version")),
        maintenance: None,
        dropped: 0.0,
        src_objects: Vec::new(),
        src_objs: HashMap::new(),
        replaced: Vec::new(),
        extra: Vec::new(),
        used: HashSet::new(),
        rec_for: Vec::new(),
        fake: 0.0,
        ver_num: 0.0,
        rep_class: None,
        active: false,
    };
    let Some(src) = src else { return out };
    let Some(V::Arr(objects)) = jsv::get(Some(&src), "objects").cloned() else { return out };
    out.active = true;
    let own: Vec<V> = jsv::arr(jsv::get(Some(&src), "records")).to_vec();
    let has_own = |x: Option<&V>| x.is_some_and(|x| own.iter().any(|o| crate::props::strict_eq(Some(o), Some(x))));
    for o in &objects {
        out.src_objs.insert(hex_key(jsv::get(Some(o), "handle")), o.clone());
    }
    out.src_objects = objects.clone();
    for b in doc.blocks.values() {
        let Some(dv) = b.dyn_.as_ref() else { continue };
        let d = match jsv::from_json(dv) {
            V::Obj(o) => o,
            _ => continue,
        };
        let mine = if jsv::truthy(d.get("made")) {
            let of = jsv::to_str(d.get("of"));
            let src_of = doc.blocks.get(&of).and_then(|x| x.dyn_.as_ref()).and_then(|x| x.get("src")).map(jsv::from_json);
            has_own(src_of.as_ref())
        } else {
            has_own(d.get("src"))
        };
        if !mine {
            continue;
        }
        let all = dyn_entities(b);
        let is_def = jsv::truthy(d.get("def"));
        let edited = jsv::truthy(d.get("visEdited"));
        let vp = if all.is_some() && is_def && edited { edited_visibility(&d, &out.src_objs, &objects) } else { None };
        if let Some(vp) = &vp {
            let k = hex_key(jsv::get(Some(vp), "handle"));
            out.replace(k, vp.clone());
        }
        match all {
            Some(all) if !is_def || !edited || vp.is_some() => {
                let mut dd = d.clone();
                dd.set("all", V::Arr(all.iter().map(|p| V::Arr(p.iter().map(jsv::from_json).collect())).collect()));
                out.blocks.push((b.name.clone(), dd));
            }
            _ => {
                if is_def {
                    out.dropped += 1.0;
                }
            }
        }
    }
    let defs: Vec<(String, bool)> = out.blocks.iter().map(|(n, d)| (n.clone(), jsv::truthy(d.get("def")))).collect();
    out.blocks.retain(|(_, d)| {
        jsv::truthy(d.get("def")) || {
            let of = d.get("of").filter(|o| o.truthy()).map(|o| jsv::to_str(Some(o)));
            of.is_some_and(|of| defs.iter().any(|(n, def)| *n == of && *def))
        }
    });
    out.classes = jsv::arr(jsv::get(Some(&src), "classes")).to_vec();
    out.maintenance = jsv::fin(jsv::get(Some(&src), "maintenance"));
    let mut top = 0.0f64;
    let mut seen = |h: Ov| {
        let n = parse_hex(&jsv::to_str(h));
        if n.is_finite() && n > top {
            top = n;
        }
    };
    for o in &objects {
        seen(jsv::get(Some(o), "handle"));
    }
    for r in &own {
        seen(Some(r));
    }
    for (_, d) in &out.blocks {
        for h in jsv::arr(d.get("h")) {
            seen(Some(h));
        }
    }
    out.fake = top + 65536.0;
    let names: Vec<(String, bool)> = out.blocks.iter().map(|(n, d)| (n.clone(), jsv::truthy(d.get("made")))).collect();
    for (name, made) in names {
        let r = if made { out.new_h() } else { out.get(&name).map(|d| jsv::to_str(d.get("src"))).unwrap_or_default() };
        out.rec_for.push((name, r));
    }
    let made: Vec<(String, V)> = out
        .blocks
        .iter()
        .filter(|(_, d)| jsv::truthy(d.get("made")))
        .map(|(n, d)| {
            let of = jsv::to_str(d.get("of"));
            let src = out.get(&of).and_then(|x| x.get("src")).cloned().unwrap_or(V::Null);
            (n.clone(), src)
        })
        .collect();
    for (name, src) in made {
        if let Some((_, d)) = out.blocks.iter_mut().find(|(n, _)| *n == name) {
            d.set("xd", cc_io_dwg::obj! { "AcDbBlockRepBTag" => V::Arr(vec![V::Arr(vec![V::Num(1070.0), V::Num(1.0)]), V::Arr(vec![V::Num(1005.0), src])]) });
        }
    }
    out.ver_num = match out.version.as_str() {
        "AC1012" => 13.0,
        "AC1014" => 14.0,
        "AC1015" => 15.0,
        "AC1018" => 18.0,
        "AC1021" => 21.0,
        "AC1024" => 24.0,
        "AC1027" => 27.0,
        "AC1032" => 32.0,
        _ => 0.0,
    };
    out.rep_class = out
        .classes
        .iter()
        .find(|c| jsv::s(jsv::get(Some(c), "dxfName")) == Some("ACDB_BLOCKREPRESENTATION_DATA"))
        .and_then(|c| jsv::obj(Some(c)).cloned());
    out
}
