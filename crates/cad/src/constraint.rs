//! A constraint (gcons.js) as the drawing keeps it (gcons.js: a plain object `{ id, type, refs: [{ e, k }], …type fields }`,
//! its keys in the order the JS made them, so files and snapshots spell it the same) and a typed view for reading.

use serde_json::{Map, Value, json};

/// `{ e: entityId, k }`
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    pub e: u64,
    pub k: String,
}

/// A constraint: the object (key order kept) plus its id, type and references read out.
#[derive(Clone, Debug, PartialEq)]
pub struct Con {
    pub id: String,
    pub typ: String,
    /// shared: copying a constraint copies no references
    pub refs: std::sync::Arc<Vec<Ref>>,
    /// the whole object, `refs` included (shared: copying a list of constraints copies no maps)
    pub obj: std::sync::Arc<Map<String, Value>>,
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

impl Con {
    /// The typed view of a stored constraint object (None when it is not one: no id / type / refs).
    pub fn from_value(v: &Value) -> Option<Con> {
        let obj = v.as_object()?.clone();
        let id = obj.get("id")?.as_str()?.to_string();
        let typ = obj.get("type")?.as_str()?.to_string();
        let refs = obj
            .get("refs")?
            .as_array()?
            .iter()
            .map(|r| Some(Ref { e: r.get("e")?.as_u64()?, k: r.get("k")?.as_str()?.to_string() }))
            .collect::<Option<Vec<_>>>()?;
        Some(Con { id, typ, refs: std::sync::Arc::new(refs), obj: std::sync::Arc::new(obj) })
    }
    /// A new constraint object `{ id, type, refs }` (more keys come after, as the JS adds them).
    pub fn new(id: &str, typ: &str, refs: Vec<Ref>) -> Con {
        let mut obj = Map::new();
        obj.insert("id".into(), Value::String(id.into()));
        obj.insert("type".into(), Value::String(typ.into()));
        obj.insert("refs".into(), refs_value(&refs));
        Con { id: id.into(), typ: typ.into(), refs: std::sync::Arc::new(refs), obj: std::sync::Arc::new(obj) }
    }
    pub fn to_value(&self) -> Value {
        Value::Object((*self.obj).clone())
    }
    /// The object to write to (copied first when it is shared).
    pub fn obj_mut(&mut self) -> &mut Map<String, Value> {
        std::sync::Arc::make_mut(&mut self.obj)
    }
    /// `c[k] = v` (a key already there keeps its place; a new one goes last)
    pub fn set(&mut self, k: &str, v: Value) {
        match k {
            "id" => self.id = v.as_str().unwrap_or_default().to_string(),
            "type" => self.typ = v.as_str().unwrap_or_default().to_string(),
            _ => {}
        }
        self.obj_mut().insert(k.into(), v);
    }
    /// `delete c[k]`
    pub fn remove(&mut self, k: &str) {
        self.obj_mut().shift_remove(k);
    }
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.obj.get(k)
    }
    pub fn set_refs(&mut self, refs: Vec<Ref>) {
        self.obj_mut().insert("refs".into(), refs_value(&refs));
        self.refs = std::sync::Arc::new(refs);
    }
    pub fn set_id(&mut self, id: &str) {
        self.id = id.into();
        self.obj_mut().insert("id".into(), Value::String(id.into()));
    }
    /// `c.sign || 1`
    pub fn sign_or_1(&self) -> f64 {
        match num(self.obj.get("sign")) {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        }
    }
    pub fn sign(&self) -> Option<f64> {
        num(self.obj.get("sign"))
    }
    /// `c.kind === 'int'`
    pub fn kind_int(&self) -> bool {
        self.obj.get("kind").and_then(Value::as_str) == Some("int")
    }
    pub fn pair(&self) -> Option<i64> {
        self.obj.get("pair").and_then(Value::as_i64)
    }
    pub fn dirs(&self) -> Option<[f64; 2]> {
        let a = self.obj.get("dirs")?.as_array()?;
        Some([num(a.first())?, num(a.get(1))?])
    }
    /// a dimension's value (`c.value`; NaN when it is not a number)
    pub fn value_num(&self) -> f64 {
        num(self.obj.get("value")).unwrap_or(f64::NAN)
    }
    /// a fixed point's `value: [x, y]`
    pub fn value_pair(&self) -> Option<[f64; 2]> {
        let a = self.obj.get("value")?.as_array()?;
        Some([num(a.first()).unwrap_or(f64::NAN), num(a.get(1)).unwrap_or(f64::NAN)])
    }
    /// `typeof c.fixed[f] === 'number'`
    pub fn fixed_field(&self, f: &str) -> Option<f64> {
        num(self.obj.get("fixed")?.get(f))
    }
    pub fn name(&self) -> Option<&str> {
        self.obj.get("name").and_then(Value::as_str)
    }
    pub fn off(&self) -> [f64; 2] {
        match self.obj.get("off").and_then(Value::as_array) {
            Some(a) => [num(a.first()).unwrap_or(f64::NAN), num(a.get(1)).unwrap_or(f64::NAN)],
            None => [0.0, 0.0],
        }
    }
}

pub fn refs_value(refs: &[Ref]) -> Value {
    Value::Array(refs.iter().map(|r| json!({ "e": r.e, "k": r.k })).collect())
}
