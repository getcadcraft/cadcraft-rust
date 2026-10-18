//! PartCraft's feature tools in the app (M13.4): partcraft.js `newFeature` (extrude, cut-extrude, revolve,
//! revolve-cut, hole, linear / circular pattern, mirror, fillet, chamfer, shell), picking edges and faces for them
//! (`startPick` / `pickRef` / `endPick`, Shift+click collects), editing a feature's values (`featureProps` with
//! `setFeature`), delete (with what depends on it), move up / down, rename and the sketch dimensions — each one undo
//! step, with 0.5's words. Everything is reachable as a `part.*` command (partview.rs `run`).

use cc_part::{jv, params as PR, params_part as PP, part as P, sketch as S};
use serde_json::Value;

use crate::partview::{Mode, PartView, Pick, tr, trv};

/// partcraft.js `SOLID`: features that make solid.
pub const SOLID: [&str; 9] = ["extrude", "revolve", "hole", "lpattern", "cpattern", "mirror", "import", "sweep", "loft"];
/// partcraft.js `PICKERS` (with the mesh kernel: the ones it builds, `PICK_ON_MESH`).
pub const PICKERS: [&str; 5] = ["fillet", "chamfer", "shell", "draft", "thread"];
/// partcraft.js `FACE_PICKERS`.
pub const FACE_PICKERS: [&str; 3] = ["shell", "draft", "thread"];

/// The feature tool bar commands (partcraft.js `run`): command, feature type, catalog key.
pub const FEATURE_CMDS: &[(&str, &str, &str)] = &[
    ("extrude", "extrude", "ui.tb.features.extrude"),
    ("cut-extrude", "extrude", "ui.tb.features.cut_extrude"),
    ("revolve", "revolve", "ui.tb.features.revolve"),
    ("revolve-cut", "revolve", "ui.tb.fmore.revolve_cut"),
    ("hole", "hole", "ui.tb.features.hole"),
    ("lpattern", "lpattern", "ui.tb.features.lpattern"),
    ("cpattern", "cpattern", "ui.tb.features.cpattern"),
    ("mirror", "mirror", "ui.tb.features.mirror"),
    ("fillet", "fillet", "ui.tb.features.fillet"),
    ("chamfer", "chamfer", "ui.tb.features.chamfer"),
    ("shell", "shell", "ui.tb.features.shell"),
];

/// partcraft.js `MAIN_NUMBER`.
pub fn main_number(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "extrude" => "depth",
        "revolve" => "angle",
        "hole" => "d",
        "lpattern" | "cpattern" => "count",
        "fillet" => "r",
        "chamfer" => "d",
        "shell" => "t",
        "draft" => "angle",
        "rib" => "t",
        "plane" => "d",
        "thread" => "length",
        _ => return None,
    })
}

fn field_label(ty: &str, field: &str) -> Option<String> {
    let f = PP::FIELDS.iter().find(|f| f.ty == ty && f.field == field)?;
    Some(match f.label {
        PP::Label::Key(k) | PP::Label::Reg(k) => tr(k),
        PP::Label::Plain(s) => s.to_string(),
        PP::Label::Axis(a) => a.to_string(),
    })
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    jv::obj(pairs)
}

impl PartView {
    fn result_status(&self, id: &str) -> Option<&str> {
        self.regen.results.get(id).and_then(|r| r.get("status")).and_then(Value::as_str)
    }

    /// The feature's build error in words, if it failed (partcraft.js `app.feature.error`).
    fn error_words(&self, id: &str) -> Option<String> {
        let r = self.regen.results.get(id).filter(|r| jv::is_str(r.get("status"), "error"))?;
        let f = P::feature_by_id(&self.part, id)?;
        Some(trv("app.feature.error", &[("name", jv::s(f, "name").to_string()), ("error", jv::s(r, "message").to_string())]))
    }

    fn named(&self) -> bool {
        self.regen.faces.is_some()
    }

    /// partcraft.js `chooseSketch(need)`: the sketch a new extrude / revolve / hole uses.
    fn choose_sketch(&mut self, points: bool) -> Option<String> {
        if self.mode == Mode::Sketch {
            let id = self.editing.clone();
            self.exit_sketch();
            return id;
        }
        if let Some(id) = self.sel_feature.clone()
            && P::feature_by_id(&self.part, &id).is_some_and(|f| jv::s(f, "type") == "sketch")
        {
            return Some(id);
        }
        let used: Vec<String> =
            P::features(&self.part).iter().filter_map(|f| f.get("sketch").and_then(Value::as_str).map(str::to_string)).collect();
        let built = cc_part::js::idx(P::built_count(&self.part)).unwrap_or(0);
        let fits = |f: &&Value| {
            let sk = f.get("sketch").unwrap_or(&Value::Null);
            jv::s(f, "type") == "sketch"
                && !used.iter().any(|u| u == jv::s(f, "id"))
                && self.result_status(jv::s(f, "id")) == Some("ok")
                && if points {
                    !S::loose_points(sk).is_empty()
                } else {
                    S::entities(sk).iter().any(|e| jv::s(e, "type") != "point" && !jv::truthy(e.get("construction")))
                }
        };
        P::features(&self.part).iter().take(built).rev().find(fits).map(|f| jv::s(f, "id").to_string())
    }

    /// A feature tool bar command (partcraft.js `run(cmd)` → `newFeature(type, props)`).
    pub fn feature_cmd(&mut self, cmd: &str) -> Result<(), String> {
        let (ty, props) = match cmd {
            "extrude" => ("extrude", obj(vec![("op", "add".into())])),
            "cut-extrude" => ("extrude", obj(vec![("op", "cut".into()), ("end", "through".into())])),
            "revolve" => ("revolve", obj(vec![("op", "add".into())])),
            "revolve-cut" => ("revolve", obj(vec![("op", "cut".into())])),
            "hole" | "lpattern" | "cpattern" | "mirror" | "fillet" | "chamfer" | "shell" => (cmd, obj(vec![])),
            other => {
                return Err(format!(
                    "part.feature: unknown tool {other} (extrude, cut-extrude, revolve, revolve-cut, hole, lpattern, cpattern, mirror, fillet, chamfer, shell)"
                ));
            }
        };
        self.new_feature(ty, props);
        Ok(())
    }

    /// partcraft.js `newFeature(type, props)`.
    pub fn new_feature(&mut self, ty: &str, props: Value) {
        let label = P::feature_label(ty);
        let mut props = props;
        if PICKERS.contains(&ty) {
            if !self.named() {
                self.note = trv("app.feature.needs_exact_off", &[("what", label)]);
                return;
            }
            if self.regen.mesh.is_none() {
                self.note = tr("app.feature.build_solid_first");
                return;
            }
            let want_face = FACE_PICKERS.contains(&ty);
            let mut cand: Vec<Pick> = self.multi.clone();
            if self.multi.is_empty()
                && let Some(p) = self.pick.clone()
            {
                cand.push(p);
            }
            let picked: Vec<Pick> = cand
                .into_iter()
                .filter(|m| m.face == want_face && m.rf.is_some())
                .filter(|m| {
                    !want_face
                        || m.rf.as_ref().is_some_and(|r| jv::truthy(r.get("name")))
                        || self.face(m.index).is_some_and(|f| jv::truthy(f.get("name")))
                })
                .filter(|m| ty != "thread" || self.face(m.index).is_some_and(|f| jv::s(f, "type") == "cylinder"))
                .collect();
            let refs: Vec<Value> = picked
                .iter()
                .filter_map(|m| {
                    let r = m.rf.clone()?;
                    if want_face && !jv::truthy(r.get("name")) { self.face(m.index).map(crate::partview::face_ref) } else { Some(r) }
                })
                .collect();
            let mut full = obj(vec![(if want_face { "faces" } else { "edges" }, Value::Array(refs))]);
            if let Some(o) = props.as_object() {
                for (k, v) in o {
                    jv::set(&mut full, k, v.clone());
                }
            }
            let mut id = String::new();
            if let Err(e) = self.commit(&label, |p| {
                let f = P::add_feature(p, ty, &full)?;
                id = jv::s(&f, "id").to_string();
                Ok(())
            }) {
                self.note = e;
                return;
            }
            self.multi.clear();
            self.pick = None;
            self.sel_face = None;
            self.sel_feature = Some(id.clone());
            self.sel_plane = None;
            self.retint();
            if picked.is_empty() {
                self.start_pick(&id);
                return;
            }
            let name = P::feature_by_id(&self.part, &id).map(|f| jv::s(f, "name").to_string()).unwrap_or_default();
            self.note = self.error_words(&id).unwrap_or_else(|| {
                trv(
                    if want_face { "app.feature.added_faces" } else { "app.feature.added_edges" },
                    &[("name", name), ("n", picked.len().to_string())],
                )
            });
            return;
        }
        if matches!(ty, "extrude" | "revolve" | "hole") {
            let Some(sk) = self.choose_sketch(ty == "hole") else {
                self.note = tr(if ty == "hole" { "app.feature.need_point_sketch" } else { "app.feature.need_sketch" });
                return;
            };
            let mut full = obj(vec![("sketch", sk.into())]);
            if let Some(o) = props.as_object() {
                for (k, v) in o {
                    jv::set(&mut full, k, v.clone());
                }
            }
            if ty == "hole" {
                jv::set(&mut full, "through", true.into());
            }
            props = full;
        } else {
            let seed_from = self.sel_feature.as_deref().and_then(|id| P::feature_by_id(&self.part, id)).cloned();
            let built_n = cc_part::js::idx(P::built_count(&self.part)).unwrap_or(0);
            let built: Vec<Value> = P::features(&self.part)
                .iter()
                .take(built_n)
                .filter(|f| SOLID.contains(&jv::s(f, "type")) && self.result_status(jv::s(f, "id")) == Some("ok"))
                .cloned()
                .collect();
            if ty == "mirror" && built.is_empty() {
                self.note = tr("app.feature.build_solid_first");
                return;
            }
            let from_solid = seed_from.as_ref().filter(|f| SOLID.contains(&jv::s(f, "type")));
            let seed = from_solid.or(built.last());
            if seed.is_none() && ty != "mirror" {
                self.note = tr("app.feature.build_feature_first");
                return;
            }
            let seeds = if ty == "mirror" {
                match from_solid {
                    Some(f) => Value::Array(vec![jv::s(f, "id").into()]),
                    None => "body".into(),
                }
            } else {
                Value::Array(vec![seed.map(|f| jv::s(f, "id").to_string()).unwrap_or_default().into()])
            };
            let mut full = obj(vec![("seeds", seeds)]);
            if let Some(o) = props.as_object() {
                for (k, v) in o {
                    jv::set(&mut full, k, v.clone());
                }
            }
            if ty == "mirror"
                && let Some(p) = self.pick.as_ref().filter(|p| p.face && p.flat)
            {
                jv::set(&mut full, "plane", p.rf.clone().unwrap_or(Value::Null));
            }
            if ty == "cpattern"
                && let Some(rev) = built.iter().rev().find(|f| jv::s(f, "type") == "revolve")
            {
                jv::set(&mut full, "axis", obj(vec![("kind", "axis".into()), ("feature", jv::s(rev, "id").into())]));
            }
            if let Some(p) = self.pick.as_ref().filter(|p| !p.face)
                && (ty == "lpattern" || ty == "cpattern")
            {
                let d = cc_part::vec::norm(cc_part::vec::sub(p.b, p.a));
                jv::set(
                    &mut full,
                    if ty == "lpattern" { "dir" } else { "axis" },
                    obj(vec![("kind", "fixed".into()), ("o", jv::pt3(p.a)), ("d", jv::pt3(d))]),
                );
            }
            props = full;
        }
        let first = self.regen.mesh.is_none();
        let mut id = String::new();
        if let Err(e) = self.commit(&label, |p| {
            let f = P::add_feature(p, ty, &props)?;
            id = jv::s(&f, "id").to_string();
            Ok(())
        }) {
            self.note = e;
            return;
        }
        self.sel_feature = Some(id.clone());
        self.sel_plane = None;
        self.pick = None;
        self.sel_face = None;
        self.retint();
        let name = P::feature_by_id(&self.part, &id).map(|f| jv::s(f, "name").to_string()).unwrap_or_default();
        let what = main_number(ty).and_then(|m| field_label(ty, m));
        self.note = self.error_words(&id).unwrap_or_else(|| match what {
            Some(w) => trv("app.feature.added_type", &[("name", name.clone()), ("what", w)]),
            None => trv("app.feature.added", &[("name", name.clone())]),
        });
        if first && self.regen.mesh.is_some() && self.mode == Mode::Model {
            self.fit();
        }
    }

    /// partcraft.js `setFeature(id, label, fn)`: one undo step on feature `id`; a build error is said.
    pub fn set_feature(&mut self, id: &str, label: &str, f: impl FnOnce(&mut Value)) -> Result<(), String> {
        let id2 = id.to_string();
        self.commit(label, |p| {
            if let Some(g) =
                p.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.iter_mut().find(|g| jv::is_str(g.get("id"), &id2)))
            {
                f(g);
            }
            Ok(())
        })?;
        if let Some(w) = self.error_words(id) {
            self.note = w;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ picking references
    /// partcraft.js `startPick(id)`: the part as it is before the feature, so its edges or faces can be clicked.
    pub fn start_pick(&mut self, id: &str) {
        let Some(f) = P::feature_by_id(&self.part, id) else { return };
        let ty = jv::s(f, "type").to_string();
        if !PICKERS.contains(&ty.as_str()) {
            return;
        }
        if !self.named() {
            self.note = tr("app.pick.needs_exact_off");
            return;
        }
        let name = jv::s(f, "name").to_string();
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        self.mode = Mode::Pick;
        self.editing = Some(id.to_string());
        self.sel_feature = Some(id.to_string());
        self.sel_plane = None;
        self.pick = None;
        self.multi.clear();
        self.sel_face = None;
        self.rebuild();
        self.note = match ty.as_str() {
            "thread" => trv("thread.ui.pick_start", &[("name", name)]),
            "draft" => trv("app.pick.start_draft", &[("name", name)]),
            "shell" => trv("app.pick.start_shell", &[("name", name)]),
            _ => trv("app.pick.start_edges", &[("name", name)]),
        };
    }

    /// partcraft.js `pickRef(h)`: a face (shell) or edge (fillet, chamfer) clicked on or off the feature being picked.
    pub fn pick_ref(&mut self, face: Option<usize>, edge: Option<usize>) {
        let Some(id) = self.editing.clone() else { return };
        let Some(f) = P::feature_by_id(&self.part, &id).cloned() else { return };
        if !self.named() {
            return;
        }
        let ty = jv::s(&f, "type").to_string();
        let name = jv::s(&f, "name").to_string();
        if FACE_PICKERS.contains(&ty.as_str()) {
            let Some(fi) = face else {
                self.note = tr("app.pick.click_face");
                return;
            };
            let Some(fc) = self.face(fi).cloned() else { return };
            if !jv::truthy(fc.get("name")) {
                self.note = tr("app.pick.face_unnamed");
                return;
            }
            if ty == "thread" && jv::s(&fc, "type") != "cylinder" {
                self.note = tr("thread.ui.pick_round");
                return;
            }
            let at = jv::arr(f.get("faces")).iter().position(|r| self.find_face(r) == Some(fi));
            let label = tr(if at.is_some() { "app.undo.drop_face" } else { "app.undo.add_face" });
            let rf = crate::partview::face_ref(&fc);
            let _ = self.commit(&label, |p| {
                if let Some(g) =
                    p.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.iter_mut().find(|g| jv::is_str(g.get("id"), &id)))
                    && let Some(list) = g.get_mut("faces").and_then(Value::as_array_mut)
                {
                    match at {
                        Some(i) if i < list.len() => {
                            list.remove(i);
                        }
                        _ => list.push(rf),
                    }
                }
                Ok(())
            });
            let n = P::feature_by_id(&self.part, &id).map_or(0, |g| jv::arr(g.get("faces")).len());
            self.note = trv("app.pick.faces_open", &[("name", name), ("n", n.to_string())]);
            return;
        }
        let Some(ei) = edge else {
            self.note = tr("app.pick.click_edge");
            return;
        };
        let Some(e) = self.edges().get(ei).cloned() else { return };
        let at = jv::arr(f.get("edges")).iter().position(|r| self.find_edge(r) == Some(ei));
        let label = tr(if at.is_some() { "app.undo.drop_edge" } else { "app.undo.add_edge" });
        let rf = crate::partview::edge_ref(&e);
        let _ = self.commit(&label, |p| {
            if let Some(g) =
                p.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.iter_mut().find(|g| jv::is_str(g.get("id"), &id)))
                && let Some(list) = g.get_mut("edges").and_then(Value::as_array_mut)
            {
                match at {
                    Some(i) if i < list.len() => {
                        list.remove(i);
                    }
                    _ => list.push(rf),
                }
            }
            Ok(())
        });
        let n = P::feature_by_id(&self.part, &id).map_or(0, |g| jv::arr(g.get("edges")).len());
        self.note = trv("app.pick.edges_count", &[("name", name), ("n", n.to_string())]);
    }

    /// partcraft.js `onPick` in model mode for an edge (or a Shift+click of an edge or face).
    pub fn pick_geometry(&mut self, face: bool, index: usize, shift: bool) {
        if self.mode == Mode::Pick {
            if face {
                self.pick_ref(Some(index), None)
            } else {
                self.pick_ref(None, Some(index))
            }
            return;
        }
        if self.mode != Mode::Model {
            return;
        }
        if shift && self.named() {
            let Some(add) = (if face { self.face_pick_at(index) } else { self.edge_pick(index) }) else { return };
            if self.multi.first().is_some_and(|m| m.face != add.face) {
                self.multi.clear();
            }
            if let Some(p) = self.pick.clone()
                && p.face == add.face
                && self.multi.is_empty()
            {
                let first = if p.face { self.face_pick_at(p.index) } else { self.edge_pick(p.index) };
                if let Some(fp) = first {
                    self.multi.push(fp);
                }
            }
            match self.multi.iter().position(|m| m.index == add.index) {
                Some(i) => {
                    self.multi.remove(i);
                }
                None => self.multi.push(add.clone()),
            }
            self.pick = None;
            self.sel_face = None;
            self.retint();
            self.note = trv(if add.face { "app.pick.multi_faces" } else { "app.pick.multi_edges" }, &[("n", self.multi.len().to_string())]);
            return;
        }
        if !self.multi.is_empty() {
            self.multi.clear();
        }
        if face {
            self.pick_face(index);
        } else {
            self.pick = self.edge_pick(index);
            self.sel_face = None;
            self.retint();
        }
    }

    // ------------------------------------------------------------------ history actions
    /// The features that use `id`, directly or through others (partcraft.js `deleteSelected`'s walk).
    pub fn dependents_of(&self, id: &str) -> Vec<String> {
        let mut deps: Vec<String> = Vec::new();
        let mut todo = vec![id.to_string()];
        let mut guard = 0usize;
        while let Some(x) = todo.pop() {
            guard += 1;
            if guard > 100_000 {
                break;
            }
            for g in P::dependents(&self.part, &x) {
                let gid = jv::s(&g, "id").to_string();
                if !deps.contains(&gid) {
                    deps.push(gid.clone());
                    todo.push(gid);
                }
            }
        }
        deps
    }

    /// partcraft.js `deleteSelected()`: with dependents, the question first (`confirm`), then `delete_feature`.
    pub fn delete_selected(&mut self, confirmed: bool) {
        let Some(id) = self.sel_feature.clone() else {
            self.note = tr("app.feature.select_to_delete");
            return;
        };
        let Some(f) = P::feature_by_id(&self.part, &id) else { return };
        let fname = jv::s(f, "name").to_string();
        let deps = self.dependents_of(&id);
        if !deps.is_empty() && !confirmed {
            let names: Vec<String> =
                deps.iter().filter_map(|d| P::feature_by_id(&self.part, d)).map(|g| jv::s(g, "name").to_string()).collect();
            self.confirm = Some((
                id,
                trv("app.feature.delete_title", &[("name", fname.clone())]),
                trv("app.feature.delete_deps", &[("n", deps.len().to_string()), ("names", names.join(", ")), ("name", fname)]),
            ));
            return;
        }
        self.confirm = None;
        let mut names = Vec::new();
        let ids = vec![id];
        if self
            .commit(&tr("app.undo.delete"), |p| {
                names = P::delete_features(p, &ids);
                Ok(())
            })
            .is_ok()
        {
            self.sel_feature = None;
            self.note = trv("app.feature.deleted", &[("names", names.join(", "))]);
        }
    }

    /// partcraft.js `moveSelected(delta)`.
    pub fn move_selected(&mut self, delta: f64) {
        let Some(id) = self.sel_feature.clone() else { return };
        let Some(i) = P::index_of(&self.part, &id) else { return };
        let to = cc_part::js::f(i) + delta;
        if let Err(reason) = P::can_move(&self.part, &id, to) {
            self.note = reason;
            return;
        }
        if self.commit(&tr("app.undo.reorder"), |p| P::move_feature(p, &id, to)).is_ok() {
            let name = P::feature_by_id(&self.part, &id).map(|f| jv::s(f, "name").to_string()).unwrap_or_default();
            self.note = trv(if delta < 0.0 { "app.feature.moved_up" } else { "app.feature.moved_down" }, &[("name", name)]);
        }
    }

    /// The name field (partcraft.js `pc-feat-name` change): control characters out, trimmed, at most 60.
    pub fn rename_feature(&mut self, id: &str, text: &str) -> Result<(), String> {
        let v: String = text.chars().filter(|c| (*c as u32) > 0x1f).collect();
        let v: String = cc_part::js::trim(&v).to_string();
        let v = cc_part::js::slice16(&v, 60);
        if v.is_empty() {
            return Ok(());
        }
        self.set_feature(id, &tr("app.undo.rename"), |g| jv::set(g, "name", v.as_str().into()))
    }

    /// A number typed into a feature's field (partcraft.js `numInput` with a params key): a plain number goes through
    /// the field's own change (one undo step with the field's label), anything else through the parameters.
    fn typed_number(&mut self, id: &str, field: &str, raw: &str, min: Option<f64>, max: Option<f64>) -> Result<Option<f64>, String> {
        let key = format!("{id}.{field}");
        let s = raw.trim();
        if s.is_empty() {
            return Ok(None);
        }
        let targets = PP::targets(&self.part);
        let linked = PP::params_of(&self.part).link(&key).is_some();
        let lit = if s.starts_with('=') { None } else { cc_part::expr::literal(s) };
        let Some(tg) = targets.get(&key).cloned() else {
            // no parameter target (a field not in use): partcraft.js numInput's own check
            let v = cc_part::js::str_num(s);
            if !v.is_finite() || min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m) {
                return Err(match (min, max) {
                    (Some(a), Some(b)) => {
                        trv("app.input.number_from_to", &[("min", cc_part::js::num_str(a)), ("max", cc_part::js::num_str(b))])
                    }
                    (Some(a), None) => trv("app.input.number_from", &[("min", cc_part::js::num_str(a))]),
                    (None, Some(b)) => trv("app.input.number_to", &[("max", cc_part::js::num_str(b))]),
                    _ => tr("app.input.number"),
                });
            }
            return Ok(Some(v));
        };
        if lit.is_none() || linked {
            let w = self.set_param(&format!("{key}={s}"))?;
            if !w.is_empty() {
                self.note = w;
            }
            return Ok(None);
        }
        let n = lit.unwrap_or(f64::NAN);
        let mut t2 = tg;
        if min.is_some() {
            t2.min = min;
        }
        if max.is_some() {
            t2.max = max;
        }
        if let Some(vp) = PR::value_problem(&t2, n) {
            return Err(vp);
        }
        Ok(Some(n))
    }

    /// `part.set`: "fid:field=value" — the feature panel's fields (partcraft.js `featureProps`), each one undo step
    /// with 0.5's label and side effects. Fields: op, end, depth, reverse, angle, axis, kind, d, through, cboreD,
    /// cboreDepth, spacing, count, dir, seed:<id> (on/off), body (mirror: whole body on/off), plane, r, t, outward,
    /// drop:<i> (a picked edge / face off the list), name.
    pub fn set_field(&mut self, spec: &str) -> Result<(), String> {
        let (target, value) = spec.split_once('=').ok_or_else(|| format!("part.set: \"{spec}\" is not feature:field=value"))?;
        let (fkey, field) = target.split_once(':').ok_or_else(|| format!("part.set: \"{spec}\" is not feature:field=value"))?;
        let id = self.feature_id(fkey.trim()).ok_or_else(|| format!("part.set: no feature {fkey}"))?;
        let f = P::feature_by_id(&self.part, &id).cloned().ok_or("part.set: the feature is gone")?;
        let ty = jv::s(&f, "type").to_string();
        let field = field.trim();
        let value = value.trim().to_string();
        let on = matches!(value.as_str(), "true" | "1" | "on" | "yes");
        let num = |pv: &mut PartView, k: &str, min: Option<f64>, max: Option<f64>| pv.typed_number(&id, k, &value, min, max);
        match (ty.as_str(), field) {
            (_, "name") => return self.rename_feature(&id, &value),
            ("extrude" | "revolve", "op") => {
                if !matches!(value.as_str(), "add" | "cut" | "intersect" | "new") {
                    return Err(format!("part.set: op is add, cut, intersect or new, not {value}"));
                }
                let extrude = ty == "extrude";
                self.set_feature(&id, &tr("app.undo.operation"), |g| {
                    jv::set(g, "op", value.as_str().into());
                    if extrude && value != "cut" && jv::is_str(g.get("end"), "through") {
                        jv::set(g, "end", "blind".into());
                    }
                })?;
            }
            ("extrude", "end") => {
                if !matches!(value.as_str(), "blind" | "mid" | "through" | "upto" | "next" | "vertex" | "offset") {
                    return Err(format!("part.set: end is blind, mid, through, upto, next, vertex or offset, not {value}"));
                }
                let pick = self.pick.as_ref().filter(|p| p.face && p.flat).and_then(|p| p.rf.clone());
                self.set_feature(&id, &tr("app.undo.end_condition"), |g| {
                    jv::set(g, "end", value.as_str().into());
                    if (value == "upto" || value == "offset")
                        && !jv::truthy(g.get("upto"))
                        && let Some(r) = pick
                    {
                        jv::set(g, "upto", r);
                    }
                    if value != "blind" {
                        jv::del(g, "two");
                    }
                    if value == "mid" {
                        jv::del(g, "taper");
                    }
                })?;
            }
            ("extrude" | "hole", "depth") => {
                if let Some(v) = num(self, "depth", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.undo.depth"), |g| jv::set(g, "depth", jv::num(v)))?;
                }
            }
            ("extrude" | "hole" | "lpattern" | "cpattern", "reverse") => {
                self.set_feature(&id, &tr("app.undo.reverse"), |g| jv::set(g, "reverse", on.into()))?;
            }
            ("revolve" | "cpattern", "angle") => {
                if let Some(v) = num(self, "angle", Some(0.001), Some(360.0))? {
                    self.set_feature(&id, &tr("app.undo.angle"), |g| jv::set(g, "angle", jv::num(v)))?;
                }
            }
            ("revolve", "axis") => {
                if value == "obj" {
                    return Ok(());
                }
                let ax = if let Some(r) = value.strip_prefix("ref:") {
                    jv::obj(vec![("kind", "ref".into()), ("feature", r.into())])
                } else if value.is_empty() {
                    Value::Null
                } else {
                    value.as_str().into()
                };
                self.set_feature(&id, &tr("app.undo.axis"), |g| jv::set(g, "axis", ax))?;
            }
            ("hole", "kind") => {
                if !matches!(value.as_str(), "simple" | "cbore" | "csink" | "tapped") {
                    return Err(format!("part.set: kind is simple, cbore, csink or tapped, not {value}"));
                }
                self.set_feature(&id, &tr("app.undo.hole_type"), |g| {
                    jv::set(g, "kind", value.as_str().into());
                    if (value == "csink" || value == "tapped") && !jv::truthy(g.get("size")) {
                        let d = jv::n(g, "d");
                        jv::set(g, "csinkD", jv::num(cc_part::js::max(12.0, d * 2.0)));
                        jv::set(g, "csinkAngle", 90.into());
                        jv::set(g, "size", "M6".into());
                        jv::set(g, "threadDepth", Value::Null);
                    }
                })?;
            }
            ("hole", "d") => {
                if let Some(v) = num(self, "d", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.undo.diameter"), |g| jv::set(g, "d", jv::num(v)))?;
                }
            }
            ("hole", "through") => self.set_feature(&id, &tr("app.hole.through_all"), |g| jv::set(g, "through", on.into()))?,
            ("hole", "cboreD") => {
                if let Some(v) = num(self, "cboreD", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.undo.cbore_d"), |g| jv::set(g, "cboreD", jv::num(v)))?;
                }
            }
            ("hole", "cboreDepth") => {
                if let Some(v) = num(self, "cboreDepth", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.hole.cbore_depth"), |g| jv::set(g, "cboreDepth", jv::num(v)))?;
                }
            }
            ("lpattern", "spacing") => {
                if let Some(v) = num(self, "spacing", None, None)? {
                    self.set_feature(&id, &tr("app.undo.spacing"), |g| jv::set(g, "spacing", jv::num(v)))?;
                }
            }
            ("lpattern" | "cpattern", "count") => {
                if let Some(v) = num(self, "count", Some(1.0), Some(1000.0))? {
                    let c = cc_part::js::max(1.0, cc_part::js::min(1000.0, cc_part::js::round(v)));
                    self.set_feature(&id, &tr("app.pattern.count"), |g| jv::set(g, "count", jv::num(c)))?;
                }
            }
            ("lpattern" | "cpattern", "dir" | "axis") => {
                let k = if ty == "lpattern" { "dir" } else { "axis" };
                if field != k {
                    return Err(format!("part.set: a {ty} has {k}"));
                }
                let nr = if value == "pick" {
                    let p = self.pick.as_ref().filter(|p| !p.face).ok_or("part.set: pick an edge first")?;
                    jv::obj(vec![
                        ("kind", "fixed".into()),
                        ("o", jv::pt3(p.a)),
                        ("d", jv::pt3(cc_part::vec::norm(cc_part::vec::sub(p.b, p.a)))),
                    ])
                } else if value == "fixed" {
                    return Ok(());
                } else {
                    let mut it = value.splitn(3, ':');
                    let (kind, a, b) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
                    match kind {
                        "world" => jv::obj(vec![("kind", "world".into()), ("axis", a.into())]),
                        "axis" => jv::obj(vec![("kind", "axis".into()), ("feature", a.into())]),
                        "ref" => jv::obj(vec![("kind", "ref".into()), ("feature", a.into())]),
                        "line" => jv::obj(vec![("kind", "line".into()), ("sketch", a.into()), ("entity", b.into())]),
                        _ => return Err(format!("part.set: {k} is world:x|y|z, axis:<id>, ref:<id>, line:<sketch>:<line> or pick")),
                    }
                };
                let label = tr(if k == "dir" { "app.pattern.direction" } else { "app.undo.axis" });
                self.set_feature(&id, &label, |g| jv::set(g, k, nr))?;
            }
            ("lpattern" | "cpattern" | "mirror", seed) if seed.starts_with("seed:") => {
                let sid = seed.trim_start_matches("seed:").to_string();
                self.set_feature(&id, &tr("app.seeds.to_copy"), |h| {
                    let mut list: Vec<Value> = jv::arr(h.get("seeds")).to_vec();
                    if on {
                        list.push(sid.as_str().into());
                    } else {
                        list.retain(|x| x.as_str() != Some(sid.as_str()));
                    }
                    jv::set(h, "seeds", Value::Array(list));
                })?;
            }
            ("mirror", "body") => {
                let i = P::index_of(&self.part, &id).unwrap_or(0);
                let earlier: Vec<String> = P::features(&self.part)
                    .iter()
                    .take(i)
                    .filter(|g| SOLID.contains(&jv::s(g, "type")))
                    .map(|g| jv::s(g, "id").to_string())
                    .collect();
                self.set_feature(&id, &tr("app.undo.mirror_body"), |g| {
                    let v =
                        if on { "body".into() } else { Value::Array(earlier.last().map(|x| vec![x.as_str().into()]).unwrap_or_default()) };
                    jv::set(g, "seeds", v);
                })?;
            }
            ("mirror", "plane") => {
                if value == "cur" {
                    return Ok(());
                }
                let nr = if value == "pick" {
                    self.pick.as_ref().filter(|p| p.face && p.flat).and_then(|p| p.rf.clone()).ok_or("part.set: pick a flat face first")?
                } else if let Some(r) = value.strip_prefix("ref:") {
                    jv::obj(vec![("kind", "ref".into()), ("feature", r.into())])
                } else {
                    let n = value.strip_prefix("base:").unwrap_or(&value);
                    if !matches!(n, "front" | "top" | "right") {
                        return Err(format!("part.set: plane is base:front|top|right, ref:<id> or pick, not {value}"));
                    }
                    jv::obj(vec![("kind", "base".into()), ("name", n.into())])
                };
                self.set_feature(&id, &tr("app.mirror.plane"), |g| jv::set(g, "plane", nr))?;
            }
            ("fillet", "r") => {
                if let Some(v) = num(self, "r", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.props.radius"), |g| jv::set(g, "r", jv::num(v)))?;
                }
            }
            ("chamfer", "d") => {
                if let Some(v) = num(self, "d", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.undo.distance"), |g| jv::set(g, "d", jv::num(v)))?;
                }
            }
            ("shell", "t") => {
                if let Some(v) = num(self, "t", Some(0.000001), None)? {
                    self.set_feature(&id, &tr("app.undo.wall"), |g| jv::set(g, "t", jv::num(v)))?;
                }
            }
            ("shell", "outward") => self.set_feature(&id, &tr("app.undo.shell_direction"), |g| jv::set(g, "outward", on.into()))?,
            ("fillet" | "chamfer" | "shell", drop) if drop.starts_with("drop:") => {
                let i: usize = drop.trim_start_matches("drop:").parse().map_err(|_| format!("part.set: {drop} is not drop:<number>"))?;
                let key = if ty == "shell" { "faces" } else { "edges" };
                let label = tr(if key == "edges" { "app.undo.drop_edge" } else { "app.undo.drop_face" });
                self.set_feature(&id, &label, |g| {
                    if let Some(l) = g.get_mut(key).and_then(Value::as_array_mut)
                        && i < l.len()
                    {
                        l.remove(i);
                    }
                })?;
            }
            _ => return Err(format!("part.set: a {ty} has no field {field} here")),
        }
        Ok(())
    }

    /// The sketch dimensions block (partcraft.js `sketchDimsBlock`): "sketch:constraint=value", one undo step.
    pub fn set_dimension(&mut self, spec: &str) -> Result<(), String> {
        let (target, value) = spec.split_once('=').ok_or_else(|| format!("part.dim: \"{spec}\" is not sketch:dimension=value"))?;
        let (skey, cid) = target.split_once(':').ok_or_else(|| format!("part.dim: \"{spec}\" is not sketch:dimension=value"))?;
        let sid = self.feature_id(skey.trim()).ok_or_else(|| format!("part.dim: no sketch {skey}"))?;
        let cid = cid.trim().to_string();
        let ty = P::feature_by_id(&self.part, &sid)
            .and_then(|f| f.get("sketch"))
            .map(S::constraints)
            .and_then(|cs| cs.iter().find(|c| jv::is_str(c.get("id"), &cid)).map(|c| jv::s(c, "type").to_string()))
            .ok_or_else(|| format!("part.dim: no dimension {cid}"))?;
        let Some(v) = self.typed_number(&sid, &cid, value, Some(0.0), None)? else { return Ok(()) };
        if (ty == "radius" || ty == "diameter") && v <= 0.0 {
            self.note = tr("app.dims.radius_positive");
            return Ok(());
        }
        if ty == "angle" && v > 180.0 {
            self.note = tr("app.dims.angle_range");
            return Ok(());
        }
        let s2 = sid.clone();
        self.commit(&tr("app.undo.change_dimension"), |p| {
            if let Some(f) =
                p.get_mut("features").and_then(Value::as_array_mut).and_then(|a| a.iter_mut().find(|g| jv::is_str(g.get("id"), &s2)))
                && let Some(c) =
                    f.get_mut("sketch").and_then(S::constraints_mut).and_then(|cs| cs.iter_mut().find(|c| jv::is_str(c.get("id"), &cid)))
            {
                jv::set(c, "value", jv::num(v));
            }
            Ok(())
        })?;
        if let Some(w) = self.error_words(&sid) {
            self.note = w;
        }
        Ok(())
    }
}
