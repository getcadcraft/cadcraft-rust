//! The Library panel's network sources (library-view.js `load` / `apiEntry` / `fetchItem` / `getJson`): LibraryCraft
//! (the same-origin API, `/api/v1/library`) and other feeds that speak the same API (https base URLs the person adds).
//! Web only: the desktop build has no HTTP client (owner, 2026-11-03), so there these sources say the library could not
//! be reached. Everything goes through `Services::fetch` (GET, no credentials, no referrer, 32 MB at most) and comes back
//! in `Services::fetched`, taken here by URL.
//!
//! Library answers are untrusted: JSON only (no previews: a card shows the box icon until its item is read, and the
//! detail's plan preview is drawn from the item), every item through `cc_bim::libitem::validate_item`, names as plain
//! text. Not built: the sign-in (0.5's personal token from `/library/connect`), Check for updates' POST.

use std::sync::Arc;

use cc_bim::libitem as li;
use serde_json::Value as J;

use crate::UiApp;
use crate::bimlib::{Card, Detail, Meta};
use crate::bimtools::bt;

/// The LibraryCraft API base (same origin).
pub const API: &str = "/api/v1/library";
/// The most feeds kept (`list.slice(0, 20)`).
const MAX_FEEDS: usize = 20;

/// A request the panel is waiting for.
#[derive(Clone, Debug)]
pub enum Want {
    /// a page of cards (`load()`), numbered so a late answer to an older search is dropped
    List { url: String, seq: u64 },
    /// an item's description (`fetchItem` step 1)
    Meta { url: String, card: Card, version: Option<f64> },
    /// the item itself (`fetchItem` step 2)
    Item { url: String, card: Card, info: J, ver: Option<J> },
}

impl Want {
    fn url(&self) -> &str {
        match self {
            Want::List { url, .. } | Want::Meta { url, .. } | Want::Item { url, .. } => url,
        }
    }
}

/// `encodeURIComponent`
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
/// `URLSearchParams` form encoding of one value (space as '+').
fn form(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.*".contains(&b) {
            out.push(char::from(b));
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `baseOf()`: the feed picked, or LibraryCraft.
fn base_of(app: &UiApp) -> Option<String> {
    if app.bimh.lib.source == "feed" { app.bimh.lib.feed.clone() } else { Some(API.to_string()) }
}

fn ask(app: &mut UiApp, w: Want) -> bool {
    let url = w.url().to_string();
    let Some(f) = app.services.fetch.as_mut() else { return false };
    f(&url);
    app.bimh.lib.want.push(w);
    true
}

/// `load()` for LibraryCraft and feeds: the cards of this page for the search words and category.
pub fn load(app: &mut UiApp) {
    let lib = &mut app.bimh.lib;
    lib.error = None;
    lib.want.retain(|w| !matches!(w, Want::List { .. }));
    let Some(base) = base_of(app) else {
        app.bimh.lib.api_cards.clear();
        app.bimh.lib.loading = false;
        return;
    };
    if app.services.fetch.is_none() {
        // no HTTP client in this build (the desktop): LibraryCraft and feeds are web only
        let lib = &mut app.bimh.lib;
        lib.api_cards.clear();
        lib.loading = false;
        lib.error = Some(bt("lib.err.unreachable", &[]));
        return;
    }
    let lib = &mut app.bimh.lib;
    lib.load_seq = lib.load_seq.wrapping_add(1);
    let url = format!("{base}/items?q={}&category={}&page={}&per=24", form(&lib.query), form(&lib.category), lib.page.max(1));
    let seq = lib.load_seq;
    lib.loading = true;
    ask(app, Want::List { url, seq });
}

/// What `getJson` makes of an answer: Ok(data) or the message to show.
fn get_json(r: &Result<(String, Vec<u8>), String>) -> Result<J, String> {
    let (_, bytes) = r.as_ref().map_err(|e| http_err(e, "lib.err.status"))?;
    let j: J = serde_json::from_slice(bytes).map_err(|_| bt("lib.err.refused", &[]))?;
    if j.get("status").and_then(J::as_str) == Some("error") {
        let m = j.get("message").and_then(J::as_str).filter(|m| !m.is_empty()).map(li::clip500);
        return Err(m.unwrap_or_else(|| bt("lib.err.refused", &[])));
    }
    Ok(match j.get("data") {
        Some(d) if j.is_object() => d.clone(),
        _ => j,
    })
}

/// A failed fetch as the panel says it: 451 withdrawn, another status as `key`, no answer as unreachable.
fn http_err(e: &str, key: &str) -> String {
    match e.strip_prefix("http:") {
        Some(rest) => {
            let status = rest.split_whitespace().next().unwrap_or("").to_string();
            if status == "451" { bt("lib.err.withdrawn_copyright", &[]) } else { bt(key, &[("status", &status)]) }
        }
        None => bt("lib.err.unreachable", &[]),
    }
}

fn js_str(v: Option<&J>) -> String {
    match v {
        Some(J::String(s)) => s.clone(),
        Some(J::Number(n)) => cc_bim::js::num_str(n.as_f64().unwrap_or(f64::NAN)),
        Some(J::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// `apiEntry(x, base)` as a card.
fn api_card(x: &J, base: &str) -> Card {
    let s = |k: &str| li::clip(&js_str(x.get(k)), 300);
    Card {
        key: s("uuid"),
        name: s("name"),
        sub: String::new(),
        maker: s("maker"),
        model: s("model"),
        category: s("category"),
        item: None,
        notes: Vec::new(),
        block: None,
        uuid: s("uuid"),
        license: s("license"),
        version: x.get("version").and_then(J::as_f64),
        withdrawn: x.get("status").and_then(J::as_str) == Some("withdrawn"),
        team: x.get("visibility").and_then(J::as_str) == Some("team"),
        base: base.to_string(),
    }
}

/// `showDetail(entry)` for a library card: its item is asked for (`fetchItem(uuid, version, base)`).
pub fn open_card(app: &mut UiApp, card: Card) {
    let url = format!("{}/items/{}", card.base, enc(&card.uuid));
    let version = card.version;
    app.bimh.lib.detail = Some(Detail { card: card.clone(), item: None, meta: Meta::default(), error: None, loading: true });
    app.bimh.lib.want.retain(|w| matches!(w, Want::List { .. }));
    if !ask(app, Want::Meta { url, card, version })
        && let Some(d) = app.bimh.lib.detail.as_mut()
    {
        d.loading = false;
        d.error = Some(bt("lib.err.unreachable", &[]));
    }
}

/// PHP writes an empty object as []: `normalizeEmpty(item)`.
fn normalize_empty(item: &mut J) {
    let fix = |o: &mut serde_json::Map<String, J>, k: &str| {
        if o.get(k).and_then(J::as_array).is_some_and(Vec::is_empty) {
            o.insert(k.into(), J::Object(serde_json::Map::new()));
        }
    };
    let Some(o) = item.as_object_mut() else { return };
    for k in ["source_attrs", "params", "ifc", "license", "provenance"] {
        fix(o, k);
    }
    if let Some(p) = o.get_mut("params").and_then(J::as_object_mut) {
        fix(p, "elec");
        fix(p, "plumb");
    }
}

/// Answers for the panel (taken out of `fetched` by URL; the rest left for other views).
pub fn sync(app: &mut UiApp) {
    if let Some((text, due)) = app.bimh.lib.search_due.clone()
        && cc_clock::now_ms() >= due
    {
        app.bimh.lib.search_due = None;
        app.run_ui("bim.lib.search", &crate::Params { name: Some(text), ..Default::default() });
    }
    if app.bimh.lib.want.is_empty() {
        return;
    }
    let Some(inbox) = app.services.fetched.clone() else { return };
    let got: Vec<crate::Fetched> = {
        let mut q = inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let wanted: Vec<String> = app.bimh.lib.want.iter().map(|w| w.url().to_string()).collect();
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *q).into_iter().partition(|f| wanted.contains(&f.url));
        *q = rest;
        mine
    };
    for g in got {
        let Some(i) = app.bimh.lib.want.iter().position(|w| w.url() == g.url) else { continue };
        let w = app.bimh.lib.want.remove(i);
        answer(app, w, &g.result);
    }
}

fn answer(app: &mut UiApp, w: Want, r: &Result<(String, Vec<u8>), String>) {
    match w {
        Want::List { seq, url } => {
            let lib = &mut app.bimh.lib;
            if seq != lib.load_seq {
                return;
            }
            lib.loading = false;
            let base = url.split("/items?").next().unwrap_or(API).to_string();
            match get_json(r) {
                Ok(d) => {
                    lib.total = d.get("total").and_then(J::as_f64).unwrap_or(0.0);
                    lib.api_cards = d
                        .get("items")
                        .and_then(J::as_array)
                        .map_or_else(Vec::new, |a| a.iter().take(300).map(|x| api_card(x, &base)).collect());
                }
                Err(e) => {
                    lib.api_cards.clear();
                    lib.error = Some(e);
                }
            }
        }
        Want::Meta { card, version, .. } => {
            let meta = match get_json(r) {
                Ok(m) => m,
                Err(e) => return fail(app, &card, e),
            };
            let it = meta.get("item").filter(|x| x.is_object()).cloned().unwrap_or(meta);
            let versions: Vec<J> = it.get("versions").and_then(J::as_array).cloned().unwrap_or_default();
            let vnum = |x: &J| x.get("version").and_then(J::as_f64);
            let v = match version {
                Some(want) => versions.iter().find(|x| vnum(x) == Some(want)).cloned(),
                None => {
                    let mut ok: Vec<&J> = versions.iter().filter(|x| x.get("status").and_then(J::as_str) != Some("withdrawn")).collect();
                    ok.sort_by(|a, b| cc_bim::js::num_cmp(vnum(b).unwrap_or(f64::NAN) - vnum(a).unwrap_or(f64::NAN), 0.0));
                    ok.first().copied().or(versions.first()).cloned()
                }
            };
            let url = match v.as_ref().and_then(|x| x.get("item_url")).and_then(J::as_str).filter(|u| !u.is_empty()) {
                Some(u) => u.to_string(),
                None => {
                    let ver = version.map(cc_bim::js::num_str).unwrap_or_else(|| js_str(it.get("version")));
                    format!("{}/items/{}/{}/item.json", card.base, card.uuid, ver)
                }
            };
            if !ask(app, Want::Item { url, card: card.clone(), info: it, ver: v }) {
                fail(app, &card, bt("lib.err.unreachable", &[]));
            }
        }
        Want::Item { card, info, ver, .. } => {
            let bytes = match r {
                Ok((_, b)) => b,
                Err(e) => return fail(app, &card, http_err(e, "lib.err.download")),
            };
            let Ok(mut item) = serde_json::from_slice::<J>(bytes) else { return fail(app, &card, bt("lib.err.refused", &[])) };
            normalize_empty(&mut item);
            if let Some(o) = item.as_object_mut() {
                if !o.get("uuid").is_some_and(|u| u.as_str().is_some_and(|s| !s.is_empty())) {
                    let u = info.get("uuid").and_then(J::as_str).filter(|s| !s.is_empty()).map_or(card.uuid.clone(), str::to_string);
                    o.insert("uuid".into(), J::String(u));
                }
                if !o.get("version").is_some_and(|v| v.as_f64().is_some_and(|x| x != 0.0)) {
                    let v = ver
                        .as_ref()
                        .and_then(|x| x.get("version"))
                        .cloned()
                        .or(card.version.map(J::from))
                        .or_else(|| info.get("version").cloned());
                    if let Some(v) = v {
                        o.insert("version".into(), v);
                    }
                }
                let lic = ver.as_ref().and_then(|x| x.get("license")).or(info.get("license")).and_then(J::as_str).filter(|s| !s.is_empty());
                let has = o.get("license").is_some_and(|l| !matches!(l, J::Null | J::Bool(false)) && l.as_str() != Some(""));
                if !has && let Some(l) = lic {
                    o.insert("license".into(), serde_json::json!({ "id": l, "holder": "", "attribution": "", "url": "" }));
                }
            }
            let (ok, errors) = li::validate_item(&item, false);
            if !ok {
                let e = errors.iter().take(2).cloned().collect::<Vec<_>>().join("; ");
                return fail(app, &card, bt("lib.err.format", &[("errors", &e)]));
            }
            let sha =
                item.get("provenance").and_then(|p| p.get("sha256")).and_then(J::as_str).filter(|s| !s.is_empty()).map(str::to_string);
            let library = if card.base == API { "librarycraft".to_string() } else { card.base.clone() };
            let source = if card.base == API { "LibraryCraft".to_string() } else { bt("lib.source.feed", &[]) };
            let withdrawn = ver.as_ref().and_then(|v| v.get("status")).and_then(J::as_str) == Some("withdrawn");
            let meta = Meta {
                library,
                source,
                source_file: String::new(),
                sha256: sha.unwrap_or_else(|| li::item_hash(&item)),
                notes: Vec::new(),
                slug: info.get("slug").and_then(J::as_str).unwrap_or("").to_string(),
                withdrawn,
                withdrawn_reason: ver.as_ref().and_then(|v| v.get("withdrawn_reason")).and_then(J::as_str).unwrap_or("").to_string(),
            };
            if let Some(d) = app.bimh.lib.detail.as_mut()
                && d.card.key == card.key
                && d.loading
            {
                d.loading = false;
                d.item = Some(Arc::new(item));
                d.meta = meta;
            }
        }
    }
}

fn fail(app: &mut UiApp, card: &Card, e: String) {
    if let Some(d) = app.bimh.lib.detail.as_mut()
        && d.card.key == card.key
    {
        d.loading = false;
        d.error = Some(e);
    }
}

/// The Add button of the feeds row: an https address (or localhost) kept first in the list, and picked.
pub fn add_feed(app: &mut UiApp, text: &str) -> Result<(), String> {
    let t = text.trim();
    let (scheme, rest) = t.split_once("://").ok_or_else(|| bt("lib.feed.bad_url", &[]))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostname = host.rsplit_once('@').map_or(host, |(_, h)| h).split(':').next().unwrap_or("");
    if hostname.is_empty() || t.len() > 2000 || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bt("lib.feed.bad_url", &[]));
    }
    if !scheme.eq_ignore_ascii_case("https") && !hostname.eq_ignore_ascii_case("localhost") {
        return Err(bt("lib.feed.https", &[]));
    }
    let url = t.trim_end_matches('/').to_string();
    let lib = &mut app.bimh.lib;
    lib.feeds.retain(|f| *f != url);
    lib.feeds.insert(0, url.clone());
    lib.feeds.truncate(MAX_FEEDS);
    lib.feed = Some(url);
    lib.page = 1;
    lib.feed_input.clear();
    load(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded_as_the_browser_does() {
        assert_eq!(form("hand sink/2&x"), "hand+sink%2F2%26x");
        assert_eq!(enc("a b/é"), "a%20b%2F%C3%A9");
    }

    #[test]
    fn answers_are_read_as_get_json_does() {
        assert_eq!(
            get_json(&Ok((String::new(), br#"{"status":"ok","data":{"total":3}}"#.to_vec()))).ok(),
            Some(serde_json::json!({ "total": 3 }))
        );
        assert!(get_json(&Ok((String::new(), br#"{"status":"error","message":"no"}"#.to_vec()))).is_err());
        assert!(get_json(&Ok((String::new(), b"<html>".to_vec()))).is_err());
        assert!(get_json(&Err("net:".into())).is_err());
    }
}
