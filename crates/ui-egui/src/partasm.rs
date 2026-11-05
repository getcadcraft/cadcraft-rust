//! PartCraft's assemblies in the app (M13.11): partcraft.js's assembly mode on the mesh kernel — New assembly (the
//! part grounded), the part copies drawn with their placements (assembly.rs `place_all`), Add copy / Insert a part
//! file, Joint (a flat face on each of two parts; revolve faces make a revolute joint), a joint's type, values, centre
//! axes and limits, the motion check, Check interference with its filters, Edit this part (in place, Back returns the
//! changes), remove, ground, rename, undo / redo of the assembly, the Edit menu (copy / cut / paste / duplicate part
//! copies, between assemblies too), the keys (J, Esc, Delete, F), Save / Open `.partcraft-asm.json`, Close (asking when
//! there are changes). Commands `part.asm` and `part.asm.set`; the panels and the view are below.

use cc_part::{asm as A, clipboard as C, js, jv, kernel, kernel_asm as KA, part as P, vec as V};
use serde_json::Value;
use std::collections::HashMap;

use crate::partview::{Mode, PartView, tr, trv};

/// One part of the assembly as the kernel built it.
pub struct AsmBuild {
    /// the part's JSON the build was made from
    pub src: String,
    pub regen: kernel::Regen,
    pub build: A::Build,
    pub solid: KA::SolidBuild,
}

/// partcraft.js's assembly state (`asm`, `asmBuilds`, `asmPlace`, `asmSel`, `jointPick`, `clashes`, `asmHist`, `asmDirty`).
#[derive(Default)]
pub struct AsmSt {
    pub asm: Value,
    pub builds: HashMap<String, AsmBuild>,
    pub world: Vec<(String, V::M4)>,
    pub problems: Vec<Value>,
    /// ("inst" | "joint", id)
    pub sel: Option<(String, String)>,
    /// Joint pressed: the first face picked (instance, plane reference) once there is one
    pub joint_pick: Option<Option<(String, Value)>>,
    pub clashes: Option<Vec<Value>>,
    pub undo: Vec<(String, Value)>,
    pub redo: Vec<(String, Value)>,
    pub dirty: bool,
    /// the motion check's value (a joint with several)
    pub sweep: Option<String>,
    /// a question waiting for an answer: "remove" (a part with joints) or "close" (changes not saved)
    pub ask: Option<&'static str>,
    /// how long the last placement took (ms)
    pub ms: f64,
    /// text typed into the panel's fields, by key
    pub edits: HashMap<String, String>,
    /// the instance under the pointer
    pub hover: Option<String>,
    /// the explode / move tools' page state (asm-ui.js `st`)
    pub tools: crate::partasmui::ToolsSt,
}

/// Edit this part: the assembly's part being edited and what goes back afterwards (partcraft.js `editPartOf`).
pub struct EditPartOf {
    pub pid: String,
    pub part: Value,
    pub dirty: bool,
    pub undo: Vec<(String, Value)>,
    pub redo: Vec<(String, Value)>,
    pub before: String,
}

const IDENT: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn insts(a: &Value) -> &[Value] {
    jv::arr(a.get("instances"))
}

fn m4_json(m: &V::M4) -> Value {
    jv::nums(m)
}

/// The [x, y, z] of a JSON array.
pub fn p3(v: Option<&Value>) -> [f64; 3] {
    let a = jv::arr(v);
    let g = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    [g(0), g(1), g(2)]
}

/// `name.replace(/[^A-Za-z0-9 _.-]+/g, '_').trim().slice(0, 80) || 'assembly'`.
pub fn asm_file_base(name: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-') {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    let t: String = out.trim().chars().take(80).collect();
    if t.is_empty() { "assembly".into() } else { t }
}

/// Control characters out, trimmed, cut at `max` characters.
fn clean_name(v: &str, max: usize) -> String {
    let s: String = v.chars().filter(|c| (*c as u32) >= 0x20).collect();
    s.trim().chars().take(max).collect()
}

fn fmt3(v: f64) -> String {
    V::fmt(v, 3)
}

impl AsmSt {
    pub fn inst(&self, id: &str) -> Option<&Value> {
        insts(&self.asm).iter().find(|i| jv::is_str(i.get("id"), id))
    }
    pub fn joint(&self, id: &str) -> Option<&Value> {
        jv::arr(self.asm.get("joints")).iter().find(|j| jv::is_str(j.get("id"), id))
    }
    pub fn inst_name(&self, id: &str) -> String {
        self.inst(id).map_or_else(|| id.to_string(), |i| jv::s(i, "name").to_string())
    }
    pub fn world_of(&self, id: &str) -> Option<V::M4> {
        self.world.iter().find(|(k, _)| k == id).map(|(_, m)| *m)
    }
    pub fn part_of(&self, pid: &str) -> Option<&Value> {
        self.asm.get("parts").and_then(|p| p.get(pid))
    }
    pub fn builds_map(&self) -> A::Builds {
        self.builds.iter().map(|(k, b)| (k.clone(), b.build.clone())).collect()
    }
    pub fn solids(&self) -> Vec<(String, KA::SolidBuild)> {
        self.builds.iter().map(|(k, b)| (k.clone(), b.solid.clone())).collect()
    }
    pub fn stats_of(&self, pid: &str) -> Option<&Value> {
        self.builds.get(pid).and_then(|b| b.regen.stats.as_ref())
    }
    pub fn bbox_of(&self, pid: &str) -> Option<([f64; 3], [f64; 3])> {
        let b = self.stats_of(pid)?.get("bbox")?;
        Some((p3(b.get("min")), p3(b.get("max"))))
    }
    /// Is a selection still there?
    fn sel_alive(&self) -> bool {
        match &self.sel {
            Some((k, id)) if k == "inst" => self.inst(id).is_some(),
            Some((_, id)) => self.joint(id).is_some(),
            None => true,
        }
    }
}

impl PartView {
    /// In the assembly (not editing one of its parts): partcraft.js `mode === 'assembly'`.
    pub fn in_asm(&self) -> bool {
        self.asm.is_some() && self.asm_edit.is_none()
    }

    /// partcraft.js `rebuildAsm()` (the mesh kernel builds in place): new builds for changed parts, the placement.
    pub fn asm_rebuild(&mut self) {
        let t0 = web_time::Instant::now();
        let Some(st) = self.asm.as_mut() else { return };
        let parts: Vec<(String, Value)> = st
            .asm
            .get("parts")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, p)| (k.clone(), p.get("part").cloned().unwrap_or(Value::Null))).collect())
            .unwrap_or_default();
        for (pid, pt) in &parts {
            let src = jv::stringify(pt);
            if st.builds.get(pid).is_some_and(|b| b.src == src) {
                continue;
            }
            let regen = kernel::regenerate(pt, None);
            let j = regen.to_json();
            st.builds.insert(pid.clone(), AsmBuild { src, build: A::Build::from_json(&j), solid: KA::SolidBuild::from_json(&j), regen });
        }
        st.builds.retain(|k, _| parts.iter().any(|(p, _)| p == k));
        let (world, problems) = A::place_all(&st.asm, &st.builds_map());
        st.world = world;
        st.problems = problems;
        if !st.sel_alive() {
            st.sel = None;
        }
        st.ms = t0.elapsed().as_secs_f64() * 1000.0;
        self.asm_scene();
        self.message = self.asm_words();
    }

    /// The build chip in the assembly (`ui.build`).
    pub fn asm_words(&self) -> String {
        let Some(st) = self.asm.as_ref() else { return String::new() };
        let bad = st.problems.iter().filter(|x| !jv::truthy(x.get("free"))).count();
        if bad > 0 {
            cc_part::i18n::t("app.asm.joint_problems", &[("n", cc_part::i18n::F::from(bad))])
        } else {
            trv("app.asm.placed", &[("ms", js::num_str(st.ms.round()))])
        }
    }

    /// partcraft.js `enterAssembly()`.
    pub fn asm_enter(&mut self) {
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        if self.drawing.is_some() {
            return;
        }
        if self.asm.is_none() {
            if self.regen.mesh.is_none() {
                self.note = tr("app.asm.build_first");
                return;
            }
            let name = jv::s(&self.part, "name");
            let mut a = A::new_assembly(&format!("{} assembly", if name.is_empty() { "Part" } else { name }));
            if let Err(e) = A::insert_part(&mut a, Some(&self.part), None, None) {
                self.note = e;
                return;
            }
            self.asm =
                Some(AsmSt { asm: a, dirty: true, tools: crate::partasmui::ToolsSt { t: 1.0, ..Default::default() }, ..AsmSt::default() });
            self.note = trv("app.asm.started", &[("name", name.to_string())]);
        } else if let Some(st) = self.asm.as_ref() {
            self.note = trv("app.asm.back_in", &[("name", jv::s(&st.asm, "name").to_string())]);
        }
        self.measure_stop();
        self.section_stop();
        self.analysis_stop();
        self.sel_feature = None;
        self.pick = None;
        self.multi.clear();
        if let Some(st) = self.asm.as_mut() {
            st.sel = None;
            st.joint_pick = None;
            st.clashes = None;
        }
        self.asm_rebuild();
        self.standard("iso");
        self.fit();
    }

    /// partcraft.js `commitAsm(label, fn)`: one undo step, the placement again.
    pub fn asm_commit<R>(&mut self, label: &str, f: impl FnOnce(&mut Value) -> R) -> Option<R> {
        let st = self.asm.as_mut()?;
        st.undo.push((label.to_string(), st.asm.clone()));
        if st.undo.len() > 200 {
            st.undo.remove(0);
        }
        st.redo.clear();
        let r = f(&mut st.asm);
        st.dirty = true;
        st.clashes = None;
        self.asm_rebuild();
        Some(r)
    }

    /// partcraft.js `asmPick(h)`: a face of a part copy (instance id, face index in its build), or nothing.
    pub fn asm_pick(&mut self, hit: Option<(String, usize)>) {
        if self.asm_tools_pick(hit.as_ref()) {
            return;
        }
        let Some(st) = self.asm.as_mut() else { return };
        if st.joint_pick.is_some() {
            let Some((iid, fi)) = hit else {
                self.note = tr("app.joint.pick_flat");
                return;
            };
            let pid = st.inst(&iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
            let face = st.builds.get(&pid).and_then(|b| b.regen.faces.as_ref()).and_then(|f| f.get(fi)).cloned();
            let Some(f) = face.filter(|f| jv::s(f, "type") == "plane") else {
                self.note = tr("app.joint.face_curved");
                return;
            };
            let plane = if jv::s(&f, "name").is_empty() {
                let r9 = |v: [f64; 3], d: usize| v.map(|x| cc_part::js::str_num(&cc_part::js::to_fixed(x, d)));
                jv::obj(vec![("kind", "fixed".into()), ("o", jv::nums(&r9(p3(f.get("o")), 9))), ("n", jv::nums(&r9(p3(f.get("n")), 12)))])
            } else {
                crate::partview::ref_for_name(jv::s(&f, "name"))
            };
            let first = st.joint_pick.clone().flatten();
            let Some((ia, pa)) = first else {
                let name = st.inst_name(&iid);
                st.joint_pick = Some(Some((iid, plane)));
                self.note = trv("app.joint.pick_second", &[("name", name)]);
                return;
            };
            if ia == iid {
                self.note = tr("app.joint.different_part");
                return;
            }
            let a = jv::obj(vec![("inst", ia.as_str().into()), ("plane", pa)]);
            let b = jv::obj(vec![("inst", iid.as_str().into()), ("plane", plane)]);
            let revolve_face = |e: &Value| -> bool {
                let pl = e.get("plane").unwrap_or(&Value::Null);
                let pid = st.inst(jv::s(e, "inst")).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
                jv::s(pl, "kind") == "face"
                    && st
                        .part_of(&pid)
                        .and_then(|p| p.get("part"))
                        .and_then(|p| P::feature_by_id(p, jv::s(pl, "feature")))
                        .is_some_and(|f| jv::s(f, "type") == "revolve")
            };
            let ty = if revolve_face(&a) && revolve_face(&b) { "revolute" } else { "rigid" };
            st.joint_pick = None;
            let (na, nb) = (st.inst_name(&ia), st.inst_name(&iid));
            let j = self.asm_commit(&tr("app.undo.joint"), |x| A::add_joint(x, ty, &a, &b, &Value::Null));
            match j {
                Some(Ok(j)) => {
                    let jid = jv::s(&j, "id").to_string();
                    let Some(st) = self.asm.as_mut() else { return };
                    st.sel = Some(("joint".into(), jid.clone()));
                    let pr = st.problems.iter().find(|p| jv::is_str(p.get("joint"), &jid)).map(|p| jv::s(p, "message").to_string());
                    self.note =
                        pr.unwrap_or_else(|| trv("app.joint.joins", &[("joint", jv::s(&j, "name").to_string()), ("a", na), ("b", nb)]));
                }
                Some(Err(e)) => self.note = e,
                None => {}
            }
            return;
        }
        st.sel = hit.map(|(iid, _)| ("inst".to_string(), iid));
        self.asm_scene();
    }

    /// partcraft.js `asmStartJoint()`.
    pub fn asm_start_joint(&mut self) {
        if !self.in_asm() {
            return;
        }
        let Some(st) = self.asm.as_mut() else { return };
        if insts(&st.asm).len() < 2 {
            self.note = tr("app.joint.need_two");
            return;
        }
        st.joint_pick = Some(None);
        st.sel = None;
        self.note = tr("app.joint.start");
        self.asm_scene();
    }

    /// partcraft.js `placeBeside()`: the largest X of every copy's box.
    fn place_beside(&self) -> f64 {
        let Some(st) = self.asm.as_ref() else { return 0.0 };
        let mut max_x = f64::NEG_INFINITY;
        for i in insts(&st.asm) {
            let pid = js::str_of(i.get("part"));
            let (Some((lo, hi)), Some(w)) = (st.bbox_of(&pid), st.world_of(&js::str_of(i.get("id")))) else { continue };
            for x in [lo[0], hi[0]] {
                for y in [lo[1], hi[1]] {
                    for z in [lo[2], hi[2]] {
                        max_x = js::max(max_x, V::apply_mat(&w, [x, y, z])[0]);
                    }
                }
            }
        }
        if max_x == f64::NEG_INFINITY { 0.0 } else { max_x }
    }

    /// partcraft.js `asmInsertCopy()`.
    pub fn asm_insert_copy(&mut self) {
        let Some(st) = self.asm.as_ref() else { return };
        let src = match &st.sel {
            Some((k, id)) if k == "inst" => st.inst(id).cloned(),
            _ => insts(&st.asm).last().cloned(),
        };
        let Some(src) = src else { return };
        let pid = js::str_of(src.get("part"));
        let min0 = st.bbox_of(&pid).map_or(0.0, |(lo, _)| lo[0]);
        let dx = self.place_beside() + 20.0 - min0;
        let place = m4_json(&V::translation([dx, 0.0, 0.0]));
        match self.asm_commit(&tr("app.undo.insert_copy"), |a| A::insert_part(a, None, Some(&place), Some(&pid))) {
            Some(Ok(inst)) => {
                if let Some(st) = self.asm.as_mut() {
                    st.sel = Some(("inst".into(), jv::s(&inst, "id").to_string()));
                }
                self.note = trv("app.asm.inserted", &[("name", jv::s(&inst, "name").to_string())]);
                self.asm_scene();
            }
            Some(Err(e)) => self.note = e,
            None => {}
        }
    }

    /// The Insert picker's file (partcraft.js `ui.insertFile` change).
    pub fn asm_insert_file(&mut self, file: &str, bytes: &[u8]) {
        if self.asm.is_none() {
            return;
        }
        if bytes.len() > crate::partview::MAX_FILE_BYTES {
            self.note = tr("app.file.too_big");
            return;
        }
        // part.js readPart(text): the file's text, parsed and checked there
        let raw = Value::String(String::from_utf8_lossy(bytes).into_owned());
        let p = match P::read_part(&raw) {
            Ok(p) => p,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let r = kernel::regenerate(&p, None);
        let Some(min0) = r.mesh.as_ref().and(r.stats.as_ref()).and_then(|s| s.get("bbox")).map(|b| p3(b.get("min"))[0]) else {
            self.note = trv("app.asm.no_solid", &[("file", file.to_string())]);
            return;
        };
        let dx = self.place_beside() + 20.0 - min0;
        let place = m4_json(&V::translation([dx, 0.0, 0.0]));
        match self.asm_commit(&tr("app.undo.insert_part"), |a| A::insert_part(a, Some(&p), Some(&place), None)) {
            Some(Ok(inst)) => {
                if let Some(st) = self.asm.as_mut() {
                    st.sel = Some(("inst".into(), jv::s(&inst, "id").to_string()));
                }
                self.note = trv("app.asm.inserted_from", &[("name", jv::s(&inst, "name").to_string()), ("file", file.to_string())]);
                self.asm_scene();
            }
            Some(Err(e)) => self.note = e,
            None => {}
        }
    }

    /// partcraft.js `asmDelete()`: a joint, or a part copy (asking first when joints hold it).
    pub fn asm_delete(&mut self, confirmed: bool) {
        let Some(st) = self.asm.as_mut() else { return };
        let Some((kind, id)) = st.sel.clone() else {
            self.note = tr("app.asm.select_to_remove");
            return;
        };
        if kind == "joint" {
            let name = st.joint(&id).map(|j| jv::s(j, "name").to_string()).unwrap_or_default();
            self.asm_commit(&tr("app.undo.remove_joint"), |a| {
                if let Some(l) = a.get_mut("joints").and_then(Value::as_array_mut) {
                    l.retain(|j| !jv::is_str(j.get("id"), &id));
                }
            });
            if let Some(st) = self.asm.as_mut() {
                st.sel = None;
            }
            self.note = trv("app.asm.removed", &[("name", name)]);
            self.asm_scene();
            return;
        }
        if insts(&st.asm).len() == 1 {
            self.note = tr("app.asm.keep_one");
            return;
        }
        let nj = jv::arr(st.asm.get("joints"))
            .iter()
            .filter(|j| jv::is_str(j.get("a").and_then(|e| e.get("inst")), &id) || jv::is_str(j.get("b").and_then(|e| e.get("inst")), &id))
            .count();
        if nj > 0 && !confirmed {
            st.ask = Some("remove");
            return;
        }
        let name = st.inst_name(&id);
        self.asm_commit(&tr("app.undo.remove_part"), |a| A::remove_instance(a, &id));
        if let Some(st) = self.asm.as_mut() {
            st.sel = None;
        }
        self.note = trv("app.asm.removed", &[("name", name)]);
        self.asm_scene();
    }

    /// One interference result in words (`clashWords`).
    pub fn clash_words(&self, c: &Value) -> String {
        let Some(st) = self.asm.as_ref() else { return String::new() };
        let (a, b) = (st.inst_name(jv::s(c, "a")), st.inst_name(jv::s(c, "b")));
        if jv::s(c, "kind") == "touch" {
            trv("asm.clash.row_touch", &[("a", a), ("b", b)])
        } else {
            trv(
                "asm.clash.row",
                &[("a", a), ("b", b), ("kind", tr(&format!("asm.clash.kind.{}", jv::s(c, "kind")))), ("vol", fmt3(jv::n(c, "volume")))],
            )
        }
    }

    /// partcraft.js `asmCheck()`.
    pub fn asm_check(&mut self) {
        let t0 = web_time::Instant::now();
        let Some(st) = self.asm.as_mut() else { return };
        let r = KA::interference(&st.asm, &st.solids(), &st.world, 1e-6, true, 1e-3);
        let list = match r {
            Ok(l) => l,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let (shown, hidden) = A::filter_clashes(&st.asm, &list);
        st.clashes = Some(list);
        let n = insts(&st.asm).len();
        let more = if hidden.is_empty() {
            String::new()
        } else {
            cc_part::i18n::t("asm.clash.hidden_note", &[("n", cc_part::i18n::F::from(hidden.len()))])
        };
        let ms = (t0.elapsed().as_secs_f64() * 1000.0).round();
        self.note = if shown.is_empty() {
            trv("app.asm.no_interference", &[("n", n.to_string()), ("ms", js::num_str(ms))]) + &more
        } else {
            let words: Vec<String> = shown.iter().map(|c| self.clash_words(c)).collect();
            trv("app.asm.interference", &[("list", words.join("; "))]) + &more
        };
    }

    /// `setClashFilter(label, fn)`: one undo step; the results stay.
    fn asm_clash_filter(&mut self, label: &str, f: impl FnOnce(&mut Value)) {
        let Some(st) = self.asm.as_mut() else { return };
        st.undo.push((label.to_string(), st.asm.clone()));
        st.redo.clear();
        f(&mut st.asm);
        st.dirty = true;
    }

    /// partcraft.js `asmEditPart()`: the selected copy's part opens as the part being edited.
    pub fn asm_edit_part(&mut self) {
        let Some(st) = self.asm.as_ref() else { return };
        let Some(iid) = st.sel.as_ref().filter(|(k, _)| k == "inst").map(|(_, id)| id.clone()) else {
            self.note = tr("app.asm.select_to_edit");
            return;
        };
        let pid = st.inst(&iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
        let Some(p) = st.part_of(&pid).cloned() else { return };
        let (pname, aname) = (jv::s(&p, "name").to_string(), jv::s(&st.asm, "name").to_string());
        let pt = p.get("part").cloned().unwrap_or(Value::Null);
        self.asm_tools_reset();
        let stash = EditPartOf {
            pid,
            part: std::mem::replace(&mut self.part, jv::json_copy(&pt)),
            dirty: self.dirty,
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            before: jv::stringify(&pt),
        };
        self.asm_edit = Some(stash);
        self.dirty = false;
        self.mode = Mode::Model;
        self.sel_feature = None;
        self.pick = None;
        self.rebuild();
        self.standard("iso");
        self.fit();
        self.note = trv("app.asm.editing_part", &[("part", pname), ("asm", aname)]);
    }

    /// partcraft.js `backToAssembly()`.
    pub fn asm_back(&mut self) {
        let Some(ed) = self.asm_edit.take() else { return };
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        self.leave_drawing();
        self.analysis_stop();
        let now = jv::stringify(&self.part);
        if now != ed.before
            && let Some(st) = self.asm.as_mut()
        {
            let pname = st.part_of(&ed.pid).map(|p| jv::s(p, "name").to_string()).unwrap_or_default();
            st.undo.push((trv("app.undo.edit_part", &[("name", pname)]), st.asm.clone()));
            st.redo.clear();
            let name = self.part.get("name").cloned().unwrap_or(Value::Null);
            if let Some(p) = st.asm.get_mut("parts").and_then(|ps| ps.get_mut(&ed.pid)) {
                jv::set(p, "part", jv::json_copy(&self.part));
                jv::set(p, "name", name);
            }
            st.dirty = true;
            st.clashes = None;
        }
        self.part = ed.part;
        self.dirty = ed.dirty;
        self.undo = ed.undo;
        self.redo = ed.redo;
        self.rebuild();
        self.asm_rebuild();
        self.standard("iso");
        let name = self.asm.as_ref().map(|s| jv::s(&s.asm, "name").to_string()).unwrap_or_default();
        self.note = trv("app.asm.back_in", &[("name", name)]);
    }

    /// Save in the assembly: `<name>.partcraft-asm.json` (assembly.js `toJSON`).
    pub fn asm_save_bytes(&self) -> Option<(String, Vec<u8>, String)> {
        let st = self.asm.as_ref()?;
        let file = format!("{}.partcraft-asm.json", asm_file_base(jv::s(&st.asm, "name")));
        let words = trv("app.asm.saved", &[("file", file.clone())]);
        Some((file, A::to_json(&st.asm).into_bytes(), words))
    }

    /// partcraft.js `closeAssembly()` with the question's answer (None = not asked yet): whether it closed.
    pub fn asm_close(&mut self, answer: Option<&str>) -> Result<bool, Option<(String, Vec<u8>, String)>> {
        if self.asm_edit.is_some() {
            self.asm_back();
        }
        let Some(st) = self.asm.as_mut() else { return Ok(true) };
        let mut saved = None;
        if st.dirty {
            match answer {
                None => {
                    st.ask = Some("close");
                    return Ok(false);
                }
                Some("cancel") => return Ok(false),
                Some("save") => saved = self.asm_save_bytes(),
                _ => {}
            }
        }
        self.asm_tools_reset();
        self.asm = None;
        self.rebuild();
        self.standard("iso");
        self.note = tr("app.asm.closed");
        match saved {
            Some(s) => Err(Some(s)),
            None => Ok(true),
        }
    }

    /// partcraft.js `openAssemblyText(text, name)`.
    pub fn asm_open_text(&mut self, text: &str, name: &str) {
        let raw = Value::String(text.to_string());
        let a = match A::read_assembly(&raw) {
            Ok(a) => a,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        self.leave_drawing();
        if self.asm_edit.is_some() {
            self.asm_back();
        }
        self.measure_stop();
        self.section_stop();
        self.analysis_stop();
        let (n, nj) = (insts(&a).len(), jv::arr(a.get("joints")).len());
        self.asm = Some(AsmSt { asm: a, tools: crate::partasmui::ToolsSt { t: 1.0, ..Default::default() }, ..AsmSt::default() });
        self.asm_rebuild();
        self.standard("iso");
        self.fit();
        self.note = trv("app.asm.opened", &[("name", name.to_string()), ("parts", n.to_string()), ("joints", nj.to_string())]);
    }

    /// partcraft.js `undoRedo` in the assembly.
    pub fn asm_undo_redo(&mut self, undo: bool) {
        let Some(st) = self.asm.as_mut() else { return };
        let (from, to) = if undo { (&mut st.undo, &mut st.redo) } else { (&mut st.redo, &mut st.undo) };
        let Some((label, snap)) = from.pop() else {
            self.note = tr(if undo { "app.undo.nothing_undo" } else { "app.undo.nothing_redo" });
            return;
        };
        to.push((label.clone(), st.asm.clone()));
        let bare = |a: &Value| {
            let mut b = a.clone();
            jv::del(&mut b, "clashFilter");
            jv::del(&mut b, "version");
            jv::stringify(&b)
        };
        let before = bare(&st.asm);
        st.asm = A::read_assembly(&snap).unwrap_or(snap);
        let same = bare(&st.asm) == before;
        st.dirty = true;
        st.joint_pick = None;
        if !same {
            st.clashes = None;
        }
        if !st.sel_alive() {
            st.sel = None;
        }
        self.asm_rebuild();
        self.note = trv(if undo { "app.undo.undid" } else { "app.undo.redid" }, &[("what", label.to_lowercase())]);
    }

    /// The joint's value range for its slider and the motion check (`span(k)`).
    pub fn joint_span(&self, j: &Value, k: &str) -> (f64, f64) {
        if let Some((lo, hi)) = A::limit_of(j, Some(k)) {
            return (lo, hi);
        }
        if matches!(k, "angle" | "tiltX" | "tiltY" | "slotDir") {
            return if k == "angle" { (-180.0, 180.0) } else { (-90.0, 90.0) };
        }
        let Some(st) = self.asm.as_ref() else { return (-10.0, 10.0) };
        let mut size = 10.0f64;
        for i in insts(&st.asm) {
            let s = st
                .bbox_of(&js::str_of(i.get("part")))
                .map_or(10.0, |(lo, hi)| js::max(js::max(hi[0] - lo[0], hi[1] - lo[1]), hi[2] - lo[2]));
            size = js::max(size, s);
        }
        (-size.round(), size.round())
    }

    /// The motion check's button (renderAsmProps `motion`).
    pub fn asm_motion(&mut self) {
        let Some(st) = self.asm.as_ref() else { return };
        let Some(j) = st.sel.as_ref().filter(|(k, _)| k == "joint").and_then(|(_, id)| st.joint(id)).cloned() else { return };
        let dof = A::joint_dof(jv::s(&j, "type")).unwrap_or(&[]);
        let Some(first) = dof.first() else { return };
        let key = st.sweep.clone().filter(|s| dof.contains(&s.as_str())).unwrap_or_else(|| (*first).to_string());
        let (from, to) = self.joint_span(&j, &key);
        let Some(st) = self.asm.as_mut() else { return };
        st.sweep = Some(key.clone());
        let r = KA::motion_check(&st.asm, &st.builds_map(), &st.solids(), jv::s(&j, "id"), from, to, 24.0, Some(&key));
        let res = match r {
            Ok(r) => r,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let label = tr(&format!("asm.joint.value.{}", key.to_lowercase()));
        let bad: Vec<(f64, Vec<Value>)> = res
            .iter()
            .map(|x| (jv::n(x, "value"), A::filter_clashes(&st.asm, jv::arr(x.get("pairs"))).0))
            .filter(|(_, p)| !p.is_empty())
            .collect();
        self.note = match bad.first() {
            Some((v, pairs)) => trv(
                "asm.joint.check_bad",
                &[
                    ("n", bad.len().to_string()),
                    ("of", res.len().to_string()),
                    ("value", label),
                    ("at", fmt3(*v)),
                    ("vol", fmt3(pairs.first().map_or(f64::NAN, |p| jv::n(p, "volume")))),
                ],
            ),
            None => trv("asm.joint.check_ok", &[("value", label), ("from", fmt3(from)), ("to", fmt3(to)), ("n", res.len().to_string())]),
        };
    }

    /// `part.asm.set=<field>=<value>` for the selection (renderAsmProps / asm-ui.js fields).
    pub fn asm_set(&mut self, spec: &str) -> Result<(), String> {
        let (field, value) = spec.split_once('=').unwrap_or((spec, ""));
        let st = self.asm.as_ref().ok_or("part.asm.set: no assembly")?;
        let num = |s: &str| -> Option<f64> {
            let v = cc_part::js::str_num(s);
            (!s.trim().is_empty() && v.is_finite()).then_some(v)
        };
        match st.sel.clone() {
            Some((k, jid)) if k == "joint" && st.joint(&jid).is_some() => {
                let j = st.joint(&jid).cloned().unwrap_or(Value::Null);
                let ty = jv::s(&j, "type").to_string();
                let dof: Vec<&str> = A::joint_dof(&ty).unwrap_or(&[]).to_vec();
                let set_j = |pv: &mut PartView, label: &str, f: &dyn Fn(&mut Value)| {
                    pv.asm_commit(label, |a| {
                        if let Some(x) = a
                            .get_mut("joints")
                            .and_then(Value::as_array_mut)
                            .and_then(|l| l.iter_mut().find(|x| jv::is_str(x.get("id"), &jid)))
                        {
                            f(x);
                        }
                    });
                };
                if field == "type" {
                    if A::joint_label(value).is_none() {
                        return Err(format!("part.asm.set: no joint type {value}"));
                    }
                    let v = value.to_string();
                    set_j(self, &tr("asm.joint.undo.type"), &|x| {
                        jv::set(x, "type", v.as_str().into());
                        if let Some(l) = x.get("limits").cloned()
                            && !matches!(A::read_limits(&l, &v), Ok(Some(_)))
                        {
                            jv::del(x, "limits");
                        }
                    });
                    return Ok(());
                }
                if let Some(k) = field.strip_prefix("value:") {
                    let Some(v) = num(value) else {
                        self.note = tr("app.input.number");
                        return Ok(());
                    };
                    let k = k.to_string();
                    set_j(self, &tr(&format!("asm.joint.value.{}", k.to_lowercase())), &|x| jv::set(x, &k, jv::num(v)));
                    return Ok(());
                }
                if let Some(side) = field.strip_prefix("centre:") {
                    if side != "a" && side != "b" {
                        return Err("part.asm.set: centre:a or centre:b".into());
                    }
                    let (side, v) = (side.to_string(), value.to_string());
                    set_j(self, &tr("app.undo.joint_centre"), &|x| {
                        if let Some(en) = x.get_mut(&side) {
                            if v.is_empty() {
                                jv::del(en, "axis");
                            } else {
                                let (fid, idx) = v.split_once(':').map_or((v.as_str(), None), |(a, b)| (a, Some(b)));
                                let mut ax = jv::obj(vec![("feature", fid.into())]);
                                if let Some(i) = idx {
                                    jv::set(&mut ax, "index", jv::num(cc_part::js::str_num(i)));
                                }
                                jv::set(en, "axis", ax);
                            }
                        }
                    });
                    return Ok(());
                }
                if field == "sweep" {
                    if let Some(st) = self.asm.as_mut() {
                        st.sweep = Some(value.to_string());
                    }
                    return Ok(());
                }
                // asm-ui.js jointSections: the limits, kept per value; one value's own limit in the one-value shape
                let keyed = |x: &Value| -> Vec<(String, (f64, f64))> {
                    dof.iter().filter_map(|k| A::limit_of(x, Some(k)).map(|l| ((*k).to_string(), l))).collect()
                };
                let store = |x: &mut Value, m: &[(String, (f64, f64))]| {
                    let lim = |l: (f64, f64)| jv::obj(vec![("min", jv::num(l.0)), ("max", jv::num(l.1))]);
                    match m {
                        [] => jv::del(x, "limits"),
                        [(k, l)] if dof.len() == 1 && dof.first() == Some(&k.as_str()) => jv::set(x, "limits", lim(*l)),
                        _ => jv::set(x, "limits", Value::Object(m.iter().map(|(k, l)| (k.clone(), lim(*l))).collect())),
                    }
                };
                if let Some(k) = field.strip_prefix("limit:") {
                    let deg = matches!(k, "angle" | "tiltX" | "tiltY" | "slotDir");
                    let (k, on) = (k.to_string(), value == "on");
                    set_j(self, &tr("asm.limits.undo"), &|x| {
                        let mut m = keyed(x);
                        m.retain(|(q, _)| *q != k);
                        if on {
                            let v = jv::as_num(x.get(&k)).unwrap_or(0.0);
                            let w = if deg { 90.0 } else { 50.0 };
                            let at = dof.iter().position(|d| *d == k).unwrap_or(0);
                            let pos = m.iter().filter(|(q, _)| dof.iter().position(|d| d == q).unwrap_or(0) < at).count();
                            m.insert(pos.min(m.len()), (k.clone(), (js::min(v, 0.0), js::max(v, 0.0) + w)));
                        }
                        store(x, &m);
                    });
                    return Ok(());
                }
                let end = if field.starts_with("limit-min:") {
                    Some(true)
                } else if field.starts_with("limit-max:") {
                    Some(false)
                } else {
                    None
                };
                if let Some(is_min) = end {
                    let k = field.get(10..).unwrap_or("").to_string();
                    let Some(v) = num(value) else {
                        self.note = tr("app.input.number");
                        return Ok(());
                    };
                    let mut m = keyed(&j);
                    if let Some((_, l)) = m.iter_mut().find(|(q, _)| *q == k) {
                        if is_min {
                            l.0 = v;
                        } else {
                            l.1 = v;
                        }
                    }
                    let probe = Value::Object(
                        m.iter().map(|(k, l)| (k.clone(), jv::obj(vec![("min", jv::num(l.0)), ("max", jv::num(l.1))]))).collect(),
                    );
                    if let Err(e) = A::read_limits(&probe, &ty) {
                        self.note = e;
                        return Ok(());
                    }
                    set_j(self, &tr("asm.limits.undo"), &|x| {
                        let mut mm = keyed(x);
                        if let Some((_, l)) = mm.iter_mut().find(|(q, _)| *q == k) {
                            if is_min {
                                l.0 = v;
                            } else {
                                l.1 = v;
                            }
                        }
                        store(x, &mm);
                    });
                    return Ok(());
                }
                Err(format!("part.asm.set: no joint field {field}"))
            }
            Some((k, iid)) if k == "inst" && st.inst(&iid).is_some() => {
                if field == "name" {
                    let v = clean_name(value, 80);
                    if !v.is_empty() {
                        self.asm_commit(&tr("app.undo.rename"), |a| {
                            if let Some(i) = a
                                .get_mut("instances")
                                .and_then(Value::as_array_mut)
                                .and_then(|l| l.iter_mut().find(|i| jv::is_str(i.get("id"), &iid)))
                            {
                                jv::set(i, "name", v.as_str().into());
                            }
                        });
                    }
                    return Ok(());
                }
                self.asm_tool_set(Some(&iid), field, value)
            }
            _ => {
                if field == "name" {
                    let v = clean_name(value, 120);
                    if !v.is_empty() {
                        self.asm_commit(&tr("app.undo.rename_assembly"), |a| jv::set(a, "name", v.as_str().into()));
                    }
                    return Ok(());
                }
                self.asm_tool_set(None, field, value)
            }
        }
    }

    /// partcraft.js `editState()` in the assembly.
    pub fn asm_edit_state(&self, cmd: &str, clip: Option<&Value>) -> Result<(), String> {
        let Some(st) = self.asm.as_ref() else { return Err(tr("app.edit.select_part")) };
        let inst = st.sel.as_ref().is_some_and(|(k, _)| k == "inst");
        let can = if inst {
            Ok(())
        } else if st.sel.is_some() {
            Err(tr("app.edit.joint_no_copy"))
        } else {
            Err(tr("app.edit.select_part"))
        };
        let word = clip.map(C::describe_clip).unwrap_or_default();
        let paste = match clip.map(|c| jv::s(c, "kind")) {
            Some("instance") => Ok(()),
            Some(k) => {
                Err(trv(if k == "sketch" { "app.edit.clip_sketch_to_sketch" } else { "app.edit.clip_features_to_part" }, &[("what", word)]))
            }
            None => Err(tr("app.edit.nothing_to_paste_part")),
        };
        match cmd {
            "copy" | "duplicate" => can,
            "paste" | "paste-in-place" => paste,
            "cut" if !inst => can,
            "cut" => {
                if insts(&st.asm).len() > 1 {
                    Ok(())
                } else {
                    Err(tr("app.edit.asm_keeps_one"))
                }
            }
            "delete" => {
                if st.sel.is_some() {
                    Ok(())
                } else {
                    Err(tr("app.edit.select_part_or_joint"))
                }
            }
            "select-all" => Err(tr("app.edit.parts_one_at_a_time")),
            _ => {
                if st.sel.is_some() || st.joint_pick.is_some() {
                    Ok(())
                } else {
                    Err(tr("app.edit.nothing_selected"))
                }
            }
        }
    }

    /// partcraft.js `pastePlace(clip, inPlace)`.
    fn paste_place(&self, c: &Value, in_place: bool) -> Result<Value, String> {
        let place: V::M4 = {
            let a = jv::arr(c.get("place"));
            let mut m = [0.0; 16];
            for (i, x) in m.iter_mut().enumerate() {
                *x = a.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
            }
            m
        };
        if in_place {
            return Ok(m4_json(&place));
        }
        let read = P::read_part(c.get("part").unwrap_or(&Value::Null))?;
        let want = P::to_json(&read);
        let st = self.asm.as_ref().ok_or("no assembly")?;
        let mut bbox = None;
        if let Some(ps) = st.asm.get("parts").and_then(Value::as_object) {
            for (pid, p) in ps {
                if st.stats_of(pid).is_some() && P::read_part(p.get("part").unwrap_or(&Value::Null)).is_ok_and(|x| P::to_json(&x) == want) {
                    bbox = st.bbox_of(pid);
                }
            }
        }
        if bbox.is_none() {
            let r = kernel::regenerate(&read, None);
            bbox = r.stats.as_ref().and_then(|s| s.get("bbox")).map(|b| (p3(b.get("min")), p3(b.get("max"))));
        }
        let mut min_x = 0.0;
        if let Some((lo, hi)) = bbox {
            min_x = f64::INFINITY;
            for x in [lo[0], hi[0]] {
                for y in [lo[1], hi[1]] {
                    for z in [lo[2], hi[2]] {
                        min_x = js::min(min_x, V::apply_mat(&place, [x, y, z])[0]);
                    }
                }
            }
        }
        Ok(m4_json(&V::mat_mul(&V::translation([self.place_beside() + 20.0 - min_x, 0.0, 0.0]), &place)))
    }

    /// partcraft.js `asmEdit(cmd)` → the clipboard afterwards.
    pub fn asm_edit_act(&mut self, cmd: &str, clip: Option<Value>) -> Option<Value> {
        if let Err(why) = self.asm_edit_state(cmd, clip.as_ref()) {
            self.note = why;
            return clip;
        }
        let Some(st) = self.asm.as_mut() else { return clip };
        match cmd {
            "delete" => {
                self.asm_delete(false);
                return clip;
            }
            "select-none" => {
                st.sel = None;
                st.joint_pick = None;
                self.asm_scene();
                return clip;
            }
            "copy" | "cut" => {
                let id = st.sel.clone().map(|(_, id)| id).unwrap_or_default();
                let c = C::copy_instance(&st.asm, &id);
                let name = st.inst_name(&id);
                if cmd == "cut" {
                    self.asm_delete(false);
                } else {
                    self.note = trv("app.clip.part_copied", &[("name", name)]);
                }
                return Some(c);
            }
            _ => {}
        }
        let in_place = cmd == "paste-in-place";
        let c = if cmd == "duplicate" { st.sel.clone().map(|(_, id)| C::copy_instance(&st.asm, &id)) } else { clip.clone() };
        let Some(c) = c else { return clip };
        let place = match self.paste_place(&c, in_place) {
            Ok(p) => p,
            Err(e) => {
                self.note = trv("app.clip.paste_failed", &[("error", e)]);
                return clip;
            }
        };
        let label = match cmd {
            "duplicate" => tr("app.undo.duplicate"),
            "paste" => tr("app.undo.paste"),
            _ => tr("app.undo.paste_in_place"),
        };
        match self.asm_commit(&label, |a| C::paste_instance(a, &c, &place)) {
            Some(Ok(inst)) => {
                if let Some(st) = self.asm.as_mut() {
                    st.sel = Some(("inst".into(), jv::s(&inst, "id").to_string()));
                }
                let key = match (in_place, cmd == "duplicate") {
                    (true, true) => "app.clip.part_duplicated_in_place",
                    (true, false) => "app.clip.part_pasted_in_place",
                    (false, true) => "app.clip.part_duplicated",
                    (false, false) => "app.clip.part_pasted",
                };
                self.note = trv(key, &[("name", jv::s(&inst, "name").to_string())]);
                self.asm_scene();
            }
            Some(Err(e)) => self.note = trv("app.clip.paste_failed", &[("error", e)]),
            None => {}
        }
        clip
    }

    /// The facts and the placement's problems in words (`part.asm=facts`, for scripts and the parity kit).
    pub fn asm_facts_words(&self) -> String {
        let Some(st) = self.asm.as_ref() else { return String::new() };
        crate::partasmui::facts(self).into_iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(" · ")
            + " | "
            + &st.problems.iter().map(|p| jv::s(p, "message").to_string()).collect::<Vec<_>>().join(" | ")
    }

    /// `part.asm=<op>`.
    pub fn asm_cmd(&mut self, name: &str) -> Result<Option<(String, Vec<u8>, String)>, String> {
        let (k, v) = name.split_once(':').unwrap_or((name, ""));
        match k {
            "new" => self.asm_enter(),
            "back" => self.asm_back(),
            "close" => {
                return Ok(match self.asm_close((!v.is_empty()).then_some(v)) {
                    Err(saved) => saved,
                    Ok(_) => None,
                });
            }
            "save" => return Ok(self.asm_save_bytes()),
            "insert-copy" => self.asm_insert_copy(),
            "joint" => self.asm_start_joint(),
            "check" => self.asm_check(),
            "edit" => self.asm_edit_part(),
            "edit-inst" => {
                if let Some(st) = self.asm.as_mut() {
                    st.sel = Some(("inst".into(), v.to_string()));
                    st.joint_pick = None;
                }
                self.asm_edit_part();
            }
            "delete" => self.asm_delete(v == "yes"),
            "answer" => {
                let q = self.asm.as_mut().and_then(|s| s.ask.take());
                match q {
                    Some("remove") if v == "remove" => self.asm_delete(true),
                    Some("close" | "close-new") => {
                        if let Err(saved) = self.asm_close(Some(v)) {
                            return Ok(saved);
                        }
                    }
                    _ => {}
                }
            }
            "ground" => {
                let Some(st) = self.asm.as_ref() else { return Ok(None) };
                let iid = st.sel.as_ref().filter(|(k, _)| k == "inst").map(|(_, id)| id.clone()).unwrap_or_default();
                let n = insts(&st.asm).iter().position(|i| jv::is_str(i.get("id"), &iid));
                if let Some(n) = n.filter(|n| *n > 0) {
                    self.asm_commit(&tr("app.undo.ground"), |a| {
                        if let Some(l) = a.get_mut("instances").and_then(Value::as_array_mut)
                            && n < l.len()
                        {
                            let i = l.remove(n);
                            l.insert(0, i);
                        }
                    });
                }
            }
            "sel" => {
                let Some(st) = self.asm.as_mut() else { return Ok(None) };
                let (kind, id) = v.split_once(':').unwrap_or((v, ""));
                let known = match kind {
                    "" => true,
                    "inst" => st.inst(id).is_some(),
                    "joint" => st.joint(id).is_some(),
                    _ => false,
                };
                if !known {
                    return Err(format!("part.asm: no {kind} {id}"));
                }
                st.sel = if kind.is_empty() { None } else { Some((kind.to_string(), id.to_string())) };
                st.joint_pick = None;
                self.asm_scene();
            }
            "pick" => {
                let (iid, face) = v.split_once(':').unwrap_or((v, ""));
                if iid.is_empty() {
                    self.asm_pick(None);
                    return Ok(None);
                }
                let st = self.asm.as_ref().ok_or("part.asm: no assembly")?;
                let pid = st.inst(iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
                let fi = st
                    .builds
                    .get(&pid)
                    .and_then(|b| b.regen.faces.as_ref())
                    .and_then(Value::as_array)
                    .and_then(|l| l.iter().position(|f| jv::s(f, "name") == face));
                self.asm_pick(fi.map(|f| (iid.to_string(), f)));
            }
            "motion" => self.asm_motion(),
            "clash-hide" => {
                let (a, b) = v.split_once('|').unwrap_or((v, ""));
                let (a, b) = (a.to_string(), b.to_string());
                self.asm_clash_filter(&tr("asm.clash.undo.hide_pair"), |x| A::set_pair_hidden(x, &a, &b, true));
            }
            "clash-kind" => {
                let (kind, on) = v.split_once('=').unwrap_or((v, ""));
                if !A::CLASH_KINDS.contains(&kind) {
                    return Err(format!("part.asm: no interference kind {kind}"));
                }
                let (kind, hide) = (kind.to_string(), on != "on");
                self.asm_clash_filter(&tr("asm.clash.undo.kind"), |x| A::set_kind_hidden(x, &kind, hide));
            }
            "clash-show-pairs" => {
                let pairs: Vec<(String, String)> = self
                    .asm
                    .as_ref()
                    .map(|s| {
                        jv::arr(s.asm.get("clashFilter").and_then(|c| c.get("pairs")))
                            .iter()
                            .map(|p| (js::str_of(jv::arr(Some(p)).first()), js::str_of(jv::arr(Some(p)).get(1))))
                            .collect()
                    })
                    .unwrap_or_default();
                self.asm_clash_filter(&tr("asm.clash.undo.show_pairs"), |x| {
                    for (a, b) in &pairs {
                        A::set_pair_hidden(x, a, b, false);
                    }
                });
            }
            "key" => match v {
                "Escape" => {
                    // the gizmo's Esc comes first (partcraft.js: asmTools.keydown before the assembly's keys)
                    if self.asm_tools_escape() {
                        return Ok(None);
                    }
                    let Some(st) = self.asm.as_mut() else { return Ok(None) };
                    if st.joint_pick.take().is_some() {
                        self.note = tr("app.joint.cancelled");
                    } else if let Some(st) = self.asm.as_mut() {
                        st.sel = None;
                    }
                    self.asm_scene();
                }
                "Delete" => self.asm_delete(false),
                "j" => self.asm_start_joint(),
                "f" => self.fit(),
                _ => {}
            },
            "facts" => self.note = self.asm_facts_words(),
            _ => return self.asm_tool_cmd(k, v),
        }
        Ok(None)
    }

    // ------------------------------------------------------------------ the view

    /// The assembly's scene: one mesh per part, a copy per instance at its (shown) placement.
    pub fn asm_scene(&mut self) {
        use cc_render::mesh3d::{Instance3, MeshData};
        // asm-ui.js afterShow: the gizmo's stand-in at the middle of its parts again
        self.asm_place_proxy();
        let Some(st) = self.asm.as_ref() else { return };
        let shown = self.asm_display_world();
        let mut meshes: Vec<MeshData> = Vec::new();
        let mut mesh_of: HashMap<String, usize> = HashMap::new();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut instances = Vec::new();
        let mut inst_ids = Vec::new();
        for i in insts(&st.asm) {
            let pid = js::str_of(i.get("part"));
            let Some(b) = st.builds.get(&pid) else { continue };
            let Some(m) = b.regen.raw_mesh.as_ref() else { continue };
            let mi = match mesh_of.get(&pid) {
                Some(mi) => *mi,
                None => {
                    let mut tris = Vec::with_capacity(m.indices.len() * 3);
                    for &vi in &m.indices {
                        let k = vi as usize * 3;
                        let p = [
                            m.positions.get(k).copied().unwrap_or(0.0),
                            m.positions.get(k + 1).copied().unwrap_or(0.0),
                            m.positions.get(k + 2).copied().unwrap_or(0.0),
                        ];
                        tris.extend_from_slice(&[p[0], -p[2], p[1]]);
                    }
                    let segs: Vec<f64> =
                        jv::arr(b.regen.mesh.as_ref().and_then(|x| x.get("edgeSegs"))).iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
                    let mut e = Vec::with_capacity(segs.len());
                    for [x, y, z] in segs.as_chunks::<3>().0 {
                        e.extend_from_slice(&[*x, -*z, *y]);
                    }
                    let n = tris.len() / 9;
                    meshes.push(MeshData::from_triangles(&tris, &vec![crate::partview::BODY; n], &e));
                    mesh_of.insert(pid.clone(), meshes.len() - 1);
                    meshes.len() - 1
                }
            };
            let iid = js::str_of(i.get("id"));
            let w = shown.iter().find(|(k, _)| *k == iid).map(|(_, m)| *m).unwrap_or(IDENT);
            // the view is Z up: Zup · W · Zup⁻¹
            let zm: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
            let zi: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
            let mm = V::mat_mul(&zm, &V::mat_mul(&w, &zi));
            if let Some((blo, bhi)) = st.bbox_of(&pid) {
                for k in 0..8 {
                    let c = [
                        if k & 1 == 0 { blo[0] } else { bhi[0] },
                        if k & 2 == 0 { blo[1] } else { bhi[1] },
                        if k & 4 == 0 { blo[2] } else { bhi[2] },
                    ];
                    let p = V::apply_mat(&w, c);
                    let q = [p[0], -p[2], p[1]];
                    for a in 0..3 {
                        if let (Some(l), Some(h), Some(x)) = (lo.get_mut(a), hi.get_mut(a), q.get(a)) {
                            *l = l.min(*x);
                            *h = h.max(*x);
                        }
                    }
                }
            }
            instances.push(Instance3 { mesh: mi, m: mm, tint: None, hidden: false });
            inst_ids.push(iid);
        }
        let (min, max) = if lo[0].is_finite() { (lo, hi) } else { ([-50.0; 3], [50.0; 3]) };
        self.asm_bump_scene(meshes, instances, min, max);
        self.hover = None;
        self.asm_inst_ids = inst_ids;
        self.asm_retint();
    }

    /// The selected copy (or the first face of a joint being made) and the one under the pointer, tinted.
    pub fn asm_retint(&mut self) {
        let Some(st) = self.asm.as_ref() else { return };
        let (th, ts) = self.tints;
        let sel = match (&st.sel, &st.joint_pick) {
            (Some((k, id)), _) if k == "inst" => Some(id.clone()),
            (_, Some(Some((a, _)))) => Some(a.clone()),
            _ => None,
        };
        let hover = st.hover.clone();
        let ids = self.asm_inst_ids.clone();
        self.scene.set_tints(&|i| {
            let id = ids.get(i)?;
            if Some(id) == sel.as_ref() {
                Some(ts)
            } else if Some(id) == hover.as_ref() {
                Some(th)
            } else {
                None
            }
        });
    }

    /// The part copy and face under a view pixel: (instance id, face index, point in the part's frame).
    pub fn asm_hit(&self, sx: f64, sy: f64) -> Option<(String, Option<usize>, [f64; 3])> {
        let st = self.asm.as_ref()?;
        let (o, d) = self.view.cam.ray(sx, sy);
        let (o, d) = ([o[0], o[2], -o[1]], [d[0], d[2], -d[1]]);
        let shown = self.asm_display_world();
        let mut best: Option<(f64, String, Option<usize>, [f64; 3])> = None;
        for i in insts(&st.asm) {
            let pid = js::str_of(i.get("part"));
            let iid = js::str_of(i.get("id"));
            let (Some(b), Some(w)) = (st.builds.get(&pid), shown.iter().find(|(k, _)| *k == iid).map(|(_, m)| *m)) else { continue };
            let Some(m) = b.regen.raw_mesh.as_ref() else { continue };
            let inv = A::rigid_inverse(&w);
            let lo = V::apply_mat(&inv, o);
            let ld = V::apply_dir(&inv, d);
            let pt = |vi: u32| -> [f64; 3] {
                let k = vi as usize * 3;
                [
                    m.positions.get(k).copied().unwrap_or(0.0),
                    m.positions.get(k + 1).copied().unwrap_or(0.0),
                    m.positions.get(k + 2).copied().unwrap_or(0.0),
                ]
            };
            for (t, tri) in m.indices.as_chunks::<3>().0.iter().enumerate() {
                let [a, b2, c] = tri.map(pt);
                if let Some(h) = crate::partview::ray_tri(lo, ld, a, b2, c)
                    && best.as_ref().is_none_or(|x| h < x.0)
                {
                    let face = m.tri_face.as_deref().and_then(|tf| tf.get(t)).and_then(|&f| usize::try_from(f).ok());
                    best = Some((h, iid.clone(), face, [lo[0] + ld[0] * h, lo[1] + ld[1] * h, lo[2] + ld[2] * h]));
                }
            }
        }
        best.map(|(_, i, f, p)| (i, f, p))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() })
    }

    #[test]
    fn an_assembly_of_two_brackets_joined_checked_and_undone() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "").unwrap();
        run(&mut app, "part.asm", "new").unwrap();
        let pv = app.part.as_ref().unwrap();
        assert!(pv.in_asm());
        assert_eq!(pv.asm.as_ref().unwrap().world.len(), 1);
        run(&mut app, "part.asm", "insert-copy").unwrap();
        run(&mut app, "part.asm", "joint").unwrap();
        run(&mut app, "part.asm", "pick:i2:f2.end").unwrap();
        run(&mut app, "part.asm", "pick:i3:f2.start").unwrap();
        let st = app.part.as_ref().unwrap().asm.as_ref().unwrap();
        assert_eq!(cc_part::jv::arr(st.asm.get("joints")).len(), 1, "{}", app.part.as_ref().unwrap().note);
        run(&mut app, "part.asm.set", "value:angle=30").unwrap();
        run(&mut app, "part.asm.set", "limit:offset=on").unwrap();
        run(&mut app, "part.asm", "check").unwrap();
        run(&mut app, "part.undo", "").unwrap();
        run(&mut app, "part.undo", "").unwrap();
        // hostile fields: refused or a note, never a panic
        assert!(run(&mut app, "part.asm.set", "type=../../x").is_err());
        run(&mut app, "part.asm.set", "value:angle=1e999").unwrap();
        run(&mut app, "part.asm.set", &format!("limit-min:offset={}", "9".repeat(400))).unwrap();
        run(&mut app, "part.asm", "pick:nope:nothing").unwrap();
        run(&mut app, "part.asm", "sel:inst:i3").unwrap();
        run(&mut app, "part.asm", "edit").unwrap();
        run(&mut app, "part.param", "length=150").unwrap();
        run(&mut app, "part.asm", "back").unwrap();
        run(&mut app, "part.asm", "close").unwrap();
        assert!(app.part.as_ref().unwrap().asm.is_some(), "asks first: changes not saved");
        run(&mut app, "part.asm", "answer:discard").unwrap();
        assert!(app.part.as_ref().unwrap().asm.is_none());
    }

    #[test]
    fn bom_exploded_views_moves_and_exports() {
        let saved: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::default();
        let mut app = UiApp::default();
        let s2 = saved.clone();
        app.services.save = Some(Box::new(move |name: &str, _bytes: Vec<u8>| {
            s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(name.to_string());
            Ok(Some(name.to_string()))
        }));
        run(&mut app, "part.sample", "").unwrap();
        run(&mut app, "part.asm", "new").unwrap();
        run(&mut app, "part.asm", "insert-copy").unwrap();
        run(&mut app, "part.asm", "sel:inst:i3").unwrap();
        run(&mut app, "part.asm.set", "part:material=0").unwrap();
        run(&mut app, "part.asm.set", "part:density=1e400").unwrap();
        run(&mut app, "part.asm.set", "move=10,NaN,5").unwrap();
        run(&mut app, "part.asm.set", "turn=q,45").unwrap();
        assert!(app.part.as_ref().unwrap().asm_bom_words().contains(" g"));
        run(&mut app, "part.asm", "explode-auto:radial").unwrap();
        run(&mut app, "part.asm", "explode-t:-5").unwrap();
        assert!(app.part.as_ref().unwrap().asm_steps_words().contains("1."));
        assert!(run(&mut app, "part.asm", "step-dir:../x").is_err());
        for k in ["stl", "3mf", "ifc@1760700000000", "bom", "step", "drawing"] {
            run(&mut app, "part.export", k).unwrap();
        }
        let names = saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        assert_eq!(names.len(), 4, "{names:?}");
    }
}
