//! `packs.js` — trade packs (TK.1): a data-only kind of add-on. A pack file is as untrusted as a project file: every
//! field is checked, bad parts are dropped with a warning, anything malformed is refused. Items install with EMPTY
//! rates; sample rates only as a labelled block (region, date, currency), only when asked.

use std::collections::HashSet;

use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::formula;
use crate::i18n::{F, t, tf};
use crate::jv::{self, Obj, n, s};
use crate::library::{LIBRARY_MAX, entry_from_item, entry_meta, merge_entries, pack_id_ok, same_entry, semver_ok};
use crate::model::{TYPES, csi_ok, js_entries, normal_item, vars_of};
use crate::units;

pub const PACK_FORMAT: &str = "takeoffcraft-pack";
pub const PACK_VERSION: f64 = 1.0;
pub const PACK_EXT: &str = ".ccpack.json";
pub const PACK_STATUS: [&str; 2] = ["starter", "reviewed"];

/// `str(v, max)` of packs.js: only strings; control characters and runs of white space → one space; trimmed.
fn pstr(v: Option<&Value>, max: usize) -> String {
    let Some(x) = jv::str(v) else { return String::new() };
    let a: String = x.chars().map(|c| if (c as u32) <= 0x1f || c == '\u{7f}' { ' ' } else { c }).collect();
    cc_m3d::js::slice16(&crate::units::collapse_ws(&a), max)
}
fn pint(v: Option<&Value>, lo: f64, hi: f64) -> Option<f64> {
    jv::int(v).filter(|x| *x >= lo && *x <= hi)
}
fn https_url(v: Option<&Value>) -> Option<String> {
    let x = jv::str(v)?;
    let rest = x.strip_prefix("https://")?;
    let len = jv::len16(rest);
    ((3..=500).contains(&len) && !rest.chars().any(|c| jv::is_space(c) || c == '"' || c == '<' || c == '>')).then(|| x.to_string())
}
fn date_ok(v: Option<&Value>) -> Option<String> {
    let x = jv::str(v)?;
    let b = x.as_bytes();
    (b.len() == 10
        && b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()))
    .then(|| x.to_string())
}
fn key_ok(v: &str) -> bool {
    let b = v.as_bytes();
    !b.is_empty()
        && b.len() <= 80
        && b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
}
fn src_id_ok(v: &str) -> bool {
    key_ok(v) && v.len() <= 40
}
fn currency_ok(v: Option<&Value>) -> Option<String> {
    let x = jv::str(v)?;
    (x.len() == 3 && x.bytes().all(|c| c.is_ascii_uppercase())).then(|| x.to_string())
}

/// 'imperial' (LF, SF, SY), 'metric' (m, m²) or 'any' (EA).
pub fn pack_system_of(it: &Value) -> &'static str {
    match units::unit(&jv::to_string(it.get("unit"))) {
        Some(u) if u.system == "any" => "any",
        Some(u) => u.system,
        None => "imperial",
    }
}

fn read_source(raw: &Value) -> Option<Value> {
    let o = jv::obj(Some(raw))?;
    let id = jv::str(o.get("id")).filter(|x| src_id_ok(x))?;
    let title = pstr(o.get("title"), 300);
    if title.is_empty() {
        return None;
    }
    let mut so = Obj::new();
    so.insert("id".into(), s(id));
    so.insert("title".into(), s(title));
    for k in ["publisher", "edition", "note"] {
        let v = pstr(o.get(k), 300);
        if !v.is_empty() {
            so.insert(k.into(), s(v));
        }
    }
    if let Some(u) = https_url(o.get("url")) {
        so.insert("url".into(), s(u));
    }
    Some(Value::Object(so))
}

/// A read pack and its warnings.
#[derive(Debug, Clone)]
pub struct ReadPack {
    pub pack: Value,
    pub warnings: Vec<String>,
}

/// Validate a pack file's JSON (untrusted).
pub fn read_pack(json: &Value) -> Result<ReadPack, String> {
    let Value::Object(j) = json else { return Err(t("pack.err.format")) };
    if jv::str(j.get("format")) != Some(PACK_FORMAT) {
        return Err(t("pack.err.format"));
    }
    let ver = jv::to_number(j.get("version"));
    if !(ver >= 1.0) {
        return Err(t("pack.err.version"));
    }
    if ver > PACK_VERSION {
        return Err(t("pack.err.newer"));
    }
    let Some(id) = jv::str(j.get("id")).filter(|x| pack_id_ok(x)) else { return Err(t("pack.err.id")) };
    let name = pstr(j.get("name"), 80);
    if name.is_empty() {
        return Err(t("pack.err.name"));
    }
    let Some(version) = jv::str(j.get("packVersion")).filter(|x| semver_ok(x)) else { return Err(t("pack.err.semver")) };
    let mut warnings = Vec::new();
    let license = pstr(j.get("license"), 80);
    let status = jv::str(j.get("status")).filter(|x| PACK_STATUS.contains(x)).unwrap_or("starter");
    let csi: Vec<Value> =
        jv::arr(j.get("csi")).map(|l| l.iter().filter(|c| csi_ok(Some(c))).take(60).cloned().collect()).unwrap_or_default();
    let mut pack = Obj::new();
    pack.insert("format".into(), s(PACK_FORMAT));
    pack.insert("version".into(), n(PACK_VERSION));
    pack.insert("id".into(), s(id));
    pack.insert("name".into(), s(name));
    pack.insert("packVersion".into(), s(version));
    pack.insert("trade".into(), s(pstr(j.get("trade"), 80)));
    pack.insert("description".into(), s(pstr(j.get("description"), 600)));
    pack.insert("author".into(), s(pstr(j.get("author"), 200)));
    pack.insert("license".into(), s(if license.is_empty() { "CC0-1.0".to_string() } else { license }));
    pack.insert("status".into(), s(status));
    pack.insert("csi".into(), Value::Array(csi));
    pack.insert("sources".into(), Value::Array(Vec::new()));
    pack.insert("items".into(), Value::Array(Vec::new()));
    if status == "reviewed" {
        let by = pstr(j.get("reviewedBy"), 200);
        let on = date_ok(j.get("reviewedOn")).unwrap_or_default();
        if !by.is_empty() && !on.is_empty() {
            pack.insert("reviewedBy".into(), s(by));
            pack.insert("reviewedOn".into(), s(on));
        } else {
            pack.insert("status".into(), s("starter"));
            warnings.push(t("pack.warn.reviewed"));
        }
    }
    if let Some(h) = https_url(j.get("homepage")) {
        pack.insert("homepage".into(), s(h));
    }
    let mut src_ids: HashSet<String> = HashSet::new();
    let mut sources = Vec::new();
    for raw in jv::arr(j.get("sources")).map_or(&[][..], Vec::as_slice).iter().take(200) {
        match read_source(raw) {
            Some(sv) if !src_ids.contains(&jv::to_string(sv.get("id"))) => {
                src_ids.insert(jv::to_string(sv.get("id")));
                sources.push(sv);
            }
            _ => warnings.push(t("pack.warn.source")),
        }
    }
    pack.insert("sources".into(), Value::Array(sources));
    let mut keys: Vec<String> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    let (mut bad_items, mut bad_comps) = (0usize, 0usize);
    let mut items: Vec<Value> = Vec::new();
    for raw in jv::arr(j.get("items")).map_or(&[][..], Vec::as_slice).iter().take(2000) {
        let Some(ro) = jv::obj(Some(raw)) else {
            bad_items = bad_items.saturating_add(1);
            continue;
        };
        let Some(key) = jv::str(ro.get("key")).filter(|k| key_ok(k) && !keys.iter().any(|x| x == k)) else {
            bad_items = bad_items.saturating_add(1);
            continue;
        };
        if jv::str(ro.get("type")).is_none_or(|x| !TYPES.contains(&x)) {
            bad_items = bad_items.saturating_add(1);
            continue;
        }
        let mut f = ro.clone();
        f.insert("id".into(), s("x"));
        f.insert("mat".into(), n(0.0));
        f.insert("lab".into(), n(0.0));
        jv::del(&mut f, "xd");
        let comps_in: Vec<Value> = jv::arr(ro.get("components"))
            .map(|l| {
                l.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                        co.insert("id".into(), s(format!("c{}", i.saturating_add(1))));
                        co.insert("mat".into(), n(0.0));
                        co.insert("lab".into(), n(0.0));
                        Value::Object(co)
                    })
                    .collect()
            })
            .unwrap_or_default();
        f.insert("components".into(), Value::Array(comps_in));
        let it = normal_item(&f);
        let ity = jv::to_string(it.get("type"));
        let nk = format!("{ity}\u{0}{}", jv::lower(&jv::to_string(it.get("name"))));
        if !jv::truthy(ro.get("name")) || names.contains(&nk) {
            bad_items = bad_items.saturating_add(1);
            continue;
        }
        let comps: Vec<Value> = jv::arr(it.get("components"))
            .map(|l| {
                l.iter()
                    .filter(|c| {
                        let fm = jv::to_string_or_empty(c.get("formula"));
                        let ok = !fm.is_empty() && formula::parse(&fm, vars_of(&ity)).is_ok();
                        if !ok {
                            bad_comps = bad_comps.saturating_add(1);
                        }
                        ok
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        keys.push(key.to_string());
        names.insert(nk);
        let mut it2 = it.clone();
        it2.insert("components".into(), Value::Array(comps));
        let mut item = Obj::new();
        item.insert("key".into(), s(key));
        jv::assign(&mut item, jv::obj(Some(&entry_from_item(&Value::Object(it2)))).unwrap_or(&Obj::new()));
        item.insert("mat".into(), n(0.0));
        item.insert("lab".into(), n(0.0));
        let mut srcs: Vec<Value> = Vec::new();
        for x in jv::arr(ro.get("sources")).map_or(&[][..], Vec::as_slice) {
            if let Some(xs) = jv::str(Some(x))
                && src_ids.contains(xs)
                && !srcs.contains(x)
            {
                srcs.push(x.clone());
            }
        }
        if !srcs.is_empty() {
            item.insert("sources".into(), Value::Array(srcs));
        }
        let assume: Vec<Value> = jv::arr(ro.get("assume"))
            .map(|l| l.iter().map(|a| pstr(Some(a), 300)).filter(|a| !a.is_empty()).take(20).map(s).collect())
            .unwrap_or_default();
        if !assume.is_empty() {
            item.insert("assume".into(), Value::Array(assume));
        }
        let note = pstr(ro.get("note"), 1000);
        if !note.is_empty() {
            item.insert("note".into(), s(note));
        }
        let group = pstr(ro.get("group"), 80);
        if !group.is_empty() {
            item.insert("group".into(), s(group));
        }
        if let Some(tw) = jv::str(ro.get("twin")).filter(|x| key_ok(x)) {
            item.insert("twin".into(), s(tw));
        }
        items.push(Value::Object(item));
    }
    if bad_items > 0 {
        warnings.push(tf("pack.warn.items", &[("n", F::from(bad_items))]));
    }
    if bad_comps > 0 {
        warnings.push(tf("pack.warn.comps", &[("n", F::from(bad_comps))]));
    }
    for it in &mut items {
        if let Value::Object(o) = it
            && let Some(tw) = jv::str(o.get("twin")).map(str::to_string)
            && !keys.contains(&tw)
        {
            jv::del(o, "twin");
        }
    }
    if items.is_empty() {
        return Err(t("pack.err.empty"));
    }
    let (samples, warning) = read_samples(j.get("samples"), &keys, &items);
    pack.insert("items".into(), Value::Array(items));
    if let Some(sm) = samples {
        pack.insert("samples".into(), sm);
    }
    if let Some(w) = warning {
        warnings.push(w);
    }
    Ok(ReadPack { pack: Value::Object(pack), warnings })
}

fn read_samples(raw: Option<&Value>, keys: &[String], items: &[Value]) -> (Option<Value>, Option<String>) {
    let Some(raw) = raw.filter(|r| !r.is_null()) else { return (None, None) };
    let g = |k: &str| match raw {
        Value::Object(o) => o.get(k),
        _ => None,
    };
    let label = pstr(g("label"), 200);
    let region = pstr(g("region"), 120);
    let date = date_ok(g("date")).unwrap_or_default();
    let currency = currency_ok(g("currency")).unwrap_or_default();
    let rates_raw = g("rates").filter(|r| matches!(r, Value::Object(_) | Value::Array(_)));
    let Some(rates_raw) = rates_raw.filter(|_| !label.is_empty() && !region.is_empty() && !date.is_empty() && !currency.is_empty()) else {
        return (None, Some(t("pack.warn.samples")));
    };
    let mut rates = Obj::new();
    let entries: Vec<(String, Value)> = match rates_raw {
        Value::Object(o) => js_entries(o).into_iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Value::Array(a) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
        _ => Vec::new(),
    };
    for (k, v) in entries {
        if !keys.contains(&k) || !jv::is_objlike(Some(&v)) {
            continue;
        }
        let it = items.iter().find(|x| jv::str(x.get("key")) == Some(k.as_str()));
        let mut r = Obj::new();
        r.insert("mat".into(), n(pint(v.get("mat"), 0.0, 1e13).unwrap_or(0.0)));
        r.insert("lab".into(), n(pint(v.get("lab"), 0.0, 1e13).unwrap_or(0.0)));
        let mut comps = Obj::new();
        let craw: Vec<(String, Value)> = match v.get("components") {
            Some(Value::Object(o)) => js_entries(o).into_iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Some(Value::Array(a)) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
            _ => Vec::new(),
        };
        for (cn, cv) in craw {
            let has = it
                .and_then(|it| jv::arr(it.get("components")))
                .is_some_and(|l| l.iter().any(|c| jv::str(c.get("name")) == Some(cn.as_str())));
            if !has || !jv::is_objlike(Some(&cv)) {
                continue;
            }
            comps.insert(
                cn,
                json!({ "mat": n(pint(cv.get("mat"), 0.0, 1e13).unwrap_or(0.0)), "lab": n(pint(cv.get("lab"), 0.0, 1e13).unwrap_or(0.0)) }),
            );
        }
        if !comps.is_empty() {
            r.insert("components".into(), Value::Object(comps));
        }
        rates.insert(k, Value::Object(r));
    }
    (Some(json!({ "label": label, "region": region, "date": date, "currency": currency, "rates": rates })), None)
}

/// The pack's items as library entries for the chosen unit system.
pub fn pack_entries(pack: &Value, system: &str, samples: bool) -> Vec<Value> {
    let sm = if samples { pack.get("samples").filter(|x| jv::truthy(Some(x))) } else { None };
    let sources = jv::arr(pack.get("sources")).map_or(&[][..], Vec::as_slice);
    let src_text = |it: &Value| -> String {
        jv::arr(it.get("sources"))
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(|id| sources.iter().find(|x| jv::strict_eq(x.get("id"), Some(id))))
            .map(|x| {
                ["title", "edition", "publisher"]
                    .iter()
                    .filter_map(|k| x.get(*k).filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v))))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let mut out = Vec::new();
    for it in jv::arr(pack.get("items")).map_or(&[][..], Vec::as_slice) {
        let sy = pack_system_of(it);
        if !(system == "both" || sy == "any" || sy == system) {
            continue;
        }
        let r = sm.and_then(|sm| sm.get("rates")).and_then(|rt| rt.get(jv::to_string(it.get("key"))));
        let rn = |k: &str| r.and_then(|r| r.get(k)).filter(|v| jv::truthy(Some(v))).cloned().unwrap_or(n(0.0));
        let mut e = Obj::new();
        e.insert("name".into(), it.get("name").cloned().unwrap_or(Value::Null));
        e.insert("type".into(), it.get("type").cloned().unwrap_or(Value::Null));
        e.insert("unit".into(), it.get("unit").cloned().unwrap_or(Value::Null));
        e.insert("heightFt".into(), it.get("heightFt").filter(|h| jv::truthy(Some(h))).cloned().unwrap_or(n(0.0)));
        e.insert("waste".into(), it.get("waste").cloned().unwrap_or(Value::Null));
        e.insert("mat".into(), rn("mat"));
        e.insert("lab".into(), rn("lab"));
        if let Some(c) = it.get("crew").filter(|c| jv::truthy(Some(c))) {
            e.insert("crew".into(), c.clone());
        }
        if let Some(c) = it.get("csi").filter(|c| jv::truthy(Some(c))) {
            e.insert("csi".into(), c.clone());
        }
        let comps: Vec<Value> = jv::arr(it.get("components"))
            .map(|l| {
                l.iter()
                    .map(|c| {
                        let mut co = jv::obj(Some(c)).cloned().unwrap_or_default();
                        let cr = r.and_then(|r| r.get("components")).and_then(|x| x.get(jv::to_string(c.get("name"))));
                        let cv = |k: &str| cr.and_then(|x| x.get(k)).filter(|v| jv::truthy(Some(v))).cloned().unwrap_or(n(0.0));
                        co.insert("mat".into(), cv("mat"));
                        co.insert("lab".into(), cv("lab"));
                        Value::Object(co)
                    })
                    .collect()
            })
            .unwrap_or_default();
        e.insert("components".into(), Value::Array(comps));
        let note_parts: Vec<String> = std::iter::once(it.get("note"))
            .chain(jv::arr(it.get("assume")).map_or(&[][..], Vec::as_slice).iter().map(Some))
            .filter(|v| jv::truthy(*v))
            .map(jv::to_string)
            .collect();
        let meta_in = json!({
            "pack": { "id": pack.get("id"), "version": pack.get("packVersion"), "name": pack.get("name") },
            "src": src_text(it), "note": note_parts.join(" "),
        });
        jv::assign(&mut e, &entry_meta(Some(&meta_in)));
        if let (Some(sm), Some(_)) = (sm, r) {
            e.insert("sample".into(), json!({ "region": sm.get("region"), "date": sm.get("date"), "currency": sm.get("currency") }));
        }
        out.push(Value::Object(e));
    }
    out
}

fn from_pack(e: &Value, id: &str) -> bool {
    jv::str(e.get("pack").and_then(|p| p.get("id"))) == Some(id)
}

/// `installPack(lib, pack, opts)` → (lib, added, updated, kept, full).
pub fn install_pack(lib: &[Value], pack: &Value, system: &str, samples: bool) -> (Vec<Value>, f64, f64, f64, bool) {
    let entries = pack_entries(pack, system, samples);
    let pid = jv::to_string(pack.get("id"));
    let mut fresh = Vec::new();
    let mut kept = 0.0;
    for e in entries {
        if lib.iter().any(|x| same_entry(x, &e) && !from_pack(x, &pid)) {
            kept += 1.0;
        } else {
            fresh.push(e);
        }
    }
    let room = LIBRARY_MAX as f64 - lib.len() as f64;
    let (out, added, updated) = merge_entries(lib, &fresh);
    let full = fresh.len() as f64 - updated > room;
    (out, added, updated, kept, full)
}

/// Every entry that came from pack `id` out of the library.
pub fn remove_pack(lib: &[Value], id: &str) -> (Vec<Value>, f64) {
    let out: Vec<Value> = lib.iter().filter(|e| !from_pack(e, id)).cloned().collect();
    let removed = lib.len() as f64 - out.len() as f64;
    (out, removed)
}

/// The packs the library holds entries of, in the order they first appear.
pub fn installed_packs(lib: &[Value]) -> Vec<Value> {
    let mut by: Vec<(String, Value)> = Vec::new();
    for e in lib {
        let Some(p) = e.get("pack").filter(|p| jv::truthy(Some(p))) else { continue };
        let id = jv::to_string(p.get("id"));
        if let Some((_, v)) = by.iter_mut().find(|(k, _)| *k == id) {
            if let Value::Object(o) = v {
                let c = jv::num(o.get("count")).unwrap_or(0.0) + 1.0;
                o.insert("count".into(), n(c));
            }
        } else {
            by.push((id, json!({ "id": p.get("id"), "name": p.get("name"), "version": p.get("version"), "count": 1 })));
        }
    }
    by.into_iter().map(|(_, v)| v).collect()
}

/// True when neither the entry nor any component has a rate.
pub fn rate_unset(e: &Value) -> bool {
    !jv::truthy(e.get("mat"))
        && !jv::truthy(e.get("lab"))
        && !jv::truthy(e.get("crew"))
        && jv::arr(e.get("components"))
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .all(|c| !jv::truthy(c.get("mat")) && !jv::truthy(c.get("lab")))
}

/// Your own library as a pack (JSON-ready).
pub fn library_pack(entries: &[Value], meta: &Obj) -> Value {
    let id = jv::str(meta.get("id")).filter(|x| pack_id_ok(x)).unwrap_or("my-library");
    let mut items = Vec::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut rates = Obj::new();
    let want_rates = jv::truthy(meta.get("rates"));
    for e in entries.iter().take(2000) {
        let lower = jv::lower(&jv::to_string(e.get("name")));
        let nfkd: String = lower.nfkd().collect();
        let mut dashed = String::new();
        let mut in_run = false;
        for c in nfkd.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                dashed.push(c);
                in_run = false;
            } else if !in_run {
                dashed.push('-');
                in_run = true;
            }
        }
        let trimmed = dashed.trim_matches('-');
        let mut key = cc_m3d::js::slice16(if trimmed.is_empty() { "item" } else { trimmed }, 60);
        let mut k = 2u64;
        while keys.contains(&key) {
            // key.replace(/-\d+$/, '') + '-' + k
            let base = match key.rfind('-') {
                Some(i) if key.get(i.saturating_add(1)..).is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())) => {
                    key.get(..i).unwrap_or("").to_string()
                }
                _ => key.clone(),
            };
            key = format!("{base}-{k}");
            k = k.saturating_add(1);
        }
        keys.insert(key.clone());
        let mut it = Obj::new();
        it.insert("key".into(), s(key.clone()));
        for k2 in ["name", "type", "unit"] {
            it.insert(k2.into(), e.get(k2).cloned().unwrap_or(Value::Null));
        }
        if let Some(h) = e.get("heightFt").filter(|h| jv::truthy(Some(h))) {
            it.insert("heightFt".into(), h.clone());
        }
        it.insert("waste".into(), e.get("waste").cloned().unwrap_or(Value::Null));
        if let Some(c) = e.get("csi").filter(|c| jv::truthy(Some(c))) {
            it.insert("csi".into(), c.clone());
        }
        if let Some(c) = e.get("note").filter(|c| jv::truthy(Some(c))) {
            it.insert("note".into(), c.clone());
        }
        let comps: Vec<Value> = jv::arr(e.get("components"))
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|c| json!({ "name": c.get("name"), "unit": c.get("unit"), "formula": c.get("formula"), "waste": c.get("waste") }))
            .collect();
        it.insert("components".into(), Value::Array(comps));
        items.push(Value::Object(it));
        if want_rates && !rate_unset(e) {
            let mut comps = Obj::new();
            for c in jv::arr(e.get("components")).map_or(&[][..], Vec::as_slice) {
                if jv::truthy(c.get("mat")) || jv::truthy(c.get("lab")) {
                    comps.insert(jv::to_string(c.get("name")), json!({ "mat": c.get("mat"), "lab": c.get("lab") }));
                }
            }
            let mut r = Obj::new();
            r.insert("mat".into(), e.get("mat").cloned().unwrap_or(Value::Null));
            r.insert("lab".into(), e.get("lab").cloned().unwrap_or(Value::Null));
            if !comps.is_empty() {
                r.insert("components".into(), Value::Object(comps));
            }
            rates.insert(key, Value::Object(r));
        }
    }
    let pname = pstr(meta.get("name"), 80);
    let pname = if pname.is_empty() { t("pack.export.name") } else { pname };
    let license = pstr(meta.get("license"), 80);
    let mut pack = Obj::new();
    pack.insert("format".into(), s(PACK_FORMAT));
    pack.insert("version".into(), n(PACK_VERSION));
    pack.insert("id".into(), s(id));
    pack.insert("name".into(), s(pname.clone()));
    pack.insert("packVersion".into(), s(jv::str(meta.get("version")).filter(|v| semver_ok(v)).unwrap_or("1.0.0")));
    pack.insert("description".into(), s(pstr(meta.get("description"), 600)));
    pack.insert("author".into(), s(pstr(meta.get("author"), 200)));
    pack.insert("license".into(), s(if license.is_empty() { "CC0-1.0".to_string() } else { license }));
    pack.insert("status".into(), s("starter"));
    pack.insert("sources".into(), Value::Array(Vec::new()));
    pack.insert("items".into(), Value::Array(items));
    let region = pstr(meta.get("region"), 120);
    let date = date_ok(meta.get("date")).unwrap_or_default();
    if want_rates && !region.is_empty() && !date.is_empty() && !rates.is_empty() {
        let cur = jv::to_string_or_empty(meta.get("currency"));
        let cur_ok =
            cur.len() == 3 && cur.bytes().all(|c| c.is_ascii_uppercase()) && meta.get("currency").is_some_and(|c| jv::truthy(Some(c)));
        pack.insert(
            "samples".into(),
            json!({
                "label": tf("pack.export.samples", &[("name", F::S(pname))]), "region": region, "date": date,
                "currency": if cur_ok { meta.get("currency").cloned().unwrap_or(Value::Null) } else { s("USD") },
                "rates": rates,
            }),
        );
    }
    Value::Object(pack)
}
