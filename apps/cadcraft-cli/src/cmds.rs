//! Dev: `cadcraft-cli cmds <script.json>` (or `--names`: the ported commands and aliases as JSON) — drive the command line (cc-engine) with a stub host, for the parity
//! harness (`forks/tools/kdiff.mjs`, which runs the frozen JS Engine on the same script).
//!
//! Script: `{ strings: {key: text}, doc: <doc.load input> | null, inputs: [input], time?: true }` (`time`: records are
//! only `{ ms, printed, running }`, for timing commands on big drawings). An input sets the stub host first
//! (`cursor: {x, y}`, `shift: bool`, `select: [ids]` = the selection before the input, `dragPath: [{x, y}…] | null`) and then does one of
//! `text: "line"` · `click: {x, y}` (+ `raw: {x, y}`, `tan: {c: {cx, cy, r}, near: {x, y}}`) ·
//! `window: {minX, minY, maxX, maxY}` (+ `crossing`) · `path: [{x, y}, …]` · `escape: true` ·
//! `lisp: "line"` (a line typed at the command line that may be LISP: appload.js `lispLine`, else `text`) ·
//! `lispLoad: [{name, text, kind}]` (APPLOAD's files: `_LISPLOAD`). `now` (ms) fixes DATE / CDATE's clock.
//! The stub picks the nearest object within 0.5 (geom distToEntity) and windows with geom inWindow, as
//! bin/test-cadcraft.mjs does; its view is 100 × 50 and every other ZOOM Previous finds a previous view.
//!
//! Output: one record per input `{ snap, printed, prompts, previews, pv, sel, undo, expects, running, events, ret }`:
//! the doc snapshot after it, what was printed and prompted, every preview the engine sent, the rubber band at the
//! cursor afterwards (`engine.preview(cursor)`), the selection, the undo depth, the waiting question's kind, the
//! running command, the host calls (onCommand / zoom*) and what click / path / escape returned.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::{Entity, num};
use cc_cad_geom::geom::Bounds;
use cc_cad_geom::pick::{Circ, dist_to_entity, in_window};
use cc_engine::prompt::Kind;
use cc_engine::{Engine, Host, Lang, Tan};
use cc_geom::P2;
use serde_json::{Value, json};

/// The doc's sheets: each one's space and the ids on it (the stub's viewport_entered looks an id up there).
type Sheets = Rc<RefCell<Vec<(cc_cad::SpaceId, Vec<u64>)>>>;

/// The stub host's state, shared with the script runner (the engine owns the host).
#[derive(Default)]
struct Log {
    cursor: Option<P2>,
    shift: bool,
    printed: Vec<Value>,
    prompts: Vec<Value>,
    previews: Vec<Value>,
    events: Vec<Value>,
    prev_calls: u32,
    /// timing run: previews are not turned into JSON
    timing: bool,
}

struct Stub {
    selection: Vec<u64>,
    log: Rc<RefCell<Log>>,
    /// the viewport MSPACE entered (its id)
    entered: Option<u64>,
    /// the doc's sheets' spaces, refreshed by the runner after each input (viewport_entered looks the id up there)
    spaces: Sheets,
    lwt: bool,
    polar_inc: f64,
    /// the script's `files`: what the pickers hand over, in order (kdiff.mjs's stub pickDrawing / pickPictures)
    drawings: std::collections::VecDeque<Option<cc_engine::PickedDrawing>>,
    pictures: std::collections::VecDeque<Vec<cc_engine::PickedFile>>,
    /// the script's `files.library`: what the library panel hands INSERT L ({ item, slug } or null), in order
    library: std::collections::VecDeque<Option<(Value, Option<String>)>>,
    /// the script's `tab3d: true`: the stub has a 3D tab (`host.open3d`, M4.25)
    tab3d: bool,
    /// M4.32 (`extras: true`): the page's linked views, lasso setting, keys editor, Compare settings and Open in 3D,
    /// as kdiff-cadsheets.mjs's stub has them
    extras: bool,
    lasso: bool,
    linked: bool,
    /// the script's `now`: the clock the JS side fixes (Date.now)
    now: Option<f64>,
}

/// Standard base64 (the script's picture bytes) → bytes; None on anything else.
fn b64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        acc = ((acc << 6) | u32::from(v)) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap_or(0));
        }
    }
    Some(out)
}

/// `files` of a script → the pickers' queues. A drawing is `{ fileName, drawing }` (doc.load input) or `{ fileName,
/// path }` (a DWG / DXF read as the window reads it); `null` = the picker cancelled. Pictures are lists of
/// `{ name, b64 | path }`.
fn files_of(
    script: &Value,
) -> (std::collections::VecDeque<Option<cc_engine::PickedDrawing>>, std::collections::VecDeque<Vec<cc_engine::PickedFile>>) {
    let f = script.get("files");
    let drawings = f
        .and_then(|f| f.get("drawings"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|d| {
                    let file_name = d.get("fileName")?.as_str()?.to_string();
                    let drawing = match (d.get("drawing"), d.get("path").and_then(Value::as_str)) {
                        (Some(v), _) => v.clone(),
                        (None, Some(p)) => cc_io_cad::import_bytes(&file_name, &std::fs::read(p).ok()?).ok()?,
                        _ => return None,
                    };
                    Some(cc_engine::PickedDrawing::from_value(&file_name, &drawing))
                })
                .collect()
        })
        .unwrap_or_default();
    let pictures = f
        .and_then(|f| f.get("pictures"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|list| {
                    list.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| {
                            let name = p.get("name")?.as_str()?.to_string();
                            let bytes = match (p.get("b64").and_then(Value::as_str), p.get("path").and_then(Value::as_str)) {
                                (Some(b), _) => b64(b)?,
                                (None, Some(path)) => std::fs::read(path).ok()?,
                                _ => return None,
                            };
                            Some(cc_engine::PickedFile { name, bytes })
                        })
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default();
    (drawings, pictures)
}

fn pt_v(p: P2) -> Value {
    json!({ "x": num(p.x), "y": num(p.y) })
}

impl Host for Stub {
    fn print(&mut self, s: &str) {
        self.log.borrow_mut().printed.push(Value::String(s.into()));
    }
    fn prompt(&mut self, s: &str) {
        self.log.borrow_mut().prompts.push(Value::String(s.into()));
    }
    fn cursor(&self) -> Option<P2> {
        self.log.borrow().cursor
    }
    fn shift(&self) -> bool {
        self.log.borrow().shift
    }
    fn selection(&self) -> Vec<u64> {
        self.selection.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.selection = ids.to_vec();
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        let mut best = None;
        let mut bd = 0.5;
        for e in doc.entities() {
            let d = dist_to_entity(e, p, Some(&doc.blocks));
            if d < bd {
                bd = d;
                best = e.id;
            }
        }
        best
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        doc.entities().filter(|e| in_window(e, w, crossing, Some(&doc.blocks))).filter_map(|e| e.id).collect()
    }
    fn preview(&mut self, ents: Vec<Entity>) {
        if self.log.borrow().timing {
            return;
        }
        self.log.borrow_mut().previews.push(Value::Array(ents.iter().map(Entity::to_value).collect()));
    }
    fn on_command(&mut self, name: Option<&str>) {
        self.log.borrow_mut().events.push(json!(["cmd", name]));
    }
    fn zoom_extents(&mut self) {
        self.log.borrow_mut().events.push(json!(["zoomExtents"]));
    }
    fn zoom_window(&mut self, a: P2, b: P2, exact: bool) {
        self.log.borrow_mut().events.push(json!(["zoomWindow", pt_v(a), pt_v(b), exact]));
    }
    fn zoom_by(&mut self, f: f64) {
        self.log.borrow_mut().events.push(json!(["zoomBy", num(f)]));
    }
    /// every other call has a previous view
    fn zoom_previous(&mut self) -> bool {
        let mut l = self.log.borrow_mut();
        l.prev_calls = l.prev_calls.saturating_add(1);
        let ok = l.prev_calls % 2 == 1;
        l.events.push(json!(["zoomPrevious", ok]));
        ok
    }
    fn view_rect(&self) -> Option<(f64, f64)> {
        Some((100.0, 50.0))
    }
    fn view_center(&self) -> Option<P2> {
        Some(P2::new(0.0, 0.0))
    }
    fn now_ms(&self) -> Option<f64> {
        if let Some(n) = self.now {
            return Some(n);
        }
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64() * 1000.0)
    }
    // the page's split panes and viewport work, logged as kdiff.mjs's stub logs them
    fn has_panes(&self) -> bool {
        true
    }
    fn set_panes(&mut self, layout: &str) {
        self.log.borrow_mut().events.push(json!(["setPanes", layout]));
    }
    fn has_viewports(&self) -> bool {
        true
    }
    fn viewport_entered(&self) -> Option<(cc_cad::SpaceId, u64)> {
        let id = self.entered?;
        self.spaces.borrow().iter().find(|(_, ids)| ids.contains(&id)).map(|(s, _)| (*s, id))
    }
    fn viewport_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        doc.entities()
            .find(|e| match &e.geo {
                cc_cad::entity::Geo::Viewport(v) => {
                    e.get("on") != Some(&Value::Bool(false)) && (p.x - v.x).abs() <= v.w / 2.0 && (p.y - v.y).abs() <= v.h / 2.0
                }
                _ => false,
            })
            .and_then(|e| e.id)
    }
    fn enter_viewport(&mut self, space: cc_cad::SpaceId, id: u64) {
        let _ = space;
        self.entered = Some(id);
        self.log.borrow_mut().events.push(json!(["enterViewport", id]));
    }
    fn leave_viewport(&mut self) {
        self.entered = None;
        self.log.borrow_mut().events.push(json!(["leaveViewport"]));
    }
    // settings { polarInc: 90, lwt: false }, setToggle, turnView and saveBlob, as kdiff.mjs's stub has them
    fn has_turn_view(&self) -> bool {
        true
    }
    fn turn_view(&mut self, ang: f64) {
        self.log.borrow_mut().events.push(json!(["turnView", num(ang)]));
    }
    fn lwt(&self) -> Option<bool> {
        Some(self.lwt)
    }
    fn set_toggle(&mut self, name: &str, on: bool) {
        if name == "lwt" {
            self.lwt = on;
        }
        self.log.borrow_mut().events.push(json!(["setToggle", name, on]));
    }
    fn polar_inc(&self) -> Option<f64> {
        Some(self.polar_inc)
    }
    fn set_polar_inc(&mut self, v: f64) {
        self.polar_inc = v;
    }
    fn can_save_blob(&self) -> bool {
        true
    }
    fn save_blob(&mut self, name: &str, data: &[u8], _mime: &str) {
        if self.log.borrow().timing {
            return;
        }
        // a PDF (PUBLISH) as Latin-1 text with its creation time blanked, as kdiff.mjs's stub logs it
        // GLB / STL (EXPORT3D, M4.24): bytes as Latin-1 text
        let text = if data.starts_with(b"%PDF") {
            pdf_for_log(data)
        } else if matches!(_mime, "model/gltf-binary" | "model/stl") {
            data.iter().map(|b| char::from(*b)).collect()
        } else {
            String::from_utf8_lossy(data).into_owned()
        };
        self.log.borrow_mut().events.push(json!(["saveBlob", name, text]));
    }
    fn link_views(&self) -> Option<bool> {
        self.extras.then_some(self.linked)
    }
    fn set_link_views(&mut self, on: bool) {
        self.linked = on;
        self.log.borrow_mut().events.push(json!(["linkViews", on]));
    }
    fn lasso(&self) -> Option<bool> {
        self.extras.then_some(self.lasso)
    }
    fn set_lasso(&mut self, on: bool) {
        self.lasso = on;
        self.log.borrow_mut().events.push(json!(["setLasso", on]));
    }
    fn shortcuts(&mut self) -> bool {
        if self.extras {
            self.log.borrow_mut().events.push(json!(["shortcuts"]));
        }
        self.extras
    }
    fn compare_settings(&mut self) -> bool {
        if self.extras {
            self.log.borrow_mut().events.push(json!(["compareSettings"]));
        }
        self.extras
    }
    fn open_in_3d(&mut self) -> Option<Result<String, (String, String)>> {
        if !self.extras {
            return None;
        }
        self.log.borrow_mut().events.push(json!(["openIn3d"]));
        Some(Ok("ModelCraft".into()))
    }
    fn can_pick_files(&self) -> bool {
        true
    }
    fn pick_drawing(&mut self) -> cc_engine::Pick<Option<cc_engine::PickedDrawing>> {
        self.log.borrow_mut().events.push(json!(["pickDrawing"]));
        cc_engine::Pick::Now(self.drawings.pop_front().flatten())
    }
    fn pick_pictures(&mut self, multiple: bool) -> cc_engine::Pick<Vec<cc_engine::PickedFile>> {
        self.log.borrow_mut().events.push(json!(["pickPictures", multiple]));
        cc_engine::Pick::Now(self.pictures.pop_front().unwrap_or_default())
    }
    // the library panel, as kdiff.mjs's stub has it (M4.22)
    fn open_library(&mut self) -> bool {
        self.log.borrow_mut().events.push(json!(["openLibrary"]));
        true
    }
    fn library(&mut self) -> cc_engine::Pick<Option<(Value, Option<String>)>> {
        self.log.borrow_mut().events.push(json!(["library"]));
        cc_engine::Pick::Now(self.library.pop_front().flatten())
    }
    // the 3D tab (kdiff-cad3d.mjs's stub opens a Workspace3D; M4.25)
    fn open3d(&mut self) -> bool {
        if self.tab3d {
            self.log.borrow_mut().events.push(json!(["open3d"]));
        }
        self.tab3d
    }
    fn set_cursor(&mut self, p: P2) {
        self.log.borrow_mut().cursor = Some(p);
    }
}

/// FNV-1a (64-bit) of bytes, as hex: a saved DWG in the records (kdiff-cad3d.mjs hashes the JS bytes the same way).
fn fnv64(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= u64::from(*x);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A 3D-tab pointer event from a script: `{kind, x, y, shift, ctrl, alt, time}`.
fn ev3_of(v: &Value) -> cc_engine::ws3d::Ev3 {
    let n = |k: &str| v.get(k).and_then(Value::as_f64);
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    cc_engine::ws3d::Ev3 {
        // the JS's NaN pixels come as null
        x: v.get("x").map_or(0.0, |x| if x.is_null() { f64::NAN } else { x.as_f64().unwrap_or(0.0) }),
        y: v.get("y").map_or(0.0, |x| if x.is_null() { f64::NAN } else { x.as_f64().unwrap_or(0.0) }),
        shift: b("shift"),
        ctrl: b("ctrl"),
        alt: b("alt"),
        time: n("time"),
    }
}

/// The 3D tab's state for the records (kdiff-cad3d.mjs `wsState`).
fn ws_state(eng: &Engine) -> Option<Value> {
    eng.with_ws3d(|ws, _| {
        let c = &ws.tools.host.cam;
        let h = &ws.tools.host;
        json!({
            "active": ws.active, "style": ws.style_key, "eye": c.eye, "target": c.target, "persp": c.persp, "fov": c.fov,
            "path": h.path, "sel": h.sel.iter().map(|s| json!({"t": s.t, "id": s.id})).collect::<Vec<_>>(),
            "tool": ws.tools.name, "busy": ws.tools.busy(),
            "overlay": h.overlay.as_ref().map_or(Value::Null, cc_m3d_tools::overlay::Overlay::to_json),
            "tip": h.tip, "hover2d": ws.hover2d,
        })
    })
}

/// `Number(x.toPrecision(6))`
fn precision6(x: f64) -> f64 {
    format!("{x:.5e}").parse::<f64>().unwrap_or(x)
}

fn p_of(v: Option<&Value>) -> Option<P2> {
    let o = v?;
    // JSON has no NaN: the harness writes the JS's NaN coordinates as null (a click there still happens)
    let n = |k: &str| o.get(k).and_then(|x| if x.is_null() { Some(f64::NAN) } else { x.as_f64() });
    Some(P2::new(n("x")?, n("y")?))
}

fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Point => "point",
        Kind::Distance => "distance",
        Kind::Angle => "angle",
        Kind::Number => "number",
        Kind::Text => "string",
        Kind::Keyword => "keyword",
        Kind::Select => "select",
        Kind::Entity => "entity",
        Kind::File => "file",
    }
}

/// Run a script → one record per input.
pub fn run_script(script: &Value) -> Vec<Value> {
    let mut doc = Doc::new();
    if let Some(d) = script.get("doc").filter(|d| d.is_object()) {
        doc.load(d);
    }
    run_inputs(doc, script).0
}

/// Run a script's inputs on a drawing → the records and the drawing after them (`save --script`).
pub fn run_inputs(doc: Doc, script: &Value) -> (Vec<Value>, Doc) {
    let lang = Lang::new(script.get("strings").unwrap_or(&Value::Null));
    let log = Rc::new(RefCell::new(Log { cursor: Some(P2::new(0.0, 0.0)), ..Log::default() }));
    let spaces: Sheets = Rc::default();
    let (drawings, pictures) = files_of(script);
    let library = script
        .get("files")
        .and_then(|f| f.get("library"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|p| {
                    p.get("item").filter(|i| !i.is_null()).map(|i| (i.clone(), p.get("slug").and_then(Value::as_str).map(str::to_string)))
                })
                .collect()
        })
        .unwrap_or_default();
    let stub = Stub {
        selection: Vec::new(),
        log: Rc::clone(&log),
        entered: None,
        spaces: Rc::clone(&spaces),
        lwt: false,
        polar_inc: 90.0,
        drawings,
        pictures,
        library,
        tab3d: script.get("tab3d").and_then(Value::as_bool).unwrap_or(false),
        extras: script.get("extras").and_then(Value::as_bool).unwrap_or(false),
        lasso: true,
        linked: false,
        now: script.get("now").and_then(Value::as_f64),
    };
    let mut eng = Engine::new(doc, Box::new(stub), lang);
    if let Some(now) = script.get("now").and_then(Value::as_f64) {
        eng.set_clock(Some((now, cc_clock::Zone::System)));
    }
    let empty = Vec::new();
    let inputs = script.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut out = Vec::with_capacity(inputs.len());
    // `time: true`: each record is only { ms, printed } (timing scripts on big drawings: no snapshots)
    let timing = script.get("time").and_then(Value::as_bool).unwrap_or(false);
    log.borrow_mut().timing = timing;
    let refresh = |eng: &Engine, spaces: &Sheets| {
        let d = eng.doc();
        *spaces.borrow_mut() = d.layouts.iter().map(|l| (l.space, d.space_entities(l.space).filter_map(|e| e.id).collect())).collect();
    };
    for inp in inputs {
        refresh(&eng, &spaces);
        let t0 = std::time::Instant::now();
        if let Some(ids) = inp.get("select").and_then(Value::as_array) {
            eng.host_mut().set_selection(&ids.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
        }
        {
            let mut l = log.borrow_mut();
            if let Some(c) = p_of(inp.get("cursor")) {
                l.cursor = Some(c);
            }
            if let Some(b) = inp.get("shift").and_then(Value::as_bool) {
                l.shift = b;
            }
        }
        match inp.get("dragPath") {
            Some(Value::Array(a)) => eng.set_drag_path(Some(a.iter().filter_map(|p| p_of(Some(p))).collect())),
            Some(Value::Null) => eng.set_drag_path(None),
            _ => {}
        }
        let mut ret = Value::Null;
        if let Some(t) = inp.get("text") {
            eng.text(t.as_str().unwrap_or(""));
        } else if let Some(t) = inp.get("lisp") {
            let line = t.as_str().unwrap_or("");
            if !eng.lisp_line(line) {
                eng.text(line);
            }
        } else if let Some(files) = inp.get("lispLoad").and_then(Value::as_array) {
            let files = files
                .iter()
                .map(|f| cc_engine::lisp::AppFile {
                    name: f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    text: f.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                    kind: f.get("kind").and_then(Value::as_str).unwrap_or("lsp").to_string(),
                })
                .collect();
            eng.lisp_load(files, "APPLOAD");
        } else if let Some(c) = p_of(inp.get("click")) {
            let raw = p_of(inp.get("raw")).unwrap_or(c);
            let tan = inp.get("tan").and_then(|t| {
                let cc = t.get("c")?;
                Some(Tan {
                    c: Circ { cx: cc.get("cx")?.as_f64()?, cy: cc.get("cy")?.as_f64()?, r: cc.get("r")?.as_f64()? },
                    near: p_of(t.get("near"))?,
                })
            });
            ret = Value::Bool(eng.click(c, tan, raw));
        } else if let Some(w) = inp.get("window") {
            let crossing = inp.get("crossing").and_then(Value::as_bool).unwrap_or(false);
            eng.window(Bounds::from_value(w), crossing);
        } else if let Some(pts) = inp.get("lasso").and_then(Value::as_array) {
            // M4.32: a lasso dragged while "Select objects" waits
            let pts: Vec<P2> = pts.iter().filter_map(|p| p_of(Some(p))).collect();
            eng.lasso(&pts, inp.get("mode").and_then(Value::as_str).unwrap_or("window"));
        } else if let Some(pts) = inp.get("path").and_then(Value::as_array) {
            let pts: Vec<P2> = pts.iter().filter_map(|p| p_of(Some(p))).collect();
            ret = Value::Bool(eng.path(&pts));
        } else if inp.get("escape").is_some() {
            ret = Value::Bool(eng.escape());
        } else if inp.get("open3d").is_some() {
            // the 3D tab clicked (cadcraft.js open3d)
            ret = Value::Bool(eng.open3d());
        } else if inp.get("hide3d").is_some() {
            eng.hide3d();
        } else if let Some(w) = inp.get("ws") {
            let e = ev3_of(w);
            match w.get("kind").and_then(Value::as_str).unwrap_or("click") {
                "down" => {
                    eng.ws_down(e);
                }
                "move" => {
                    eng.ws_move(e);
                }
                "up" => eng.ws_up(e),
                _ => eng.ws_click(e),
            }
        } else if let Some(k) = inp.get("wsKey").and_then(Value::as_str) {
            ret = Value::Bool(eng.ws_key(k, false));
        } else if let Some(a) = inp.get("wsOrbit").and_then(Value::as_array) {
            let g = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            eng.with_ws3d(|ws, _| ws.tools.host.cam.orbit(g(0), g(1)));
        } else if let Some(a) = inp.get("wsZoom").and_then(Value::as_array) {
            // the wheel on the 3D view: [factor, x, y]
            let g = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            eng.with_ws3d(|ws, _| ws.tools.host.cam.zoom_at(g(0), Some(g(1)), Some(g(2)), None));
        } else if let Some(u) = inp.get("units").and_then(Value::as_str) {
            // props.js unitsSelect: the Properties panel's drawing units
            let (scaled, ratio) = cc_engine::cmd::solid3d::set_units(eng.shared(), u);
            let lang = eng.lang().clone();
            let name = lang.s(&format!("props.units.{}", if u.is_empty() { "none" } else { u }));
            let status = lang.t(if scaled { "props.units.set.scaled" } else { "props.units.set" }, &[("units", name.clone())]);
            log.borrow_mut().events.push(json!(["status", status]));
            if scaled {
                let m = lang.t("props.units.set.3d", &[("units", name), ("k", cc_cad::js::num_to_string(precision6(ratio)))]);
                eng.host_mut().print(&m);
            }
        } else if let Some(v) = inp.get("save").and_then(Value::as_str) {
            let d = eng.doc().clone();
            let opts = cc_io_cad::dwg_save::SaveOpts {
                version: v.to_string(),
                created_ms: Some(1_760_184_000_000.0),
                guid_sequence: true,
                ..cc_io_cad::dwg_save::SaveOpts::default()
            };
            let ev = match v {
                "dxf2000" => cc_io_cad::dxf2000::write_dxf2000(&d).map(|(t, _)| t).map_err(|e| e.0),
                "dxf12" => cc_io_cad::dxf_write::write_dxf(&d).map(|(t, _)| t).map_err(|e| e.0),
                _ => cc_io_cad::dwg_save::save_dwg(&d, &opts).map(|s| fnv64(&s.bytes)).map_err(|e| e.0),
            };
            let e = match ev {
                Ok(t) => json!(["save", v, t]),
                Err(m) => json!(["save", v, {"error": m}]),
            };
            log.borrow_mut().events.push(e);
        }
        if timing {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let mut l = log.borrow_mut();
            out.push(json!({ "ms": ms, "printed": std::mem::take(&mut l.printed), "running": eng.running() }));
            l.prompts.clear();
            l.previews.clear();
            l.events.clear();
            continue;
        }
        let cursor = eng.host().cursor();
        let pv = cursor.map_or_else(Vec::new, |c| eng.preview(c));
        let mut l = log.borrow_mut();
        let rec = json!({
            "snap": snap_with_m3d(&eng.doc()),
            "printed": std::mem::take(&mut l.printed),
            "prompts": std::mem::take(&mut l.prompts),
            "previews": std::mem::take(&mut l.previews),
            "pv": pv.iter().map(Entity::to_value).collect::<Vec<_>>(),
            "sel": eng.host().selection(),
            "undo": eng.doc().undo_stack.len(),
            "expects": eng.expects().map(|k| kind_name(&k)),
            "running": eng.running(),
            "events": std::mem::take(&mut l.events),
            "ret": ret,
        });
        drop(l);
        let mut rec = rec;
        // M4.25: the 3D tab, once it was opened (and its model is the drawing's model)
        if let (Some(w), Some(o)) = (ws_state(&eng), rec.as_object_mut()) {
            o.insert("ws".into(), w);
            let m = eng
                .with_ws3d(|ws, _| (!cc_io_cad::m3dmodel::is_empty(&ws.tools.host.model)).then(|| ws.tools.host.model.snapshot()))
                .flatten();
            if let Some(snap) = o.get_mut("snap").and_then(Value::as_object_mut) {
                match m {
                    Some(m) => snap.insert("m3d".into(), Value::String(m)),
                    None => snap.remove("m3d"),
                };
            }
        }
        out.push(rec);
    }
    let doc = eng.doc().clone();
    (out, doc)
}

pub fn cmds(rest: &[&str]) -> ExitCode {
    if rest.first() == Some(&"--names") {
        // the ported commands and their aliases, for the harness's list of what may be typed
        let m: serde_json::Map<String, Value> = cc_engine::cmd::COMMANDS.iter().map(|c| (c.name.to_string(), json!(c.aliases))).collect();
        println!("{}", Value::Object(m));
        return ExitCode::SUCCESS;
    }
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli cmds <script.json> | --names");
        return ExitCode::from(2);
    };
    let script: Value =
        match std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                println!(r#"{{"$error":{{"code":"io","message":{}}}}}"#, Value::String(e));
                return ExitCode::from(1);
            }
        };
    // `tzName`: the zone CDATE reads (an IANA name), as the harness runs the JS under TZ=…
    let records = match script.get("tzName").and_then(Value::as_str) {
        Some(tz) => cc_clock::with_zone_name(tz, || run_script(&script)).unwrap_or_else(|_| run_script(&script)),
        None => run_script(&script),
    };
    println!("{}", Value::Array(records));
    ExitCode::SUCCESS
}

/// kdiff.mjs `pdfCanon`: a PDF as Latin-1 text the same on both sides however each deflates: every FlateDecode stream
/// inflated, /Length keys and the xref offsets left out, the creation time blanked.
pub(crate) fn pdf_for_log(s: &[u8]) -> String {
    fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
        h.get(from..)?.windows(n.len()).position(|w| w == n).map(|k| k.saturating_add(from))
    }
    fn rfind(h: &[u8], n: &[u8], before: usize) -> Option<usize> {
        h.get(..before)?.windows(n.len()).rposition(|w| w == n)
    }
    fn digits_at(h: &[u8], at: usize) -> (usize, usize) {
        let d: Vec<u8> = h.get(at..).unwrap_or(&[]).iter().copied().take_while(u8::is_ascii_digit).collect();
        (d.len(), std::str::from_utf8(&d).ok().and_then(|x| x.parse().ok()).unwrap_or(0))
    }
    const ST: &[u8] = b">>\nstream\n";
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    while let Some(at) = find(s, ST, pos) {
        let ds = rfind(s, b"obj\n", at).map_or(0, |i| i.saturating_add(4));
        let mut dict = s.get(ds..at.saturating_add(2)).unwrap_or(&[]).to_vec();
        let len = find(&dict, b"/Length ", 0).map_or(0, |i| digits_at(&dict, i.saturating_add(8)).1);
        let d0 = at.saturating_add(ST.len());
        let end = d0.saturating_add(len).min(s.len());
        let raw = s.get(d0..end).unwrap_or(&[]);
        let body = if find(&dict, b"/FlateDecode", 0).is_some() {
            miniz_oxide::inflate::decompress_to_vec_zlib(raw).unwrap_or_else(|_| b"(bad deflate)".to_vec())
        } else {
            raw.to_vec()
        };
        if let Some(i) = find(&dict, b" /Length ", 0) {
            let (n, _) = digits_at(&dict, i.saturating_add(9));
            dict.drain(i..i.saturating_add(9).saturating_add(n));
        }
        out.extend_from_slice(s.get(pos..ds).unwrap_or(&[]));
        out.extend_from_slice(&dict);
        out.extend_from_slice(b"\nstream\n");
        out.extend_from_slice(&body);
        pos = end;
    }
    out.extend_from_slice(s.get(pos..).unwrap_or(&[]));
    if let Some(x) = rfind(&out, b"\nxref\n", out.len())
        && let Some(tr) = find(&out, b"trailer", x)
    {
        out.drain(x.saturating_add(1)..tr);
    }
    if let Some(i) = find(&out, b"startxref\n", 0) {
        let from = i.saturating_add(10);
        let (n, _) = digits_at(&out, from);
        out.splice(from..from.saturating_add(n), *b"-");
    }
    if let Some(i) = find(&out, b"/CreationDate (D:", 0) {
        let from = i.saturating_add(17);
        if let Some(k) = find(&out, b")", from) {
            out.splice(from..k, *b"-");
        }
    }
    out.iter().map(|&b| char::from(b)).collect()
}

/// The doc snapshot with the 3D model's (kdiff.mjs: `m3d` = `doc.model3d.snapshot()` unless the model is empty; M4.24).
pub(crate) fn snap_with_m3d(d: &cc_cad::Doc) -> serde_json::Value {
    let mut v = d.snapshot();
    if d.m3d.is_some() || !d.faces3d.is_empty() || d.model3d_text.is_some() {
        let name = cc_io_cad::i18n::Catalog::en().t("model3d.faces_file", &[]);
        let mut m = cc_io_cad::m3dmodel::model_of(d, &name);
        if !cc_io_cad::m3dmodel::is_empty(&m)
            && let Some(o) = v.as_object_mut()
        {
            o.insert("m3d".into(), serde_json::Value::String(m.snapshot()));
        }
    }
    v
}
