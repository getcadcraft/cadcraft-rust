//! The call-script runner behind `cadcraft-cli civil <script.json>` and the parity harness (`forks/tools/cvdiff.mjs`,
//! whose JS driver mirrors every op here).
//!
//! A script is `{ "calls": [{ "op": "tin.buildSurface", "args": [...], "as": "S", "self": "S" }, …] }`: `as` keeps
//! the call's object (a surface, an alignment, a profile, a project …) under a name, `self` names the object a method
//! runs on. Each call gives `{ "ok": <the result's JSON form> }` or `{ "err": "<message>" }`. Numbers that JSON cannot
//! carry travel as "$NaN", "$Inf", "$-Inf" both ways.

use crate::js::{J, arr, as_str, numf};
use serde_json::{Value, json};
use std::any::Any;
use std::collections::HashMap;

/// Objects kept between calls, and the zone writers stamp local-time fields in (LandXML `date` / `time`) when a
/// call gives no `tz`: UTC unless the host passes its own ([`run_with_in`]).
#[derive(Default)]
pub struct Registry {
    objs: HashMap<String, Box<dyn Any>>,
    pub zone: cc_clock::Zone,
}
impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }
    pub fn put<T: Any>(&mut self, name: &str, v: T) {
        self.objs.insert(name.to_string(), Box::new(v));
    }
    pub fn get<T: Any>(&self, name: &str) -> Option<&T> {
        self.objs.get(name).and_then(|b| b.downcast_ref::<T>())
    }
    pub fn get_mut<T: Any>(&mut self, name: &str) -> Option<&mut T> {
        self.objs.get_mut(name).and_then(|b| b.downcast_mut::<T>())
    }
    pub fn take<T: Any>(&mut self, name: &str) -> Option<T> {
        let b = self.objs.remove(name)?;
        match b.downcast::<T>() {
            Ok(v) => Some(*v),
            Err(b) => {
                self.objs.insert(name.to_string(), b);
                None
            }
        }
    }
}

/// One call: op name, arguments, the name to keep the result under, the object it runs on.
pub struct Call<'a> {
    pub op: &'a str,
    pub args: &'a [Value],
    pub as_name: Option<&'a str>,
    pub this: Option<&'a str>,
}
impl Call<'_> {
    pub fn arg(&self, i: usize) -> Option<&Value> {
        self.args.get(i).filter(|v| !v.is_null())
    }
    pub fn raw(&self, i: usize) -> Option<&Value> {
        self.args.get(i)
    }
    pub fn num(&self, i: usize) -> f64 {
        numf(self.args.get(i))
    }
    pub fn str(&self, i: usize) -> &str {
        as_str(self.args.get(i)).unwrap_or("")
    }
}

/// A handler: Some(result) when it knows the op.
pub type Handler = fn(&mut Registry, &Call) -> Option<Result<Value, String>>;

/// Run a script through the handlers (tried in order) → the JSON array of results.
pub fn run_with(script: &Value, handlers: &[Handler]) -> Value {
    run_with_in(script, handlers, cc_clock::Zone::Utc)
}

/// [`run_with`] with local-time fields in `zone` (the host's; the CLI passes the machine's) unless a call gives `tz`.
pub fn run_with_in(script: &Value, handlers: &[Handler], zone: cc_clock::Zone) -> Value {
    let mut reg = Registry::new();
    reg.zone = zone;
    let mut out = Vec::new();
    for c in arr(script.get("calls")) {
        let call = Call {
            op: as_str(c.get("op")).unwrap_or(""),
            args: arr(c.get("args")),
            as_name: as_str(c.get("as")),
            this: as_str(c.get("self")),
        };
        let mut res = None;
        for h in handlers {
            res = h(&mut reg, &call);
            if res.is_some() {
                break;
            }
        }
        out.push(match res {
            Some(Ok(v)) => json!({ "ok": v }),
            Some(Err(e)) => json!({ "err": e }),
            None => json!({ "err": format!("unknown op {}", call.op) }),
        });
    }
    Value::Array(out)
}

/// Run a script with this crate's ops.
pub fn run(script: &Value) -> Value {
    run_with(script, &[handle])
}

fn need<'a, T: Any>(reg: &'a Registry, name: Option<&str>, what: &str) -> Result<&'a T, String> {
    let n = name.ok_or_else(|| format!("no {what} named"))?;
    reg.get::<T>(n).ok_or_else(|| format!("no object {n}"))
}

/// This crate's ops.
pub fn handle(reg: &mut Registry, c: &Call) -> Option<Result<Value, String>> {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    match module {
        "units" => crate::script_ops::units(c),
        "points" => crate::script_ops::points(c),
        "tin" => crate::script_ops::tin(reg, c),
        "surface" => crate::script_ops::surface(reg, c),
        "alignment" | "al" => crate::script_ops::alignment(reg, c),
        "profile" | "prof" => crate::script_ops::profile(reg, c),
        _ => crate::script_ops2::handle(reg, c),
    }
}

/// `need` for the op modules.
pub fn obj<'a, T: Any>(reg: &'a Registry, name: Option<&str>, what: &str) -> Result<&'a T, String> {
    need(reg, name, what)
}

/// Keep `v` under the call's `as` name (if any) and return `j`.
pub fn keep<T: Any>(reg: &mut Registry, c: &Call, v: T) {
    if let Some(n) = c.as_name {
        reg.put(n, v);
    }
}

/// `J` of a value for results.
pub fn jv<T: J + ?Sized>(v: &T) -> Value {
    v.j()
}
