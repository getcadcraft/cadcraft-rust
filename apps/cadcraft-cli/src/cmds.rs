//! Dev: `cadcraft-cli cmds <script.json>` (or `--names`: the ported commands and aliases as JSON) — drive the command line (cc-engine) with a stub host, for the parity
//! harness (`forks/tools/kdiff.mjs`, which runs the frozen JS Engine on the same script).
//!
//! Script: `{ strings: {key: text}, doc: <doc.load input> | null, inputs: [input], time?: true }` (`time`: records are
//! only `{ ms, printed, running }`, for timing commands on big drawings). An input sets the stub host first
//! (`cursor: {x, y}`, `shift: bool`, `select: [ids]` = the selection before the input, `dragPath: [{x, y}…] | null`) and then does one of
//! `text: "line"` · `click: {x, y}` (+ `raw: {x, y}`, `tan: {c: {cx, cy, r}, near: {x, y}}`) ·
//! `window: {minX, minY, maxX, maxY}` (+ `crossing`) · `path: [{x, y}, …]` · `escape: true`.
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
}

fn p_of(v: Option<&Value>) -> Option<P2> {
    let o = v?;
    Some(P2::new(o.get("x")?.as_f64()?, o.get("y")?.as_f64()?))
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
    let stub = Stub { selection: Vec::new(), log: Rc::clone(&log), entered: None, spaces: Rc::clone(&spaces) };
    let mut eng = Engine::new(doc, Box::new(stub), lang);
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
        } else if let Some(pts) = inp.get("path").and_then(Value::as_array) {
            let pts: Vec<P2> = pts.iter().filter_map(|p| p_of(Some(p))).collect();
            ret = Value::Bool(eng.path(&pts));
        } else if inp.get("escape").is_some() {
            ret = Value::Bool(eng.escape());
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
            "snap": eng.doc().snapshot(),
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
    println!("{}", Value::Array(run_script(&script)));
    ExitCode::SUCCESS
}
