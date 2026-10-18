//! The window side of the control channel: what Studio's hub (host.js `createControlRelay`) and an app window's
//! `host.control` (host.js `makeControl`) do together in CadCraft 0.5, for the fork's one window.
//!
//! - Studio's own tool `windows_list` and its routing: a tool acts on the window named by `arguments.window` (from
//!   `windows_list`), else the one used last; `tools/list` = `windows_list` + the window's tools, each with an optional
//!   `window` argument added; `initialize`, `ping`, `logging/setLevel`, `control.hello` and notifications are Studio's.
//! - The window: assistants off → -32001; a session the person disconnected → -32002; an edit / save / export call while
//!   the person is in the middle of a command → refused with the busy message; every call logged (last 500).
//!
//! The fork has one window, `main`; which app it is follows what is open in it (CadCraft's drawing, a ModelCraft model,
//! a plan set for MarkupCraft or TakeoffCraft …). Its tools come from a [`Window`] implementation the desktop app gives
//! (apps/cadcraft/src/control.rs over `UiApp`).

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::rpc::{CallCtx, Dispatcher, RpcError, Session, ToolDef, ToolError, ToolOut, Tools};

/// The window's id in `windows_list` (the fork has one window).
pub const WINDOW_ID: &str = "main";
const LOG_MAX: usize = 500;

/// One app window's tools and state.
pub trait Window: Tools {
    /// The app slug shown now (cadcraft, modelcraft, markupcraft, takeoffcraft, bimcraft, civilcraft, detailcraft,
    /// partcraft) or "studio" when no app document is open.
    fn app(&self) -> &'static str;
    /// The app's title (CadCraft, ModelCraft, …).
    fn title(&self) -> String;
    /// What the person is doing that an edit must not interrupt (a command waiting for input), else None.
    fn busy(&self) -> Option<String> {
        None
    }
}

/// The app's product name for a slug.
pub fn app_title(slug: &str) -> &'static str {
    match slug {
        "cadcraft" => "CadCraft",
        "bimcraft" => "BimCraft",
        "modelcraft" => "ModelCraft",
        "markupcraft" => "MarkupCraft",
        "takeoffcraft" => "TakeoffCraft",
        "civilcraft" => "CivilCraft",
        "detailcraft" => "DetailCraft",
        "partcraft" => "PartCraft",
        _ => "CadCraft Studio",
    }
}

#[derive(Debug, Clone)]
struct WinSession {
    ended: bool,
    rpc: Session,
}

/// Studio's relay + the window's control, for one window.
pub struct Control {
    /// Studio's dispatcher (windows_list; the MCP handshake)
    pub studio: Dispatcher,
    /// the window's dispatcher (its tools; app.info)
    pub window: Dispatcher,
    /// Settings › Assistants: on when the app was started with the control channel
    pub enabled: bool,
    /// the permission groups the person allowed
    pub groups: Vec<String>,
    sessions: HashMap<String, WinSession>,
    /// the activity log (AGENTS-SPEC § 4), newest last
    pub log: std::rc::Rc<std::cell::RefCell<Vec<Value>>>,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control").field("enabled", &self.enabled).field("groups", &self.groups).finish()
    }
}

/// Studio's own tool list (the relay's dispatcher allows read only).
struct StudioTools {
    windows: Value,
}

impl Tools for StudioTools {
    fn tools(&self) -> Vec<ToolDef> {
        vec![ToolDef::new(
            "windows_list",
            "read",
            "List app windows",
            "The app windows open in Studio, which of them take assistants, and what each allows.",
            None,
        )]
    }
    fn run(&mut self, _name: &str, _args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        Ok(ToolOut::Json(json!({"windows": self.windows})))
    }
}

/// A window's tools seen through the window's session rules.
struct Gate<'a> {
    w: &'a mut dyn Window,
}

impl Tools for Gate<'_> {
    fn tools(&self) -> Vec<ToolDef> {
        self.w.tools()
    }
    fn run(&mut self, name: &str, args: Value, ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        self.w.run(name, args, ctx)
    }
    fn info(&mut self) -> Option<Value> {
        self.w.info()
    }
    fn method(&mut self, name: &str, params: &Value, session: &mut Session) -> Option<Result<Value, RpcError>> {
        self.w.method(name, params, session)
    }
}

impl Control {
    /// `version`: what `serverInfo.version` says (the fork's version).
    pub fn new(groups: Vec<String>, version: &str) -> Self {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut studio = Dispatcher::new("cadcraft-studio", "CadCraft Studio", version);
        studio.list_changed = false;
        studio.allowed = vec!["read".into()];
        studio.instructions = Some(
            "Tools of the CadCraft app windows the person has opened in Studio and allowed in Settings › Assistants. windows_list shows them; each tool acts on the window used last unless you pass window."
                .into(),
        );
        let mut window = Dispatcher::new("cadcraft", "CadCraft", version);
        window.list_changed = false;
        window.allowed = groups.clone();
        let l2 = log.clone();
        window.log = Some(Box::new(move |e: &Value| {
            let mut l = l2.borrow_mut();
            l.push(e.clone());
            if l.len() > LOG_MAX {
                l.remove(0);
            }
        }));
        let allowed_now = groups.clone();
        window.how_to_allow = Some(Box::new(move |g: &str| {
            let mut all = allowed_now.clone();
            if !all.iter().any(|x| x == g) {
                all.push(g.to_string());
            }
            format!("The person can allow it by starting CadCraft with --allow {}.", all.join(","))
        }));
        Control { studio, window, enabled: true, groups, sessions: HashMap::new(), log }
    }

    /// The person pressed Disconnect (or a new token was made): that session's next request is refused (-32002).
    pub fn disconnect(&mut self, sid: &str) {
        if let Some(s) = self.sessions.get_mut(sid) {
            s.ended = true;
        }
    }

    /// Sessions connected now (not ended).
    pub fn connected(&self) -> Vec<(String, Option<(String, String)>)> {
        self.sessions.iter().filter(|(_, s)| !s.ended).map(|(k, s)| (k.clone(), s.rpc.client.clone())).collect()
    }

    fn windows(&self, w: &dyn Window) -> Value {
        let n = if self.enabled { self.window.list(w as &dyn Tools).len() } else { 0 };
        json!([{"window": WINDOW_ID, "app": w.app(), "title": w.title(), "assistants": self.enabled, "allowed": if self.enabled { self.groups.clone() } else { Vec::new() }, "tools": n}])
    }

    fn session(&mut self, sid: &str) -> &mut WinSession {
        let key: String = sid.chars().take(120).collect();
        self.sessions.entry(key.clone()).or_insert_with(|| WinSession { ended: false, rpc: Session::new(&key) })
    }

    /// The window's own handling (`makeControl().handle`).
    fn window_handle(&mut self, msg: &Value, sid: &str, w: &mut dyn Window) -> Option<Value> {
        let id = msg.as_object().and_then(|o| o.get("id")).cloned();
        let refuse =
            |code: i64, text: String| id.clone().map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}}));
        let title = w.title();
        if !self.enabled {
            return refuse(-32001, format!("{title} does not take assistants: they are turned off in its Settings › Assistants."));
        }
        if self.session(sid).ended {
            return refuse(-32002, format!("The person disconnected this assistant from {title}. Connect again to continue."));
        }
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        if method == "tools/call" || method == "tools.call" {
            let name = msg.get("params").and_then(|p| p.get("name")).and_then(Value::as_str).unwrap_or_default();
            let group = w.tools().into_iter().find(|t| t.name == name).map(|t| t.group);
            if let Some(g) = group
                && g != "read"
                && let Some(doing) = w.busy()
            {
                let client = self.session(sid).rpc.client.as_ref().map(|c| c.0.clone());
                let t = (self.window.now)();
                let args = msg.get("params").and_then(|p| p.get("arguments")).cloned().unwrap_or_else(|| json!({}));
                let entry = json!({"time": crate::rpc::iso_time(t), "client": client, "tool": name, "group": g, "args": crate::rpc::log_args(&args, 0), "outcome": "refused", "undo": null, "ms": 0, "message": "busy"});
                if let Some(f) = self.window.log.as_mut() {
                    f(&entry);
                }
                return Some(
                    json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": {"content": [{"type": "text", "text": format!("{title} is in the middle of {doing} (the person is working). Try again when it is done.")}], "isError": true}}),
                );
            }
        }
        self.window.server.name = w.app().to_string();
        self.window.server.title = title;
        let mut s = self.session(sid).rpc.clone();
        let out = self.window.handle(msg, &mut s, &mut Gate { w });
        self.session(sid).rpc = s;
        out
    }

    /// One JSON-RPC message from the control channel (session = the HTTP session id) → the answer (None: none).
    pub fn handle(&mut self, msg: &Value, sid: &str, w: &mut dyn Window) -> Option<Value> {
        let windows = self.windows(w);
        let obj = match msg.as_object() {
            Some(o) if o.get("jsonrpc") == Some(&json!("2.0")) => o,
            _ => {
                let mut s = self.session(sid).rpc.clone();
                return self.studio.handle(msg, &mut s, &mut StudioTools { windows });
            }
        };
        let m = obj.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let id = obj.get("id").cloned();
        if m == "tools/list" || m == "tools.list" {
            let base = self.studio.list(&StudioTools { windows: windows.clone() });
            if !self.enabled {
                return Some(json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": {"tools": base}}));
            }
            let ask = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
            let theirs = self.window_handle(&ask, sid, w);
            let list = theirs
                .as_ref()
                .and_then(|a| a.get("result"))
                .and_then(|r| r.get("tools"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let win_prop = json!({"type": "string", "maxLength": 90, "description": "The app window to use (from windows_list); default the one used last."});
            let mut all = base;
            for mut t in list {
                if t.get("name").and_then(Value::as_str) == Some("windows_list") {
                    continue;
                }
                if let Some(schema) = t.get_mut("inputSchema").and_then(Value::as_object_mut) {
                    let props = schema.entry("properties").or_insert_with(|| Value::Object(Map::new()));
                    if let Some(p) = props.as_object_mut() {
                        p.insert("window".into(), win_prop.clone());
                    }
                }
                all.push(t);
            }
            return Some(json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": {"tools": all}}));
        }
        if m == "tools/call" || m == "tools.call" {
            let p = obj.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
            if p.get("name").and_then(Value::as_str) == Some("windows_list") {
                let mut s = self.session(sid).rpc.clone();
                let out = self.studio.handle(msg, &mut s, &mut StudioTools { windows });
                self.session(sid).rpc = s;
                return out;
            }
            let mut args = p.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
            let wid = args.get("window").and_then(Value::as_str).map(str::to_string);
            args.remove("window");
            let ok = self.enabled && wid.as_deref().is_none_or(|x| x == WINDOW_ID);
            if !ok {
                let text = match wid {
                    Some(x) => format!("There is no app window {x} taking assistants (windows_list shows them)."),
                    None => "No app window takes assistants yet: the person opens an app and turns on Settings › Assistants.".into(),
                };
                return id.map(
                    |id| json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}], "isError": true}}),
                );
            }
            let mut np = p.clone();
            np.insert("arguments".into(), Value::Object(args));
            let mut nm = obj.clone();
            nm.insert("params".into(), Value::Object(np));
            return self.window_handle(&Value::Object(nm), sid, w);
        }
        if m == "initialize" || m == "ping" || m == "logging/setLevel" || m == "control.hello" || m.starts_with("notifications/") {
            let mut s = self.session(sid).rpc.clone();
            let out = self.studio.handle(msg, &mut s, &mut StudioTools { windows });
            // the window learns who the client is (its log names it)
            self.session(sid).rpc = s;
            return out;
        }
        self.window_handle(msg, sid, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Win {
        busy: Option<String>,
        calls: Vec<String>,
    }
    impl Tools for Win {
        fn tools(&self) -> Vec<ToolDef> {
            vec![
                ToolDef::new("app_info", "read", "About this window", "This window.", None),
                ToolDef::new(
                    "command_run",
                    "edit",
                    "Run a CadCraft command",
                    "Types.",
                    Some(
                        json!({"type": "object", "properties": {"input": {"type": "string"}}, "required": ["input"], "additionalProperties": false}),
                    ),
                ),
            ]
        }
        fn run(&mut self, name: &str, args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
            self.calls.push(name.into());
            Ok(ToolOut::Json(json!({"tool": name, "args": args})))
        }
    }
    impl Window for Win {
        fn app(&self) -> &'static str {
            "cadcraft"
        }
        fn title(&self) -> String {
            "CadCraft".into()
        }
        fn busy(&self) -> Option<String> {
            self.busy.clone()
        }
    }

    fn call(c: &mut Control, w: &mut Win, sid: &str, m: Value) -> Value {
        c.handle(&m, sid, w).unwrap_or(Value::Null)
    }

    #[test]
    fn routing() {
        let mut c = Control::new(vec!["read".into()], "0.0.1");
        c.window.now = Box::new(|| 0.0);
        c.studio.now = Box::new(|| 0.0);
        let mut w = Win { busy: None, calls: vec![] };
        let init = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "claude"}}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "cadcraft-studio");
        assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], false);
        let list = call(&mut c, &mut w, "s1", json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["windows_list", "app_info"], "edit tools are not listed without the edit permission");
        assert_eq!(list["result"]["tools"][1]["inputSchema"]["properties"]["window"]["maxLength"], 90);
        let wl = call(&mut c, &mut w, "s1", json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "windows_list"}}));
        assert_eq!(
            wl["result"]["structuredContent"]["windows"][0],
            json!({"window": "main", "app": "cadcraft", "title": "CadCraft", "assistants": true, "allowed": ["read"], "tools": 1})
        );
        let info = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "app_info", "arguments": {"window": "main"}}}),
        );
        assert_eq!(info["result"]["structuredContent"]["args"], json!({}), "window is taken off before the tool sees it");
        let other = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "app_info", "arguments": {"window": "w2"}}}),
        );
        assert_eq!(other["result"]["content"][0]["text"], "There is no app window w2 taking assistants (windows_list shows them).");
        let refused = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "command_run", "arguments": {"input": "LINE"}}}),
        );
        assert_eq!(refused["result"]["isError"], true);
        assert!(refused["result"]["content"][0]["text"].as_str().unwrap().starts_with("command_run needs the edit permission"));
        assert_eq!(c.log.borrow().last().unwrap()["outcome"], "refused");
        assert_eq!(c.log.borrow().last().unwrap()["client"], "claude");
        // busy: an edit is refused while the person is mid-command; reads still work
        c.groups = vec!["read".into(), "edit".into()];
        c.window.allowed = c.groups.clone();
        w.busy = Some("LINE".into());
        let busy = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "command_run", "arguments": {"input": "x"}}}),
        );
        assert_eq!(
            busy["result"]["content"][0]["text"],
            "CadCraft is in the middle of LINE (the person is working). Try again when it is done."
        );
        assert!(w.calls.iter().all(|c| c != "command_run"));
        w.busy = None;
        let ran = call(
            &mut c,
            &mut w,
            "s1",
            json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "command_run", "arguments": {"input": "x"}}}),
        );
        assert_eq!(ran["result"]["structuredContent"]["tool"], "command_run");
        // Disconnect: -32002 from then on
        c.disconnect("s1");
        let gone = call(&mut c, &mut w, "s1", json!({"jsonrpc": "2.0", "id": 9, "method": "app.info"}));
        assert_eq!(gone["error"]["code"], -32002);
        // assistants off: -32001, and nothing but windows_list listed
        c.enabled = false;
        let off = call(&mut c, &mut w, "s2", json!({"jsonrpc": "2.0", "id": 10, "method": "app.info"}));
        assert_eq!(off["error"]["code"], -32001);
        let list = call(&mut c, &mut w, "s2", json!({"jsonrpc": "2.0", "id": 11, "method": "tools/list"}));
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 1);
    }
}
