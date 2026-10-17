//! ifc-compare.js — what changed between two versions of an IFC model, element by element through their GlobalIds
//! (0.4 B21): added, removed, changed (properties, moved, reshaped), same, and GlobalIds used twice (left out).

use std::collections::HashSet;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use crate::checks::tris_and_boxes;
use crate::js::{OMap, PKey, PV, hypot3, jn, str_num, to_fixed};
use crate::model::Model;
use crate::props::{Index, idf};
use crate::qto::{Cell, csv};

/// One side: the index, the load result, the matrix into the shared frame.
#[derive(Clone, Copy, Debug)]
pub struct Side<'m> {
    pub ix: &'m Index,
    pub model: &'m Model,
    pub matrix: Option<&'m [f64; 16]>,
}

/// One changed element.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub guid: PV,
    pub a: f64,
    pub b: f64,
    pub props: Vec<(String, Option<PV>, Option<PV>)>,
    pub moved: f64,
    pub reshaped: bool,
}

/// The comparison.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Comparison {
    pub added: Vec<f64>,
    pub removed: Vec<f64>,
    pub changed: Vec<Changed>,
    pub same: usize,
    pub dupes: Vec<PV>,
}

impl Comparison {
    pub fn json(&self) -> J {
        let keys = |v: &[f64]| J::Array(v.iter().map(|k| jn(*k)).collect());
        let mut o = Map::new();
        o.insert("added".into(), keys(&self.added));
        o.insert("removed".into(), keys(&self.removed));
        o.insert(
            "changed".into(),
            J::Array(
                self.changed
                    .iter()
                    .map(|c| {
                        let mut x = Map::new();
                        x.insert("guid".into(), c.guid.json());
                        x.insert("a".into(), jn(c.a));
                        x.insert("b".into(), jn(c.b));
                        x.insert(
                            "props".into(),
                            J::Array(
                                c.props
                                    .iter()
                                    .map(|(f, before, after)| {
                                        let mut p = Map::new();
                                        p.insert("field".into(), J::String(f.clone()));
                                        if let Some(v) = before {
                                            p.insert("before".into(), v.json());
                                        }
                                        if let Some(v) = after {
                                            p.insert("after".into(), v.json());
                                        }
                                        J::Object(p)
                                    })
                                    .collect(),
                            ),
                        );
                        x.insert("moved".into(), jn(c.moved));
                        x.insert("reshaped".into(), J::Bool(c.reshaped));
                        J::Object(x)
                    })
                    .collect(),
            ),
        );
        o.insert("same".into(), J::from(self.same));
        o.insert("dupes".into(), J::Array(self.dupes.iter().map(PV::json).collect()));
        J::Object(o)
    }
}

/// `byGuid(ix)`: GlobalId → the first id, and the GlobalIds seen twice.
fn by_guid(ix: &Index) -> (OMap<PKey, (PV, f64)>, OMap<PKey, PV>) {
    let mut m: OMap<PKey, (PV, f64)> = OMap::new();
    let mut dup: OMap<PKey, PV> = OMap::new();
    for (k, g) in ix.values("guid").iter() {
        let gk = g.key();
        if m.has(&gk) {
            if !dup.has(&gk) {
                dup.set(gk, g.clone());
            }
        } else {
            m.set(gk, (g.clone(), idf(*k)));
        }
    }
    (m, dup)
}

fn same_val(a: Option<&PV>, b: Option<&PV>) -> bool {
    match (a, b) {
        (Some(PV::Num(x)), Some(PV::Num(y))) => (x - y).abs() <= 1e-9 * crate::js::max(crate::js::max(1.0, x.abs()), y.abs()),
        (Some(x), Some(y)) => x.strict_eq(y),
        (None, None) => true,
        _ => false,
    }
}

/// `compareModels(A, B, { tolerance })`.
pub fn compare_models(a: Side<'_>, b: Side<'_>, tolerance: f64) -> Comparison {
    let (ga_m, ga_dup) = by_guid(a.ix);
    let (gb_m, gb_dup) = by_guid(b.ix);
    let mut dupes: OMap<PKey, PV> = OMap::new();
    for (k, v) in ga_dup.iter().chain(gb_dup.iter()) {
        if !dupes.has(k) {
            dupes.set(k.clone(), v.clone());
        }
    }
    let skip: HashSet<PKey> = dupes.keys.iter().cloned().collect();
    let (sa_t, sa_b) = tris_and_boxes(a.model, a.matrix);
    let (sb_t, sb_b) = tris_and_boxes(b.model, b.matrix);
    let mut fields: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for f in a.ix.fields.iter().chain(b.ix.fields.iter()) {
        if seen.insert(&f.id) && f.id != "guid" {
            fields.push(&f.id);
        }
    }
    let va: Vec<&crate::props::Vals> = fields.iter().map(|f| a.ix.values(f)).collect();
    let vb: Vec<&crate::props::Vals> = fields.iter().map(|f| b.ix.values(f)).collect();
    let mut out = Comparison::default();
    for (gk, (g, id_a)) in ga_m.iter() {
        if skip.contains(gk) {
            continue;
        }
        let Some((_, id_b)) = gb_m.get(gk) else {
            out.removed.push(a.ix.key_base + id_a);
            continue;
        };
        let ka = a.ix.key_base + id_a;
        let kb = b.ix.key_base + id_b;
        let mut props = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            let x = va.get(i).and_then(|m| m.get(&id_key(*id_a)));
            let y = vb.get(i).and_then(|m| m.get(&id_key(*id_b)));
            if x.is_none() && y.is_none() {
                continue;
            }
            if !same_val(x, y) {
                props.push(((*f).to_string(), x.cloned(), y.cloned()));
            }
        }
        let p = sa_b.get(&id_key(ka));
        let q = sb_b.get(&id_key(kb));
        let mut moved = 0.0;
        let mut reshaped = false;
        if let (Some(p), Some(q)) = (p, q) {
            let d = hypot3((p[0] + p[3] - q[0] - q[3]) / 2.0, (p[1] + p[4] - q[1] - q[4]) / 2.0, (p[2] + p[5] - q[2] - q[5]) / 2.0);
            if d > tolerance {
                moved = d;
            }
            reshaped = (0..3usize).any(|i| {
                let (pl, ph, ql, qh) = (p.get(i), p.get(i.wrapping_add(3)), q.get(i), q.get(i.wrapping_add(3)));
                match (pl, ph, ql, qh) {
                    (Some(pl), Some(ph), Some(ql), Some(qh)) => ((ph - pl) - (qh - ql)).abs() > tolerance,
                    _ => false,
                }
            }) || {
                let (ta, tb) = (sa_t.get(&id_key(ka)), sb_t.get(&id_key(kb)));
                // `!==` on numbers (undefined only equals undefined)
                match (ta, tb) {
                    (Some(x), Some(y)) => x != y,
                    (None, None) => false,
                    _ => true,
                }
            };
        }
        let moved_truthy = moved != 0.0 && !moved.is_nan();
        if !props.is_empty() || moved_truthy || reshaped {
            out.changed.push(Changed { guid: g.clone(), a: ka, b: kb, props, moved, reshaped });
        } else {
            out.same = out.same.saturating_add(1);
        }
    }
    for (gk, (_, id_b)) in gb_m.iter() {
        if !skip.contains(gk) && !ga_m.has(gk) {
            out.added.push(b.ix.key_base + id_b);
        }
    }
    out.dupes = dupes.vals;
    out
}

/// The words compareCsv takes; `use_index`: name / GlobalId of a key from its side's index (as the tests do).
#[derive(Clone, Debug, Default)]
pub struct CompareWords {
    pub words: std::collections::HashMap<String, String>,
    pub labels: std::collections::HashMap<String, String>,
    pub use_index: bool,
}

/// `compareCsv(res, { name, guid, label, words })`.
pub fn compare_csv(res: &Comparison, w: &CompareWords, a: &Index, b: &Index) -> String {
    let word = |k: &str, d: &str| w.words.get(k).cloned().unwrap_or_else(|| d.to_string());
    let name = |side: char, k: f64| -> Cell {
        if w.use_index {
            let ix = if side == 'a' { a } else { b };
            ix.value("name", k - ix.key_base).map_or(Cell::Undef, |v| Cell::of(Some(v)))
        } else {
            Cell::Str(crate::js::num_str(k))
        }
    };
    let guid = |side: char, k: f64| -> Cell {
        if w.use_index {
            let ix = if side == 'a' { a } else { b };
            ix.value("guid", k - ix.key_base).map_or(Cell::Undef, |v| Cell::of(Some(v)))
        } else {
            Cell::Str(String::new())
        }
    };
    let label = |f: &str| w.labels.get(f).cloned().unwrap_or_else(|| f.to_string());
    let s = |x: String| Cell::Str(x);
    let e = || Cell::Str(String::new());
    let mut rows: Vec<Vec<Cell>> = vec![vec![
        s(word("change", "Change")),
        s(word("guid", "GlobalId")),
        s(word("element", "Element")),
        s(word("what", "What")),
        s(word("before", "Before")),
        s(word("after", "After")),
    ]];
    let pv = |v: &Option<PV>| v.as_ref().map_or(Cell::Undef, |x| Cell::of(Some(x)));
    for c in &res.changed {
        let gc = Cell::of(Some(&c.guid));
        for (f, before, after) in &c.props {
            rows.push(vec![s(word("changed", "Changed")), gc.clone(), name('b', c.b), s(label(f)), pv(before), pv(after)]);
        }
        if c.moved != 0.0 && !c.moved.is_nan() {
            rows.push(vec![s(word("moved", "Moved")), gc.clone(), name('b', c.b), e(), e(), Cell::Num(str_num(&to_fixed(c.moved, 4)))]);
        }
        if c.reshaped {
            rows.push(vec![s(word("reshaped", "Shape changed")), gc.clone(), name('b', c.b), e(), e(), e()]);
        }
    }
    for &k in &res.added {
        rows.push(vec![s(word("added", "Added")), guid('b', k), name('b', k), e(), e(), e()]);
    }
    for &k in &res.removed {
        rows.push(vec![s(word("removed", "Removed")), guid('a', k), name('a', k), e(), e(), e()]);
    }
    csv(&rows)
}
