//! libitem.js — the "CadCraft library item", format 1 (docs/LIBRARY-SPEC.md §1), and the file adapters that need
//! nothing but bytes or an already-parsed file (§2): one for one with the frozen 0.5 JS (the same file in BimCraft and
//! CadCraft). Lengths in mm, angles in degrees, Z up, the item's front faces −Y.
//!
//! - [`validate_item`], [`item_hash`] (sha256 of the canonical JSON without `provenance`), [`plan_bounds`],
//!   [`box_from_plan`], [`convex_hull`], [`params_from_attrs`], [`attrs_info`], [`suggest_category`],
//!   [`parse_length`], [`clean_item`]
//! - adapters, each → [`Read`] { items, notes }: [`items_from_csv`], [`mesh::item_from_mesh`] (glTF / GLB / OBJ /
//!   STL), [`dwg::items_from_dwg`] / [`dwg::item_from_block`] (a drawing's blocks), [`rfa::item_from_rfa`] (a Revit
//!   family's PartAtom), [`ifc::items_from_ifc`] (libitem-ifc.js: IFC types).
//!
//! Every file is hostile: sizes, counts and depths are capped as the JS caps them (8 MB items, 200,000 triangles,
//! 50,000 plan entities, 500-character texts, coordinates within 10 km).

pub mod dwg;
pub mod ifc;
pub mod mesh;
pub mod rfa;

use serde_json::{Map, Value as J};

use crate::draw::m;
use crate::draw::tr;
use crate::js::{len16, num_str, slice16};

pub const FORMAT: &str = "cadcraft-library-item";
pub const FORMAT_VERSION: f64 = 1.0;

/// The taxonomy (core/classes/LibraryCatalog.php CATEGORIES), in its order.
pub const CATEGORY_KEYS: [&str; 36] = [
    "foodservice/cooking",
    "foodservice/refrigeration",
    "foodservice/sinks",
    "foodservice/worktables",
    "foodservice/shelving",
    "foodservice/warewashing",
    "foodservice/ventilation",
    "foodservice/prep",
    "foodservice/holding",
    "foodservice/beverage",
    "foodservice/ice",
    "foodservice/serving",
    "foodservice/other",
    "furniture/seating",
    "furniture/tables",
    "furniture/desks",
    "furniture/storage",
    "furniture/beds",
    "furniture/other",
    "casework/base",
    "casework/wall",
    "casework/tall",
    "plumbing/fixtures",
    "plumbing/equipment",
    "electrical/fixtures",
    "electrical/equipment",
    "mechanical/equipment",
    "appliances/residential",
    "site/landscape",
    "site/vehicles",
    "people/entourage",
    "symbols/annotation",
    "symbols/electrical",
    "symbols/plumbing",
    "details/generic",
    "other/other",
];
/// A category's label in the page's language.
pub fn category_label(k: &str) -> Option<String> {
    CATEGORY_KEYS.contains(&k).then(|| msg(&format!("libitem.cat.{}", k.replacen('/', ".", 1)), &[]))
}
pub const KINDS: [&str; 5] = ["equipment", "furniture", "fixture", "symbol", "detail"];
pub const LICENSES: [&str; 6] = ["CC0-1.0", "CC-BY-4.0", "CC-BY-3.0", "CC-BY-SA-4.0", "MAKER-TERMS", "TEAM-ONLY"];
pub const LIMIT_BYTES: usize = 8 * 1024 * 1024;
pub const LIMIT_TRIANGLES: usize = 200_000;
pub const LIMIT_ENTITIES: usize = 50_000;
pub const LIMIT_TEXT: usize = 500;
pub const LIMIT_COORD: f64 = 1e7;

/// Length units → mm (UNIT_MM).
pub fn unit_mm(k: &str) -> Option<f64> {
    Some(match k {
        "mm" => 1.0,
        "cm" => 10.0,
        "dm" => 100.0,
        "m" => 1000.0,
        "km" => 1e6,
        "in" => 25.4,
        "ft" => 304.8,
        "yd" => 914.4,
        "mi" => 1_609_344.0,
        "mil" => 0.0254,
        "um" => 0.001,
        _ => return None,
    })
}
pub const UNIT_KEYS: [&str; 11] = ["mm", "cm", "dm", "m", "km", "in", "ft", "yd", "mi", "mil", "um"];
/// DWG INSUNITS → a UNIT_MM key ('' = unitless).
pub fn insunits(i: f64) -> &'static str {
    match i {
        1.0 => "in",
        2.0 => "ft",
        3.0 => "mi",
        4.0 => "mm",
        5.0 => "cm",
        6.0 => "m",
        7.0 => "km",
        9.0 => "mil",
        10.0 => "yd",
        13.0 => "um",
        14.0 => "dm",
        _ => "",
    }
}
pub const ELEC_KEYS: [&str; 9] = ["volts", "phase", "amps", "kw", "hp", "hz", "plug", "nema", "direct"];
pub const PLUMB_KEYS: [&str; 5] = ["cw", "hw", "dd", "id", "gas"];

/// `msg(key, fills)`: the page's words for a libitem.* key (the app catalog holds the English MESSAGES).
pub fn msg(key: &str, fills: &[(&str, String)]) -> String {
    let v: Vec<(&str, &str)> = fills.iter().map(|(k, x)| (*k, x.as_str())).collect();
    tr::t(key, &v)
}

/// What an adapter read: items and plain sentences about what it did.
#[derive(Clone, Debug, Default)]
pub struct Read {
    pub items: Vec<J>,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------------------------------------- numbers
/// `Math.round(v * 10000) / 10000`
pub fn r4(v: f64) -> f64 {
    m::round(v * 10000.0) / 10000.0
}
/// `Math.round(v * 100) / 100`
pub fn r2(v: f64) -> f64 {
    m::round(v * 100.0) / 100.0
}
/// A number as the item JSON holds it (whole numbers as integers, as JSON.stringify writes them).
pub fn n(x: f64) -> J {
    if x == 0.0 {
        return J::from(0);
    }
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        #[allow(clippy::cast_possible_truncation)]
        return J::from(x as i64);
    }
    serde_json::Number::from_f64(x).map_or(J::Null, J::Number)
}
pub fn s(x: &str) -> J {
    J::String(x.to_string())
}

// ---------------------------------------------------------------------------------------------------------- text
fn is_ctl(c: char) -> bool {
    matches!(c as u32, 0..=8 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)
}
/// `clip(s, n)`: control characters out, trimmed, at most n UTF-16 units.
pub fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().filter(|c| !is_ctl(*c)).collect();
    let t = crate::js::trim(&t);
    if len16(t) > n { slice16(t, n) } else { t.to_string() }
}
pub fn clip500(s: &str) -> String {
    clip(s, LIMIT_TEXT)
}
/// `clean(s)`: `\p{C}` (control, format, surrogate, private use, unassigned) out, trimmed.
pub fn clean(s: &str) -> String {
    static RX: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let t = match RX.get_or_init(|| regex::Regex::new(r"\p{C}").ok()) {
        Some(rx) => rx.replace_all(s, "").into_owned(),
        None => s.to_string(),
    };
    crate::js::trim(&t).to_string()
}
/// `String(v ?? '')` of a JSON value.
pub fn jstr(v: Option<&J>) -> String {
    match v {
        None | Some(J::Null) => String::new(),
        Some(J::String(s)) => s.clone(),
        Some(J::Number(x)) => num_str(x.as_f64().unwrap_or(f64::NAN)),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Array(a)) => a.iter().map(|x| if x.is_null() { String::new() } else { jstr(Some(x)) }).collect::<Vec<_>>().join(","),
        Some(J::Object(_)) => "[object Object]".into(),
    }
}

// ---------------------------------------------------------------------------------------------------------- validation
/// ENTITY_KEYS
fn entity_keys(t: &str) -> Option<&'static [&'static str]> {
    Some(match t {
        "line" => &["x1", "y1", "x2", "y2"],
        "circle" => &["x", "y", "r"],
        "arc" => &["x", "y", "r", "a0", "a1"],
        "poly" => &["pts", "closed"],
        "ellipse" => &["cx", "cy", "mx", "my", "ratio", "t0", "t1"],
        "text" => &["x", "y", "h", "rot", "value"],
        "point" => &["x", "y"],
        "hatch" => &["loops", "solid"],
        _ => return None,
    })
}
const TOP_KEYS: [&str; 18] = [
    "format",
    "format_version",
    "uuid",
    "version",
    "name",
    "kind",
    "category",
    "description",
    "maker",
    "model",
    "ifc",
    "plan",
    "body",
    "clearances",
    "params",
    "source_attrs",
    "license",
    "provenance",
];

fn is_num(v: &J) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}
fn is_int(x: f64) -> bool {
    x.is_finite() && x.fract() == 0.0
}
fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() })
}
/// `/^Ifc[A-Za-z]{2,60}$/`
pub fn is_ifc_class(s: &str) -> bool {
    s.strip_prefix("Ifc").is_some_and(|r| (2..=60).contains(&r.len()) && r.bytes().all(|c| c.is_ascii_alphabetic()))
}

struct V {
    errors: Vec<String>,
}
impl V {
    fn err(&mut self, path: &str, text: String) {
        if self.errors.len() < 50 {
            self.errors.push(format!("{path}: {text}"));
        }
    }
    fn num(&mut self, path: &str, v: Option<&J>, min: f64, max: f64, positive: bool, int: bool) {
        let Some(x) = v.and_then(is_num) else { return self.err(path, msg("libitem.v.number", &[])) };
        if x < min || x > max {
            return self.err(path, msg("libitem.v.range", &[]));
        }
        if positive && !(x > 0.0) {
            return self.err(path, msg("libitem.v.positive", &[]));
        }
        if int && !is_int(x) {
            self.err(path, msg("libitem.v.whole", &[]));
        }
    }
    fn coord(&mut self, path: &str, v: Option<&J>) {
        self.num(path, v, -LIMIT_COORD, LIMIT_COORD, false, false);
    }
    fn str(&mut self, path: &str, v: Option<&J>, required: bool) {
        let Some(J::String(t)) = v else { return self.err(path, msg("libitem.v.text", &[])) };
        if len16(t) > LIMIT_TEXT {
            self.err(path, msg("libitem.v.too_long", &[("n", LIMIT_TEXT.to_string())]));
        }
        if required && crate::js::trim(t).is_empty() {
            self.err(path, msg("libitem.v.empty", &[]));
        }
    }
    fn obj<'j>(&mut self, path: &str, v: Option<&'j J>) -> Option<&'j Map<String, J>> {
        match v {
            Some(J::Object(o)) => Some(o),
            _ => {
                self.err(path, msg("libitem.v.object", &[]));
                None
            }
        }
    }
    fn keys(&mut self, path: &str, v: &Map<String, J>, allowed: &[&str]) {
        for k in v.keys() {
            if !allowed.contains(&k.as_str()) {
                self.err(&format!("{path}.{k}"), msg("libitem.v.unknown_key", &[]));
            }
        }
    }
    fn color(&mut self, path: &str, v: &J) {
        match v {
            J::Null => {}
            J::Number(_) => {
                let x = v.as_f64().unwrap_or(f64::NAN);
                if !is_int(x) || !(0.0..=256.0).contains(&x) {
                    self.err(path, msg("libitem.v.aci", &[]));
                }
            }
            J::String(t) if t.len() == 7 && t.starts_with('#') && t.bytes().skip(1).all(|c| c.is_ascii_hexdigit()) => {}
            _ => self.err(path, msg("libitem.v.color", &[])),
        }
    }
    fn bx(&mut self, p: &str, b: &Map<String, J>, named: bool) {
        let allowed: &[&str] = if named { &["name", "x", "y", "z", "w", "d", "h"] } else { &["type", "x", "y", "z", "w", "d", "h"] };
        self.keys(p, b, allowed);
        for k in ["x", "y", "z"] {
            self.coord(&format!("{p}.{k}"), b.get(k));
        }
        for k in ["w", "d", "h"] {
            self.num(&format!("{p}.{k}"), b.get(k), -LIMIT_COORD, LIMIT_COORD, true, false);
        }
    }
}

/// `validateItem(obj, { partial })` → (ok, errors)
pub fn validate_item(obj: &J, partial: bool) -> (bool, Vec<String>) {
    let J::Object(o) = obj else { return (false, vec![format!("item: {}", msg("libitem.v.not_object", &[]))]) };
    let mut v = V { errors: Vec::new() };
    let size = serde_json::to_string(obj).map(|x| x.len()).unwrap_or(0);
    if size > LIMIT_BYTES {
        v.err("item", msg("libitem.v.too_large", &[]));
    }
    v.keys("item", o, &TOP_KEYS);
    let mut required = vec!["format", "format_version", "name", "kind", "category", "plan", "body", "params"];
    if !partial {
        required.extend(["uuid", "version", "license"]);
    }
    for k in required {
        if !o.contains_key(k) {
            v.err(k, msg("libitem.v.required", &[]));
        }
    }
    if o.contains_key("format") && o.get("format").and_then(J::as_str) != Some(FORMAT) {
        v.err("format", msg("libitem.v.must_be", &[("value", FORMAT.to_string())]));
    }
    if o.contains_key("format_version") && o.get("format_version").and_then(J::as_f64) != Some(FORMAT_VERSION) {
        v.err("format_version", msg("libitem.v.must_be_1", &[]));
    }
    if o.contains_key("uuid") && !o.get("uuid").and_then(J::as_str).is_some_and(is_uuid) {
        v.err("uuid", msg("libitem.v.uuid", &[]));
    }
    if o.contains_key("version") {
        v.num("version", o.get("version"), 1.0, LIMIT_COORD, false, true);
    }
    if o.contains_key("name") {
        v.str("name", o.get("name"), true);
    }
    if o.contains_key("kind") && !o.get("kind").and_then(J::as_str).is_some_and(|k| KINDS.contains(&k)) {
        v.err("kind", msg("libitem.v.one_of", &[("list", KINDS.join(", "))]));
    }
    if o.contains_key("category") && !o.get("category").and_then(J::as_str).is_some_and(|k| CATEGORY_KEYS.contains(&k)) {
        v.err("category", msg("libitem.v.taxonomy", &[]));
    }
    for k in ["description", "maker", "model"] {
        if o.contains_key(k) {
            v.str(k, o.get(k), false);
        }
    }
    if o.contains_key("ifc")
        && let Some(ifc) = v.obj("ifc", o.get("ifc"))
    {
        v.keys("ifc", ifc, &["class", "predefined"]);
        if !ifc.get("class").and_then(J::as_str).is_some_and(is_ifc_class) {
            v.err("ifc.class", msg("libitem.v.ifc_class", &[]));
        }
        if ifc.contains_key("predefined")
            && !ifc
                .get("predefined")
                .and_then(J::as_str)
                .is_some_and(|p| (1..=60).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_uppercase() || c == b'_'))
        {
            v.err("ifc.predefined", msg("libitem.v.enum", &[]));
        }
    }
    if o.contains_key("plan")
        && let Some(plan) = v.obj("plan", o.get("plan"))
    {
        validate_plan(&mut v, plan);
    }
    if o.contains_key("body")
        && !matches!(o.get("body"), Some(J::Null))
        && let Some(b) = v.obj("body", o.get("body"))
    {
        match b.get("type").and_then(J::as_str) {
            Some("box") => v.bx("body", b, false),
            Some("mesh") => {
                v.keys("body", b, &["type", "positions", "indices"]);
                let pn: f64 = match b.get("positions") {
                    Some(J::Array(p)) if p.len() >= 9 && p.len() % 3 == 0 => {
                        for (i, x) in p.iter().enumerate() {
                            if !is_num(x).is_some_and(|x| x.abs() <= LIMIT_COORD) {
                                v.err("body.positions", msg("libitem.v.bad_coord_at", &[("i", i.to_string())]));
                                break;
                            }
                        }
                        p.len() as f64 / 3.0
                    }
                    Some(J::Array(p)) => {
                        v.err("body.positions", msg("libitem.v.positions", &[]));
                        p.len() as f64 / 3.0
                    }
                    _ => {
                        v.err("body.positions", msg("libitem.v.positions", &[]));
                        0.0
                    }
                };
                match b.get("indices") {
                    Some(J::Array(ix)) if !ix.is_empty() && ix.len() % 3 == 0 => {
                        if ix.len() / 3 > LIMIT_TRIANGLES {
                            v.err("body.indices", msg("libitem.v.too_many_triangles", &[("n", LIMIT_TRIANGLES.to_string())]));
                        }
                        for (i, x) in ix.iter().enumerate() {
                            let k = x.as_f64().unwrap_or(f64::NAN);
                            if !is_int(k) || k < 0.0 || k >= pn {
                                v.err("body.indices", msg("libitem.v.index_range", &[("i", i.to_string())]));
                                break;
                            }
                        }
                    }
                    _ => v.err("body.indices", msg("libitem.v.triangles", &[])),
                }
            }
            _ => v.err("body.type", msg("libitem.v.body_type", &[])),
        }
    }
    if o.contains_key("clearances") {
        match o.get("clearances") {
            Some(J::Array(cs)) => {
                for (i, c) in cs.iter().enumerate() {
                    let p = format!("clearances[{i}]");
                    if let Some(c) = v.obj(&p, Some(c)) {
                        v.bx(&p, c, true);
                        v.str(&format!("{p}.name"), c.get("name"), false);
                    }
                }
            }
            _ => v.err("clearances", msg("libitem.v.list", &[])),
        }
    }
    if o.contains_key("params")
        && let Some(p) = v.obj("params", o.get("params"))
    {
        v.keys("params", p, &["width", "depth", "height", "weight_kg", "elec", "plumb", "remarks"]);
        for k in ["width", "depth", "height", "weight_kg"] {
            if p.contains_key(k) {
                v.num(&format!("params.{k}"), p.get(k), 0.0, LIMIT_COORD, false, false);
            }
        }
        if p.contains_key("remarks") {
            v.str("params.remarks", p.get("remarks"), false);
        }
        for (k, list) in [("elec", &ELEC_KEYS[..]), ("plumb", &PLUMB_KEYS[..])] {
            if !p.contains_key(k) {
                continue;
            }
            let path = format!("params.{k}");
            let Some(sub) = v.obj(&path, p.get(k)) else { continue };
            v.keys(&path, sub, list);
            for (f, x) in sub {
                if list.contains(&f.as_str()) {
                    v.str(&format!("{path}.{f}"), Some(x), false);
                }
            }
        }
    }
    if o.contains_key("source_attrs")
        && let Some(sa) = v.obj("source_attrs", o.get("source_attrs"))
    {
        for (k, x) in sa {
            if len16(k) > 255 {
                v.err("source_attrs", msg("libitem.v.attr_name", &[]));
            }
            v.str(&format!("source_attrs.{}", slice16(k, 40)), Some(x), false);
        }
    }
    if o.contains_key("license")
        && let Some(l) = v.obj("license", o.get("license"))
    {
        v.keys("license", l, &["id", "holder", "attribution", "url"]);
        if !l.get("id").and_then(J::as_str).is_some_and(|x| LICENSES.contains(&x)) {
            v.err("license.id", msg("libitem.v.license", &[]));
        }
        for k in ["holder", "attribution", "url"] {
            if l.contains_key(k) {
                v.str(&format!("license.{k}"), l.get(k), false);
            }
        }
    }
    if o.contains_key("provenance")
        && let Some(pv) = v.obj("provenance", o.get("provenance"))
    {
        v.keys("provenance", pv, &["library", "item", "version", "sha256", "source_file", "source_sha256", "published_at"]);
        for k in ["library", "item", "sha256", "source_file", "source_sha256", "published_at"] {
            if pv.contains_key(k) && !matches!(pv.get(k), Some(J::Null)) {
                v.str(&format!("provenance.{k}"), pv.get(k), false);
            }
        }
        if pv.contains_key("version") {
            v.num("provenance.version", pv.get("version"), 1.0, LIMIT_COORD, false, true);
        }
    }
    (v.errors.is_empty(), v.errors)
}

fn validate_plan(v: &mut V, plan: &Map<String, J>) {
    v.keys("plan", plan, &["layers", "entities"]);
    match plan.get("layers") {
        Some(J::Array(ls)) => {
            for (i, l) in ls.iter().enumerate() {
                let p = format!("plan.layers[{i}]");
                let Some(l) = v.obj(&p, Some(l)) else { continue };
                v.keys(&p, l, &["name", "color"]);
                v.str(&format!("{p}.name"), l.get("name"), true);
                if let Some(c) = l.get("color") {
                    v.color(&format!("{p}.color"), c);
                }
            }
        }
        _ => v.err("plan.layers", msg("libitem.v.list", &[])),
    }
    let Some(J::Array(es)) = plan.get("entities") else { return v.err("plan.entities", msg("libitem.v.list", &[])) };
    if es.len() > LIMIT_ENTITIES {
        v.err("plan.entities", msg("libitem.v.too_many_entities", &[("n", LIMIT_ENTITIES.to_string())]));
    }
    for (i, e) in es.iter().take(LIMIT_ENTITIES).enumerate() {
        let p = format!("plan.entities[{i}]");
        let Some(e) = v.obj(&p, Some(e)) else { continue };
        let ty = jstr(e.get("type"));
        let Some(want) = entity_keys(&ty) else {
            v.err(&format!("{p}.type"), msg("libitem.v.entity_type", &[]));
            continue;
        };
        let mut allowed = vec!["type", "layer", "color"];
        allowed.extend_from_slice(want);
        v.keys(&p, e, &allowed);
        match e.get("layer") {
            None => v.err(&format!("{p}.layer"), msg("libitem.v.required", &[])),
            Some(l) => v.str(&format!("{p}.layer"), Some(l), false),
        }
        match e.get("color") {
            None => v.err(&format!("{p}.color"), msg("libitem.v.required", &[])),
            Some(c) => v.color(&format!("{p}.color"), c),
        }
        for k in want {
            let path = format!("{p}.{k}");
            let Some(x) = e.get(*k) else {
                v.err(&path, msg("libitem.v.required", &[]));
                continue;
            };
            match *k {
                "value" => v.str(&path, Some(x), false),
                "closed" | "solid" => {
                    if !x.is_boolean() {
                        v.err(&path, msg("libitem.v.boolean", &[]));
                    }
                }
                "pts" => match x {
                    J::Array(pts) if pts.len() >= 2 => {
                        for (j, q) in pts.iter().enumerate() {
                            let qp = format!("{p}.pts[{j}]");
                            match q {
                                J::Array(c) if c.len() == 2 || c.len() == 3 => {
                                    for cc in c {
                                        v.coord(&qp, Some(cc));
                                    }
                                }
                                _ => v.err(&qp, msg("libitem.v.point", &[])),
                            }
                        }
                    }
                    _ => v.err(&path, msg("libitem.v.two_points", &[])),
                },
                "loops" => match x {
                    J::Array(loops) if !loops.is_empty() => {
                        for (j, lp) in loops.iter().enumerate() {
                            let lpath = format!("{p}.loops[{j}]");
                            match lp {
                                J::Array(c) if c.len() >= 6 && c.len() % 2 == 0 => {
                                    if c.iter().any(|q| !is_num(q).is_some_and(|x| x.abs() <= LIMIT_COORD)) {
                                        v.err(&lpath, msg("libitem.v.bad_coord", &[]));
                                    }
                                }
                                _ => v.err(&lpath, msg("libitem.v.loop", &[])),
                            }
                        }
                    }
                    _ => v.err(&path, msg("libitem.v.one_loop", &[])),
                },
                "r" | "h" => v.num(&path, Some(x), -LIMIT_COORD, LIMIT_COORD, true, false),
                "ratio" => v.num(&path, Some(x), -LIMIT_COORD, 1.0, true, false),
                _ => v.coord(&path, Some(x)),
            }
        }
        if ty == "hatch" && e.get("solid") != Some(&J::Bool(true)) {
            v.err(&format!("{p}.solid"), msg("libitem.v.solid_hatch", &[]));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- hash
/// `itemHash(obj)`: sha256 (hex) of the canonical JSON without `provenance`.
pub fn item_hash(obj: &J) -> String {
    use sha2::{Digest, Sha256};
    let input = crate::draw::place::item_hash_input(obj);
    let d = Sha256::digest(input.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// `cleanItem(item)`: working fields (a leading underscore) out.
pub fn clean_item(item: &J) -> J {
    match item {
        J::Object(o) => J::Object(o.iter().filter(|(k, _)| !k.starts_with('_')).map(|(k, v)| (k.clone(), v.clone())).collect()),
        x => x.clone(),
    }
}

// ---------------------------------------------------------------------------------------------------------- geometry
const D2R: f64 = std::f64::consts::PI / 180.0;
fn g(e: &Map<String, J>, k: &str) -> f64 {
    e.get(k).and_then(J::as_f64).unwrap_or(f64::NAN)
}
/// `arcSample(cx, cy, r, a0, a1)` (degrees)
pub fn arc_sample(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let mut sweep = a1 - a0;
    if !sweep.is_finite() {
        return Vec::new();
    }
    if sweep <= 0.0 {
        sweep += ((-sweep / 360.0).floor() + 1.0) * 360.0;
        while sweep <= 0.0 {
            sweep += 360.0;
        }
    }
    let n = m::max(4.0, (sweep / 10.0).ceil()).min(100_000.0);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= n {
        let a = (a0 + sweep * i / n) * D2R;
        out.push([cx + r * m::cos(a), cy + r * m::sin(a)]);
        i += 1.0;
    }
    out
}
/// `ellipseSample(e)`
pub fn ellipse_sample(cx: f64, cy: f64, mx: f64, my: f64, ratio: f64, t0: f64, t1: f64) -> Vec<[f64; 2]> {
    let mut sweep = t1 - t0;
    if !sweep.is_finite() {
        return Vec::new();
    }
    while sweep <= 0.0 {
        sweep += 360.0;
    }
    let len = m::hypot(mx, my);
    let rot = m::atan2(my, mx);
    let (c, s) = (m::cos(rot), m::sin(rot));
    let n = m::max(8.0, (sweep / 10.0).ceil()).min(100_000.0);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= n {
        let t = (t0 + sweep * i / n) * D2R;
        let (x, y) = (len * m::cos(t), len * ratio * m::sin(t));
        out.push([cx + x * c - y * s, cy + x * s + y * c]);
        i += 1.0;
    }
    out
}
/// `polyPoints(pts, closed)`: bulged segments sampled. pts as [x, y, bulge].
pub fn poly_points(pts: &[[f64; 3]], closed: bool) -> Vec<[f64; 2]> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let a = pts.get(i).copied().unwrap_or([0.0; 3]);
        out.push([a[0], a[1]]);
        if i.saturating_add(1) == n && !closed {
            break;
        }
        let b = pts.get(m::nx(i, n)).copied().unwrap_or([0.0; 3]);
        let bulge = if a[2] != 0.0 && !a[2].is_nan() { a[2] } else { 0.0 };
        if bulge.abs() > 1e-9 {
            out.extend(bulge_sample(a, b, bulge));
        }
    }
    out
}
fn bulge_sample(a: [f64; 3], b: [f64; 3], bulge: f64) -> Vec<[f64; 2]> {
    let theta = 4.0 * m::atan(bulge);
    let chord = m::hypot(b[0] - a[0], b[1] - a[1]);
    if !(chord > 0.0) {
        return Vec::new();
    }
    let r = chord / (2.0 * m::sin(theta / 2.0));
    let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let h = r * m::cos(theta / 2.0);
    let (ux, uy) = (-(b[1] - a[1]) / chord, (b[0] - a[0]) / chord);
    let (cx, cy) = (mx + ux * h, my + uy * h);
    let a0 = m::atan2(a[1] - cy, a[0] - cx);
    let steps = m::max(2.0, (theta.abs() / (10.0 * D2R)).ceil());
    let mut out = Vec::new();
    let mut k = 1.0;
    while k < steps {
        let t = a0 + theta * k / steps;
        out.push([cx + r.abs() * m::cos(t), cy + r.abs() * m::sin(t)]);
        k += 1.0;
    }
    out
}
/// [x, y, bulge?] points of a plan poly
pub fn pts3(v: Option<&J>) -> Vec<[f64; 3]> {
    match v {
        Some(J::Array(a)) => a
            .iter()
            .map(|q| {
                let g = |i: usize| q.get(i).and_then(J::as_f64);
                [g(0).unwrap_or(f64::NAN), g(1).unwrap_or(f64::NAN), g(2).unwrap_or(0.0)]
            })
            .collect(),
        _ => Vec::new(),
    }
}
/// `entityPoints(e)`
pub fn entity_points(e: &Map<String, J>) -> Vec<[f64; 2]> {
    match e.get("type").and_then(J::as_str).unwrap_or("") {
        "line" => vec![[g(e, "x1"), g(e, "y1")], [g(e, "x2"), g(e, "y2")]],
        "circle" => {
            let (x, y, r) = (g(e, "x"), g(e, "y"), g(e, "r"));
            vec![[x - r, y - r], [x + r, y + r]]
        }
        "arc" => arc_sample(g(e, "x"), g(e, "y"), g(e, "r"), g(e, "a0"), g(e, "a1")),
        "poly" => poly_points(&pts3(e.get("pts")), e.get("closed").and_then(J::as_bool).unwrap_or(false)),
        "ellipse" => ellipse_sample(g(e, "cx"), g(e, "cy"), g(e, "mx"), g(e, "my"), g(e, "ratio"), g(e, "t0"), g(e, "t1")),
        "point" | "text" => vec![[g(e, "x"), g(e, "y")]],
        "hatch" => match e.get("loops") {
            Some(J::Array(ls)) => ls
                .iter()
                .flat_map(|l| {
                    let c: Vec<f64> = l.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
                    c.chunks(2).map(|p| [p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN)]).collect::<Vec<_>>()
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `planBounds(plan)` → [minX, minY, maxX, maxY]
pub fn plan_bounds(plan: Option<&J>) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for e in plan.and_then(|p| p.get("entities")).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(e) = e.as_object() else { continue };
        for [x, y] in entity_points(e) {
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            if x < b[0] {
                b[0] = x;
            }
            if x > b[2] {
                b[2] = x;
            }
            if y < b[1] {
                b[1] = y;
            }
            if y > b[3] {
                b[3] = y;
            }
        }
    }
    (b[0] <= b[2]).then_some(b)
}

/// `boxFromPlan(plan, h, z)`
pub fn box_from_plan(plan: Option<&J>, h: f64, z: f64) -> Option<J> {
    let b = plan_bounds(plan)?;
    if !(h > 0.0) {
        return None;
    }
    let (w, d) = (b[2] - b[0], b[3] - b[1]);
    if !(w > 0.0) || !(d > 0.0) {
        return None;
    }
    let mut o = Map::new();
    o.insert("type".into(), s("box"));
    for (k, x) in [("x", b[0]), ("y", b[1]), ("z", z), ("w", w), ("d", d), ("h", h)] {
        o.insert(k.into(), n(r4(x)));
    }
    Some(J::Object(o))
}

/// A number as a map key the way `String(n)` keys it (−0 is "0"; every NaN is "NaN").
pub fn nkey(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}
/// `convexHull(points)` (monotone chain, CCW, points rounded to 0.01 and deduplicated first)
pub fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut seen = std::collections::HashSet::new();
    let mut p: Vec<[f64; 2]> = Vec::new();
    for q in points {
        let (x, y) = (r2(q[0]), r2(q[1]));
        // String(x) + ',' + String(y): equal numbers (0 and -0 alike) give equal keys
        if seen.insert((nkey(x), nkey(y))) {
            p.push([x, y]);
        }
    }
    p.sort_by(|a, b| {
        let d = a[0] - b[0];
        crate::js::sign_ord(if d != 0.0 && !d.is_nan() { d } else { a[1] - b[1] })
    });
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<[f64; 2]> = Vec::new();
    let mut hi: Vec<[f64; 2]> = Vec::new();
    for &q in &p {
        while let [.., a, b] = lo.as_slice() {
            if cross(*a, *b, q) <= 0.0 {
                lo.pop();
            } else {
                break;
            }
        }
        lo.push(q);
    }
    for &q in p.iter().rev() {
        while let [.., a, b] = hi.as_slice() {
            if cross(*a, *b, q) <= 0.0 {
                hi.pop();
            } else {
                break;
            }
        }
        hi.push(q);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo
}
pub fn pts_json(p: &[[f64; 2]]) -> J {
    J::Array(p.iter().map(|q| J::Array(vec![n(q[0]), n(q[1])])).collect())
}

/// `baseItem(name)`
pub fn base_item(name: &str) -> Map<String, J> {
    let nm = if name.is_empty() { "Unnamed item".to_string() } else { name.to_string() };
    let mut o = Map::new();
    o.insert("format".into(), s(FORMAT));
    o.insert("format_version".into(), n(FORMAT_VERSION));
    o.insert("name".into(), s(&clip500(&nm)));
    o.insert("kind".into(), s("equipment"));
    o.insert("category".into(), s("other/other"));
    o.insert("description".into(), s(""));
    o.insert("maker".into(), s(""));
    o.insert("model".into(), s(""));
    o.insert("ifc".into(), serde_json::json!({ "class": "IfcFurnishingElement", "predefined": "NOTDEFINED" }));
    o.insert("plan".into(), serde_json::json!({ "layers": [], "entities": [] }));
    o.insert("body".into(), J::Null);
    o.insert("clearances".into(), J::Array(Vec::new()));
    o.insert("params".into(), J::Object(Map::new()));
    o.insert("source_attrs".into(), J::Object(Map::new()));
    o
}

fn obj_mut<'a>(o: &'a mut Map<String, J>, k: &str) -> Option<&'a mut Map<String, J>> {
    o.get_mut(k).and_then(J::as_object_mut)
}

/// `boxItemGeometry(item, w, d, h, z, label)`: a rectangle plan (+ the name as text), a box body centred in X with its
/// back on y = 0.
pub fn box_item_geometry(item: &mut Map<String, J>, w: f64, d: f64, h: f64, z: f64, label: &str) {
    let layer = "EQUIP";
    let (x0, y0) = (-w / 2.0, -d);
    let mut ents = vec![serde_json::json!({ "type": "poly", "layer": layer, "color": null,
        "pts": [[n(r4(x0)), n(r4(y0))], [n(r4(x0 + w)), n(r4(y0))], [n(r4(x0 + w)), 0], [n(r4(x0)), 0]], "closed": true })];
    let text = clip(label, 120);
    if !text.is_empty() {
        let tl = len16(&text) as f64;
        let mut th = m::min(d * 0.18, 150.0);
        let wide = tl * th * 0.6;
        if wide > w * 0.9 {
            th = (w * 0.9) / (tl * 0.6);
        }
        if th > 1.0 {
            ents.push(serde_json::json!({ "type": "text", "layer": layer, "color": null, "x": n(r4(-tl * th * 0.3)), "y": n(r4(-d / 2.0 - th / 2.0)), "h": n(r4(th)), "rot": 0, "value": text }));
        }
    }
    if let Some(plan) = obj_mut(item, "plan") {
        plan.insert("layers".into(), serde_json::json!([{ "name": layer, "color": 7 }]));
        plan.insert("entities".into(), J::Array(ents));
    }
    let z = if z != 0.0 && !z.is_nan() { z } else { 0.0 };
    item.insert(
        "body".into(),
        if h > 0.0 {
            serde_json::json!({ "type": "box", "x": n(r4(x0)), "y": n(r4(y0)), "z": n(r4(z)), "w": n(r4(w)), "d": n(r4(d)), "h": n(r4(h)) })
        } else {
            J::Null
        },
    );
    if let Some(p) = obj_mut(item, "params") {
        p.insert("width".into(), n(r4(w)));
        p.insert("depth".into(), n(r4(d)));
        if h > 0.0 {
            p.insert("height".into(), n(r4(h)));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- attributes
/// `joinText(...xs)`: cleaned, non-empty, distinct, space-joined.
fn join_text(xs: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    for x in xs {
        let c = clean(x);
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    }
    out.join(" ")
}

/// A KCL electrical connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conn {
    pub amps: String,
    pub kw: String,
    pub hp: String,
    pub volts: String,
    pub phase: String,
    pub hz: String,
    pub conn: String,
    pub nema: String,
}
/// `decodeElec(s)`
pub fn decode_elec(s: &str) -> Vec<Conn> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split('^')
        .map(|c| {
            let f: Vec<String> = c.split('~').map(|v| crate::js::trim(v).to_string()).collect();
            let g = |i: usize| f.get(i).cloned().unwrap_or_default();
            Conn {
                amps: g(0),
                kw: g(1),
                hp: g(2),
                volts: g(3),
                phase: g(4),
                hz: g(5),
                conn: if !g(6).is_empty() {
                    "direct".into()
                } else if !g(7).is_empty() {
                    "cord and plug".into()
                } else {
                    String::new()
                },
                nema: g(8),
            }
        })
        .filter(|c| [&c.amps, &c.kw, &c.hp, &c.volts, &c.phase, &c.hz, &c.conn, &c.nema].iter().any(|x| !x.is_empty()))
        .collect()
}
fn inches(v: &str) -> String {
    let x = crate::js::str_num(v);
    if !x.is_finite() || x <= 0.0 {
        return v.to_string();
    }
    let whole = x.floor();
    let eighths = m::round((x - whole) * 8.0);
    if eighths == 8.0 {
        return format!("{}\"", num_str(whole + 1.0));
    }
    let f = ["", "1/8", "1/4", "3/8", "1/2", "5/8", "3/4", "7/8"]
        .iter()
        .zip(0..8)
        .find(|(_, k)| f64::from(*k) == eighths)
        .map_or("", |(f, _)| *f);
    let w = if whole != 0.0 { format!("{}{}", num_str(whole), if f.is_empty() { "" } else { " " }) } else { String::new() };
    format!("{w}{f}\"")
}
/// `plumbSize(s)`
pub fn plumb_size(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let sizes: Vec<String> =
        s.split('^').map(|c| crate::js::trim(c.split('~').next().unwrap_or("")).to_string()).filter(|x| !x.is_empty()).collect();
    let mut counts: Vec<(String, usize)> = Vec::new();
    for z in sizes {
        match counts.iter_mut().find(|(k, _)| *k == z) {
            Some(c) => c.1 = c.1.saturating_add(1),
            None => counts.push((z, 1)),
        }
    }
    counts
        .iter()
        .map(|(z, k)| format!("{}{}", if *k > 1 { format!("({k}) ") } else { String::new() }, inches(z)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `parseLength(s, mmPerUnit)`: "30", 30" , 2'-6", 2' 6 1/2", 762 mm, 76.2 cm, 0.76 m → mm; NaN if not a length.
pub fn parse_length(s: &str, mm_per_unit: f64) -> f64 {
    let t: String = crate::js::trim(s)
        .chars()
        .map(|c| match c {
            '\u{2032}' | '\u{2019}' => '\'',
            '\u{2033}' | '\u{201d}' => '"',
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    if t.is_empty() {
        return f64::NAN;
    }
    static R1: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    static R2: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    static R3: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let r1 = R1.get_or_init(|| regex::Regex::new(r"^(-?[0-9]+(?:\.[0-9]+)?)\s*(mm|cm|m|in|ft)$").ok());
    let r2 = R2.get_or_init(|| {
        regex::Regex::new(
            r#"^(?:([0-9]+(?:\.[0-9]+)?)\s*'\s*-?\s*)?(?:([0-9]+(?:\.[0-9]+)?)(?:\s+([0-9]+)/([0-9]+))?|([0-9]+)/([0-9]+))?\s*(?:"|in)?$"#,
        )
        .ok()
    });
    let r3 = R3.get_or_init(|| regex::Regex::new(r"^-?[0-9]+(\.[0-9]+)?$").ok());
    let num = |x: Option<regex::Match<'_>>| x.map_or(0.0, |m| crate::js::str_num(m.as_str()));
    if let Some(c) = r1.as_ref().and_then(|r| r.captures(&t)) {
        return num(c.get(1)) * c.get(2).and_then(|u| unit_mm(u.as_str())).unwrap_or(f64::NAN);
    }
    if let Some(c) = r2.as_ref().and_then(|r| r.captures(&t))
        && (c.get(1).is_some() || c.get(2).is_some() || c.get(5).is_some())
        && (t.contains('\'') || t.contains('"'))
    {
        let ft = num(c.get(1));
        let mut inch = num(c.get(2));
        if c.get(3).is_some() {
            inch += num(c.get(3)) / num(c.get(4));
        }
        if c.get(5).is_some() {
            inch += num(c.get(5)) / num(c.get(6));
        }
        return (ft * 12.0 + inch) * 25.4;
    }
    if r3.as_ref().is_some_and(|r| r.is_match(&t)) {
        return crate::js::str_num(&t) * mm_per_unit;
    }
    f64::NAN
}

/// Attributes upper-cased, values cleaned (insertion order kept: later duplicates overwrite).
pub type Attrs = Vec<(String, String)>;
fn upper(attrs: &Attrs) -> Attrs {
    let mut a: Attrs = Vec::new();
    for (k, v) in attrs {
        let k = k.to_uppercase();
        let v = clean(v);
        match a.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => a.push((k, v)),
        }
    }
    a
}
fn aget<'a>(a: &'a Attrs, k: &str) -> &'a str {
    a.iter().find(|(x, _)| x == k).map_or("", |(_, v)| v.as_str())
}
fn pick<'a>(a: &'a Attrs, names: &[&str]) -> &'a str {
    names.iter().map(|n| aget(a, n)).find(|v| !v.is_empty()).unwrap_or("")
}

/// `attrsInfo(attrs)`
pub struct Info {
    pub maker: String,
    pub model: String,
    pub title: String,
    pub item_no: String,
    pub qty: String,
}
pub fn attrs_info(attrs: &Attrs) -> Info {
    let a = upper(attrs);
    Info {
        maker: pick(&a, &["KCL-MFR-SCHDNAME", "KCL-MFR", "MANUFACTURER", "MAKER", "MFR"]).into(),
        model: pick(&a, &["KCL-MODEL", "MODEL", "MODEL NUMBER", "MODELNUMBER"]).into(),
        title: pick(&a, &["KCL-CATEGORY", "DESCRIPTION", "NAME", "TITLE"]).into(),
        item_no: pick(&a, &["KCL-ITEM", "ITEM", "ITEMNO", "ITEM NUMBER"]).into(),
        qty: pick(&a, &["KCL-QTY", "QTY", "QUANTITY"]).into(),
    }
}
/// Attribute names that belong to one placement, not to the item.
pub const PLACEMENT_ATTRS: [&str; 2] = ["KCL-ITEM", "KCL-QTY"];

/// `paramsFromAttrs(attrs, { mmPerUnit })`
pub fn params_from_attrs(attrs: &Attrs, mm_per_unit: f64) -> Map<String, J> {
    let a = upper(attrs);
    let mut p = Map::new();
    let len = |v: &str| {
        let x = parse_length(v, mm_per_unit);
        (x.is_finite() && x > 0.0).then(|| r4(x))
    };
    let mut dims = [
        pick(&a, &["WIDTH", "W", "KCL-WIDTH", "LENGTH"]).to_string(),
        pick(&a, &["DEPTH", "D", "KCL-DEPTH"]).to_string(),
        pick(&a, &["HEIGHT", "H", "KCL-HEIGHT"]).to_string(),
    ];
    let dim = {
        let x = aget(&a, "KCL-EQUIP-DIM");
        if x.is_empty() { aget(&a, "DIMENSIONS") } else { x }
    };
    if !dim.is_empty() {
        static RX: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
        let rx = RX.get_or_init(|| regex::Regex::new(r"(?i)\s*[x×]\s*").ok());
        if let Some(rx) = rx {
            let parts: Vec<&str> = rx.split(dim).collect();
            if let [w, d, h] = parts.as_slice() {
                for (slot, v) in dims.iter_mut().zip([w, d, h]) {
                    if slot.is_empty() {
                        *slot = (*v).to_string();
                    }
                }
            }
        }
    }
    for (k, v) in ["width", "depth", "height"].iter().zip(dims.iter()) {
        if !v.is_empty()
            && let Some(x) = len(v)
        {
            p.insert((*k).into(), n(x));
        }
    }
    let kg = crate::js::str_num(pick(&a, &["WEIGHT_KG", "WEIGHT (KG)"]));
    if kg.is_finite() && kg > 0.0 {
        p.insert("weight_kg".into(), n(kg));
    }
    let conns = decode_elec(aget(&a, "KCL-ELEC"));
    let mut elec: Vec<(&str, String)> = Vec::new();
    if !conns.is_empty() {
        let join = |f: &dyn Fn(&Conn) -> String| {
            let vs: Vec<String> = conns.iter().map(f).collect();
            if vs.iter().any(|v| !v.is_empty()) {
                if vs.len() > 1 {
                    vs.iter().map(|v| if v.is_empty() { "-".to_string() } else { v.clone() }).collect::<Vec<_>>().join(" + ")
                } else {
                    vs.first().cloned().unwrap_or_default()
                }
            } else {
                String::new()
            }
        };
        elec = vec![
            ("volts", join(&|c| c.volts.clone())),
            ("phase", join(&|c| c.phase.clone())),
            ("amps", join(&|c| c.amps.clone())),
            ("kw", join(&|c| c.kw.clone())),
            ("hp", join(&|c| c.hp.clone())),
            ("hz", join(&|c| c.hz.clone())),
            ("nema", join(&|c| c.nema.clone())),
            ("direct", join(&|c| if c.conn == "direct" { "yes".into() } else { String::new() })),
            ("plug", join(&|c| if c.conn == "cord and plug" { "yes".into() } else { String::new() })),
        ];
    }
    for (k, names) in [
        ("volts", &["VOLTS", "VOLTAGE"][..]),
        ("phase", &["PHASE"][..]),
        ("amps", &["AMPS"][..]),
        ("kw", &["KW"][..]),
        ("hp", &["HP"][..]),
        ("hz", &["HZ"][..]),
        ("nema", &["NEMA"][..]),
        ("plug", &["PLUG"][..]),
    ] {
        let have = elec.iter().find(|(x, _)| *x == k).is_some_and(|(_, v)| !v.is_empty());
        if !have {
            let v = pick(&a, names);
            if !v.is_empty() {
                match elec.iter_mut().find(|(x, _)| *x == k) {
                    Some(slot) => slot.1 = clip500(v),
                    None => elec.push((k, clip500(v))),
                }
            }
        }
    }
    let e2: Map<String, J> = elec.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.to_string(), s(&v))).collect();
    if !e2.is_empty() {
        p.insert("elec".into(), J::Object(e2));
    }
    let mut plumb = Map::new();
    for k in PLUMB_KEYS {
        let kcl = aget(&a, &format!("KCL-{}", k.to_uppercase()));
        let v = if kcl.is_empty() { aget(&a, &k.to_uppercase()).to_string() } else { plumb_size(kcl) };
        if !v.is_empty() {
            plumb.insert(k.into(), s(&clip500(&v)));
        }
    }
    if !plumb.is_empty() {
        p.insert("plumb".into(), J::Object(plumb));
    }
    let remarks = join_text(&[aget(&a, "KCL-REMARK-USER-EQUIP"), aget(&a, "KCL-REMARK"), aget(&a, "REMARKS"), aget(&a, "REMARK")]);
    if !remarks.is_empty() {
        p.insert("remarks".into(), s(&clip500(&remarks)));
    }
    p
}

/// `sourceAttrs(attrs)`: verbatim, minus the per-placement ones; empty values dropped.
pub fn source_attrs(attrs: &Attrs) -> Map<String, J> {
    let mut out = Map::new();
    for (k, v) in attrs {
        if PLACEMENT_ATTRS.contains(&k.to_uppercase().as_str()) {
            continue;
        }
        if crate::js::trim(v).is_empty() {
            continue;
        }
        let v = if len16(v) > LIMIT_TEXT { slice16(v, LIMIT_TEXT) } else { v.clone() };
        out.insert(slice16(k, 255), s(&v));
    }
    out
}

/// `suggestCategory(text)`
pub fn suggest_category(text: &str) -> Option<&'static str> {
    static RULES: std::sync::OnceLock<Vec<(Option<fancy_regex::Regex>, &'static str)>> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            (r"sink|faucet|lavator", "foodservice/sinks"),
            (r"refrig|freezer|cooler|walk-?in|reach-?in|chiller", "foodservice/refrigeration"),
            (r"range|fryer|oven|griddle|broiler|kettle|steamer|cooktop|grill|braising|charbroil", "foodservice/cooking"),
            (r"shelf|shelving|rack(?! conveyor)", "foodservice/shelving"),
            (r"dish|warewash|disposer|rack conveyor", "foodservice/warewashing"),
            (r"hood|ventilat|exhaust", "foodservice/ventilation"),
            (r"table|counter", "foodservice/worktables"),
            (r"warm|holding|heated|hot well|food well", "foodservice/holding"),
            (r"ice", "foodservice/ice"),
            (r"beverage|coffee|tea|soda|dispenser", "foodservice/beverage"),
            (r"mixer|slicer|prep|scale|processor", "foodservice/prep"),
            (r"serving|display|sneeze|buffet|cashier|tray", "foodservice/serving"),
        ]
        .into_iter()
        .map(|(r, k)| (fancy_regex::Regex::new(r).ok(), k))
        .collect()
    });
    let t = text.to_lowercase();
    rules.iter().find(|(r, _)| r.as_ref().is_some_and(|r| r.is_match(&t).unwrap_or(false))).map(|(_, k)| *k)
}

// ---------------------------------------------------------------------------------------------------------- CSV
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let s = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut cell, mut q) = (Vec::new(), String::new(), false);
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs.get(i).copied().unwrap_or('\0');
        let next = cs.get(i.saturating_add(1)).copied();
        if q {
            if c == '"' {
                if next == Some('"') {
                    cell.push('"');
                    i = i.saturating_add(1);
                } else {
                    q = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' {
            q = true;
        } else if c == ',' {
            row.push(std::mem::take(&mut cell));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && next == Some('\n') {
                i = i.saturating_add(1);
            }
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
        } else {
            cell.push(c);
        }
        i = i.saturating_add(1);
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows.into_iter().filter(|r| r.iter().any(|x| !crate::js::trim(x).is_empty())).collect()
}

/// `itemsFromCsv(text, { category, kind })`: spec rows → box items.
pub fn items_from_csv(text: &str, category: Option<&str>, kind: Option<&str>) -> Read {
    let rows = parse_csv(text);
    let mut out = Read::default();
    if rows.len() < 2 {
        out.notes.push(msg("libitem.csv.no_rows", &[]));
        return out;
    }
    let head: Vec<(String, Option<String>)> = rows
        .first()
        .map(|r| {
            r.iter()
                .map(|h| {
                    let t = crate::js::trim(h).to_lowercase();
                    // /^(.*?)\s*(?:\((in|mm|cm|m|ft)\))?$/
                    let mut base = t.as_str();
                    let mut unit = None;
                    for u in ["in", "mm", "cm", "m", "ft"] {
                        let suf = format!("({u})");
                        if let Some(b) = t.strip_suffix(&suf) {
                            base = b;
                            unit = Some(u.to_string());
                            break;
                        }
                    }
                    let base = base.trim_end_matches(|c: char| crate::js::trim(&c.to_string()).is_empty());
                    let key: String = {
                        let mut k = String::new();
                        let mut prev = false;
                        for c in base.chars() {
                            let sep = c == '-' || crate::js::trim(&c.to_string()).is_empty();
                            if sep {
                                if !prev {
                                    k.push('_');
                                }
                                prev = true;
                            } else {
                                k.push(c);
                                prev = false;
                            }
                        }
                        k
                    };
                    (key, unit)
                })
                .collect()
        })
        .unwrap_or_default();
    let col = |name: &str| head.iter().position(|(k, _)| k == name);
    if col("name").is_none() {
        out.notes.push(msg("libitem.csv.no_name_column", &[]));
        return out;
    }
    for (ni, r) in rows.iter().enumerate().skip(1) {
        let line = ni.saturating_add(1);
        let get = |name: &str| col(name).map_or(String::new(), |i| clean(r.get(i).map_or("", String::as_str)));
        let unit_of = |name: &str| {
            let u = col(name).and_then(|i| head.get(i)).and_then(|h| h.1.clone());
            u.unwrap_or_else(|| get("units").to_lowercase())
        };
        let mut notes: Vec<String> = Vec::new();
        let mut mm = |name: &str| -> Option<f64> {
            let raw = get(name);
            if raw.is_empty() {
                return None;
            }
            let u = unit_of(name);
            let bare = {
                let t = raw.strip_prefix('-').unwrap_or(&raw);
                let (a, b) = t.split_once('.').unwrap_or((t, ""));
                !a.is_empty()
                    && a.bytes().all(|c| c.is_ascii_digit())
                    && (!t.contains('.') || (!b.is_empty() && b.bytes().all(|c| c.is_ascii_digit())))
            };
            if u.is_empty() && bare {
                notes.push(msg("libitem.csv.no_units", &[("row", line.to_string()), ("column", name.to_string())]));
                return Some(f64::NAN);
            }
            let v = parse_length(&raw, unit_mm(&u).unwrap_or(1.0));
            if !v.is_finite() {
                notes.push(msg(
                    "libitem.csv.not_length",
                    &[("row", line.to_string()), ("column", name.to_string()), ("value", raw.clone())],
                ));
                return Some(f64::NAN);
            }
            Some(v)
        };
        let name = get("name");
        if name.is_empty() {
            out.notes.push(msg("libitem.csv.no_name", &[("row", line.to_string())]));
            continue;
        }
        let (w, d, h, z) = (mm("width"), mm("depth"), mm("height"), mm("z"));
        out.notes.append(&mut notes);
        let pos = |x: Option<f64>| x.filter(|v| *v > 0.0);
        let (Some(w), Some(d)) = (pos(w), pos(d)) else {
            out.notes.push(msg("libitem.csv.no_size", &[("row", line.to_string()), ("name", name.clone())]));
            continue;
        };
        let mut it = base_item(&name);
        it.insert("maker".into(), s(&clip500(&get("maker"))));
        it.insert("model".into(), s(&clip500(&get("model"))));
        it.insert("description".into(), s(&clip500(&get("description"))));
        let cat = {
            let c = get("category");
            if c.is_empty() { category.unwrap_or("").to_string() } else { c }
        };
        let set_cat;
        if !cat.is_empty() && CATEGORY_KEYS.contains(&cat.as_str()) {
            set_cat = cat.clone();
        } else {
            set_cat = suggest_category(&name).unwrap_or("other/other").to_string();
            out.notes.push(if cat.is_empty() {
                msg("libitem.csv.category_missing", &[("row", line.to_string()), ("name", name.clone()), ("set", set_cat.clone())])
            } else {
                msg(
                    "libitem.csv.category_unknown",
                    &[("row", line.to_string()), ("name", name.clone()), ("category", cat.clone()), ("set", set_cat.clone())],
                )
            });
        }
        it.insert("category".into(), s(&set_cat));
        let k = {
            let x = get("kind");
            if x.is_empty() { kind.unwrap_or("").to_string() } else { x }
        };
        let kind_v = if KINDS.contains(&k.as_str()) {
            k
        } else if set_cat.starts_with("furniture") || set_cat.starts_with("casework") {
            "furniture".into()
        } else {
            "equipment".into()
        };
        it.insert("kind".into(), s(&kind_v));
        let cls = get("ifc_class");
        if is_ifc_class(&cls)
            && let Some(ifc) = obj_mut(&mut it, "ifc")
        {
            ifc.insert("class".into(), s(&cls));
        }
        let hv = h.filter(|v| *v > 0.0).unwrap_or(0.0);
        box_item_geometry(&mut it, w, d, hv, z.filter(|v| *v > 0.0).unwrap_or(0.0), &name);
        if !h.is_some_and(|v| v > 0.0) {
            out.notes.push(msg("libitem.csv.no_height", &[("row", line.to_string()), ("name", name.clone())]));
        }
        let mut elec = Map::new();
        let mut plumb = Map::new();
        for f in ELEC_KEYS {
            let v = get(f);
            if !v.is_empty() {
                elec.insert(f.into(), s(&clip500(&v)));
            }
        }
        for f in PLUMB_KEYS {
            let v = get(f);
            if !v.is_empty() {
                plumb.insert(f.into(), s(&clip500(&v)));
            }
        }
        if let Some(p) = obj_mut(&mut it, "params") {
            if !elec.is_empty() {
                p.insert("elec".into(), J::Object(elec));
            }
            if !plumb.is_empty() {
                p.insert("plumb".into(), J::Object(plumb));
            }
            let rem = get("remarks");
            if !rem.is_empty() {
                p.insert("remarks".into(), s(&clip500(&rem)));
            }
        }
        out.items.push(J::Object(it));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_in_every_form() {
        assert_eq!(parse_length("30\"", 1.0), 762.0);
        assert_eq!(parse_length("2'-6\"", 1.0), 762.0);
        assert!((parse_length("2' 6 1/2\"", 1.0) - 774.7).abs() < 1e-9);
        assert_eq!(parse_length("762 mm", 1.0), 762.0);
        assert_eq!(parse_length("0.76 m", 1.0), 760.0);
        assert_eq!(parse_length("30", 25.4), 762.0);
        assert!(parse_length("thirty", 1.0).is_nan());
        assert!(parse_length("", 1.0).is_nan());
    }

    #[test]
    fn a_csv_row_makes_a_box_item_that_validates() {
        let r = items_from_csv(
            "Name,Width (in),Depth (in),Height (in),category\nHand sink,17,15.5,34,plumbing/fixtures\n,1,1,1,\n",
            None,
            None,
        );
        assert_eq!(r.items.len(), 1);
        let it = r.items.first().cloned().unwrap_or(J::Null);
        assert_eq!(it.get("category").and_then(J::as_str), Some("plumbing/fixtures"));
        assert!(validate_item(&it, true).0, "{:?}", validate_item(&it, true).1);
        assert!(!validate_item(&it, false).0);
        assert_eq!(item_hash(&it).len(), 64);
        assert!(r.notes.iter().any(|n| n.contains("Row 3")));
    }

    #[test]
    fn hostile_text_and_json_do_not_panic() {
        let _ = items_from_csv("\"unterminated,\r\n\r", None, None);
        let _ = validate_item(
            &serde_json::json!({ "plan": { "entities": [{ "type": "constructor" }, 5, { "type": "poly", "pts": [[1], "x"] }] }, "body": { "type": "mesh", "positions": [1, 2], "indices": [9.5] } }),
            false,
        );
        let _ = mesh::item_from_mesh(b"glTF\x02\x00\x00\x00\xff\xff\xff\xff", "glb", None, None, "");
        let _ = mesh::item_from_mesh(&[0u8; 84], "stl", None, None, "");
        let _ = rfa::read_revit_family(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0]);
        assert!(convex_hull(&[[0.0, 0.0], [1.0, 0.0]]).len() <= 2);
    }
}
