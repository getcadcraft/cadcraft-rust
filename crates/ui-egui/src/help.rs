//! The in-app help (0.5 help-panel.js + help-core.js over the build's compiled help): a side panel with the app's own
//! Markdown topics and the basics every app shares (`crates/ui-egui/help`, the 0.5 files unchanged), an index by
//! section, search (help-core.js `search`: every word must appear, title hits first), and topics drawn from their
//! Markdown: headings, paragraphs, lists, tables, code, notes / tips / warnings, the variant blocks (`:::mac`,
//! `:::touch` … shown when they fit this computer, help-core.js `matches`), `{{ui:…}}`, `{{key:…}}` (Mod = Ctrl, or ⌘ on
//! a Mac), `{{cmd:…}}`, `{{pick:mac=…|windows=…}}` and links between topics. F1 opens and closes it, Esc closes it.
//! The panel's own words are the shell's (`shell.help.*`). 0.5 ships no translated help yet, so topics are English
//! (as 0.5 shows them in es-MX).
//!
//! Commands: `help.open {name?: topic slug | "common:<slug>" | "<app>/<slug>"}`, `help.search {name: words}`,
//! `help.back`, `help.close`, `help.toggle`.

use std::sync::OnceLock;

use egui::RichText;

use crate::lang;
use crate::{Params, UiApp};

/// One topic: its front matter, the Markdown body and the plain text search reads.
#[derive(Debug, Clone)]
pub struct Topic {
    /// the app slug, or "common" (the basics)
    pub scope: &'static str,
    pub slug: &'static str,
    pub title: String,
    pub summary: String,
    pub section: String,
    pub order: f64,
    pub keywords: Vec<String>,
    pub headings: Vec<String>,
    pub body: &'static str,
    /// lower-case plain text (search)
    pub text: String,
}

/// The help's sections in 0.5's order.
pub const SECTIONS: [&str; 5] = ["start", "tasks", "switching", "reference", "trouble"];

fn front(md: &'static str) -> (Vec<(&'static str, &'static str)>, &'static str) {
    let Some(rest) = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n")) else { return (Vec::new(), md) };
    let Some(end) = rest.find("\n---") else { return (Vec::new(), md) };
    let head = rest.get(..end).unwrap_or("");
    let body = rest.get(end.saturating_add(4)..).unwrap_or("").trim_start_matches(['\r', '\n']);
    let kv = head.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim(), v.trim())).collect();
    (kv, body)
}

/// Markdown → plain text for search (marks and link targets dropped).
fn plain(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for sp in body.lines().flat_map(|l| spans(l.trim_start_matches(['#', '|', '-', ' ', ':']))) {
        out.push_str(&sp.text);
        out.push(' ');
    }
    out.to_lowercase()
}

fn parse(scope: &'static str, slug: &'static str, md: &'static str) -> Topic {
    let (kv, body) = front(md);
    let get = |k: &str| kv.iter().find(|(a, _)| *a == k).map(|(_, v)| v.to_string()).unwrap_or_default();
    Topic {
        scope,
        slug,
        title: get("title"),
        summary: get("summary"),
        section: get("section"),
        order: get("order").parse().unwrap_or(1000.0),
        keywords: get("keywords").split(',').map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect(),
        headings: body.lines().filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("### "))).map(str::to_string).collect(),
        body,
        text: plain(body),
    }
}

/// Every topic of every app (parsed once).
pub fn topics() -> &'static [Topic] {
    static ALL: OnceLock<Vec<Topic>> = OnceLock::new();
    ALL.get_or_init(|| crate::help_files::FILES.iter().map(|(scope, slug, md)| parse(scope, slug, md)).collect())
}

/// The topics an app's help shows: its own, then the basics.
pub fn topics_for(app: &str) -> Vec<&'static Topic> {
    topics().iter().filter(|t| t.scope == app || t.scope == "common").collect()
}

/// help-core.js `search`: every word must appear (title 10, summary / keywords / headings 4, text 1); an app's own
/// topic ranks half a point over a basic one; ties by title.
pub fn search(app: &str, q: &str) -> Vec<&'static Topic> {
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, &'static Topic)> = Vec::new();
    for t in topics_for(app) {
        let title = t.title.to_lowercase();
        let head = format!("{} {} {}", t.summary, t.keywords.join(" "), t.headings.join(" ")).to_lowercase();
        let mut score = 0.0;
        let mut ok = true;
        for w in &words {
            if title.contains(w.as_str()) {
                score += 10.0;
            } else if head.contains(w.as_str()) {
                score += 4.0;
            } else if t.text.contains(w.as_str()) {
                score += 1.0;
            } else {
                ok = false;
                break;
            }
        }
        if ok {
            scored.push((score + if t.scope == "common" { 0.0 } else { 0.5 }, t));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    scored.into_iter().map(|(_, t)| t).collect()
}

/// What the panel shows.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Index,
    Topic(&'static str, &'static str),
    Search(String),
}

/// The help panel's state.
#[derive(Debug, Clone)]
pub struct Help {
    pub open: bool,
    pub view: View,
    pub history: Vec<View>,
    pub query: String,
}

impl Default for Help {
    fn default() -> Self {
        Help { open: false, view: View::Index, history: Vec::new(), query: String::new() }
    }
}

pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("help.toggle", "Help", "F1", "{} → opens or closes the help panel"),
    ("help.open", "Help: a topic", "", "{name?: \"<slug>\" | \"common:<slug>\" | \"<app>/<slug>\"} → opens the panel (on that topic)"),
    ("help.search", "Help: search", "", "{name: words} → the topics holding every word"),
    ("help.back", "Help: back", "", "{}"),
    ("help.close", "Help: close", "", "{}"),
];

/// A topic by a link target, from the app showing: "slug" (the app's, else the basics'), "common:slug",
/// "app:<app>/<slug>" or "<app>/<slug>".
pub fn resolve(app: &str, target: &str) -> Option<&'static Topic> {
    let target = target.strip_prefix("topic:").unwrap_or(target);
    let target = target.split('#').next().unwrap_or(target);
    if let Some(s) = target.strip_prefix("common:") {
        return topics().iter().find(|t| t.scope == "common" && t.slug == s);
    }
    let target = target.strip_prefix("app:").unwrap_or(target);
    if let Some((a, s)) = target.split_once('/') {
        return topics().iter().find(|t| t.scope == a && t.slug == s);
    }
    topics()
        .iter()
        .find(|t| t.scope == app && t.slug == target)
        .or_else(|| topics().iter().find(|t| t.scope == "common" && t.slug == target))
}

/// The app whose help shows (the start screen: the basics only).
fn help_app(app: &UiApp) -> &'static str {
    app.active_app()
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let scope = help_app(app);
    let h = &mut app.help;
    match id {
        "help.toggle" => h.open = !h.open,
        "help.close" => h.open = false,
        "help.open" => {
            h.open = true;
            if let Some(n) = params.name.as_deref().filter(|n| !n.is_empty()) {
                let t = resolve(scope, n).ok_or_else(|| format!("help.open: no topic {n}"))?;
                let v = View::Topic(t.scope, t.slug);
                if h.view != v {
                    h.history.push(std::mem::replace(&mut h.view, v));
                }
            }
        }
        "help.search" => {
            h.open = true;
            let q: String = params.name.clone().unwrap_or_default().chars().take(200).collect();
            h.query = q.clone();
            h.view = if q.trim().is_empty() { View::Index } else { View::Search(q) };
        }
        "help.back" => {
            if let Some(v) = h.history.pop() {
                h.view = v;
            } else {
                h.view = View::Index;
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// This computer, as help-core.js `detect()` sees it: (os, browser, shell, net, input).
fn ctx_tokens() -> [&'static str; 5] {
    let os = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") || cfg!(target_arch = "wasm32") {
        "windows"
    } else {
        "linux"
    };
    let shell = if cfg!(target_arch = "wasm32") { "web" } else { "desktop" };
    [os, "chromium", shell, "online", "mouse"]
}

fn dim_of(tok: &str) -> Option<usize> {
    Some(match tok {
        "mac" | "windows" | "linux" => 0,
        "chromium" | "firefox" | "safari" => 1,
        "web" | "desktop" => 2,
        "online" | "offline" => 3,
        "mouse" | "touch" => 4,
        _ => return None,
    })
}

/// A `:::a b` variant block fits this computer: every dimension named matches (any of its tokens).
pub fn fits(tokens: &[&str]) -> bool {
    let ctx = ctx_tokens();
    (0..5).all(|d| {
        let named: Vec<&&str> = tokens.iter().filter(|t| dim_of(t) == Some(d)).collect();
        named.is_empty() || named.iter().any(|t| ctx.get(d) == Some(*t))
    })
}

/// One run of inline text.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub kind: SpanKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpanKind {
    Plain,
    Bold,
    Code,
    Ui,
    Key,
    Link(String),
}

fn key_text(k: &str) -> String {
    let mac = cfg!(target_os = "macos");
    k.split('+').map(|p| if p == "Mod" { if mac { "⌘" } else { "Ctrl" } } else { p }).collect::<Vec<_>>().join("+")
}

fn pick_text(spec: &str) -> String {
    let os = ctx_tokens()[0];
    let parts: Vec<(&str, &str)> = spec.split('|').filter_map(|p| p.split_once('=')).collect();
    parts.iter().find(|(k, _)| k.split(',').any(|x| x == os)).or(parts.first()).map(|(_, v)| v.to_string()).unwrap_or_default()
}

/// Inline Markdown of one line → spans.
pub fn spans(line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut rest = line;
    let flush = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span { text: std::mem::take(plain), kind: SpanKind::Plain });
        }
    };
    while let Some(c) = rest.chars().next() {
        let tail = rest;
        if tail.starts_with("{{")
            && let Some(end) = tail.find("}}")
        {
            let inner = tail.get(2..end).unwrap_or("");
            let (kind, val) = inner.split_once(':').unwrap_or(("", inner));
            flush(&mut plain, &mut out);
            out.push(match kind {
                "ui" => Span { text: val.to_string(), kind: SpanKind::Ui },
                "key" => Span { text: key_text(val), kind: SpanKind::Key },
                "cmd" => Span { text: val.to_string(), kind: SpanKind::Code },
                "pick" => Span { text: pick_text(val), kind: SpanKind::Plain },
                _ => Span { text: val.to_string(), kind: SpanKind::Plain },
            });
            rest = tail.get(end.saturating_add(2)..).unwrap_or("");
            continue;
        }
        if tail.starts_with("**")
            && let Some(end) = tail.get(2..).and_then(|r| r.find("**"))
        {
            flush(&mut plain, &mut out);
            out.push(Span { text: tail.get(2..end.saturating_add(2)).unwrap_or("").to_string(), kind: SpanKind::Bold });
            rest = tail.get(end.saturating_add(4)..).unwrap_or("");
            continue;
        }
        if c == '`'
            && let Some(end) = tail.get(1..).and_then(|r| r.find('`'))
        {
            flush(&mut plain, &mut out);
            out.push(Span { text: tail.get(1..end.saturating_add(1)).unwrap_or("").to_string(), kind: SpanKind::Code });
            rest = tail.get(end.saturating_add(2)..).unwrap_or("");
            continue;
        }
        if c == '['
            && let Some(close) = tail.find("](")
            && let Some(end) = tail.get(close..).and_then(|r| r.find(')')).map(|e| e.saturating_add(close))
        {
            flush(&mut plain, &mut out);
            let text = tail.get(1..close).unwrap_or("");
            let target = tail.get(close.saturating_add(2)..end).unwrap_or("");
            out.push(Span { text: spans(text).into_iter().map(|s| s.text).collect(), kind: SpanKind::Link(target.to_string()) });
            rest = tail.get(end.saturating_add(1)..).unwrap_or("");
            continue;
        }
        plain.push(c);
        rest = tail.get(c.len_utf8()..).unwrap_or("");
    }
    flush(&mut plain, &mut out);
    out
}

/// Draw spans as one wrapped run; returns a clicked link's target.
fn draw_spans(ui: &mut egui::Ui, sp: &[Span], t: &crate::theme::Tokens, small: bool) -> Option<String> {
    let mut go = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in sp {
            let base = |x: &str| {
                let r = RichText::new(x);
                if small { r.small() } else { r }
            };
            match &s.kind {
                SpanKind::Plain => {
                    ui.label(base(&s.text).color(t.text));
                }
                SpanKind::Bold | SpanKind::Ui => {
                    ui.label(base(&s.text).strong().color(t.text_strong));
                }
                SpanKind::Code | SpanKind::Key => {
                    ui.label(base(&s.text).code().color(t.text_strong));
                }
                SpanKind::Link(target) => {
                    if target.starts_with("http") || target.starts_with('/') {
                        let url = if target.starts_with('/') { format!("https://getcadcraft.com{target}") } else { target.clone() };
                        ui.hyperlink_to(base(&s.text), url).on_hover_text(lang::shell("shell.help.link.external", &[]));
                    } else if ui.link(base(&s.text)).clicked() {
                        go = Some(target.clone());
                    }
                }
            }
        }
    });
    go
}

/// A topic's Markdown body drawn; returns a clicked topic link.
fn draw_body(ui: &mut egui::Ui, body: &str, t: &crate::theme::Tokens) -> Option<String> {
    let mut go = None;
    let mut para: Vec<String> = Vec::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut code: Option<Vec<String>> = None;
    // the open ::: block: Some(true) = shown (a fitting variant or a note), Some(false) = skipped
    let mut block: Option<(bool, Option<&str>)> = None;
    let flush_para = |ui: &mut egui::Ui, para: &mut Vec<String>, go: &mut Option<String>| {
        if !para.is_empty() {
            let text = para.join(" ");
            para.clear();
            if let Some(g) = draw_spans(ui, &spans(&text), t, false) {
                *go = Some(g);
            }
            ui.add_space(4.0);
        }
    };
    let flush_table = |ui: &mut egui::Ui, table: &mut Vec<Vec<String>>, go: &mut Option<String>, n: usize| {
        if table.is_empty() {
            return;
        }
        let rows = std::mem::take(table);
        egui::Grid::new(("help_table", n)).striped(true).num_columns(rows.first().map_or(1, Vec::len)).show(ui, |ui| {
            for (ri, r) in rows.iter().enumerate() {
                for c in r {
                    let mut sp = spans(c);
                    if ri == 0 {
                        for s in &mut sp {
                            if s.kind == SpanKind::Plain {
                                s.kind = SpanKind::Bold;
                            }
                        }
                    }
                    ui.scope(|ui| {
                        ui.set_max_width(260.0);
                        if let Some(g) = draw_spans(ui, &sp, t, true) {
                            *go = Some(g);
                        }
                    });
                }
                ui.end_row();
            }
        });
        ui.add_space(6.0);
    };
    for (n, raw) in body.lines().enumerate() {
        let line = raw.trim_end();
        if let Some(c) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                let text = std::mem::take(c).join("\n");
                code = None;
                if block.is_none_or(|b| b.0) {
                    ui.label(RichText::new(text).monospace().small().color(t.text));
                }
            } else {
                c.push(line.to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(":::") {
            flush_para(ui, &mut para, &mut go);
            flush_table(ui, &mut table, &mut go, n);
            if rest.trim().is_empty() {
                block = None;
            } else {
                let toks: Vec<&str> = rest.split_whitespace().collect();
                let callout = toks.first().copied().filter(|k| matches!(*k, "note" | "tip" | "warning"));
                block = Some((callout.is_some() || fits(&toks), callout));
                if callout.is_some() {
                    ui.add_space(2.0);
                }
            }
            continue;
        }
        if block.is_some_and(|b| !b.0) {
            continue;
        }
        if line.trim_start().starts_with("```") {
            flush_para(ui, &mut para, &mut go);
            code = Some(Vec::new());
            continue;
        }
        if line.starts_with('|') {
            flush_para(ui, &mut para, &mut go);
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
                continue;
            }
            table.push(cells);
            continue;
        }
        flush_table(ui, &mut table, &mut go, n);
        if line.trim().is_empty() {
            flush_para(ui, &mut para, &mut go);
            continue;
        }
        if let Some(h) = line.strip_prefix("## ").or_else(|| line.strip_prefix("### ")) {
            flush_para(ui, &mut para, &mut go);
            ui.add_space(6.0);
            let size = if line.starts_with("## ") { 15.0 } else { 13.5 };
            ui.label(RichText::new(spans(h).into_iter().map(|s| s.text).collect::<String>()).size(size).strong().color(t.text_strong));
            continue;
        }
        let trimmed = line.trim_start();
        let indent = (line.len() - trimmed.len()) as f32;
        let bullet = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).map(|r| ("•".to_string(), r));
        let numbered = trimmed
            .split_once(". ")
            .filter(|(d, _)| !d.is_empty() && d.len() <= 3 && d.chars().all(|c| c.is_ascii_digit()))
            .map(|(d, r)| (format!("{d}."), r));
        if let Some((mark, rest)) = bullet.or(numbered) {
            flush_para(ui, &mut para, &mut go);
            ui.horizontal_top(|ui| {
                ui.add_space(6.0 + indent * 4.0);
                ui.label(RichText::new(mark).color(t.text_dim));
                ui.vertical(|ui| {
                    if let Some(g) = draw_spans(ui, &spans(rest), t, false) {
                        go = Some(g);
                    }
                });
            });
            continue;
        }
        if let Some(q) = trimmed.strip_prefix("> ") {
            para.push(q.to_string());
            continue;
        }
        para.push(trimmed.to_string());
    }
    flush_para(ui, &mut para, &mut go);
    flush_table(ui, &mut table, &mut go, usize::MAX);
    go
}

/// The panel (right side), and F1 / Esc.
pub fn panel(app: &mut UiApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
        app.run_ui("help.toggle", &Params::default());
    }
    if !app.help.open {
        return;
    }
    let t = app.tokens;
    let scope = help_app(app);
    let s = |k: &str| lang::shell(k, &[]);
    let mut act: Option<(&str, Params)> = None;
    let mut q = app.help.query.clone();
    let view = app.help.view.clone();
    let frame = egui::Frame::new().fill(t.panel).inner_margin(10);
    egui::Panel::right("help_panel").default_size(380.0).size_range(260.0..=640.0).resizable(true).frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(s("shell.help.title")).strong().size(15.0).color(t.text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(s("shell.help.close.title")).clicked() {
                    act = Some(("help.close", Params::default()));
                }
                if ui.small_button("≡").on_hover_text(s("shell.help.all_topics")).clicked() {
                    act = Some(("help.search", Params::default()));
                }
                if ui.small_button("←").on_hover_text(s("shell.help.back")).clicked() {
                    act = Some(("help.back", Params::default()));
                }
            });
        });
        let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(s("shell.help.search")).desired_width(f32::INFINITY));
        if r.changed() {
            act = Some(("help.search", Params { name: Some(q.clone()), ..Default::default() }));
        }
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("help_scroll", format!("{view:?}"))).show(ui, |ui| match &view {
            View::Index => {
                let all = topics_for(scope);
                let own: Vec<&&Topic> = all.iter().filter(|t| t.scope == scope).collect();
                if own.is_empty() && scope != "studio" {
                    ui.label(RichText::new(s("shell.help.none")).color(t.text_dim));
                }
                for sec in SECTIONS {
                    let mut list: Vec<&&Topic> = own.iter().copied().filter(|x| x.section == sec).collect();
                    if list.is_empty() {
                        continue;
                    }
                    list.sort_by(|a, b| a.order.total_cmp(&b.order).then_with(|| a.title.cmp(&b.title)));
                    ui.add_space(4.0);
                    ui.label(RichText::new(s(&format!("shell.help.section.{sec}"))).strong().color(t.text_strong));
                    for x in list {
                        if ui.link(RichText::new(&x.title).color(t.text)).on_hover_text(&x.summary).clicked() {
                            act = Some(("help.open", Params { name: Some(format!("{}/{}", x.scope, x.slug)), ..Default::default() }));
                        }
                    }
                }
                let mut basics: Vec<&&Topic> = all.iter().filter(|x| x.scope == "common").collect();
                basics.sort_by(|a, b| a.order.total_cmp(&b.order).then_with(|| a.title.cmp(&b.title)));
                ui.add_space(6.0);
                ui.label(RichText::new(s("shell.help.basics")).strong().color(t.text_strong));
                for x in basics {
                    if ui.link(RichText::new(&x.title).color(t.text)).on_hover_text(&x.summary).clicked() {
                        act = Some(("help.open", Params { name: Some(format!("common:{}", x.slug)), ..Default::default() }));
                    }
                }
            }
            View::Search(q) => {
                let hits = search(scope, q);
                if hits.is_empty() {
                    ui.label(RichText::new(lang::shell("shell.help.no_hits", &[("q", q.clone())])).color(t.text));
                    ui.label(RichText::new(s("shell.help.no_hits.hint")).small().color(t.text_dim));
                } else {
                    ui.label(
                        RichText::new(lang::shell("shell.help.hits", &[("n", hits.len().to_string()), ("q", q.clone())]))
                            .small()
                            .color(t.text_dim),
                    );
                }
                for x in hits {
                    let name =
                        if x.scope == "common" { format!("{} · {}", x.title, s("shell.help.basics.short")) } else { x.title.clone() };
                    if ui.link(RichText::new(name).color(t.text)).clicked() {
                        let target = if x.scope == "common" { format!("common:{}", x.slug) } else { format!("{}/{}", x.scope, x.slug) };
                        act = Some(("help.open", Params { name: Some(target), ..Default::default() }));
                    }
                    ui.label(RichText::new(&x.summary).small().color(t.text_dim));
                    ui.add_space(3.0);
                }
            }
            View::Topic(sc, sl) => {
                if let Some(x) = topics().iter().find(|x| x.scope == *sc && x.slug == *sl) {
                    ui.label(RichText::new(&x.title).size(17.0).strong().color(t.text_strong));
                    if x.scope == "common" {
                        ui.label(RichText::new(s("shell.help.basics")).small().color(t.text_dim));
                    }
                    if cc_lang::is_es() {
                        ui.label(RichText::new(s("shell.help.untranslated")).small().italics().color(t.text_dim));
                    }
                    ui.label(RichText::new(&x.summary).color(t.text_dim));
                    ui.add_space(6.0);
                    if let Some(g) = draw_body(ui, x.body, &t) {
                        let target = resolve(x.scope, &g).map(|r| format!("{}/{}", r.scope, r.slug)).unwrap_or(g);
                        act = Some(("help.open", Params { name: Some(target), ..Default::default() }));
                    }
                }
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && ctx.memory(|m| m.focused()).is_none() {
        act = Some(("help.close", Params::default()));
    }
    if let Some((id, p)) = act
        && let Err(e) = app.run(id, &p)
    {
        app.error = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_parses_and_every_topic_link_resolves() {
        let all = topics();
        assert!(all.len() > 400, "{}", all.len());
        let mut broken = Vec::new();
        for t in all {
            assert!(!t.title.is_empty(), "{}/{} has no title", t.scope, t.slug);
            assert!(SECTIONS.contains(&t.section.as_str()), "{}/{}: section {}", t.scope, t.slug, t.section);
            for line in t.body.lines() {
                for s in spans(line) {
                    if let SpanKind::Link(target) = s.kind
                        && !target.starts_with("http")
                        && !target.starts_with('/')
                        && resolve(t.scope, &target).is_none()
                    {
                        broken.push(format!("{}/{} → {target}", t.scope, t.slug));
                    }
                }
            }
        }
        assert!(broken.len() < 10, "broken help links: {broken:?}");
    }

    #[test]
    fn search_and_inline_marks() {
        let hits = search("cadcraft", "layer freeze");
        assert!(hits.first().is_some_and(|t| t.slug == "layers"), "{:?}", hits.iter().map(|t| t.slug).collect::<Vec<_>>());
        assert!(search("cadcraft", "").is_empty());
        let sp = spans("Press {{key:Mod+Z}} or click {{ui:Undo}}, see [Layers](topic:layers) and `LA`.");
        assert!(sp.iter().any(|s| s.kind == SpanKind::Ui && s.text == "Undo"));
        assert!(sp.iter().any(|s| s.kind == SpanKind::Link("topic:layers".into()) && s.text == "Layers"));
        assert!(sp.iter().any(|s| s.kind == SpanKind::Code && s.text == "LA"));
        assert!(fits(&["note"]) || !fits(&["note"]));
        assert!(fits(&["online"]));
        assert!(!fits(&["offline"]));
        assert!(!fits(&["touch"]));
        // a broken mark never panics
        let _ = spans("[x](  {{ **a ` {{key:");
    }

    #[test]
    fn help_commands() {
        let mut app = UiApp::default();
        app.run("help.open", &Params { name: Some("common:keys".into()), ..Default::default() }).unwrap();
        assert!(app.help.open);
        assert_eq!(app.help.view, View::Topic("common", "keys"));
        app.run("help.open", &Params { name: Some("cadcraft/layers".into()), ..Default::default() }).unwrap();
        app.run("help.back", &Params::default()).unwrap();
        assert_eq!(app.help.view, View::Topic("common", "keys"));
        assert!(app.run("help.open", &Params { name: Some("no-such-topic".into()), ..Default::default() }).is_err());
        app.run("help.search", &Params { name: Some("snap".into()), ..Default::default() }).unwrap();
        app.run("help.close", &Params::default()).unwrap();
        assert!(!app.help.open);
    }
}
