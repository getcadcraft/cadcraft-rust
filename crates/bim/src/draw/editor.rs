//! ifc-edit.js — edits an open IFC model and saves it as a new IFC file: attributes (Name, Description, ObjectType,
//! Tag), single property values (copy-on-write where exporters share sets and values), and building-tool changes
//! (model-ops.js), each one change with undo and redo. Library placements, pastes and deletes of other elements are
//! not ported yet (their parity lives with the library work).

use std::collections::HashMap;
use std::sync::Arc;

use cc_io_ifc::model::{Id, id_key};

use crate::api::{JsErr, R};
use crate::draw::ops::{Op, OpResult};
use crate::draw::tr::t0;
use crate::draw::writer::{Journal, undo_journal};
use crate::wapi::{Api, JV, P, TV};

pub const EDITABLE_ATTRS: [&str; 4] = ["Name", "Description", "ObjectType", "Tag"];

fn attr_type(a: &str) -> &'static str {
    match a {
        "Description" => "IFCTEXT",
        "Tag" => "IFCIDENTIFIER",
        _ => "IFCLABEL",
    }
}

/// What ifc-edit.js needs to change a property value (ifc-model.js properties() row `edit`).
#[derive(Clone, Debug, PartialEq)]
pub struct PropRef {
    pub pset_id: Id,
    pub prop_id: Id,
    pub from_type: bool,
    pub set_name: String,
    pub prop_name: String,
    pub value_type: Option<String>,
}

/// One change in the list.
#[derive(Clone, Debug)]
pub enum Change {
    Attr { id: Id, attr: String, old: Option<P>, value: Option<String>, label: String },
    Prop { id: Id, r: PropRef, old: P, value: P, label: String },
    Model { id: Option<Id>, ids: Vec<Id>, made: Vec<(String, Id)>, label: String, verb: String, journal: Box<Journal>, op: Box<Op> },
}

impl Change {
    pub fn label(&self) -> &str {
        match self {
            Change::Attr { label, .. } | Change::Prop { label, .. } | Change::Model { label, .. } => label,
        }
    }
    pub fn id(&self) -> Option<Id> {
        match self {
            Change::Attr { id, .. } | Change::Prop { id, .. } => Some(*id),
            Change::Model { id, .. } => *id,
        }
    }
}

/// The editor of one open model.
pub struct Editor {
    pub api: Api,
    pub changes: Vec<Change>,
    pub undone: Vec<Change>,
    moved: HashMap<u64, Id>,
    prop_refs: Option<HashMap<u64, usize>>,
    rels_by_pset: Option<HashMap<u64, Vec<Id>>>,
    /// a new building, unsaved until Save as IFC writes it
    pub is_new: bool,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor").field("changes", &self.changes.len()).field("undone", &self.undone.len()).finish()
    }
}

/// `t(key)` as an error (ifc-edit.js throws these to the page).
fn terr(key: &str) -> JsErr {
    JsErr(t0(key))
}

impl Editor {
    pub fn open(bytes: Arc<[u8]>, seed: u32) -> R<Editor> {
        Ok(Editor {
            api: Api::open(bytes, seed)?,
            changes: Vec::new(),
            undone: Vec::new(),
            moved: HashMap::new(),
            prop_refs: None,
            rels_by_pset: None,
            is_new: false,
        })
    }
    pub fn dirty(&self) -> bool {
        !self.changes.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
    fn record(&mut self, c: Change) {
        self.changes.push(c);
        self.undone.clear();
    }
    /// The last change's label (bimcraft.js `tagLast`).
    pub fn tag_last(&mut self, label: &str) {
        if let Some(c) = self.changes.last_mut() {
            match c {
                Change::Attr { label: l, .. } | Change::Prop { label: l, .. } | Change::Model { label: l, .. } => *l = label.to_string(),
            }
        }
    }

    /// `setAttribute(id, attr, value)` → whether it changed.
    pub fn set_attribute(&mut self, id: Id, attr: &str, value: Option<&str>, record: bool) -> R<bool> {
        if !EDITABLE_ATTRS.contains(&attr) {
            return Err(JsErr(format!("{attr} cannot be edited.")));
        }
        let mut line = self.api.must(id)?;
        let cur = line.a(attr).clone();
        let old = match &cur {
            JV::Null => None,
            other => match other.val() {
                P::Undef | P::Null => None,
                v => Some(v),
            },
        };
        let next = match value {
            Some(v) if !crate::js::trim(v).is_empty() => Some(v.to_string()),
            _ => None,
        };
        let same = match (&next, &old) {
            (None, None) => true,
            (Some(n), Some(P::Str(o))) => o.lossy() == n.as_str(),
            _ => false,
        };
        if same {
            return Ok(false);
        }
        match &next {
            None => line.set(attr, JV::Null),
            Some(n) => match cur {
                JV::T(mut t) => {
                    t.v = P::s(n);
                    line.set(attr, JV::T(t));
                }
                JV::Null => line.set(attr, self.api.create_type(attr_type(attr), P::s(n))),
                _ => line.set(attr, self.api.create_type(attr_type(attr), P::s(n))),
            },
        }
        self.api.write(&mut line)?;
        if record {
            self.record(Change::Attr { id, attr: attr.to_string(), old, value: next, label: String::new() });
        }
        Ok(true)
    }

    fn typed_value(&self, current: &TV, raw: &str) -> R<JV> {
        let name = current.name.as_deref().unwrap_or("");
        let value = if name == "IFCBOOLEAN" || name == "IFCLOGICAL" {
            let s = crate::js::trim(raw).to_lowercase();
            if ["yes", "true", "t", "1"].contains(&s.as_str()) {
                P::Bool(true)
            } else if ["no", "false", "f", "0"].contains(&s.as_str()) {
                P::Bool(false)
            } else {
                return Err(terr("ifcedit.err.yesno"));
            }
        } else if ["REAL", "INTEGER", "MEASURE", "NUMERIC", "COUNT", "RATIO", "TIMESTAMP"].iter().any(|k| name.contains(k)) {
            let t = crate::js::trim(raw);
            let v = crate::js::str_num(t);
            if !v.is_finite() || t.is_empty() {
                return Err(terr("ifcedit.err.number"));
            }
            if (name.contains("INTEGER") || name.contains("COUNT")) && v.fract() != 0.0 {
                return Err(terr("ifcedit.err.whole"));
            }
            P::Num(v)
        } else {
            P::s(raw)
        };
        let known = ["IFC2X3", "IFC4", "IFC4X3"].iter().any(|s| {
            let sc = cc_io_ifc::schema::schema_for(s).0;
            !name.is_empty() && (sc.has(name) || sc.type_code(name).is_some())
        });
        Ok(if known {
            self.api.create_type(name, value)
        } else {
            JV::T(Box::new(TV { ty: current.ty, name: current.name.clone(), v: value }))
        })
    }

    fn index(&mut self) -> R<()> {
        if self.prop_refs.is_some() {
            return Ok(());
        }
        let mut refs: HashMap<u64, usize> = HashMap::new();
        for ps in self.api.ids("IFCPROPERTYSET", false) {
            for h in self.api.must(ps)?.a("HasProperties").hrefs() {
                let e = refs.entry(id_key(h)).or_insert(0);
                *e = e.saturating_add(1);
            }
        }
        let mut rels: HashMap<u64, Vec<Id>> = HashMap::new();
        for rid in self.api.ids("IFCRELDEFINESBYPROPERTIES", false) {
            let r = self.api.must(rid)?;
            let Some(p) = r.a("RelatingPropertyDefinition").href() else { continue };
            rels.entry(id_key(p)).or_default().push(rid);
        }
        self.prop_refs = Some(refs);
        self.rels_by_pset = Some(rels);
        Ok(())
    }

    /// The element's own copy of a property set (copied when it is related to other elements too).
    fn own_pset(&mut self, element: Id, pset_id: Id) -> R<Id> {
        let rel_ids = self.rels_by_pset.as_ref().and_then(|m| m.get(&id_key(pset_id)).cloned()).unwrap_or_default();
        let mut others = 0usize;
        let mut mine = None;
        for &rid in &rel_ids {
            let r = self.api.must(rid)?;
            let objs = r.a("RelatedObjects").hrefs();
            if objs.contains(&element) {
                mine = Some(r.clone());
            }
            others = others.saturating_add(objs.iter().filter(|o| **o != element).count());
        }
        let Some(mut mine) = mine.filter(|_| others > 0) else { return Ok(pset_id) };
        let pset = self.api.must(pset_id)?;
        let g = self.api.new_guid();
        let mut copy = self.api.create(
            &pset.up,
            vec![g, pset.a("OwnerHistory").clone(), pset.a("Name").clone(), pset.a("Description").clone(), pset.a("HasProperties").clone()],
        )?;
        self.api.write(&mut copy)?;
        if let Some(refs) = self.prop_refs.as_mut() {
            for h in copy.a("HasProperties").hrefs() {
                let e = refs.entry(id_key(h)).or_insert(0);
                *e = e.saturating_add(1);
            }
        }
        let rest: Vec<JV> = match mine.a("RelatedObjects") {
            JV::L(l) => l.iter().filter(|h| h.href() != Some(element)).cloned().collect(),
            _ => Vec::new(),
        };
        if rest.is_empty() {
            mine.set("RelatingPropertyDefinition", JV::H(copy.id));
            self.api.write(&mut mine)?;
            if let Some(m) = self.rels_by_pset.as_mut() {
                m.insert(id_key(pset_id), rel_ids.iter().copied().filter(|r| *r != mine.id).collect());
                m.insert(id_key(copy.id), vec![mine.id]);
            }
            return Ok(copy.id);
        }
        let g = self.api.new_guid();
        let mut rel = self.api.create(
            "IFCRELDEFINESBYPROPERTIES",
            vec![g, mine.a("OwnerHistory").clone(), JV::Null, JV::Null, JV::L(vec![JV::H(element)]), JV::H(copy.id)],
        )?;
        self.api.write(&mut rel)?;
        mine.set("RelatedObjects", JV::L(rest));
        self.api.write(&mut mine)?;
        if let Some(m) = self.rels_by_pset.as_mut() {
            m.insert(id_key(copy.id), vec![rel.id]);
        }
        Ok(copy.id)
    }

    /// `setProperty(elementId, ref, value)` → whether it changed.
    pub fn set_property(&mut self, element: Id, r: &PropRef, value: &str, record: bool) -> R<bool> {
        let prop = self
            .api
            .line(r.prop_id)?
            .filter(|p| p.get("NominalValue").is_some_and(JV::truthy))
            .ok_or_else(|| terr("ifcedit.err.single_value"))?;
        let JV::T(cur) = prop.a("NominalValue").clone() else { return Err(terr("ifcedit.err.single_value")) };
        let old = cur.v.clone();
        let typed = self.typed_value(&cur, value)?;
        if crate::draw::writer::strict_eq(&typed.val(), &old) {
            return Ok(false);
        }
        self.index()?;
        let mut pset_id = r.pset_id;
        if !r.from_type {
            pset_id = self.own_pset(element, pset_id)?;
        }
        let mut pset = self.api.must(pset_id)?;
        let shared = self.prop_refs.as_ref().and_then(|m| m.get(&id_key(r.prop_id)).copied()).unwrap_or(0) > 1;
        if shared {
            let mut copy = self
                .api
                .create(&prop.up, vec![prop.a("Name").clone(), prop.a("Description").clone(), typed.clone(), prop.a("Unit").clone()])?;
            self.api.write(&mut copy)?;
            let list: Vec<JV> = match pset.a("HasProperties") {
                JV::L(l) => l.iter().map(|h| if h.href() == Some(r.prop_id) { JV::H(copy.id) } else { h.clone() }).collect(),
                _ => return Err(JsErr("TypeError: pset.HasProperties.map is not a function".into())),
            };
            pset.set("HasProperties", JV::L(list));
            self.api.write(&mut pset)?;
            if let Some(m) = self.prop_refs.as_mut() {
                let n = m.get(&id_key(r.prop_id)).copied().unwrap_or(0);
                m.insert(id_key(r.prop_id), n.saturating_sub(1));
                m.insert(id_key(copy.id), 1);
            }
        } else {
            let mut p = prop.clone();
            p.set("NominalValue", typed.clone());
            self.api.write(&mut p)?;
        }
        if record {
            let rr = PropRef { pset_id, ..r.clone() };
            self.record(Change::Prop { id: element, r: rr, old, value: typed.val(), label: String::new() });
        }
        Ok(true)
    }

    fn set_property_raw(&mut self, id: Id, r: &PropRef, value: &P) -> R<()> {
        let pset = self.api.must(r.pset_id)?;
        for h in pset.a("HasProperties").hrefs() {
            if let Some(p) = self.api.line(h)?
                && p.a("Name").val_str().as_deref() == Some(r.prop_name.as_str())
                && p.get("NominalValue").is_some_and(JV::truthy)
            {
                let text = if matches!(value, P::Null) { String::new() } else { value.string() };
                self.set_property(id, &PropRef { prop_id: h, ..r.clone() }, &text, false)?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// `modelChange(op, { label, verb })`: one building-tool change.
    pub fn model_change(&mut self, mut op: Op, label: &str, verb: &str) -> R<OpResult> {
        let r = op.run(&mut self.api)?;
        self.reindex();
        self.record(Change::Model {
            id: r.id,
            ids: r.ids.clone(),
            made: r.made.clone(),
            label: label.into(),
            verb: verb.into(),
            journal: Box::new(r.journal.clone()),
            op: Box::new(op),
        });
        Ok(r)
    }

    pub fn reindex(&mut self) {
        self.prop_refs = None;
        self.rels_by_pset = None;
    }

    /// Undo the last change → it (None when there is none).
    pub fn undo(&mut self) -> R<Option<&Change>> {
        let Some(c) = self.changes.pop() else { return Ok(None) };
        match &c {
            Change::Model { journal, .. } => {
                undo_journal(&mut self.api, journal)?;
                self.reindex();
            }
            Change::Attr { id, attr, old, .. } => {
                let o = old.as_ref().map(P::string);
                self.set_attribute(*id, attr, o.as_deref(), false)?;
            }
            Change::Prop { id, r, old, .. } => {
                self.set_property_raw(*id, r, old)?;
            }
        }
        self.undone.push(c);
        Ok(self.undone.last())
    }

    /// The id an element has now (a redo that made it again gave it a new one).
    pub fn now(&self, id: Id) -> Id {
        let mut seen = std::collections::HashSet::new();
        let mut x = id;
        while let Some(&n) = self.moved.get(&id_key(x)) {
            if !seen.insert(id_key(x)) {
                break;
            }
            x = n;
        }
        x
    }

    /// Apply the change undo() last took back → the new change.
    pub fn redo(&mut self) -> R<Option<&Change>> {
        let Some(c) = self.undone.pop() else { return Ok(None) };
        let keep = std::mem::take(&mut self.undone);
        let res: R<Change> = (|| match c.clone() {
            Change::Attr { id, attr, old, value, label } => {
                let nid = self.now(id);
                self.set_attribute(nid, &attr, value.as_deref(), false)?;
                Ok(Change::Attr { id: nid, attr, old, value, label })
            }
            Change::Prop { id, r, old, value, label } => {
                let nid = self.now(id);
                self.set_property_raw(nid, &r, &value)?;
                Ok(Change::Prop { id: nid, r, old, value, label })
            }
            Change::Model { id, ids, made, label, verb, mut op, .. } => {
                let r = op.run(&mut self.api)?;
                for (g, old_id) in &made {
                    if let Some(nid) = r.made.iter().find(|x| x.0 == *g).map(|x| x.1)
                        && nid != *old_id
                    {
                        self.moved.insert(id_key(*old_id), nid);
                    }
                }
                Ok(Change::Model {
                    id: r.id.or(id),
                    ids: if r.ids.is_empty() { ids } else { r.ids.clone() },
                    made: r.made,
                    label,
                    verb,
                    journal: Box::new(r.journal),
                    op,
                })
            }
        })();
        match res {
            Ok(n) => {
                self.reindex();
                self.changes.push(n);
                self.undone = keep;
                Ok(self.changes.last())
            }
            Err(e) => {
                self.undone = keep;
                self.undone.push(c);
                Err(e)
            }
        }
    }

    /// `save()`: the bytes of a new IFC file with every change (FILE_NAME's time stamp from the caller's clock).
    pub fn save(&self, time_stamp: &str) -> R<Vec<u8>> {
        self.api.f.save(Some(time_stamp)).map_err(|e| JsErr(e.0))
    }
}

/// `new Date(ms).toISOString().slice(0, 19)` (a clock reading: clamped to the years 1970–9999).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn iso_seconds(ms: f64) -> String {
    let ms = if ms.is_finite() { ms.clamp(0.0, 253_402_300_799_000.0) } else { 0.0 };
    let secs = (ms / 1000.0).floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, mo, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's civil_from_days; days within 0..3 000 000).
#[allow(clippy::arithmetic_side_effects)]
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_time_stamps() {
        assert_eq!(iso_seconds(0.0), "1970-01-01T00:00:00");
        assert_eq!(iso_seconds(1_792_000_000_000.0), "2026-10-14T17:46:40");
    }
}
