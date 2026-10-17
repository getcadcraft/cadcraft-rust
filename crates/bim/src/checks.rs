//! ifc-checks.js — model checks (0.4 B19): required properties, duplicate GlobalIds, elements in no storey, and
//! stacked duplicates (same class and type, same triangle count, same bounds within a tolerance).

use std::collections::HashMap;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use crate::js::{OMap, PKey, PV, jn, num_str, round, slice16};
use crate::model::Model;
use crate::props::{Index, idf};

pub const CHECKS: [&str; 4] = ["required", "guid", "storey", "stacked"];

/// A required-property rule `{ cls, field }` (as given: the rule is echoed in the result).
#[derive(Clone, Debug, PartialEq)]
pub struct Required {
    pub raw: J,
    pub cls: String,
    pub field: Option<String>,
}

impl Required {
    pub fn from_json(r: &J) -> Required {
        Required {
            raw: r.clone(),
            cls: r.get("cls").filter(|c| cc_cad::js::truthy(Some(c))).map(cc_cad::js::to_js_string).unwrap_or_default(),
            field: r.get("field").filter(|f| cc_cad::js::truthy(Some(f))).map(cc_cad::js::to_js_string),
        }
    }
}

/// One group of a check.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub label: PV,
    pub rule: Option<J>,
    pub keys: Vec<f64>,
}

/// One check's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub id: &'static str,
    pub count: usize,
    pub groups: Vec<Group>,
}

impl Check {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("id".into(), J::String(self.id.into()));
        o.insert("count".into(), J::from(self.count));
        o.insert(
            "groups".into(),
            J::Array(
                self.groups
                    .iter()
                    .map(|g| {
                        let mut x = Map::new();
                        x.insert("label".into(), g.label.json());
                        if let Some(r) = &g.rule {
                            x.insert("rule".into(), r.clone());
                        }
                        x.insert("keys".into(), J::Array(g.keys.iter().map(|k| jn(*k)).collect()));
                        J::Object(x)
                    })
                    .collect(),
            ),
        );
        J::Object(o)
    }
}

/// `runChecks(ix, model, { required, tolerance })`. `required` items that are not objects count as falsy rules.
pub fn run_checks(ix: &Index, model: &Model, required: &[Option<Required>], tolerance: f64) -> Vec<Check> {
    let kb = ix.key_base;
    let mut out = Vec::new();
    let mut req = Vec::new();
    for r in required.iter().flatten() {
        let Some(field) = &r.field else { continue };
        let vals = ix.values(field);
        let miss: Vec<f64> = ix
            .ids
            .iter()
            .filter(|&&id| {
                (r.cls.is_empty() || matches!(ix.value("cls", id), Some(PV::Str(c)) if **c == *r.cls))
                    && vals.get(&id_key(id)).is_none_or(PV::is_empty_str)
            })
            .map(|id| kb + id)
            .collect();
        if !miss.is_empty() {
            let label = format!("{} · {}", if r.cls.is_empty() { "*" } else { &r.cls }, field);
            req.push(Group { label: PV::s(&label), rule: Some(r.raw.clone()), keys: miss });
        }
    }
    out.push(Check { id: "required", count: req.iter().map(|g| g.keys.len()).sum(), groups: req });
    let mut by_guid: OMap<PKey, (PV, Vec<f64>)> = OMap::new();
    for (k, g) in ix.values("guid").iter() {
        if let Some(e) = by_guid.entry(g.key(), || (g.clone(), Vec::new())) {
            e.1.push(kb + idf(*k));
        }
    }
    let dup: Vec<Group> =
        by_guid.vals.into_iter().filter(|(_, ks)| ks.len() > 1).map(|(g, ks)| Group { label: g, rule: None, keys: ks }).collect();
    out.push(Check { id: "guid", count: dup.iter().map(|g| g.keys.len()).sum(), groups: dup });
    let loose: Vec<f64> = ix.ids.iter().filter(|id| model.storey(**id).is_none()).map(|id| kb + id).collect();
    let n = loose.len();
    out.push(Check {
        id: "storey",
        count: n,
        groups: if loose.is_empty() { Vec::new() } else { vec![Group { label: PV::s(""), rule: None, keys: loose }] },
    });
    let (tris, boxes) = tris_and_boxes(model, None);
    let q = |v: f64| num_str(round(v / tolerance));
    let mut groups: OMap<String, Vec<f64>> = OMap::new();
    for &id in &ix.ids {
        let key = kb + id;
        let Some(bb) = boxes.get(&id_key(key)) else { continue };
        let mut sig = vec![
            ix.value("cls", id).map_or_else(String::new, PV::text),
            ix.value("type", id).map_or_else(String::new, PV::text),
            num_str(tris.get(&id_key(key)).copied().filter(|t| *t != 0.0 && !t.is_nan()).unwrap_or(0.0)),
        ];
        sig.extend(bb.iter().map(|v| q(*v)));
        if let Some(e) = groups.entry(sig.join("|"), Vec::new) {
            e.push(key);
        }
    }
    let stacked: Vec<Group> =
        groups.vals.into_iter().filter(|ks| ks.len() > 1).map(|ks| Group { label: PV::s(""), rule: None, keys: ks }).collect();
    out.push(Check { id: "stacked", count: stacked.iter().map(|g| g.keys.len()).sum(), groups: stacked });
    out
}

/// Each element's triangle count and bounds over the batches (vertices through `m` when given, as ifc-compare.js).
pub fn tris_and_boxes(model: &Model, m: Option<&[f64; 16]>) -> (HashMap<u64, f64>, HashMap<u64, [f64; 6]>) {
    let mut tris: HashMap<u64, f64> = HashMap::new();
    let mut boxes: HashMap<u64, [f64; 6]> = HashMap::new();
    for b in &model.batches {
        let e = &b.elems;
        for (r, &k) in e.ids.iter().enumerate() {
            let kk = id_key(f64::from(k));
            let t = tris.entry(kk).or_insert(0.0);
            *t += f64::from(e.tn.get(r).copied().unwrap_or(0));
            let bb = boxes.entry(kk).or_insert([
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ]);
            let v0 = usize::try_from(e.v0.get(r).copied().unwrap_or(0)).unwrap_or(usize::MAX);
            let vn = usize::try_from(e.vn.get(r).copied().unwrap_or(0)).unwrap_or(0);
            let end = v0.saturating_add(vn);
            for v in v0..end {
                let at = |c: usize| b.positions.get(v.saturating_mul(3).saturating_add(c)).map_or(f64::NAN, |&x| f64::from(x));
                let (mut x, mut y, mut z) = (at(0), at(1), at(2));
                if let Some(mm) = m {
                    let xx = mm[0] * x + mm[4] * y + mm[8] * z + mm[12];
                    let yy = mm[1] * x + mm[5] * y + mm[9] * z + mm[13];
                    let zz = mm[2] * x + mm[6] * y + mm[10] * z + mm[14];
                    (x, y, z) = (xx, yy, zz);
                }
                if x < bb[0] {
                    bb[0] = x;
                }
                if y < bb[1] {
                    bb[1] = y;
                }
                if z < bb[2] {
                    bb[2] = z;
                }
                if x > bb[3] {
                    bb[3] = x;
                }
                if y > bb[4] {
                    bb[4] = y;
                }
                if z > bb[5] {
                    bb[5] = z;
                }
            }
        }
    }
    (tris, boxes)
}

/// `cleanRules(raw)`: required-property rules from storage (untrusted) → None = null.
pub fn clean_rules(raw_text: Option<&str>, parsed: Option<&J>) -> Option<Vec<(String, String)>> {
    let owned;
    let arr = match (raw_text, parsed) {
        (Some(t), _) => {
            owned = serde_json::from_str::<J>(t).ok()?;
            &owned
        }
        (None, Some(p)) => p,
        (None, None) => return None,
    };
    let arr = arr.as_array()?;
    Some(
        arr.iter()
            .take(50)
            .filter(|r| {
                cc_cad::js::truthy(Some(r))
                    && matches!(r.get("field"), Some(J::String(f)) if !f.is_empty() && crate::js::len16(f) <= 300)
                    && matches!(r.get("cls"), None | Some(J::String(_)))
            })
            .map(|r| {
                let cls = r.get("cls").and_then(J::as_str).unwrap_or("");
                (slice16(cls, 80), r.get("field").and_then(J::as_str).unwrap_or("").to_string())
            })
            .collect(),
    )
}
