//! The tool dispatcher every surface shares — tools/mcp/core.mjs (0.5), answer for answer: JSON-RPC 2.0 with the MCP
//! methods (`initialize`, `ping`, `tools/list`, `tools/call`, `logging/setLevel`, the notifications) and the JSON
//! control channel's (`control.hello`, `tools.list`, `tools.call`, `app.info`; docs/AGENTS-SPEC.md §6.1).
//!
//! The dispatcher keeps no tools itself: a [`Tools`] implementation lists them and runs them (the headless tools, or the
//! window's live tools). The dispatcher checks permission groups and arguments (`schema::check`) before a tool runs,
//! turns a tool's answer into an MCP `CallToolResult` (structured content + its JSON text with an indent of one, as
//! `JSON.stringify(x, null, 1)`), and logs every call (AGENTS-SPEC §4). Messages never panic: anything malformed gets
//! the JSON-RPC error 0.5 sends.

use serde_json::{Map, Value, json};

use crate::json::{js_string, slice16, stringify_indent};
use crate::schema;

/// MCP protocol versions, newest first (the answer is the client's when it is one of these, else the newest).
pub const MCP_PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Permission groups (AGENTS-SPEC §3).
pub const TOOL_GROUPS: [&str; 4] = ["read", "edit", "save", "export"];

/// One tool's definition (AGENTS-SPEC §1; `run` lives in the [`Tools`] implementation).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    /// read | edit | save | export
    pub group: &'static str,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

impl ToolDef {
    /// A tool with an input schema given as JSON (`{type: object, …}`); a missing schema means no arguments.
    pub fn new(name: &str, group: &'static str, title: &str, description: &str, input_schema: Option<Value>) -> Self {
        ToolDef {
            name: name.into(),
            group,
            title: title.into(),
            description: description.into(),
            input_schema: input_schema.unwrap_or_else(|| json!({"type": "object", "properties": {}, "additionalProperties": false})),
            output_schema: None,
        }
    }

    /// The MCP tool definition (`definition()` in core.mjs).
    pub fn definition(&self) -> Value {
        let mut ann = Map::new();
        ann.insert("title".into(), Value::String(self.title.clone()));
        if let Value::Object(h) = group_hints(self.group) {
            ann.extend(h);
        }
        let mut d = Map::new();
        d.insert("name".into(), Value::String(self.name.clone()));
        d.insert("title".into(), Value::String(self.title.clone()));
        d.insert("description".into(), Value::String(format!("{} (Permission: {}.)", self.description, self.group)));
        d.insert("inputSchema".into(), self.input_schema.clone());
        d.insert("annotations".into(), Value::Object(ann));
        if let Some(o) = &self.output_schema {
            d.insert("outputSchema".into(), o.clone());
        }
        Value::Object(d)
    }
}

/// What a tool returns: plain JSON (sent as structured content and as text), or its own content list (images).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOut {
    Json(Value),
    Raw { content: Vec<Value>, structured: Option<Value> },
}

/// A tool that failed: a plain sentence for the assistant (never a stack). `refused` = the log's outcome is "refused".
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub message: String,
    pub refused: bool,
}

impl ToolError {
    pub fn new(message: impl Into<String>) -> Self {
        ToolError { message: message.into(), refused: false }
    }
    pub fn refused(message: impl Into<String>) -> Self {
        ToolError { message: message.into(), refused: true }
    }
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError::new(s)
    }
}
impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        ToolError::new(s)
    }
}

/// A JSON-RPC error a method can raise.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// What a call knows about itself (`ctx` in core.mjs).
#[derive(Debug, Clone)]
pub struct CallCtx {
    pub group: &'static str,
    pub client: Option<String>,
    pub session: String,
    pub tool: String,
}

/// The tools behind a dispatcher.
pub trait Tools {
    /// Every tool there is (the dispatcher filters by the allowed groups).
    fn tools(&self) -> Vec<ToolDef>;
    /// Run one tool on arguments already checked against its schema (defaults filled in).
    fn run(&mut self, name: &str, args: Value, ctx: &CallCtx) -> Result<ToolOut, ToolError>;
    /// `app.info`; None = `{app, version}` from the server info.
    fn info(&mut self) -> Option<Value> {
        None
    }
    /// A raw method (`host.control.on('<method>')`); None = not known here.
    fn method(&mut self, _name: &str, _params: &Value, _session: &mut Session) -> Option<Result<Value, RpcError>> {
        None
    }
}

/// One client's session (per stdio connection, Mcp-Session-Id or control session).
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub id: String,
    pub protocol_version: Option<String>,
    /// (name, version) from `initialize`'s clientInfo or `control.hello`'s client
    pub client: Option<(String, String)>,
    pub initialized_at: Option<f64>,
    pub ready: bool,
    pub log_level: Option<String>,
}

impl Session {
    pub fn new(id: &str) -> Self {
        Session { id: id.into(), ..Default::default() }
    }
}

/// Annotations an MCP client shows, from the tool's group (`groupHints`).
pub fn group_hints(group: &str) -> Value {
    let (ro, de, id) = match group {
        "read" => (true, false, true),
        "edit" => (false, false, false),
        "save" => (false, true, true),
        _ => (false, false, true),
    };
    json!({"readOnlyHint": ro, "destructiveHint": de, "idempotentHint": id, "openWorldHint": false})
}

/// Arguments for the log: strings over 200 characters cut, at most 20 keys, nested once (`logArgs`).
pub fn log_args(v: &Value, depth: usize) -> Value {
    match v {
        Value::String(s) => {
            if crate::json::len16(s) > 200 {
                Value::String(format!("{}…", slice16(s, 200)))
            } else {
                v.clone()
            }
        }
        Value::Array(a) => {
            if depth > 1 {
                Value::String(format!("[{} items]", a.len()))
            } else {
                Value::Array(a.iter().take(20).map(|x| log_args(x, depth.saturating_add(1))).collect())
            }
        }
        Value::Object(o) => {
            if depth > 1 {
                return Value::String("{…}".into());
            }
            Value::Object(o.iter().take(20).map(|(k, x)| (k.clone(), log_args(x, depth.saturating_add(1)))).collect())
        }
        _ => v.clone(),
    }
}

/// Milliseconds since 1970 as an ISO 8601 string (`new Date(t).toISOString()`).
// the day arithmetic runs on a clamped range (years 0000–9999): no step can overflow
#[allow(clippy::arithmetic_side_effects)]
pub fn iso_time(ms: f64) -> String {
    let ms = if ms.is_finite() { ms.floor() } else { 0.0 };
    // clamp to the years JavaScript's Date prints as YYYY (0000–9999)
    let ms = ms.clamp(-62_167_219_200_000.0, 253_402_300_799_999.0);
    #[allow(clippy::cast_possible_truncation)]
    let total = ms as i64;
    let days = total.div_euclid(86_400_000);
    let rem = total.rem_euclid(86_400_000);
    // civil from days (Howard Hinnant's algorithm)
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, (rem / 60_000) % 60, (rem / 1000) % 60, rem % 1000)
}

/// The clock: milliseconds since 1970 (0 on the web build, which has no system clock here; hosts set their own).
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0).floor()
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

/// Server identity (`serverInfo`).
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub title: String,
    pub version: String,
}

type LogFn = Box<dyn FnMut(&Value)>;
type HowFn = Box<dyn Fn(&str) -> String>;
type ClockFn = Box<dyn Fn() -> f64>;

/// The dispatcher (`createDispatcher`).
pub struct Dispatcher {
    pub server: ServerInfo,
    /// the groups allowed now
    pub allowed: Vec<String>,
    pub instructions: Option<String>,
    /// tools/list_changed notifications can be pushed (false over HTTP without a stream)
    pub list_changed: bool,
    pub how_to_allow: Option<HowFn>,
    pub log: Option<LogFn>,
    pub now: ClockFn,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher").field("server", &self.server).field("allowed", &self.allowed).finish()
    }
}

fn rpc(code: i64, message: impl Into<String>) -> RpcError {
    RpcError { code, message: message.into() }
}

/// `String(x || '').slice(0, n)` for a client name field.
fn field_text(v: Option<&Value>, n: usize) -> String {
    let s = match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) if s.is_empty() => String::new(),
        Some(Value::Number(x)) if x.as_f64() == Some(0.0) => String::new(),
        Some(x) => js_string(x),
    };
    slice16(&s, n)
}

impl Dispatcher {
    pub fn new(name: &str, title: &str, version: &str) -> Self {
        Dispatcher {
            server: ServerInfo { name: name.into(), title: title.into(), version: version.into() },
            allowed: vec!["read".into()],
            instructions: None,
            list_changed: true,
            how_to_allow: None,
            log: None,
            now: Box::new(now_ms),
        }
    }

    fn allows(&self, group: &str) -> bool {
        self.allowed.iter().any(|g| g == group)
    }

    /// The tool definitions the permissions allow.
    pub fn list(&self, tools: &dyn Tools) -> Vec<Value> {
        tools.tools().into_iter().filter(|t| self.allows(t.group)).map(|t| t.definition()).collect()
    }

    fn write_log(&mut self, entry: &Value) {
        if let Some(f) = self.log.as_mut() {
            f(entry);
        }
    }

    /// Run a tool: permission, schema, the tool, the log. Always a CallToolResult, except an unknown tool (-32602).
    pub fn call(&mut self, name: &str, args: Option<&Value>, session: &Session, tools: &mut dyn Tools) -> Result<Value, RpcError> {
        let t0 = (self.now)();
        let def = tools.tools().into_iter().find(|t| t.name == name);
        let client = session.client.as_ref().map(|c| c.0.clone());
        let mut entry = Map::new();
        entry.insert("time".into(), Value::String(iso_time(t0)));
        entry.insert("client".into(), client.clone().map_or(Value::Null, Value::String));
        entry.insert("tool".into(), Value::String(name.into()));
        entry.insert("group".into(), def.as_ref().map_or(Value::Null, |t| Value::String(t.group.into())));
        entry.insert("args".into(), log_args(args.unwrap_or(&json!({})), 0));
        entry.insert("outcome".into(), Value::String("ok".into()));
        entry.insert("undo".into(), Value::Null);
        entry.insert("ms".into(), json!(0));
        let Some(t) = def else {
            return Err(rpc(-32602, format!("Unknown tool: {name}")));
        };
        let mut finish = |me: &mut Dispatcher, res: Value, outcome: &str, message: Option<&str>, undo: Option<Value>| -> Value {
            entry.insert("outcome".into(), Value::String(outcome.into()));
            entry.insert("ms".into(), json!((me.now)() - t0));
            if let Some(u) = undo {
                entry.insert("undo".into(), u);
            }
            if let Some(m) = message {
                entry.insert("message".into(), Value::String(slice16(m, 300)));
            }
            me.write_log(&Value::Object(entry.clone()));
            res
        };
        let fail = |msg: &str| json!({"content": [{"type": "text", "text": msg}], "isError": true});
        if !self.allows(t.group) {
            let how = self.how_to_allow.as_ref().map(|f| f(t.group)).unwrap_or_default();
            let msg = format!("{} needs the {} permission, which is not allowed. {how}", t.name, t.group);
            return Ok(finish(self, fail(&msg), "refused", Some(&msg), None));
        }
        let a = match args {
            None | Some(Value::Null) => json!({}),
            Some(v) => v.clone(),
        };
        if !a.is_object() {
            let msg = "arguments must be an object.";
            return Ok(finish(self, fail(msg), "refused", Some(msg), None));
        }
        let mut copy = a;
        let problems = schema::check(&t.input_schema, &mut copy, "arguments");
        if !problems.is_empty() {
            let msg = format!("Invalid arguments: {}", problems.iter().take(8).cloned().collect::<Vec<_>>().join(" "));
            return Ok(finish(self, fail(&msg), "refused", Some(&msg), None));
        }
        let ctx = CallCtx { group: t.group, client, session: session.id.clone(), tool: t.name.clone() };
        match tools.run(&t.name, copy, &ctx) {
            Ok(ToolOut::Raw { content, structured }) => {
                let mut res = Map::new();
                res.insert("content".into(), Value::Array(content));
                let undo = structured.as_ref().and_then(|s| s.get("undo")).filter(|u| truthy(u)).cloned();
                if let Some(s) = structured {
                    res.insert("structuredContent".into(), s);
                }
                Ok(finish(self, Value::Object(res), "ok", None, undo))
            }
            Ok(ToolOut::Json(r)) => {
                let structured = match r {
                    Value::Null => json!({"ok": true}),
                    Value::Object(_) => r,
                    other => json!({"result": other}),
                };
                let undo = structured.get("undo").filter(|u| truthy(u)).cloned();
                let text = stringify_indent(&structured, 1);
                let is_err = structured.get("error").is_some_and(truthy) && structured.get("isError").is_some_and(truthy);
                let err_text = structured.get("error").map(js_string);
                let mut res = Map::new();
                res.insert("content".into(), json!([{"type": "text", "text": text}]));
                res.insert("structuredContent".into(), structured);
                if is_err {
                    res.insert("isError".into(), Value::Bool(true));
                    return Ok(finish(self, Value::Object(res), "error", err_text.as_deref(), undo));
                }
                Ok(finish(self, Value::Object(res), "ok", None, undo))
            }
            Err(e) => {
                let outcome = if e.refused { "refused" } else { "error" };
                Ok(finish(self, fail(&e.message), outcome, Some(&e.message), None))
            }
        }
    }

    /// One JSON-RPC message → a response, or None for a notification. Never panics.
    pub fn handle(&mut self, msg: &Value, session: &mut Session, tools: &mut dyn Tools) -> Option<Value> {
        let obj = msg.as_object();
        let is_note = obj.is_some_and(|o| !o.contains_key("id"));
        let id = obj.and_then(|o| o.get("id")).cloned();
        let valid = obj.is_some_and(|o| o.get("jsonrpc") == Some(&json!("2.0")) && o.get("method").is_some_and(Value::is_string));
        if !valid {
            if let Some(o) = obj
                && o.get("jsonrpc") == Some(&json!("2.0"))
                && !o.contains_key("method")
                && (o.contains_key("result") || o.contains_key("error"))
            {
                return None; // a response to us: nothing was asked
            }
            let id = id.filter(|v| v.is_string() || v.is_number()).unwrap_or(Value::Null);
            return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32600, "message": "Invalid request"}}));
        }
        let method = obj.and_then(|o| o.get("method")).and_then(Value::as_str).unwrap_or_default().to_string();
        let empty = Value::Object(Map::new());
        let p = obj.and_then(|o| o.get("params")).filter(|v| v.is_object()).unwrap_or(&empty).clone();
        let reply = |result: Value| -> Option<Value> {
            if is_note { None } else { Some(json!({"jsonrpc": "2.0", "id": id.clone().unwrap_or(Value::Null), "result": result})) }
        };
        let result: Result<Option<Value>, RpcError> = (|| {
            Ok(match method.as_str() {
                "initialize" => {
                    let asked = p.get("protocolVersion").and_then(Value::as_str);
                    let v = asked.filter(|a| MCP_PROTOCOLS.contains(a)).unwrap_or(MCP_PROTOCOLS[0]).to_string();
                    session.protocol_version = Some(v.clone());
                    session.client = Some(match p.get("clientInfo") {
                        Some(ci @ (Value::Object(_) | Value::Array(_))) => {
                            (field_text(ci.get("name"), 100), field_text(ci.get("version"), 40))
                        }
                        _ => ("unknown".into(), String::new()),
                    });
                    session.initialized_at = Some((self.now)());
                    let mut r = Map::new();
                    r.insert("protocolVersion".into(), Value::String(v));
                    r.insert("capabilities".into(), json!({"tools": {"listChanged": self.list_changed}, "logging": {}}));
                    r.insert(
                        "serverInfo".into(),
                        json!({"name": self.server.name, "title": self.server.title, "version": self.server.version}),
                    );
                    if let Some(i) = &self.instructions {
                        r.insert("instructions".into(), Value::String(i.clone()));
                    }
                    reply(Value::Object(r))
                }
                "notifications/initialized" => {
                    session.ready = true;
                    None
                }
                "notifications/cancelled" | "notifications/progress" | "notifications/roots/list_changed" => None,
                "ping" => reply(json!({})),
                "logging/setLevel" => {
                    let lv = p.get("level").filter(|l| truthy(l)).map_or_else(|| "info".to_string(), js_string);
                    session.log_level = Some(lv);
                    reply(json!({}))
                }
                "tools/list" | "tools.list" => reply(json!({"tools": self.list(tools)})),
                "tools/call" | "tools.call" => {
                    let Some(name) = p.get("name").and_then(Value::as_str) else {
                        return Err(rpc(-32602, format!("{method} needs a tool name.")));
                    };
                    let r = self.call(name, p.get("arguments"), session, tools)?;
                    reply(r)
                }
                "control.hello" => {
                    session.client = Some(match p.get("client") {
                        Some(c @ (Value::Object(_) | Value::Array(_))) => {
                            (field_text(c.get("name"), 100), field_text(c.get("version"), 40))
                        }
                        _ => ("script".into(), String::new()),
                    });
                    let n = self.list(tools).len();
                    reply(
                        json!({"app": self.server.name, "title": self.server.title, "version": self.server.version, "protocol": 1, "tools": n}),
                    )
                }
                "app.info" => {
                    let info = tools.info().unwrap_or_else(|| json!({"app": self.server.name, "version": self.server.version}));
                    reply(info)
                }
                other => match tools.method(other, &p, session) {
                    Some(r) => reply(r?),
                    None if is_note => None,
                    None => return Err(rpc(-32601, format!("Method not found: {other}"))),
                },
            })
        })();
        match result {
            Ok(r) => r,
            Err(e) => {
                if is_note {
                    None
                } else {
                    Some(json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "error": {"code": e.code, "message": e.message}}))
                }
            }
        }
    }
}

/// JavaScript truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Two;
    impl Tools for Two {
        fn tools(&self) -> Vec<ToolDef> {
            vec![
                ToolDef::new(
                    "echo",
                    "read",
                    "Echo",
                    "Says it back.",
                    Some(json!({"type": "object", "properties": {"x": {"type": "integer", "default": 7}}, "additionalProperties": false})),
                ),
                ToolDef::new("change", "edit", "Change", "Changes.", None),
            ]
        }
        fn run(&mut self, name: &str, args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
            match name {
                "echo" => Ok(ToolOut::Json(json!({"got": args["x"]}))),
                _ => Err(ToolError::new("no")),
            }
        }
    }

    fn d() -> Dispatcher {
        let mut d = Dispatcher::new("cadcraft", "CadCraft", "0.0.1");
        d.now = Box::new(|| 0.0);
        d.how_to_allow = Some(Box::new(|g| format!("Restart with --allow {g}.")));
        d
    }

    #[test]
    fn protocol() {
        let mut d = d();
        let mut s = Session::new("t");
        let init = d.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "clientInfo": {"name": "x", "version": "1"}}}), &mut s, &mut Two).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(crate::json::stringify(&init["result"]["capabilities"]), r#"{"tools":{"listChanged":true},"logging":{}}"#);
        let other = d
            .handle(
                &json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
                &mut s,
                &mut Two,
            )
            .unwrap();
        assert_eq!(other["result"]["protocolVersion"], "2025-11-25");
        assert!(d.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), &mut s, &mut Two).is_none());
        let list = d.handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}), &mut s, &mut Two).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(list["result"]["tools"][0]["description"], "Says it back. (Permission: read.)");
        let call =
            d.handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "echo"}}), &mut s, &mut Two).unwrap();
        assert_eq!(call["result"]["structuredContent"], json!({"got": 7}));
        assert_eq!(call["result"]["content"][0]["text"], "{\n \"got\": 7\n}");
        let refused =
            d.handle(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "change"}}), &mut s, &mut Two).unwrap();
        assert_eq!(
            refused["result"]["content"][0]["text"],
            "change needs the edit permission, which is not allowed. Restart with --allow edit."
        );
        let bad = d
            .handle(
                &json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "echo", "arguments": {"x": "a"}}}),
                &mut s,
                &mut Two,
            )
            .unwrap();
        assert_eq!(bad["result"]["content"][0]["text"], "Invalid arguments: arguments.x must be integer.");
        let unknown =
            d.handle(&json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "zz"}}), &mut s, &mut Two).unwrap();
        assert_eq!(unknown["error"]["code"], -32602);
        let nf = d.handle(&json!({"jsonrpc": "2.0", "id": 8, "method": "resources/list"}), &mut s, &mut Two).unwrap();
        assert_eq!(nf["error"], json!({"code": -32601, "message": "Method not found: resources/list"}));
        let inv = d.handle(&json!([1]), &mut s, &mut Two).unwrap();
        assert_eq!(inv, json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Invalid request"}}));
        assert!(d.handle(&json!({"jsonrpc": "2.0", "id": 9, "result": {}}), &mut s, &mut Two).is_none());
        let hello = d
            .handle(
                &json!({"jsonrpc": "2.0", "id": 10, "method": "control.hello", "params": {"client": {"name": "script"}}}),
                &mut s,
                &mut Two,
            )
            .unwrap();
        assert_eq!(hello["result"]["protocol"], 1);
    }

    #[test]
    fn times() {
        assert_eq!(iso_time(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(1_760_745_600_123.0), "2025-10-18T00:00:00.123Z");
        assert_eq!(iso_time(f64::NAN), "1970-01-01T00:00:00.000Z");
    }
}
