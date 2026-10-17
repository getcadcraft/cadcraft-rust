//! `library.js` — the assembly and price library (T9): items with their rates and assemblies, read from CSV, kept by
//! the app and reused across takeoffs; `csvCell` / `csvLine` of `export.js` live here too (the library writes CSV).

use serde_json::{Value, json};

use crate::i18n::{F, t, tf};
use crate::jv::{self, Obj, n, s};
use crate::model::{self, TYPES, limits, normal_component, normal_item};
use crate::units::{self, default_unit, parse_length, parse_percent, parse_rate, round_to, units_for};

pub const LIBRARY_MAX: usize = 5000;

/// A CSV cell value: `null`, a number or text.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Num(f64),
    Str(String),
}
impl From<&str> for Cell {
    fn from(v: &str) -> Self {
        Cell::Str(v.to_string())
    }
}
impl From<String> for Cell {
    fn from(v: String) -> Self {
        Cell::Str(v)
    }
}
impl From<f64> for Cell {
    fn from(v: f64) -> Self {
        Cell::Num(v)
    }
}
impl From<Option<f64>> for Cell {
    fn from(v: Option<f64>) -> Self {
        v.map_or(Cell::Null, Cell::Num)
    }
}
/// A JSON value as a cell (what the JS would pass to `csvCell`).
pub fn cell_of(v: Option<&Value>) -> Cell {
    match v {
        None | Some(Value::Null) => Cell::Null,
        Some(Value::Number(x)) => Cell::Num(x.as_f64().unwrap_or(f64::NAN)),
        Some(other) => Cell::Str(jv::to_string(Some(other))),
    }
}

/// CSV cell: quoted when needed; text that a spreadsheet would run as a formula gets a leading '.
pub fn csv_cell(v: &Cell) -> String {
    match v {
        Cell::Null => String::new(),
        Cell::Num(x) => {
            if x.is_finite() {
                crate::jv::num_str(*x)
            } else {
                String::new()
            }
        }
        Cell::Str(st) => {
            let mut x = st.clone();
            if matches!(x.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r')) {
                x.insert(0, '\'');
            }
            if x.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", x.replace('"', "\"\"")) } else { x }
        }
    }
}
pub fn csv_line(cells: &[Cell]) -> String {
    cells.iter().map(csv_cell).collect::<Vec<_>>().join(",")
}

/// RFC 4180 CSV → rows of strings (',' or ';' or a tab, whichever the header line has most of; a BOM is dropped).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let st = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let first_end = st
        .find('\n')
        .map_or(st.len(), |i| if i > 0 && st.as_bytes().get(i.saturating_sub(1)) == Some(&b'\r') { i.saturating_sub(1) } else { i });
    let first = st.get(..first_end).unwrap_or("");
    let mut counts: Vec<(char, usize)> = [',', ';', '\t'].iter().map(|d| (*d, first.split(*d).count())).collect();
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    let sep = counts.first().map_or(',', |c| c.0);
    let chars: Vec<char> = st.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut q = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or('\0');
        if q {
            if c == '"' {
                if chars.get(i.saturating_add(1)) == Some(&'"') {
                    cell.push('"');
                    i = i.saturating_add(1);
                } else {
                    q = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.is_empty() {
            q = true;
        } else if c == sep {
            row.push(std::mem::take(&mut cell));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.get(i.saturating_add(1)) == Some(&'\n') {
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
    rows.into_iter().filter(|r| r.iter().any(|c| !cc_m3d::js::trim(c).is_empty())).collect()
}

/// `key(s)`: lower case, only letters a–z, digits and characters from U+00C0 up.
pub fn key(st: &str) -> String {
    jv::lower(st).chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || (*c as u32) >= 0xC0).collect()
}

fn header_map() -> Vec<(String, &'static str)> {
    let mut m: Vec<(String, &'static str)> = Vec::new();
    let mut add = |field: &'static str, names: &[&str]| {
        for nm in names {
            if nm.is_empty() {
                continue;
            }
            let k = key(nm);
            if let Some(e) = m.iter_mut().find(|(x, _)| *x == k) {
                e.1 = field;
            } else {
                m.push((k, field));
            }
        }
    };
    add("name", &["item", "name", "description", "itemname", "condition"]);
    add("type", &["type", "kind", "measure"]);
    add("unit", &["unit", "uom", "units"]);
    add("waste", &["waste", "waste%", "wastepercent", "wastepct"]);
    add("mat", &["material", "materialperunit", "materialrate", "materialunit", "mat", "matrate", "materialcost", "materialprice"]);
    add("lab", &["labor", "labour", "laborperunit", "labourperunit", "laborrate", "labourrate", "lab", "laborcost", "labourcost"]);
    add("height", &["height", "depth", "heightdepth", "heightordepth", "thickness"]);
    add("component", &["component", "part", "assemblycomponent"]);
    add("formula", &["formula", "quantityformula", "qtyformula"]);
    add("notes", &["notes", "note", "comments"]);
    for (k, f) in
        [("item", "name"), ("type", "type"), ("unit", "unit"), ("waste", "waste"), ("matu", "mat"), ("labu", "lab"), ("notes", "notes")]
    {
        add(f, &[&t(&format!("x.head.{k}"))]);
    }
    for (k, f) in [
        ("name", "name"),
        ("type", "type"),
        ("unit", "unit"),
        ("waste", "waste"),
        ("mat", "mat"),
        ("lab", "lab"),
        ("height", "height"),
        ("component", "component"),
        ("formula", "formula"),
    ] {
        add(f, &[&t(&format!("lib.head.{k}"))]);
    }
    m
}

fn read_type(v: &str) -> Option<&'static str> {
    let k = key(v);
    if k.is_empty() {
        return None;
    }
    if k == key(&t("x.component")) || k == "component" || k == "part" {
        return Some("component");
    }
    for ty in TYPES {
        if k == key(&t(&format!("type.{ty}"))) || k == ty {
            return Some(ty);
        }
    }
    if ["lf", "length", "len", "lin", "m", "meter", "metre", "linearfeet", "line", "run"].contains(&k.as_str()) {
        return Some("linear");
    }
    if ["sf", "sy", "m2", "m²", "sqft", "surface", "squarefeet", "floor"].contains(&k.as_str()) {
        return Some("area");
    }
    if ["ea", "each", "no", "pcs", "qty", "number", "cnt"].contains(&k.as_str()) {
        return Some("count");
    }
    None
}
fn sq2(k: &str) -> String {
    k.replacen('²', "2", 1)
}
fn read_unit(v: &str, ty: &str) -> Option<&'static str> {
    let k = sq2(&key(v));
    if k.is_empty() {
        return None;
    }
    units_for(ty).iter().find(|u| sq2(&key(u)) == k || sq2(&key(&units::unit_label(u))) == k).copied()
}
fn type_of_unit(v: &str) -> Option<&'static str> {
    TYPES.iter().find(|ty| read_unit(v, ty).is_some()).copied()
}

/// `readLibraryCsv(text)` → entries and warnings.
pub fn read_library_csv(text: &str) -> Result<(Vec<Value>, Vec<String>), String> {
    let rows = parse_csv(text);
    if rows.len() < 2 {
        return Err(t("lib.err.empty"));
    }
    let hm = header_map();
    let mut cols: Vec<(&'static str, usize)> = Vec::new();
    if let Some(h0) = rows.first() {
        for (i, h) in h0.iter().enumerate() {
            let k = key(h);
            if let Some((_, f)) = hm.iter().find(|(x, _)| *x == k)
                && !cols.iter().any(|(c, _)| c == f)
            {
                cols.push((f, i));
            }
        }
    }
    if !cols.iter().any(|(c, _)| *c == "name") {
        return Err(t("lib.err.head"));
    }
    let tpl = tf("x.note.formula", &[("formula", F::S("\u{0}".into()))]);
    let (pre, post) = tpl.split_once('\u{0}').map_or((tpl.clone(), String::new()), |(a, b)| (a.to_string(), b.to_string()));
    let formula_note = |part: &str| -> Option<String> {
        let mid = part.strip_prefix(pre.as_str())?.strip_suffix(post.as_str())?;
        if mid.is_empty() || mid.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            return None;
        }
        Some(mid.to_string())
    };
    let total = key(&t("x.total"));
    let base = key(&t("export.alt.base"));
    let alt_head = key(&t("export.alt.h"));
    let mut entries: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut last: Option<usize> = None;
    for (i, r) in rows.iter().enumerate().skip(1) {
        let line = i as f64 + 1.0;
        let cell = |f: &str| -> String {
            cols.iter()
                .find(|(c, _)| *c == f)
                .map_or(String::new(), |(_, ix)| cc_m3d::js::trim(r.get(*ix).map_or("", String::as_str)).to_string())
        };
        let strip_q = |x: String| x.strip_prefix('\'').map_or(x.clone(), str::to_string);
        let name = strip_q(cell("name"));
        let comp = strip_q(cell("component"));
        let ty_raw = cell("type");
        let mut ty = read_type(&ty_raw);
        if name.is_empty() && comp.is_empty() {
            continue;
        }
        if (key(&name) == total || key(&name) == base) && ty_raw.is_empty() {
            continue;
        }
        if key(&name) == alt_head && comp.is_empty() {
            break;
        }
        let waste = parse_percent(&strip_q(cell("waste")));
        let mat = parse_rate(&cell("mat"));
        let lab = parse_rate(&cell("lab"));
        let (waste, mat, lab) = match (waste, mat, lab) {
            (Ok(w), Ok(m), Ok(l)) => (w, m, l),
            (w, m, l) => {
                let why = w.err().or(m.err()).or(l.err()).unwrap_or_default();
                warnings.push(tf("lib.warn.row", &[("line", F::N(line)), ("why", F::S(why))]));
                continue;
            }
        };
        if !comp.is_empty() || ty == Some("component") {
            let Some(li) = last else {
                warnings.push(tf("lib.warn.orphan", &[("line", F::N(line))]));
                continue;
            };
            let mut formula = strip_q(cell("formula"));
            if formula.is_empty() {
                for part in split_semi(&cell("notes")) {
                    if let Some(m) = formula_note(cc_m3d::js::trim(&part)) {
                        formula = cc_m3d::js::trim(&m).to_string();
                        break;
                    }
                }
            }
            if formula.is_empty() {
                warnings.push(tf("lib.warn.formula", &[("line", F::N(line))]));
                continue;
            }
            let Some(Value::Object(e)) = entries.get_mut(li) else { continue };
            let ncomp = jv::arr(e.get("components")).map_or(0, Vec::len);
            if ncomp >= limits::COMPONENTS {
                warnings.push(tf("lib.warn.comps", &[("line", F::N(line))]));
                continue;
            }
            let unit = cell("unit");
            let mut f = Obj::new();
            f.insert("id".into(), s(format!("c{}", ncomp.saturating_add(1))));
            f.insert("name".into(), s(if comp.is_empty() { name.clone() } else { comp.clone() }));
            f.insert("unit".into(), s(if unit.is_empty() { "EA".to_string() } else { unit }));
            f.insert("formula".into(), s(formula));
            f.insert("waste".into(), n(waste));
            f.insert("mat".into(), n(mat));
            f.insert("lab".into(), n(lab));
            if let Some(Value::Array(cl)) = e.get_mut("components") {
                cl.push(Value::Object(normal_component(&f)));
            }
            continue;
        }
        if ty.is_none() {
            ty = type_of_unit(&cell("unit"));
        }
        let Some(ty) = ty else {
            warnings.push(tf(
                "lib.warn.type",
                &[("line", F::N(line)), ("type", F::S(if ty_raw.is_empty() { "—".into() } else { ty_raw.clone() }))],
            ));
            last = None;
            continue;
        };
        let unit_raw = cell("unit");
        let unit = read_unit(&unit_raw, ty);
        if !unit_raw.is_empty() && unit.is_none() {
            warnings.push(tf(
                "lib.warn.unit",
                &[("line", F::N(line)), ("unit", F::S(unit_raw.clone())), ("def", F::S(units::unit_label(default_unit(ty))))],
            ));
        }
        let mut height_ft = 0.0;
        let h_raw = cell("height");
        if !h_raw.is_empty() && ty != "count" {
            match parse_length(&h_raw, if unit == Some("m") || unit == Some("m²") { "m" } else { "ft" }) {
                Ok(h) => height_ft = h,
                Err(e) => warnings.push(tf("lib.warn.height", &[("line", F::N(line)), ("why", F::S(e))])),
            }
        }
        if entries.len() >= LIBRARY_MAX {
            warnings.push(tf("lib.warn.max", &[("n", F::from(LIBRARY_MAX))]));
            break;
        }
        entries.push(json!({
            "name": cc_m3d::js::slice16(&name, limits::NAME), "type": ty, "unit": unit.unwrap_or(default_unit(ty)),
            "heightFt": n(height_ft), "waste": n(waste), "mat": n(mat), "lab": n(lab), "components": [],
        }));
        last = Some(entries.len().saturating_sub(1));
    }
    if entries.is_empty() {
        return Err(warnings.first().cloned().unwrap_or_else(|| t("lib.err.none")));
    }
    Ok((merge_entries(&[], &entries).0, warnings))
}

/// `s.split(/;\s*/)`.
fn split_semi(st: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = st.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ';' {
            out.push(std::mem::take(&mut cur));
            while chars.peek().is_some_and(|c| jv::is_space(*c)) {
                chars.next();
            }
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn name_key(v: &Value) -> String {
    jv::lower(cc_m3d::js::trim(&jv::to_string(v.get("name"))))
}
/// Same name and type (any case) = the same entry.
pub fn same_entry(a: &Value, b: &Value) -> bool {
    jv::strict_eq(a.get("type"), b.get("type")) && name_key(a) == name_key(b)
}

/// Add entries to a library: one with the same name and type replaces it in its place, the others go at the end.
pub fn merge_entries(lib: &[Value], entries: &[Value]) -> (Vec<Value>, f64, f64) {
    let mut out = lib.to_vec();
    let (mut added, mut updated) = (0.0, 0.0);
    for e in entries {
        if let Some(i) = out.iter().position(|x| same_entry(x, e)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = e.clone();
            }
            updated += 1.0;
        } else if out.len() < LIBRARY_MAX {
            out.push(e.clone());
            added += 1.0;
        }
    }
    (out, added, updated)
}

/// A takeoff item → a library entry.
pub fn entry_from_item(it: &Value) -> Value {
    let mut o = Obj::new();
    for k in ["name", "type", "unit"] {
        o.insert(k.into(), it.get(k).cloned().unwrap_or(Value::Null));
    }
    let h = it.get("heightFt").filter(|h| jv::truthy(Some(h))).cloned().unwrap_or(n(0.0));
    o.insert("heightFt".into(), h);
    for k in ["waste", "mat", "lab"] {
        o.insert(k.into(), it.get(k).cloned().unwrap_or(Value::Null));
    }
    if let Some(c) = it.get("crew").filter(|c| jv::truthy(Some(c))) {
        o.insert("crew".into(), c.clone());
    }
    if let Some(c) = it.get("csi").filter(|c| jv::truthy(Some(c))) {
        o.insert("csi".into(), c.clone());
    }
    let comps: Vec<Value> = jv::arr(it.get("components"))
        .map(|l| {
            l.iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut co = Obj::new();
                    co.insert("id".into(), s(format!("c{}", i.saturating_add(1))));
                    for k in ["name", "unit", "formula", "waste", "mat", "lab"] {
                        co.insert(k.into(), c.get(k).cloned().unwrap_or(Value::Null));
                    }
                    Value::Object(co)
                })
                .collect()
        })
        .unwrap_or_default();
    o.insert("components".into(), Value::Array(comps));
    Value::Object(o)
}

/// A library entry → item fields (the model gives it an id, a colour and component ids).
pub fn item_from_entry(e: &Value) -> Value {
    let mut f = Obj::new();
    f.insert("id".into(), s("x"));
    for k in ["name", "type", "unit", "heightFt", "waste", "mat", "lab", "crew", "csi"] {
        if let Some(v) = e.get(k) {
            f.insert(k.into(), v.clone());
        }
    }
    let comps = jv::arr(e.get("components")).filter(|_| jv::truthy(e.get("components"))).cloned().unwrap_or_default();
    f.insert("components".into(), Value::Array(comps));
    let mut it = normal_item(&f);
    jv::del(&mut it, "id");
    Value::Object(it)
}

fn clean(v: Option<&Value>, max: usize) -> String {
    match jv::str(v) {
        Some(x) => cc_m3d::js::clean_text(x, max),
        None => String::new(),
    }
}

/// Pack ids are add-on ids: `^(?!.*(?:\.\.|--))[a-z][a-z0-9.-]{0,62}[a-z0-9]$`.
pub fn pack_id_ok(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() >= 2
        && b.len() <= 64
        && !v.contains("..")
        && !v.contains("--")
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.last().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'.' || *c == b'-')
}
/// `SEMVER_RE`.
pub fn semver_ok(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    if let Some(p) = pre
        && (p.is_empty() || p.len() > 20 || !p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.'))
    {
        return false;
    }
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            let b = p.as_bytes();
            !b.is_empty() && b.len() <= 6 && b.iter().all(u8::is_ascii_digit) && (b.len() == 1 || b.first() != Some(&b'0'))
        })
}

/// What a library entry may carry beyond its item fields: `pack`, `src`, `note` (validated).
pub fn entry_meta(e: Option<&Value>) -> Obj {
    let mut o = Obj::new();
    let p = e.and_then(|e| e.get("pack"));
    if let Some(Value::Object(po)) = p
        && let Some(id) = jv::str(po.get("id")).filter(|x| pack_id_ok(x))
        && let Some(ver) = jv::str(po.get("version")).filter(|x| semver_ok(x))
    {
        let nm = clean(po.get("name"), limits::NAME);
        o.insert("pack".into(), json!({ "id": id, "version": ver, "name": if nm.is_empty() { id.to_string() } else { nm } }));
    }
    let src = clean(e.and_then(|e| e.get("src")), 1000);
    let note = clean(e.and_then(|e| e.get("note")), 1000);
    if !src.is_empty() {
        o.insert("src".into(), s(src));
    }
    if !note.is_empty() {
        o.insert("note".into(), s(note));
    }
    o
}

/// Library entries as read from storage (anything else dropped).
pub fn read_stored(json: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for e in jv::arr(Some(json)).map_or(&[][..], Vec::as_slice).iter().take(LIBRARY_MAX) {
        if !jv::truthy(Some(e)) || jv::str(e.get("name")).is_none() || jv::str(e.get("type")).is_none_or(|x| !TYPES.contains(&x)) {
            continue;
        }
        let Some(eo) = jv::obj(Some(e)) else { continue };
        let mut f = Obj::new();
        f.insert("id".into(), s("x"));
        jv::assign(&mut f, eo);
        let comps: Vec<Value> = jv::arr(eo.get("components"))
            .map(|l| {
                l.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                        co.insert("id".into(), s(format!("c{}", i.saturating_add(1))));
                        Value::Object(co)
                    })
                    .collect()
            })
            .unwrap_or_default();
        f.insert("components".into(), Value::Array(comps));
        let it = Value::Object(normal_item(&f));
        let mut o = jv::obj(Some(&entry_from_item(&it))).cloned().unwrap_or_default();
        jv::assign(&mut o, &entry_meta(Some(e)));
        out.push(Value::Object(o));
    }
    out
}

/// The library as CSV in the import format.
pub fn library_csv(entries: &[Value]) -> String {
    let head: Vec<Cell> = ["name", "type", "unit", "waste", "mat", "lab", "height", "component", "formula"]
        .iter()
        .map(|k| Cell::Str(t(&format!("lib.head.{k}"))))
        .collect();
    let mut lines = vec![csv_line(&head)];
    let money = |r4: Option<&Value>| Cell::Num(round_to(jv::to_number(r4) / 10000.0, 4));
    for e in entries {
        let unit = jv::to_string(e.get("unit"));
        let h = jv::to_number(e.get("heightFt"));
        let hcell = if h > 0.0 {
            if unit == "m" || unit == "m²" {
                Cell::Str(crate::jv::num_str(round_to(h * 0.3048, 4)) + " m")
            } else {
                Cell::Str(units::ft_in(h))
            }
        } else {
            Cell::Str(String::new())
        };
        lines.push(csv_line(&[
            cell_of(e.get("name")),
            Cell::Str(t(&format!("type.{}", jv::to_string(e.get("type"))))),
            Cell::Str(units::unit_label(&unit)),
            Cell::Num(jv::to_number(e.get("waste")) / 100.0),
            money(e.get("mat")),
            money(e.get("lab")),
            hcell,
            Cell::Str(String::new()),
            Cell::Str(String::new()),
        ]));
        for c in jv::arr(e.get("components")).map_or(&[][..], Vec::as_slice) {
            lines.push(csv_line(&[
                Cell::Str(String::new()),
                Cell::Str(t("x.component")),
                cell_of(c.get("unit")),
                Cell::Num(jv::to_number(c.get("waste")) / 100.0),
                money(c.get("mat")),
                money(c.get("lab")),
                Cell::Str(String::new()),
                cell_of(c.get("name")),
                cell_of(c.get("formula")),
            ]));
        }
    }
    lines.join("\r\n") + "\r\n"
}

/// New prices for the takeoff's items that the library has: `[(id, patch)]`, changed ones only.
pub fn reprice_patches(items: &[Value], lib: &[Value]) -> Vec<(String, Obj)> {
    let mut out = Vec::new();
    for it in items {
        let Some(e) = lib.iter().find(|x| same_entry(x, it)) else { continue };
        let mut patch = Obj::new();
        for k in ["waste", "mat", "lab"] {
            if !jv::strict_eq(it.get(k), e.get(k))
                && let Some(v) = e.get(k)
            {
                patch.insert(k.into(), v.clone());
            }
        }
        if let Some(ec) = e.get("crew").filter(|c| jv::truthy(Some(c))) {
            let a = jv::stringify(ec);
            let b = it.get("crew").map(jv::stringify);
            if b.as_deref() != Some(a.as_str()) {
                patch.insert("crew".into(), ec.clone());
            }
        }
        let mut changed = false;
        let comps: Vec<Value> = jv::arr(it.get("components"))
            .map(|l| {
                l.iter()
                    .map(|c| {
                        let cn = name_key(c);
                        let ec = jv::arr(e.get("components")).and_then(|el| el.iter().find(|x| name_key(x) == cn));
                        match ec {
                            Some(ec)
                                if !(jv::strict_eq(ec.get("waste"), c.get("waste"))
                                    && jv::strict_eq(ec.get("mat"), c.get("mat"))
                                    && jv::strict_eq(ec.get("lab"), c.get("lab"))) =>
                            {
                                changed = true;
                                let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                                for k in ["waste", "mat", "lab"] {
                                    co.insert(k.into(), ec.get(k).cloned().unwrap_or(Value::Null));
                                }
                                Value::Object(co)
                            }
                            _ => c.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if changed {
            patch.insert("components".into(), Value::Array(comps));
        }
        if !patch.is_empty() {
            out.push((jv::to_string(it.get("id")), patch));
        }
    }
    out
}

/// The model's item fields check is shared: re-exported for packs.
pub use model::csi_ok;
