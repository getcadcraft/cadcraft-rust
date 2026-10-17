//! The line objects BimCraft's modules read through cc-ifc's IfcAPI (`api.GetLine(mid, id, false)`, api.js
//! `lineObject` / `toApi` / `typedApi`): an attribute as the JS sees it — `null`, a handle `{ type: 5, value: id }`,
//! a value `{ type, name, value }` (its `value` a number, text, boolean, undefined, a list or a raw object) or a list
//! of those — and the few JS operations the modules apply to them (`plain`, `val`, `?.value`, iteration).
//!
//! Lines come from `cc-io-ifc`'s [`IfcFile`] (the same STEP index and parser the JS uses); nothing is copied.

use cc_io_ifc::model::{Id, IfcFile};
use cc_io_ifc::schema::{AttrCode, Entity, Schema, schema_for};
use cc_io_ifc::value::Value;

use crate::js::{PV, num_str, round, str_num};

/// What went wrong where the JS would throw (a TypeError on a malformed line, a parse error).
#[derive(Clone, Debug, PartialEq)]
pub struct JsErr(pub String);

impl std::fmt::Display for JsErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type R<T> = Result<T, JsErr>;

pub fn type_err(what: &str) -> JsErr {
    JsErr(format!("TypeError: {what}"))
}

/// A value's `value` member (`typedApi`'s unwrapped primitive).
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Prim>),
    /// a raw step.js object left as it is (`{ ref }`, `{ bin }`, `{ derived }`): `String(x)` = "[object Object]"
    Obj,
}

impl Prim {
    /// `String(x)` (an array joins its items with ',', null / undefined items empty).
    pub fn to_js_string(&self) -> String {
        match self {
            Prim::Undef => "undefined".into(),
            Prim::Null => "null".into(),
            Prim::Bool(b) => b.to_string(),
            Prim::Num(n) => num_str(*n),
            Prim::Str(s) => s.clone(),
            Prim::Arr(a) => a
                .iter()
                .map(|x| if matches!(x, Prim::Undef | Prim::Null) { String::new() } else { x.to_js_string() })
                .collect::<Vec<_>>()
                .join(","),
            Prim::Obj => "[object Object]".into(),
        }
    }
}

/// An attribute in the API shape.
#[derive(Clone, Debug, PartialEq)]
pub enum AV {
    /// `null` (also an attribute the parameter list is too short for)
    Null,
    /// `{ type: 0, value: null, derived: true }`
    Derived,
    /// `{ type: 5, value: id }`
    Handle(f64),
    /// `{ type, name, value }`
    Val(Prim),
    List(Vec<AV>),
}

impl AV {
    /// JS truthiness (every object and array is truthy).
    pub fn truthy(&self) -> bool {
        !matches!(self, AV::Null)
    }
    /// `v?.value` as a number when it is one (a handle's id, a numeric value).
    pub fn value_num(&self) -> Option<f64> {
        match self {
            AV::Handle(n) => Some(*n),
            AV::Val(Prim::Num(n)) => Some(*n),
            _ => None,
        }
    }
    /// `h.value` for a list member: TypeError on `null`.
    pub fn member_num(&self) -> R<Option<f64>> {
        if matches!(self, AV::Null) {
            return Err(type_err("Cannot read properties of null (reading 'value')"));
        }
        Ok(self.value_num())
    }
    /// `for (const x of v)`: a list's members; TypeError for anything else that is not iterable. A value whose
    /// `value` is text iterates nothing useful here (callers only read `.value` / handles), so it is refused too:
    /// api.js values are objects, never strings.
    pub fn iter_list(&self) -> R<&[AV]> {
        match self {
            AV::List(l) => Ok(l),
            _ => Err(type_err("object is not iterable")),
        }
    }
    pub fn is_handle(&self) -> bool {
        matches!(self, AV::Handle(_))
    }
}

/// `toApi(v, t, schema)`.
pub fn to_api(v: &Value, t: &AttrCode, schema: &Schema) -> AV {
    to_api_at(v, t.agg, &t.base, schema)
}

fn to_api_at(v: &Value, agg: u32, base: &str, schema: &Schema) -> AV {
    match v {
        Value::Null => AV::Null,
        Value::Derived => AV::Derived,
        Value::List(l) => {
            let inner = agg.saturating_sub(1);
            AV::List(l.iter().map(|x| to_api_at(x, inner, base, schema)).collect())
        }
        Value::Ref(n) => AV::Handle(*n),
        Value::Typed(t) => typed_api(&t.0, &t.1, schema),
        Value::Enum(e) => {
            if base == "b" || base == "l" {
                AV::Val(match &**e {
                    "T" => Prim::Bool(true),
                    "F" => Prim::Bool(false),
                    _ => Prim::Undef,
                })
            } else {
                AV::Val(Prim::Str(e.to_string()))
            }
        }
        Value::Bin(h) => AV::Val(Prim::Str(h.to_string())),
        Value::Str(s) => AV::Val(Prim::Str(s.lossy().into_owned())),
        Value::Num(n) => AV::Val(Prim::Num(*n)),
    }
}

/// A defined type's code in this schema, else in any (`anyTypeCode`), else 's'.
fn type_code<'s>(name: &str, schema: &'s Schema) -> &'s str {
    if let Some(c) = schema.type_code(name) {
        return c;
    }
    for s in ["IFC4X3", "IFC4", "IFC2X3"] {
        if let Some(c) = schema_for(s).0.type_code(name) {
            return c;
        }
    }
    "s"
}

/// `typedApi(name, value, schema)` → its `{ value }`.
fn typed_api(name: &str, value: &Value, schema: &Schema) -> AV {
    let code = type_code(name, schema);
    let base = code.trim_start_matches('[');
    AV::Val(unwrap(value, base))
}

fn unwrap(x: &Value, base: &str) -> Prim {
    let logical = base == "b" || base == "l";
    match x {
        Value::List(l) => Prim::Arr(l.iter().map(|y| unwrap(y, base)).collect()),
        Value::Enum(e) => {
            if logical {
                match &**e {
                    "T" => Prim::Bool(true),
                    "F" => Prim::Bool(false),
                    _ => Prim::Undef,
                }
            } else {
                Prim::Str(e.to_string())
            }
        }
        Value::Typed(t) => unwrap(&t.1, base),
        Value::Ref(_) | Value::Bin(_) | Value::Derived => Prim::Obj,
        Value::Null => Prim::Null,
        Value::Str(s) => {
            let s = s.lossy().into_owned();
            if logical {
                return match s.as_str() {
                    "T" => Prim::Bool(true),
                    "F" => Prim::Bool(false),
                    _ => Prim::Str(s),
                };
            }
            if matches!(base, "r" | "n" | "i") && !crate::js::trim(&s).is_empty() {
                let n = str_num(&s);
                if n.is_finite() {
                    return Prim::Num(n);
                }
            }
            Prim::Str(s)
        }
        Value::Num(n) => Prim::Num(*n),
    }
}

/// One line read the way `api.GetLine(mid, id, false)` reads it: None = `null` (no such line, or a complex instance).
#[derive(Clone, Copy)]
pub struct Line<'f, 'a> {
    pub f: &'f IfcFile<'a>,
    pub id: Id,
    pub ent: Option<&'static Entity>,
    /// the upper-case type name
    pub up: &'f str,
    params: &'f [Value],
}

impl std::fmt::Debug for Line<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Line").field("id", &self.id).field("up", &self.up).finish()
    }
}

/// `GetLine(mid, id, false)`: Ok(None) = null; Err = the parse error the JS throws.
pub fn get_line<'f, 'a>(f: &'f IfcFile<'a>, id: Id) -> R<Option<Line<'f, 'a>>> {
    let Some(up) = f.ty(id) else { return Ok(None) };
    if up.is_empty() {
        return Ok(None);
    }
    let params = f.params(id).map_err(|e| JsErr(e.0))?.unwrap_or(&[]);
    Ok(Some(Line { f, id, ent: f.schema.entity(up), up, params }))
}

impl<'f, 'a> Line<'f, 'a> {
    /// `line.constructor.name`: the schema's name ("IfcWall"), else the upper-case type.
    pub fn class(&self) -> &str {
        match self.ent {
            Some(e) => &e.name,
            None => self.up,
        }
    }
    /// `'Name' in line`
    pub fn has(&self, name: &str) -> bool {
        match self.ent {
            Some(e) => e.attr_index(name).is_some(),
            None => name.strip_prefix("Arg").and_then(|k| k.parse::<usize>().ok()).is_some_and(|k| k < self.params.len()),
        }
    }
    /// `line[name]`: None = undefined (no such attribute).
    pub fn get(&self, name: &str) -> Option<AV> {
        let e = self.ent?;
        let k = e.attr_index(name)?;
        let a = e.attrs.get(k)?;
        Some(match self.params.get(k) {
            Some(v) => to_api(v, &a.t, self.f.schema),
            None => AV::Null,
        })
    }
    /// The attributes in schema order (`Object.keys(line)` minus expressID / type).
    pub fn attrs(&self) -> Vec<(String, AV)> {
        match self.ent {
            Some(e) => e
                .attrs
                .iter()
                .enumerate()
                .map(|(k, a)| {
                    let v = self.params.get(k).map_or(AV::Null, |v| to_api(v, &a.t, self.f.schema));
                    (a.name.to_string(), v)
                })
                .collect(),
            None => {
                let t = cc_io_ifc::schema::parse_attr_code("S");
                self.params.iter().enumerate().map(|(k, v)| (format!("Arg{k}"), to_api(v, &t, self.f.schema))).collect()
            }
        }
    }
}

/// `plain(v)` of ifc-props.js: a number (to 6 decimals), 'Yes' / 'No', text; None = undefined.
pub fn plain(v: Option<&AV>) -> Option<PV> {
    let v = v?;
    match v {
        AV::Null | AV::Derived => None,
        AV::List(l) => {
            let parts: Vec<String> = l.iter().filter_map(|x| plain(Some(x))).map(|p| p.text()).collect();
            if parts.is_empty() { None } else { Some(PV::s(&parts.join(", "))) }
        }
        AV::Handle(n) => plain_num(*n),
        AV::Val(p) => plain_prim(p),
    }
}

fn plain_num(n: f64) -> Option<PV> {
    if n.is_finite() { Some(PV::Num(round(n * 1e6) / 1e6)) } else { None }
}

fn plain_prim(p: &Prim) -> Option<PV> {
    match p {
        Prim::Undef | Prim::Null => None,
        Prim::Str(s) if s.is_empty() => None,
        Prim::Num(n) => plain_num(*n),
        Prim::Bool(true) => Some(PV::s("Yes")),
        Prim::Bool(false) => Some(PV::s("No")),
        Prim::Str(s) if s == ".T." || s == "T" => Some(PV::s("Yes")),
        Prim::Str(s) if s == ".F." || s == "F" => Some(PV::s("No")),
        other => Some(PV::s(&other.to_js_string())),
    }
}

/// `val(v)` of ifc-model.js (`''` → null): the raw value; None = null / undefined.
pub fn val_model(v: Option<&AV>) -> Option<Prim> {
    match v? {
        AV::Null => None,
        AV::Derived => None,
        AV::Handle(n) => Some(Prim::Num(*n)),
        AV::Val(Prim::Str(s)) if s.is_empty() => None,
        AV::Val(Prim::Undef | Prim::Null) => None,
        AV::Val(p) => Some(p.clone()),
        // an array has no `value`: val returns it as it is (truthy); callers only keep text / numbers
        AV::List(_) => Some(Prim::Obj),
    }
}

/// `GetNameFromTypeCode(GetLineType(mid, id))` for a line of type `up`, not upper-cased: the type code's first
/// registered name, pretty in IFC4X3 / IFC4 / IFC2X3.
pub fn name_from_code(up: &str) -> String {
    if up.is_empty() {
        return String::new();
    }
    // nameOfCode(code) is undefined for a name no schema registers → ''
    let known = ["IFC2X3", "IFC4", "IFC4X3"].iter().any(|s| {
        let sc = schema_for(s).0;
        sc.has(up) || sc.type_code(up).is_some()
    }) || matches!(up, "FILE_DESCRIPTION" | "FILE_NAME" | "FILE_SCHEMA");
    if !known {
        return String::new();
    }
    let upper = cc_io_ifc::schema::api_type_name(up);
    for s in ["IFC4X3", "IFC4", "IFC2X3"] {
        if let Some(p) = schema_for(s).0.pretty(&upper) {
            return p.to_string();
        }
    }
    upper
}

/// `IfcModel.className(id)`: the line's class ("IfcWallStandardCase"), else the type code's name with Ifc.
pub fn class_name(f: &IfcFile<'_>, id: Id) -> String {
    if let Ok(Some(l)) = get_line(f, id) {
        let c = l.class();
        if c.starts_with("Ifc") {
            return c.to_string();
        }
    }
    let up = f.ty(id).unwrap_or("");
    let n = name_from_code(up);
    match n.strip_prefix("IFC") {
        Some(rest) => format!("Ifc{rest}"),
        None => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_values() {
        assert_eq!(plain(Some(&AV::Val(Prim::Num(1.23456789)))), Some(PV::Num(1.234568)));
        assert_eq!(plain(Some(&AV::Val(Prim::Bool(true)))), Some(PV::s("Yes")));
        assert_eq!(plain(Some(&AV::Val(Prim::Str("F".into())))), Some(PV::s("No")));
        assert_eq!(plain(Some(&AV::Val(Prim::Str(String::new())))), None);
        let l = AV::List(vec![AV::Val(Prim::Num(2.0)), AV::Null, AV::Val(Prim::Str("x".into()))]);
        assert_eq!(plain(Some(&l)), Some(PV::s("2, x")));
        assert_eq!(plain(Some(&AV::Val(Prim::Arr(vec![Prim::Num(1.0), Prim::Num(2.5)])))), Some(PV::s("1,2.5")));
    }
}
