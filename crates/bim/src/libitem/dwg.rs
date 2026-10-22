//! libitem.js DWG / DXF blocks → items, on the drawing model cc-io-dwg reads (lib/dwg's `readDwg` /
//! dxf-model.js, field for field): the block candidates (KCL item tags naming a block, a dynamic block's
//! visibility states as their own candidates), units, nested inserts exploded with layer 0 / by-block colours
//! inherited, plan entities in mm, params and source attributes from the attributes, and a body (the block's 3D
//! faces, else a box from the plan with the attributes' height, else none).

use std::collections::{HashMap, HashSet};

use cc_io_dwg::value::{Obj, V};
use serde_json::{Map, Value as J};

use super::{
    Attrs, LIMIT_ENTITIES, LIMIT_TEXT, LIMIT_TRIANGLES, PLACEMENT_ATTRS, attrs_info, base_item, box_from_plan, clean, clip, clip500,
    convex_hull, insunits, msg, n, params_from_attrs, plan_bounds, poly_points, pts_json, r4, s, source_attrs, suggest_category, unit_mm,
};
use crate::draw::m;
use crate::js::{len16, num_str, slice16};

const D2R: f64 = std::f64::consts::PI / 180.0;

fn vn(v: Option<&V>) -> f64 {
    match v {
        Some(V::Num(x)) => *x,
        Some(V::Bool(b)) => f64::from(u8::from(*b)),
        Some(V::Str(s)) => crate::js::str_num(s),
        Some(V::Null) => 0.0,
        _ => f64::NAN,
    }
}
/// `x & n` of a model number (NaN and infinities as 0)
#[allow(clippy::cast_possible_truncation)]
fn bits(x: f64) -> i64 {
    if x.is_finite() { x as i64 } else { 0 }
}
/// `String(v)` of a model value
fn vs(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(V::Null) => "null".into(),
        Some(V::Str(s)) => s.clone(),
        Some(V::Num(x)) => num_str(*x),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::Arr(a)) => a.iter().map(|x| if matches!(x, V::Null) { String::new() } else { vs(Some(x)) }).collect::<Vec<_>>().join(","),
        Some(V::Obj(_)) => "[object Object]".into(),
        Some(V::Bytes(_)) => String::new(),
    }
}
/// `String(v ?? '')`
fn vso(v: Option<&V>) -> String {
    match v {
        None | Some(V::Null) => String::new(),
        x => vs(x),
    }
}
fn truthy(v: Option<&V>) -> bool {
    v.is_some_and(V::truthy)
}
/// `a || b` for numbers: a when truthy (non-zero, not NaN)
fn or(a: f64, b: f64) -> f64 {
    if a != 0.0 && !a.is_nan() { a } else { b }
}
fn arr<'a>(o: &'a Obj, k: &str) -> &'a [V] {
    o.arr(k).map_or(&[], Vec::as_slice)
}
fn objs(v: &[V]) -> impl Iterator<Item = &Obj> {
    v.iter().filter_map(V::as_obj)
}
fn pt(o: &Obj, k: &str) -> (f64, f64, f64) {
    match o.obj(k) {
        Some(p) => (vn(p.get("x")), vn(p.get("y")), vn(p.get("z"))),
        None => (f64::NAN, f64::NAN, f64::NAN),
    }
}
/// `attText(t)`
fn att_text(t: &Obj) -> String {
    clean(&vso(t.get("value")))
}

/// One block a drawing offers as an item.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub key: String,
    pub name: String,
    /// a visibility state's block
    pub block: Option<String>,
    pub state: Option<String>,
    pub entities: usize,
    pub attdefs: Vec<String>,
    pub attrs: Attrs,
    pub attrs_from: Option<&'static str>,
    pub tag: bool,
    pub has_body: bool,
}

fn set_attr(a: &mut Attrs, k: String, v: String) {
    match a.iter_mut().find(|(x, _)| *x == k) {
        Some(slot) => slot.1 = v,
        None => a.push((k, v)),
    }
}

/// `itemBlockName(b, all)`: its own name, or an anonymous dynamic block's true name when no block has that name.
fn item_block_name(b: &Obj, all: &[V]) -> String {
    let name = vso(b.get("name"));
    let tn = b.str("trueName").unwrap_or("");
    if name.is_empty() || !name.starts_with('*') || tn.is_empty() || tn.starts_with('*') {
        return name;
    }
    if objs(all).any(|x| x.str("name") == Some(tn)) { name } else { tn.to_string() }
}

/// A dynamic block's visibility states with their entity handles.
fn visibility_states(b: &Obj) -> Option<Vec<(String, HashSet<String>)>> {
    let params = b.obj("dynamic").and_then(|d| d.arr("params"))?;
    let p = objs(params).find(|x| x.arr("states").is_some_and(|s| s.len() > 1))?;
    Some(
        objs(arr(p, "states"))
            .map(|st| {
                let nm = clean(&vso(st.get("name")));
                let nm = if nm.is_empty() { "?".to_string() } else { nm };
                (nm, arr(st, "entities").iter().map(|h| vs(Some(h))).collect())
            })
            .collect(),
    )
}

fn key_part(s: &str) -> String {
    // .toLowerCase().replace(/[^a-z0-9_-]+/g, '-')
    let mut out = String::new();
    let mut run = false;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
            out.push(c);
            run = false;
        } else if !run {
            out.push('-');
            run = true;
        }
    }
    out
}
fn trim_dashes(s: &str) -> &str {
    s.trim_matches('-')
}

/// `blockCandidates(db)`
pub fn block_candidates(db: &Obj) -> Vec<Candidate> {
    let records = arr(db, "blocks");
    let mut first_attrs: HashMap<String, Attrs> = HashMap::new();
    let mut tag_for: HashMap<String, Attrs> = HashMap::new();
    for sp in objs(records) {
        let nm = vso(sp.get("name"));
        let low = nm.to_lowercase();
        if !(low.starts_with("*model_space") || low.starts_with("*paper_space")) {
            continue;
        }
        let in_model = low == "*model_space";
        for e in objs(arr(sp, "entities")) {
            let ty = e.str("type").unwrap_or("");
            if (ty != "INSERT" && ty != "MINSERT") || arr(e, "attribs").is_empty() {
                continue;
            }
            let mut a: Attrs = Vec::new();
            for t in objs(arr(e, "attribs")) {
                let tag = vso(t.get("tag"));
                if !tag.is_empty() {
                    set_attr(&mut a, tag, att_text(t));
                }
            }
            let blk = vso(e.get("block"));
            if in_model && !first_attrs.contains_key(&blk) {
                first_attrs.insert(blk, a.clone());
            }
            let fname = a.iter().find(|(k, _)| k == "KCL-FILENAME").map(|(_, v)| v.clone()).unwrap_or_default();
            if !fname.is_empty() && !tag_for.contains_key(&fname.to_uppercase()) {
                tag_for.insert(fname.to_uppercase(), a);
            }
        }
    }
    let mut keys: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for b in objs(records) {
        let name = item_block_name(b, records);
        let flags = vn(b.get("flags"));
        let fl = bits(flags);
        if name.is_empty() || name.starts_with('*') || fl & 4 != 0 || fl & 16 != 0 || name.contains('|') {
            continue;
        }
        let ents: Vec<&Obj> = objs(arr(b, "entities")).collect();
        let defs: Vec<&Obj> = ents.iter().copied().filter(|e| e.str("type") == Some("ATTDEF")).collect();
        let drawn: Vec<&Obj> = ents.iter().copied().filter(|e| e.str("type") != Some("ATTDEF")).collect();
        let mut attrs: Attrs = Vec::new();
        let mut from = None;
        let is_tag = defs.iter().any(|d| PLACEMENT_ATTRS.contains(&vso(d.get("tag")).to_uppercase().as_str()));
        let bname = vso(b.get("name"));
        if is_tag {
            from = Some("item tag block");
        } else if let Some(a) = first_attrs.get(&bname) {
            attrs = a.clone();
            from = Some("instance");
        } else if !defs.is_empty() {
            for d in &defs {
                let tag = vso(d.get("tag"));
                if !tag.is_empty() {
                    set_attr(&mut attrs, tag, att_text(d));
                }
            }
            from = Some("defaults");
        }
        if let Some(tag) = tag_for.get(&name.to_uppercase()) {
            let mut merged = tag.clone();
            for (k, v) in attrs.iter().filter(|(_, v)| !v.is_empty()) {
                set_attr(&mut merged, k.clone(), v.clone());
            }
            attrs = merged;
            from = Some("item tag");
        }
        let has3d = drawn.iter().any(|e| e.str("type") == Some("3DFACE"));
        let key0 = slice16(trim_dashes(&key_part(&name)), 60);
        let key = if key0.is_empty() { "block".to_string() } else { key0 };
        let mut k = key.clone();
        let mut nn = 2u32;
        while keys.contains(&k) {
            k = format!("{key}-{nn}");
            nn = nn.saturating_add(1);
        }
        keys.insert(k.clone());
        let visible: Attrs = attrs.iter().filter(|(t, _)| !PLACEMENT_ATTRS.contains(&t.to_uppercase().as_str())).cloned().collect();
        let p = params_from_attrs(&visible, 1.0);
        let attdefs: Vec<String> = defs.iter().map(|d| vso(d.get("tag"))).filter(|t| !t.is_empty()).collect();
        let has_body = has3d || p.contains_key("height");
        if let Some(states) = visibility_states(b) {
            for (st, handles) in &states {
                let nm = format!("{name}: {st}");
                let raw = format!("{key}-{}", key_part(st));
                let mut collapsed = String::new();
                for c in raw.chars() {
                    if !(c == '-' && collapsed.ends_with('-')) {
                        collapsed.push(c);
                    }
                }
                let sk0 = slice16(trim_dashes(&collapsed), 60);
                let sk = if sk0.is_empty() { key.clone() } else { sk0 };
                let mut s2 = sk.clone();
                let mut n2 = 2u32;
                while keys.contains(&s2) {
                    s2 = format!("{sk}-{n2}");
                    n2 = n2.saturating_add(1);
                }
                keys.insert(s2.clone());
                out.push(Candidate {
                    key: s2,
                    name: nm,
                    block: Some(name.clone()),
                    state: Some(st.clone()),
                    entities: drawn.iter().filter(|e| handles.contains(&vs(e.get("handle")))).count(),
                    attdefs: attdefs.clone(),
                    attrs: visible.clone(),
                    attrs_from: from,
                    tag: is_tag,
                    has_body,
                });
            }
            continue;
        }
        out.push(Candidate {
            key: k,
            name: name.clone(),
            block: None,
            state: None,
            entities: drawn.len(),
            attdefs,
            attrs: visible,
            attrs_from: from,
            tag: is_tag,
            has_body,
        });
    }
    out
}

/// `drawingUnits(db, override)` → (mm per unit, the note when the drawing does not say); Err = unknown units.
pub fn drawing_units(db: &Obj, over: Option<&str>) -> Result<(f64, Option<String>), String> {
    if let Some(o) = over {
        return match unit_mm(o) {
            Some(mm) => Ok((mm, None)),
            None => Err(msg("libitem.units.unknown", &[("units", o.to_string()), ("list", super::UNIT_KEYS.join(", "))])),
        };
    }
    let h = db.obj("header");
    let ins = h.and_then(|h| h.get("INSUNITS")).map_or(0.0, |v| vn(Some(v)));
    let key = insunits(ins);
    if let Some(mm) = unit_mm(key) {
        return Ok((mm, None));
    }
    let metric = h.and_then(|h| h.get("MEASUREMENT")).is_some_and(|v| vn(Some(v)) == 1.0);
    Ok(if metric { (1.0, Some(msg("libitem.units.assumed_mm", &[]))) } else { (25.4, Some(msg("libitem.units.assumed_in", &[]))) })
}

/// `itemsFromDwg(db, { units, blocks, fileName, list })` → (items, notes, candidates)
pub fn items_from_dwg(
    db: &Obj,
    units: Option<&str>,
    blocks: Option<&[String]>,
    file_name: &str,
    list: bool,
) -> Result<(super::Read, Vec<Candidate>), String> {
    let candidates = block_candidates(db);
    let mut out = super::Read::default();
    let (_, note) = drawing_units(db, units)?;
    if let Some(n) = note {
        out.notes.push(n);
    }
    if list {
        return Ok((out, candidates));
    }
    let want: Option<Vec<String>> = blocks.filter(|b| !b.is_empty()).map(|b| b.iter().map(|x| x.to_lowercase()).collect());
    for c in &candidates {
        if let Some(w) = &want
            && !w.contains(&c.name.to_lowercase())
            && !w.contains(&c.key)
        {
            continue;
        }
        if c.entities == 0 {
            out.notes.push(msg("libitem.block.empty", &[("name", c.name.clone())]));
            continue;
        }
        let (item, notes) = item_from_block(db, &c.name, units, file_name, Some(c))?;
        if let Some(mut it) = item {
            if let J::Object(o) = &mut it {
                o.insert("_key".into(), s(&c.key));
            }
            out.items.push(it);
        }
        for n in notes {
            out.notes.push(format!("{}: {n}", c.name));
        }
    }
    if let Some(w) = &want {
        let mut seen = HashSet::new();
        for x in w {
            if seen.insert(x.clone()) && !candidates.iter().any(|c| c.name.to_lowercase() == *x || c.key == *x) {
                out.notes.push(msg("libitem.block.not_in_drawing", &[("name", x.clone())]));
            }
        }
    }
    Ok((out, candidates))
}

type A = [f64; 6];
fn mul_a(p: &A, q: &A) -> A {
    [
        p[0] * q[0] + p[2] * q[1],
        p[1] * q[0] + p[3] * q[1],
        p[0] * q[2] + p[2] * q[3],
        p[1] * q[2] + p[3] * q[3],
        p[0] * q[4] + p[2] * q[5] + p[4],
        p[1] * q[4] + p[3] * q[5] + p[5],
    ]
}
fn rot_a(r: f64) -> A {
    let (c, s) = (m::cos(r), m::sin(r));
    [c, s, -s, c, 0.0, 0.0]
}
fn norm360(a: f64) -> f64 {
    let x = a % 360.0;
    if x < 0.0 { x + 360.0 } else { x }
}
fn sample_arc_rad(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let mut sweep = a1 - a0;
    if !sweep.is_finite() {
        return Vec::new();
    }
    let mut guard = 0u32;
    while sweep <= 0.0 && guard < 1_000_000 {
        sweep += 2.0 * std::f64::consts::PI;
        guard = guard.saturating_add(1);
    }
    let nn = m::max(8.0, (sweep / (5.0 * D2R)).ceil()).min(100_000.0);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= nn {
        let a = a0 + sweep * i / nn;
        out.push([cx + r * m::cos(a), cy + r * m::sin(a)]);
        i += 1.0;
    }
    out
}
fn spline_points(e: &Obj) -> Vec<[f64; 2]> {
    let fit = arr(e, "fit");
    if fit.len() > 1 {
        return objs(fit).map(|p| [vn(p.get("x")), vn(p.get("y"))]).collect();
    }
    let cp: Vec<&Obj> = objs(arr(e, "control")).collect();
    let k: Vec<f64> = arr(e, "knots").iter().map(|x| vn(Some(x))).collect();
    let deg = or(vn(e.get("degree")), 3.0);
    if cp.len() < 2 {
        return Vec::new();
    }
    if !((1.0..=32.0).contains(&deg) && deg.fract() == 0.0) || k.len() as f64 != cp.len() as f64 + deg + 1.0 {
        return cp.iter().map(|q| [vn(q.get("x")), vn(q.get("y"))]).collect();
    }
    let p = (1..=32usize).find(|k| *k as f64 == deg).unwrap_or(3);
    let wts: Vec<f64> = arr(e, "weights").iter().map(|x| vn(Some(x))).collect();
    let w = (wts.len() == cp.len()).then_some(wts);
    let kg = |i: usize| k.get(i).copied().unwrap_or(f64::NAN);
    let lo = kg(p);
    let hi = kg(k.len().saturating_sub(p).saturating_sub(1));
    let nn = cp.len().saturating_mul(8).clamp(16, 1_000_000);
    let mut pts = Vec::new();
    for s in 0..=nn {
        let u = if s == nn { hi - 1e-9 * (hi - lo) } else { lo + (hi - lo) * (s as f64 / nn as f64) };
        let mut span = p;
        while span < cp.len().saturating_sub(1) && u >= kg(span.saturating_add(1)) {
            span = span.saturating_add(1);
        }
        let mut d: Vec<[f64; 3]> = Vec::with_capacity(p.saturating_add(1));
        for j in 0..=p {
            let idx = span.saturating_add(j).saturating_sub(p);
            let q = cp.get(idx);
            let wt = w.as_ref().map_or(1.0, |w| w.get(idx).copied().unwrap_or(f64::NAN));
            d.push([q.map_or(f64::NAN, |q| vn(q.get("x"))) * wt, q.map_or(f64::NAN, |q| vn(q.get("y"))) * wt, wt]);
        }
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = span.saturating_add(j).saturating_sub(p);
                let den = kg(i.saturating_add(p).saturating_add(1).saturating_sub(r)) - kg(i);
                let a = if den == 0.0 { 0.0 } else { (u - kg(i)) / den };
                let (Some(dj1), Some(dj)) = (d.get(j.saturating_sub(1)).copied(), d.get(j).copied()) else { continue };
                if let Some(x) = d.get_mut(j) {
                    *x = [(1.0 - a) * dj1[0] + a * dj[0], (1.0 - a) * dj1[1] + a * dj[1], (1.0 - a) * dj1[2] + a * dj[2]];
                }
            }
        }
        let dp = d.get(p).copied().unwrap_or([f64::NAN; 3]);
        pts.push([dp[0] / dp[2], dp[1] / dp[2]]);
    }
    pts
}
fn hatch_loops(e: &Obj) -> Vec<Vec<f64>> {
    let mut loops = Vec::new();
    for lp in objs(arr(e, "loops")) {
        if let Some(pl) = lp.obj("polyline") {
            let vs3: Vec<[f64; 3]> =
                objs(arr(pl, "vertices")).map(|v| [vn(v.get("x")), vn(v.get("y")), or(vn(v.get("bulge")), 0.0)]).collect();
            loops.push(poly_points(&vs3, true).into_iter().flatten().collect());
            continue;
        }
        let mut pts = Vec::new();
        for ed in objs(arr(lp, "edges")) {
            match ed.str("type") {
                Some("line") => {
                    let (sx, sy, _) = pt(ed, "start");
                    let (ex, ey, _) = pt(ed, "end");
                    pts.extend([sx, sy, ex, ey]);
                }
                Some("arc") => {
                    let (s0, f) = (vn(ed.get("startAngle")), vn(ed.get("endAngle")));
                    let (cx, cy, _) = pt(ed, "center");
                    let r = vn(ed.get("radius"));
                    let arc = if ed.get("ccw") == Some(&V::Bool(false)) {
                        let mut a = sample_arc_rad(cx, cy, r, -f, -s0);
                        a.reverse();
                        a
                    } else {
                        sample_arc_rad(cx, cy, r, s0, f)
                    };
                    for [x, y] in arc {
                        pts.extend([x, y]);
                    }
                }
                _ => {}
            }
        }
        if pts.len() >= 6 {
            loops.push(pts);
        }
    }
    loops
}
/// `plainText(s)`: %%c %%d %%p %%% as their characters, %%u %%o out, \U+XXXX decoded.
fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or('\0');
        if c == '%' && b.get(i.saturating_add(1)) == Some(&'%') {
            match b.get(i.saturating_add(2)) {
                Some('c' | 'C') => {
                    out.push('ø');
                    i = i.saturating_add(3);
                    continue;
                }
                Some('d' | 'D') => {
                    out.push('°');
                    i = i.saturating_add(3);
                    continue;
                }
                Some('p' | 'P') => {
                    out.push('±');
                    i = i.saturating_add(3);
                    continue;
                }
                Some('%') => {
                    out.push('%');
                    i = i.saturating_add(3);
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
        i = i.saturating_add(1);
    }
    // %%u / %%o after the first pass (the JS replaces in two steps)
    let out2 = {
        let mut o = String::new();
        let c: Vec<char> = out.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            if c.get(i) == Some(&'%')
                && c.get(i.saturating_add(1)) == Some(&'%')
                && matches!(c.get(i.saturating_add(2)), Some('u' | 'U' | 'o' | 'O'))
            {
                i = i.saturating_add(3);
                continue;
            }
            if let Some(&x) = c.get(i) {
                o.push(x);
            }
            i = i.saturating_add(1);
        }
        o
    };
    // \U+XXXX → String.fromCharCode (a lone surrogate becomes U+FFFD here)
    let c: Vec<char> = out2.chars().collect();
    let mut o = String::new();
    let mut i = 0usize;
    while i < c.len() {
        if c.get(i) == Some(&'\\') && c.get(i.saturating_add(1)) == Some(&'U') && c.get(i.saturating_add(2)) == Some(&'+') {
            let hex: String = c.iter().skip(i.saturating_add(3)).take(4).collect();
            if hex.len() == 4 && hex.chars().all(|x| x.is_ascii_hexdigit()) {
                let v = u32::from_str_radix(&hex, 16).unwrap_or(0xfffd);
                o.push(char::from_u32(v).unwrap_or('\u{fffd}'));
                i = i.saturating_add(7);
                continue;
            }
        }
        if let Some(&x) = c.get(i) {
            o.push(x);
        }
        i = i.saturating_add(1);
    }
    o
}
fn mtext_lines(s: &str) -> Vec<String> {
    static RX: std::sync::OnceLock<Option<[regex::Regex; 3]>> = std::sync::OnceLock::new();
    let t = plain_text(s).replace("\\P", "\n").replace("\\~", " ");
    let Some(rx) = RX.get_or_init(|| {
        Some([
            regex::Regex::new(r"\\S([^;]*?)[#^/]([^;]*?);").ok()?,
            regex::Regex::new(r"\\[ACcFfHhQqTtWwp][^;\\]*;").ok()?,
            regex::Regex::new(r"\\[LlOoKkNn]").ok()?,
        ])
    }) else {
        return t.split('\n').map(str::to_string).collect();
    };
    let t = rx[0].replace_all(&t, "$1/$2");
    let t = rx[1].replace_all(&t, "");
    let t = rx[2].replace_all(&t, "");
    let t: String = t.chars().filter(|c| *c != '{' && *c != '}').collect();
    t.replace("\\\\", "\\").split('\n').map(str::to_string).collect()
}

struct Ctx {
    m: A,
    zs: f64,
    zo: f64,
    layer: Option<String>,
    color: Option<J>,
    depth: u32,
}

struct Emit<'a> {
    blocks: &'a HashMap<String, &'a Obj>,
    used_layers: Vec<String>,
    entities: Vec<J>,
    faces: Vec<[f64; 3]>,
    skipped: Vec<(String, usize)>,
}
fn pa(m: &A, x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}
fn lin(m: &A, x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y, m[1] * x + m[3] * y]
}
fn det(m: &A) -> f64 {
    m[0] * m[3] - m[1] * m[2]
}
fn similar(m: &A) -> bool {
    (m::hypot(m[0], m[1]) - m::hypot(m[2], m[3])).abs() < 1e-9 * m::hypot(m[0], m[1])
        && (m[0] * m[2] + m[1] * m[3]).abs() < 1e-9 * (m[0] * m[0] + m[1] * m[1])
}
fn ang(x: f64, y: f64) -> f64 {
    let mut a = m::atan2(y, x) / D2R;
    if a < 0.0 {
        a += 360.0;
    }
    r4(a)
}
fn hex_rgb(rgb: f64) -> String {
    let v = if rgb.is_finite() { bits(rgb) & 0xff_ffff } else { 0 };
    format!("#{v:06x}")
}
fn r4p(p: [f64; 2]) -> J {
    J::Array(vec![n(r4(p[0])), n(r4(p[1]))])
}

impl Emit<'_> {
    fn skip(&mut self, t: String) {
        match self.skipped.iter_mut().find(|(k, _)| *k == t) {
            Some(s) => s.1 = s.1.saturating_add(1),
            None => self.skipped.push((t, 1)),
        }
    }
    fn push(&mut self, layer: &str, mut ent: Map<String, J>) {
        if self.entities.len() >= LIMIT_ENTITIES {
            self.skip(msg("libitem.block.skip.limit", &[]));
            return;
        }
        if !self.used_layers.iter().any(|l| l == layer) {
            self.used_layers.push(layer.to_string());
        }
        ent.shift_insert(1, "layer".into(), s(layer));
        self.entities.push(J::Object(ent));
    }
    fn ent(ty: &str, color: &Option<J>) -> Map<String, J> {
        let mut o = Map::new();
        o.insert("type".into(), s(ty));
        o.insert("color".into(), color.clone().unwrap_or(J::Null));
        o
    }
    fn poly_from(&mut self, pts: Vec<J>, closed: bool, layer: &str, color: &Option<J>) {
        if pts.len() < 2 {
            return;
        }
        let mut o = Self::ent("poly", color);
        o.insert("pts".into(), J::Array(pts));
        o.insert("closed".into(), J::Bool(closed));
        self.push(layer, o);
    }
    fn poly_xy(&mut self, pts: &[[f64; 2]], closed: bool, layer: &str, color: &Option<J>) {
        self.poly_from(pts.iter().map(|p| r4p(*p)).collect(), closed, layer, color);
    }
    #[allow(clippy::too_many_arguments)]
    fn text_ent(
        &mut self,
        t: &Obj,
        m: &A,
        layer: &str,
        color: &Option<J>,
        value: Option<&str>,
        at: Option<(f64, f64)>,
        rot_over: Option<f64>,
        h_over: Option<f64>,
    ) {
        let raw = value.map_or_else(|| vso(t.get("value")), str::to_string);
        let value = clip500(&plain_text(&raw));
        let h = h_over.unwrap_or_else(|| vn(t.get("height")));
        if value.is_empty() || !(h > 0.0) {
            return;
        }
        let sy = m::hypot(m[2], m[3]);
        let (hal, val) = if h_over.is_some() { (0.0, 0.0) } else { (or(vn(t.get("hAlign")), 0.0), or(vn(t.get("vAlign")), 0.0)) };
        let aligned = hal != 0.0 || val != 0.0;
        let (px, py) = match at {
            Some(p) => p,
            None => {
                let al = t.obj("align");
                if aligned && al.is_some_and(|a| truthy(a.get("x")) || truthy(a.get("y"))) {
                    let (x, y, _) = pt(t, "align");
                    (x, y)
                } else if t.obj("insert").is_some() {
                    let (x, y, _) = pt(t, "insert");
                    (x, y)
                } else {
                    (0.0, 0.0)
                }
            }
        };
        let wf = if h_over.is_some() { 1.0 } else { or(vn(t.get("widthFactor")), 1.0) };
        let w = len16(&value) as f64 * h * 0.6 * wf;
        let rot = rot_over.unwrap_or_else(|| or(vn(t.get("rotation")), 0.0));
        let ha = match bits(hal) {
            1 | 4 => 1.0,
            2 => 2.0,
            _ => 0.0,
        };
        let va = if hal == 4.0 { 2.0 } else { val };
        let dx = if ha == 1.0 {
            -w / 2.0
        } else if ha == 2.0 {
            -w
        } else {
            0.0
        };
        let dy = if va == 2.0 {
            -h / 2.0
        } else if va == 3.0 {
            -h
        } else {
            0.0
        };
        let lx = px + dx * m::cos(rot) - dy * m::sin(rot);
        let ly = py + dx * m::sin(rot) + dy * m::cos(rot);
        let [x, y] = pa(m, lx, ly);
        let [ux, uy] = lin(m, m::cos(rot), m::sin(rot));
        let mut o = Self::ent("text", color);
        o.insert("x".into(), n(r4(x)));
        o.insert("y".into(), n(r4(y)));
        o.insert("h".into(), n(r4(h * sy)));
        o.insert("rot".into(), n(ang(ux, uy)));
        o.insert("value".into(), s(&value));
        self.push(layer, o);
    }

    fn emit(&mut self, e: &Obj, ctx: &Ctx, invisible_off: bool) {
        if !invisible_off && truthy(e.get("invisible")) {
            return;
        }
        let l0 = vso(e.get("layer"));
        let l = if l0.is_empty() { "0".to_string() } else { l0 };
        let layer = match (&ctx.layer, l.as_str()) {
            (Some(cl), "0") => cl.clone(),
            _ => l,
        };
        let color = {
            let c = e.obj("color");
            match c {
                Some(c) if c.get("rgb").is_some() => Some(s(&hex_rgb(vn(c.get("rgb"))))),
                _ => {
                    let idx = c.and_then(|c| c.get("index")).map_or(256.0, |v| vn(Some(v)));
                    if idx == 0.0 {
                        Some(ctx.color.clone().unwrap_or(J::from(0)))
                    } else if c.is_some_and(|c| c.get("index").is_none()) || idx == 256.0 || idx.is_nan() {
                        None
                    } else {
                        Some(n(m::max(1.0, m::min(255.0, idx.abs()))))
                    }
                }
            }
        };
        // a negative Z extrusion mirrors in X
        let mm = match e.obj("extrusion") {
            Some(x) if vn(x.get("z")) < -0.5 => mul_a(&ctx.m, &[-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            _ => ctx.m,
        };
        let sim = similar(&mm);
        let sc = det(&mm).abs().sqrt();
        let flip = det(&mm) < 0.0;
        let ty = e.str("type").unwrap_or("");
        match ty {
            "LINE" => {
                let (x1, y1, _) = pt(e, "start");
                let (x2, y2, _) = pt(e, "end");
                let (a, b) = (pa(&mm, x1, y1), pa(&mm, x2, y2));
                let mut o = Self::ent("line", &color);
                for (k, v) in [("x1", a[0]), ("y1", a[1]), ("x2", b[0]), ("y2", b[1])] {
                    o.insert(k.into(), n(r4(v)));
                }
                self.push(&layer, o);
            }
            "CIRCLE" => {
                let (cx, cy, _) = pt(e, "center");
                let r = vn(e.get("radius"));
                if sim {
                    let c = pa(&mm, cx, cy);
                    let mut o = Self::ent("circle", &color);
                    o.insert("x".into(), n(r4(c[0])));
                    o.insert("y".into(), n(r4(c[1])));
                    o.insert("r".into(), n(r4(r * sc)));
                    self.push(&layer, o);
                } else {
                    let pts: Vec<[f64; 2]> =
                        sample_arc_rad(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI).into_iter().map(|p| pa(&mm, p[0], p[1])).collect();
                    self.poly_xy(&pts, true, &layer, &color);
                }
            }
            "ARC" => {
                let (cx, cy, _) = pt(e, "center");
                let r = vn(e.get("radius"));
                let (sa, ea) = (vn(e.get("startAngle")), vn(e.get("endAngle")));
                if sim {
                    let c = pa(&mm, cx, cy);
                    let s0 = lin(&mm, m::cos(sa), m::sin(sa));
                    let s1 = lin(&mm, m::cos(ea), m::sin(ea));
                    let (a0, a1) = (ang(s0[0], s0[1]), ang(s1[0], s1[1]));
                    let mut o = Self::ent("arc", &color);
                    o.insert("x".into(), n(r4(c[0])));
                    o.insert("y".into(), n(r4(c[1])));
                    o.insert("r".into(), n(r4(r * sc)));
                    o.insert("a0".into(), n(if flip { a1 } else { a0 }));
                    o.insert("a1".into(), n(if flip { a0 } else { a1 }));
                    self.push(&layer, o);
                } else {
                    let pts: Vec<[f64; 2]> = sample_arc_rad(cx, cy, r, sa, ea).into_iter().map(|p| pa(&mm, p[0], p[1])).collect();
                    self.poly_xy(&pts, false, &layer, &color);
                }
            }
            "LWPOLYLINE" | "POLYLINE_2D" | "POLYLINE_3D" => {
                let vs: Vec<&Obj> = objs(arr(e, "vertices")).filter(|v| bits(vn(v.get("flags"))) & 16 == 0).collect();
                let closed = truthy(e.get("closed"));
                if ty != "POLYLINE_3D" && sim {
                    let pts: Vec<J> = vs
                        .iter()
                        .map(|v| {
                            let [x, y] = pa(&mm, vn(v.get("x")), vn(v.get("y")));
                            let b = or(vn(v.get("bulge")), 0.0) * if flip { -1.0 } else { 1.0 };
                            if b.abs() > 1e-9 { J::Array(vec![n(r4(x)), n(r4(y)), n(r4(b))]) } else { J::Array(vec![n(r4(x)), n(r4(y))]) }
                        })
                        .collect();
                    self.poly_from(pts, closed, &layer, &color);
                } else {
                    let p3: Vec<[f64; 3]> = vs
                        .iter()
                        .map(|v| [vn(v.get("x")), vn(v.get("y")), if ty == "POLYLINE_3D" { 0.0 } else { or(vn(v.get("bulge")), 0.0) }])
                        .collect();
                    let pts: Vec<[f64; 2]> = poly_points(&p3, closed).into_iter().map(|p| pa(&mm, p[0], p[1])).collect();
                    self.poly_xy(&pts, closed, &layer, &color);
                }
            }
            "ELLIPSE" => {
                let (cx0, cy0, _) = pt(e, "center");
                let (mx0, my0, _) = pt(e, "majorAxis");
                let (sp, ep, ratio) = (vn(e.get("startParam")), vn(e.get("endParam")), vn(e.get("ratio")));
                if sim {
                    let c = pa(&mm, cx0, cy0);
                    let mv = lin(&mm, mx0, my0);
                    let (mut t0, mut t1) = (sp / D2R, ep / D2R);
                    if flip {
                        let (a, b) = (-t1, -t0);
                        t0 = a;
                        t1 = b;
                    }
                    let full = (ep - sp - 2.0 * std::f64::consts::PI).abs() < 1e-6;
                    let mut o = Self::ent("ellipse", &color);
                    o.insert("cx".into(), n(r4(c[0])));
                    o.insert("cy".into(), n(r4(c[1])));
                    o.insert("mx".into(), n(r4(mv[0])));
                    o.insert("my".into(), n(r4(mv[1])));
                    o.insert("ratio".into(), n(r4(m::min(1.0, ratio))));
                    o.insert("t0".into(), n(if full { 0.0 } else { r4(norm360(t0)) }));
                    o.insert("t1".into(), n(if full { 360.0 } else { r4(norm360(t1)) }));
                    self.push(&layer, o);
                } else {
                    let len = m::hypot(mx0, my0);
                    let rot = m::atan2(my0, mx0);
                    let pts = super::ellipse_sample(cx0, cy0, m::cos(rot) * len, m::sin(rot) * len, ratio, sp / D2R, ep / D2R);
                    let pts: Vec<[f64; 2]> = pts.into_iter().map(|p| pa(&mm, p[0], p[1])).collect();
                    self.poly_xy(&pts, false, &layer, &color);
                }
            }
            "SPLINE" => {
                let pts: Vec<[f64; 2]> = spline_points(e).into_iter().map(|p| pa(&mm, p[0], p[1])).collect();
                self.poly_xy(&pts, false, &layer, &color);
            }
            "LEADER" => {
                let pts: Vec<[f64; 2]> = objs(arr(e, "vertices")).map(|v| pa(&mm, vn(v.get("x")), vn(v.get("y")))).collect();
                self.poly_xy(&pts, false, &layer, &color);
            }
            "TEXT" => self.text_ent(e, &mm, &layer, &color, None, None, None, None),
            "ATTRIB" => {
                if bits(vn(e.get("flags"))) & 1 == 0 {
                    self.text_ent(e, &mm, &layer, &color, None, None, None, None);
                }
            }
            "MTEXT" => {
                let lines: Vec<String> = mtext_lines(&vso(e.get("value"))).into_iter().filter(|l| !crate::js::trim(l).is_empty()).collect();
                let h = vn(e.get("height"));
                if lines.is_empty() || !(h > 0.0) {
                    return;
                }
                let xa = e.obj("xAxis");
                let rot = if xa.is_some_and(|x| truthy(x.get("x")) || truthy(x.get("y"))) {
                    let (x, y, _) = pt(e, "xAxis");
                    m::atan2(y, x)
                } else {
                    or(vn(e.get("rotation")), 0.0)
                };
                let ap = or(vn(e.get("attach")), 1.0) - 1.0;
                let ha = ap % 3.0;
                let row = (ap / 3.0).floor();
                let gap = h * 1.667 * or(vn(e.get("lineSpacing")), 1.0);
                let total = gap * (lines.len() as f64 - 1.0);
                let top = if row == 0.0 {
                    -h
                } else if row == 1.0 {
                    total / 2.0 - h / 2.0
                } else {
                    total
                };
                let (ix, iy, _) = pt(e, "insert");
                for (i, ln) in lines.iter().enumerate() {
                    let dy = top - gap * i as f64;
                    let w = len16(ln) as f64 * h * 0.6;
                    let dx = if ha == 1.0 {
                        -w / 2.0
                    } else if ha == 2.0 {
                        -w
                    } else {
                        0.0
                    };
                    let x = ix + dx * m::cos(rot) - dy * m::sin(rot);
                    let y = iy + dx * m::sin(rot) + dy * m::cos(rot);
                    self.text_ent(e, &mm, &layer, &color, Some(ln), Some((x, y)), Some(rot), Some(h));
                }
            }
            "POINT" => {
                let p = e.obj("position");
                let x = p.and_then(|p| p.get("x")).map_or(0.0, |v| vn(Some(v)));
                let y = p.and_then(|p| p.get("y")).map_or(0.0, |v| vn(Some(v)));
                let q = pa(&mm, x, y);
                let mut o = Self::ent("point", &color);
                o.insert("x".into(), n(r4(q[0])));
                o.insert("y".into(), n(r4(q[1])));
                self.push(&layer, o);
            }
            "SOLID" | "TRACE" => {
                let p4: Vec<&Obj> = objs(arr(e, "points")).collect();
                let c4: Vec<&Obj> = [p4.first(), p4.get(1), p4.get(3).or(p4.get(2)), p4.get(2)].into_iter().flatten().copied().collect();
                let loop_: Vec<J> = c4.iter().flat_map(|p| pa(&mm, vn(p.get("x")), vn(p.get("y")))).map(|v| n(r4(v))).collect();
                let mut o = Self::ent("hatch", &color);
                o.insert("loops".into(), J::Array(vec![J::Array(loop_)]));
                o.insert("solid".into(), J::Bool(true));
                self.push(&layer, o);
            }
            "HATCH" => {
                if !truthy(e.get("solid")) {
                    self.skip("HATCH (pattern)".into());
                    return;
                }
                let loops: Vec<J> = hatch_loops(e)
                    .into_iter()
                    .map(|l| {
                        let mut o = Vec::new();
                        for c in l.chunks(2) {
                            let q = pa(&mm, c.first().copied().unwrap_or(f64::NAN), c.get(1).copied().unwrap_or(f64::NAN));
                            o.push(n(r4(q[0])));
                            o.push(n(r4(q[1])));
                        }
                        o
                    })
                    .filter(|l| l.len() >= 6)
                    .map(J::Array)
                    .collect();
                if !loops.is_empty() {
                    let mut o = Self::ent("hatch", &color);
                    o.insert("loops".into(), J::Array(loops));
                    o.insert("solid".into(), J::Bool(true));
                    self.push(&layer, o);
                }
            }
            "3DFACE" => {
                let p4: Vec<&Obj> = objs(arr(e, "points")).collect();
                let (Some(p0), Some(p1), Some(c3)) = (p4.first(), p4.get(1), p4.get(2)) else { return };
                let c4 = p4.get(3).copied();
                let q = |p: &Obj| {
                    let [x, y] = pa(&mm, vn(p.get("x")), vn(p.get("y")));
                    [x, y, or(vn(p.get("z")), 0.0) * ctx.zs + ctx.zo]
                };
                let (q0, q1, q2) = (q(p0), q(p1), q(c3));
                let q3 = q(c4.unwrap_or(c3));
                self.faces.extend([q0, q1, q2]);
                if let Some(c4) = c4
                    && (vn(c4.get("x")) != vn(c3.get("x"))
                        || vn(c4.get("y")) != vn(c3.get("y"))
                        || or(vn(c4.get("z")), 0.0) != or(vn(c3.get("z")), 0.0))
                {
                    self.faces.extend([q0, q2, q3]);
                }
            }
            "INSERT" | "MINSERT" => {
                let bn = vso(e.get("block"));
                let Some(sub) = self.blocks.get(&bn).copied() else {
                    self.skip(msg("libitem.block.skip.missing", &[]));
                    return;
                };
                if bits(vn(sub.get("flags"))) & 4 != 0 {
                    self.skip(msg("libitem.block.skip.xref", &[]));
                    return;
                }
                if ctx.depth > 20 {
                    self.skip(msg("libitem.block.skip.deep", &[]));
                    return;
                }
                let (bx, by, bz) = match sub.obj("base") {
                    Some(b) => (vn(b.get("x")), vn(b.get("y")), or(vn(b.get("z")), 0.0)),
                    None => (0.0, 0.0, 0.0),
                };
                let scl = e.obj("scale");
                let sv = |k: &str| or(scl.map_or(f64::NAN, |s| vn(s.get(k))), 1.0);
                let (sx, sy, sz) = (sv("x"), sv("y"), sv("z"));
                let (ipx, ipy, ipz) = pt(e, "insert");
                let cols = m::max(1.0, or(vn(e.get("cols")), 1.0));
                let rows = m::max(1.0, or(vn(e.get("rows")), 1.0));
                let (cs, rs) = (or(vn(e.get("colSpacing")), 0.0), or(vn(e.get("rowSpacing")), 0.0));
                let mut r = 0.0;
                let mut count = 0usize;
                while r < rows && count < 10_000 {
                    let mut cc = 0.0;
                    while cc < cols && count < 10_000 {
                        count = count.saturating_add(1);
                        let local = mul_a(
                            &mul_a(&[1.0, 0.0, 0.0, 1.0, ipx, ipy], &rot_a(or(vn(e.get("rotation")), 0.0))),
                            &mul_a(&[sx, 0.0, 0.0, sy, 0.0, 0.0], &[1.0, 0.0, 0.0, 1.0, -bx + cc * cs / sx, -by + r * rs / sy]),
                        );
                        let sub_ctx = Ctx {
                            m: mul_a(&mm, &local),
                            zs: ctx.zs * sz,
                            zo: ctx.zo + (or(ipz, 0.0) - bz * sz) * ctx.zs,
                            layer: Some(layer.clone()),
                            color: color.clone(),
                            depth: ctx.depth.saturating_add(1),
                        };
                        for be in objs(arr(sub, "entities")) {
                            if be.str("type") != Some("ATTDEF") {
                                self.emit(be, &sub_ctx, false);
                            }
                        }
                        cc += 1.0;
                    }
                    r += 1.0;
                }
                for a in objs(arr(e, "attribs")) {
                    self.emit(a, ctx, false);
                }
            }
            "ATTDEF" => {}
            _ => {
                let t = if ty == "UNKNOWN" {
                    let d = vso(e.get("dxfName"));
                    if d.is_empty() { "UNKNOWN".to_string() } else { d }
                } else if ty.contains("DIMENSION") {
                    "DIMENSION".to_string()
                } else {
                    ty.to_string()
                };
                self.skip(t);
            }
        }
    }
}

/// `itemFromBlock(db, name, { units, fileName, candidate })` → (item, notes)
pub fn item_from_block(
    db: &Obj,
    name: &str,
    units: Option<&str>,
    file_name: &str,
    candidate: Option<&Candidate>,
) -> Result<(Option<J>, Vec<String>), String> {
    let mut notes = Vec::new();
    let all = arr(db, "blocks");
    let mut blocks: HashMap<String, &Obj> = HashMap::new();
    for b in objs(all) {
        blocks.insert(vso(b.get("name")), b);
    }
    for b in objs(all) {
        let nm = item_block_name(b, all);
        if nm != vso(b.get("name")) && !blocks.contains_key(&nm) {
            blocks.insert(nm, b);
        }
    }
    let found;
    let candidate = match candidate {
        Some(c) => Some(c),
        None if !blocks.contains_key(name) => {
            found = block_candidates(db).into_iter().find(|x| x.name == name);
            found.as_ref()
        }
        None => None,
    };
    let bname = candidate.and_then(|c| c.block.clone()).unwrap_or_else(|| name.to_string());
    let Some(blk) = blocks.get(&bname).copied() else {
        return Ok((None, vec![msg("libitem.block.missing", &[("name", name.to_string())])]));
    };
    let only: Option<HashSet<String>> = candidate
        .and_then(|c| c.state.as_ref())
        .and_then(|st| visibility_states(blk).and_then(|v| v.into_iter().find(|(n, _)| n == st).map(|(_, h)| h)));
    let (umm, _) = drawing_units(db, units)?;
    let rec = db
        .obj("tables")
        .and_then(|t| t.arr("blockRecords"))
        .and_then(|r| objs(r).find(|x| x.get("handle").is_some() && x.get("handle") == blk.get("record")));
    let block_units = rec.map(|r| insunits(vn(r.get("insertUnits")))).filter(|k| !k.is_empty());
    let mm = if units.is_some() { umm } else { block_units.and_then(unit_mm).unwrap_or(umm) };
    let cands;
    let c = match candidate {
        Some(c) => Some(c),
        None => {
            cands = block_candidates(db);
            cands.iter().find(|x| x.name == name)
        }
    };
    let attrs: Attrs = c.map(|c| c.attrs.clone()).unwrap_or_default();
    let layer_table: Vec<&Obj> = db.obj("tables").and_then(|t| t.arr("layers")).map(|l| objs(l).collect()).unwrap_or_default();
    let (bx, by, bz) = match blk.obj("base") {
        Some(b) => (vn(b.get("x")), vn(b.get("y")), or(vn(b.get("z")), 0.0)),
        None => (0.0, 0.0, 0.0),
    };
    let root = Ctx { m: [mm, 0.0, 0.0, mm, -bx * mm, -by * mm], zs: mm, zo: -bz * mm, layer: None, color: None, depth: 0 };
    let mut em = Emit { blocks: &blocks, used_layers: Vec::new(), entities: Vec::new(), faces: Vec::new(), skipped: Vec::new() };
    for e in objs(arr(blk, "entities")) {
        if e.str("type") == Some("ATTDEF") {
            continue;
        }
        if let Some(only) = &only {
            if only.contains(&vs(e.get("handle"))) {
                em.emit(e, &root, true);
            }
            continue;
        }
        em.emit(e, &root, false);
    }
    let info = attrs_info(&attrs);
    let mut it = base_item(if info.title.is_empty() { name } else { &info.title });
    it.insert("maker".into(), s(&clip500(&info.maker)));
    it.insert("model".into(), s(&clip500(&info.model)));
    let what = match candidate.and_then(|c| c.state.as_ref()) {
        Some(st) => format!("{} ({st})", candidate.and_then(|c| c.block.clone()).unwrap_or_else(|| name.to_string())),
        None => name.to_string(),
    };
    it.insert(
        "description".into(),
        s(&clip500(&format!("Block {what}{}.", if file_name.is_empty() { String::new() } else { format!(" in {file_name}") }))),
    );
    let layers: Vec<J> = em
        .used_layers
        .iter()
        .map(|nm| {
            let l = layer_table.iter().find(|l| l.str("name") == Some(nm.as_str()));
            let c = l.and_then(|l| l.obj("color"));
            let col = match c {
                Some(c) if c.get("rgb").is_some() => s(&hex_rgb(vn(c.get("rgb")))),
                _ => {
                    let idx = c.map_or(7.0, |c| or(vn(c.get("index")), 7.0));
                    let idx =
                        if c.is_some_and(|c| c.get("index").is_none() || matches!(c.get("index"), Some(V::Null))) { 7.0 } else { idx };
                    n(m::max(1.0, m::min(255.0, or(idx.abs(), 7.0))))
                }
            };
            let lname = clip(nm, 255);
            serde_json::json!({ "name": if lname.is_empty() { "0".to_string() } else { lname }, "color": col })
        })
        .collect();
    let entities = em.entities;
    let nents = entities.len();
    it.insert("plan".into(), serde_json::json!({ "layers": layers, "entities": entities }));
    let mut params = params_from_attrs(&attrs, mm);
    it.insert("source_attrs".into(), J::Object(source_attrs(&attrs)));
    if let Some(b) = plan_bounds(it.get("plan")) {
        if !params.contains_key("width") {
            params.insert("width".into(), n(r4(b[2] - b[0])));
        }
        if !params.contains_key("depth") {
            params.insert("depth".into(), n(r4(b[3] - b[1])));
        }
    }
    let faces = em.faces;
    let f0z = faces.first().map_or(0.0, |p| p[2]);
    if faces.len() >= 6 && faces.iter().any(|p| (p[2] - f0z).abs() > 1e-6) {
        let mut positions: Vec<f64> = Vec::new();
        let mut indices: Vec<f64> = Vec::new();
        let mut seen: HashMap<[u64; 3], f64> = HashMap::new();
        for p in &faces {
            let q = [r4(p[0]), r4(p[1]), r4(p[2])];
            let k = [super::nkey(q[0]), super::nkey(q[1]), super::nkey(q[2])];
            let i = *seen.entry(k).or_insert_with(|| {
                let i = positions.len() as f64 / 3.0;
                positions.extend(q);
                i
            });
            indices.push(i);
        }
        if indices.len() / 3 <= LIMIT_TRIANGLES {
            let (mut z_min, mut z_max) = (f64::INFINITY, f64::NEG_INFINITY);
            for z in positions.iter().skip(2).step_by(3) {
                z_min = m::min(z_min, *z);
                z_max = m::max(z_max, *z);
            }
            it.insert(
                "body".into(),
                serde_json::json!({ "type": "mesh", "positions": positions.iter().map(|x| n(*x)).collect::<Vec<_>>(), "indices": indices.iter().map(|x| n(*x)).collect::<Vec<_>>() }),
            );
            if !params.contains_key("height") {
                params.insert("height".into(), n(r4(z_max - z_min)));
            }
            notes.push(msg("libitem.block.faces_body", &[("n", num_str(faces.len() as f64 / 3.0))]));
            if nents == 0 {
                let hull = convex_hull(&faces.iter().map(|p| [p[0], p[1]]).collect::<Vec<_>>());
                if hull.len() >= 3
                    && let Some(plan) = it.get_mut("plan").and_then(J::as_object_mut)
                {
                    if let Some(J::Array(es)) = plan.get_mut("entities") {
                        es.push(serde_json::json!({ "type": "poly", "layer": "0", "color": null, "pts": pts_json(&hull), "closed": true }));
                    }
                    if let Some(J::Array(ls)) = plan.get_mut("layers")
                        && ls.is_empty()
                    {
                        ls.push(serde_json::json!({ "name": "0", "color": 7 }));
                    }
                }
            }
        } else {
            notes.push(msg("libitem.block.faces_over", &[]));
        }
    } else if let Some(h) = params.get("height").and_then(J::as_f64).filter(|h| *h != 0.0) {
        let b = box_from_plan(it.get("plan"), h, 0.0).unwrap_or(J::Null);
        it.insert("body".into(), b);
        notes.push(msg("libitem.block.box_body", &[("h", num_str(m::round(h)))]));
    } else {
        notes.push(msg("libitem.block.no_height", &[]));
    }
    it.insert("params".into(), J::Object(params));
    // (the hull of the faces went into the same list)
    let nents = it.get("plan").and_then(|p| p.get("entities")).and_then(J::as_array).map_or(nents, Vec::len);
    if nents == 0 {
        notes.push(msg("libitem.block.draws_nothing", &[]));
    }
    if !em.skipped.is_empty() {
        let list = em.skipped.iter().map(|(t, k)| format!("{t} {k}")).collect::<Vec<_>>().join(", ");
        notes.push(msg("libitem.block.not_converted", &[("list", list)]));
    }
    let title = if info.title.is_empty() { name.to_string() } else { info.title.clone() };
    match suggest_category(&title) {
        Some(sg) => {
            it.insert("category".into(), s(sg));
            notes.push(msg("libitem.category.suggested_from", &[("category", sg.to_string()), ("from", title)]));
        }
        None => {
            let cat = if attrs.iter().any(|(k, _)| k.to_uppercase().starts_with("KCL-")) { "foodservice/other" } else { "other/other" };
            it.insert("category".into(), s(cat));
            notes.push(msg("libitem.category.set", &[("category", cat.to_string())]));
        }
    }
    if c.is_some_and(|c| c.attrs_from == Some("item tag")) {
        notes.push(msg("libitem.block.from_tag", &[]));
    }
    let _ = LIMIT_TEXT;
    Ok((Some(J::Object(it)), notes))
}
