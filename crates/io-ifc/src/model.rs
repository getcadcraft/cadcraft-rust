//! model.js — one open IFC file: the bytes, the instance index (step.rs `scan_step`), lines parsed on first read,
//! attributes by name, ids by type, inverse references, and (write.rs) edits kept beside the original bytes and saving:
//! untouched lines copied byte for byte, written ones formatted anew. Every read sees the edits, as model.js does.
//!
//! Ids are JS numbers: a line's id is what the scan's `Int32Array` holds; a reference `#n` may be any size and finds
//! a line only where the JS lookup would (`slot[id]` when the largest id is small, else a Map of the wrapped ids).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::schema::{Schema, schema_for};
use crate::step::{Part, StepError, StepIndex, parse_complex, parse_params, scan_step};
use crate::util::{f64_index, usize_f64};
use crate::value::Value;

/// A line id (a JS number).
pub type Id = f64;

/// The key of an id in a hash map (the double's bits; ids are never −0).
#[inline]
pub fn id_key(id: Id) -> u64 {
    if id == 0.0 { 0 } else { id.to_bits() }
}

enum Lookup {
    Slot(Vec<i32>),
    Map(HashMap<i32, usize>),
}

pub type Parsed = Result<Box<[Value]>, Box<str>>;
/// element id → relation ids
pub type Inverse = Arc<HashMap<u64, Vec<Id>>>;

/// Where the bytes live: borrowed from the caller, or owned (an editable model the app keeps).
enum Src<'a> {
    Borrowed(&'a [u8]),
    Owned(Arc<[u8]>),
}

/// A line written over the file (new, or replacing an original one; the type may change).
#[derive(Clone, Debug)]
pub struct Edit {
    pub id: Id,
    pub ty: Box<str>,
    pub params: Box<[Value]>,
}

/// One open file.
pub struct IfcFile<'a> {
    src: Src<'a>,
    pub ix: StepIndex,
    pub schema: &'static Schema,
    pub schema_name: &'static str,
    pub guessed: bool,
    pub max_id: f64,
    lookup: Lookup,
    /// the entity of each type name of the index (by name number)
    ents: Vec<Option<&'static crate::schema::Entity>>,
    parsed: Vec<OnceLock<Parsed>>,
    by_type: OnceLock<HashMap<Box<str>, Vec<Id>>>,
    inv: Mutex<HashMap<String, Inverse>>,
    /// written lines (new or replacing), by id key
    pub(crate) edits: HashMap<u64, Edit>,
    /// removed original ids (keys)
    pub(crate) gone: HashSet<u64>,
    /// bumps on every write
    pub version: u64,
}

impl std::fmt::Debug for IfcFile<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IfcFile").field("count", &self.ix.count).field("schema", &self.schema_name).finish()
    }
}

impl<'a> IfcFile<'a> {
    /// `IfcFile.open(bytes)`.
    pub fn open(bytes: &'a [u8]) -> Result<IfcFile<'a>, StepError> {
        let ix = scan_step(bytes)?;
        Ok(IfcFile::new(bytes, ix))
    }

    pub fn new(bytes: &'a [u8], ix: StepIndex) -> IfcFile<'a> {
        IfcFile::with_src(Src::Borrowed(bytes), ix)
    }

    /// The bytes of the file as it was opened.
    pub fn bytes(&self) -> &[u8] {
        match &self.src {
            Src::Borrowed(b) => b,
            Src::Owned(b) => b,
        }
    }

    fn with_src(src: Src<'a>, ix: StepIndex) -> IfcFile<'a> {
        let (schema, guessed) = schema_for(&ix.schema.lossy());
        let n = ix.count;
        let lookup = if ix.max_id <= usize_f64(n) * 4.0 + 1_000_000.0 {
            let len = f64_index(ix.max_id, usize::MAX).map_or(0, |m| m.saturating_add(1));
            let mut slot = vec![-1i32; len];
            for (i, &id) in ix.ids.iter().enumerate() {
                if let Ok(k) = usize::try_from(id)
                    && let Some(s) = slot.get_mut(k)
                {
                    *s = i32::try_from(i).unwrap_or(-1);
                }
            }
            Lookup::Slot(slot)
        } else {
            let mut m = HashMap::with_capacity(n);
            for (i, &id) in ix.ids.iter().enumerate() {
                m.insert(id, i);
            }
            Lookup::Map(m)
        };
        let parsed = (0..n).map(|_| OnceLock::new()).collect();
        let ents = ix.names.iter().map(|t| schema.entity(t)).collect();
        IfcFile {
            src,
            max_id: ix.max_id,
            ix,
            schema,
            schema_name: schema.name,
            guessed,
            lookup,
            ents,
            parsed,
            by_type: OnceLock::new(),
            inv: Mutex::new(HashMap::new()),
            edits: HashMap::new(),
            gone: HashSet::new(),
            version: 0,
        }
    }

    /// The record index of an id.
    pub fn rec(&self, id: Id) -> Option<usize> {
        match &self.lookup {
            Lookup::Slot(s) => {
                // ids are whole numbers below 2^31 here: an exact cast, checked by the round trip
                if !(0.0..2_147_483_648.0).contains(&id) {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let k = id as u32;
                if f64::from(k) != id {
                    return None;
                }
                usize::try_from(*s.get(usize::try_from(k).ok()?)?).ok()
            }
            Lookup::Map(m) => {
                if id.fract() != 0.0 || !(-2_147_483_648.0..=2_147_483_647.0).contains(&id) {
                    return None;
                }
                let v = crate::util::to_int32(id);
                m.get(&v).copied()
            }
        }
    }

    pub fn has(&self, id: Id) -> bool {
        self.ty(id).is_some()
    }

    /// 'IFCWALL' (upper case), "" for a complex instance, None when there is no such line.
    pub fn ty(&self, id: Id) -> Option<&str> {
        if !self.edits.is_empty() || !self.gone.is_empty() {
            let k = id_key(id);
            if self.gone.contains(&k) {
                return None;
            }
            if let Some(e) = self.edits.get(&k) {
                return Some(&e.ty);
            }
        }
        let r = self.rec(id)?;
        Some(self.ix.type_name(r))
    }

    /// The written line at an id, if any (None also when the id was removed).
    pub fn edit(&self, id: Id) -> Option<&Edit> {
        self.edits.get(&id_key(id))
    }

    /// Was this original line removed?
    pub fn is_gone(&self, id: Id) -> bool {
        self.gone.contains(&id_key(id))
    }

    /// Parse every plain line now (natively in parallel); reads then never parse.
    pub fn parse_all(&self) {
        let one = |r: usize| {
            if !self.ix.type_name(r).is_empty() {
                let _ = self.parsed_rec(r);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            (0..self.ix.count).into_par_iter().with_min_len(512).for_each(one);
        }
        #[cfg(target_arch = "wasm32")]
        for r in 0..self.ix.count {
            one(r);
        }
    }

    fn parsed_rec(&self, r: usize) -> Option<&Parsed> {
        let cell = self.parsed.get(r)?;
        Some(cell.get_or_init(|| {
            let (Some(&s), Some(&e)) = (self.ix.starts.get(r), self.ix.ends.get(r)) else { return Err("no such record".into()) };
            parse_params(self.bytes(), s, e).map(Vec::into_boxed_slice).map_err(|e| e.0.into_boxed_str())
        }))
    }

    /// The raw parameter list; Ok(None) when there is no such line or it is complex; Err = the parse error (thrown in JS).
    pub fn params(&self, id: Id) -> Result<Option<&[Value]>, StepError> {
        if !self.edits.is_empty() || !self.gone.is_empty() {
            let k = id_key(id);
            if self.gone.contains(&k) {
                return Ok(None);
            }
            if let Some(e) = self.edits.get(&k) {
                return Ok(Some(&e.params));
            }
        }
        let Some(r) = self.rec(id) else { return Ok(None) };
        if self.ix.types.get(r).copied().unwrap_or(0) == 0 {
            return Ok(None); // complex: use complex(id)
        }
        match self.parsed_rec(r) {
            Some(Ok(p)) => Ok(Some(p)),
            Some(Err(e)) => Err(StepError(e.to_string())),
            None => Ok(None),
        }
    }

    /// The parts of a complex instance; None for a plain one.
    pub fn complex(&self, id: Id) -> Result<Option<Vec<Part>>, StepError> {
        if self.gone.contains(&id_key(id)) || self.edits.contains_key(&id_key(id)) {
            return Ok(None);
        }
        let Some(r) = self.rec(id) else { return Ok(None) };
        if self.ix.types.get(r).copied().unwrap_or(0) != 0 {
            return Ok(None);
        }
        let (Some(&s), Some(&e)) = (self.ix.starts.get(r), self.ix.ends.get(r)) else { return Ok(None) };
        parse_complex(self.bytes(), s, e).map(Some)
    }

    /// Attribute value of a line by name (raw): Ok(None) = undefined.
    pub fn attr(&self, id: Id, name: &str) -> Result<Option<&Value>, StepError> {
        if !self.edits.is_empty() || !self.gone.is_empty() {
            let k = id_key(id);
            if self.gone.contains(&k) {
                return Ok(None);
            }
            if let Some(e) = self.edits.get(&k) {
                let Some(ent) = self.schema.entity(&e.ty) else { return Ok(None) };
                let Some(i) = ent.attr_index(name) else { return Ok(None) };
                return Ok(e.params.get(i));
            }
        }
        let Some(r) = self.rec(id) else { return Ok(None) };
        let t = usize::from(self.ix.types.get(r).copied().unwrap_or(0));
        let Some(e) = self.ents.get(t).copied().flatten() else { return Ok(None) };
        let Some(k) = e.attr_index(name) else { return Ok(None) };
        if t == 0 {
            return Ok(None);
        }
        match self.parsed_rec(r) {
            Some(Ok(p)) => Ok(p.get(k)),
            Some(Err(e)) => Err(StepError(e.to_string())),
            None => Ok(None),
        }
    }

    /// The parsed lines, to drop where it costs nothing (another thread).
    pub fn take_parsed(&mut self) -> Vec<OnceLock<Parsed>> {
        std::mem::take(&mut self.parsed)
    }

    fn index_types(&self) -> &HashMap<Box<str>, Vec<Id>> {
        self.by_type.get_or_init(|| {
            let ix = &self.ix;
            let mut lists: Vec<Vec<Id>> = vec![Vec::new(); ix.names.len()];
            for (i, &t) in ix.types.iter().enumerate() {
                if let (Some(l), Some(&id)) = (lists.get_mut(usize::from(t)), ix.ids.get(i)) {
                    l.push(f64::from(id));
                }
            }
            let mut m = HashMap::with_capacity(ix.names.len());
            for (t, n) in ix.names.iter().enumerate() {
                let l = lists.get_mut(t).map(std::mem::take).unwrap_or_default();
                m.insert(n.clone(), l);
            }
            m
        })
    }

    /// Ids of one entity type (upper case); inherited = its subtypes too. File order within each type (sorted when
    /// several types are merged).
    pub fn of_type(&self, up: &str, inherited: bool) -> Vec<Id> {
        let by_type = self.index_types();
        let edited = !self.edits.is_empty() || !self.gone.is_empty();
        if !inherited && !edited {
            return by_type.get(up).cloned().unwrap_or_default();
        }
        let types: std::borrow::Cow<'_, [Box<str>]> =
            if inherited { self.schema.subtypes(up) } else { std::borrow::Cow::Owned(vec![Box::from(up)]) };
        let mut out = Vec::new();
        for t in types.iter() {
            if let Some(l) = by_type.get(t) {
                if edited {
                    out.extend(l.iter().copied().filter(|id| {
                        let k = id_key(*id);
                        !self.gone.contains(&k) && !self.edits.contains_key(&k)
                    }));
                } else {
                    out.extend_from_slice(l);
                }
            }
        }
        if !self.edits.is_empty() {
            for e in self.edits.values() {
                if types.iter().any(|t| **t == *e.ty) {
                    out.push(e.id);
                }
            }
        }
        if types.len() > 1 || !self.edits.is_empty() {
            out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        }
        out
    }

    /// Relations of one entity type keyed by the target of one attribute: inverse('IFCRELVOIDSELEMENT',
    /// 'RelatingBuildingElement') → element id → [relation ids]. Cached.
    pub fn inverse(&self, up: &str, attr: &str) -> Result<std::sync::Arc<HashMap<u64, Vec<Id>>>, StepError> {
        let key = format!("{up}.{attr}+");
        if let Some(m) = self.inv.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return Ok(m.clone());
        }
        let mut m: HashMap<u64, Vec<Id>> = HashMap::new();
        for id in self.of_type(up, true) {
            let v = self.attr(id, attr)?;
            let mut add = |x: &Value| {
                if let Value::Ref(r) = x {
                    m.entry(id_key(*r)).or_default().push(id);
                }
            };
            match v {
                Some(Value::List(l)) => l.iter().for_each(&mut add),
                Some(x) => add(x),
                None => {}
            }
        }
        let m = std::sync::Arc::new(m);
        self.inv.lock().unwrap_or_else(PoisonError::into_inner).insert(key, m.clone());
        Ok(m)
    }
}

impl IfcFile<'static> {
    /// `IfcFile.open(bytes)` on bytes the file keeps (an editable model the app holds on to).
    pub fn open_owned(bytes: Arc<[u8]>) -> Result<IfcFile<'static>, StepError> {
        let ix = scan_step(&bytes)?;
        Ok(IfcFile::with_src(Src::Owned(bytes), ix))
    }
}

impl IfcFile<'_> {
    /// Forget the cached inverse indexes (after a write).
    pub(crate) fn clear_inverse(&mut self) {
        self.inv.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
    }
}
