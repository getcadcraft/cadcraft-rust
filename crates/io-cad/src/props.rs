//! dwg-props.js: the fields the Properties panel sets, as DWG and DXF keep them — transparency words, CadCraft's xdata
//! for fields with no group code, and the dimension variables (DIMVARS) both ways.

use cc_cad::js;
use cc_io_dwg::value::{Obj, V};

use crate::jsv::{self, Ov};
use crate::text::slice16;

// ------------------------------------------------------------------------------------------------ transparency

fn pct(alpha: f64) -> Option<V> {
    let p = js::round((1.0 - alpha / 255.0) * 100.0);
    if p > 0.0 { Some(V::Num(js::min(90.0, p))) } else { None }
}

/// `trFrom(v)`: a DXF 440 word or a cc-dwg transparency ({ byLayer } | { byBlock } | { alpha }) → tr ('ByBlock' or
/// 0–90) or None.
pub fn tr_from(v: Ov) -> Option<V> {
    match v {
        None | Some(V::Null) => None,
        Some(V::Str(s)) if s.is_empty() => None,
        Some(V::Obj(_) | V::Arr(_) | V::Bytes(_)) => {
            if jsv::truthy(jsv::get(v, "byBlock")) {
                return Some(V::from("ByBlock"));
            }
            jsv::fin(jsv::get(v, "alpha")).and_then(pct)
        }
        Some(_) => {
            let w = jsv::u32_of(jsv::to_num(v));
            match w >> 24 {
                1 => Some(V::from("ByBlock")),
                2 | 3 => pct(jsv::i2f(i64::try_from(w & 0xff).unwrap_or(0))),
                _ => None,
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ xdata

const STR: usize = 250;

/// `chunks(prefix, s)`: prefix + pieces of s, each at most 250 characters in all (UTF-16 units).
pub fn chunks(prefix: &str, s: &str) -> Vec<String> {
    let step = STR.saturating_sub(crate::text::len16(prefix)).max(1);
    let n = crate::text::len16(s);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n || out.is_empty() {
        out.push(format!("{prefix}{}", slice16(s, i, i.saturating_add(step))));
        i = i.saturating_add(step);
    }
    out
}
fn joined(strs: &[String], prefix: &str) -> String {
    strs.iter().filter(|x| x.starts_with(prefix)).map(|x| x.get(prefix.len()..).unwrap_or("")).collect::<Vec<_>>().join("")
}

fn hex6(n: u64) -> String {
    format!("#{:06x}", n & 0xff_ffff)
}

/// `colorWord(c)`: '#rrggbb' → 0xC2rrggbb, ACI 1–255 → 0xC3000000 | c.
fn color_word(c: Ov) -> Option<u64> {
    if let Some(s) = jsv::s(c)
        && is_hex_color(s)
    {
        let n = u64::from_str_radix(s.get(1..).unwrap_or("0"), 16).unwrap_or(0);
        return Some(0xC200_0000 | n);
    }
    if let Some(n) = jsv::num(c)
        && n.fract() == 0.0
        && (1.0..=255.0).contains(&n)
    {
        return Some(0xC300_0000 | jsv::f2u(n));
    }
    None
}
fn color_from_word(v: Ov) -> Option<V> {
    let w = jsv::u32_of(jsv::to_num(v));
    let m = w >> 24;
    if m == 0xC2 {
        return Some(V::Str(hex6(w)));
    }
    if m == 0xC3 && (w & 0xff) >= 1 {
        return Some(V::from(w & 0xff));
    }
    None
}
/// `/^#[0-9a-f]{6}$/i`
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.bytes().skip(1).all(|c| c.is_ascii_hexdigit())
}

/// The applications whose xdata CadCraft reads itself; every other one passes through as e.xd.
const KNOWN_APPS: [&str; 5] = ["ACAD", "CADCRAFT", "PE_URL", "ACADANNOTATIVE", "HATCHBACKGROUNDCOLOR"];

/// `[c, v] = item` of an xdata list (a non-array item reads as two undefineds).
pub fn cv(item: &V) -> (Ov<'_>, Ov<'_>) {
    (jsv::at(Some(item), 0), jsv::at(Some(item), 1))
}
fn code_is(c: Ov, n: f64) -> bool {
    jsv::num(c) == Some(n)
}

/// `propsFromXdata(xd)`: xdata by application → { mat?, url?, ann?, bg?, ox?, oy?, assoc?, tr?, xd? }.
pub fn props_from_xdata(xd: Ov) -> Obj {
    let mut out = Obj::new();
    let Some(V::Obj(x)) = xd else {
        if matches!(xd, Some(V::Arr(_))) {
            // an array is an object too: its entries are index keys (never an application CadCraft knows)
            return out;
        }
        return out;
    };
    let app = |n: &str| -> Option<&[V]> {
        let up = jsv::upper(n);
        let (_, v) = x.iter().find(|(k, _)| jsv::upper(k) == up)?;
        Some(if v.truthy() { jsv::arr(Some(v)) } else { &[] })
    };
    let cc: Vec<String> =
        app("CADCRAFT").unwrap_or(&[]).iter().filter(|it| code_is(cv(it).0, 1000.0)).map(|it| jsv::to_str(cv(it).1)).collect();
    let mat = joined(&cc, "CCMAT:");
    if !mat.is_empty() {
        out.set("mat", mat);
    }
    if let Some(org) = cc.iter().find(|s| s.starts_with("CCHORG:")) {
        let parts: Vec<f64> = org.get(7..).unwrap_or("").split(',').map(js::str_to_number).collect();
        let x = parts.first().copied().unwrap_or(f64::NAN);
        let y = parts.get(1).copied().unwrap_or(f64::NAN);
        if x.is_finite() && y.is_finite() {
            if x != 0.0 {
                out.set("ox", x);
            }
            if y != 0.0 {
                out.set("oy", y);
            }
        }
    }
    if cc.iter().any(|s| s == "CCHASSOC") {
        out.set("assoc", true);
    }
    if let Some(tr) = cc.iter().find(|s| s.starts_with("CCTR:")) {
        let v = tr.get(5..).unwrap_or("");
        if v == "ByBlock" {
            out.set("tr", v);
        } else {
            let n = js::str_to_number(v);
            if n.is_finite() && (0.0..=90.0).contains(&n) {
                out.set("tr", n);
            }
        }
    }
    if let Some(url) = app("PE_URL").unwrap_or(&[]).iter().find(|it| code_is(cv(it).0, 1000.0)) {
        let s = jsv::to_str(cv(url).1);
        if !js::trim(&s).is_empty() {
            out.set("url", slice16(&s, 0, 2048));
        }
    }
    if let Some(ann) = app("AcadAnnotative")
        && ann.iter().any(|it| code_is(cv(it).0, 1000.0) && jsv::to_str(cv(it).1).eq_ignore_ascii_case("AnnotativeData"))
    {
        let ints: Vec<f64> = ann.iter().filter(|it| code_is(cv(it).0, 1070.0)).map(|it| jsv::to_num(cv(it).1)).collect();
        if ints.get(1) == Some(&1.0) || (ints.len() == 1 && ints.first() == Some(&1.0)) {
            out.set("ann", true);
        }
    }
    if let Some(bg) = app("HATCHBACKGROUNDCOLOR").unwrap_or(&[]).iter().find(|it| code_is(cv(it).0, 1071.0))
        && let Some(c) = color_from_word(cv(bg).1)
    {
        out.set("bg", c);
    }
    let mut keep = Obj::new();
    for (name, items) in x.iter() {
        let up = jsv::upper(name);
        let V::Arr(list) = items else { continue };
        if KNOWN_APPS.contains(&up.as_str()) || list.is_empty() || list.iter().any(|it| jsv::to_num(cv(it).0) == 1005.0) {
            continue;
        }
        let mapped: Vec<V> = list
            .iter()
            .map(|it| {
                let (c, v) = cv(it);
                let n = jsv::to_num(c);
                let val = if (1010.0..=1071.0).contains(&n) { V::Num(jsv::to_num(v)) } else { V::Str(jsv::to_str(v)) };
                V::Arr(vec![V::Num(n), val])
            })
            .collect();
        keep.set(up, V::Arr(mapped));
    }
    if !keep.is_empty() {
        out.set("xd", keep);
    }
    out
}

/// `propsXdata(e, { r12 })`: the entity's own xdata for CadCraft fields with no group code → { APP: [[code, value]…] }.
pub fn props_xdata(e: &Obj, r12: bool) -> Obj {
    let g = |k: &str| e.get(k);
    let mut x = Obj::new();
    let mut cc: Vec<String> = Vec::new();
    let tr = g("tr");
    if r12 && (jsv::fin(tr).is_some() || jsv::s(tr) == Some("ByBlock")) {
        cc.push(format!("CCTR:{}", jsv::to_str(tr)));
    }
    if let Some(m) = jsv::s(g("mat")).filter(|m| !m.is_empty()) {
        cc.extend(chunks("CCMAT:", &slice16(m, 0, 255)));
    }
    let hatch = jsv::s(g("type")) == Some("hatch");
    if hatch
        && !jsv::truthy(g("solid"))
        && (jsv::fin(g("ox")).is_some() || jsv::fin(g("oy")).is_some())
        && (jsv::truthy(g("ox")) || jsv::truthy(g("oy")))
    {
        cc.push(format!("CCHORG:{},{}", js::num_to_string(jsv::or(g("ox"), 0.0)), js::num_to_string(jsv::or(g("oy"), 0.0))));
    }
    if hatch && jsv::truthy(g("assoc")) {
        cc.push("CCHASSOC".into());
    }
    if !cc.is_empty() {
        x.set("CADCRAFT", V::Arr(cc.into_iter().map(|s| V::Arr(vec![V::Num(1000.0), V::Str(s)])).collect()));
    }
    if let Some(u) = jsv::s(g("url")).filter(|u| !u.is_empty()) {
        x.set("PE_URL", V::Arr(vec![V::Arr(vec![V::Num(1000.0), V::Str(slice16(u, 0, 255))])]));
    }
    if jsv::truthy(g("ann")) {
        let it = |c: f64, v: V| V::Arr(vec![V::Num(c), v]);
        x.set(
            "AcadAnnotative",
            V::Arr(vec![
                it(1000.0, V::from("AnnotativeData")),
                it(1002.0, V::from("{")),
                it(1070.0, V::Num(1.0)),
                it(1070.0, V::Num(1.0)),
                it(1002.0, V::from("}")),
            ]),
        );
    }
    if hatch
        && !jsv::nullish(g("bg"))
        && let Some(w) = color_word(g("bg"))
    {
        x.set(
            "HATCHBACKGROUNDCOLOR",
            V::Arr(vec![V::Arr(vec![V::Num(1071.0), V::Num(js::int32(jsv::i2f(i64::try_from(w).unwrap_or(0))))])]),
        );
    }
    if let Some(V::Obj(xd)) = g("xd") {
        for (app, items) in xd.iter() {
            if x.get(app).is_some_and(V::truthy) {
                continue;
            }
            if let V::Arr(a) = items
                && !a.is_empty()
            {
                x.set(app.to_string(), items.clone());
            }
        }
    }
    x
}

// ------------------------------------------------------------------------------------------------ dimension variables

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Real,
    Int,
    Bool,
    Color,
    Lw,
    Sep,
}
use Kind::*;

/// [st key, variable, DXF group code, kind, in R12].
pub const DIMVARS: [(&str, &str, i64, Kind, bool); 46] = [
    ("scale", "DIMSCALE", 40, Real, true),
    ("asz", "DIMASZ", 41, Real, true),
    ("exo", "DIMEXO", 42, Real, true),
    ("dli", "DIMDLI", 43, Real, true),
    ("exe", "DIMEXE", 44, Real, true),
    ("rnd", "DIMRND", 45, Real, true),
    ("tp", "DIMTP", 47, Real, true),
    ("tm", "DIMTM", 48, Real, true),
    ("txt", "DIMTXT", 140, Real, true),
    ("tsz", "DIMTSZ", 142, Real, true),
    ("altf", "DIMALTF", 143, Real, true),
    ("lfac", "DIMLFAC", 144, Real, true),
    ("tfac", "DIMTFAC", 146, Real, true),
    ("gap", "DIMGAP", 147, Real, true),
    ("altrnd", "DIMALTRND", 148, Real, false),
    ("se1", "DIMSE1", 75, Bool, true),
    ("se2", "DIMSE2", 76, Bool, true),
    ("tad", "DIMTAD", 77, Int, true),
    ("zin", "DIMZIN", 78, Int, true),
    ("azin", "DIMAZIN", 79, Int, false),
    ("alt", "DIMALT", 170, Bool, true),
    ("altd", "DIMALTD", 171, Int, true),
    ("tofl", "DIMTOFL", 172, Bool, true),
    ("tix", "DIMTIX", 174, Bool, true),
    ("clrd", "DIMCLRD", 176, Color, true),
    ("clre", "DIMCLRE", 177, Color, true),
    ("clrt", "DIMCLRT", 178, Color, true),
    ("adec", "DIMADEC", 179, Int, false),
    ("dec", "DIMDEC", 271, Int, false),
    ("tdec", "DIMTDEC", 272, Int, false),
    ("altu", "DIMALTU", 273, Int, false),
    ("aunit", "DIMAUNIT", 275, Int, false),
    ("lunit", "DIMLUNIT", 277, Int, false),
    ("dsep", "DIMDSEP", 278, Sep, false),
    ("tmove", "DIMTMOVE", 279, Int, false),
    ("just", "DIMJUST", 280, Int, false),
    ("sd1", "DIMSD1", 281, Bool, false),
    ("sd2", "DIMSD2", 282, Bool, false),
    ("tolj", "DIMTOLJ", 283, Int, false),
    ("tzin", "DIMTZIN", 284, Int, false),
    ("altz", "DIMALTZ", 285, Int, false),
    ("atfit", "DIMATFIT", 289, Int, false),
    ("lwd", "DIMLWD", 371, Lw, false),
    ("lwe", "DIMLWE", 372, Lw, false),
    ("tfill", "DIMTFILL", 69, Int, false),
    ("tfillclr", "DIMTFILLCLR", 70, Color, false),
];
const SPECIAL: [(i64, &str); 8] =
    [(3, "DIMPOST"), (4, "DIMAPOST"), (71, "DIMTOL"), (72, "DIMLIM"), (73, "DIMTIH"), (74, "DIMTOH"), (173, "DIMSAH"), (270, "DIMUNIT")];

/// dims.js DIM_DEFAULTS as an object.
pub fn dim_defaults() -> Obj {
    let mut o = Obj::new();
    for (k, v) in cc_cad::doc::DIM_DEFAULTS {
        o.set(k, v);
    }
    o
}
/// dimstyle.js DIM_EXTRA as an object.
pub fn dim_extra() -> Obj {
    match jsv::from_json(&serde_json::Value::Object(cc_cad_geom::dimstyle::dim_extra())) {
        V::Obj(o) => o,
        _ => Obj::new(),
    }
}
/// `{ ...DIM_DEFAULTS, ...DIM_EXTRA }`
fn def() -> Obj {
    let mut o = dim_defaults();
    o.assign(dim_extra());
    o
}

/// `parseFloat(n)` for any value (ToString first).
fn pf(n: Ov) -> f64 {
    match n {
        Some(V::Num(x)) => *x,
        _ => jsv::parse_float(&jsv::to_str(n)),
    }
}

/// `in1(kind, raw)`: a variable's value → the st value (checked), or None.
fn in1(kind: Kind, raw: Ov) -> Option<V> {
    let conv: Option<V> = match raw {
        Some(V::Obj(_) | V::Arr(_)) => {
            let rgb = jsv::get(raw, "rgb");
            if rgb.is_some() { Some(V::Str(hex6(jsv::u32_of(js::int32(jsv::to_num(rgb)))))) } else { jsv::get(raw, "index").cloned() }
        }
        other => other.cloned(),
    };
    let n = conv.as_ref();
    match kind {
        Real => {
            let x = pf(n);
            x.is_finite().then_some(V::Num(x))
        }
        Int => {
            let x = js::round(pf(n));
            x.is_finite().then_some(V::Num(x))
        }
        Bool => {
            let x = pf(n);
            Some(V::Bool((x != 0.0 && !x.is_nan()) || matches!(n, Some(V::Bool(true)))))
        }
        Color => {
            if let Some(s) = jsv::s(n)
                && is_hex_color(s)
            {
                return Some(V::Str(s.to_lowercase()));
            }
            let x = js::round(pf(n));
            if x == 256.0 {
                Some(V::Null)
            } else if x == 0.0 {
                Some(V::Num(0.0))
            } else if (1.0..=255.0).contains(&x) {
                Some(V::Num(x))
            } else {
                None
            }
        }
        Lw => {
            let x = js::round(pf(n));
            if x == -1.0 || x == -2.0 || x == -3.0 {
                Some(V::Num(x))
            } else if (0.0..=211.0).contains(&x) {
                Some(V::Num(x / 100.0))
            } else {
                None
            }
        }
        Sep => Some(V::from(if js::round(pf(n)) == 44.0 { "," } else { "." })),
    }
}

/// `post(s)`: DIMPOST "pre<>suf" → (pre, suf); a text with no "<>" is a suffix.
fn post(v: Ov) -> (String, String) {
    let s = jsv::str_nn(v);
    match s.find("<>") {
        None => (String::new(), s),
        Some(k) => (s.get(..k).unwrap_or("").to_string(), s.get(k.saturating_add(2)..).unwrap_or("").to_string()),
    }
}

fn specials(get: &dyn Fn(&str) -> Option<V>, st: &mut Obj) {
    let tih = get("DIMTIH");
    let toh = get("DIMTOH");
    if tih.is_some() || toh.is_some() {
        let t1 = tih.clone().unwrap_or(V::Num(0.0));
        let t2 = toh.or(tih).unwrap_or(V::Num(0.0));
        let b = |v: &V| {
            let x = jsv::to_num(Some(v));
            x != 0.0 && !x.is_nan()
        };
        st.set("tih", b(&t1) && b(&t2));
    }
    for (name, kp, ks) in [("DIMPOST", "pre", "suf"), ("DIMAPOST", "apre", "asuf")] {
        if let Some(p) = get(name) {
            let (pre, suf) = post(Some(&p));
            if pre.is_empty() {
                st.remove(kp);
            } else {
                st.set(kp, pre);
            }
            if suf.is_empty() {
                st.remove(ks);
            } else {
                st.set(ks, suf);
            }
        }
    }
    let tol = get("DIMTOL");
    let lim = get("DIMLIM");
    let gap_neg = |st: &Obj| jsv::to_num(st.get("gap")) < 0.0;
    let truthy_num = |v: &Option<V>| {
        let x = jsv::to_num(v.as_ref());
        x != 0.0 && !x.is_nan()
    };
    if tol.is_some() || lim.is_some() || gap_neg(st) {
        if truthy_num(&lim) {
            st.set("tol", "lim");
        } else if truthy_num(&tol) {
            let d = jsv::or(st.get("tp"), 0.0) - jsv::or(st.get("tm"), 0.0);
            st.set("tol", if d.abs() < 1e-12 { "sym" } else { "dev" });
        } else if gap_neg(st) {
            st.set("tol", "basic");
        } else if tol.is_some() || lim.is_some() {
            st.remove("tol");
        }
    }
    if gap_neg(st) {
        let g = -jsv::to_num(st.get("gap"));
        st.set("gap", g);
    }
    if let Some(unit) = get("DIMUNIT")
        && st.get("lunit").is_none()
    {
        let u = js::round(jsv::to_num(Some(&unit)));
        st.set(
            "lunit",
            if u == 6.0 {
                4.0
            } else if u == 7.0 {
                5.0
            } else {
                u
            },
        );
    }
}

/// `stFromCodes(get, st)`: fill st from DXF group codes (`get(code)` → raw value or None).
pub fn st_from_codes(get: &dyn Fn(i64) -> Option<V>, st: &mut Obj) {
    for (key, _, code, kind, _) in DIMVARS {
        if let Some(raw) = get(code)
            && let Some(x) = in1(kind, Some(&raw))
        {
            st.set(key, x);
        }
    }
    specials(&|name: &str| SPECIAL.iter().find(|(_, n)| *n == name).and_then(|(c, _)| get(*c)), st);
}

/// `stFromVars(vars, st)`: fill st from variables by name (cc-dwg DIMSTYLE vars, the DWG header).
pub fn st_from_vars(vars: Ov, st: &mut Obj) {
    let get = |name: &str| jsv::get(vars, name).cloned();
    for (key, name, _, kind, _) in DIMVARS {
        if let Some(raw) = get(name)
            && let Some(x) = in1(kind, Some(&raw))
        {
            st.set(key, x);
        }
    }
    specials(&get, st);
}

/// `dstyleMap(items)`: the ACAD xdata list → (group code → value) of its "DSTYLE" { … } overrides.
pub fn dstyle_map(items: &[V]) -> Vec<(f64, Option<V>)> {
    let mut m: Vec<(f64, Option<V>)> = Vec::new();
    let Some(k) = items.iter().position(|it| {
        let (c, v) = cv(it);
        code_is(c, 1000.0) && js::trim(&jsv::to_str(v)) == "DSTYLE"
    }) else {
        return m;
    };
    let mut j = k.saturating_add(1);
    while j.saturating_add(1) < items.len() {
        let Some(it) = items.get(j) else { break };
        let (c, v) = cv(it);
        if code_is(c, 1002.0) && js::trim(&jsv::to_str(v)) == "}" {
            break;
        }
        if code_is(c, 1002.0) {
            j = j.saturating_add(1);
            continue;
        }
        if code_is(c, 1070.0) {
            let key = js::round(jsv::to_num(v));
            let val = items.get(j.saturating_add(1)).and_then(|n| cv(n).1).cloned();
            if let Some(slot) = m.iter_mut().find(|(k2, _)| *k2 == key || (k2.is_nan() && key.is_nan())) {
                slot.1 = val;
            } else {
                m.push((key, val));
            }
            j = j.saturating_add(1);
        }
        j = j.saturating_add(1);
    }
    m
}

/// `ccDimFrom(strs, st)`: CADCRAFT strings "CCDIMST:<json>" → st keys (untrusted: known keys, values of the right kind).
pub fn cc_dim_from(strs: &[String], st: &mut Obj) {
    let j = joined(strs, "CCDIMST:");
    if j.is_empty() {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&j) else { return };
    let serde_json::Value::Object(o) = parsed else { return };
    let defs = def();
    let extra = dim_extra();
    let dd = dim_defaults();
    for (k, v) in o {
        if !extra.has(&k) && !dd.has(&k) {
            continue;
        }
        let v = jsv::from_json(&v);
        let d = defs.get(&k);
        if k == "blk1" || k == "blk2" {
            if let V::Str(s) = &v
                && cc_cad_geom::dimstyle::ARROWS.contains(&s.as_str())
            {
                st.set(k, v);
            }
            continue;
        }
        if k == "tol" {
            if let V::Str(s) = &v
                && cc_cad_geom::dimstyle::TOL_MODES.contains(&s.as_str())
            {
                st.set(k, v);
            }
            continue;
        }
        if k.starts_with("clr") || k == "tfillclr" {
            let ok = match &v {
                V::Null => true,
                V::Num(n) => *n == 0.0 || (n.fract() == 0.0 && (1.0..=255.0).contains(n)),
                V::Str(s) => is_hex_color(s),
                _ => false,
            };
            if ok {
                st.set(k, v);
            }
            continue;
        }
        match (d, &v) {
            (Some(V::Num(_)), V::Num(n)) if n.is_finite() => st.set(k, v),
            (Some(V::Bool(_)), V::Bool(_)) => st.set(k, v),
            (Some(V::Str(_)), V::Str(s)) => {
                let cut = slice16(s, 0, 255);
                st.set(k, cut);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------------ writing

fn aci(c: Ov) -> f64 {
    match c {
        None | Some(V::Null) => 256.0,
        Some(V::Num(n)) if *n == 0.0 => 0.0,
        Some(V::Num(n)) if n.fract() == 0.0 && n.is_finite() => *n,
        _ => 256.0,
    }
}
fn lw100(v: Ov) -> f64 {
    match jsv::num(v) {
        Some(x) if x == -1.0 || x == -2.0 || x == -3.0 => x,
        Some(x) if x.is_finite() && x >= 0.0 => js::round(x * 100.0),
        _ => -2.0,
    }
}

/// `out1(kind, v)`: one st value → its variable's value.
fn out1(kind: Kind, v: Ov) -> V {
    match kind {
        Real => {
            let x = jsv::to_num(v);
            V::Num(if x.is_finite() { x } else { 0.0 })
        }
        Int => {
            let x = jsv::to_num(v);
            V::Num(js::round(if x != 0.0 && !x.is_nan() { x } else { 0.0 }))
        }
        Bool => V::Num(if jsv::truthy(v) { 1.0 } else { 0.0 }),
        Color => match v {
            Some(V::Str(s)) => V::from(cc_cad::nearest_aci(s)),
            _ => V::Num(aci(v)),
        },
        Lw => V::Num(lw100(v)),
        Sep => V::Num(if jsv::to_str(v) == "," { 44.0 } else { 46.0 }),
    }
}

/// JS `a !== b` for primitives (objects compare by identity: never equal here).
pub fn strict_eq(a: Ov, b: Ov) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(V::Null), Some(V::Null)) => true,
        (Some(V::Bool(x)), Some(V::Bool(y))) => x == y,
        (Some(V::Num(x)), Some(V::Num(y))) => x == y,
        (Some(V::Str(x)), Some(V::Str(y))) => x == y,
        _ => false,
    }
}

/// `x || ''` then string concatenation.
fn or_empty_str(v: Ov) -> String {
    if jsv::truthy(v) { jsv::to_str(v) } else { String::new() }
}
fn post_of(pre: Ov, suf: Ov) -> String {
    if jsv::truthy(pre) || jsv::truthy(suf) { format!("{}<>{}", or_empty_str(pre), or_empty_str(suf)) } else { String::new() }
}

/// `varsOf(st)`: the style's variables by name.
pub fn vars_of(st: Option<&Obj>) -> Obj {
    let mut s = def();
    if let Some(st) = st {
        s.assign(st.clone());
    }
    let mut v = Obj::new();
    for (key, name, _, kind, _) in DIMVARS {
        v.set(name, out1(kind, s.get(key)));
    }
    let tih = if jsv::truthy(s.get("tih")) { 1.0 } else { 0.0 };
    v.set("DIMTIH", tih);
    v.set("DIMTOH", tih);
    v.set("DIMPOST", post_of(s.get("pre"), s.get("suf")));
    v.set("DIMAPOST", post_of(s.get("apre"), s.get("asuf")));
    let tol = jsv::s(s.get("tol"));
    v.set("DIMTOL", if matches!(tol, Some("sym" | "dev")) { 1.0 } else { 0.0 });
    v.set("DIMLIM", if tol == Some("lim") { 1.0 } else { 0.0 });
    if tol == Some("basic") {
        let g = jsv::or(v.get("DIMGAP"), 0.625);
        v.set("DIMGAP", -g.abs());
    }
    let b1 = if jsv::truthy(s.get("blk1")) { s.get("blk1").cloned() } else { Some(V::from("")) };
    let b2 = if jsv::truthy(s.get("blk2")) { s.get("blk2").cloned() } else { Some(V::from("")) };
    v.set("DIMSAH", if strict_eq(b1.as_ref(), b2.as_ref()) { 0.0 } else { 1.0 });
    if !st.is_some_and(|st| jsv::truthy(st.get("lunit"))) {
        v.remove("DIMLUNIT");
    }
    v
}

/// One DSTYLE item: (group code, value).
pub type Item = (i64, V);

/// `dstyleItems(st, { r12 })`: the ACAD "DSTYLE" override items.
pub fn dstyle_items(st: Option<&Obj>, r12: bool) -> Vec<Item> {
    let v = vars_of(st);
    let mut items: Vec<Item> = vec![(1000, V::from("DSTYLE")), (1002, V::from("{"))];
    let put = |items: &mut Vec<Item>, code: i64, kind: &str, value: Ov| {
        items.push((1070, V::from(code)));
        match kind {
            "str" => items.push((1000, V::Str(jsv::to_str(value)))),
            "real" => items.push((1040, value.cloned().unwrap_or(V::Num(f64::NAN)))),
            _ => items.push((1070, V::Num(js::int32(jsv::to_num(value))))),
        }
    };
    for (_, name, code, kind, in_r12) in DIMVARS {
        if r12 && !in_r12 {
            continue;
        }
        let Some(x) = v.get(name) else { continue };
        put(&mut items, code, if kind == Real { "real" } else { "int" }, Some(x));
    }
    put(&mut items, 73, "int", v.get("DIMTIH"));
    put(&mut items, 74, "int", v.get("DIMTOH"));
    put(&mut items, 71, "int", v.get("DIMTOL"));
    put(&mut items, 72, "int", v.get("DIMLIM"));
    put(&mut items, 173, "int", v.get("DIMSAH"));
    if jsv::truthy(v.get("DIMPOST")) {
        put(&mut items, 3, "str", v.get("DIMPOST"));
    }
    if jsv::truthy(v.get("DIMAPOST")) {
        put(&mut items, 4, "str", v.get("DIMAPOST"));
    }
    if r12 && let Some(st) = st {
        let lu = st.get("lunit");
        if jsv::truthy(lu) && !strict_eq(lu, Some(&V::Num(2.0))) {
            let n = if strict_eq(lu, Some(&V::Num(4.0))) {
                6.0
            } else if strict_eq(lu, Some(&V::Num(5.0))) {
                7.0
            } else {
                js::int32(jsv::to_num(lu))
            };
            put(&mut items, 270, "int", Some(&V::Num(n)));
        }
    }
    items.push((1002, V::from("}")));
    items
}

/// `dimstylePairs(st, { r12 })`: the same variables as (group code, value) pairs.
pub fn dimstyle_pairs(st: Option<&Obj>, r12: bool) -> Vec<(f64, V)> {
    let items = dstyle_items(st, r12);
    let mut out = Vec::new();
    let mut j = 2usize;
    while j.saturating_add(1) < items.len() {
        if let (Some((1070, code)), Some((_, val))) = (items.get(j), items.get(j.saturating_add(1))) {
            out.push((jsv::to_num(Some(code)), val.clone()));
        }
        j = j.saturating_add(2);
    }
    out
}

/// `ccDimItems(st, { r12 })`: keys a format cannot hold exactly → CADCRAFT strings "CCDIMST:<json>".
pub fn cc_dim_items(st: Option<&Obj>, r12: bool) -> Vec<String> {
    let empty = Obj::new();
    let s = st.unwrap_or(&empty);
    let mut in_r12: Vec<&str> = DIMVARS.iter().filter(|d| d.4).map(|d| d.0).collect();
    in_r12.extend(["tih", "pre", "suf", "apre", "asuf", "tol"]);
    let mut keep = Obj::new();
    let extra = dim_extra();
    for (k, dv) in extra.iter() {
        let v = s.get(k);
        if v.is_none() || strict_eq(v, Some(dv)) {
            continue;
        }
        let never = k == "blk1"
            || k == "blk2"
            || k == "txsty"
            || (matches!(v, Some(V::Str(_))) && (k.starts_with("clr") || k.starts_with("tfillclr")));
        if never || (r12 && !in_r12.contains(&k)) {
            keep.set(k.to_string(), v.cloned().unwrap_or(V::Null));
        }
    }
    if r12 {
        let dd = dim_defaults();
        for k in ["dec", "adec", "lunit"] {
            if let Some(v) = s.get(k)
                && !strict_eq(Some(v), dd.get(k))
            {
                keep.set(k, v.clone());
            }
        }
    }
    if keep.is_empty() {
        return Vec::new();
    }
    chunks("CCDIMST:", &stringify(&V::Obj(keep)))
}

/// `JSON.stringify(v)` (V8's output: shortest numbers, non-finite → null, the JSON escapes).
pub fn stringify(v: &V) -> String {
    let mut out = String::new();
    stringify_into(v, &mut out);
    out
}
fn stringify_into(v: &V, out: &mut String) {
    match v {
        V::Null => out.push_str("null"),
        V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        V::Num(n) => out.push_str(&if n.is_finite() { js::num_to_string(*n) } else { "null".into() }),
        V::Str(s) => json_str(s, out),
        V::Bytes(b) => json_str(&cc_io_dwg::util::b64(b), out),
        V::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                stringify_into(x, out);
            }
            out.push(']');
        }
        V::Obj(o) => {
            out.push('{');
            let mut first = true;
            for (k, x) in o.iter() {
                if k.starts_with(jsv::HIDDEN) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                json_str(k, out);
                out.push(':');
                stringify_into(x, out);
            }
            out.push('}');
        }
    }
}
fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparency() {
        assert_eq!(tr_from(Some(&V::Num(f64::from(0x0200_0000u32 | 127)))), Some(V::Num(50.0)));
        assert_eq!(tr_from(Some(&V::Num(f64::from(0x0100_0000u32)))), Some(V::from("ByBlock")));
        assert_eq!(tr_from(Some(&V::Num(f64::from(0x0200_00ffu32)))), None);
    }
    #[test]
    fn dstyle_round_trip() {
        let mut st = dim_defaults();
        st.set("tol", "lim");
        st.set("pre", "R");
        let items = dstyle_items(Some(&st), false);
        let m: Vec<V> = items.iter().map(|(c, v)| V::Arr(vec![V::from(*c), v.clone()])).collect();
        let map = dstyle_map(&m);
        let mut back = dim_defaults();
        st_from_codes(&|c| map.iter().find(|(k, _)| *k == jsv::i2f(c)).and_then(|(_, v)| v.clone()), &mut back);
        assert_eq!(back.get("tol"), Some(&V::from("lim")));
        assert_eq!(back.get("pre"), Some(&V::from("R")));
    }
}
