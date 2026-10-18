//! The writing side of cc-ifc's IfcAPI (lib/ifc/api.js) as BimCraft's editor and building tools use it: line objects
//! with their values in the JS shapes (`GetLine`, `toApi`), `CreateIfcEntity`, `CreateIfcType`, `WriteLine` (`fromApi`,
//! a new line at the next free id), `DeleteLine`, `CreateIFCGloballyUniqueId` (from the caller's seeded bytes),
//! `GetWorldTransformMatrix` (cc-ifc's placements on the edited file), and ifc-util.js `ids` / `val` /
//! `lengthUnitMm` / `retypeLine`.
//!
//! The model is an [`IfcFile`] holding its own bytes; every edit lands in its overlay (model.js), so readers anywhere in
//! cc-ifc / cc-bim see the edited file. Values keep what the JS objects carry (`type`, `name`, the unwrapped value), so a
//! line read, changed and written back comes out byte for byte as the JS writes it.

use std::collections::HashMap;
use std::sync::Arc;

use cc_io_ifc::model::{Id, IfcFile, id_key};
use cc_io_ifc::schema::{AttrCode, Schema, schema_for};
use cc_io_ifc::value::{JsStr, Value};

use crate::api::{JsErr, R};
use crate::js::{num_str, str_num};

// web-ifc's value kinds (api.js)
pub const STRING: u8 = 1;
pub const LABEL: u8 = 2;
pub const ENUM: u8 = 3;
pub const REAL: u8 = 4;
pub const REF: u8 = 5;
pub const INTEGER: u8 = 10;

/// The `value` of a typed object, as `typedApi` unwraps it.
#[derive(Clone, Debug, PartialEq)]
pub enum P {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(JsStr),
    Arr(Vec<P>),
    /// a raw step.js object left as it is ({ ref }, { bin }, DERIVED)
    Obj,
}

impl P {
    pub fn s(t: &str) -> P {
        P::Str(JsStr::Utf8(t.into()))
    }
    /// `Number(x)`
    pub fn number(&self) -> f64 {
        match self {
            P::Undef | P::Obj => f64::NAN,
            P::Null => 0.0,
            P::Bool(b) => f64::from(u8::from(*b)),
            P::Num(n) => *n,
            P::Str(s) => str_num(&s.lossy()),
            P::Arr(a) => match a.as_slice() {
                [] => 0.0,
                [x] => P::s(&x.string()).number(),
                _ => f64::NAN,
            },
        }
    }
    /// `String(x)`
    pub fn string(&self) -> String {
        match self {
            P::Undef => "undefined".into(),
            P::Null => "null".into(),
            P::Bool(b) => b.to_string(),
            P::Num(n) => num_str(*n),
            P::Str(s) => s.lossy().into_owned(),
            P::Arr(a) => {
                a.iter().map(|x| if matches!(x, P::Undef | P::Null) { String::new() } else { x.string() }).collect::<Vec<_>>().join(",")
            }
            P::Obj => "[object Object]".into(),
        }
    }
    /// JS truthiness
    pub fn truthy(&self) -> bool {
        match self {
            P::Undef | P::Null => false,
            P::Bool(b) => *b,
            P::Num(n) => *n != 0.0 && !n.is_nan(),
            P::Str(s) => !s.is_empty(),
            P::Arr(_) | P::Obj => true,
        }
    }
    pub fn as_str(&self) -> Option<std::borrow::Cow<'_, str>> {
        if let P::Str(s) = self { Some(s.lossy()) } else { None }
    }
}

/// A typed value `{ type, name, value }` (name None: an enumeration `{ type: 3, value }` or a bare string kind).
#[derive(Clone, Debug, PartialEq)]
pub struct TV {
    pub ty: u8,
    pub name: Option<Box<str>>,
    pub v: P,
}

/// An attribute value as the API objects hold it.
#[derive(Clone, Debug, PartialEq)]
pub enum JV {
    /// null / undefined
    Null,
    /// `{ type: 0, value: null, derived: true }`
    Derived,
    /// a Handle `{ type: 5, value: id }`
    H(f64),
    /// a bare JS number
    Num(f64),
    /// a bare JS string
    Str(JsStr),
    /// a bare boolean
    Bool(bool),
    T(Box<TV>),
    L(Vec<JV>),
}

impl JV {
    pub fn s(t: &str) -> JV {
        JV::Str(JsStr::Utf8(t.into()))
    }
    /// `{ type: 3, value: v }` (IfcPlacer `E(v)`)
    pub fn e(v: &str) -> JV {
        JV::T(Box::new(TV { ty: ENUM, name: None, v: P::s(v) }))
    }
    /// JS truthiness of the attribute (objects and arrays are truthy).
    pub fn truthy(&self) -> bool {
        match self {
            JV::Null => false,
            JV::Num(n) => *n != 0.0 && !n.is_nan(),
            JV::Str(s) => !s.is_empty(),
            JV::Bool(b) => *b,
            _ => true,
        }
    }
    /// `v?.value` of a handle (the referenced id).
    pub fn href(&self) -> Option<f64> {
        if let JV::H(n) = self { Some(*n) } else { None }
    }
    /// The handles of a list attribute (`(v || []).map(h => h.value)`), non-handles skipped.
    pub fn hrefs(&self) -> Vec<f64> {
        match self {
            JV::L(l) => l.iter().filter_map(JV::href).collect(),
            _ => Vec::new(),
        }
    }
    /// ifc-util.js `val(v)`: null → null, an object with `value` → its value, a primitive → itself.
    pub fn val(&self) -> P {
        match self {
            JV::Null => P::Null,
            JV::Derived => P::Null,
            JV::H(n) => P::Num(*n),
            JV::Num(n) => P::Num(*n),
            JV::Str(s) => P::Str(s.clone()),
            JV::Bool(b) => P::Bool(*b),
            JV::T(t) => t.v.clone(),
            JV::L(_) => P::Obj,
        }
    }
    /// `val(v)` when it is text.
    pub fn val_str(&self) -> Option<String> {
        match self.val() {
            P::Str(s) => Some(s.lossy().into_owned()),
            _ => None,
        }
    }
    /// `val(v) || ''` as text (a number or boolean value as `String(x)`, a falsy one as '').
    pub fn text_or_empty(&self) -> String {
        let v = self.val();
        if v.truthy() { v.string() } else { String::new() }
    }
    /// `v.name` of a typed value.
    pub fn tname(&self) -> Option<&str> {
        if let JV::T(t) = self { t.name.as_deref() } else { None }
    }
}

/// A line object (`api.GetLine(mid, id, false)` / `CreateIfcEntity`): its id (−1 until written), its entity (upper
/// case) and its attributes in schema order.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub id: f64,
    pub up: Box<str>,
    /// `line.constructor.name` ("IfcWall"), or the upper-case type of an unknown entity
    pub class: Box<str>,
    pub attrs: Vec<(Box<str>, JV)>,
}

impl Line {
    /// `line[name]`: None = undefined (no such attribute); the last attribute of a repeated name, as on the object.
    pub fn get(&self, name: &str) -> Option<&JV> {
        self.attrs.iter().rev().find(|(n, _)| &**n == name).map(|(_, v)| v)
    }
    /// `line[name]` with undefined read as null.
    pub fn a(&self, name: &str) -> &JV {
        self.get(name).unwrap_or(&JV::Null)
    }
    /// `line[name] = v` (every attribute of that name: the object has one property).
    pub fn set(&mut self, name: &str, v: JV) {
        let mut hit = false;
        for (n, x) in &mut self.attrs {
            if &**n == name {
                *x = v.clone();
                hit = true;
            }
        }
        if !hit {
            self.attrs.push((name.into(), v));
        }
    }
    pub fn is(&self, class_re: &str) -> bool {
        self.class.eq_ignore_ascii_case(class_re)
    }
}

/// KIND of api.js: a defined type's base → web-ifc value kind.
fn kind_of(base: &str) -> u8 {
    match base {
        "s" | "x" => STRING,
        "r" | "n" => REAL,
        "i" => INTEGER,
        "b" | "l" | "e" => ENUM,
        _ => STRING,
    }
}

/// A defined type's code in this schema, else in any (`anyTypeCode`).
fn any_type_code<'s>(name: &str, schema: &'s Schema) -> Option<&'s str> {
    if let Some(c) = schema.type_code(name) {
        return Some(c);
    }
    for s in ["IFC4X3", "IFC4", "IFC2X3"] {
        if let Some(c) = schema_for(s).0.type_code(name) {
            return Some(c);
        }
    }
    None
}

fn unwrap_raw(x: &Value, base: &str) -> P {
    let logical = base == "b" || base == "l";
    match x {
        Value::List(l) => P::Arr(l.iter().map(|y| unwrap_raw(y, base)).collect()),
        Value::Enum(e) => {
            if logical {
                match &**e {
                    "T" => P::Bool(true),
                    "F" => P::Bool(false),
                    _ => P::Undef,
                }
            } else {
                P::s(e)
            }
        }
        Value::Typed(t) => unwrap_raw(&t.1, base),
        Value::Ref(_) | Value::Bin(_) | Value::Derived => P::Obj,
        Value::Null => P::Null,
        Value::Str(s) => {
            if logical {
                let l = s.lossy();
                return match &*l {
                    "T" => P::Bool(true),
                    "F" => P::Bool(false),
                    _ => P::Str(s.clone()),
                };
            }
            if matches!(base, "r" | "n" | "i") {
                let l = s.lossy();
                if !crate::js::trim(&l).is_empty() {
                    let n = str_num(&l);
                    if n.is_finite() {
                        return P::Num(n);
                    }
                }
            }
            P::Str(s.clone())
        }
        Value::Num(n) => P::Num(*n),
    }
}

/// `typedApi(name, value, schema)` on a raw value.
fn typed_api_raw(name: &str, value: &Value, schema: &Schema) -> TV {
    let code = any_type_code(name, schema).unwrap_or("s");
    let base = code.trim_start_matches('[');
    TV { ty: kind_of(base), name: Some(name.into()), v: unwrap_raw(value, base) }
}

/// `typedApi(name, value, schema)` on an API value (`CreateIfcType`): arrays mapped, enum objects / typed objects
/// unwrapped, logical text and numeric text converted.
pub fn typed_api(name: &str, value: &P, schema: &Schema) -> TV {
    let code = any_type_code(name, schema).unwrap_or("s");
    let base = code.trim_start_matches('[');
    fn un(x: &P, base: &str) -> P {
        let logical = base == "b" || base == "l";
        match x {
            P::Arr(a) => P::Arr(a.iter().map(|y| un(y, base)).collect()),
            P::Str(s) if logical => match &*s.lossy() {
                "T" => P::Bool(true),
                "F" => P::Bool(false),
                _ => x.clone(),
            },
            P::Str(s) if matches!(base, "r" | "n" | "i") => {
                let l = s.lossy();
                let n = str_num(&l);
                if !crate::js::trim(&l).is_empty() && n.is_finite() { P::Num(n) } else { x.clone() }
            }
            _ => x.clone(),
        }
    }
    TV { ty: kind_of(base), name: Some(name.into()), v: un(value, base) }
}

/// `toApi(v, t, schema)`.
pub fn to_api(v: &Value, t: &AttrCode, schema: &Schema) -> JV {
    to_api_at(v, t.agg, &t.base, t.tname.as_deref(), schema)
}

fn to_api_at(v: &Value, agg: u32, base: &str, tname: Option<&str>, schema: &Schema) -> JV {
    match v {
        Value::Null => JV::Null,
        Value::Derived => JV::Derived,
        Value::List(l) => {
            let inner = agg.saturating_sub(1);
            JV::L(l.iter().map(|x| to_api_at(x, inner, base, tname, schema)).collect())
        }
        Value::Ref(n) => JV::H(*n),
        Value::Typed(t) => JV::T(Box::new(typed_api_raw(&t.0, &t.1, schema))),
        Value::Enum(e) => {
            if base == "b" || base == "l" {
                let name = tname.unwrap_or(if base == "b" { "IFCBOOLEAN" } else { "IFCLOGICAL" });
                let v = match &**e {
                    "T" => P::Bool(true),
                    "F" => P::Bool(false),
                    _ => P::Undef,
                };
                JV::T(Box::new(TV { ty: ENUM, name: Some(name.into()), v }))
            } else {
                JV::T(Box::new(TV { ty: ENUM, name: None, v: P::s(e) }))
            }
        }
        Value::Bin(h) => JV::T(Box::new(TV { ty: STRING, name: None, v: P::s(h) })),
        Value::Str(s) => JV::T(Box::new(TV { ty: STRING, name: tname.map(Box::from), v: P::Str(s.clone()) })),
        Value::Num(n) => {
            let k = if base == "i" { INTEGER } else { REAL };
            let name = tname.unwrap_or(if k == INTEGER { "IFCINTEGER" } else { "IFCREAL" });
            JV::T(Box::new(TV { ty: k, name: Some(name.into()), v: P::Num(*n) }))
        }
    }
}

fn str_value(s: String) -> Value {
    Value::Str(JsStr::Utf8(s.into_boxed_str()))
}

/// `asRaw(x, base)` of `fromApi`.
fn as_raw(x: &P, base: &str, is_enum: bool) -> Value {
    if let P::Arr(a) = x {
        return Value::List(a.iter().map(|y| as_raw(y, base, is_enum)).collect());
    }
    if base == "b" || base == "l" || is_enum {
        let txt = x.as_str().map(|s| s.into_owned());
        match (x, txt.as_deref()) {
            (P::Bool(true), _) | (_, Some("T" | ".T.")) => return Value::Enum("T".into()),
            (P::Bool(false), _) | (_, Some("F" | ".F.")) => return Value::Enum("F".into()),
            (P::Undef | P::Null, _) | (_, Some("U" | ".U.")) => {
                return if base == "b" || base == "l" { Value::Enum("U".into()) } else { Value::Null };
            }
            _ => {}
        }
        let s = x.string();
        let s = s.strip_prefix('.').unwrap_or(&s);
        let s = s.strip_suffix('.').unwrap_or(s);
        return Value::Enum(s.into());
    }
    if matches!(base, "r" | "n" | "i") {
        let n = x.number();
        return Value::Num(if n.is_finite() { n } else { 0.0 });
    }
    match x {
        P::Undef | P::Null => Value::Null,
        P::Str(s) => Value::Str(s.clone()),
        other => str_value(other.string()),
    }
}

/// `fromApi(v, t)`: an API value → the raw value written into the file.
pub fn from_api(v: &JV, t: &AttrCode, schema: &Schema) -> R<Value> {
    from_api_at(v, t.agg, &t.base, t.tname.as_deref(), schema)
}

fn from_api_at(v: &JV, agg: u32, base: &str, tname: Option<&str>, schema: &Schema) -> R<Value> {
    Ok(match v {
        JV::Null => Value::Null,
        JV::Derived => Value::Derived,
        JV::L(l) => {
            let inner = agg.saturating_sub(1);
            let mut out = Vec::with_capacity(l.len());
            for x in l {
                out.push(from_api_at(x, inner, base, tname, schema)?);
            }
            Value::List(out.into())
        }
        JV::Num(n) => Value::Num(*n),
        JV::Str(s) => {
            if base == "e" {
                Value::Enum(s.lossy().into())
            } else {
                Value::Str(s.clone())
            }
        }
        JV::Bool(b) => Value::Enum(if *b { "T" } else { "F" }.into()),
        JV::H(n) => Value::Ref(*n),
        JV::T(tv) => {
            let name = tv.name.as_deref().filter(|n| !n.is_empty()).map(str::to_ascii_uppercase);
            let is_enum = tv.ty == ENUM;
            match name {
                None => {
                    let b = if is_enum {
                        "e"
                    } else if tv.ty == STRING || tv.ty == LABEL {
                        "s"
                    } else if tv.ty == INTEGER {
                        "i"
                    } else {
                        "r"
                    };
                    as_raw(&tv.v, b, is_enum)
                }
                Some(name) => {
                    let code = schema.type_code(&name);
                    let b = code.map_or(base, |c| c.trim_start_matches('['));
                    let raw = as_raw(&tv.v, if b == "e" { "e" } else { b }, is_enum);
                    if base == "S" || tname.is_some_and(|tn| tn != name) {
                        Value::Typed(Box::new((name.into_boxed_str(), raw)))
                    } else {
                        raw
                    }
                }
            }
        }
    })
}

/// IFC's 22-character base-64 GlobalId of 16 bytes (api.js `guidFromBytes`, new-model.js `ifcGuid`).
pub fn guid_from_bytes(b: &[u8; 16]) -> String {
    const B64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let acc = u128::from_be_bytes(*b);
    let mut out = String::with_capacity(22);
    out.push(char::from(B64.get(usize::from(b[0] >> 6)).copied().unwrap_or(b'0')));
    for i in (0..21u32).rev() {
        let d = usize::try_from(acc.checked_shr(i.saturating_mul(6)).unwrap_or(0) & 63).unwrap_or(0);
        out.push(char::from(B64.get(d).copied().unwrap_or(b'0')));
    }
    out
}

/// The bytes a GlobalId is made of: the caller's seed (mulberry32, one byte per draw — the parity harness gives the JS
/// `crypto.getRandomValues` the same stream), so a session with the same seed makes the same ids.
#[derive(Clone, Debug)]
pub struct Rand {
    pub state: u32,
}

impl Rand {
    pub fn new(seed: u32) -> Rand {
        Rand { state: seed }
    }
    /// mulberry32
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let a = self.state;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        t ^ (t >> 14)
    }
    pub fn fill(&mut self, b: &mut [u8]) {
        for x in b {
            *x = u8::try_from(self.next_u32() & 255).unwrap_or(0);
        }
    }
    /// A version-4 GlobalId (api.js `newGuid`).
    pub fn guid(&mut self) -> String {
        let mut b = [0u8; 16];
        self.fill(&mut b);
        b[6] = (b[6] & 0x0F) | 0x40;
        b[8] = (b[8] & 0x3F) | 0x80;
        guid_from_bytes(&b)
    }
}

/// A 4×4 column-major matrix.
pub type M4 = [f64; 16];

/// The open model the editor writes (api.js on one model id).
pub struct Api {
    pub f: IfcFile<'static>,
    pub rand: Rand,
    /// world matrices of placements at the file version they were read (api.js drops its geometry on every write)
    places: HashMap<u64, M4>,
    places_version: u64,
}

impl std::fmt::Debug for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Api").field("file", &self.f).finish()
    }
}

impl Api {
    /// `api.OpenModel(bytes)` (the bytes are kept: untouched lines are saved from them).
    pub fn open(bytes: Arc<[u8]>, seed: u32) -> R<Api> {
        let f = IfcFile::open_owned(bytes).map_err(|e| JsErr(e.0))?;
        Ok(Api { f, rand: Rand::new(seed), places: HashMap::new(), places_version: u64::MAX })
    }

    pub fn schema(&self) -> &'static Schema {
        self.f.schema
    }
    /// `api.GetModelSchema(mid)`
    pub fn schema_name(&self) -> &'static str {
        self.f.schema_name
    }

    /// `GetLine(mid, id, false)`: Ok(None) = null (no such line, or a complex instance).
    pub fn line(&self, id: Id) -> R<Option<Line>> {
        let Some(up) = self.f.ty(id) else { return Ok(None) };
        if up.is_empty() {
            return Ok(None);
        }
        let params = self.f.params(id).map_err(|e| JsErr(e.0))?.unwrap_or(&[]);
        let schema = self.f.schema;
        Ok(Some(match schema.entity(up) {
            Some(ent) => Line {
                id,
                up: up.into(),
                class: ent.name.clone(),
                attrs: ent
                    .attrs
                    .iter()
                    .enumerate()
                    .map(|(k, a)| (a.name.clone(), params.get(k).map_or(JV::Null, |v| to_api(v, &a.t, schema))))
                    .collect(),
            },
            None => {
                let t = cc_io_ifc::schema::parse_attr_code("S");
                Line {
                    id,
                    up: up.into(),
                    class: up.into(),
                    attrs: params.iter().enumerate().map(|(k, v)| (format!("Arg{k}").into(), to_api(v, &t, schema))).collect(),
                }
            }
        }))
    }

    /// `GetLine`, a missing line as a TypeError (the JS reads an attribute of `null`).
    pub fn must(&self, id: Id) -> R<Line> {
        self.line(id)?.ok_or_else(|| JsErr(format!("TypeError: Cannot read properties of null (line #{})", num_str(id))))
    }

    /// `CreateIfcEntity(mid, code, ...args)` (expressID −1 until written).
    pub fn create(&self, up: &str, args: Vec<JV>) -> R<Line> {
        let ent = self.f.schema.entity(up).ok_or_else(|| JsErr(format!("Unknown IFC entity code {up} in {}", self.f.schema_name)))?;
        let mut it = args.into_iter();
        Ok(Line {
            id: -1.0,
            up: up.into(),
            class: ent.name.clone(),
            attrs: ent.attrs.iter().map(|a| (a.name.clone(), it.next().unwrap_or(JV::Null))).collect(),
        })
    }

    /// `CreateIfcType(mid, code, value)`
    pub fn create_type(&self, name: &str, v: P) -> JV {
        JV::T(Box::new(typed_api(name, &v, self.f.schema)))
    }

    /// `CreateIFCGloballyUniqueId`
    pub fn new_guid(&mut self) -> JV {
        JV::T(Box::new(TV { ty: STRING, name: Some("IFCGLOBALLYUNIQUEID".into()), v: P::s(&self.rand.guid()) }))
    }

    /// `WriteLine(mid, line)`: a line without an id gets the next free one (written back into `line.id`).
    pub fn write(&mut self, line: &mut Line) -> R<f64> {
        let schema = self.f.schema;
        let ent = schema.entity(&line.up).ok_or_else(|| JsErr(format!("Cannot write a line of unknown type {}", line.up)))?;
        if !(line.id > 0.0) {
            line.id = self.f.max_id + 1.0;
        }
        let mut params = Vec::with_capacity(ent.attrs.len());
        for a in &ent.attrs {
            params.push(from_api(line.a(&a.name), &a.t, schema)?);
        }
        self.f.set(line.id, &line.up, params).map_err(|e| JsErr(e.0))?;
        Ok(line.id)
    }

    /// ifc-util.js `deleteLine` (api.DeleteLine).
    pub fn delete(&mut self, id: Id) {
        self.f.remove(id);
    }

    /// ifc-util.js `ids(api, mid, name, inherited)`.
    pub fn ids(&self, up: &str, inherited: bool) -> Vec<Id> {
        self.f.of_type(up, inherited)
    }

    /// `GetWorldTransformMatrix(mid, placementId)`: file units, Z up.
    pub fn world(&mut self, placement: Id) -> R<M4> {
        if self.places_version != self.f.version {
            self.places.clear();
            self.places_version = self.f.version;
        }
        if let Some(m) = self.places.get(&id_key(placement)) {
            return Ok(*m);
        }
        let mut g = cc_io_ifc::geo::ctx::GeoCtx::new(&self.f).map_err(|e| JsErr(e.0.to_string()))?;
        let m = g.placement(Some(placement), 0).map_err(|e| JsErr(e.0.to_string()))?;
        self.places.insert(id_key(placement), m);
        Ok(m)
    }

    /// ifc-util.js `retypeLine`: every typed value made anew from its plain value (a no-op on cc-ifc's own values, kept
    /// for the cases where it is not).
    pub fn retype(&self, line: &mut Line) {
        fn fix(v: &mut JV, schema: &Schema) {
            match v {
                JV::L(l) => l.iter_mut().for_each(|x| fix(x, schema)),
                JV::T(t) if t.ty != ENUM && t.ty != REF => {
                    if let Some(name) = t.name.clone()
                        && let Some(up) = known_type_name(&name)
                    {
                        **t = typed_api(&up, &t.v, schema);
                    }
                }
                _ => {}
            }
        }
        for (_, v) in &mut line.attrs {
            fix(v, self.f.schema);
        }
    }

    /// ifc-util.js `lengthUnitMm`: mm per file length unit (project units; SI metre with prefix, or a conversion unit).
    pub fn length_unit_mm(&mut self) -> R<f64> {
        let proj = self.ids("IFCPROJECT", false).first().copied();
        let mut assign: Option<Id> = None;
        if let Some(p) = proj {
            assign = self.must(p)?.a("UnitsInContext").href();
        }
        if assign.is_none() {
            assign = self.ids("IFCUNITASSIGNMENT", false).first().copied();
        }
        let Some(a) = assign else { return Ok(1000.0) };
        let Some(al) = self.line(a)? else { return Ok(1000.0) };
        for uid in al.a("Units").hrefs() {
            let Some(u) = self.line(uid)? else { continue };
            if u.a("UnitType").val_str().as_deref() != Some("LENGTHUNIT") {
                continue;
            }
            return self.unit_mm(&u, 0);
        }
        Ok(1000.0)
    }

    /// ifc-util.js `unitMm(u)` on a flattened unit line.
    fn unit_mm(&self, u: &Line, depth: u32) -> R<f64> {
        const PREFIX: [(&str, f64); 13] = [
            ("EXA", 1e21),
            ("PETA", 1e18),
            ("TERA", 1e15),
            ("GIGA", 1e12),
            ("MEGA", 1e9),
            ("KILO", 1e6),
            ("HECTO", 1e5),
            ("DECA", 1e4),
            ("DECI", 100.0),
            ("CENTI", 10.0),
            ("MILLI", 1.0),
            ("MICRO", 1e-3),
            ("NANO", 1e-6),
        ];
        let conv = u.get("ConversionFactor").filter(|v| v.truthy());
        if u.get("Name").is_some_and(JV::truthy) && u.a("Name").val_str().as_deref() == Some("METRE") && conv.is_none() {
            let pre = u.a("Prefix").val();
            if !pre.truthy() {
                return Ok(1000.0);
            }
            let p = pre.string();
            return Ok(PREFIX.iter().find(|(k, _)| *k == p).map_or(1000.0, |(_, v)| *v));
        }
        if let Some(c) = conv {
            let Some(fid) = c.href() else { return Ok(1000.0) };
            let Some(f) = self.line(fid)? else { return Ok(1000.0) };
            let v = f.a("ValueComponent").val().number();
            let base = match f.a("UnitComponent").href() {
                Some(uc) if depth < 16 => match self.line(uc)? {
                    Some(ul) => self.unit_mm(&ul, depth.saturating_add(1))?,
                    None => 1000.0,
                },
                _ => 1000.0,
            };
            if v.is_finite() && v > 0.0 {
                return Ok(v * base);
            }
            let n = u.a("Name").val();
            let n = if n.truthy() { n.string().to_uppercase() } else { String::new() };
            return Ok(if n.contains("FOOT") {
                304.8
            } else if n.contains("INCH") {
                25.4
            } else {
                1000.0
            });
        }
        Ok(1000.0)
    }
}

/// `nameOfCode(typeCode(name))` for a defined type or entity name: the upper-case name when any schema registers it.
fn known_type_name(name: &str) -> Option<String> {
    let up = name.to_ascii_uppercase();
    ["IFC2X3", "IFC4", "IFC4X3"]
        .iter()
        .any(|s| {
            let sc = schema_for(s).0;
            sc.has(&up) || sc.type_code(&up).is_some()
        })
        .then_some(up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_as_the_js_encodes_them() {
        assert_eq!(guid_from_bytes(&[0u8; 16]), "0000000000000000000000");
        assert_eq!(guid_from_bytes(&[255u8; 16]), "3$$$$$$$$$$$$$$$$$$$$$");
        let mut r = Rand::new(1);
        let g = r.guid();
        assert_eq!(g.len(), 22);
        // mulberry32(1)'s first draw, as the JS computes it
        assert_eq!(Rand::new(1).next_u32(), 2_693_262_067);
    }

    #[test]
    fn a_line_written_back_unchanged_keeps_its_values() {
        let text = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',$,'W',$,$,$,$,$,.STANDARD.);\n#2=IFCPROPERTYSINGLEVALUE('A',$,IFCBOOLEAN(.T.),$);\nENDSEC;\nEND-ISO-10303-21;\n";
        let mut api = Api::open(Arc::from(text.as_bytes()), 7).unwrap();
        for id in [1.0, 2.0] {
            let mut l = api.must(id).unwrap();
            api.write(&mut l).unwrap();
        }
        let out = String::from_utf8(api.f.save(None).unwrap()).unwrap();
        assert!(out.contains("#1=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',$,'W',$,$,$,$,$,.STANDARD.);"), "{out}");
        assert!(out.contains("#2=IFCPROPERTYSINGLEVALUE('A',$,IFCBOOLEAN(.T.),$);"), "{out}");
        let mut w = api.must(1.0).unwrap();
        w.set("Name", api.create_type("IFCLABEL", P::s("Walk-in cooler")));
        api.write(&mut w).unwrap();
        let mut p = api.create("IFCCARTESIANPOINT", vec![JV::L(vec![JV::Num(1.0), JV::Num(2.5)])]).unwrap();
        assert_eq!(api.write(&mut p).unwrap(), 3.0);
        let out = String::from_utf8(api.f.save(None).unwrap()).unwrap();
        assert!(out.contains("#1=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',$,'Walk-in cooler',$,$,$,$,$,.STANDARD.);"), "{out}");
        assert!(out.contains("#3=IFCCARTESIANPOINT((1.,2.5));"), "{out}");
    }
}
