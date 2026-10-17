//! `model.js` — the takeoff project: items (+ assembly components), marks, sheet scales, undo, quantities and the
//! priced estimate. The state is plain JSON data, kept as a `serde_json::Value` in the JS key order (what the
//! `.takeoff.json` file holds); quantities are always computed from the marks and the sheet scales, never stored.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::bid::{bid_used, normal_bid};
use crate::formula;
use crate::geom::{self, P};
use crate::i18n::{F, t, tf};
use crate::jv::{self, Obj, n, s};
use crate::units::{self, area_unit_of, default_unit, ft_to, length_unit_of, sqft_to, units_for};

pub const FORMAT: &str = "takeoffcraft-project";
pub const FORMAT_VERSION: f64 = 1.0;
pub const TYPES: [&str; 3] = ["linear", "area", "count"];
pub const PALETTE: [&str; 10] =
    ["#D62728", "#1F77B4", "#2CA02C", "#FF7F0E", "#9467BD", "#8C564B", "#E377C2", "#17BECF", "#BCBD22", "#5A5A5A"];

/// `LIMITS`.
pub mod limits {
    pub const ITEMS: usize = 2000;
    pub const MARKS: usize = 200_000;
    pub const POINTS: usize = 5000;
    pub const HOLES: usize = 200;
    pub const COMPONENTS: usize = 50;
    pub const NAME: usize = 200;
    pub const FORMULA: usize = 500;
    pub const AREAS: usize = 200;
    pub const GROUPS: usize = 500;
    pub const REPEAT: f64 = 10000.0;
    pub const NOTES: usize = 5000;
    pub const NOTE_TEXT: usize = 2000;
}

/// Variables a component formula may use, by item type.
pub fn vars_of(ty: &str) -> &'static [&'static str] {
    match ty {
        "area" => &["A", "P", "H", "N"],
        "count" => &["N"],
        _ => &["L", "H", "A", "N"],
    }
}

pub const CREW_DAY: f64 = 8.0;
pub const NOTE_KINDS: [&str; 3] = ["note", "cloud", "highlight"];
pub const NOTE_STATUS: [&str; 3] = ["open", "answered", "closed"];
pub const MARK_EXTRAS: [&str; 7] = ["deducts", "area", "flag", "pitch", "slopes", "group", "z"];
pub const SLOPE_KINDS: [&str; 3] = ["flat", "rake", "hip"];

/// An edit error: the JS throws an `Error` with this message.
pub type Res<T> = Result<T, String>;

fn is_type(v: Option<&Value>) -> Option<&'static str> {
    jv::str(v).and_then(|x| TYPES.iter().find(|t| **t == x).copied())
}
fn type_index(k: &str) -> usize {
    TYPES.iter().position(|t| *t == k).unwrap_or(usize::MAX)
}

/// `str(v, max)`: control characters → spaces, trimmed, at most `max` UTF-16 units.
pub fn clean_str(v: Option<&Value>, max: usize) -> String {
    let st = jv::to_string_or_empty(v);
    cc_m3d::js::clean_text(&st, max)
}
fn int_in(v: Option<&Value>, lo: f64, hi: f64, d: f64) -> f64 {
    match jv::int(v) {
        Some(x) if x >= lo && x <= hi => x,
        _ => d,
    }
}
fn pitch_ok(v: Option<&Value>) -> Option<f64> {
    jv::finite(v).filter(|p| *p >= 0.0 && *p <= units::MAX_PITCH)
}
fn feet_ok(v: Option<&Value>) -> Option<f64> {
    jv::finite(v).filter(|x| *x > 0.0 && *x < 1e4)
}
fn z_ok(v: Option<&Value>) -> Option<f64> {
    jv::finite(v).filter(|z| z.abs() < 1e5)
}
fn repeat_ok(v: Option<&Value>) -> Option<f64> {
    jv::int(v).filter(|r| *r >= 1.0 && *r <= limits::REPEAT)
}

/// `sheetKey(hash, pageIndex)`.
pub fn sheet_key(hash: &str, page: f64) -> String {
    format!("{hash}:{}", crate::jv::num_str(page))
}

/// The unit an item measures marks of `kind` in.
pub fn unit_of(it: &Value, kind: &str) -> String {
    if jv::str(it.get("type")) == Some(kind) {
        return jv::to_string(it.get("unit"));
    }
    let c = jv::arr(it.get("conds")).and_then(|l| l.iter().find(|c| jv::str(c.get("kind")) == Some(kind)));
    match c.and_then(|c| c.get("unit")) {
        Some(u) if jv::truthy(Some(u)) => jv::to_string(Some(u)),
        _ => default_unit(kind).to_string(),
    }
}
/// `kindsOf(it)`: its main condition, then the others.
pub fn kinds_of(it: Option<&Value>) -> Vec<String> {
    let Some(it) = it else { return Vec::new() };
    let mut out = vec![jv::to_string(it.get("type"))];
    if let Some(l) = jv::arr(it.get("conds")) {
        for c in l {
            out.push(jv::to_string(c.get("kind")));
        }
    }
    out
}

/// A second condition of an item, clean, or None.
pub fn normal_cond(f: Option<&Value>, ty: &str) -> Option<Value> {
    let kind = is_type(f.and_then(|f| f.get("kind"))).filter(|k| *k != ty)?;
    let f = f?;
    let unit = match jv::str(f.get("unit")) {
        Some(u) if units_for(kind).contains(&u) => u.to_string(),
        _ => default_unit(kind).to_string(),
    };
    Some(json!({
        "kind": kind, "unit": unit,
        "waste": n(int_in(f.get("waste"), 0.0, 100000.0, 0.0)),
        "mat": n(int_in(f.get("mat"), 0.0, 1e13, 0.0)),
        "lab": n(int_in(f.get("lab"), 0.0, 1e13, 0.0)),
    }))
}

/// A crew `{prod, size, wage}`, or None.
pub fn crew_of(c: Option<&Value>) -> Option<Value> {
    let c = match c {
        Some(Value::Object(o)) => o,
        Some(Value::Array(_)) => return None, // an array has no prod
        _ => return None,
    };
    let prod = jv::finite(c.get("prod")).filter(|p| *p > 0.0 && *p < 1e7)?;
    let size = jv::int(c.get("size")).filter(|x| *x >= 1.0 && *x <= 1000.0).unwrap_or(1.0);
    let wage = jv::int(c.get("wage")).filter(|x| *x >= 0.0 && *x <= 1e9).unwrap_or(0.0);
    Some(json!({ "prod": n(prod), "size": n(size), "wage": n(wage) }))
}
/// Hours and labor of a crew on a quantity (with waste).
#[derive(Debug, Clone, Copy)]
pub struct Work {
    pub hours: f64,
    pub worker_hours: f64,
    pub days: f64,
    pub lab_ext: f64,
    pub rate: f64,
}
pub fn crew_work(crew: &Value, qty_w: f64) -> Work {
    let prod = jv::num(crew.get("prod")).unwrap_or(f64::NAN);
    let size = jv::num(crew.get("size")).unwrap_or(f64::NAN);
    let wage = jv::num(crew.get("wage")).unwrap_or(f64::NAN);
    let hours = qty_w / prod;
    let worker_hours = hours * size;
    Work {
        hours,
        worker_hours,
        days: hours / CREW_DAY,
        lab_ext: cc_m3d::js::round(units::round_to(worker_hours * wage / 100.0, 0)),
        rate: cc_m3d::js::round(size * wage / prod),
    }
}

/// A markup (T22), clean, or None.
pub fn normal_note(f: &Obj) -> Option<Value> {
    let kind = jv::str(f.get("kind")).filter(|k| NOTE_KINDS.contains(k))?;
    let sheet = jv::str(f.get("sheet"))?;
    let want = if kind == "note" { 1 } else { 2 };
    let raw: &[Value] = jv::arr(f.get("pts")).map_or(&[], Vec::as_slice);
    let pts: Vec<(Option<f64>, Option<f64>)> = raw
        .iter()
        .map(|p| match p {
            Value::Array(a) => (jv::num(a.first()), jv::num(a.get(1))),
            Value::Object(o) => (jv::num(o.get("x")), jv::num(o.get("y"))),
            _ => (None, None),
        })
        .collect();
    if pts.len() != want {
        return None;
    }
    let mut ok = Vec::with_capacity(want);
    for (x, y) in &pts {
        match (x, y) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() && x.abs() < 1e7 && y.abs() < 1e7 => ok.push((*x, *y)),
            _ => return None,
        }
    }
    if let [a, b] = ok.as_slice()
        && ((a.0 - b.0).abs() < 1e-6 || (a.1 - b.1).abs() < 1e-6)
    {
        return None;
    }
    let text = crate::bid::text(f.get("text"), limits::NOTE_TEXT);
    let status = jv::str(f.get("status")).filter(|x| NOTE_STATUS.contains(x)).unwrap_or("open");
    Some(json!({
        "id": jv::to_string(f.get("id")), "sheet": sheet, "kind": kind,
        "pts": ok.iter().map(|(x, y)| json!({ "x": n(*x), "y": n(*y) })).collect::<Vec<_>>(),
        "text": text, "status": status,
    }))
}

/// A mark's optional fields beyond its geometry, as plain data.
pub fn mark_extras(m: &Value) -> Obj {
    let mut o = Obj::new();
    let kind = jv::str(m.get("kind"));
    if kind == Some("linear")
        && let Some(d) = jv::arr(m.get("deducts")).filter(|d| !d.is_empty())
    {
        o.insert("deducts".into(), Value::Array(d.clone()));
    }
    for k in ["area", "group", "flag"] {
        if jv::truthy(m.get(k))
            && let Some(v) = m.get(k)
        {
            o.insert(k.into(), v.clone());
        }
    }
    if let Some(p) = m.get("pitch") {
        o.insert("pitch".into(), p.clone());
    }
    if kind == Some("linear")
        && let Some(sl) = jv::arr(m.get("slopes")).filter(|l| l.iter().any(|x| jv::truthy(Some(x))))
    {
        o.insert("slopes".into(), Value::Array(sl.iter().map(|x| if jv::truthy(Some(x)) { x.clone() } else { Value::Null }).collect()));
    }
    if kind == Some("count")
        && let Some(z) = z_ok(m.get("z"))
    {
        o.insert("z".into(), n(z));
    }
    o
}

/// The ids of the takeoff's bid areas and typical groups (a mark is in one of each at most).
#[derive(Debug, Default, Clone)]
pub struct ExtrasCtx {
    pub areas: HashSet<String>,
    pub groups: HashSet<String>,
}
pub fn extras_ctx(s: &Value) -> ExtrasCtx {
    let ids = |k: &str| -> HashSet<String> {
        jv::arr(s.get(k)).map(|l| l.iter().filter_map(|a| jv::str(a.get("id")).map(str::to_string)).collect()).unwrap_or_default()
    };
    ExtrasCtx { areas: ids("areas"), groups: ids("groups") }
}

/// Validate the optional fields of an untrusted mark `raw` whose geometry `m` is already clean.
pub fn read_extras(raw: Option<&Value>, m: &Obj, ctx: &ExtrasCtx) -> Obj {
    let mut o = Obj::new();
    let g = |k: &str| raw.and_then(|r| if let Value::Object(x) = r { x.get(k) } else { None });
    if let Some(a) = jv::str(g("area"))
        && ctx.areas.contains(a)
    {
        o.insert("area".into(), s(a));
    }
    if let Some(a) = jv::str(g("group"))
        && ctx.groups.contains(a)
    {
        o.insert("group".into(), s(a));
    }
    if jv::str(g("flag")) == Some("rev") {
        o.insert("flag".into(), s("rev"));
    }
    let kind = jv::str(m.get("kind"));
    if kind == Some("count")
        && let Some(z) = z_ok(g("z"))
    {
        o.insert("z".into(), n(z));
    }
    if kind != Some("count")
        && let Some(p) = pitch_ok(g("pitch"))
    {
        o.insert("pitch".into(), n(p));
    }
    let npts = jv::arr(m.get("pts")).map_or(0, Vec::len);
    if kind == Some("linear")
        && let Some(raw_sl) = jv::arr(g("slopes"))
    {
        let nseg = npts.saturating_sub(1);
        let sl: Vec<Value> = (0..nseg)
            .map(|i| {
                let x = raw_sl.get(i);
                match x {
                    Some(Value::Object(xo)) => match jv::str(xo.get("k")).filter(|k| SLOPE_KINDS.contains(k)) {
                        Some(k) => match pitch_ok(xo.get("p")) {
                            Some(p) => json!({ "k": k, "p": n(p) }),
                            None => json!({ "k": k }),
                        },
                        None => Value::Null,
                    },
                    _ => Value::Null,
                }
            })
            .collect();
        if npts >= 1 && raw_sl.len() == nseg && sl.iter().any(|x| !x.is_null()) {
            o.insert("slopes".into(), Value::Array(sl));
        }
    }
    if kind == Some("linear")
        && let Some(raw_d) = jv::arr(g("deducts"))
    {
        #[allow(clippy::cast_precision_loss)]
        let nseg = npts as f64 - 1.0;
        let mut d: Vec<f64> = Vec::new();
        for v in raw_d {
            if let Some(i) = jv::int(Some(v))
                && i >= 0.0
                && i < nseg
                && !d.contains(&i)
            {
                d.push(i);
            }
        }
        d.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss)]
        if !d.is_empty() && (d.len() as f64) < nseg {
            o.insert("deducts".into(), Value::Array(d.into_iter().map(n).collect()));
        }
    }
    o
}

/// `emptyState()`.
pub fn empty_state() -> Value {
    json!({ "name": "Takeoff", "files": [], "sheets": {}, "items": [], "marks": [], "areas": [], "seq": 0 })
}

/// `pt(p)`: a point in memory, `{x, y}` plus `b` only when it is an arc.
pub fn pt(p: &Value) -> Value {
    let (x, y, b) = match p {
        Value::Array(a) => (a.first().cloned(), a.get(1).cloned(), a.get(2).cloned()),
        Value::Object(o) => (o.get("x").cloned(), o.get("y").cloned(), o.get("b").cloned()),
        _ => (None, None, None),
    };
    let mut q = Obj::new();
    // Spec gap: a missing x / y is `undefined` in the JS (dropped by the next JSON snapshot); kept absent here
    if let Some(x) = x {
        q.insert("x".into(), x);
    }
    if let Some(y) = y {
        q.insert("y".into(), y);
    }
    if let Some(b) = b
        && jv::truthy(Some(&b))
    {
        q.insert("b".into(), b);
    }
    Value::Object(q)
}
/// `arr(p)`: `[x, y]` or `[x, y, bulge]`.
pub fn arr_of(p: &Value) -> Value {
    let x = p.get("x").cloned().unwrap_or(Value::Null);
    let y = p.get("y").cloned().unwrap_or(Value::Null);
    match p.get("b") {
        Some(b) if jv::truthy(Some(b)) => Value::Array(vec![x, y, b.clone()]),
        _ => Value::Array(vec![x, y]),
    }
}
/// A point list of a mark as geometry.
pub fn pts_of(v: Option<&Value>) -> Vec<P> {
    jv::arr(v)
        .map(|l| {
            l.iter()
                .map(|p| {
                    let b = jv::num(p.get("b")).unwrap_or(0.0);
                    P::arc(
                        jv::num(p.get("x")).unwrap_or(f64::NAN),
                        jv::num(p.get("y")).unwrap_or(f64::NAN),
                        if jv::truthy(p.get("b")) { b } else { 0.0 },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}
/// The cut-outs of a mark as geometry.
pub fn holes_of(v: Option<&Value>) -> Vec<Vec<P>> {
    jv::arr(v).map(|l| l.iter().map(|h| pts_of(Some(h))).collect()).unwrap_or_default()
}

/// A CSI MasterFormat number: "09", "09 21", "09 21 16" or "09 21 16.23".
pub fn csi_ok(v: Option<&Value>) -> bool {
    let Some(x) = jv::str(v) else { return false };
    let b = x.as_bytes();
    let d2 = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit) && b.get(i.saturating_add(1)).is_some_and(u8::is_ascii_digit);
    match b.len() {
        2 => d2(0),
        5 => d2(0) && b.get(2) == Some(&b' ') && d2(3),
        8 => d2(0) && b.get(2) == Some(&b' ') && d2(3) && b.get(5) == Some(&b' ') && d2(6),
        11 => d2(0) && b.get(2) == Some(&b' ') && d2(3) && b.get(5) == Some(&b' ') && d2(6) && b.get(8) == Some(&b'.') && d2(9),
        _ => false,
    }
}

fn hex_color(v: Option<&Value>) -> Option<String> {
    // /^#[0-9a-f]{6}$/i.test(f.color || '')
    let st = if jv::truthy(v) { jv::to_string(v) } else { String::new() };
    let b = st.as_bytes();
    (b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit)).then(|| st.to_ascii_uppercase())
}

/// `cleanAddonData(v)` (addons.js): add-ons' own data on an item, untrusted.
pub fn clean_addon_data(v: Option<&Value>) -> Option<Value> {
    let o = jv::obj(v)?;
    let mut out = Obj::new();
    for (id, x) in js_entries(o).into_iter().take(20) {
        let idb = id.as_bytes();
        let id_ok = idb.len() >= 2
            && idb.len() <= 64
            && idb.first().is_some_and(u8::is_ascii_lowercase)
            && idb.iter().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'.' || *c == b'-');
        if !id_ok || id.contains("..") || id.contains("--") || id.ends_with('.') || id.ends_with('-') {
            continue;
        }
        let Some(xo) = jv::obj(Some(x)) else { continue };
        let kind = jv::str(xo.get("kind")).filter(|k| {
            let kb = k.as_bytes();
            kb.is_empty()
                || (kb.len() <= 41
                    && kb.first().is_some_and(u8::is_ascii_lowercase)
                    && kb.iter().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'))
        });
        let ver = jv::int(xo.get("v")).filter(|v| *v >= 1.0 && *v <= 9999.0);
        let data = match xo.get("data") {
            None | Some(Value::Null) => Value::Null,
            Some(d) => d.clone(),
        };
        let json = jv::stringify(&data);
        let (Some(kind), Some(ver)) = (kind, ver) else { continue };
        if jv::len16(&json) > 12000 {
            continue;
        }
        out.insert(id.clone(), json!({ "kind": kind, "v": n(ver), "data": data }));
    }
    if out.is_empty() { None } else { Some(Value::Object(out)) }
}

/// `Object.entries(o)` in JS order: array-index keys ascending, then the others in insertion order.
pub fn js_entries(o: &Obj) -> Vec<(&String, &Value)> {
    let idx = |k: &str| -> Option<u64> {
        if k.is_empty() || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|c| c.is_ascii_digit()) || k.len() > 10 {
            return None;
        }
        k.parse::<u64>().ok().filter(|v| *v < 4_294_967_295)
    };
    let mut ints: Vec<(u64, (&String, &Value))> = o.iter().filter_map(|(k, v)| idx(k).map(|i| (i, (k, v)))).collect();
    ints.sort_by_key(|(i, _)| *i);
    let mut out: Vec<(&String, &Value)> = ints.into_iter().map(|(_, e)| e).collect();
    out.extend(o.iter().filter(|(k, _)| idx(k).is_none()));
    out
}

/// `normalItem(f)`.
pub fn normal_item(f: &Obj) -> Obj {
    let ty = is_type(f.get("type")).unwrap_or("linear");
    let unit = match jv::str(f.get("unit")) {
        Some(u) if units_for(ty).contains(&u) => u.to_string(),
        _ => default_unit(ty).to_string(),
    };
    let mut o = Obj::new();
    o.insert("id".into(), s(jv::to_string(f.get("id"))));
    let name = clean_str(f.get("name"), limits::NAME);
    o.insert("name".into(), s(if name.is_empty() { t("item.untitled") } else { name }));
    o.insert("type".into(), s(ty));
    o.insert("color".into(), s(hex_color(f.get("color")).unwrap_or_else(|| PALETTE[0].to_string())));
    o.insert("unit".into(), s(unit));
    o.insert("heightFt".into(), n(jv::finite(f.get("heightFt")).filter(|h| *h >= 0.0 && *h < 1e6).unwrap_or(0.0)));
    o.insert("waste".into(), n(int_in(f.get("waste"), 0.0, 100000.0, 0.0)));
    o.insert("mat".into(), n(int_in(f.get("mat"), 0.0, 1e13, 0.0)));
    o.insert("lab".into(), n(int_in(f.get("lab"), 0.0, 1e13, 0.0)));
    let comps: Vec<Value> = jv::arr(f.get("components"))
        .map(|l| {
            l.iter().take(limits::COMPONENTS).map(|c| Value::Object(normal_component(jv::obj(Some(c)).unwrap_or(&Obj::new())))).collect()
        })
        .unwrap_or_default();
    o.insert("components".into(), Value::Array(comps));
    // conds
    let mut cl: Vec<Value> = Vec::new();
    if let Some(l) = jv::arr(f.get("conds")) {
        for c in l {
            if let Some(nc) = normal_cond(Some(c), ty)
                && !cl.iter().any(|x| x.get("kind") == nc.get("kind"))
            {
                cl.push(nc);
            }
        }
    }
    cl.sort_by_key(|c| type_index(jv::str(c.get("kind")).unwrap_or("")));
    if !cl.is_empty() {
        o.insert("conds".into(), Value::Array(cl));
    }
    if let Some(c) = crew_of(f.get("crew")) {
        o.insert("crew".into(), c);
    }
    if ty != "count"
        && let Some(p) = pitch_ok(f.get("pitch")).filter(|p| *p > 0.0)
    {
        o.insert("pitch".into(), n(p));
    }
    if ty == "linear"
        && let Some(v) = feet_ok(f.get("thickFt"))
    {
        o.insert("thickFt".into(), n(v));
    }
    if ty != "count"
        && let Some(v) = feet_ok(f.get("baseFt"))
    {
        o.insert("baseFt".into(), n(v));
    }
    if ty == "linear"
        && let Some(v) = feet_ok(f.get("headFt"))
    {
        o.insert("headFt".into(), n(v));
    }
    if ty == "linear"
        && let Some(sl) = jv::str(f.get("slope")).filter(|x| *x == "rake" || *x == "hip")
    {
        o.insert("slope".into(), s(sl));
    }
    if let Some(xd) = clean_addon_data(f.get("xd")) {
        o.insert("xd".into(), xd);
    }
    if csi_ok(f.get("csi"))
        && let Some(c) = f.get("csi")
    {
        o.insert("csi".into(), c.clone());
    }
    if ty == "area" && f.get("backout") == Some(&Value::Bool(true)) {
        let own = jv::to_string(f.get("id"));
        let mut list: Vec<Value> = Vec::new();
        if let Some(l) = jv::arr(f.get("backoutOf")) {
            for x in l.iter().filter(|x| jv::str(Some(x)).is_some_and(|v| v != own)).take(limits::ITEMS) {
                if !list.contains(x) {
                    list.push(x.clone());
                }
            }
        }
        o.insert("backout".into(), Value::Bool(true));
        o.insert("backoutOf".into(), Value::Array(list));
    }
    o
}

/// `normalComponent(f)`.
pub fn normal_component(f: &Obj) -> Obj {
    let mut o = Obj::new();
    o.insert("id".into(), s(jv::to_string(f.get("id"))));
    let name = clean_str(f.get("name"), limits::NAME);
    o.insert("name".into(), s(if name.is_empty() { t("comp.default") } else { name }));
    let unit = clean_str(f.get("unit"), 20);
    o.insert("unit".into(), s(if unit.is_empty() { "EA".to_string() } else { unit }));
    o.insert("formula".into(), s(clean_str(f.get("formula"), limits::FORMULA)));
    o.insert("waste".into(), n(int_in(f.get("waste"), 0.0, 100000.0, 0.0)));
    o.insert("mat".into(), n(int_in(f.get("mat"), 0.0, 1e13, 0.0)));
    o.insert("lab".into(), n(int_in(f.get("lab"), 0.0, 1e13, 0.0)));
    o
}

/// A JS relational `v >= k` / `v > k` against a number: ToNumber of the value.
fn rel_num(v: Option<&Value>) -> f64 {
    jv::to_number(v)
}

fn hex64(v: Option<&Value>) -> bool {
    let st = jv::to_string(v);
    st.len() == 64 && st.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn id40(v: &str) -> bool {
    !v.is_empty() && v.len() <= 40 && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// `parseInt(String(id).slice(1), 10) || 0`.
fn id_number(id: &str) -> f64 {
    let u = jv::utf16(id);
    let rest = jv::from16(u.get(1..).unwrap_or(&[]));
    let st = rest.trim_start_matches(jv::is_space);
    let (neg, body) = match st.as_bytes().first() {
        Some(b'-') => (true, st.get(1..).unwrap_or("")),
        Some(b'+') => (false, st.get(1..).unwrap_or("")),
        _ => (false, st),
    };
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return 0.0;
    }
    let v = digits.parse::<f64>().unwrap_or(0.0);
    let v = if neg { -v } else { v };
    if v == 0.0 || v.is_nan() { 0.0 } else { v }
}

/// The result of reading a project file.
#[derive(Debug, Clone)]
pub struct Read {
    pub state: Value,
    pub warnings: Vec<String>,
}

/// Validate + normalise a project file (an untrusted file).
pub fn read_project(json: &Value) -> Result<Read, String> {
    let Value::Object(j) = json else { return Err(t("file.err.format")) };
    if jv::str(j.get("format")) != Some(FORMAT) {
        return Err(t("file.err.format"));
    }
    let ver = rel_num(j.get("version"));
    if !(ver >= 1.0) {
        return Err(t("file.err.version"));
    }
    if ver > FORMAT_VERSION {
        return Err(t("file.err.newer"));
    }
    let mut warnings = Vec::new();
    let name = clean_str(j.get("name"), limits::NAME);
    let mut files: Vec<Value> = Vec::new();
    let finite = |v: Option<&Value>| jv::finite(v).filter(|x| x.abs() < 1e7);
    let pt_ok = |p: &Value| -> bool {
        let Value::Array(a) = p else { return false };
        (a.len() == 2 || (a.len() == 3 && finite(a.get(2)).is_some_and(|b| b.abs() <= 1000.0)))
            && finite(a.first()).is_some()
            && finite(a.get(1)).is_some()
    };
    let pts_ok = |a: Option<&Value>, min: usize| -> bool {
        match a {
            Some(Value::Array(l)) => l.len() >= min && l.len() <= limits::POINTS && l.iter().all(pt_ok),
            _ => false,
        }
    };
    if let Some(l) = jv::arr(j.get("files")) {
        for f in l {
            if !jv::truthy(Some(f)) || !hex64(f.get("hash")) {
                warnings.push(t("file.warn.hash"));
                continue;
            }
            let fname = clean_str(f.get("name"), 255);
            files.push(json!({
                "hash": f.get("hash").cloned().unwrap_or(Value::Null),
                "name": if fname.is_empty() { "drawing.pdf".to_string() } else { fname },
                "size": n(int_in(f.get("size"), 0.0, 1e13, 0.0)),
                "pages": n(int_in(f.get("pages"), 0.0, 1e6, 0.0)),
            }));
        }
    }
    let known: HashSet<String> = files.iter().filter_map(|f| jv::str(f.get("hash")).map(str::to_string)).collect();
    let sheet_ok = |k: Option<&Value>| -> bool {
        let Some(k) = jv::str(k) else { return false };
        let Some((h, p)) = k.split_once(':') else { return false };
        h.len() == 64
            && h.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            && !p.is_empty()
            && p.len() <= 6
            && p.bytes().all(|c| c.is_ascii_digit())
            && known.contains(h)
    };
    let mut sheets = Obj::new();
    if let Some(sh) = jv::obj(j.get("sheets")) {
        for (k, v) in js_entries(sh) {
            if !sheet_ok(Some(&s(k.clone()))) {
                continue;
            }
            let sc = v.get("scale").filter(|x| jv::truthy(Some(x)));
            let mut scale = Value::Null;
            if let Some(sc) = sc
                && let Some(fpu) = finite(sc.get("ftPerUnit")).filter(|x| *x > 0.0)
                && let Some(method) = jv::str(sc.get("method")).filter(|m| *m == "preset" || *m == "calibrated")
            {
                let preset = match jv::str(sc.get("preset")) {
                    Some(p) => s(cc_m3d::js::slice16(p, 20)),
                    None => Value::Null,
                };
                let mut so = json!({ "method": method, "ftPerUnit": n(fpu), "preset": preset, "label": clean_str(sc.get("label"), 80), "cal": null });
                if let Some(cal) = sc.get("cal").filter(|c| jv::truthy(Some(c))) {
                    let pair =
                        Value::Array(vec![cal.get("a").cloned().unwrap_or(Value::Null), cal.get("b").cloned().unwrap_or(Value::Null)]);
                    if pts_ok(Some(&pair), 2)
                        && let Some(ft) = finite(cal.get("ft"))
                    {
                        let two =
                            |v: Option<&Value>| Value::Array(jv::arr(v).map(|a| a.iter().take(2).cloned().collect()).unwrap_or_default());
                        if let Value::Object(m) = &mut so {
                            m.insert(
                                "cal".into(),
                                json!({ "a": two(cal.get("a")), "b": two(cal.get("b")), "ft": n(ft), "text": clean_str(cal.get("text"), 40) }),
                            );
                        }
                    }
                }
                scale = so;
            }
            let mut entry = Obj::new();
            entry.insert("scale".into(), scale);
            if let Some(level) = jv::finite(v.get("level")).filter(|l| l.abs() < 1e5) {
                entry.insert("level".into(), n(level));
            }
            sheets.insert(k.clone(), Value::Object(entry));
        }
    }
    let mut ids: HashSet<String> = HashSet::new();
    let mut items: Vec<Value> = Vec::new();
    if let Some(l) = jv::arr(j.get("items")) {
        for raw in l.iter().take(limits::ITEMS) {
            let Some(id) = jv::str(raw.get("id")).filter(|_| jv::truthy(Some(raw))) else { continue };
            if ids.contains(id) {
                continue;
            }
            let Some(ro) = jv::obj(Some(raw)) else { continue };
            let mut f = ro.clone();
            let comps: Vec<Value> = jv::arr(ro.get("components"))
                .map(|cl| cl.iter().filter(|c| jv::truthy(Some(c)) && jv::str(c.get("id")).is_some()).cloned().collect())
                .unwrap_or_default();
            f.insert("components".into(), Value::Array(comps));
            let it = normal_item(&f);
            ids.insert(jv::to_string(it.get("id")));
            items.push(Value::Object(it));
        }
    }
    let mut areas: Vec<Value> = Vec::new();
    if let Some(l) = jv::arr(j.get("areas")) {
        for a in l.iter().take(limits::AREAS) {
            let Some(id) = jv::str(a.get("id")) else { continue };
            if !id40(id) || areas.iter().any(|x| jv::str(x.get("id")) == Some(id)) {
                continue;
            }
            let nm = clean_str(a.get("name"), limits::NAME);
            if nm.is_empty() {
                continue;
            }
            let mut o = Obj::new();
            o.insert("id".into(), s(id));
            o.insert("name".into(), s(nm));
            if let Some(alt) = jv::str(a.get("alt")).filter(|x| *x == "add" || *x == "deduct") {
                o.insert("alt".into(), s(alt));
            }
            areas.push(Value::Object(o));
        }
    }
    let bid = j.get("bid").filter(|b| matches!(b, Value::Object(_) | Value::Array(_))).map(|b| normal_bid(Some(b)));
    let mut notes: Option<Vec<Value>> = None;
    let mut nids: HashSet<String> = HashSet::new();
    if let Some(l) = jv::arr(j.get("notes")) {
        for raw in l.iter().take(limits::NOTES) {
            let Some(id) = jv::str(raw.get("id")) else { continue };
            if !id40(id) || nids.contains(id) || !sheet_ok(raw.get("sheet")) {
                continue;
            }
            if let Some(ro) = jv::obj(Some(raw))
                && let Some(nn) = normal_note(ro)
            {
                nids.insert(jv::to_string(nn.get("id")));
                notes.get_or_insert_with(Vec::new).push(nn);
            }
        }
    }
    let mut groups: Option<Vec<Value>> = None;
    if let Some(l) = jv::arr(j.get("groups")) {
        for g in l.iter().take(limits::GROUPS) {
            let Some(id) = jv::str(g.get("id")) else { continue };
            if !id40(id) {
                continue;
            }
            let list = groups.get_or_insert_with(Vec::new);
            if list.iter().any(|x| jv::str(x.get("id")) == Some(id)) {
                continue;
            }
            let nm = clean_str(g.get("name"), limits::NAME);
            if nm.is_empty() {
                continue;
            }
            list.push(json!({ "id": id, "name": nm, "repeat": n(repeat_ok(g.get("repeat")).unwrap_or(1.0)) }));
        }
    }
    let types: HashMap<String, String> = items.iter().map(|i| (jv::to_string(i.get("id")), jv::to_string(i.get("type")))).collect();
    let kinds: HashMap<String, Vec<String>> = items.iter().map(|i| (jv::to_string(i.get("id")), kinds_of(Some(i)))).collect();
    let backout_ids: HashSet<String> = items.iter().filter(|i| jv::truthy(i.get("backout"))).map(|i| jv::to_string(i.get("id"))).collect();
    for it in &mut items {
        if jv::truthy(it.get("backout"))
            && let Some(Value::Array(l)) = it.get_mut("backoutOf")
        {
            l.retain(|id| {
                let id = jv::to_string(Some(id));
                types.get(&id).map(String::as_str) == Some("area") && !backout_ids.contains(&id)
            });
        }
    }
    let mut state = empty_state();
    let ctx = ExtrasCtx {
        areas: areas.iter().filter_map(|a| jv::str(a.get("id")).map(str::to_string)).collect(),
        groups: groups.as_ref().map(|g| g.iter().filter_map(|a| jv::str(a.get("id")).map(str::to_string)).collect()).unwrap_or_default(),
    };
    let mut dropped = 0usize;
    let mut mids: HashSet<String> = HashSet::new();
    let mut marks: Vec<Value> = Vec::new();
    if let Some(l) = jv::arr(j.get("marks")) {
        for m in l.iter().take(limits::MARKS) {
            let id = jv::str(m.get("id"));
            let item = jv::str(m.get("item"));
            let kind = jv::str(m.get("kind"));
            let ok = jv::truthy(Some(m))
                && id.is_some_and(|i| !mids.contains(i))
                && item.is_some_and(|i| types.contains_key(i))
                && sheet_ok(m.get("sheet"))
                && match (item, kind) {
                    (Some(i), Some(k)) => kinds.get(i).is_some_and(|ks| ks.iter().any(|x| x == k)),
                    _ => false,
                };
            if !ok {
                dropped = dropped.saturating_add(1);
                continue;
            }
            let kind = kind.unwrap_or("");
            let min = if kind == "area" || kind == "linear" { 2 } else { 1 };
            let ring_ok =
                |r: Option<&Value>| pts_ok(r, 2) && jv::arr(r).is_some_and(|l| l.len() >= 3 || l.iter().any(|p| jv::truthy(p.get(2))));
            let pts_raw = m.get("pts");
            if !pts_ok(pts_raw, min)
                || (kind == "area" && !ring_ok(pts_raw))
                || (kind == "count" && jv::arr(pts_raw).is_some_and(|l| l.iter().any(|p| jv::truthy(p.get(2)))))
            {
                dropped = dropped.saturating_add(1);
                continue;
            }
            let holes: Vec<Value> = if kind == "area" {
                jv::arr(m.get("holes"))
                    .map(|hl| {
                        hl.iter()
                            .take(limits::HOLES)
                            .filter(|h| ring_ok(Some(h)))
                            .map(|h| Value::Array(jv::arr(Some(h)).map(|r| r.iter().map(pt).collect()).unwrap_or_default()))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let id = id.unwrap_or("").to_string();
            mids.insert(id.clone());
            let mut nm = Obj::new();
            nm.insert("id".into(), s(id));
            nm.insert("item".into(), s(item.unwrap_or("")));
            nm.insert("sheet".into(), m.get("sheet").cloned().unwrap_or(Value::Null));
            nm.insert("kind".into(), s(kind));
            nm.insert("pts".into(), Value::Array(jv::arr(pts_raw).map(|r| r.iter().map(pt).collect()).unwrap_or_default()));
            nm.insert("holes".into(), Value::Array(holes));
            let ex = read_extras(Some(m), &nm, &ctx);
            jv::assign(&mut nm, &ex);
            marks.push(Value::Object(nm));
        }
    }
    if dropped > 0 {
        warnings.push(tf("file.warn.dropped", &[("n", F::from(dropped))]));
    }
    let mut max_n = 0.0f64;
    let mut consider = |id: &str| {
        max_n = cc_m3d::js::max(max_n, id_number(id));
    };
    for id in &ids {
        consider(id);
    }
    for id in &mids {
        consider(id);
    }
    for a in &areas {
        consider(&jv::to_string(a.get("id")));
    }
    for g in groups.iter().flatten() {
        consider(&jv::to_string(g.get("id")));
    }
    for nn in notes.iter().flatten() {
        consider(&jv::to_string(nn.get("id")));
    }
    for it in &items {
        for c in jv::arr(it.get("components")).map_or(&[][..], Vec::as_slice) {
            consider(&jv::to_string(c.get("id")));
        }
    }
    let seq = int_in(j.get("seq"), 0.0, 1e9, 0.0);
    if let Value::Object(st) = &mut state {
        st.insert("name".into(), s(if name.is_empty() { "Takeoff".to_string() } else { name }));
        st.insert("files".into(), Value::Array(files));
        st.insert("sheets".into(), Value::Object(sheets));
        st.insert("items".into(), Value::Array(items));
        st.insert("marks".into(), Value::Array(marks));
        st.insert("areas".into(), Value::Array(areas));
        st.insert("seq".into(), n(cc_m3d::js::max(seq, max_n)));
        if let Some(b) = bid {
            st.insert("bid".into(), b);
        }
        if let Some(nl) = notes {
            st.insert("notes".into(), Value::Array(nl));
        }
        if let Some(gl) = groups {
            st.insert("groups".into(), Value::Array(gl));
        }
    }
    Ok(Read { state, warnings })
}

// ---------------------------------------------------------------------------------------------------- quantities

/// One measured mark (`measureMark`).
#[derive(Debug, Clone)]
pub enum Measure {
    Count,
    Linear {
        length_ft: f64,
        gross_ft: f64,
        deduct_ft: f64,
        plan_ft: f64,
        segments_ft: Vec<f64>,
        sloped_ft: Vec<f64>,
        slopes: Vec<(String, f64)>,
        deducts: Vec<f64>,
    },
    Area {
        area_sqft: f64,
        plan_sqft: f64,
        factor: f64,
        pitch: f64,
        gross_sqft: f64,
        cut_sqft: f64,
        perimeter_ft: f64,
        outer_perimeter_ft: f64,
    },
}
impl Measure {
    pub fn length_ft(&self) -> f64 {
        if let Measure::Linear { length_ft, .. } = self { *length_ft } else { f64::NAN }
    }
    pub fn area_sqft(&self) -> f64 {
        if let Measure::Area { area_sqft, .. } = self { *area_sqft } else { f64::NAN }
    }
    pub fn perimeter_ft(&self) -> f64 {
        if let Measure::Area { perimeter_ft, .. } = self { *perimeter_ft } else { f64::NAN }
    }
    /// As the JS returns it.
    pub fn to_json(&self) -> Value {
        match self {
            Measure::Count => json!({ "count": 1 }),
            Measure::Linear { length_ft, gross_ft, deduct_ft, plan_ft, segments_ft, sloped_ft, slopes, deducts } => json!({
                "lengthFt": n(*length_ft), "grossFt": n(*gross_ft), "deductFt": n(*deduct_ft), "planFt": n(*plan_ft),
                "segmentsFt": segments_ft.iter().map(|v| n(*v)).collect::<Vec<_>>(),
                "slopedFt": sloped_ft.iter().map(|v| n(*v)).collect::<Vec<_>>(),
                "slopes": slopes.iter().map(|(k, p)| json!({ "k": k, "p": n(*p) })).collect::<Vec<_>>(),
                "deducts": deducts.iter().map(|v| n(*v)).collect::<Vec<_>>(),
            }),
            Measure::Area { area_sqft, plan_sqft, factor, pitch, gross_sqft, cut_sqft, perimeter_ft, outer_perimeter_ft } => json!({
                "areaSqft": n(*area_sqft), "planSqft": n(*plan_sqft), "factor": n(*factor), "pitch": n(*pitch),
                "grossSqft": n(*gross_sqft), "cutSqft": n(*cut_sqft), "perimeterFt": n(*perimeter_ft), "outerPerimeterFt": n(*outer_perimeter_ft),
            }),
        }
    }
}

/// `m.pitch ?? it?.pitch ?? 0` style lookups that keep JS `??` (only null / undefined fall through).
fn num_or(v: Option<&Value>) -> Option<f64> {
    match v {
        None | Some(Value::Null) => None,
        Some(x) => Some(jv::to_number(Some(x))),
    }
}

/// A run segment's slope: its own, else the item's, else flat.
pub fn seg_slope(m: &Value, it: Option<&Value>, i: usize) -> (String, f64) {
    let own = jv::arr(m.get("slopes")).and_then(|l| l.get(i)).filter(|x| !x.is_null());
    let k = match own.and_then(|o| o.get("k")).filter(|x| !x.is_null()) {
        Some(k) => jv::to_string(Some(k)),
        None => match it.and_then(|i| i.get("slope")).filter(|x| !x.is_null()) {
            Some(k) => jv::to_string(Some(k)),
            None => "flat".into(),
        },
    };
    let p = num_or(own.and_then(|o| o.get("p")))
        .or_else(|| num_or(m.get("pitch")))
        .or_else(|| num_or(it.and_then(|i| i.get("pitch"))))
        .unwrap_or(0.0);
    if k == "flat" || !(p > 0.0) { ("flat".into(), 0.0) } else { (k, p) }
}

/// One mark measured with a scale (ftPerUnit) — feet / square feet. No scale → None.
pub fn measure_mark(m: &Value, ft_per_unit: f64, it0: Option<&Value>) -> Option<Measure> {
    let kind = jv::str(m.get("kind")).unwrap_or("");
    if kind == "count" {
        return Some(Measure::Count);
    }
    let it = it0.filter(|it| jv::str(it.get("type")) == Some(kind));
    if !(ft_per_unit > 0.0) {
        return None;
    }
    let pitch = num_or(m.get("pitch")).or_else(|| num_or(it.and_then(|i| i.get("pitch")))).unwrap_or(0.0);
    let pts = pts_of(m.get("pts"));
    if kind == "linear" {
        let segs: Vec<f64> = geom::segment_lengths(&pts).iter().map(|d| d * ft_per_unit).collect();
        let kinds: Vec<(String, f64)> = (0..segs.len()).map(|i| seg_slope(m, it, i)).collect();
        let sloped: Vec<f64> = segs
            .iter()
            .zip(&kinds)
            .map(|(d, (k, p))| {
                d * if k == "rake" {
                    units::slope_factor(*p)
                } else if k == "hip" {
                    units::hip_factor(*p)
                } else {
                    1.0
                }
            })
            .collect();
        let mut ded: Vec<f64> = Vec::new();
        for v in jv::arr(m.get("deducts")).map_or(&[][..], Vec::as_slice) {
            let x = jv::to_number(Some(v));
            if !ded.iter().any(|d| *d == x || (d.is_nan() && x.is_nan())) {
                ded.push(x);
            }
        }
        let (mut len, mut cut, mut plan) = (0.0, 0.0, 0.0);
        for (i, d) in sloped.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let fi = i as f64;
            if ded.contains(&fi) {
                cut += d;
            } else {
                len += d;
                plan += segs.get(i).copied().unwrap_or(0.0);
            }
        }
        return Some(Measure::Linear {
            length_ft: len,
            gross_ft: len + cut,
            deduct_ft: cut,
            plan_ft: plan,
            segments_ft: segs,
            sloped_ft: sloped,
            slopes: kinds,
            deducts: ded,
        });
    }
    let holes = holes_of(m.get("holes"));
    let a = geom::area_with_holes(&pts, &holes);
    let k2 = ft_per_unit * ft_per_unit;
    let f = units::slope_factor(pitch);
    Some(Measure::Area {
        area_sqft: a.net * k2 * f,
        plan_sqft: a.net * k2,
        factor: f,
        pitch,
        gross_sqft: a.gross * k2 * f,
        cut_sqft: a.cut * k2 * f,
        perimeter_ft: a.perimeter * ft_per_unit,
        outer_perimeter_ft: a.outer_perimeter * ft_per_unit,
    })
}

/// Totals of an item, a sheet or a condition.
#[derive(Debug, Clone, Default)]
pub struct Tot {
    pub length_ft: f64,
    pub area_sqft: f64,
    pub perimeter_ft: f64,
    pub count: f64,
    pub marks: f64,
    /// second conditions (T8) by kind, in the order they were first met
    pub conds: Option<Vec<(String, Tot)>>,
}
impl Tot {
    pub fn cond(&self, kind: &str) -> Option<&Tot> {
        self.conds.as_ref().and_then(|l| l.iter().find(|(k, _)| k == kind).map(|(_, t)| t))
    }
    fn cond_mut(&mut self, kind: &str) -> Option<&mut Tot> {
        let l = self.conds.get_or_insert_with(Vec::new);
        if !l.iter().any(|(k, _)| k == kind) {
            l.push((kind.to_string(), Tot::default()));
        }
        l.iter_mut().find(|(k, _)| k == kind).map(|(_, t)| t)
    }
    /// marks of the second conditions together
    pub fn cond_marks(&self) -> f64 {
        self.conds.as_ref().map_or(0.0, |l| l.iter().fold(0.0, |s, (_, c)| s + c.marks))
    }
    fn to_json(&self, with_marks_first: bool) -> Value {
        let _ = with_marks_first;
        let mut o = Obj::new();
        o.insert("lengthFt".into(), n(self.length_ft));
        o.insert("areaSqft".into(), n(self.area_sqft));
        o.insert("perimeterFt".into(), n(self.perimeter_ft));
        o.insert("count".into(), n(self.count));
        o.insert("marks".into(), n(self.marks));
        if let Some(c) = &self.conds {
            let mut co = Obj::new();
            for (k, t) in c {
                co.insert(k.clone(), t.to_json(false));
            }
            o.insert("conds".into(), Value::Object(co));
        }
        Value::Object(o)
    }
}
/// An item's quantities (`quantities(state).get(id)`).
#[derive(Debug, Clone, Default)]
pub struct Q {
    pub tot: Tot,
    pub unscaled: f64,
    pub backout_sqft: f64,
    /// per sheet, in the order the sheets were first met
    pub by_sheet: Vec<(String, Tot)>,
}
impl Q {
    pub fn sheet(&self, key: &str) -> Option<&Tot> {
        self.by_sheet.iter().find(|(k, _)| k == key).map(|(_, t)| t)
    }
    pub fn to_json(&self, item: &Value) -> Value {
        let mut o = Obj::new();
        o.insert("item".into(), item.clone());
        o.insert("lengthFt".into(), n(self.tot.length_ft));
        o.insert("areaSqft".into(), n(self.tot.area_sqft));
        o.insert("perimeterFt".into(), n(self.tot.perimeter_ft));
        o.insert("count".into(), n(self.tot.count));
        o.insert("marks".into(), n(self.tot.marks));
        o.insert("unscaled".into(), n(self.unscaled));
        o.insert("backoutSqft".into(), n(self.backout_sqft));
        let mut bs = Obj::new();
        for (k, t) in &self.by_sheet {
            bs.insert(k.clone(), t.to_json(true));
        }
        o.insert("bySheet".into(), Value::Object(bs));
        if let Some(c) = &self.tot.conds {
            let mut co = Obj::new();
            for (k, t) in c {
                co.insert(k.clone(), t.to_json(false));
            }
            o.insert("conds".into(), Value::Object(co));
        }
        Value::Object(o)
    }
}

/// `quantities(state, opts)`: per item (in the items' order), keyed by item id.
#[derive(Debug, Clone, Default)]
pub struct Quantities {
    pub list: Vec<(String, Q)>,
    index: HashMap<String, usize>,
}
impl Quantities {
    pub fn get(&self, id: &str) -> Option<&Q> {
        self.index.get(id).and_then(|i| self.list.get(*i)).map(|(_, q)| q)
    }
    pub fn to_json(&self, state: &Value) -> Value {
        let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
        let mut o = Obj::new();
        for (k, q) in &self.list {
            let it = items.iter().find(|i| jv::str(i.get("id")) == Some(k)).cloned().unwrap_or(Value::Null);
            o.insert(k.clone(), q.to_json(&it));
        }
        Value::Object(o)
    }
}

/// Which marks `quantities` takes.
#[derive(Debug, Clone, Default)]
pub struct QOpts {
    /// one bid area (Some(None): the marks in none)
    pub area: Option<Option<String>>,
    /// add alternates, out of the base bid
    pub exclude: Option<HashSet<String>>,
}

/// Totals per item (feet / square feet) and per sheet.
pub fn quantities(state: &Value, opts: &QOpts) -> Quantities {
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let mut out = Quantities::default();
    for it in items {
        let id = jv::to_string(it.get("id"));
        if !out.index.contains_key(&id) {
            out.index.insert(id.clone(), out.list.len());
            out.list.push((id, Q::default()));
        } else if let Some(i) = out.index.get(&id).copied()
            && let Some(slot) = out.list.get_mut(i)
        {
            // a later item with the same id replaces the earlier in a JS Map (its place stays)
            slot.1 = Q::default();
        }
    }
    let item_by_id: HashMap<String, &Value> = items.iter().map(|i| (jv::to_string(i.get("id")), i)).collect();
    let bo = Backouts::new(state);
    let rep: HashMap<String, f64> = jv::arr(state.get("groups"))
        .map(|l| l.iter().map(|g| (jv::to_string(g.get("id")), jv::to_number(g.get("repeat")))).collect())
        .unwrap_or_default();
    let sheets = state.get("sheets");
    for m in jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice) {
        let marea = m.get("area").filter(|a| jv::truthy(Some(a))).map(|a| jv::to_string(Some(a)));
        if let Some(want) = &opts.area
            && marea.as_ref() != want.as_ref()
        {
            continue;
        }
        if let (Some(ex), Some(a)) = (&opts.exclude, &marea)
            && ex.contains(a)
        {
            continue;
        }
        let item_id = jv::to_string(m.get("item"));
        let Some(&qi) = out.index.get(&item_id) else { continue };
        let Some(item) = item_by_id.get(&item_id).copied() else { continue };
        let sheet = jv::to_string(m.get("sheet"));
        let fpu = sheets
            .and_then(|s| s.get(&sheet))
            .and_then(|s| s.get("scale"))
            .filter(|s| jv::truthy(Some(s)))
            .map_or(f64::NAN, |sc| jv::num(sc.get("ftPerUnit")).unwrap_or(f64::NAN));
        let Some((_, q)) = out.list.get_mut(qi) else { continue };
        q.tot.marks += 1.0;
        let Some(r) = measure_mark(m, fpu, Some(item)) else {
            q.unscaled += 1.0;
            continue;
        };
        let si = match q.by_sheet.iter().position(|(k, _)| *k == sheet) {
            Some(i) => i,
            None => {
                q.by_sheet.push((sheet.clone(), Tot::default()));
                q.by_sheet.len().saturating_sub(1)
            }
        };
        let k = {
            let g = m.get("group").map(|g| jv::to_string(Some(g)));
            let v = g.and_then(|g| rep.get(&g).copied()).unwrap_or(0.0);
            if v == 0.0 || v.is_nan() { 1.0 } else { v }
        };
        let kind = jv::str(m.get("kind")).unwrap_or("");
        let ity = jv::str(item.get("type")).unwrap_or("");
        let or0 = |v: f64| if v.is_nan() || v == 0.0 { 0.0 } else { v };
        if kind != ity {
            let (lf, af, pf) = (or0(r.length_ft()), or0(r.area_sqft()), or0(r.perimeter_ft()));
            if let Some(qc) = q.tot.cond_mut(kind) {
                qc.marks += 1.0;
                qc.count += k;
                qc.length_ft += lf * k;
                qc.area_sqft += af * k;
                qc.perimeter_ft += pf * k;
            }
            if let Some((_, st)) = q.by_sheet.get_mut(si) {
                st.marks += 1.0;
                if let Some(sc2) = st.cond_mut(kind) {
                    sc2.marks += 1.0;
                    sc2.count += k;
                    sc2.length_ft += lf * k;
                    sc2.area_sqft += af * k;
                    sc2.perimeter_ft += pf * k;
                }
            }
            continue;
        }
        let Some((_, st)) = q.by_sheet.get_mut(si) else { continue };
        st.marks += 1.0;
        match kind {
            "count" => {
                q.tot.count += k;
                st.count += k;
            }
            "linear" => {
                let l = r.length_ft();
                q.tot.length_ft += l * k;
                st.length_ft += l * k;
                q.tot.count += k;
                st.count += k;
            }
            _ => {
                let factor = if let Measure::Area { factor, .. } = r { factor } else { f64::NAN };
                let cut = bo.cut(m, fpu) * factor;
                let ra = r.area_sqft();
                let a = cc_m3d::js::max(0.0, ra - cut);
                q.tot.area_sqft += a * k;
                st.area_sqft += a * k;
                q.backout_sqft += (ra - a) * k;
                q.tot.perimeter_ft += r.perimeter_ft() * k;
                st.perimeter_ft += r.perimeter_ft() * k;
                q.tot.count += k;
                st.count += k;
            }
        }
    }
    out
}

/// Back-outs (T14): the marks of back-out items, by sheet, and what they take out of an area mark.
pub struct Backouts {
    by_sheet: HashMap<String, Vec<BackoutMark>>,
    pub bad: Vec<String>,
}
struct BackoutMark {
    id: String,
    of: HashSet<String>,
    box_: geom::Bounds,
    pts: Vec<P>,
}
impl Backouts {
    pub fn new(state: &Value) -> Self {
        let items: HashMap<String, &Value> =
            jv::arr(state.get("items")).map(|l| l.iter().map(|i| (jv::to_string(i.get("id")), i)).collect()).unwrap_or_default();
        let mut by_sheet: HashMap<String, Vec<BackoutMark>> = HashMap::new();
        let mut bad = Vec::new();
        for m in jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice) {
            let Some(it) = items.get(&jv::to_string(m.get("item"))) else { continue };
            let of = jv::arr(it.get("backoutOf"));
            if !jv::truthy(it.get("backout")) || jv::str(m.get("kind")) != Some("area") || of.is_none_or(Vec::is_empty) {
                continue;
            }
            let pts = pts_of(m.get("pts"));
            if !geom::is_convex(&pts) || jv::arr(m.get("holes")).is_some_and(|h| !h.is_empty()) {
                bad.push(jv::to_string(m.get("id")));
                continue;
            }
            let box_ = geom::bounds(&pts);
            by_sheet.entry(jv::to_string(m.get("sheet"))).or_default().push(BackoutMark {
                id: jv::to_string(m.get("id")),
                of: of.map(|l| l.iter().map(|x| jv::to_string(Some(x))).collect()).unwrap_or_default(),
                box_,
                pts,
            });
        }
        Backouts { by_sheet, bad }
    }
    pub fn any(&self) -> bool {
        !self.by_sheet.is_empty()
    }
    /// Square feet a mark loses to back-outs (0 when none applies).
    pub fn cut(&self, m: &Value, ft_per_unit: f64) -> f64 {
        let Some(list) = self.by_sheet.get(&jv::to_string(m.get("sheet"))) else { return 0.0 };
        if jv::str(m.get("kind")) != Some("area") || !(ft_per_unit > 0.0) {
            return 0.0;
        }
        let pts = pts_of(m.get("pts"));
        let holes = holes_of(m.get("holes"));
        let ob = geom::bounds(&pts);
        let item = jv::to_string(m.get("item"));
        let id = jv::to_string(m.get("id"));
        let mut a = 0.0;
        for b in list {
            if !b.of.contains(&item) || b.id == id {
                continue;
            }
            if b.box_.min_x >= ob.max_x || b.box_.max_x <= ob.min_x || b.box_.min_y >= ob.max_y || b.box_.max_y <= ob.min_y {
                continue;
            }
            a += geom::overlap_area(&pts, &holes, &b.pts);
        }
        a * ft_per_unit * ft_per_unit
    }
}

/// Formula variables for an item's totals, in the item's own units, in the JS key order.
pub fn item_vars(it: &Value, t: &Tot) -> Vec<(String, f64)> {
    let unit = jv::to_string(it.get("unit"));
    let lu = length_unit_of(&unit);
    let h = jv::num(it.get("heightFt")).unwrap_or(f64::NAN);
    match jv::str(it.get("type")) {
        Some("linear") => vec![
            ("L".into(), ft_to(t.length_ft, lu)),
            ("H".into(), ft_to(h, lu)),
            ("A".into(), sqft_to(t.length_ft * h, area_unit_of(&unit))),
            ("N".into(), t.count),
        ],
        Some("area") => vec![
            ("A".into(), sqft_to(t.area_sqft, &unit)),
            ("P".into(), ft_to(t.perimeter_ft, lu)),
            ("H".into(), ft_to(h, lu)),
            ("N".into(), t.count),
        ],
        _ => vec![("N".into(), t.count)],
    }
}
fn var(v: &[(String, f64)], k: &str) -> f64 {
    v.iter().find(|(n, _)| n == k).map_or(f64::NAN, |(_, x)| *x)
}
/// A second condition's quantity in its unit.
pub fn cond_qty(c: &Value, t: Option<&Tot>) -> f64 {
    let Some(t) = t else { return 0.0 };
    let unit = jv::to_string(c.get("unit"));
    match jv::str(c.get("kind")) {
        Some("linear") => ft_to(t.length_ft, length_unit_of(&unit)),
        Some("area") => sqft_to(t.area_sqft, &unit),
        _ => t.count,
    }
}
/// The item's own quantity in its unit.
pub fn item_qty(it: &Value, t: &Tot) -> f64 {
    let v = item_vars(it, t);
    match jv::str(it.get("type")) {
        Some("linear") => var(&v, "L"),
        Some("area") => var(&v, "A"),
        _ => var(&v, "N"),
    }
}

/// The priced estimate.
#[derive(Debug, Clone)]
pub struct Estimate {
    pub rows: Vec<Value>,
    pub mat_total: f64,
    pub lab_total: f64,
    pub total: f64,
    pub errors: f64,
    pub unscaled: f64,
    pub incomplete: bool,
    pub hours: f64,
    pub worker_hours: f64,
    pub days: f64,
}
impl Estimate {
    pub fn to_json(&self) -> Value {
        json!({
            "rows": self.rows, "matTotal": n(self.mat_total), "labTotal": n(self.lab_total), "total": n(self.total),
            "errors": n(self.errors), "unscaled": n(self.unscaled), "incomplete": self.incomplete,
            "hours": n(self.hours), "workerHours": n(self.worker_hours), "days": n(self.days),
        })
    }
}

/// A row's number field (absent / not a number = NaN).
pub fn rnum(r: &Value, k: &str) -> f64 {
    jv::num(r.get(k)).unwrap_or(f64::NAN)
}
/// A row's string field.
pub fn rstr<'a>(r: &'a Value, k: &str) -> &'a str {
    jv::str(r.get(k)).unwrap_or("")
}

fn priced(mut r: Obj, crew: Option<&Value>) -> Obj {
    let qty = jv::num(r.get("qty")).unwrap_or(f64::NAN);
    let qty_w = units::with_waste(qty, jv::num(r.get("waste")).unwrap_or(f64::NAN));
    let mat_ext = units::extend_cents(qty_w, jv::num(r.get("mat")).unwrap_or(f64::NAN));
    if let Some(crew) = crew {
        let w = crew_work(crew, qty_w);
        r.insert("lab".into(), n(w.rate));
        r.insert("qtyW".into(), n(qty_w));
        r.insert("matExt".into(), n(mat_ext));
        r.insert("labExt".into(), n(w.lab_ext));
        r.insert("total".into(), n(mat_ext + w.lab_ext));
        r.insert(
            "work".into(),
            json!({ "hours": n(w.hours), "workerHours": n(w.worker_hours), "days": n(w.days), "labExt": n(w.lab_ext), "rate": n(w.rate) }),
        );
        return r;
    }
    let lab_ext = units::extend_cents(qty_w, jv::num(r.get("lab")).unwrap_or(f64::NAN));
    r.insert("qtyW".into(), n(qty_w));
    r.insert("matExt".into(), n(mat_ext));
    r.insert("labExt".into(), n(lab_ext));
    r.insert("total".into(), n(mat_ext + lab_ext));
    r
}

fn obj_of(pairs: Vec<(&str, Value)>) -> Obj {
    let mut o = Obj::new();
    for (k, v) in pairs {
        o.insert(k.to_string(), v);
    }
    o
}

/// The priced estimate: a row per item, condition and component, extended in cents.
pub fn estimate(state: &Value, q: &Quantities) -> Estimate {
    let mut rows = Vec::new();
    let (mut mat, mut lab, mut errors, mut unscaled, mut hours, mut worker_hours) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let empty = Q::default();
    for it in jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice) {
        let id = jv::to_string(it.get("id"));
        let qq = q.get(&id).unwrap_or(&empty);
        let t = &qq.tot;
        let vars = item_vars(it, t);
        let qty = item_qty(it, t);
        let unit = jv::to_string(it.get("unit"));
        let ty = jv::to_string(it.get("type"));
        let crew = it.get("crew").filter(|c| jv::truthy(Some(c)));
        let mut base = obj_of(vec![
            ("kind", s("item")),
            ("itemId", s(id.clone())),
            ("name", it.get("name").cloned().unwrap_or(Value::Null)),
            ("unit", s(units::unit_label(&unit))),
            ("qty", n(qty)),
            ("waste", it.get("waste").cloned().unwrap_or(Value::Null)),
            ("mat", it.get("mat").cloned().unwrap_or(Value::Null)),
            ("lab", it.get("lab").cloned().unwrap_or(Value::Null)),
            ("type", s(ty.clone())),
        ]);
        if let Some(c) = crew {
            base.insert("crew".into(), c.clone());
        }
        let mut row = priced(base, crew);
        if let Some(w) = row.get("work") {
            hours += jv::num(w.get("hours")).unwrap_or(f64::NAN);
            worker_hours += jv::num(w.get("workerHours")).unwrap_or(f64::NAN);
        }
        row.insert("unscaled".into(), n(qq.unscaled));
        row.insert("marks".into(), n(t.marks - t.cond_marks()));
        if qq.backout_sqft > 0.0 {
            row.insert("backout".into(), json!({ "value": n(sqft_to(qq.backout_sqft, &unit)), "unit": units::unit_label(&unit) }));
        }
        let h = jv::num(it.get("heightFt")).unwrap_or(f64::NAN);
        if ty == "linear" && h > 0.0 {
            row.insert("wallArea".into(), json!({ "value": n(var(&vars, "A")), "unit": area_unit_of(&unit) }));
        }
        if ty == "area" {
            row.insert(
                "perimeter".into(),
                json!({ "value": n(var(&vars, "P")), "unit": if length_unit_of(&unit) == "m" { "m" } else { "LF" } }),
            );
        }
        unscaled += qq.unscaled;
        mat += jv::num(row.get("matExt")).unwrap_or(f64::NAN);
        lab += jv::num(row.get("labExt")).unwrap_or(f64::NAN);
        rows.push(Value::Object(row));
        for c in jv::arr(it.get("conds")).map_or(&[][..], Vec::as_slice) {
            let ck = jv::to_string(c.get("kind"));
            let cu = jv::to_string(c.get("unit"));
            let ct = t.cond(&ck);
            let base = obj_of(vec![
                ("kind", s("condition")),
                ("itemId", s(id.clone())),
                ("cond", s(ck.clone())),
                ("name", it.get("name").cloned().unwrap_or(Value::Null)),
                ("unit", s(units::unit_label(&cu))),
                ("qty", n(cond_qty(c, ct))),
                ("waste", c.get("waste").cloned().unwrap_or(Value::Null)),
                ("mat", c.get("mat").cloned().unwrap_or(Value::Null)),
                ("lab", c.get("lab").cloned().unwrap_or(Value::Null)),
                ("type", s(ck.clone())),
            ]);
            let mut cr = priced(base, None);
            cr.insert("marks".into(), n(ct.map_or(0.0, |x| if x.marks == 0.0 || x.marks.is_nan() { 0.0 } else { x.marks })));
            mat += jv::num(cr.get("matExt")).unwrap_or(f64::NAN);
            lab += jv::num(cr.get("labExt")).unwrap_or(f64::NAN);
            rows.push(Value::Object(cr));
        }
        let vnames = vars_of(&ty);
        for c in jv::arr(it.get("components")).map_or(&[][..], Vec::as_slice) {
            let formula = jv::to_string_or_empty(c.get("formula"));
            let err_row = |msg: String| {
                obj_of(vec![
                    ("kind", s("component")),
                    ("itemId", s(id.clone())),
                    ("compId", c.get("id").cloned().unwrap_or(Value::Null)),
                    ("name", c.get("name").cloned().unwrap_or(Value::Null)),
                    ("unit", c.get("unit").cloned().unwrap_or(Value::Null)),
                    ("error", s(msg)),
                    ("waste", c.get("waste").cloned().unwrap_or(Value::Null)),
                    ("mat", c.get("mat").cloned().unwrap_or(Value::Null)),
                    ("lab", c.get("lab").cloned().unwrap_or(Value::Null)),
                ])
            };
            let mut cr = match formula::parse(&formula, vnames) {
                Err(e) => err_row(e.message),
                Ok(p) => match formula::evaluate(&p.ast, &|name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| *v)) {
                    Ok(v) => priced(
                        obj_of(vec![
                            ("kind", s("component")),
                            ("itemId", s(id.clone())),
                            ("compId", c.get("id").cloned().unwrap_or(Value::Null)),
                            ("name", c.get("name").cloned().unwrap_or(Value::Null)),
                            ("unit", c.get("unit").cloned().unwrap_or(Value::Null)),
                            ("qty", n(v)),
                            ("waste", c.get("waste").cloned().unwrap_or(Value::Null)),
                            ("mat", c.get("mat").cloned().unwrap_or(Value::Null)),
                            ("lab", c.get("lab").cloned().unwrap_or(Value::Null)),
                        ]),
                        None,
                    ),
                    Err(e) => err_row(e.message),
                },
            };
            cr.insert("formula".into(), c.get("formula").cloned().unwrap_or(Value::Null));
            if cr.contains_key("error") {
                errors += 1.0;
            } else {
                mat += jv::num(cr.get("matExt")).unwrap_or(f64::NAN);
                lab += jv::num(cr.get("labExt")).unwrap_or(f64::NAN);
            }
            rows.push(Value::Object(cr));
        }
    }
    Estimate {
        rows,
        mat_total: mat,
        lab_total: lab,
        total: mat + lab,
        errors,
        unscaled,
        incomplete: errors > 0.0 || unscaled > 0.0,
        hours,
        worker_hours,
        days: hours / CREW_DAY,
    }
}

/// `estimate(state)` with its own quantities.
pub fn estimate_all(state: &Value) -> Estimate {
    estimate(state, &quantities(state, &QOpts::default()))
}

/// The bid areas that are add alternates.
pub fn add_alternates(state: &Value) -> HashSet<String> {
    jv::arr(state.get("areas"))
        .map(|l| l.iter().filter(|a| jv::str(a.get("alt")) == Some("add")).map(|a| jv::to_string(a.get("id"))).collect())
        .unwrap_or_default()
}

/// The base bid: the estimate without the add alternates' marks.
pub fn base_estimate(state: &Value) -> Estimate {
    let ex = add_alternates(state);
    if ex.is_empty() { estimate_all(state) } else { estimate(state, &quantities(state, &QOpts { area: None, exclude: Some(ex) })) }
}

/// The estimate of each bid area (T12).
pub fn area_estimates(state: &Value) -> Vec<(Option<Value>, Estimate)> {
    let marks = jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice);
    let used: HashSet<Option<String>> =
        marks.iter().map(|m| m.get("area").filter(|a| jv::truthy(Some(a))).map(|a| jv::to_string(Some(a)))).collect();
    let areas = jv::arr(state.get("areas")).map_or(&[][..], Vec::as_slice);
    let mut out = Vec::new();
    for a in areas {
        let id = jv::to_string(a.get("id"));
        if used.contains(&Some(id.clone())) {
            out.push((Some(a.clone()), estimate(state, &quantities(state, &QOpts { area: Some(Some(id)), exclude: None }))));
        }
    }
    if used.contains(&None) && !areas.is_empty() {
        out.push((None, estimate(state, &quantities(state, &QOpts { area: Some(None), exclude: None }))));
    }
    out
}

/// An alternate (T17).
#[derive(Debug, Clone)]
pub struct Alternate {
    pub area: Value,
    pub alt: String,
    pub est: Estimate,
    pub amount: f64,
}
pub fn alternates(state: &Value) -> Vec<Alternate> {
    let mut out = Vec::new();
    for a in jv::arr(state.get("areas")).map_or(&[][..], Vec::as_slice) {
        if !jv::truthy(a.get("alt")) {
            continue;
        }
        let alt = jv::to_string(a.get("alt"));
        let id = jv::to_string(a.get("id"));
        let est = estimate(state, &quantities(state, &QOpts { area: Some(Some(id)), exclude: None }));
        let live: HashSet<String> = est
            .rows
            .iter()
            .filter(|r| matches!(rstr(r, "kind"), "item" | "condition") && rnum(r, "marks") > 0.0)
            .map(|r| rstr(r, "itemId").to_string())
            .collect();
        let rows: Vec<Value> = est.rows.iter().filter(|r| live.contains(rstr(r, "itemId"))).cloned().collect();
        let sum = |k: &str| rows.iter().fold(0.0, |acc, r| acc + if r.get("error").is_some() { 0.0 } else { rnum(r, k) });
        let total = sum("total");
        let mat_total = sum("matExt");
        let lab_total = sum("labExt");
        let est2 = Estimate { rows, mat_total, lab_total, total, ..est };
        out.push(Alternate { area: a.clone(), amount: if alt == "add" { total } else { -total }, alt, est: est2 });
    }
    out
}
impl Alternate {
    pub fn to_json(&self) -> Value {
        json!({ "area": self.area, "alt": self.alt, "est": self.est.to_json(), "amount": n(self.amount) })
    }
    pub fn bid_in(&self) -> crate::bid::AltIn {
        crate::bid::AltIn {
            name: jv::to_string(self.area.get("name")),
            alt: self.alt.clone(),
            amount: self.amount,
            mat_total: self.est.mat_total,
            lab_total: self.est.lab_total,
        }
    }
}

/// Per-sheet breakdown rows: `[{ sheet, itemId, cond?, name, unit, qty, marks }]`.
pub fn sheet_breakdown(state: &Value, q: &Quantities) -> Vec<Value> {
    let mut rows = Vec::new();
    for it in jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice) {
        let id = jv::to_string(it.get("id"));
        let Some(qq) = q.get(&id) else { continue };
        for (sheet, t) in &qq.by_sheet {
            let own = t.marks - t.cond_marks();
            if own != 0.0 || t.conds.is_none() {
                rows.push(json!({ "sheet": sheet, "itemId": id, "name": it.get("name"), "unit": units::unit_label(&jv::to_string(it.get("unit"))), "qty": n(item_qty(it, t)), "marks": n(own) }));
            }
            for c in jv::arr(it.get("conds")).map_or(&[][..], Vec::as_slice) {
                let ck = jv::to_string(c.get("kind"));
                if let Some(ct) = t.cond(&ck) {
                    rows.push(json!({
                        "sheet": sheet, "itemId": id, "cond": ck, "name": it.get("name"),
                        "unit": units::unit_label(&jv::to_string(c.get("unit"))), "qty": n(cond_qty(c, Some(ct))), "marks": n(ct.marks),
                    }));
                }
            }
        }
    }
    rows
}

// ---------------------------------------------------------------------------------------------------- the takeoff

/// One undo step.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub before: Value,
}

/// `Takeoff`: the state plus undo/redo; every edit is one `change`.
#[derive(Debug, Clone)]
pub struct Takeoff {
    pub state: Value,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    pub version: f64,
    pub saved_version: f64,
}

impl Default for Takeoff {
    fn default() -> Self {
        Self::new(empty_state())
    }
}

/// What a `change` body returns: commit (with a value) or cancel (nothing recorded).
pub enum Ch<T> {
    Commit(T),
    Cancel,
}

fn st_obj(s: &mut Value) -> &mut Obj {
    if !s.is_object() {
        *s = Value::Object(Obj::new());
    }
    match s {
        Value::Object(o) => o,
        _ => unreachable_obj(),
    }
}
fn unreachable_obj() -> &'static mut Obj {
    Box::leak(Box::default())
}
fn list_mut<'a>(s: &'a mut Value, k: &str) -> &'a mut Vec<Value> {
    let o = st_obj(s);
    let e = o.entry(k.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    if !e.is_array() {
        *e = Value::Array(Vec::new());
    }
    match e {
        Value::Array(a) => a,
        _ => unreachable_vec(),
    }
}
fn unreachable_vec() -> &'static mut Vec<Value> {
    Box::leak(Box::default())
}
fn find_mut<'a>(list: &'a mut [Value], id: &str) -> Option<&'a mut Obj> {
    list.iter_mut().find(|x| jv::str(x.get("id")) == Some(id)).and_then(|x| if let Value::Object(o) = x { Some(o) } else { None })
}

/// `nextId(prefix)` on a state.
pub fn next_id(s: &mut Value, prefix: &str) -> String {
    let o = st_obj(s);
    let seq = jv::num(o.get("seq")).filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(0.0) + 1.0;
    o.insert("seq".into(), n(seq));
    format!("{prefix}{}", crate::jv::num_str(seq))
}

impl Takeoff {
    pub fn new(state: Value) -> Self {
        Takeoff { state, undo_stack: Vec::new(), redo_stack: Vec::new(), version: 0.0, saved_version: 0.0 }
    }
    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }
    pub fn mark_unsaved(&mut self) {
        self.saved_version = -1.0;
    }

    /// One undoable edit. `f` mutates the state; Cancel records nothing; an error restores the state.
    pub fn change<T>(&mut self, label: &str, f: impl FnOnce(&mut Value) -> Res<Ch<T>>) -> Res<Option<T>> {
        let before = self.state.clone();
        match f(&mut self.state) {
            Err(e) => {
                self.state = before;
                Err(e)
            }
            Ok(Ch::Cancel) => {
                self.state = before;
                Ok(None)
            }
            Ok(Ch::Commit(r)) => {
                self.undo_stack.push(Step { label: label.to_string(), before });
                if self.undo_stack.len() > 300 {
                    self.undo_stack.remove(0);
                }
                self.redo_stack.clear();
                self.version += 1.0;
                Ok(Some(r))
            }
        }
    }
    pub fn undo(&mut self) -> Option<String> {
        let st = self.undo_stack.pop()?;
        self.redo_stack.push(Step { label: st.label.clone(), before: self.state.clone() });
        self.state = st.before;
        self.version += 1.0;
        Some(st.label)
    }
    pub fn redo(&mut self) -> Option<String> {
        let st = self.redo_stack.pop()?;
        self.undo_stack.push(Step { label: st.label.clone(), before: self.state.clone() });
        self.state = st.before;
        self.version += 1.0;
        Some(st.label)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    // ---- reads ----
    pub fn items(&self) -> &[Value] {
        jv::arr(self.state.get("items")).map_or(&[][..], Vec::as_slice)
    }
    pub fn marks(&self) -> &[Value] {
        jv::arr(self.state.get("marks")).map_or(&[][..], Vec::as_slice)
    }
    pub fn item(&self, id: &str) -> Option<&Value> {
        self.items().iter().find(|i| jv::str(i.get("id")) == Some(id))
    }
    pub fn mark(&self, id: &str) -> Option<&Value> {
        self.marks().iter().find(|m| jv::str(m.get("id")) == Some(id))
    }
    pub fn scale_of(&self, sheet: &str) -> Option<&Value> {
        self.state.get("sheets").and_then(|s| s.get(sheet)).and_then(|s| s.get("scale")).filter(|s| jv::truthy(Some(s)))
    }
    pub fn level_of(&self, sheet: &str) -> Option<f64> {
        self.state
            .get("sheets")
            .and_then(|s| s.get(sheet))
            .and_then(|s| s.get("level"))
            .and_then(|l| if l.is_null() { None } else { jv::num(Some(l)) })
    }

    // ---- edits ----
    /// Not an undo step: opening a PDF is not an edit of the takeoff.
    pub fn add_file(&mut self, f: &Obj) -> bool {
        let files = list_mut(&mut self.state, "files");
        if files.iter().any(|x| jv::strict_eq(x.get("hash"), f.get("hash"))) {
            return false;
        }
        let mut o = Obj::new();
        for k in ["hash", "name", "size", "pages"] {
            if let Some(v) = f.get(k) {
                o.insert(k.into(), v.clone());
            }
        }
        files.push(Value::Object(o));
        true
    }
    fn level_val(v: Option<&Value>) -> Option<f64> {
        jv::finite(v).filter(|x| x.abs() < 1e5)
    }
    fn put_level(s: &mut Value, sheet: &str, next: Option<f64>) {
        let sheets = st_obj(s).entry("sheets".to_string()).or_insert_with(|| Value::Object(Obj::new()));
        let so = st_obj(sheets);
        let mut entry = match so.get(sheet) {
            Some(v) if jv::truthy(Some(v)) => jv::obj(Some(v)).cloned().unwrap_or_default(),
            _ => obj_of(vec![("scale", Value::Null)]),
        };
        match next {
            None => {
                jv::del(&mut entry, "level");
            }
            Some(x) => {
                entry.insert("level".into(), n(x));
            }
        }
        so.insert(sheet.to_string(), Value::Object(entry));
    }
    pub fn set_level(&mut self, sheet: &str, elev: Option<&Value>) -> Res<bool> {
        let next = Self::level_val(elev);
        let r = self.change(&t("undo.level"), |s| {
            let cur = s.get("sheets").and_then(|x| x.get(sheet)).and_then(|x| x.get("level")).filter(|l| !l.is_null()).cloned();
            let same = match (&cur, next) {
                (None, None) => true,
                (Some(c), Some(nx)) => jv::num(Some(c)) == Some(nx),
                _ => false,
            };
            if same {
                return Ok(Ch::Cancel);
            }
            Self::put_level(s, sheet, next);
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn set_levels(&mut self, map: &Obj) -> Res<bool> {
        let r = self.change(&t("undo.level"), |s| {
            let mut cnt: usize = 0;
            for (sheet, v) in js_entries(map) {
                let next = Self::level_val(Some(v));
                let cur = s.get("sheets").and_then(|x| x.get(sheet)).and_then(|x| x.get("level")).filter(|l| !l.is_null()).cloned();
                let same = match (&cur, next) {
                    (None, None) => true,
                    (Some(c), Some(nx)) => jv::num(Some(c)) == Some(nx),
                    _ => false,
                };
                if same {
                    continue;
                }
                Self::put_level(s, sheet, next);
                cnt = cnt.saturating_add(1);
            }
            Ok(if cnt == 0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }
    fn put_scale(s: &mut Value, sheet: &str, scale: &Value) {
        let sheets = st_obj(s).entry("sheets".to_string()).or_insert_with(|| Value::Object(Obj::new()));
        let so = st_obj(sheets);
        let mut entry = match so.get(sheet) {
            Some(v) if jv::truthy(Some(v)) => jv::obj(Some(v)).cloned().unwrap_or_default(),
            _ => Obj::new(),
        };
        entry.insert("scale".into(), scale.clone());
        so.insert(sheet.to_string(), Value::Object(entry));
    }
    pub fn set_scales(&mut self, map: &Obj, label: Option<&str>) -> Res<bool> {
        if map.is_empty() {
            return Ok(false);
        }
        let label = label.map_or_else(|| tf("undo.scales", &[("n", F::from(map.len()))]), str::to_string);
        let r = self.change(&label, |s| {
            for (k, sc) in js_entries(map) {
                Self::put_scale(s, k, sc);
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn set_scale(&mut self, sheet: &str, scale: &Value, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.scale"), str::to_string);
        let r = self.change(&label, |s| {
            Self::put_scale(s, sheet, scale);
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn add_item(&mut self, fields: &Obj) -> Res<Option<String>> {
        self.change(&t("undo.item.new"), |s| {
            if list_mut(s, "items").len() >= limits::ITEMS {
                return Err(t("err.items.many"));
            }
            let ty = is_type(fields.get("type")).unwrap_or("linear");
            let id = next_id(s, "i");
            let items = list_mut(s, "items");
            let mut f = fields.clone();
            f.insert("id".into(), s_(&id));
            f.insert("type".into(), s_(ty));
            let color = match fields.get("color") {
                Some(c) if jv::truthy(Some(c)) => c.clone(),
                _ => s_(PALETTE.get(items.len().checked_rem(PALETTE.len()).unwrap_or(0)).copied().unwrap_or("#2f6fde")),
            };
            f.insert("color".into(), color);
            items.push(Value::Object(normal_item(&f)));
            Ok(Ch::Commit(id))
        })
    }
    pub fn add_items(&mut self, list: &[Value], label: Option<&str>) -> Res<Vec<String>> {
        if list.is_empty() {
            return Ok(Vec::new());
        }
        let label = label.map_or_else(|| tf("undo.items.add", &[("n", F::from(list.len()))]), str::to_string);
        let r = self.change(&label, |s| {
            if list_mut(s, "items").len().saturating_add(list.len()) > limits::ITEMS {
                return Err(t("err.items.many"));
            }
            let mut ids = Vec::new();
            for f in list {
                let id = next_id(s, "i");
                ids.push(id.clone());
                let fo = jv::obj(Some(f)).cloned().unwrap_or_default();
                let comps: Vec<Value> = jv::arr(fo.get("components"))
                    .map(|l| {
                        l.iter()
                            .take(limits::COMPONENTS)
                            .map(|c| {
                                let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                                co.insert("id".into(), s_(&next_id(s, "c")));
                                Value::Object(co)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let items = list_mut(s, "items");
                let mut ff = fo.clone();
                ff.insert("id".into(), s_(&id));
                ff.insert("components".into(), Value::Array(comps));
                let color = match fo.get("color") {
                    Some(c) if jv::truthy(Some(c)) => c.clone(),
                    _ => s_(PALETTE.get(items.len().checked_rem(PALETTE.len()).unwrap_or(0)).copied().unwrap_or("#2f6fde")),
                };
                ff.insert("color".into(), color);
                items.push(Value::Object(normal_item(&ff)));
            }
            Ok(Ch::Commit(ids))
        })?;
        Ok(r.unwrap_or_default())
    }
    pub fn update_items(&mut self, patches: &[(String, Obj)], label: Option<&str>) -> Res<bool> {
        if patches.is_empty() {
            return Ok(false);
        }
        let label = label.map_or_else(|| tf("undo.items.reprice", &[("n", F::from(patches.len()))]), str::to_string);
        let r = self.change(&label, |s| {
            let items = list_mut(s, "items");
            for (id, patch) in patches {
                if let Some(it) = find_mut(items, id) {
                    let next = normal_item(&jv::spread(it, patch));
                    jv::assign(it, &next);
                }
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn update_item(&mut self, id: &str, patch: &Obj, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.item.edit"), str::to_string);
        let r = self.change(&label, |s| {
            let marks: Vec<(String, String)> = jv::arr(s.get("marks"))
                .map(|l| l.iter().map(|m| (jv::to_string(m.get("item")), jv::to_string(m.get("kind")))).collect())
                .unwrap_or_default();
            let items = list_mut(s, "items");
            let Some(it) = find_mut(items, id) else { return Ok(Ch::Cancel) };
            let next = normal_item(&jv::spread(it, patch));
            let kinds = kinds_of(Some(&Value::Object(next.clone())));
            let it_type = jv::to_string(it.get("type"));
            if jv::to_string(next.get("type")) != it_type && marks.iter().any(|(mi, mk)| mi == id && *mk == it_type) {
                return Err(t("err.item.type"));
            }
            if marks.iter().any(|(mi, mk)| mi == id && !kinds.contains(mk)) {
                return Err(t("err.item.cond"));
            }
            for k in ["backout", "backoutOf", "pitch", "slope", "thickFt", "baseFt", "headFt", "conds", "crew", "xd", "csi"] {
                if !next.contains_key(k) {
                    jv::del(it, k);
                }
            }
            jv::assign(it, &next);
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn set_cond(&mut self, id: &str, kind: &str, on: bool) -> Res<bool> {
        let Some(it) = self.item(id) else { return Ok(false) };
        if jv::str(it.get("type")) == Some(kind) {
            return Ok(false);
        }
        let mut list: Vec<Value> = jv::arr(it.get("conds"))
            .map(|l| l.iter().filter(|c| jv::str(c.get("kind")) != Some(kind)).cloned().collect())
            .unwrap_or_default();
        if on {
            let unit = if TYPES.contains(&kind) { s_(default_unit(kind)) } else { Value::Null };
            list.push(json!({ "kind": kind, "unit": unit }));
        }
        let patch = obj_of(vec![("conds", Value::Array(list))]);
        self.update_item(id, &patch, Some(&t(if on { "undo.cond.add" } else { "undo.cond.remove" })))
    }
    pub fn update_cond(&mut self, id: &str, kind: &str, patch: &Obj, label: Option<&str>) -> Res<bool> {
        let Some(it) = self.item(id) else { return Ok(false) };
        let Some(conds) = jv::arr(it.get("conds")).filter(|l| l.iter().any(|c| jv::str(c.get("kind")) == Some(kind))) else {
            return Ok(false);
        };
        let list: Vec<Value> = conds
            .iter()
            .map(|c| {
                if jv::str(c.get("kind")) == Some(kind) {
                    Value::Object(jv::spread(jv::obj(Some(c)).unwrap_or(&Obj::new()), patch))
                } else {
                    c.clone()
                }
            })
            .collect();
        let label = label.map_or_else(|| t("undo.cond.edit"), str::to_string);
        self.update_item(id, &obj_of(vec![("conds", Value::Array(list))]), Some(&label))
    }
    pub fn remove_item(&mut self, id: &str) -> Res<bool> {
        let r = self.change(&t("undo.item.delete"), |s| {
            let items = list_mut(s, "items");
            let n0 = items.len();
            items.retain(|i| jv::str(i.get("id")) != Some(id));
            if items.len() == n0 {
                return Ok(Ch::Cancel);
            }
            list_mut(s, "marks").retain(|m| jv::str(m.get("item")) != Some(id));
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    fn move_in(list: &mut [Value], id: &str, dir: f64) -> bool {
        let Some(i) = list.iter().position(|x| jv::str(x.get("id")) == Some(id)) else { return false };
        if !cc_m3d::js::is_integer(dir) {
            return false;
        }
        #[allow(clippy::cast_precision_loss)]
        let j = i as f64 + dir;
        #[allow(clippy::cast_precision_loss)]
        if j < 0.0 || j >= list.len() as f64 {
            return false;
        }
        // j is an integer (dir is) within 0..len (checked above): the conversion is exact
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ju = j as usize;
        list.swap(i, ju);
        true
    }
    pub fn move_item(&mut self, id: &str, dir: f64) -> Res<bool> {
        let r = self
            .change(&t("undo.item.move"), |s| Ok(if Self::move_in(list_mut(s, "items"), id, dir) { Ch::Commit(()) } else { Ch::Cancel }))?;
        Ok(r.is_some())
    }
    pub fn add_component(&mut self, item_id: &str, fields: &Obj) -> Res<Option<String>> {
        self.change(&t("undo.comp.add"), |s| {
            let can = jv::arr(s.get("items"))
                .and_then(|l| l.iter().find(|i| jv::str(i.get("id")) == Some(item_id)))
                .is_some_and(|it| jv::arr(it.get("components")).map_or(0, Vec::len) < limits::COMPONENTS);
            if !can {
                return Ok(Ch::Cancel);
            }
            let id = next_id(s, "c");
            let items = list_mut(s, "items");
            if let Some(it) = find_mut(items, item_id) {
                let mut f = fields.clone();
                f.insert("id".into(), s_(&id));
                let comp = Value::Object(normal_component(&f));
                match it.get_mut("components") {
                    Some(Value::Array(c)) => c.push(comp),
                    _ => {
                        it.insert("components".into(), Value::Array(vec![comp]));
                    }
                }
            }
            Ok(Ch::Commit(id))
        })
    }
    pub fn update_component(&mut self, item_id: &str, comp_id: &str, patch: &Obj) -> Res<bool> {
        let r = self.change(&t("undo.comp.edit"), |s| {
            let items = list_mut(s, "items");
            let Some(it) = find_mut(items, item_id) else { return Ok(Ch::Cancel) };
            let Some(Value::Array(cl)) = it.get_mut("components") else { return Ok(Ch::Cancel) };
            let Some(c) = find_mut(cl, comp_id) else { return Ok(Ch::Cancel) };
            let next = normal_component(&jv::spread(c, patch));
            jv::assign(c, &next);
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn remove_component(&mut self, item_id: &str, comp_id: &str) -> Res<bool> {
        let r = self.change(&t("undo.comp.delete"), |s| {
            let items = list_mut(s, "items");
            let Some(it) = find_mut(items, item_id) else { return Ok(Ch::Cancel) };
            let Some(Value::Array(cl)) = it.get_mut("components") else { return Ok(Ch::Cancel) };
            let n0 = cl.len();
            cl.retain(|c| jv::str(c.get("id")) != Some(comp_id));
            Ok(if cl.len() == n0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }

    fn new_mark(id: &str, item: &str, sheet: &Value, kind: &str, pts: &Value, holes: Option<&Value>) -> Obj {
        let mut m = Obj::new();
        m.insert("id".into(), s_(id));
        m.insert("item".into(), s_(item));
        m.insert("sheet".into(), sheet.clone());
        m.insert("kind".into(), s_(kind));
        m.insert("pts".into(), Value::Array(jv::arr(Some(pts)).map(|l| l.iter().map(pt).collect()).unwrap_or_default()));
        let hl = if kind == "area" {
            jv::arr(holes)
                .map(|l| l.iter().map(|h| Value::Array(jv::arr(Some(h)).map(|r| r.iter().map(pt).collect()).unwrap_or_default())).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        m.insert("holes".into(), Value::Array(hl));
        m
    }

    /// Add a mark; its kind (the item's type unless given) must be one of the item's conditions.
    pub fn add_mark(&mut self, f: &Obj) -> Res<Option<String>> {
        let item = jv::to_string(f.get("item"));
        let Some(it) = self.item(&item) else { return Err(t("err.item.none")) };
        let k = match f.get("kind") {
            Some(k) if jv::truthy(Some(k)) => jv::to_string(Some(k)),
            _ => jv::to_string(it.get("type")),
        };
        if !kinds_of(Some(it)).contains(&k) {
            return Err(t("err.mark.kind"));
        }
        let sheet = f.get("sheet").cloned().unwrap_or(Value::Null);
        let pts = f.get("pts").cloned().unwrap_or(Value::Array(Vec::new()));
        let holes = f.get("holes").cloned();
        let mut extras = f.clone();
        for key in ["item", "sheet", "pts", "holes", "kind"] {
            jv::del(&mut extras, key);
        }
        let extras_v = Value::Object(extras);
        self.change(&t(&format!("undo.mark.{k}")), |s| {
            if list_mut(s, "marks").len() >= limits::MARKS {
                return Err(t("err.marks.many"));
            }
            let id = next_id(s, "m");
            let mut m = Self::new_mark(&id, &item, &sheet, &k, &pts, holes.as_ref());
            let ex = read_extras(Some(&extras_v), &m, &extras_ctx(s));
            jv::assign(&mut m, &ex);
            list_mut(s, "marks").push(Value::Object(m));
            Ok(Ch::Commit(id))
        })
    }
    /// Several marks of one item in ONE undo step.
    pub fn add_marks(&mut self, item: &str, list: &[Value], label: Option<&str>, kind: Option<&str>) -> Res<Vec<String>> {
        let Some(it) = self.item(item) else { return Err(t("err.item.none")) };
        let k = match kind {
            Some(k) if !k.is_empty() => k.to_string(),
            _ => jv::to_string(it.get("type")),
        };
        if !kinds_of(Some(it)).contains(&k) {
            return Err(t("err.mark.kind"));
        }
        if list.is_empty() {
            return Ok(Vec::new());
        }
        let label = match label {
            Some(l) if !l.is_empty() => l.to_string(),
            _ => tf(&format!("undo.marks.{k}"), &[("n", F::from(list.len()))]),
        };
        let r = self.change(&label, |s| {
            if list_mut(s, "marks").len().saturating_add(list.len()) > limits::MARKS {
                return Err(t("err.marks.many"));
            }
            let ctx = extras_ctx(s);
            let mut ids = Vec::new();
            for m in list {
                let id = next_id(s, "m");
                ids.push(id.clone());
                let mut nm = Self::new_mark(
                    &id,
                    item,
                    m.get("sheet").unwrap_or(&Value::Null),
                    &k,
                    m.get("pts").unwrap_or(&Value::Array(Vec::new())),
                    Some(m.get("holes").filter(|h| jv::truthy(Some(h))).unwrap_or(&Value::Array(Vec::new()))),
                );
                let ex = read_extras(Some(m), &nm, &ctx);
                jv::assign(&mut nm, &ex);
                list_mut(s, "marks").push(Value::Object(nm));
            }
            Ok(Ch::Commit(ids))
        })?;
        Ok(r.unwrap_or_default())
    }
    pub fn update_mark(&mut self, id: &str, patch: &Obj, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.mark.edit"), str::to_string);
        let r = self.change(&label, |s| {
            let ctx = extras_ctx(s);
            let items: Vec<Value> = jv::arr(s.get("items")).cloned().unwrap_or_default();
            let marks = list_mut(s, "marks");
            let Some(m) = find_mut(marks, id) else { return Ok(Ch::Cancel) };
            if let Some(p) = patch.get("pts").filter(|p| jv::truthy(Some(p))) {
                let n0 = jv::arr(m.get("pts")).map_or(0, Vec::len);
                let np: Vec<Value> = jv::arr(Some(p)).map(|l| l.iter().map(pt).collect()).unwrap_or_default();
                let nl = np.len();
                m.insert("pts".into(), Value::Array(np));
                if nl != n0 && !patch.contains_key("deducts") {
                    jv::del(m, "deducts");
                }
                if nl != n0 && !patch.contains_key("slopes") {
                    jv::del(m, "slopes");
                }
            }
            if let Some(h) = patch.get("holes").filter(|h| jv::truthy(Some(h)))
                && jv::str(m.get("kind")) == Some("area")
            {
                let hl: Vec<Value> = jv::arr(Some(h))
                    .map(|l| {
                        l.iter().map(|r| Value::Array(jv::arr(Some(r)).map(|x| x.iter().map(pt).collect()).unwrap_or_default())).collect()
                    })
                    .unwrap_or_default();
                m.insert("holes".into(), Value::Array(hl));
            }
            for k in MARK_EXTRAS {
                if !patch.contains_key(k) {
                    continue;
                }
                jv::del(m, k);
                let one = Value::Object(obj_of(vec![(k, patch.get(k).cloned().unwrap_or(Value::Null))]));
                let ex = read_extras(Some(&one), m, &ctx);
                jv::assign(m, &ex);
            }
            if let Some(pi) = patch.get("item").filter(|p| jv::truthy(Some(p))) {
                let it = items.iter().find(|i| jv::strict_eq(i.get("id"), Some(pi)));
                if !kinds_of(it).contains(&jv::to_string(m.get("kind"))) {
                    return Err(t("err.mark.move"));
                }
                m.insert("item".into(), pi.clone());
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn remove_marks(&mut self, ids: &[String]) -> Res<bool> {
        let set: HashSet<&String> = ids.iter().collect();
        let r = self.change(&tf("undo.mark.delete", &[("n", F::from(set.len()))]), |s| {
            let marks = list_mut(s, "marks");
            let n0 = marks.len();
            marks.retain(|m| !jv::str(m.get("id")).is_some_and(|i| set.contains(&i.to_string())));
            Ok(if marks.len() == n0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }

    // ---- the bid summary and proposal (T19) ----
    pub fn bid(&self) -> Value {
        normal_bid(self.state.get("bid"))
    }
    pub fn set_bid(&mut self, patch: &Obj, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.bid"), str::to_string);
        let r = self.change(&label, |s| {
            let cur = normal_bid(s.get("bid"));
            let mut merged = jv::obj(Some(&cur)).cloned().unwrap_or_default();
            jv::assign(&mut merged, patch);
            let next = normal_bid(Some(&Value::Object(merged)));
            if jv::stringify(&next) == jv::stringify(&cur) {
                return Ok(Ch::Cancel);
            }
            st_obj(s).insert("bid".into(), next);
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }

    // ---- markups (T22) ----
    pub fn note(&self, id: &str) -> Option<&Value> {
        jv::arr(self.state.get("notes")).and_then(|l| l.iter().find(|x| jv::str(x.get("id")) == Some(id)))
    }
    pub fn add_note(&mut self, fields: &Obj) -> Res<Option<String>> {
        let kind = jv::str(fields.get("kind")).filter(|k| NOTE_KINDS.contains(k)).unwrap_or("note");
        self.change(&t(&format!("undo.note.{kind}")), |s| {
            if list_mut(s, "notes").len() >= limits::NOTES {
                return Err(t("err.notes.many"));
            }
            let seq = jv::num(s.get("seq")).unwrap_or(f64::NAN);
            let mut f = fields.clone();
            f.insert("id".into(), s_(&format!("n{}", crate::jv::num_str(seq + 1.0))));
            let Some(nn) = normal_note(&f) else { return Ok(Ch::Cancel) };
            let id = next_id(s, "n");
            let mut no = jv::obj(Some(&nn)).cloned().unwrap_or_default();
            no.insert("id".into(), s_(&id));
            list_mut(s, "notes").push(Value::Object(no));
            Ok(Ch::Commit(id))
        })
    }
    pub fn update_note(&mut self, id: &str, patch: &Obj, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.note.edit"), str::to_string);
        let r = self.change(&label, |s| {
            let notes = list_mut(s, "notes");
            let Some(i) = notes.iter().position(|x| jv::str(x.get("id")) == Some(id)) else { return Ok(Ch::Cancel) };
            let Some(cur) = notes.get(i).and_then(|x| jv::obj(Some(x))).cloned() else { return Ok(Ch::Cancel) };
            let mut f = jv::spread(&cur, patch);
            f.insert("id".into(), s_(id));
            f.insert("sheet".into(), cur.get("sheet").cloned().unwrap_or(Value::Null));
            f.insert("kind".into(), cur.get("kind").cloned().unwrap_or(Value::Null));
            let Some(nn) = normal_note(&f) else { return Ok(Ch::Cancel) };
            if jv::stringify(&nn) == jv::stringify(&Value::Object(cur)) {
                return Ok(Ch::Cancel);
            }
            if let Some(slot) = notes.get_mut(i) {
                *slot = nn;
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn remove_note(&mut self, id: &str) -> Res<bool> {
        let r = self.change(&t("undo.note.delete"), |s| {
            let notes = list_mut(s, "notes");
            let n0 = notes.len();
            notes.retain(|x| jv::str(x.get("id")) != Some(id));
            Ok(if notes.len() == n0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }

    // ---- typical groups (T16) ----
    pub fn group(&self, id: &str) -> Option<&Value> {
        jv::arr(self.state.get("groups")).and_then(|l| l.iter().find(|x| jv::str(x.get("id")) == Some(id)))
    }
    pub fn add_group(&mut self, name: Option<&Value>, repeat: Option<&Value>, ids: &[String]) -> Res<Option<String>> {
        let one = n(1.0);
        let repeat = repeat.unwrap_or(&one);
        self.change(&t("undo.group.new"), |s| {
            if list_mut(s, "groups").len() >= limits::GROUPS {
                return Err(t("err.group.many"));
            }
            let nm = clean_str(name, limits::NAME);
            if nm.is_empty() {
                return Ok(Ch::Cancel);
            }
            let Some(rp) = repeat_ok(Some(repeat)) else {
                return Err(tf("err.group.repeat", &[("max", F::N(limits::REPEAT))]));
            };
            let id = next_id(s, "g");
            list_mut(s, "groups").push(json!({ "id": id, "name": nm, "repeat": n(rp) }));
            let set: HashSet<&String> = ids.iter().collect();
            for m in list_mut(s, "marks").iter_mut() {
                if jv::str(m.get("id")).is_some_and(|i| set.contains(&i.to_string()))
                    && let Value::Object(mo) = m
                {
                    mo.insert("group".into(), s_(&id));
                }
            }
            Ok(Ch::Commit(id))
        })
    }
    pub fn update_group(&mut self, id: &str, name: Option<&Value>, repeat: Option<&Value>) -> Res<bool> {
        let label = if repeat.is_some() { t("undo.group.repeat") } else { t("undo.group.rename") };
        let r = self.change(&label, |s| {
            let groups = list_mut(s, "groups");
            let Some(g) = find_mut(groups, id) else { return Ok(Ch::Cancel) };
            if name.is_some() {
                let nm = clean_str(name, limits::NAME);
                if nm.is_empty() {
                    return Ok(Ch::Cancel);
                }
                g.insert("name".into(), s_(&nm));
            }
            if let Some(rv) = repeat {
                let Some(rp) = repeat_ok(Some(rv)) else {
                    return Err(tf("err.group.repeat", &[("max", F::N(limits::REPEAT))]));
                };
                g.insert("repeat".into(), n(rp));
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn remove_group(&mut self, id: &str) -> Res<bool> {
        let r = self.change(&t("undo.group.delete"), |s| {
            let groups = list_mut(s, "groups");
            let n0 = groups.len();
            groups.retain(|g| jv::str(g.get("id")) != Some(id));
            if groups.len() == n0 {
                return Ok(Ch::Cancel);
            }
            for m in list_mut(s, "marks").iter_mut() {
                if jv::str(m.get("group")) == Some(id)
                    && let Value::Object(mo) = m
                {
                    jv::del(mo, "group");
                }
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    fn set_marks_field(&mut self, ids: &[String], field: &str, list: &str, value: Option<&str>, err_key: &str, label: &str) -> Res<bool> {
        let set: HashSet<&String> = ids.iter().collect();
        let r = self.change(label, |s| {
            if let Some(v) = value
                && !jv::arr(s.get(list)).is_some_and(|l| l.iter().any(|g| jv::str(g.get("id")) == Some(v)))
            {
                return Err(t(err_key));
            }
            let mut cnt: usize = 0;
            for m in list_mut(s, "marks").iter_mut() {
                let Value::Object(mo) = m else { continue };
                if !jv::str(mo.get("id")).is_some_and(|i| set.contains(&i.to_string())) {
                    continue;
                }
                let cur = mo.get(field).filter(|x| jv::truthy(Some(x))).map(|x| jv::to_string(Some(x)));
                if cur.as_deref() == value {
                    continue;
                }
                match value {
                    Some(v) => {
                        mo.insert(field.into(), s_(v));
                    }
                    None => jv::del(mo, field),
                }
                cnt = cnt.saturating_add(1);
            }
            Ok(if cnt == 0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }
    pub fn set_marks_group(&mut self, ids: &[String], group: Option<&str>, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.group.set"), str::to_string);
        self.set_marks_field(ids, "group", "groups", group.filter(|g| !g.is_empty()), "err.group.none", &label)
    }

    // ---- bid areas (T12) ----
    pub fn area(&self, id: &str) -> Option<&Value> {
        jv::arr(self.state.get("areas")).and_then(|l| l.iter().find(|x| jv::str(x.get("id")) == Some(id)))
    }
    pub fn add_area(&mut self, name: Option<&Value>) -> Res<Option<String>> {
        self.change(&t("undo.area.new"), |s| {
            if list_mut(s, "areas").len() >= limits::AREAS {
                return Err(t("err.area.many"));
            }
            let nm = clean_str(name, limits::NAME);
            if nm.is_empty() {
                return Ok(Ch::Cancel);
            }
            let id = next_id(s, "b");
            list_mut(s, "areas").push(json!({ "id": id, "name": nm }));
            Ok(Ch::Commit(id))
        })
    }
    pub fn rename_area(&mut self, id: &str, name: Option<&Value>) -> Res<bool> {
        let r = self.change(&t("undo.area.rename"), |s| {
            let nm = clean_str(name, limits::NAME);
            let areas = list_mut(s, "areas");
            let Some(a) = find_mut(areas, id) else { return Ok(Ch::Cancel) };
            if nm.is_empty() || jv::str(a.get("name")) == Some(nm.as_str()) {
                return Ok(Ch::Cancel);
            }
            a.insert("name".into(), s_(&nm));
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn set_area_alt(&mut self, id: &str, alt: Option<&str>) -> Res<bool> {
        let r = self.change(&t("undo.area.alt"), |s| {
            let v = alt.filter(|a| *a == "add" || *a == "deduct");
            let areas = list_mut(s, "areas");
            let Some(a) = find_mut(areas, id) else { return Ok(Ch::Cancel) };
            let cur = a.get("alt").filter(|x| jv::truthy(Some(x))).map(|x| jv::to_string(Some(x)));
            if cur.as_deref() == v {
                return Ok(Ch::Cancel);
            }
            match v {
                Some(v) => {
                    a.insert("alt".into(), s_(v));
                }
                None => jv::del(a, "alt"),
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn remove_area(&mut self, id: &str) -> Res<bool> {
        let r = self.change(&t("undo.area.delete"), |s| {
            let areas = list_mut(s, "areas");
            let n0 = areas.len();
            areas.retain(|a| jv::str(a.get("id")) != Some(id));
            if areas.len() == n0 {
                return Ok(Ch::Cancel);
            }
            for m in list_mut(s, "marks").iter_mut() {
                if jv::str(m.get("area")) == Some(id)
                    && let Value::Object(mo) = m
                {
                    jv::del(mo, "area");
                }
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(r.is_some())
    }
    pub fn move_area(&mut self, id: &str, dir: f64) -> Res<bool> {
        let r = self
            .change(&t("undo.area.move"), |s| Ok(if Self::move_in(list_mut(s, "areas"), id, dir) { Ch::Commit(()) } else { Ch::Cancel }))?;
        Ok(r.is_some())
    }
    pub fn set_marks_area(&mut self, ids: &[String], area: Option<&str>, label: Option<&str>) -> Res<bool> {
        let label = label.map_or_else(|| t("undo.area.set"), str::to_string);
        self.set_marks_field(ids, "area", "areas", area.filter(|a| !a.is_empty()), "err.area.none", &label)
    }

    // ---- revisions (T13) ----
    /// Move marks onto another sheet in ONE undo step: each mark's points through `mv`. → number moved
    pub fn carry_marks(
        &mut self,
        ids: &[String],
        to: &str,
        mv: &dyn Fn(&[Value]) -> Vec<Value>,
        flagged: &HashSet<String>,
        scale: Option<&Value>,
        label: Option<&str>,
    ) -> Res<f64> {
        let set: HashSet<&String> = ids.iter().collect();
        let label = label.map_or_else(|| t("undo.rev.carry"), str::to_string);
        let mut cnt = 0.0;
        self.change(&label, |s| {
            for m in list_mut(s, "marks").iter_mut() {
                let Value::Object(mo) = m else { continue };
                let Some(mid) = jv::str(mo.get("id")).map(str::to_string) else { continue };
                if !set.contains(&mid) {
                    continue;
                }
                mo.insert("sheet".into(), s_(to));
                let pts = mv(jv::arr(mo.get("pts")).map_or(&[][..], Vec::as_slice));
                mo.insert("pts".into(), Value::Array(pts.iter().map(pt).collect()));
                if let Some(hl) = jv::arr(mo.get("holes")).filter(|h| !h.is_empty()).cloned() {
                    let nh: Vec<Value> = hl
                        .iter()
                        .map(|h| Value::Array(mv(jv::arr(Some(h)).map_or(&[][..], Vec::as_slice)).iter().map(pt).collect()))
                        .collect();
                    mo.insert("holes".into(), Value::Array(nh));
                }
                if flagged.contains(&mid) {
                    mo.insert("flag".into(), s_("rev"));
                } else {
                    jv::del(mo, "flag");
                }
                cnt += 1.0;
            }
            if cnt == 0.0 {
                return Ok(Ch::Cancel);
            }
            if let Some(sc) = scale.filter(|x| jv::truthy(Some(x))) {
                let has = s.get("sheets").and_then(|x| x.get(to)).and_then(|x| x.get("scale")).is_some_and(|x| jv::truthy(Some(x)));
                if !has {
                    Self::put_scale(s, to, sc);
                }
            }
            Ok(Ch::Commit(()))
        })?;
        Ok(cnt)
    }
    pub fn clear_flags(&mut self, ids: &[String]) -> Res<bool> {
        let set: HashSet<&String> = ids.iter().collect();
        let r = self.change(&t("undo.rev.checked"), |s| {
            let mut cnt: usize = 0;
            for m in list_mut(s, "marks").iter_mut() {
                let Value::Object(mo) = m else { continue };
                if jv::str(mo.get("id")).is_some_and(|i| set.contains(&i.to_string())) && jv::truthy(mo.get("flag")) {
                    jv::del(mo, "flag");
                    cnt = cnt.saturating_add(1);
                }
            }
            Ok(if cnt == 0 { Ch::Cancel } else { Ch::Commit(()) })
        })?;
        Ok(r.is_some())
    }

    // ---- files ----
    /// The project file (`toJSON(savedAt)`).
    pub fn to_json(&self, saved_at: &str) -> Value {
        project_json(&self.state, saved_at)
    }
    /// Replace the project with a parsed `.takeoff.json` (validated).
    pub fn load(&mut self, json: &Value) -> Result<Vec<String>, String> {
        let r = read_project(json)?;
        self.state = r.state;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.version += 1.0;
        self.saved_version = self.version;
        Ok(r.warnings)
    }
}

fn s_(x: &str) -> Value {
    Value::String(x.to_string())
}

/// `toJSON(savedAt)` of a state.
pub fn project_json(st: &Value, saved_at: &str) -> Value {
    let mut o = Obj::new();
    o.insert("format".into(), s_(FORMAT));
    o.insert("version".into(), n(FORMAT_VERSION));
    o.insert("app".into(), s_("TakeoffCraft"));
    o.insert("savedAt".into(), s_(saved_at));
    if let Some(v) = st.get("name") {
        o.insert("name".into(), v.clone());
    }
    if let Some(v) = st.get("seq") {
        o.insert("seq".into(), v.clone());
    }
    let files: Vec<Value> = jv::arr(st.get("files"))
        .map(|l| {
            l.iter()
                .map(|f| {
                    let mut fo = Obj::new();
                    for k in ["hash", "name", "size", "pages"] {
                        if let Some(v) = f.get(k) {
                            fo.insert(k.into(), v.clone());
                        }
                    }
                    Value::Object(fo)
                })
                .collect()
        })
        .unwrap_or_default();
    o.insert("files".into(), Value::Array(files));
    if let Some(v) = st.get("sheets") {
        o.insert("sheets".into(), v.clone());
    }
    if let Some(areas) = jv::arr(st.get("areas")).filter(|l| !l.is_empty()) {
        let l: Vec<Value> = areas
            .iter()
            .map(|a| {
                let mut ao = Obj::new();
                ao.insert("id".into(), a.get("id").cloned().unwrap_or(Value::Null));
                ao.insert("name".into(), a.get("name").cloned().unwrap_or(Value::Null));
                if jv::truthy(a.get("alt"))
                    && let Some(v) = a.get("alt")
                {
                    ao.insert("alt".into(), v.clone());
                }
                Value::Object(ao)
            })
            .collect();
        o.insert("areas".into(), Value::Array(l));
    }
    if bid_used(st.get("bid")) {
        o.insert("bid".into(), normal_bid(st.get("bid")));
    }
    if let Some(notes) = jv::arr(st.get("notes")).filter(|l| !l.is_empty()) {
        let l: Vec<Value> = notes
            .iter()
            .map(|nn| {
                json!({
                    "id": nn.get("id"), "sheet": nn.get("sheet"), "kind": nn.get("kind"),
                    "pts": jv::arr(nn.get("pts")).map(|p| p.iter().map(|q| json!([q.get("x"), q.get("y")])).collect::<Vec<_>>()).unwrap_or_default(),
                    "text": nn.get("text"), "status": nn.get("status"),
                })
            })
            .collect();
        o.insert("notes".into(), Value::Array(l));
    }
    if let Some(groups) = jv::arr(st.get("groups")).filter(|l| !l.is_empty()) {
        let l: Vec<Value> = groups.iter().map(|g| json!({ "id": g.get("id"), "name": g.get("name"), "repeat": g.get("repeat") })).collect();
        o.insert("groups".into(), Value::Array(l));
    }
    if let Some(v) = st.get("items") {
        o.insert("items".into(), v.clone());
    }
    let marks: Vec<Value> = jv::arr(st.get("marks"))
        .map(|l| {
            l.iter()
                .map(|m| {
                    let mut mo = Obj::new();
                    for k in ["id", "item", "sheet", "kind"] {
                        if let Some(v) = m.get(k) {
                            mo.insert(k.into(), v.clone());
                        }
                    }
                    mo.insert(
                        "pts".into(),
                        Value::Array(jv::arr(m.get("pts")).map(|p| p.iter().map(arr_of).collect()).unwrap_or_default()),
                    );
                    if let Some(h) = jv::arr(m.get("holes")).filter(|h| !h.is_empty()) {
                        mo.insert(
                            "holes".into(),
                            Value::Array(
                                h.iter()
                                    .map(|r| Value::Array(jv::arr(Some(r)).map(|p| p.iter().map(arr_of).collect()).unwrap_or_default()))
                                    .collect(),
                            ),
                        );
                    }
                    jv::assign(&mut mo, &mark_extras(m));
                    Value::Object(mo)
                })
                .collect()
        })
        .unwrap_or_default();
    o.insert("marks".into(), Value::Array(marks));
    Value::Object(o)
}
