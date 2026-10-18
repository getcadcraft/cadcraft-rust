//! Fuzz MCP / control-channel message handling (cc_mcp::rpc and cc_mcp::live; messages come from an assistant over
//! stdio or the control channel): any bytes must parse as JSON and get the JSON-RPC answer 0.5 gives, or be refused;
//! tool arguments are checked against their schemas (patterns, enums, nesting) without panicking or stalling.
#![no_main]

use cc_mcp::live::{Control, Window};
use cc_mcp::{CallCtx, Dispatcher, Session, ToolDef, ToolError, ToolOut, Tools};
use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};

struct T;
impl Tools for T {
    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new(
                "objects_list",
                "read",
                "List objects",
                "Objects.",
                Some(json!({"type": "object", "properties": {
                    "doc": {"type": "string", "pattern": "^d[0-9]{1,6}$"},
                    "type": {"type": "string", "enum": ["line", "circle"]},
                    "within": {"type": "object", "properties": {"minX": {"type": "number"}}, "required": ["minX"], "additionalProperties": false},
                    "input": {"anyOf": [{"type": "string", "maxLength": 100}, {"type": "array", "items": {"anyOf": [{"type": "string"}, {"type": "object", "properties": {"pick": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}}, "required": ["pick"], "additionalProperties": false}]}, "maxItems": 50}]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 100}
                }, "required": ["doc"], "additionalProperties": false})),
            ),
            ToolDef::new("command_run", "edit", "Run", "Runs.", None),
        ]
    }
    fn run(&mut self, _name: &str, args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        Ok(ToolOut::Json(args))
    }
}
impl Window for T {
    fn app(&self) -> &'static str {
        "cadcraft"
    }
    fn title(&self) -> String {
        "CadCraft".into()
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(msg) = serde_json::from_slice::<Value>(data) else { return };
    let mut d = Dispatcher::new("cadcraft", "CadCraft", "0");
    d.now = Box::new(|| 0.0);
    let mut s = Session::new("fuzz");
    if let Some(a) = d.handle(&msg, &mut s, &mut T) {
        let _ = cc_mcp::json::stringify(&a);
    }
    // as tool arguments and as a tools/call
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "objects_list", "arguments": msg}});
    let _ = d.handle(&call, &mut s, &mut T);
    let mut c = Control::new(vec!["read".into(), "edit".into()], "0");
    c.window.now = Box::new(|| 0.0);
    c.studio.now = Box::new(|| 0.0);
    let _ = c.handle(&msg, "fuzz", &mut T);
    let _ = c.handle(&call, "fuzz", &mut T);
});
