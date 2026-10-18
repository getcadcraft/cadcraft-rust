//! TakeoffCraft's property panels on the sheet view, as takeoffcraft.js has them: the item's settings (name, type, colour,
//! unit, second conditions, height / depth, back-out, slope and pitch, the 3D shape, waste, material and labour, labour by
//! crew, CSI number), its measured values and its assembly (components with a quantity formula); the selected mark (item,
//! bid area, segments that are deducts and their slopes, its pitch, cut-outs, spot elevation); bid areas (the picker new
//! marks go in, the manage window with alternates) and the By-area table. Every edit is the handler 0.5 runs: the same
//! parsing, the same `Takeoff` call with the same patch and undo label, so one edit is one undo step and the saved
//! project matches the JS doing the same thing. The panel widgets and the `takeoff.*` commands call the same methods.

use std::collections::{BTreeSet, HashMap};

use cc_takeoff::geom::{self as G, P};
use cc_takeoff::i18n::{F, t, tf, tn};
use cc_takeoff::model::{
    Backouts, Measure, QOpts, TYPES, alternates, area_estimates, item_vars, kinds_of, measure_mark, quantities, vars_of,
};
use cc_takeoff::units as U;
use egui::{Color32, RichText};
use serde_json::{Map, Value, json};

use crate::takeofftools::TakeoffMode;
use crate::theme::Tokens;

type Obj = Map<String, Value>;

fn o(pairs: Vec<(&str, Value)>) -> Obj {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}
fn f(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}
fn on(text: &str) -> bool {
    matches!(text, "on" | "true" | "1" | "yes")
}
fn pts_p(m: &Value, k: &str) -> Vec<P> {
    cc_takeoff::model::pts_of(m.get(k))
}
/// `TYPE_LABELS[k]`.
fn type_label(k: &str) -> String {
    U::type_label(k)
}

/// The 3D-only lengths of an item: (field, patch key, catalog label).
const SHAPE: [(&str, &str, &str); 3] =
    [("thick", "thickFt", "item.f.thick"), ("head", "headFt", "item.f.head"), ("base", "baseFt", "item.f.base")];

impl TakeoffMode {
    fn fail(&mut self, msg: String) -> Result<(), String> {
        self.message = msg.clone();
        Err(msg)
    }

    /// `commit(patch, label)` of buildItemProps: an error from the model goes to the status line.
    fn commit_item(&mut self, id: &str, patch: Obj, label: &str) -> Result<(), String> {
        match self.tk.update_item(id, &patch, Some(&t(label))) {
            Ok(_) => Ok(()),
            Err(e) => self.fail(e),
        }
    }

    /// One field of the current item's settings set from what was typed or picked (the handler of that field in
    /// buildItemProps / condFields / backoutFields / pitchFields / shapeFields / crewFields). Fields: name, type, color,
    /// unit, height, waste, mat, lab, csi, pitch, slope, thick, head, base, backout (on/off), `bo:<item>` (on/off),
    /// `cond:<kind>` (on/off), `cond-unit:<kind>`, crew (on/off), crew-prod, crew-size, crew-wage.
    pub fn item_field(&mut self, field: &str, text: &str) -> Result<(), String> {
        let Some(id) = self.item.clone() else { return self.fail(t("tool.err.item")) };
        let Some(it) = self.tk.item(&id).cloned() else { return self.fail(t("tool.err.item")) };
        let unit = s(it.get("unit"));
        let lu = U::length_unit_of(&unit);
        let len_in = |v: &str| -> Result<f64, String> { if v.trim().is_empty() { Ok(0.0) } else { U::parse_length(v, lu) } };
        match field {
            "name" => {
                if text.trim().is_empty() {
                    return self.fail(t("item.err.name"));
                }
                self.commit_item(&id, o(vec![("name", json!(text))]), "undo.item.rename")
            }
            "type" => {
                let u = U::default_unit(text);
                self.commit_item(&id, o(vec![("type", json!(text)), ("unit", json!(u))]), "undo.item.type")
            }
            "color" => self.commit_item(&id, o(vec![("color", json!(text))]), "undo.item.color"),
            "unit" => self.commit_item(&id, o(vec![("unit", json!(text))]), "undo.item.unit"),
            "height" => match len_in(text) {
                Ok(ft) => self.commit_item(&id, o(vec![("heightFt", json!(ft))]), "undo.item.height"),
                Err(e) => self.fail(e),
            },
            "thick" | "head" | "base" => {
                let key = SHAPE.iter().find(|x| x.0 == field).map_or("baseFt", |x| x.1);
                match len_in(text) {
                    Ok(ft) => self.commit_item(&id, o(vec![(key, json!(ft))]), "undo.item.3d"),
                    Err(e) => self.fail(e),
                }
            }
            "waste" => match U::parse_percent(text) {
                Ok(bp) => self.commit_item(&id, o(vec![("waste", json!(bp))]), "undo.item.waste"),
                Err(e) => self.fail(e),
            },
            "mat" | "lab" => match U::parse_rate(text) {
                Ok(r) => self.commit_item(&id, o(vec![(field, json!(r))]), if field == "mat" { "undo.item.mat" } else { "undo.item.lab" }),
                Err(e) => self.fail(e),
            },
            "csi" => {
                let x = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !x.is_empty() && !cc_takeoff::model::csi_ok(Some(&json!(x))) {
                    return self.fail(t("item.csi.err"));
                }
                self.commit_item(&id, o(vec![("csi", if x.is_empty() { Value::Null } else { json!(x) })]), "undo.item.csi")
            }
            "pitch" => match U::parse_pitch(text) {
                Ok(rise) => self.commit_item(&id, o(vec![("pitch", json!(rise))]), "pitch.undo"),
                Err(k) => self.fail(t(&k)),
            },
            "slope" => self.commit_item(&id, o(vec![("slope", json!(text))]), "slope.undo"),
            "backout" => {
                if on(text) {
                    let others: Vec<Value> = self
                        .tk
                        .items()
                        .iter()
                        .filter(|x| s(x.get("type")) == "area" && s(x.get("id")) != id && x.get("backout") != Some(&Value::Bool(true)))
                        .map(|x| json!(s(x.get("id"))))
                        .collect();
                    self.commit_item(&id, o(vec![("backout", json!(true)), ("backoutOf", Value::Array(others))]), "backout.undo.on")
                } else {
                    self.commit_item(&id, o(vec![("backout", json!(false))]), "backout.undo.off")
                }
            }
            "crew" => {
                let wage = it.get("crew").and_then(|c| c.get("wage")).filter(|w| !w.is_null()).cloned().unwrap_or(json!(0));
                let v = if on(text) { json!({ "prod": 10, "size": 2, "wage": wage }) } else { Value::Null };
                self.commit_item(&id, o(vec![("crew", v)]), "undo.crew")
            }
            "crew-prod" | "crew-size" | "crew-wage" => {
                let Some(Value::Object(c)) = it.get("crew").cloned() else { return Ok(()) };
                // num(): commas out, then /^\d*\.?\d+$/
                let num = |v: &str| -> f64 {
                    let s2: String = v.trim().chars().filter(|c| *c != ',').collect();
                    let ok = !s2.is_empty() && {
                        let (a, b) = s2.split_once('.').unwrap_or(("", s2.as_str()));
                        a.chars().all(|c| c.is_ascii_digit()) && !b.is_empty() && b.chars().all(|c| c.is_ascii_digit())
                    };
                    if ok { cc_takeoff::jv::str_to_number(&s2) } else { f64::NAN }
                };
                let mut c2 = c.clone();
                match field {
                    "crew-prod" => {
                        let x = num(text);
                        if !(x > 0.0 && x < 1e7) {
                            return self.fail(t("crew.err.prod"));
                        }
                        c2.insert("prod".into(), json!(x));
                    }
                    "crew-size" => {
                        let x = num(text);
                        if !(x.fract() == 0.0 && (1.0..=1000.0).contains(&x)) {
                            return self.fail(t("crew.err.size"));
                        }
                        c2.insert("size".into(), json!(x));
                    }
                    _ => match U::parse_rate(text) {
                        Ok(r) => {
                            c2.insert("wage".into(), json!(r));
                        }
                        Err(e) => return self.fail(e),
                    },
                }
                self.commit_item(&id, o(vec![("crew", Value::Object(c2))]), "undo.crew")
            }
            _ => {
                if let Some(x) = field.strip_prefix("bo:") {
                    let mut next: Vec<Value> = it.get("backoutOf").and_then(Value::as_array).cloned().unwrap_or_default();
                    if on(text) {
                        if !next.iter().any(|v| v.as_str() == Some(x)) {
                            next.push(json!(x));
                        }
                    } else {
                        next.retain(|v| v.as_str() != Some(x));
                    }
                    return self.commit_item(&id, o(vec![("backoutOf", Value::Array(next))]), "backout.undo.of");
                }
                if let Some(k) = field.strip_prefix("cond:") {
                    return match self.tk.set_cond(&id, k, on(text)) {
                        Ok(_) => Ok(()),
                        Err(e) => self.fail(e),
                    };
                }
                if let Some(k) = field.strip_prefix("cond-unit:") {
                    return match self.tk.update_cond(&id, k, &o(vec![("unit", json!(text))]), Some(&t("undo.item.unit"))) {
                        Ok(_) => Ok(()),
                        Err(e) => self.fail(e),
                    };
                }
                Err(format!("takeoff.set: unknown field {field}"))
            }
        }
    }

    /// The assembly: "add" (asm.add: a component named comp.default, EA, the type's first variable), "remove:<id>", or
    /// "<id>:<field>=<text>" with field name, unit, formula, waste, mat or lab (compEditor's handlers).
    pub fn comp_edit(&mut self, spec: &str) -> Result<(), String> {
        let Some(id) = self.item.clone() else { return self.fail(t("tool.err.item")) };
        let ty = self.tk.item(&id).map(|it| s(it.get("type"))).unwrap_or_default();
        if spec == "add" {
            let first = vars_of(&ty).first().copied().unwrap_or("N");
            let fields = o(vec![("name", json!(t("comp.default"))), ("unit", json!("EA")), ("formula", json!(first))]);
            return self.tk.add_component(&id, &fields).map(|_| ()).or_else(|e| self.fail(e));
        }
        if let Some(c) = spec.strip_prefix("remove:") {
            return self.tk.remove_component(&id, c).map(|_| ()).or_else(|e| self.fail(e));
        }
        let (lhs, text) = spec.split_once('=').ok_or("takeoff.comp: add | remove:<id> | <id>:<field>=<text>")?;
        let (c, field) = lhs.split_once(':').ok_or("takeoff.comp: <id>:<field>=<text>")?;
        let patch = match field {
            "name" | "unit" | "formula" => o(vec![(field, json!(text))]),
            "waste" => match U::parse_percent(text) {
                Ok(bp) => o(vec![("waste", json!(bp))]),
                Err(e) => return self.fail(e),
            },
            "mat" | "lab" => match U::parse_rate(text) {
                Ok(r) => o(vec![(field, json!(r))]),
                Err(e) => return self.fail(e),
            },
            _ => return Err(format!("takeoff.comp: unknown field {field}")),
        };
        self.tk.update_component(&id, c, &patch).map(|_| ()).or_else(|e| self.fail(e))
    }

    /// The item's actions: up, down, delete (deleteItem, without its question), dup (duplicateItem: a copy through the
    /// clipboard format, with its assembly and costs, no marks).
    pub fn item_action(&mut self, act: &str) -> Result<(), String> {
        let Some(id) = self.item.clone() else { return self.fail(t("tool.err.item")) };
        match act {
            "up" | "down" => self.tk.move_item(&id, if act == "up" { -1.0 } else { 1.0 }).map(|_| ()).or_else(|e| self.fail(e)),
            "delete" => {
                self.tk.remove_item(&id).or_else(|e| self.fail(e).map(|()| false))?;
                self.item = self.tk.items().first().and_then(|i| i.get("id")).and_then(Value::as_str).map(str::to_string);
                if self.sel.as_deref().is_some_and(|m| self.tk.mark(m).is_none()) {
                    self.sel = None;
                }
                Ok(())
            }
            "dup" => {
                use cc_takeoff::clipboard::{CopyCtx, PasteOpts, copy_items, paste_clip, read_clip};
                let Some(wire) = copy_items(&self.tk.state, std::slice::from_ref(&id), &CopyCtx::default()) else { return Ok(()) };
                let Ok(r) = read_clip(&wire) else { return Ok(()) };
                let opts = PasteOpts { label: t("undo.item.dup"), ..PasteOpts::default() };
                match paste_clip(&mut self.tk, &r.clip, &opts) {
                    Ok(Some(out)) => {
                        if let Some(n) = out.items.first() {
                            self.item = Some(n.clone());
                            let name = self.tk.item(n).map(|i| s(i.get("name"))).unwrap_or_default();
                            self.message = tf("item.dup.done", &[("item", F::S(name))]);
                        }
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(e) => self.fail(e),
                }
            }
            _ => Err(format!("takeoff.itemact: unknown action {act} (up, down, delete, dup)")),
        }
    }

    // ---- the selected mark ----

    /// markAt: the mark of `sheet` under `p` (PDF user space): counts and runs on top, then areas (edge or inside, not
    /// in a cut-out), `hit` PDF units around.
    pub fn mark_at(&self, sheet: &str, p: [f64; 2], hit: f64) -> Option<String> {
        let q = P::new(p[0], p[1]);
        let list: Vec<&Value> = self.tk.marks().iter().filter(|m| s(m.get("sheet")) == sheet).collect();
        for m in list.iter().rev() {
            let pts = pts_p(m, "pts");
            match s(m.get("kind")).as_str() {
                "count" if pts.first().is_some_and(|a| G::dist(&q, a) <= hit * 1.3) => return Some(s(m.get("id"))),
                "linear" if G::dist_to_path(&q, &pts, false) <= hit => return Some(s(m.get("id"))),
                _ => {}
            }
        }
        for m in list.iter().rev() {
            if s(m.get("kind")) != "area" {
                continue;
            }
            let pts = pts_p(m, "pts");
            let holes = cc_takeoff::model::holes_of(m.get("holes"));
            if G::dist_to_path(&q, &pts, true) <= hit || holes.iter().any(|h| G::dist_to_path(&q, h, true) <= hit) {
                return Some(s(m.get("id")));
            }
            if G::point_in_polygon(&q, &pts) && !holes.iter().any(|h| G::point_in_polygon(&q, h)) {
                return Some(s(m.get("id")));
            }
        }
        None
    }

    /// selectAt: select the mark under `p` (or nothing); its item becomes the current one.
    pub fn select_at(&mut self, sheet: &str, p: [f64; 2]) {
        let hit = 7.0 * self.px;
        self.select(self.mark_at(sheet, p, hit));
    }

    /// Select a mark by id (None clears).
    pub fn select(&mut self, id: Option<String>) {
        let m = id.as_deref().and_then(|i| self.tk.mark(i)).cloned();
        self.sel = m.as_ref().map(|m| s(m.get("id")));
        if let Some(m) = m {
            let item = s(m.get("item"));
            let name = self.tk.item(&item).map(|i| s(i.get("name"))).unwrap_or_default();
            self.item = Some(item);
            self.message = tf(&format!("sel.one.{}", s(m.get("kind"))), &[("item", F::S(name)), ("mod", F::S("Ctrl".into()))]);
        }
    }

    /// One field of the selected mark (renderMarkProps' handlers): item, area, `ded:<i>` (on/off), `sl:<i>` (flat,
    /// rake, hip or empty for the item's), pitch, z, cutout.remove, delete.
    pub fn mark_field(&mut self, field: &str, text: &str) -> Result<(), String> {
        let Some(id) = self.sel.clone() else { return Err("takeoff.mark: no mark is selected".into()) };
        let Some(m) = self.tk.mark(&id).cloned() else { return Err("takeoff.mark: no mark is selected".into()) };
        let kind = s(m.get("kind"));
        let npts = m.get("pts").and_then(Value::as_array).map_or(0, Vec::len);
        let up = |me: &mut Self, patch: Obj, label: &str| -> Result<(), String> {
            me.tk.update_mark(&id, &patch, Some(&t(label))).map(|_| ()).or_else(|e| me.fail(e))
        };
        match field {
            "item" => {
                up(self, o(vec![("item", json!(text))]), "undo.mark.item")?;
                self.item = Some(text.to_string());
                Ok(())
            }
            "area" => {
                let area = (!text.is_empty()).then_some(text);
                match self.tk.set_marks_area(std::slice::from_ref(&id), area, None) {
                    Ok(true) => {
                        let name = area.and_then(|a| self.tk.area(a)).map_or_else(|| t("area.none"), |a| s(a.get("name")));
                        self.message = cc_takeoff::i18n::tf("area.moved", &[("n", F::N(1.0)), ("area", F::S(name))]);
                        Ok(())
                    }
                    Ok(false) => Ok(()),
                    Err(e) => self.fail(e),
                }
            }
            "pitch" => {
                if text.trim().is_empty() {
                    return up(self, o(vec![("pitch", Value::Null)]), "pitch.undo");
                }
                match U::parse_pitch(text) {
                    Ok(r) => up(self, o(vec![("pitch", json!(r))]), "pitch.undo"),
                    Err(k) => self.fail(t(&k)),
                }
            }
            "z" => {
                let v = text.trim();
                if v.is_empty() {
                    return up(self, o(vec![("z", Value::Null)]), "undo.mark.z");
                }
                let plain = {
                    let b = v.strip_prefix('-').unwrap_or(v);
                    let (a, c) = b.split_once('.').map_or((b, None), |(a, c)| (a, Some(c)));
                    !a.is_empty()
                        && a.chars().all(|x| x.is_ascii_digit())
                        && c.is_none_or(|c| !c.is_empty() && c.chars().all(|x| x.is_ascii_digit()))
                };
                let ft = if plain {
                    Some(cc_takeoff::jv::str_to_number(v))
                } else {
                    U::parse_length(v.strip_prefix('-').unwrap_or(v), "ft").ok().map(|ft| if v.starts_with('-') { -ft } else { ft })
                };
                match ft.filter(|x| x.abs() < 1e5) {
                    Some(ft) => up(self, o(vec![("z", json!(ft))]), "undo.mark.z"),
                    None => self.fail(t("spots.err")),
                }
            }
            "cutout.remove" => {
                let mut holes = m.get("holes").and_then(Value::as_array).cloned().unwrap_or_default();
                if kind != "area" || holes.is_empty() {
                    return Ok(());
                }
                holes.pop();
                up(self, o(vec![("holes", Value::Array(holes))]), "undo.cutout.remove")
            }
            "delete" => {
                self.tk.remove_marks(std::slice::from_ref(&id)).or_else(|e| self.fail(e).map(|()| false))?;
                self.sel = None;
                Ok(())
            }
            _ => {
                let seg = |p: &str| field.strip_prefix(p).and_then(|i| i.parse::<usize>().ok());
                if let Some(i) = seg("ded:") {
                    // setDeduct: a run keeps one counted segment
                    if kind != "linear" {
                        return Ok(());
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let mut ded: BTreeSet<i64> = m
                        .get("deducts")
                        .and_then(Value::as_array)
                        .map(|l| {
                            l.iter().filter_map(Value::as_f64).filter(|x| x.fract() == 0.0 && x.abs() < 1e9).map(|x| x as i64).collect()
                        })
                        .unwrap_or_default();
                    let i = i64::try_from(i).unwrap_or(i64::MAX);
                    if on(text) {
                        ded.insert(i);
                    } else {
                        ded.remove(&i);
                    }
                    if ded.len() >= npts.saturating_sub(1) {
                        return self.fail(t("deduct.all"));
                    }
                    let list: Vec<Value> = ded.iter().map(|d| json!(d)).collect();
                    return up(self, o(vec![("deducts", Value::Array(list))]), if on(text) { "deduct.undo.on" } else { "deduct.undo.off" });
                }
                if let Some(i) = seg("sl:") {
                    // setSegSlope: null = the item's
                    if kind != "linear" {
                        return Ok(());
                    }
                    let own = m.get("slopes").and_then(Value::as_array);
                    let mut sl: Vec<Value> = (0..npts.saturating_sub(1))
                        .map(|j| {
                            own.and_then(|l| l.get(j))
                                .filter(|x| !x.is_null() && x.as_bool() != Some(false))
                                .cloned()
                                .unwrap_or(Value::Null)
                        })
                        .collect();
                    if let Some(x) = sl.get_mut(i) {
                        *x = if text.is_empty() { Value::Null } else { json!({ "k": text }) };
                    }
                    return up(self, o(vec![("slopes", Value::Array(sl))]), "slope.undo");
                }
                Err(format!("takeoff.mark: unknown field {field}"))
            }
        }
    }

    // ---- bid areas ----

    /// The bid areas: "new:<name>" (and it becomes the current one, as the picker's New does), "cur:<id>" (empty = none),
    /// "rename:<id>:<name>", "alt:<id>:<add|deduct|>", "up:<id>", "down:<id>", "delete:<id>".
    pub fn area_edit(&mut self, spec: &str) -> Result<(), String> {
        let (op, rest) = spec.split_once(':').unwrap_or((spec, ""));
        let r = match op {
            "new" => {
                if rest.trim().is_empty() {
                    return self.fail(t("area.name.empty"));
                }
                match self.tk.add_area(Some(&json!(rest))) {
                    Ok(Some(id)) => {
                        self.set_cur_area(Some(id));
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            "add" => {
                // the manage window's Add: the area is made, the current one stays
                if rest.trim().is_empty() {
                    return Ok(());
                }
                self.tk.add_area(Some(&json!(rest))).map(|_| ())
            }
            "cur" => {
                self.set_cur_area((!rest.is_empty()).then(|| rest.to_string()));
                Ok(())
            }
            "rename" => {
                let (id, name) = rest.split_once(':').ok_or("takeoff.area: rename:<id>:<name>")?;
                if name.trim().is_empty() {
                    return Ok(());
                }
                self.tk.rename_area(id, Some(&json!(name))).map(|_| ())
            }
            "alt" => {
                let (id, alt) = rest.split_once(':').unwrap_or((rest, ""));
                self.tk.set_area_alt(id, (!alt.is_empty()).then_some(alt)).map(|_| ())
            }
            "up" | "down" => self.tk.move_area(rest, if op == "up" { -1.0 } else { 1.0 }).map(|_| ()),
            "delete" => self.tk.remove_area(rest).map(|_| ()),
            _ => return Err(format!("takeoff.area: unknown {op} (new, add, cur, rename, alt, up, down, delete)")),
        };
        if let Err(e) = r {
            return self.fail(e);
        }
        if self.cur_area.as_deref().is_some_and(|a| self.tk.area(a).is_none()) {
            self.cur_area = None;
        }
        Ok(())
    }

    fn set_cur_area(&mut self, id: Option<String>) {
        self.cur_area = id.filter(|a| self.tk.area(a).is_some());
        self.message = match self.cur_area.as_deref().and_then(|a| self.tk.area(a)) {
            Some(a) => tf("area.cur", &[("area", F::S(s(a.get("name"))))]),
            None => t("area.cur.none"),
        };
    }

    /// inArea(): what a new mark carries.
    pub fn in_area(&self, f: &mut Obj) {
        if let Some(a) = self.cur_area.as_deref().filter(|a| self.tk.area(a).is_some()) {
            f.insert("area".into(), json!(a));
        }
    }

    /// Deduct mode (T2): the segments a run gains next are drawn but not counted.
    pub fn toggle_deduct(&mut self, on: bool) {
        self.deduct = on;
        self.message = t(if on { "deduct.on" } else { "deduct.off" });
    }

    // ---- the panels ----

    /// A text field that commits when it loses focus (or Enter), as the JS `change` event: `key` keeps what is typed
    /// while it differs from the model's `value`; a model change (an undo) replaces it.
    fn text_field(&mut self, ui: &mut egui::Ui, key: &str, value: &str, label: &str, hint: &str) -> Option<String> {
        let e = self.edits.entry(key.to_string()).or_insert_with(|| (value.to_string(), value.to_string()));
        if e.0 != value {
            *e = (value.to_string(), value.to_string());
        }
        let l = ui.label(RichText::new(label).small());
        let r = ui.add(egui::TextEdit::singleline(&mut e.1).hint_text(hint).desired_width(ui.available_width())).labelled_by(l.id);
        (r.lost_focus() && e.1 != e.0).then(|| e.1.clone())
    }

    /// The bid area picker (top of the items): None, the areas, New…, Manage….
    pub fn areas_row(&mut self, ui: &mut egui::Ui) {
        let areas: Vec<Value> = self.tk.state.get("areas").and_then(Value::as_array).cloned().unwrap_or_default();
        let label = |a: &Value| match a.get("alt").and_then(Value::as_str).filter(|x| !x.is_empty()) {
            Some(alt) => tf("qty.alt.row", &[("name", F::S(s(a.get("name")))), ("kind", F::S(t(&format!("alt.{alt}")).to_lowercase()))]),
            None => s(a.get("name")),
        };
        let cur = self.cur_area.as_deref().and_then(|c| areas.iter().find(|a| s(a.get("id")) == c)).map_or_else(|| t("area.none"), label);
        let mut pick: Option<String> = None;
        egui::ComboBox::new("tk_area", t("area.label")).selected_text(cur).width(ui.available_width().clamp(80.0, 200.0)).show_ui(
            ui,
            |ui| {
                if ui.selectable_label(self.cur_area.is_none(), t("area.none")).clicked() {
                    pick = Some(String::new());
                }
                for a in &areas {
                    let id = s(a.get("id"));
                    if ui.selectable_label(self.cur_area.as_deref() == Some(id.as_str()), label(a)).clicked() {
                        pick = Some(id);
                    }
                }
                if ui.selectable_label(false, t("area.new")).clicked() {
                    self.area_dialog = Some(String::new());
                }
                if !areas.is_empty() && ui.selectable_label(false, t("area.manage")).clicked() {
                    self.manage_areas = true;
                }
            },
        );
        if let Some(p) = pick {
            let _ = self.area_edit(&format!("cur:{p}"));
        }
    }

    /// The New bid area and Manage bid areas windows.
    pub fn area_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut name) = self.area_dialog.take() {
            let mut open = true;
            let mut done = None;
            egui::Window::new(t("area.new.title")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.label(t("area.new.text"));
                let l = ui.label(t("area.name"));
                let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text(t("area.name.placeholder"))).labelled_by(l.id);
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if ui.button(t("btn.cancel")).clicked() {
                        done = Some(false);
                    }
                    if ui.button(t("btn.create")).clicked() || enter {
                        done = Some(true);
                    }
                });
            });
            match done {
                Some(true) => {
                    if self.area_edit(&format!("new:{name}")).is_err() {
                        self.area_dialog = Some(name);
                    }
                }
                Some(false) => {}
                None if open => self.area_dialog = Some(name),
                None => {}
            }
        }
        if self.manage_areas {
            let mut open = true;
            let mut act: Option<String> = None;
            let areas: Vec<Value> = self.tk.state.get("areas").and_then(Value::as_array).cloned().unwrap_or_default();
            egui::Window::new(t("area.manage.title")).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label(t("area.manage.text"));
                if areas.is_empty() {
                    ui.label(t("area.manage.none"));
                }
                let n = areas.len();
                for (i, a) in areas.iter().enumerate() {
                    let id = s(a.get("id"));
                    let name = s(a.get("name"));
                    ui.horizontal(|ui| {
                        let key = format!("area:{id}");
                        let e = self.edits.entry(key).or_insert_with(|| (name.clone(), name.clone()));
                        if e.0 != name {
                            *e = (name.clone(), name.clone());
                        }
                        let r = ui.add(egui::TextEdit::singleline(&mut e.1).desired_width(160.0));
                        if r.lost_focus() && e.1 != e.0 {
                            act = Some(format!("rename:{id}:{}", e.1));
                        }
                        let alt = a.get("alt").and_then(Value::as_str).unwrap_or("").to_string();
                        let shown = if alt.is_empty() { t("alt.base") } else { t(&format!("alt.{alt}.long")) };
                        egui::ComboBox::new(format!("tk_alt_{id}"), "").selected_text(shown).show_ui(ui, |ui| {
                            for v in ["", "add", "deduct"] {
                                let l = if v.is_empty() { t("alt.base") } else { t(&format!("alt.{v}.long")) };
                                if ui.selectable_label(alt == v, l).on_hover_text(t("alt.title")).clicked() {
                                    act = Some(format!("alt:{id}:{v}"));
                                }
                            }
                        });
                        if ui.add_enabled(i > 0, egui::Button::new("↑")).on_hover_text(t("area.up")).clicked() {
                            act = Some(format!("up:{id}"));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("↓")).on_hover_text(t("area.down")).clicked() {
                            act = Some(format!("down:{id}"));
                        }
                        if ui.button(t("area.delete")).clicked() {
                            act = Some(format!("delete:{id}"));
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let r =
                        ui.add(egui::TextEdit::singleline(&mut self.new_area).hint_text(t("area.name.placeholder")).desired_width(180.0));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button(t("area.add")).clicked() || enter) && !self.new_area.trim().is_empty() {
                        act = Some(format!("add:{}", self.new_area));
                        self.new_area.clear();
                    }
                });
                if ui.button(t("area.done")).clicked() {
                    self.manage_areas = false;
                }
            });
            if let Some(a) = act {
                let _ = self.area_edit(&a);
            }
            if !open {
                self.manage_areas = false;
            }
        }
    }

    /// The current item's settings, measured values and assembly (buildItemProps).
    pub fn item_props(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let Some(id) = self.item.clone() else { return };
        let Some(it) = self.tk.item(&id).cloned() else { return };
        let ty = s(it.get("type"));
        let unit = s(it.get("unit"));
        let lu = U::length_unit_of(&unit);
        let name = s(it.get("name"));
        let mut set: Vec<(String, String)> = Vec::new();
        ui.label(RichText::new(tf("item.h.name", &[("item", F::S(name.clone()))])).strong().color(tok.text_strong));
        if let Some(v) = self.text_field(ui, "name", &name, &t("item.f.name"), "") {
            set.push(("name".into(), v));
        }
        let n_marks = self.tk.marks().iter().filter(|m| s(m.get("item")) == id && s(m.get("kind")) == ty).count();
        ui.add_enabled_ui(n_marks == 0, |ui| {
            let l = ui.label(RichText::new(t("item.f.type")).small());
            egui::ComboBox::new("tk_i_type", "")
                .selected_text(type_label(&ty))
                .show_ui(ui, |ui| {
                    for k in TYPES {
                        if ui.selectable_label(ty == k, type_label(k)).clicked() && ty != k {
                            set.push(("type".into(), k.to_string()));
                        }
                    }
                })
                .response
                .labelled_by(l.id)
                .on_disabled_hover_text(t("item.type.locked"));
        });
        // colour: the item palette, or a hex
        ui.label(RichText::new(t("item.f.color")).small());
        let col = s(it.get("color")).to_lowercase();
        ui.horizontal_wrapped(|ui| {
            for c in cc_takeoff::model::PALETTE {
                let c32 = crate::pdfview::hex_color(c, 1.0).unwrap_or(Color32::GRAY);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                ui.painter().rect_filled(r, 2.0, c32);
                if col == c.to_lowercase() {
                    ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(2.0, tok.text_strong), egui::StrokeKind::Outside);
                }
                if resp.clicked() && col != c.to_lowercase() {
                    set.push(("color".into(), c.to_lowercase()));
                }
            }
        });
        let l = ui.label(RichText::new(t("item.f.unit")).small());
        let unit_text = |u: &str| {
            let nm = U::unit(u).map(|x| t(x.name_key)).unwrap_or_default();
            tf("unit.option", &[("unit", F::S(U::unit_label(u))), ("name", F::S(nm))])
        };
        egui::ComboBox::new("tk_i_unit", "")
            .selected_text(unit_text(&unit))
            .show_ui(ui, |ui| {
                for u in U::units_for(&ty) {
                    if ui.selectable_label(unit == *u, unit_text(u)).clicked() && unit != *u {
                        set.push(("unit".into(), (*u).to_string()));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        // second conditions
        ui.label(RichText::new(t("item.f.also")).small());
        let conds: Vec<Value> = it.get("conds").and_then(Value::as_array).cloned().unwrap_or_default();
        for k in TYPES.iter().filter(|k| **k != ty) {
            let c = conds.iter().find(|c| s(c.get("kind")) == *k);
            let n =
                if c.is_some() { self.tk.marks().iter().filter(|m| s(m.get("item")) == id && s(m.get("kind")) == *k).count() } else { 0 };
            ui.horizontal(|ui| {
                let mut checked = c.is_some();
                let tip = if n > 0 {
                    tn("item.cond.locked", n as f64)
                } else {
                    tf("item.cond.title", &[("type", F::S(type_label(k).to_lowercase()))])
                };
                if ui.add_enabled(n == 0, egui::Checkbox::new(&mut checked, type_label(k))).on_hover_text(tip).changed() {
                    set.push((format!("cond:{k}"), if checked { "on".into() } else { "off".into() }));
                }
                if let Some(c) = c {
                    let cu = s(c.get("unit"));
                    egui::ComboBox::new(format!("tk_cu_{k}"), "").selected_text(U::unit_label(&cu)).show_ui(ui, |ui| {
                        for u in U::units_for(k) {
                            if ui.selectable_label(cu == *u, U::unit_label(u)).clicked() && cu != *u {
                                set.push((format!("cond-unit:{k}"), (*u).to_string()));
                            }
                        }
                    });
                }
            });
        }
        let len_text = |ft: f64| -> String {
            if ft > 0.0 {
                if lu == "m" { format!("{} m", U::fixed(ft * U::M_PER_FT, 3)) } else { U::format_ft_in(ft, 16.0) }
            } else {
                String::new()
            }
        };
        if ty != "count" {
            let label = t(if ty == "linear" { "item.f.height" } else { "item.f.depth" });
            let hint = if lu == "m" { "3.0 m" } else { "9'-0\"" };
            if let Some(v) = self.text_field(ui, "height", &len_text(f(it.get("heightFt"))), &label, hint) {
                set.push(("height".into(), v));
            }
        }
        if ty == "area" {
            let mut bo = it.get("backout") == Some(&Value::Bool(true));
            if ui.checkbox(&mut bo, t("item.backout")).changed() {
                set.push(("backout".into(), if bo { "on".into() } else { "off".into() }));
            }
            if bo {
                ui.label(RichText::new(t("item.backout.of")).small());
                let of: Vec<String> =
                    it.get("backoutOf").and_then(Value::as_array).map(|l| l.iter().map(|v| s(Some(v))).collect()).unwrap_or_default();
                let targets: Vec<Value> = self
                    .tk
                    .items()
                    .iter()
                    .filter(|x| s(x.get("type")) == "area" && s(x.get("id")) != id && x.get("backout") != Some(&Value::Bool(true)))
                    .cloned()
                    .collect();
                if targets.is_empty() {
                    ui.label(RichText::new(t("item.backout.none")).color(tok.text_dim));
                }
                for x in targets {
                    let xid = s(x.get("id"));
                    let mut c = of.contains(&xid);
                    if ui.checkbox(&mut c, s(x.get("name"))).changed() {
                        set.push((format!("bo:{xid}"), if c { "on".into() } else { "off".into() }));
                    }
                }
                let bad =
                    Backouts::new(&self.tk.state).bad.iter().filter(|m| self.tk.mark(m).is_some_and(|m| s(m.get("item")) == id)).count();
                if bad > 0 {
                    ui.label(RichText::new(tn("item.backout.bad", bad as f64)).color(ui.visuals().error_fg_color));
                }
            }
        }
        if ty != "count" {
            let mut show_pitch = true;
            if ty == "linear" {
                let sl = it.get("slope").and_then(Value::as_str).unwrap_or("flat").to_string();
                let l = ui.label(RichText::new(t("slope.label")).small());
                egui::ComboBox::new("tk_i_slope", "")
                    .selected_text(t(&format!("slope.{sl}")))
                    .show_ui(ui, |ui| {
                        for k in ["flat", "rake", "hip"] {
                            if ui.selectable_label(sl == k, t(&format!("slope.{k}"))).clicked() && sl != k {
                                set.push(("slope".into(), k.into()));
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
                show_pitch = it.get("slope").is_some_and(|x| !x.is_null());
            }
            if show_pitch
                && let Some(v) =
                    self.text_field(ui, "pitch", &U::format_pitch(f(it.get("pitch"))), &t("pitch.label"), &t("pitch.placeholder"))
            {
                set.push(("pitch".into(), v));
            }
            ui.label(RichText::new(t("item.f.3d")).small().strong());
            for (field, key, label) in SHAPE {
                if (field == "thick" || field == "head") && ty != "linear" {
                    continue;
                }
                if let Some(v) = self.text_field(ui, field, &len_text(f(it.get(key))), &t(label), "") {
                    set.push((field.into(), v));
                }
            }
        }
        let ul = U::unit_label(&unit);
        if let Some(v) = self.text_field(ui, "waste", &U::percent_input(f(it.get("waste"))), &t("item.f.waste"), "") {
            set.push(("waste".into(), v));
        }
        if let Some(v) = self.text_field(ui, "mat", &U::rate_input(f(it.get("mat"))), &tf("item.f.mat", &[("unit", F::S(ul.clone()))]), "")
        {
            set.push(("mat".into(), v));
        }
        let q = quantities(&self.tk.state, &QOpts::default());
        // the panel's `est` is the base bid (baseEstimate: without the add alternates)
        let est = cc_takeoff::model::base_estimate(&self.tk.state);
        let row = est.rows.iter().find(|r| r.get("kind").and_then(Value::as_str) == Some("item") && s(r.get("itemId")) == id).cloned();
        let crew = it.get("crew").filter(|c| c.is_object()).cloned();
        if crew.is_some() {
            let lab = row.as_ref().map_or(0.0, |r| f(r.get("lab")));
            ui.add_enabled_ui(false, |ui| {
                let _ = self.text_field(ui, "lab-locked", &U::rate_input(lab), &tf("item.f.lab", &[("unit", F::S(ul.clone()))]), "");
            });
        } else if let Some(v) =
            self.text_field(ui, "lab", &U::rate_input(f(it.get("lab"))), &tf("item.f.lab", &[("unit", F::S(ul.clone()))]), "")
        {
            set.push(("lab".into(), v));
        }
        // labour by crew (T18)
        let mut crew_on = crew.is_some();
        if ui.checkbox(&mut crew_on, t("crew.on")).on_hover_text(t("crew.title")).changed() {
            set.push(("crew".into(), if crew_on { "on".into() } else { "off".into() }));
        }
        if let Some(c) = &crew {
            let prod = cc_takeoff::jv::num_str(U::round_to(f(c.get("prod")), 4));
            if let Some(v) = self.text_field(ui, "crew-prod", &prod, &tf("crew.prod", &[("unit", F::S(ul.clone()))]), "") {
                set.push(("crew-prod".into(), v));
            }
            if let Some(v) = self.text_field(ui, "crew-size", &cc_takeoff::jv::num_str(f(c.get("size"))), &t("crew.size"), "") {
                set.push(("crew-size".into(), v));
            }
            if let Some(v) = self.text_field(ui, "crew-wage", &U::rate_input(f(c.get("wage"))), &t("crew.wage"), "") {
                set.push(("crew-wage".into(), v));
            }
            if let Some(w) = row.as_ref().and_then(|r| r.get("work")).filter(|w| w.is_object()) {
                let r = row.as_ref().map_or(&Value::Null, |r| r);
                let fills = [
                    ("hours", F::S(U::fixed(f(w.get("hours")), 1))),
                    ("wh", F::S(U::fixed(f(w.get("workerHours")), 1))),
                    ("days", F::S(U::fixed(f(w.get("days")), 1))),
                    ("lab", F::S(U::format_cents(f(r.get("labExt"))))),
                    ("rate", F::S(U::format_rate(f(r.get("lab"))))),
                    ("unit", F::S(ul.clone())),
                ];
                ui.label(RichText::new(tf("crew.sum", &fills)).color(tok.text_dim));
            }
        }
        if let Some(v) =
            self.text_field(ui, "csi", it.get("csi").and_then(Value::as_str).unwrap_or(""), &t("item.f.csi"), &t("item.csi.ph"))
        {
            set.push(("csi".into(), v));
        }
        // measured
        ui.add_space(4.0);
        ui.label(RichText::new(t("item.measured")).small().strong());
        let tot = q.get(&id).map(|x| x.tot.clone()).unwrap_or_default();
        let vars: HashMap<String, f64> = item_vars(&it, &tot).into_iter().collect();
        let var = |k: &str| vars.get(k).copied().unwrap_or(0.0);
        let lunit = if lu == "m" { "m" } else { "LF" };
        let kv = |ui: &mut egui::Ui, k: String, v: String| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(k).color(tok.text_dim));
                ui.label(v);
            });
        };
        match ty.as_str() {
            "linear" => {
                let extra = if lu == "ft" { format!("  ({})", U::format_ft_in(tot.length_ft, 16.0)) } else { String::new() };
                kv(ui, t("item.m.length"), format!("{} {lunit}{extra}", U::fixed(var("L"), 2)));
                kv(ui, t("item.m.runs"), cc_takeoff::jv::num_str(var("N")));
                if f(it.get("heightFt")) > 0.0 {
                    kv(ui, t("item.m.wall"), format!("{} {}", U::fixed(var("A"), 2), U::area_unit_of(&unit)));
                }
            }
            "area" => {
                kv(ui, t("item.m.area"), format!("{} {ul}", U::fixed(var("A"), 2)));
                kv(ui, t("item.m.perimeter"), format!("{} {lunit}", U::fixed(var("P"), 2)));
                kv(ui, t("item.m.areas"), cc_takeoff::jv::num_str(var("N")));
            }
            _ => kv(ui, t("item.m.count"), cc_takeoff::jv::num_str(var("N"))),
        }
        // assembly
        ui.add_space(4.0);
        let mut comp_act: Vec<String> = Vec::new();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("asm.h")).strong().color(tok.text_strong));
            if ui.small_button(format!("+ {}", t("asm.add"))).clicked() {
                comp_act.push("add".into());
            }
        });
        let vl = vars_of(&ty);
        ui.label(RichText::new(format!("{} {}{}", t("asm.vars.a"), vl.join(", "), t("asm.vars.b"))).small().color(tok.text_dim));
        let comps: Vec<Value> = it.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
        let values: Vec<(String, f64)> = vars.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for c in comps {
            let cid = s(c.get("id"));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                for (field, label, value) in [
                    ("name", t("item.f.name"), s(c.get("name"))),
                    ("unit", t("item.f.unit"), s(c.get("unit"))),
                    ("formula", t("comp.formula"), s(c.get("formula"))),
                ] {
                    if let Some(v) = self.text_field(ui, &format!("c:{cid}:{field}"), &value, &label, "") {
                        comp_act.push(format!("{cid}:{field}={v}"));
                    }
                    if field == "formula" {
                        // the result as typed (show()): the live text, not only the saved formula
                        let typed = self.edits.get(&format!("c:{cid}:formula")).map_or(value.clone(), |e| e.1.clone());
                        let (txt, bad) = match cc_takeoff::formula::parse(&typed, vl) {
                            Err(e) => (e.message, true),
                            Ok(p) => match cc_takeoff::formula::evaluate(&p.ast, &|k| values.iter().find(|(n, _)| n == k).map(|(_, v)| *v))
                            {
                                Ok(v) => {
                                    let fx = U::fixed(v, 3);
                                    let trimmed =
                                        if fx.contains('.') { fx.trim_end_matches('0').trim_end_matches('.').to_string() } else { fx };
                                    (format!("= {trimmed} {}", s(c.get("unit"))), false)
                                }
                                Err(e) => (e.message, true),
                            },
                        };
                        ui.label(RichText::new(txt).color(if bad { ui.visuals().error_fg_color } else { tok.text_dim }));
                    }
                }
                for (field, label, value) in [
                    ("waste", t("item.f.waste"), U::percent_input(f(c.get("waste")))),
                    ("mat", t("comp.mat"), U::rate_input(f(c.get("mat")))),
                    ("lab", t("comp.lab"), U::rate_input(f(c.get("lab")))),
                ] {
                    if let Some(v) = self.text_field(ui, &format!("c:{cid}:{field}"), &value, &label, "") {
                        comp_act.push(format!("{cid}:{field}={v}"));
                    }
                }
                if ui.small_button(t("comp.remove")).clicked() {
                    comp_act.push(format!("remove:{cid}"));
                }
            });
        }
        let mut act: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            for (a, k) in [("dup", "item.dup"), ("up", "item.up"), ("down", "item.down"), ("delete", "item.delete")] {
                if ui.small_button(t(k)).clicked() {
                    act = Some(a);
                }
            }
        });
        for (k, v) in set {
            let _ = self.item_field(&k, &v);
        }
        for a in comp_act {
            let _ = self.comp_edit(&a);
        }
        if let Some(a) = act {
            let _ = self.item_action(a);
        }
    }

    /// The selected mark (renderMarkProps).
    pub fn mark_props(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let Some(id) = self.sel.clone() else { return };
        let Some(m) = self.tk.mark(&id).cloned() else {
            self.sel = None;
            return;
        };
        let kind = s(m.get("kind"));
        let item = s(m.get("item"));
        let it = self.tk.item(&item).cloned().unwrap_or(Value::Null);
        let fpu = self.ft_per_unit(&s(m.get("sheet"))).unwrap_or(0.0);
        let r = measure_mark(&m, fpu, Some(&it));
        let mut set: Vec<(String, String)> = Vec::new();
        ui.label(RichText::new(t("mark.h")).strong().color(tok.text_strong));
        let items: Vec<Value> = self.tk.items().iter().filter(|i| kinds_of(Some(i)).contains(&kind)).cloned().collect();
        let l = ui.label(RichText::new(t("mark.f.item")).small());
        egui::ComboBox::new("tk_m_item", "")
            .selected_text(s(it.get("name")))
            .show_ui(ui, |ui| {
                for i in &items {
                    let iid = s(i.get("id"));
                    if ui.selectable_label(iid == item, s(i.get("name"))).clicked() && iid != item {
                        set.push(("item".into(), iid));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        let kv = |ui: &mut egui::Ui, k: String, v: String| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k).color(tok.text_dim));
                ui.label(v);
            });
        };
        kv(ui, t("mark.f.sheet"), self.sheet_label(&s(m.get("sheet"))));
        let areas: Vec<Value> = self.tk.state.get("areas").and_then(Value::as_array).cloned().unwrap_or_default();
        if !areas.is_empty() {
            let cur = s(m.get("area"));
            let shown = areas.iter().find(|a| s(a.get("id")) == cur).map_or_else(|| t("area.none"), |a| s(a.get("name")));
            let l = ui.label(RichText::new(t("props.area")).small());
            egui::ComboBox::new("tk_m_area", "")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(cur.is_empty(), t("area.none")).clicked() && !cur.is_empty() {
                        set.push(("area".into(), String::new()));
                    }
                    for a in &areas {
                        let aid = s(a.get("id"));
                        if ui.selectable_label(aid == cur, s(a.get("name"))).clicked() && aid != cur {
                            set.push(("area".into(), aid));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
        }
        let ft_in = |v: f64| U::format_ft_in(v, 16.0);
        let pitch_field = |me: &mut Self, ui: &mut egui::Ui, set: &mut Vec<(String, String)>| {
            let own_slopes = m
                .get("slopes")
                .and_then(Value::as_array)
                .is_some_and(|l| l.iter().any(|x| x.get("k").and_then(Value::as_str).is_some_and(|k| k != "flat")));
            if kind == "linear" && it.get("slope").is_none_or(Value::is_null) && !own_slopes {
                return;
            }
            let v = match m.get("pitch").and_then(Value::as_f64) {
                Some(p) => {
                    let s2 = U::format_pitch(p);
                    if s2.is_empty() { "0".into() } else { s2 }
                }
                None => String::new(),
            };
            let ip = f(it.get("pitch"));
            let hint = if ip > 0.0 { tf("pitch.item", &[("pitch", F::S(U::format_pitch(ip)))]) } else { t("pitch.flat") };
            if let Some(x) = me.text_field(ui, &format!("m-pitch:{id}"), &v, &t("pitch.mark"), &hint) {
                set.push(("pitch".into(), x));
            }
        };
        match (&r, kind.as_str()) {
            (None, _) => kv(ui, t("item.measured"), t("mark.noscale")),
            (Some(Measure::Linear { length_ft, gross_ft, deduct_ft, plan_ft, segments_ft, sloped_ft, slopes, deducts }), _) => {
                kv(ui, t("mark.f.length"), tf("mark.length", &[("len", F::S(ft_in(*length_ft))), ("lf", F::S(U::fixed(*length_ft, 2)))]));
                kv(ui, t("mark.f.segments"), segments_ft.len().to_string());
                if !deducts.is_empty() {
                    kv(ui, t("props.deducts"), ft_in(*deduct_ft));
                    kv(ui, t("props.drawn"), ft_in(*gross_ft));
                }
                if f(it.get("heightFt")) > 0.0 && s(it.get("type")) == kind {
                    kv(ui, t("mark.f.wall"), format!("{} SF", U::fixed(length_ft * f(it.get("heightFt")), 2)));
                }
                if slopes.iter().any(|x| x.0 != "flat") {
                    kv(ui, t("props.plan_length"), ft_in(*plan_ft));
                }
                pitch_field(self, ui, &mut set);
                let pts = m.get("pts").and_then(Value::as_array).cloned().unwrap_or_default();
                let item_slope = it.get("slope").and_then(Value::as_str).unwrap_or("flat").to_string();
                for (i, seg) in segments_ft.iter().enumerate() {
                    #[allow(clippy::cast_precision_loss)]
                    let is_ded = deducts.contains(&(i as f64));
                    let (k, p) = slopes.get(i).cloned().unwrap_or(("flat".into(), 0.0));
                    let len = if k == "flat" {
                        ft_in(*seg)
                    } else {
                        let fills = [
                            ("len", F::S(ft_in(sloped_ft.get(i).copied().unwrap_or(0.0)))),
                            ("kind", F::S(t(&format!("slope.{k}")).to_lowercase())),
                            ("pitch", F::S(U::format_pitch(p))),
                        ];
                        tf("slope.seg.len", &fills)
                    };
                    let arc = pts.get(i).and_then(|q| q.get("b")).and_then(Value::as_f64).is_some_and(|b| b != 0.0);
                    let own = m
                        .get("slopes")
                        .and_then(Value::as_array)
                        .and_then(|l| l.get(i))
                        .and_then(|x| x.get("k"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    ui.horizontal_wrapped(|ui| {
                        let text = format!("{}. {len}{}", i + 1, if arc { format!(" {}", t("mark.arc")) } else { String::new() });
                        ui.label(if is_ded { RichText::new(text).strikethrough().color(tok.text_dim) } else { RichText::new(text) });
                        let opt = |v: &str| {
                            if v.is_empty() {
                                tf("slope.item", &[("kind", F::S(t(&format!("slope.{item_slope}"))))])
                            } else {
                                t(&format!("slope.{v}"))
                            }
                        };
                        egui::ComboBox::new(format!("tk_sl_{i}"), "").selected_text(opt(&own)).show_ui(ui, |ui| {
                            for v in ["", "flat", "rake", "hip"] {
                                if ui.selectable_label(own == v, opt(v)).clicked() && own != v {
                                    set.push((format!("sl:{i}"), v.into()));
                                }
                            }
                        });
                        let mut d = is_ded;
                        if ui.checkbox(&mut d, t("props.deduct")).changed() {
                            set.push((format!("ded:{i}"), if d { "on".into() } else { "off".into() }));
                        }
                    });
                }
            }
            (Some(Measure::Area { area_sqft, plan_sqft, factor, gross_sqft, cut_sqft, perimeter_ft, .. }), _) => {
                let backout = it.get("backout") == Some(&Value::Bool(true));
                let cut = if backout { 0.0 } else { Backouts::new(&self.tk.state).cut(&m, fpu) * factor };
                kv(ui, t("mark.f.area"), format!("{} SF", U::fixed((area_sqft - cut).max(0.0), 2)));
                if cut > 0.0 {
                    kv(ui, t("props.backouts"), format!("−{} SF", U::fixed(cut, 2)));
                }
                if *factor > 1.0 {
                    kv(ui, t("props.plan_area"), format!("{} SF", U::fixed((plan_sqft - cut / factor).max(0.0), 2)));
                    kv(ui, t("props.slope_factor"), U::fixed(*factor, 4));
                }
                pitch_field(self, ui, &mut set);
                if backout && !G::is_convex(&pts_p(&m, "pts")) {
                    ui.label(RichText::new(t("backout.mark.convex")).color(ui.visuals().error_fg_color));
                }
                let nh = m.get("holes").and_then(Value::as_array).map_or(0, Vec::len);
                if nh > 0 {
                    kv(ui, t("mark.f.outline"), format!("{} SF", U::fixed(*gross_sqft, 2)));
                    kv(ui, t("mark.f.cutouts"), tf("mark.cutouts", &[("n", F::N(nh as f64)), ("sf", F::S(U::fixed(*cut_sqft, 2)))]));
                    kv(ui, t("mark.f.perimeter"), tf("mark.perimeter.holes", &[("len", F::S(ft_in(*perimeter_ft)))]));
                } else {
                    kv(ui, t("mark.f.perimeter"), ft_in(*perimeter_ft));
                }
            }
            (Some(_), _) => {
                kv(ui, t("mark.f.count"), "1".into());
                let z = m.get("z").and_then(Value::as_f64).map(|z| cc_takeoff::jv::num_str(U::round_to(z, 4))).unwrap_or_default();
                if let Some(v) = self.text_field(ui, &format!("m-z:{id}"), &z, &t("spots.field"), &t("spots.ph")) {
                    set.push(("z".into(), v));
                }
            }
        }
        let mut act: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            if kind == "area" {
                if ui.small_button(t("mark.cutout.add")).clicked() {
                    act = Some("cutout");
                }
                if m.get("holes").and_then(Value::as_array).is_some_and(|h| !h.is_empty())
                    && ui.small_button(t("mark.cutout.remove")).clicked()
                {
                    act = Some("cutout.remove");
                }
            }
            if ui.small_button(t("mark.delete")).clicked() {
                act = Some("delete");
            }
        });
        for (k, v) in set {
            let _ = self.mark_field(&k, &v);
        }
        match act {
            Some("cutout") => self.set_tool("cutout"),
            Some(a) => {
                let _ = self.mark_field(a, "");
            }
            None => {}
        }
    }

    /// The tool (select, draw with the item, cut-out of the selected area).
    pub fn set_tool(&mut self, tool: &'static str) {
        self.tool = tool;
        self.pts.clear();
        self.ded.clear();
        if tool == "cutout" {
            self.message = t("tool.cutout.title");
        }
    }

    /// Quantities by bid area with the alternates under them (buildAreaTable), as text rows.
    pub fn by_area(&self, ui: &mut egui::Ui, tok: &Tokens) {
        if self.tk.state.get("areas").and_then(Value::as_array).is_none_or(Vec::is_empty) {
            return;
        }
        ui.label(RichText::new(t("qty.area.col")).strong().color(tok.text_strong));
        let mut any = false;
        egui::Grid::new("tk_by_area").striped(true).num_columns(4).show(ui, |ui| {
            for (area, e) in area_estimates(&self.tk.state) {
                let name = match &area {
                    Some(a) => match a.get("alt").and_then(Value::as_str).filter(|x| !x.is_empty()) {
                        Some(alt) => {
                            tf("qty.alt.row", &[("name", F::S(s(a.get("name")))), ("kind", F::S(t(&format!("alt.{alt}")).to_lowercase()))])
                        }
                        None => s(a.get("name")),
                    },
                    None => t("area.none"),
                };
                for r in &e.rows {
                    let k = r.get("kind").and_then(Value::as_str).unwrap_or("");
                    if !(k == "item" || k == "condition") || f(r.get("marks")) <= 0.0 {
                        continue;
                    }
                    any = true;
                    let cond = r.get("cond").and_then(Value::as_str);
                    let nm = match cond {
                        Some(c) => tf("qty.cond.name", &[("item", F::S(s(r.get("name")))), ("type", F::S(type_label(c).to_lowercase()))]),
                        None => s(r.get("name")),
                    };
                    let ity = self.tk.item(&s(r.get("itemId"))).map(|i| s(i.get("type"))).unwrap_or_default();
                    let digits = if cond.unwrap_or(&ity) == "count" { 0 } else { 2 };
                    ui.label(&name);
                    ui.label(nm);
                    ui.label(format!("{} {}", U::fixed(f(r.get("qty")), digits), s(r.get("unit"))));
                    ui.label(U::format_cents(f(r.get("total"))));
                    ui.end_row();
                }
                ui.label(RichText::new(tf("qty.area.subtotal", &[("area", F::S(name))])).strong());
                ui.label("");
                ui.label("");
                ui.label(RichText::new(U::format_cents(e.total)).strong());
                ui.end_row();
            }
            let alts = alternates(&self.tk.state);
            if !alts.is_empty() {
                let base = cc_takeoff::model::base_estimate(&self.tk.state);
                ui.label(RichText::new(t("qty.alt.base")).strong());
                ui.label("");
                ui.label("");
                ui.label(U::format_cents(base.total));
                ui.end_row();
                for a in alts {
                    let kind = t(&format!("alt.{}", a.alt)).to_lowercase();
                    ui.label(tf("qty.alt.row", &[("name", F::S(s(a.area.get("name")))), ("kind", F::S(kind))]));
                    ui.label("");
                    ui.label("");
                    let sign = if a.amount > 0.0 {
                        "+"
                    } else if a.amount < 0.0 {
                        "−"
                    } else {
                        ""
                    };
                    ui.label(format!("{sign}{}", U::format_cents(a.amount.abs())));
                    ui.end_row();
                }
            }
        });
        if !any {
            ui.label(RichText::new(t("qty.area.empty")).color(tok.text_dim));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> (TakeoffMode, String) {
        let mut m = TakeoffMode::new("abc");
        m.add_file("Plan.pdf", 10, 1);
        let sheet = m.sheet(0);
        m.set_preset(&sheet, "a1/4", 1.0);
        (m, sheet)
    }

    #[test]
    fn item_fields_are_parsed_as_the_js_does_and_each_is_one_undo_step() {
        let (mut m, _) = mode();
        m.add_item("Walls", "linear");
        let n0 = m.tk.undo_stack.len();
        m.item_field("mat", "$1,234.5").unwrap();
        m.item_field("waste", "7.5%").unwrap();
        m.item_field("height", "9'-0\"").unwrap();
        m.item_field("slope", "rake").unwrap();
        m.item_field("pitch", "6:12").unwrap();
        assert!(m.item_field("pitch", "banana").is_err());
        assert!(m.item_field("name", "  ").is_err());
        assert_eq!(m.tk.undo_stack.len(), n0 + 5);
        let it = m.tk.item(m.item.as_deref().unwrap()).unwrap();
        assert_eq!(it["mat"], json!(12_345_000.0));
        assert_eq!(it["waste"], json!(750.0));
        assert_eq!(it["heightFt"], json!(9.0));
        assert_eq!(it["slope"], json!("rake"));
        assert_eq!(it["pitch"], json!(6.0));
        m.tk.undo();
        assert!(m.tk.item(m.item.as_deref().unwrap()).unwrap().get("pitch").is_none());
    }

    #[test]
    fn assemblies_bid_areas_and_alternates() {
        let (mut m, sheet) = mode();
        m.add_item("Outlets", "count");
        m.item_field("mat", "10").unwrap();
        m.comp_edit("add").unwrap();
        let cid = m.tk.item(m.item.as_deref().unwrap()).unwrap()["components"][0]["id"].as_str().unwrap().to_string();
        m.comp_edit(&format!("{cid}:formula=N * 2")).unwrap();
        m.comp_edit(&format!("{cid}:mat=1.5")).unwrap();
        m.area_edit("new:Building A").unwrap();
        let a = m.cur_area.clone().unwrap();
        m.click(&sheet, [10.0, 10.0]);
        m.click(&sheet, [50.0, 10.0]);
        assert!(m.tk.marks().iter().all(|k| k["area"] == json!(a)));
        m.area_edit(&format!("alt:{a}:add")).unwrap();
        let alts = alternates(&m.tk.state);
        assert_eq!(alts.len(), 1);
        assert!(alts[0].amount > 0.0);
        // the base bid leaves the add alternate out
        assert_eq!(cc_takeoff::model::base_estimate(&m.tk.state).total, 0.0);
        m.area_edit(&format!("delete:{a}")).unwrap();
        assert!(m.cur_area.is_none());
        assert!(m.tk.marks().iter().all(|k| k.get("area").is_none()));
    }

    #[test]
    fn deducts_and_segment_slopes_on_a_selected_run() {
        let (mut m, sheet) = mode();
        m.add_item("Walls", "linear");
        m.px = 0.5;
        for p in [[0.0, 0.0], [180.0, 0.0]] {
            m.click(&sheet, p);
        }
        m.toggle_deduct(true);
        m.click(&sheet, [180.0, 36.0]);
        m.toggle_deduct(false);
        m.click(&sheet, [360.0, 36.0]);
        m.finish(&sheet);
        let id = m.sel.clone().unwrap();
        assert_eq!(m.tk.mark(&id).unwrap()["deducts"], json!([1.0]));
        // every segment a deduct is refused
        m.mark_field("ded:0", "on").unwrap();
        assert!(m.mark_field("ded:2", "on").is_err());
        m.mark_field("ded:0", "off").unwrap();
        m.mark_field("sl:2", "rake").unwrap();
        assert_eq!(m.tk.mark(&id).unwrap()["slopes"], json!([null, null, { "k": "rake" }]));
        // picking: on the run, not beside it
        assert_eq!(m.mark_at(&sheet, [90.0, 1.0], 3.0), Some(id.clone()));
        assert_eq!(m.mark_at(&sheet, [90.0, 20.0], 3.0), None);
    }

    #[test]
    fn a_cut_out_goes_into_the_selected_area() {
        let (mut m, sheet) = mode();
        m.add_item("Floor", "area");
        for p in [[0.0, 0.0], [200.0, 0.0], [200.0, 200.0], [0.0, 200.0]] {
            m.click(&sheet, p);
        }
        m.finish(&sheet);
        let id = m.sel.clone().unwrap();
        m.set_tool("cutout");
        for p in [[50.0, 50.0], [100.0, 50.0], [100.0, 100.0]] {
            m.click(&sheet, p);
        }
        m.finish(&sheet);
        assert_eq!(m.tk.mark(&id).unwrap()["holes"].as_array().map(Vec::len), Some(1));
        m.mark_field("cutout.remove", "").unwrap();
        assert_eq!(m.tk.mark(&id).unwrap()["holes"].as_array().map(Vec::len), Some(0));
    }
}
