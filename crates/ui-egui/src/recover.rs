//! Recovery copies of the drawing being edited — CadCraft 0.5's `dwg-autosave.js` on the shell's `app-autosave.js`
//! ([`cc_recovery::Autosave`]), and what the "Recover your work?" dialog shows (its words: the 0.5 shell catalog,
//! `assets/lang/shell`). Pure of egui: `recoverdlg.rs` draws the dialog, the commands here act.
//!
//! The drawing open for editing is one document ("tab1", "tab2" … as 0.5's tabs). Every edit while it differs from its
//! file asks for a copy; the copy (the drawing as DWG 2018 through the ported writer, with its name, source file, save
//! format and format line) is written 2 s after the edits stop, at least every 30 s. Saved, or undone back to the
//! file: the copy goes. Another drawing opened (or a new one): the old copy goes. At start the copies of windows that
//! stopped (crashed, or quit with edits) are offered; Recover opens one, unsaved, saving like the original did.
//!
//! The store is `Services::recovery` (desktop: a locked folder in the config dir; web: browser storage). Without one
//! the window says once that it cannot keep copies. Commands: `recovery.offer`, `recovery.recover {index}`,
//! `recovery.discard {index}`, `recovery.discard_all`, `recovery.later`, `recovery.sweep`, `recovery.exit`,
//! `recovery.clock {factor}`.

use std::rc::Rc;
use std::sync::OnceLock;

use cc_engine::Lang;
use cc_recovery::{Autosave, Rec, RecoveryOp};
use serde_json::{Map, Value};

use crate::{Params, UiApp, edit};

/// The app's records (0.5 `new Autosave({ app: 'cadcraft' })`).
pub const APP: &str = "cadcraft";
/// The app's name in the dialog's lede (the shell's `data-app-name`).
pub const APP_NAME: &str = "CadCraft";

static SHELL_EN: &str = include_str!("../../../assets/lang/shell/en.json");

/// The shell catalog (English).
pub fn shell() -> &'static Lang {
    static L: OnceLock<Lang> = OnceLock::new();
    L.get_or_init(|| Lang::new(&serde_json::from_str(SHELL_EN).unwrap_or(Value::Null)))
}

/// `T(key, vars)` of the shell.
pub fn st(key: &str, vars: &[(&str, String)]) -> String {
    shell().t(key, vars)
}

/// What a copy keeps about its drawing (dwg-autosave.js snapshot meta).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub source: String,
    /// "dwg" | "dxf2000" | "dxf" (0.5's save formats), "" = not chosen
    pub save_format: String,
    pub dwg_version: String,
    pub format: String,
}

impl Meta {
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), Value::String(if self.name.is_empty() { "Drawing1".into() } else { self.name.clone() }));
        m.insert("source".into(), Value::String(self.source.clone()));
        m.insert("saveFormat".into(), Value::String(self.save_format.clone()));
        m.insert("dwgVersion".into(), Value::String(self.dwg_version.clone()));
        m.insert("format".into(), Value::String(self.format.clone()));
        Value::Object(m)
    }
    pub fn from_json(v: &Value) -> Meta {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(1024).collect::<String>();
        Meta { name: s("name"), source: s("source"), save_format: s("saveFormat"), dwg_version: s("dwgVersion"), format: s("format") }
    }
}

/// dwg-import.js `VERSIONS`
fn version_name(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "AC1009" => "R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => return None,
    })
}

/// cadcraft.js `readDrawing`'s format line: "DWG 2018 (AC1032)", "DXF 2000 (AC1015)", "DXF".
pub fn format_line(file: &str, bytes: &[u8], dxf_version: Option<&str>) -> String {
    if file.to_ascii_lowercase().ends_with(".dxf") {
        let v = dxf_version.unwrap_or("");
        let mut s = "DXF".to_string();
        if let Some(n) = version_name(v) {
            s.push(' ');
            s.push_str(n);
        }
        if !v.is_empty() {
            s.push_str(&format!(" ({v})"));
        }
        return s;
    }
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(bytes)).into_owned();
    match version_name(&tag) {
        Some(n) => format!("DWG {n} ({tag})"),
        None => format!("DWG ({tag})"),
    }
}

/// The drawing being tracked.
#[derive(Clone, Debug)]
struct Tab {
    id: String,
    /// which editor it is (its host state's address)
    key: usize,
    seen: u64,
    pending: bool,
    meta: Meta,
}

/// One copy in the dialog.
#[derive(Clone, Debug)]
pub struct Row {
    pub rec: Rec,
    /// what happened to it ("Opening…", "Could not recover it: …", gone)
    pub msg: Option<String>,
    /// gone (another window took it): the row leaves at this time
    pub gone_at: Option<f64>,
}

/// The "Recover your work?" dialog.
#[derive(Clone, Debug, Default)]
pub struct Dlg {
    pub rows: Vec<Row>,
    /// how many copies it opened with (the lede's count)
    pub n: usize,
    /// dialog's result (0.5 `{recovered, discarded}`)
    pub recovered: usize,
    pub discarded: usize,
}

/// The window's recovery state.
pub struct Recovery {
    pub auto: Option<Autosave>,
    /// this window's session id (None: a new one at start)
    pub session: Option<String>,
    /// the store said it cannot be used, or there is none
    pub off: bool,
    tab: Option<Tab>,
    next_tab: u32,
    /// a fixed clock (tests and the parity harness), else the machine's
    pub clock: Option<f64>,
    /// GUIDs in the DWG copy from the parity harness's sequence instead of random ones
    pub guid_seq: bool,
    pub dlg: Option<Dlg>,
    /// offer the copies at start (the window does; headless runs ask with `recovery.offer`)
    pub auto_offer: bool,
    offered: bool,
    said_off: bool,
    /// tests: the sessions running now (see [`Autosave::alive`])
    pub alive: Option<Rc<dyn Fn() -> Vec<String>>>,
    /// how long the last copy took to make and write (ms)
    pub last_write_ms: f64,
}

impl Default for Recovery {
    fn default() -> Self {
        Recovery {
            auto: None,
            session: None,
            off: false,
            tab: None,
            next_tab: 1,
            clock: None,
            guid_seq: false,
            dlg: None,
            auto_offer: true,
            offered: false,
            said_off: false,
            alive: None,
            last_write_ms: 0.0,
        }
    }
}

impl std::fmt::Debug for Recovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recovery").field("auto", &self.auto).field("off", &self.off).finish()
    }
}

impl Recovery {
    /// The tracked drawing's document id ("tab1" …).
    pub fn tab_id(&self) -> Option<&str> {
        self.tab.as_ref().map(|t| t.id.as_str())
    }
    /// The tracked drawing's meta.
    pub fn meta(&self) -> Option<&Meta> {
        self.tab.as_ref().map(|t| &t.meta)
    }
}

/// The UI commands of this file (id, label, params).
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("recovery.offer", "Recover your work?", "{} → the dialog listing the copies windows that stopped left behind"),
    ("recovery.recover", "Recover", "{index?: row, default 0} → opens that copy, unsaved"),
    ("recovery.discard", "Discard", "{index?: row, default 0} → removes that copy"),
    ("recovery.discard_all", "Discard all", "{} → removes every copy listed and closes the dialog"),
    ("recovery.later", "Not now", "{} → closes the dialog; the copies are offered again next time"),
    ("recovery.sweep", "Keep a recovery copy", "{} → writes the copies that are due, drops the ones no longer needed"),
    ("recovery.exit", "Quit", "{} → writes every pending copy and releases this window's lock (the copies stay)"),
    (
        "recovery.clock",
        "Recovery clock",
        "{factor?: ms since 1970} → a fixed clock for copies and the dialog's times; none = the machine's",
    ),
];

fn now(app: &UiApp) -> f64 {
    app.recovery.clock.unwrap_or_else(cc_clock::now_ms)
}

/// Start the store when it is ready; offer at start (once).
fn ensure(app: &mut UiApp, t: f64) {
    if app.recovery.auto.is_some() || app.recovery.off {
        return;
    }
    let Some(svc) = app.services.recovery.as_mut() else {
        app.recovery.off = true;
        return;
    };
    match svc(RecoveryOp::Ready) {
        Ok(Some(_)) => {}
        Ok(None) => return, // not loaded yet (web)
        Err(e) => {
            app.recovery.off = true;
            app.error = Some(format!("{} ({e})", st("shell.autosave.err.unavailable", &[])));
            return;
        }
    }
    let Some(svc) = app.services.recovery.take() else { return };
    let session = app.recovery.session.clone().unwrap_or_else(cc_recovery::new_session_id);
    let mut a = Autosave::new(APP, &session, svc);
    if let Some(f) = app.recovery.alive.clone() {
        a.alive = Some(Box::new(move || f()));
    }
    let ok = a.start(t);
    app.recovery.session = Some(session);
    app.recovery.auto = Some(a);
    if !ok {
        app.recovery.off = true;
    }
    if app.recovery.auto_offer && !app.recovery.offered {
        app.recovery.offered = true;
        open_dialog(app, t, false);
    }
}

/// Every frame (and after every editor input): start, follow the drawing, write what is due, drop what is not needed.
pub fn frame(app: &mut UiApp) {
    let t = now(app);
    ensure(app, t);
    track(app, t);
    write_due(app, t);
    say_problems(app);
    if let Some(d) = app.recovery.dlg.as_mut() {
        d.rows.retain(|r| r.gone_at.is_none_or(|g| t - g < 1200.0));
        if d.rows.is_empty() {
            app.recovery.dlg = None;
        }
    }
}

fn editor_key(ed: &edit::Editor) -> usize {
    Rc::as_ptr(&ed.hs) as usize
}

/// Follow the editor: a new one is a new document; an edit asks for a copy; saved / closed drops it (the sweep).
fn track(app: &mut UiApp, t: f64) {
    let cur = app.editor.as_ref().map(editor_key);
    // the drawing that was tracked is gone (another opened, or none). Saved: its copy goes (0.5: a closed tab).
    // Unsaved: this window has one drawing, not tabs, so the copy stays and is offered back once the window ends
    // (0.5 keeps the drawing open in its other tab).
    if let Some(tab) = app.recovery.tab.as_ref()
        && Some(tab.key) != cur
    {
        let (id, pending) = (tab.id.clone(), tab.pending);
        if let Some(a) = app.recovery.auto.as_mut() {
            if pending { a.forget(&id) } else { a.close(&id) }
        }
        app.recovery.tab = None;
    }
    let Some(ed) = app.editor.as_ref() else { return };
    let version = ed.version();
    let dirty = ed.eng.doc().dirty();
    let name = ed.name.clone();
    if app.recovery.tab.is_none() {
        let id = format!("tab{}", app.recovery.next_tab);
        app.recovery.next_tab = app.recovery.next_tab.saturating_add(1);
        let mut meta = Meta { name: name.clone(), ..Meta::default() };
        if let Some(src) = app.source.as_ref() {
            meta.source = src.name.clone();
            let v = ed.eng.doc().meta.get("version").and_then(Value::as_str).map(str::to_string);
            meta.format = format_line(&src.name, &src.bytes, v.as_deref());
        }
        app.recovery.tab = Some(Tab { id: id.clone(), key: editor_key(ed), seen: version, pending: false, meta });
        if let Some(a) = app.recovery.auto.as_mut() {
            a.doc(&id, &name);
        }
    }
    let Some(tab) = app.recovery.tab.as_mut() else { return };
    tab.meta.name = name.clone();
    if tab.seen != version {
        tab.seen = version;
        if dirty {
            tab.pending = true;
            if let Some(a) = app.recovery.auto.as_mut() {
                a.rename(&tab.id, &name);
                a.changed(&tab.id, t);
            }
        }
    }
    // saved, or undone back to the file
    if tab.pending && !dirty {
        tab.pending = false;
        if let Some(a) = app.recovery.auto.as_mut() {
            a.clean(&tab.id);
        }
    }
}

/// The copy of the drawing open for editing: DWG 2018 + its meta (None: nothing to keep).
fn snapshot(app: &UiApp, t: f64) -> Option<(Vec<u8>, Value)> {
    let ed = app.editor.as_ref()?;
    if !ed.eng.doc().dirty() {
        return None;
    }
    let opts = cc_io_cad::dwg_save::SaveOpts {
        version: "2018".into(),
        created_ms: Some(t),
        guid_sequence: app.recovery.guid_seq,
        ..Default::default()
    };
    let bytes = cc_io_cad::dwg_save::save_dwg(&ed.eng.doc(), &opts).ok()?.bytes;
    let meta = app.recovery.tab.as_ref().map(|t| t.meta.to_json()).unwrap_or(Value::Null);
    Some((bytes, meta))
}

fn write_due(app: &mut UiApp, t: f64) {
    app.recovery.last_write_ms = 0.0;
    let due = match app.recovery.auto.as_mut() {
        Some(a) => a.due(t),
        None => return,
    };
    for id in due {
        write_one(app, &id, t);
    }
}

fn write_one(app: &mut UiApp, id: &str, t: f64) {
    let t0 = web_time::Instant::now();
    let snap = if app.recovery.tab_id() == Some(id) { snapshot(app, t) } else { None };
    if let Some(a) = app.recovery.auto.as_mut() {
        a.save(id, snap, t);
    }
    app.recovery.last_write_ms += t0.elapsed().as_secs_f64() * 1000.0;
}

/// Problems the store had: said once each (0.5 `warn`).
fn say_problems(app: &mut UiApp) {
    let said: Vec<cc_recovery::Problem> = app.recovery.auto.as_mut().map(|a| std::mem::take(&mut a.said)).unwrap_or_default();
    for p in said {
        app.message = st(p.key(), &[]);
    }
    let damaged = app.recovery.auto.as_mut().map(|a| std::mem::take(&mut a.damaged)).unwrap_or_default();
    if !damaged.is_empty() {
        app.message = format!("{} recovery copies could not be read and were left as they are.", damaged.len());
    }
    if app.recovery.off && !app.recovery.said_off && app.editor.as_ref().is_some_and(|e| e.eng.doc().dirty()) {
        app.recovery.said_off = true;
        app.message = st("shell.autosave.err.unavailable", &[]);
    }
}

/// Saved through the window (Save / Save as): the format and version later saves of a recovered copy keep.
pub fn saved(app: &mut UiApp, format: &str) {
    let Some(tab) = app.recovery.tab.as_mut() else { return };
    match format {
        "dxf2000" => tab.meta.save_format = "dxf2000".into(),
        "dxf12" => tab.meta.save_format = "dxf".into(),
        v => {
            tab.meta.save_format = "dwg".into();
            tab.meta.dwg_version = v.to_string();
        }
    }
}

fn open_dialog(app: &mut UiApp, t: f64, say_none: bool) {
    let list = app.recovery.auto.as_mut().map(|a| a.orphans(t)).unwrap_or_default();
    if list.is_empty() {
        if say_none {
            app.message = "There are no recovery copies to offer.".into();
        }
        return;
    }
    let n = list.len();
    app.recovery.dlg = Some(Dlg { rows: list.into_iter().map(|rec| Row { rec, msg: None, gone_at: None }).collect(), n, ..Dlg::default() });
}

/// The dialog's text, line by line (the parity harness compares it): title, lede, each row's name and "Last kept …",
/// the hint.
pub fn dialog_lines(app: &UiApp) -> Vec<String> {
    let Some(d) = app.recovery.dlg.as_ref() else { return Vec::new() };
    let t = now(app);
    let mut out = vec![st("shell.autosave.title", &[]), lede(d.n)];
    for r in &d.rows {
        out.push(row_name(&r.rec));
        out.push(row_meta(&r.rec, t, app.zone));
    }
    out.push(hint(app));
    out
}

pub fn lede(n: usize) -> String {
    st("shell.autosave.lede", &[("app", APP_NAME.into()), ("n", n.to_string())])
}
pub fn row_name(r: &Rec) -> String {
    if r.name.is_empty() { st("shell.autosave.untitled", &[]) } else { r.name.clone() }
}
pub fn row_meta(r: &Rec, t: f64, zone: cc_clock::Zone) -> String {
    st("shell.autosave.meta", &[("when", when_text(r.saved_at, t, zone)), ("size", size_text(r.size))])
}
pub fn hint(app: &UiApp) -> String {
    let days = app.recovery.auto.as_ref().map_or(30.0, |a| a.opts.keep_days);
    st("shell.autosave.later.hint", &[("days", cc_engine::input::fmt(days))])
}

/// app-autosave.js `whenText` in English: "now", "5 minutes ago", "2 hours ago", else the date and time on `zone`'s
/// wall clock ("Oct 21, 2026, 3:05 PM": `toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })`).
/// Spec gap: English only (0.5 follows the shell's language).
pub fn when_text(t: f64, now: f64, zone: cc_clock::Zone) -> String {
    let js_round = |x: f64| (x + 0.5).floor();
    let diff = js_round((t - now) / 1000.0);
    if !diff.is_finite() {
        return String::new();
    }
    let abs = diff.abs();
    let rel = |n: f64, unit: &str| {
        let k = n.abs();
        let u = format!("{k} {unit}{}", if k == 1.0 { "" } else { "s" });
        if n < 0.0 || (n == 0.0 && n.is_sign_negative()) { format!("{u} ago") } else { format!("in {u}") }
    };
    if abs < 45.0 {
        "now".into()
    } else if abs < 3600.0 {
        rel(js_round(diff / 60.0), "minute")
    } else if abs < 86_400.0 {
        rel(js_round(diff / 3600.0), "hour")
    } else {
        cc_markup::js::date_time_en(zone.local_ms(t))
    }
}

/// Round a non-negative decimal to `frac` fraction digits, half away from zero, on its shortest decimal form (as ICU
/// formats a double), with thousands separators and no trailing zeros.
fn decimal(x: f64, frac: usize) -> String {
    let s = format!("{x}");
    let (int, mut fr) = match s.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (s.clone(), String::new()),
    };
    let mut digits: Vec<u8> = int.bytes().chain(fr.bytes()).map(|b| b.saturating_sub(b'0')).collect();
    let keep = int.len().saturating_add(frac);
    let up = digits.get(keep).is_some_and(|d| *d >= 5);
    digits.truncate(keep);
    while digits.len() < keep {
        digits.push(0);
    }
    if up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i = i.saturating_sub(1);
            if let Some(d) = digits.get_mut(i) {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d = d.saturating_add(1);
                    break;
                }
            }
        }
    }
    let int_len = digits.len().saturating_sub(frac);
    let int_part: String = digits.iter().take(int_len).map(|d| char::from(b'0'.saturating_add(*d))).collect();
    fr = digits.iter().skip(int_len).map(|d| char::from(b'0'.saturating_add(*d))).collect();
    while fr.ends_with('0') {
        fr.pop();
    }
    let int_part = if int_part.is_empty() { "0".to_string() } else { int_part };
    // group by thousands
    let mut g = String::new();
    let n = int_part.len();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            g.push(',');
        }
        g.push(c);
    }
    if fr.is_empty() { g } else { format!("{g}.{fr}") }
}

/// app-autosave.js `sizeText` in English: "512 byte", "12.3 kB", "1.5 MB", "1,235 MB", "2 GB".
pub fn size_text(n: f64) -> String {
    let n = if n.is_finite() && n > 0.0 { n } else { 0.0 };
    let (unit, v) = if n < 1024.0 {
        ("byte", n)
    } else if n < 1_048_576.0 {
        ("kB", n / 1024.0)
    } else if n < 1_073_741_824.0 {
        ("MB", n / 1_048_576.0)
    } else {
        ("GB", n / 1_073_741_824.0)
    };
    let frac = if v < 10.0 && unit != "byte" { 1 } else { 0 };
    format!("{} {unit}", decimal(v, frac))
}

/// Open a recovered copy for editing (dwg-autosave.js `recover` + cadcraft.js `openLocal(file, recovered)`): unsaved,
/// with the copy's name, source file, save format and format line.
fn open_recovered(app: &mut UiApp, rec: &Rec, data: &[u8]) -> Result<(), String> {
    if data.is_empty() {
        return Err("This copy is empty.".into());
    }
    let meta = Meta::from_json(&rec.meta);
    let name: String = (if !meta.name.is_empty() {
        meta.name.clone()
    } else if !rec.name.is_empty() {
        rec.name.clone()
    } else {
        "Drawing1".into()
    })
    .chars()
    .take(255)
    .collect();
    // cadcraft.js dwgModel: a file with no blocks could not be read
    let tag = String::from_utf8_lossy(data.get(..6).unwrap_or(data)).into_owned();
    let model = cc_io_dwg::read_dwg(data, &cc_io_dwg::ReadOpts::default());
    if model.get("blocks").and_then(cc_io_dwg::V::as_arr).is_none_or(Vec::is_empty) {
        let v = if tag.is_empty() { edit::lang_text("app.open.unknown_version", &[]) } else { tag };
        return Err(edit::lang_text("app.open.unreadable", &[("version", v)]));
    }
    let input = cc_io_cad::import_dwg(&model, &tag).map_err(|e| e.0)?;
    drop(model);
    let mut doc = cc_cad::Doc::new();
    doc.load(&input);
    drop(input);
    let ed = edit::Editor::new(doc, &name);
    // recovered work is not saved to a file yet (0.5: doc.savedVersion = -1)
    ed.eng.doc_mut().saved_version = u64::MAX;
    let key = editor_key(&ed);
    let version = ed.version();
    // a new document for the window (its first copy is due as for any edit: 0.5's doc.emit() after the load)
    let t = now(app);
    if let (Some(o), Some(a)) = (app.recovery.tab.take(), app.recovery.auto.as_mut()) {
        a.close(&o.id);
    }
    let id = format!("tab{}", app.recovery.next_tab);
    app.recovery.next_tab = app.recovery.next_tab.saturating_add(1);
    // openLocal reads the copy as "<name>.dwg" (its format line), then takes the copy's meta where it has values
    let m = Meta {
        name: name.clone(),
        source: meta.source.clone(),
        save_format: meta.save_format.clone(),
        dwg_version: meta.dwg_version.clone(),
        format: if meta.format.is_empty() { format_line(&format!("{name}.dwg"), data, None) } else { meta.format.clone() },
    };
    app.recovery.tab = Some(Tab { id: id.clone(), key, seen: version, pending: true, meta: m });
    if let Some(a) = app.recovery.auto.as_mut() {
        a.doc(&id, &name);
        a.changed(&id, t);
    }
    app.civil = None;
    app.part = None;
    app.scene3 = None;
    app.model3 = None;
    app.pdf = None;
    app.source = None;
    app.editor = Some(ed);
    app.doc = None;
    app.sync_editor();
    let n = app.doc.as_ref().map_or(1, |d| d.drawing.spaces.len());
    app.views = vec![None; n];
    app.space = 0;
    app.hidden.clear();
    app.pending_fit = true;
    app.error = None;
    app.save_format = match meta.save_format.as_str() {
        "dxf2000" => "dxf2000".into(),
        "dxf" => "dxf12".into(),
        _ if !meta.dwg_version.is_empty() && cc_io_cad::SAVE_FORMATS.contains(&meta.dwg_version.as_str()) => meta.dwg_version.clone(),
        _ => app.save_format.clone(),
    };
    app.message = format!("Recovered {name}: not saved to a file yet.");
    Ok(())
}

fn row_index(app: &UiApp, params: &Params) -> Result<usize, String> {
    let d = app.recovery.dlg.as_ref().ok_or("recovery: the dialog is not open (recovery.offer)")?;
    let live: Vec<usize> = d.rows.iter().enumerate().filter(|(_, r)| r.gone_at.is_none()).map(|(i, _)| i).collect();
    let i = params.index.unwrap_or(0);
    live.get(i).copied().ok_or_else(|| format!("recovery: there is no row {i} ({} listed)", live.len()))
}

fn close_if_empty(app: &mut UiApp) {
    if app.recovery.dlg.as_ref().is_some_and(|d| d.rows.is_empty()) {
        app.recovery.dlg = None;
    }
}

/// The commands of [`COMMANDS`].
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "recovery.clock" => {
            app.recovery.clock = match params.factor {
                None => None,
                Some(v) if v.is_finite() && v.abs() <= 8.64e15 => Some(v),
                Some(_) => return Err("recovery.clock: factor = ms since 1970".into()),
            };
            return Ok(());
        }
        "recovery.sweep" => {
            frame(app);
            return Ok(());
        }
        _ => {}
    }
    let t = now(app);
    ensure(app, t);
    match id {
        "recovery.offer" => {
            if app.recovery.auto.is_none() {
                return Err(st("shell.autosave.err.unavailable", &[]));
            }
            app.recovery.offered = true;
            open_dialog(app, t, true);
        }
        "recovery.recover" => {
            let i = row_index(app, params)?;
            let Some(rec) = app.recovery.dlg.as_ref().and_then(|d| d.rows.get(i)).map(|r| r.rec.clone()) else { return Ok(()) };
            let full = app.recovery.auto.as_mut().and_then(|a| a.load(&rec.key));
            let Some((rec, data)) = full else {
                if let Some(r) = app.recovery.dlg.as_mut().and_then(|d| d.rows.get_mut(i)) {
                    r.msg = Some(st("shell.autosave.gone", &[]));
                    r.gone_at = Some(t);
                }
                return Ok(());
            };
            match open_recovered(app, &rec, &data) {
                Ok(()) => {
                    if let Some(a) = app.recovery.auto.as_mut() {
                        a.discard(&rec.key);
                    }
                    if let Some(d) = app.recovery.dlg.as_mut() {
                        d.recovered = d.recovered.saturating_add(1);
                        if i < d.rows.len() {
                            d.rows.remove(i);
                        }
                    }
                    close_if_empty(app);
                }
                Err(e) => {
                    if let Some(r) = app.recovery.dlg.as_mut().and_then(|d| d.rows.get_mut(i)) {
                        r.msg = Some(st("shell.autosave.failed", &[("reason", e)]));
                    }
                }
            }
        }
        "recovery.discard" => {
            let i = row_index(app, params)?;
            let key = app.recovery.dlg.as_ref().and_then(|d| d.rows.get(i)).map(|r| r.rec.key.clone()).unwrap_or_default();
            if let Some(a) = app.recovery.auto.as_mut() {
                a.discard(&key);
            }
            if let Some(d) = app.recovery.dlg.as_mut() {
                d.discarded = d.discarded.saturating_add(1);
                if i < d.rows.len() {
                    d.rows.remove(i);
                }
            }
            close_if_empty(app);
        }
        "recovery.discard_all" => {
            let keys: Vec<String> = app
                .recovery
                .dlg
                .as_ref()
                .map(|d| d.rows.iter().filter(|r| r.gone_at.is_none()).map(|r| r.rec.key.clone()).collect())
                .unwrap_or_default();
            if let Some(a) = app.recovery.auto.as_mut() {
                for k in &keys {
                    a.discard(k);
                }
            }
            app.recovery.dlg = None;
        }
        "recovery.later" => app.recovery.dlg = None,
        "recovery.exit" => exit(app),
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// Quitting in order: every pending copy is written now, and this window's lock goes (its copies are offered next
/// time, as 0.5's pagehide flush).
pub fn exit(app: &mut UiApp) {
    let t = now(app);
    track(app, t);
    let dirty = app.recovery.auto.as_ref().map(Autosave::dirty_docs).unwrap_or_default();
    for id in dirty {
        write_one(app, &id, t);
    }
    if let Some(a) = app.recovery.auto.as_mut() {
        a.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn window(st: &Rc<RefCell<cc_recovery::MemState>>, session: &str, t: f64) -> UiApp {
        let mut app = UiApp::default();
        app.services.recovery = Some(cc_recovery::memory_on(Rc::clone(st)));
        app.recovery.session = Some(session.into());
        app.recovery.clock = Some(t);
        app.recovery.guid_seq = true;
        app.recovery.auto_offer = false;
        app
    }

    #[test]
    fn texts() {
        assert_eq!(size_text(512.0), "512 byte");
        assert_eq!(size_text(12.0 * 1024.0 + 340.0), "12 kB");
        assert_eq!(size_text(5.0 * 1024.0 + 340.0), "5.3 kB");
        assert_eq!(size_text(1.05 * 1024.0 * 1024.0), "1.1 MB");
        assert_eq!(size_text(1000.5 * 1048576.0), "1,001 MB");
        assert_eq!(size_text(9.96 * 1024.0), "10 kB");
        let z = cc_clock::Zone::Utc;
        let now = 1_790_884_800_000.0;
        assert_eq!(when_text(now - 30_000.0, now, z), "now");
        assert_eq!(when_text(now - 300_000.0, now, z), "5 minutes ago");
        assert_eq!(when_text(now - 60_000.0, now, z), "1 minute ago");
        assert_eq!(when_text(now - 7_200_000.0, now, z), "2 hours ago");
        assert_eq!(when_text(now - 86_400_000.0, now, z), "Sep 30, 2026, 8:00 PM");
        assert_eq!(when_text(now - 86_400_000.0, now, cc_clock::Zone::Fixed(330)), "Oct 1, 2026, 1:30 AM");
        assert_eq!(lede(1), "CadCraft closed before this was saved. Recover it, or discard it.");
        assert_eq!(format_line("a.dwg", b"AC1032....", None), "DWG 2018 (AC1032)");
        assert_eq!(format_line("a.dxf", b"", Some("AC1015")), "DXF 2000 (AC1015)");
    }

    #[test]
    fn write_crash_offer_recover() {
        let store: Rc<RefCell<cc_recovery::MemState>> = Rc::default();
        let t0 = 1_790_884_800_000.0;
        let mut a = window(&store, "s1", t0);
        a.recovery.alive = Some(Rc::new(|| vec!["s1".to_string()]));
        a.run("file.new", &Params::default()).unwrap();
        frame(&mut a);
        for l in ["LINE", "0,0", "100,0", "100,50", ""] {
            a.run("cmd.line", &Params { name: Some(l.into()), ..Default::default() }).unwrap();
        }
        frame(&mut a);
        assert!(store.borrow().items.is_empty(), "nothing before the delay");
        a.recovery.clock = Some(t0 + 2000.0);
        frame(&mut a);
        let keys: Vec<String> = store.borrow().items.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["d:cadcraft:s1:tab1".to_string(), "i:cadcraft:s1:tab1".to_string()]);
        let copy = store.borrow().items[0].1.clone();
        let edited = {
            let ed = a.editor.as_ref().unwrap();
            let o = cc_io_cad::dwg_save::SaveOpts {
                version: "2018".into(),
                created_ms: Some(t0 + 2000.0),
                guid_sequence: true,
                ..Default::default()
            };
            cc_io_cad::dwg_save::save_dwg(&ed.eng.doc(), &o).unwrap().bytes
        };
        assert_eq!(copy, edited);
        // the window "crashes"; another starts
        drop(a);
        let mut b = window(&store, "s2", t0 + 300_000.0);
        b.run("recovery.offer", &Params::default()).unwrap();
        let lines = dialog_lines(&b);
        assert_eq!(lines[0], "Recover your work?");
        assert_eq!(lines[2], "Drawing1");
        assert!(lines[3].starts_with("Last kept 5 minutes ago, "), "{lines:?}");
        b.run("recovery.recover", &Params::default()).unwrap();
        assert!(b.recovery.dlg.is_none());
        let ed = b.editor.as_ref().unwrap();
        assert!(ed.eng.doc().dirty());
        let o = cc_io_cad::dwg_save::SaveOpts {
            version: "2018".into(),
            created_ms: Some(t0 + 2000.0),
            guid_sequence: true,
            ..Default::default()
        };
        let again = cc_io_cad::dwg_save::save_dwg(&ed.eng.doc(), &o).unwrap().bytes;
        let first = cc_io_cad::dwg_save::save_dwg(
            &{
                let input = cc_io_cad::import_bytes("x.dwg", &copy).unwrap();
                let mut d = cc_cad::Doc::new();
                d.load(&input);
                d
            },
            &o,
        )
        .unwrap()
        .bytes;
        assert_eq!(again, first);
        // the recovered copy left the store; the recovered drawing gets its own copy 2 s later
        assert!(!store.borrow().items.iter().any(|(k, _)| k.contains(":s1:")));
        b.recovery.clock = Some(t0 + 302_000.0);
        frame(&mut b);
        assert!(store.borrow().items.iter().any(|(k, _)| k == "i:cadcraft:s2:tab1"));
    }

    #[test]
    fn saved_or_replaced_drops_the_copy() {
        let store: Rc<RefCell<cc_recovery::MemState>> = Rc::default();
        let t0 = 1_000_000.0;
        let mut a = window(&store, "s1", t0);
        a.services.save = Some(Box::new(|n, _| Ok(Some(n.to_string()))));
        a.run("file.new", &Params::default()).unwrap();
        for l in ["CIRCLE", "0,0", "5"] {
            a.run("cmd.line", &Params { name: Some(l.into()), ..Default::default() }).unwrap();
        }
        a.recovery.clock = Some(t0 + 3000.0);
        frame(&mut a);
        assert_eq!(store.borrow().items.iter().filter(|(k, _)| !k.starts_with("b:")).count(), 2);
        a.run("file.save", &Params { name: Some("dxf2000".into()), ..Default::default() }).unwrap();
        frame(&mut a);
        assert!(store.borrow().items.iter().all(|(k, _)| k.starts_with("b:")), "saved: the copy goes");
        assert_eq!(a.recovery.meta().map(|m| m.save_format.as_str()), Some("dxf2000"));
        // edited again, then a new drawing replaces the one tracked: its copy stays (offered once the window ends)
        for l in ["CIRCLE", "9,0", "5"] {
            a.run("cmd.line", &Params { name: Some(l.into()), ..Default::default() }).unwrap();
        }
        a.recovery.clock = Some(t0 + 6000.0);
        frame(&mut a);
        assert!(store.borrow().items.iter().any(|(k, _)| k == "i:cadcraft:s1:tab1"));
        a.run("file.new", &Params::default()).unwrap();
        frame(&mut a);
        assert!(store.borrow().items.iter().any(|(k, _)| k == "i:cadcraft:s1:tab1"));
        assert_eq!(a.recovery.tab_id(), Some("tab2"));
        // a saved drawing replaced: nothing stays
        a.run("file.save", &Params { name: Some("2018".into()), ..Default::default() }).unwrap();
        a.run("file.new", &Params::default()).unwrap();
        frame(&mut a);
        assert!(!store.borrow().items.iter().any(|(k, _)| k.contains("tab2")));
    }

    #[test]
    fn no_store_is_said_not_silent() {
        let mut a = UiApp::default();
        a.run("file.new", &Params::default()).unwrap();
        for l in ["LINE", "0,0", "1,1", ""] {
            a.run("cmd.line", &Params { name: Some(l.into()), ..Default::default() }).unwrap();
        }
        frame(&mut a);
        assert!(a.recovery.off);
        assert_eq!(a.message, st("shell.autosave.err.unavailable", &[]));
        assert!(a.run("recovery.offer", &Params::default()).is_err());
    }

    #[test]
    fn hostile_copy_does_not_open() {
        let store: Rc<RefCell<cc_recovery::MemState>> = Rc::default();
        let rec = r#"{"key":"cadcraft:x:tab1","app":"cadcraft","session":"x","doc":"tab1","name":"Bad","savedAt":5,"size":3,"meta":{"name":"Bad"}}"#;
        store.borrow_mut().items.push(("i:cadcraft:x:tab1".into(), rec.as_bytes().to_vec()));
        store.borrow_mut().items.push(("d:cadcraft:x:tab1".into(), b"PK\x03\x04 not a drawing".to_vec()));
        let mut b = window(&store, "s2", 10_000.0);
        b.run("recovery.offer", &Params::default()).unwrap();
        b.run("recovery.recover", &Params::default()).unwrap();
        let d = b.recovery.dlg.as_ref().unwrap_or_else(|| panic!("{:?} {}", b.error, b.message));
        assert!(d.rows[0].msg.as_deref().unwrap_or("").starts_with("Could not recover it: "), "{:?}", d.rows[0].msg);
        assert!(b.editor.is_none());
        b.run("recovery.discard", &Params::default()).unwrap();
        assert!(b.recovery.dlg.is_none());
        assert!(store.borrow().items.iter().all(|(k, _)| !k.contains(":x:")));
    }
}
