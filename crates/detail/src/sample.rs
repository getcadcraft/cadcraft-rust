//! sample.js — the sample model (File → Sample): a two-bay frame on grids 1–3 × A–B, two levels, W12x50 columns with
//! base plates, W16x26 beams with bolted end plates, one HSS brace, pad footings with a bottom mat and a pier footing
//! with starter bars. Built only through `Model::change`, like any edit.

use crate::i18n::{F, t, t0};
use crate::js::Res;
use crate::model::{Model, grids_from_spacings, point_at};
use serde_json::{Value, json};

/// `sampleModel(name)`.
pub fn sample_model(name: &str) -> Res<Model> {
    let mut model = Model::default();
    model.change(&t0("sample.undo"), |tx| {
        let l1 = t("model.level", &[("n", F::N(1.0))]);
        let l2 = t("model.level", &[("n", F::N(2.0))]);
        tx.set_name(Some(&Value::from(name)));
        tx.set_grids(Some(&grids_from_spacings("1 2 3", "6000 6000", "A B", "7000")?))?;
        tx.set_levels(Some(&json!([{ "name": l1, "z": 0 }, { "name": l2, "z": 4000 }])))?;
        let at = |s: &Value, gy: &str, gx: &str, lv: &str| point_at(s, &Value::from(gy), &Value::from(gx), &Value::from(lv)).map(|p| json!(p));
        let mut cols = Vec::new();
        for gy in ["A", "B"] {
            for gx in ["1", "2", "3"] {
                let (a, b) = (at(tx.s, gy, gx, &l1), at(tx.s, gy, gx, &l2));
                cols.push(tx.add_member(Some(&json!({ "type": "column", "profile": "W12x50", "start": a, "end": b })))?);
            }
        }
        let mut beams = Vec::new();
        for gy in ["A", "B"] {
            for (a, b) in [("1", "2"), ("2", "3")] {
                let (p, q) = (at(tx.s, gy, a, &l2), at(tx.s, gy, b, &l2));
                beams.push(tx.add_member(Some(&json!({ "type": "beam", "profile": "W16x26", "start": p, "end": q })))?);
            }
        }
        let (p, q) = (at(tx.s, "A", "1", &l1), at(tx.s, "A", "2", &l2));
        tx.add_member(Some(&json!({ "type": "brace", "profile": "HSS4x4x1/4", "start": p, "end": q })))?;
        for id in &beams {
            for end in ["start", "end"] {
                let members = crate::js::arr(tx.s.get("members")).to_vec();
                let Some(m) = members.iter().find(|x| x.get("id") == Some(id)) else { continue };
                let pe = crate::js::p3_of(m.get(end));
                let col = members.iter().find(|c| {
                    let cs = crate::js::p3_of(c.get("start"));
                    c.get("type").and_then(Value::as_str) == Some("column") && (cs[0] - pe[0]).abs() < 1.0 && (cs[1] - pe[1]).abs() < 1.0
                });
                let colid = col.and_then(|c| c.get("id")).cloned().unwrap_or(Value::Null);
                tx.add_connection(Some(&json!({ "type": "endplate", "beam": id, "end": end, "column": colid })))?;
            }
        }
        for id in &cols {
            tx.add_connection(Some(&json!({ "type": "baseplate", "column": id })))?;
        }
        for id in &cols {
            let members = crate::js::arr(tx.s.get("members")).to_vec();
            let Some(c) = members.iter().find(|x| x.get("id") == Some(id)) else { continue };
            let s = crate::js::p3_of(c.get("start"));
            tx.add_footing(Some(&json!({ "x": s[0], "y": s[1], "top": s[2], "L": 2000, "B": 2000, "H": 600, "starters": { "on": false } })))?;
        }
        tx.add_footing(Some(&json!({ "x": 16000, "y": 3500, "top": 0, "L": 2400, "B": 1800, "H": 600, "starters": { "on": true, "bar": "#6", "nx": 3, "ny": 2, "sx": 200, "sy": 250, "projection": 750, "hook": true } })))?;
        Ok(())
    })?;
    model.undo_stack.clear();
    model.mark_saved();
    Ok(model)
}
