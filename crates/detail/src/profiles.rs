//! profiles.js — steel profile catalogs (the shared section table) and parametric section shapes: `CATALOG`,
//! `getProfile` (catalog, `PL<t>x<w>` plates, a model's custom profiles), `cleanCustom` (an untrusted custom profile),
//! `catalogGroups`, `holeFaces`, `faceThickness`, `describe`. Millimetres; sections in the (u, v) plane.

use crate::i18n::{F, t, t0};
use crate::js::{Obj, Res, clean_text, fixed_num, num, num_to_string, s, str_to_number, trim, upper};
use crate::sections::{self, Loop, STEEL_DENSITY, Shape, bbox, exact_area, loop_length, outline};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// A resolved profile: the record plus what `withDerived` adds.
#[derive(Clone, Debug)]
pub struct Profile {
    pub s: Shape,
    pub loops: Vec<Loop>,
    pub area: f64,
    pub mass: f64,
    pub mass_from: String,
    pub width: f64,
    pub depth: f64,
    pub perimeter: f64,
}
impl std::ops::Deref for Profile {
    type Target = Shape;
    fn deref(&self) -> &Shape {
        &self.s
    }
}
pub type P = Arc<Profile>;

/// `SOURCES[key]`.
pub fn source_title(key: &str) -> Option<String> {
    match key {
        "aisc" => Some("AISC Shapes Database v16.0".into()),
        "en" => Some("EN 10365:2017".into()),
        "param" => Some(t0("prof.source.param")),
        "custom" => Some(t0("prof.source.custom")),
        _ => None,
    }
}

const LEGACY: [&str; 45] = [
    "W8X10",
    "W8X31",
    "W10X33",
    "W10X49",
    "W12X26",
    "W12X40",
    "W12X50",
    "W14X22",
    "W14X30",
    "W14X90",
    "W16X26",
    "W16X31",
    "W18X35",
    "W21X44",
    "W24X55",
    "C8X11.5",
    "C10X15.3",
    "C12X20.7",
    "L3X3X1/4",
    "L4X4X3/8",
    "L4X4X1/2",
    "L5X5X1/2",
    "L6X6X1/2",
    "HSS4X4X1/4",
    "HSS6X4X1/4",
    "HSS6X6X1/4",
    "HSS8X8X3/8",
    "HSS8X8X1/2",
    "HSS4.500X0.237",
    "HSS6.625X0.280",
    "IPE100",
    "IPE160",
    "IPE200",
    "IPE240",
    "IPE300",
    "IPE360",
    "IPE400",
    "HEA160",
    "HEA200",
    "HEA240",
    "HEA300",
    "HEB160",
    "HEB200",
    "HEB240",
    "HEB300",
];
fn legacy(key: &str) -> bool {
    LEGACY.contains(&key)
}
/// `isLegacyName(name)`.
pub fn is_legacy_name(name: &str) -> bool {
    legacy(&upper(trim(name)))
}

struct Catalog {
    rows: Vec<&'static Shape>,
    by: HashMap<String, usize>,
    derived: Vec<OnceLock<P>>,
}
fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| {
        let mut rows = Vec::new();
        let mut by = HashMap::new();
        for p in sections::section_rows() {
            if p.kind == "L" && p.h.is_some() {
                continue;
            }
            let k = upper(&p.name);
            match by.get(&k) {
                Some(&i) => {
                    // a Map keeps the first key's place and takes the later value
                    if let Some(slot) = rows.get_mut(i) {
                        *slot = p;
                    }
                }
                None => {
                    by.insert(k, rows.len());
                    rows.push(p);
                }
            }
        }
        let derived = rows.iter().map(|_| OnceLock::new()).collect();
        Catalog { rows, by, derived }
    })
}
/// `CATALOG` (values, in order).
pub fn catalog_rows() -> &'static [&'static Shape] {
    &catalog().rows
}
/// `CATALOG.has(key)` (key upper-case).
pub fn catalog_has(key: &str) -> bool {
    catalog().by.contains_key(key)
}
/// `CATALOG.get(key)` resolved (`withDerived`, cached as the JS caches frozen rows).
pub fn catalog_get(key: &str) -> Option<P> {
    let c = catalog();
    let &i = c.by.get(key)?;
    let row = c.rows.get(i)?;
    Some(c.derived.get(i)?.get_or_init(|| Arc::new(with_derived((*row).clone()))).clone())
}

/// One catalog group for pickers.
#[derive(Clone, Debug)]
pub struct Group {
    pub label: String,
    pub source: String,
    pub names: Vec<String>,
}
/// `catalogGroups(custom)`.
pub fn catalog_groups(custom: &[Value]) -> Vec<Group> {
    let fams: [(String, &str); 10] = [
        (t0("prof.group.w"), "W"),
        (t0("prof.group.c"), "C"),
        (t0("prof.group.l"), "L"),
        ("HSS".into(), "HSS"),
        ("IPE".into(), "IPE"),
        ("HEA".into(), "HEA"),
        ("HEB".into(), "HEB"),
        (t0("prof.group.s"), "S"),
        (t0("prof.group.pipe"), "Pipe"),
        ("UPN".into(), "UPN"),
    ];
    let mut out = Vec::new();
    for (label, fam) in fams {
        let names: Vec<String> = catalog_rows().iter().filter(|p| p.family == fam).map(|p| p.name.clone()).collect();
        let src = names.first().and_then(|n| catalog().by.get(&upper(n))).and_then(|&i| catalog().rows.get(i)).map(|p| p.source.clone());
        out.push(Group { label, source: src.as_deref().and_then(source_title).unwrap_or_default(), names });
    }
    if !custom.is_empty() {
        out.push(Group {
            label: t0("prof.group.custom"),
            source: source_title("custom").unwrap_or_default(),
            names: custom.iter().map(|c| crate::js::string_of(c.get("name"))).collect(),
        });
    }
    out
}

/// `num(v)` = `String(Number(Number(v).toFixed(2)))`.
fn num2(v: f64) -> String {
    num_to_string(fixed_num(v, 2))
}
/// `plateName(t, w)`.
pub fn plate_name(tk: f64, w: f64) -> String {
    format!("PL{}x{}", num2(tk), num2(w))
}

fn is_js_ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}
/// `PL_RE.exec(s)`: /^PL\s*(\d+(?:\.\d+)?)\s*[xX*]\s*(\d+(?:\.\d+)?)$/ → the two numbers' texts.
pub fn pl_match(s: &str) -> Option<(String, String)> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let at = |i: usize| cs.get(i).copied();
    if at(0) != Some('P') || at(1) != Some('L') {
        return None;
    }
    i = i.saturating_add(2);
    let ws = |i: &mut usize| {
        while at(*i).is_some_and(is_js_ws) {
            *i = i.saturating_add(1);
        }
    };
    // \d+(?:\.\d+)? — greedy, with the fraction only when a digit follows the point
    let number = |i: &mut usize| -> Option<String> {
        let st = *i;
        while at(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i = i.saturating_add(1);
        }
        if *i == st {
            return None;
        }
        if at(*i) == Some('.') && at(i.saturating_add(1)).is_some_and(|c| c.is_ascii_digit()) {
            *i = i.saturating_add(1);
            while at(*i).is_some_and(|c| c.is_ascii_digit()) {
                *i = i.saturating_add(1);
            }
        }
        Some(cs.get(st..*i).unwrap_or(&[]).iter().collect())
    };
    ws(&mut i);
    let a = number(&mut i)?;
    ws(&mut i);
    if !matches!(at(i), Some('x' | 'X' | '*')) {
        return None;
    }
    i = i.saturating_add(1);
    ws(&mut i);
    let b = number(&mut i)?;
    if i != cs.len() {
        return None;
    }
    Some((a, b))
}

/// `CUSTOM_KINDS[kind]`.
pub fn custom_kind_keys(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "I" => Some(&["d", "bf", "tw", "tf", "r"]),
        "C" => Some(&["d", "bf", "tw", "tf"]),
        "L" => Some(&["b", "t"]),
        "RHS" => Some(&["h", "b", "t"]),
        "CHS" => Some(&["D", "t"]),
        "FLAT" => Some(&["b", "t"]),
        _ => None,
    }
}

fn set_dim(p: &mut Shape, k: &str, v: f64) {
    match k {
        "d" => p.d = Some(v),
        "bf" => p.bf = Some(v),
        "tw" => p.tw = Some(v),
        "tf" => p.tf = Some(v),
        "r" => p.r = Some(v),
        "b" => p.b = Some(v),
        "t" => p.t = Some(v),
        "h" => p.h = Some(v),
        "ro" => p.ro = Some(v),
        "ri" => p.ri = Some(v),
        "D" => p.dd = Some(v),
        "tNom" => p.t_nom = Some(v),
        _ => {}
    }
}

/// `cleanCustom(c, {saved})` → the stored custom profile object, or the error text.
pub fn clean_custom(c: &Value, saved: bool) -> Res<Value> {
    let Some(o) = c.as_object() else {
        // arrays are objects to the JS: they go on and fail on the name
        if !c.is_array() {
            return Err(t0("prof.err.not-profile"));
        }
        return Err(t0("prof.err.name"));
    };
    let name = clean_name(o.get("name"));
    if name.is_empty() {
        return Err(t0("prof.err.name"));
    }
    let key = upper(&name);
    if (catalog_has(&key) && (!saved || legacy(&key))) || pl_match(&name).is_some() {
        return Err(t("prof.err.catalog-name", &[("name", F::from(name.as_str()))]));
    }
    let kind = match o.get("kind") {
        None | Some(Value::Null) => String::new(),
        Some(v) if !crate::js::truthy(Some(v)) => String::new(),
        Some(v) => crate::js::to_js_string(v),
    };
    let Some(keys) = custom_kind_keys(&kind) else { return Err(t("prof.err.shape", &[("kind", F::from(kind.as_str()))])) };
    let mut out = Map::new();
    out.insert("name".into(), Value::from(name.clone()));
    out.insert("kind".into(), Value::from(kind.clone()));
    out.insert("family".into(), Value::from("custom"));
    out.insert("source".into(), Value::from("custom"));
    let mut v_of = HashMap::new();
    for &k in keys {
        let v = num(o.get(k));
        if !v.is_finite() || v < 0.0 || v > 5000.0 || (k != "r" && v == 0.0) {
            return Err(t("prof.err.size", &[("name", F::from(k))]));
        }
        out.insert(k.into(), Value::from(v));
        v_of.insert(k, v);
    }
    let gv = |k: &str| v_of.get(k).copied().unwrap_or(f64::NAN);
    if kind == "I" || kind == "C" {
        if gv("tw") >= gv("bf") || 2.0 * gv("tf") >= gv("d") {
            return Err(t0("prof.err.web-flanges"));
        }
        if kind == "I" && gv("tw") / 2.0 + gv("r") > gv("bf") / 2.0 {
            return Err(t0("prof.err.fillet-flange"));
        }
        if kind == "I" && 2.0 * (gv("tf") + gv("r")) >= gv("d") {
            return Err(t0("prof.err.fillet-depth"));
        }
    }
    if kind == "L" && gv("t") >= gv("b") {
        return Err(t0("prof.err.leg"));
    }
    if kind == "RHS" {
        if 2.0 * gv("t") >= crate::js::min(gv("h"), gv("b")) {
            return Err(t0("prof.err.wall"));
        }
        out.insert("ro".into(), Value::from(2.0 * gv("t")));
        out.insert("ri".into(), Value::from(gv("t")));
    }
    if kind == "CHS" && 2.0 * gv("t") >= gv("D") {
        return Err(t0("prof.err.wall"));
    }
    Ok(Value::Object(out))
}

/// `String(c.name || '').replace(controls, '').trim().slice(0, 40)`.
fn clean_name(v: Option<&Value>) -> String {
    let raw = if crate::js::truthy(v) { crate::js::string_or_empty(v) } else { String::new() };
    clean_text(Some(&Value::String(raw)), 40)
}

/// A custom profile object (as stored) as a record.
fn shape_of_custom(c: &Map<String, Value>, kind_override: Option<&str>) -> Shape {
    let mut p = Shape {
        name: crate::js::string_of(c.get("name")),
        family: crate::js::string_of(c.get("family")),
        kind: kind_override.map_or_else(|| crate::js::string_of(c.get("kind")), str::to_string),
        source: crate::js::string_of(c.get("source")),
        ..Shape::default()
    };
    for (k, v) in c {
        if let Some(x) = v.as_f64() {
            set_dim(&mut p, k, x);
        }
        match k.as_str() {
            "areaPub" => p.area_pub = v.as_f64(),
            "massPub" => p.mass_pub = v.as_f64(),
            _ => {}
        }
    }
    p
}

/// `getProfile(name, custom)`: null for anything unknown (or a name that is not a string).
pub fn get_profile(name: &Value, custom: &[Value]) -> Option<P> {
    get_profile_str(name.as_str()?, custom)
}
/// `getProfile` for a string name.
pub fn get_profile_str(name: &str, custom: &[Value]) -> Option<P> {
    let key = upper(trim(name));
    let find = || custom.iter().filter_map(Value::as_object).find(|x| upper(s(x.get("name"))) == key);
    let own = if legacy(&key) { None } else { find() };
    if own.is_none()
        && let Some(p) = catalog_get(&key)
    {
        return Some(p);
    }
    if let Some((a, b)) = pl_match(trim(name)) {
        let (tk, w) = (str_to_number(&a), str_to_number(&b));
        if tk > 0.0 && w > 0.0 && tk <= 500.0 && w <= 5000.0 {
            let mut p =
                Shape { name: plate_name(tk, w), family: "PL".into(), kind: "PL".into(), source: "param".into(), ..Shape::default() };
            p.t = Some(tk);
            p.b = Some(w);
            return Some(Arc::new(with_derived(p)));
        }
        return None;
    }
    let c = find()?;
    let flat = s(c.get("kind")) == "FLAT";
    Some(Arc::new(with_derived(shape_of_custom(c, if flat { Some("PL") } else { None }))))
}

fn with_derived(p: Shape) -> Profile {
    let loops = outline(&p, 6);
    let area = exact_area(&p);
    let mass = p.mass_pub.unwrap_or(area * 1e-6 * STEEL_DENSITY);
    let mass_from = if p.mass_pub.is_some() {
        source_title(&p.source).unwrap_or_else(|| "undefined".into())
    } else {
        format!("Area × {} kg/m³", num_to_string(STEEL_DENSITY))
    };
    let first = loops.first().map_or(&[][..], Vec::as_slice);
    let bb = bbox(first);
    let perimeter = loop_length(first);
    Profile { s: p, loops, area, mass, mass_from, width: bb.w, depth: bb.h, perimeter }
}

/// `holeFaces(p)`.
pub fn hole_faces(p: Option<&Profile>) -> Vec<&'static str> {
    match p.map(|p| p.kind.as_str()) {
        Some("I" | "C" | "RHS") => vec!["web", "top", "bottom"],
        Some("PL") => vec!["plate"],
        _ => vec![],
    }
}

/// `faceThickness(p, face)` (undefined when the profile has no such dimension).
pub fn face_thickness(p: &Profile, face: &str) -> Option<f64> {
    if p.kind == "PL" || p.kind == "L" {
        return p.t;
    }
    if face == "web" {
        return if p.kind == "RHS" { p.b } else { p.tw };
    }
    if p.kind == "RHS" {
        return p.t;
    }
    p.tf
}

/// `describe(p)` → [[label, value]].
pub fn describe(p: &Profile) -> Vec<[String; 2]> {
    let kind_key = match p.kind.as_str() {
        "I" => Some("i"),
        "C" => Some("c"),
        "L" => Some("l"),
        "RHS" => Some("rhs"),
        "CHS" => Some("chs"),
        "PL" => Some("pl"),
        _ => None,
    };
    let l = |k: &str| t0(&format!("prof.row.{k}"));
    let mm = |v: f64| format!("{} mm", num_to_string(fixed_num(v, 1)));
    let mut rows = vec![[l("shape"), kind_key.map_or_else(|| p.kind.clone(), |k| t0(&format!("prof.kind.{k}")))]];
    if p.kind == "I" || p.kind == "C" {
        rows.extend([[l("depth"), mm(p.d())], [l("flange-width"), mm(p.bf())], [l("web"), mm(p.tw())], [l("flange"), mm(p.tf())]]);
    }
    if p.kind == "I" {
        rows.push([l("fillet"), format!("{}{}", mm(p.r()), if p.source == "aisc" { " (kdes − tf)" } else { "" })]);
    }
    if p.kind == "L" {
        rows.extend([[l("legs"), format!("{} × {}", mm(p.b()), mm(p.b()))], [l("thickness"), mm(p.t())]]);
    }
    if p.kind == "RHS" {
        rows.extend([[l("size"), format!("{} × {}", mm(p.h.unwrap_or(f64::NAN)), mm(p.b()))], [l("wall"), mm(p.t())]]);
    }
    if p.kind == "CHS" {
        rows.extend([[l("diameter"), mm(p.big_d())], [l("wall"), mm(p.t())]]);
    }
    if p.kind == "PL" {
        rows.extend([[l("thickness"), mm(p.t())], [l("width"), mm(p.b())]]);
    }
    rows.push([l("area"), format!("{} cm²", num_to_string(fixed_num(p.area / 100.0, 2)))]);
    rows.push([l("mass"), format!("{} kg/m", num_to_string(fixed_num(p.mass, 2)))]);
    rows.push([l("from"), source_title(&p.source).unwrap_or_default()]);
    rows
}

/// The JSON form of a resolved profile (every field the JS object has; `loops` included).
pub fn profile_json(p: &Profile, with_loops: bool) -> Value {
    let mut o = Obj::new()
        .set("name", p.name.as_str())
        .set("family", p.family.as_str())
        .set("kind", p.kind.as_str())
        .set("source", p.source.as_str());
    for (k, v) in p.dims() {
        o.put(k, v);
    }
    if let Some(a) = p.area_pub {
        o.put("areaPub", a);
    }
    if let Some(m) = p.mass_pub {
        o.put("massPub", m);
    }
    if let Some(u) = p.pub_units {
        let mut po = Obj::new().set("units", u);
        for (k, v) in &p.pub_vals {
            po.put(k, *v);
        }
        o.put("pub", po.done());
    }
    if with_loops {
        let loops: Vec<Value> =
            p.loops.iter().map(|l| Value::Array(l.iter().map(|q| Value::Array(vec![q[0].into(), q[1].into()])).collect())).collect();
        o.put("loops", Value::Array(loops));
    }
    o.set("area", p.area)
        .set("mass", p.mass)
        .set("massFrom", p.mass_from.as_str())
        .set("width", p.width)
        .set("depth", p.depth)
        .set("perimeter", p.perimeter)
        .set("label", p.name.as_str())
        .done()
}

/// `num` of profiles.js as text (`toFixed(2)` then `String(Number(…))`) — exposed for the plate names others make.
pub fn fmt2(v: f64) -> String {
    num2(v)
}
