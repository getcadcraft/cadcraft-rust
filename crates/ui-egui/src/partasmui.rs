//! PartCraft's assembly panels in the app (M13.11): partcraft.js `renderAsmTree` (the part copies and the joints),
//! `renderAsmFacts`, `renderAsmProps` (the assembly, a joint with its values, centres, limits and motion check, a
//! part copy), the interference block with its filters, asm-ui.js (M13.12: part properties, moving by hand, the BOM,
//! exploded views, the assembly's exports), the Assembly group of the toolbar, the questions (remove a
//! joined part, close with changes), and the view's picking.

use cc_part::{asm as A, bom as BOM, js, jv, vec as V};
use egui::RichText;
use serde_json::Value;
use std::collections::HashMap;

use crate::partpanel::{Cmd, cmd};
use crate::partview::{PartView, tr, trv};
use crate::theme::Tokens;

/// asm-ui.js's page state (exploded view shown, slider, the new step, the face it waits for).
#[derive(Clone, Debug, Default)]
pub struct ToolsSt {
    pub view_id: Option<String>,
    pub t: f64,
    pub draft: Option<StepDraft>,
    pub await_face: bool,
    /// the move / turn gizmo (partgizmo.rs)
    pub gizmo: Option<crate::partgizmo::Gizmo>,
}

/// asm-ui.js `st.draft`: the step being made.
#[derive(Clone, Debug, Default)]
pub struct StepDraft {
    pub insts: Vec<String>,
    pub dir: String,
    pub dist: f64,
    pub face_dir: Option<[f64; 3]>,
}

fn f3(v: f64) -> String {
    V::fmt(v, 3)
}

/// renderAsmFacts's rows.
pub fn facts(pv: &PartView) -> Vec<(String, String)> {
    let Some(st) = pv.asm.as_ref() else { return Vec::new() };
    let free = st.problems.iter().filter(|x| jv::truthy(x.get("free"))).count();
    let parts = st.asm.get("parts").and_then(Value::as_object).map_or(0, serde_json::Map::len);
    let clash = match &st.clashes {
        None => tr("app.asm.not_checked"),
        Some(l) => {
            let n = A::filter_clashes(&st.asm, l).0.len();
            if n > 0 { cc_part::i18n::t("asm.clash.facts", &[("n", cc_part::i18n::F::from(n))]) } else { tr("app.asm.none") }
        }
    };
    vec![
        (tr("app.asm.tree_parts"), parts.to_string()),
        (tr("app.asm.facts_instances"), jv::arr(st.asm.get("instances")).len().to_string()),
        (tr("app.asm.tree_joints"), jv::arr(st.asm.get("joints")).len().to_string()),
        (tr("app.asm.facts_not_joined"), if free > 0 { free.to_string() } else { String::new() }),
        (tr("app.asm.facts_interference"), clash),
    ]
}

const DIRS: [(&str, [f64; 3]); 6] = [
    ("x", [1.0, 0.0, 0.0]),
    ("-x", [-1.0, 0.0, 0.0]),
    ("y", [0.0, 1.0, 0.0]),
    ("-y", [0.0, -1.0, 0.0]),
    ("z", [0.0, 0.0, 1.0]),
    ("-z", [0.0, 0.0, -1.0]),
];

fn dir_of(k: &str) -> Option<[f64; 3]> {
    DIRS.iter().find(|(d, _)| *d == k).map(|(_, v)| *v)
}

/// The step direction choices (`dirLabels`).
fn dir_labels() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = [("x", "+X"), ("-x", "−X"), ("y", "+Y"), ("-y", "−Y"), ("z", "+Z"), ("-z", "−Z")]
        .iter()
        .map(|(k, l)| ((*k).to_string(), (*l).to_string()))
        .collect();
    v.push(("face".into(), tr("ui.asm.dir.face")));
    v.push(("drag".into(), tr("ui.asm.dir.drag")));
    v
}

impl PartView {
    /// What is drawn: the placement, or the exploded view at the slider's position (asm-ui.js `displayWorld`).
    pub fn asm_display_world(&self) -> Vec<(String, V::M4)> {
        let Some(st) = self.asm.as_ref() else { return Vec::new() };
        // moving a part by hand shows the assembly as placed
        if st.tools.gizmo.as_ref().is_some_and(|g| g.purpose == "move") {
            return st.world.clone();
        }
        match self.asm_active_view().filter(|v| !jv::arr(v.get("steps")).is_empty()) {
            Some(v) => cc_part::explode::exploded(&st.world, &cc_part::explode::offsets(Some(&v), st.tools.t)),
            None => st.world.clone(),
        }
    }

    /// The exploded view shown (`activeView`).
    pub fn asm_active_view(&self) -> Option<Value> {
        let st = self.asm.as_ref()?;
        let id = st.tools.view_id.as_deref()?;
        jv::arr(st.asm.get("views")).iter().find(|v| jv::is_str(v.get("id"), id)).cloned()
    }

    /// A part copy's middle in the world (`centreOf`).
    pub(crate) fn asm_centre_of(&self, iid: &str, world: &[(String, V::M4)]) -> [f64; 3] {
        let Some(st) = self.asm.as_ref() else { return [0.0; 3] };
        let pid = st.inst(iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
        let c = st.bbox_of(&pid).map_or([0.0; 3], |(lo, hi)| [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]);
        let w = world.iter().find(|(k, _)| k == iid).map(|(_, m)| *m);
        match w {
            Some(w) => V::apply_mat(&w, c),
            None => [f64::NAN; 3],
        }
    }

    /// Esc with an assembly tool waiting (the gizmo): whether it took the key.
    pub fn asm_tools_escape(&mut self) -> bool {
        self.asm_gizmo_escape()
    }

    /// asm-ui.js `reset()`.
    pub fn asm_tools_reset(&mut self) {
        if let Some(st) = self.asm.as_mut() {
            st.tools = ToolsSt { t: 1.0, ..ToolsSt::default() };
        }
        self.scene.top_lines = None;
    }

    /// `moveBy(id, opts, label)`.
    fn asm_move_by(&mut self, iid: &str, mv: Option<[f64; 3]>, rotate: Option<([f64; 3], f64, [f64; 3])>, label: &str) {
        let Some(st) = self.asm.as_ref() else { return };
        let Some(inst) = st.inst(iid).cloned() else { return };
        let place: V::M4 = {
            let a = jv::arr(inst.get("place"));
            let mut m = [0.0; 16];
            for (i, x) in m.iter_mut().enumerate() {
                *x = a.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
            }
            m
        };
        let np = match A::move_place(&place, mv, rotate) {
            Ok(m) => m,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let iid2 = iid.to_string();
        let r = self.asm_commit(label, |a| A::move_instance(a, &iid2, &jv::nums(&np)));
        match r {
            Some(Err(e)) => {
                // the JS commits first and throws inside: the undo step is there with nothing changed
                self.note = e;
            }
            _ => self.note = trv("ui.asm.move.done", &[("name", jv::s(&inst, "name").to_string())]),
        }
    }

    /// The assembly tools' fields (asm-ui.js instSections).
    pub fn asm_tool_set(&mut self, inst: Option<&str>, field: &str, value: &str) -> Result<(), String> {
        let Some(iid) = inst else { return Err(format!("part.asm.set: no field {field}")) };
        let st = self.asm.as_ref().ok_or("part.asm.set: no assembly")?;
        let pid = st.inst(iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
        let p = st.part_of(&pid).cloned().unwrap_or(Value::Null);
        let set_p = |pv: &mut PartView, label: &str, f: &dyn Fn(&mut Value)| {
            pv.asm_commit(label, |a| {
                if let Some(x) = a.get_mut("parts").and_then(|ps| ps.get_mut(&pid)) {
                    f(x);
                }
            });
        };
        match field {
            "part:number" | "part:description" => {
                let k = field.trim_start_matches("part:").to_string();
                let v = BOM::read_part_props(Some(&jv::obj(vec![(k.as_str(), value.into())])))?
                    .get(&k)
                    .cloned()
                    .filter(|v| jv::truthy(Some(v)));
                let label = tr(if k == "number" { "ui.asm.part.number" } else { "ui.asm.part.description" });
                set_p(self, &label, &|x| match &v {
                    Some(v) => jv::set(x, &k, v.clone()),
                    None => jv::del(x, &k),
                });
            }
            "part:material" => {
                let mats = BOM::materials();
                if value == "custom" {
                    let d = p.get("material").and_then(|m| jv::finite(m.get("density"))).filter(|d| *d != 0.0).unwrap_or(1.0);
                    set_p(self, &tr("part.material.undo"), &|x| {
                        jv::set(x, "material", jv::obj(vec![("name", "Custom".into()), ("density", jv::num(d))]))
                    });
                } else {
                    let m = if value.is_empty() { None } else { value.parse::<usize>().ok().and_then(|i| mats.get(i).cloned()) };
                    set_p(self, &tr("part.material.undo"), &|x| match &m {
                        Some((n, d)) => jv::set(x, "material", jv::obj(vec![("name", n.as_str().into()), ("density", jv::num(*d))])),
                        None => jv::del(x, "material"),
                    });
                }
            }
            "part:density" => {
                let v = cc_part::js::str_num(value);
                if value.trim().is_empty() || !v.is_finite() || !(0.001..=BOM::MAX_DENSITY).contains(&v) {
                    self.note = trv("app.input.number_from_to", &[("min", "0.001".into()), ("max", js::num_str(BOM::MAX_DENSITY))]);
                    return Ok(());
                }
                let name =
                    p.get("material").map(|m| jv::s(m, "name").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Custom".into());
                let m = BOM::read_material(Some(&jv::obj(vec![("name", name.as_str().into()), ("density", jv::num(v))])))?;
                set_p(self, &tr("ui.asm.part.density_undo"), &|x| match &m {
                    Some(m) => jv::set(x, "material", m.clone()),
                    None => jv::del(x, "material"),
                });
            }
            "move" => {
                let d: Vec<f64> = value.split(',').map(cc_part::js::str_num).map(|x| if x.is_nan() { 0.0 } else { x }).collect();
                let g = |i: usize| d.get(i).copied().unwrap_or(0.0);
                self.asm_move_by(iid, Some([g(0), g(1), g(2)]), None, &tr("ui.asm.move.go"));
            }
            "turn" => {
                let (ax, a) = value.split_once(',').unwrap_or((value, ""));
                let a = cc_part::js::str_num(a);
                let a = if a.is_nan() { 0.0 } else { a };
                let axis = dir_of(ax).unwrap_or([f64::NAN; 3]);
                let world = self.asm.as_ref().map(|s| s.world.clone()).unwrap_or_default();
                let about = self.asm_centre_of(iid, &world);
                self.asm_move_by(iid, None, Some((axis, a, about)), &tr("ui.asm.turn.go"));
            }
            // Drag to move / Drag to turn (startDrag)
            "drag" => match value {
                "translate" => self.asm_start_drag(iid, crate::gizmo3::Mode::Translate),
                "rotate" => self.asm_start_drag(iid, crate::gizmo3::Mode::Rotate),
                _ => return Err(format!("part.asm.set: drag={value}: translate or rotate")),
            },
            _ => return Err(format!("part.asm.set: no field {field}")),
        }
        Ok(())
    }

    /// One exploded view changed (`setV`).
    pub(crate) fn asm_set_view(&mut self, label: &str, f: impl FnOnce(&mut Value, &str) -> Result<(), String>) {
        let Some(vid) = self.asm_active_view().map(|v| jv::s(&v, "id").to_string()) else { return };
        let r = self.asm_commit(label, |a| f(a, &vid));
        if let Some(Err(e)) = r {
            self.note = e;
        }
    }

    /// The boxes of the copies in the world (`items()` for the automatic explode).
    fn asm_explode_items(&self) -> Vec<(String, cc_part::explode::Box3)> {
        let Some(st) = self.asm.as_ref() else { return Vec::new() };
        jv::arr(st.asm.get("instances"))
            .iter()
            .map(|i| {
                let iid = js::str_of(i.get("id"));
                let bb = st.bbox_of(&js::str_of(i.get("part")));
                let w = st.world_of(&iid);
                let b = match (bb, w) {
                    (Some((lo, hi)), Some(w)) => {
                        let pts: Vec<[f64; 3]> = (0..8u32)
                            .map(|k| {
                                V::apply_mat(
                                    &w,
                                    [
                                        if k & 1 != 0 { hi[0] } else { lo[0] },
                                        if k & 2 != 0 { hi[1] } else { lo[1] },
                                        if k & 4 != 0 { hi[2] } else { lo[2] },
                                    ],
                                )
                            })
                            .collect();
                        let mn = |c: usize| pts.iter().map(|p| p.get(c).copied().unwrap_or(0.0)).fold(f64::INFINITY, js::min);
                        let mx = |c: usize| pts.iter().map(|p| p.get(c).copied().unwrap_or(0.0)).fold(f64::NEG_INFINITY, js::max);
                        cc_part::explode::Box3 { min: [mn(0), mn(1), mn(2)], max: [mx(0), mx(1), mx(2)] }
                    }
                    _ => cc_part::explode::Box3 { min: [0.0; 3], max: [0.0; 3] },
                };
                (iid, b)
            })
            .collect()
    }

    /// The BOM rows (bom.js `bomRows` with the builds' volumes).
    pub fn asm_bom_rows(&self) -> Vec<Value> {
        let Some(st) = self.asm.as_ref() else { return Vec::new() };
        BOM::bom_rows(&st.asm, &|pid: &str| st.stats_of(pid).map(|s| (jv::n(s, "volume"), false)))
    }

    /// The assembly's exports (asm-ui.js `exportCmd`) → (file, bytes, words), or a status line.
    pub fn asm_export(&mut self, kind: &str, now_ms: f64) -> Option<(String, Vec<u8>, String)> {
        let st = self.asm.as_ref()?;
        let insts = jv::arr(st.asm.get("instances"));
        let built = |iid: &Value| st.builds.get(&js::str_of(iid.get("part"))).is_some_and(|b| b.regen.raw_mesh.is_some());
        if st.world.is_empty() || !insts.iter().all(built) {
            self.note = tr("ui.asm.export.building");
            return None;
        }
        let name = crate::partasm::asm_file_base(jv::s(&st.asm, "name"));
        let nparts = st.asm.get("parts").and_then(Value::as_object).map_or(0, serde_json::Map::len);
        let world_of = |i: &Value| st.world_of(&js::str_of(i.get("id"))).unwrap_or([f64::NAN; 16]);
        let mesh_of = |pid: &str| st.builds.get(pid).and_then(|b| b.regen.raw_mesh.as_ref());
        match kind {
            "bom" => {
                let file = format!("{name} - BOM.csv");
                let rows = self.asm_bom_rows();
                let words = trv("ui.asm.export.bom", &[("file", file.clone()), ("n", nparts.to_string())]);
                Some((file, BOM::bom_csv(&rows).into_bytes(), words))
            }
            "ifc" => {
                let file = format!("{name}.ifc");
                let builds: HashMap<String, cc_part::ifc_export::BuiltPart> = st
                    .builds
                    .iter()
                    .filter_map(|(k, b)| {
                        let m = b.regen.raw_mesh.as_ref()?;
                        Some((
                            k.clone(),
                            cc_part::ifc_export::BuiltPart {
                                positions: m.positions.clone(),
                                indices: m.indices.clone(),
                                stats: b.regen.stats.clone(),
                            },
                        ))
                    })
                    .collect();
                let iso: String = crate::civilexport::iso_time(now_ms).chars().take(19).collect();
                let rows = self.asm_bom_rows();
                match cc_part::ifc_export::assembly_ifc(&st.asm, &builds, &st.world, &rows, &iso, Some(&file)) {
                    Ok((text, counts)) => {
                        let words = trv(
                            "ui.asm.export.ifc",
                            &[
                                ("file", file.clone()),
                                ("parts", nparts.to_string()),
                                ("n", insts.len().to_string()),
                                ("props", js::string_of(counts.get("properties"))),
                            ],
                        );
                        Some((file, text.into_bytes(), words))
                    }
                    Err(e) => {
                        self.note = e;
                        None
                    }
                }
            }
            "stl" => {
                let items: Vec<(Vec<f64>, Vec<u32>, V::M4)> = insts
                    .iter()
                    .filter_map(|i| mesh_of(&js::str_of(i.get("part"))).map(|m| (m.positions.clone(), m.indices.clone(), world_of(i))))
                    .collect();
                let (positions, indices) = cc_part::asm_drawing::assembly_mesh(&items);
                let m = cc_part::faces::Mesh { positions, indices, ..Default::default() };
                let file = format!("{name}.stl");
                let words = trv("ui.asm.export.stl", &[("file", file.clone()), ("n", (m.indices.len() / 3).to_string())]);
                Some((file, cc_part::export::stl_binary(&m, &name), words))
            }
            "3mf" => {
                let parts: Vec<(String, String, Vec<f64>, Vec<u32>)> = st
                    .asm
                    .get("parts")
                    .and_then(Value::as_object)
                    .map(|o| {
                        o.iter()
                            .filter_map(|(pid, p)| {
                                mesh_of(pid).map(|m| (pid.clone(), js::string_of(p.get("name")), m.positions.clone(), m.indices.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let items: Vec<(String, V::M4)> = insts.iter().map(|i| (js::str_of(i.get("part")), world_of(i))).collect();
                let file = format!("{name}.3mf");
                let words = trv(
                    "ui.asm.export.3mf",
                    &[("file", file.clone()), ("n", parts.len().to_string()), ("copies", insts.len().to_string())],
                );
                Some((file, cc_part::asm_drawing::assembly_3mf(jv::s(&st.asm, "name"), &parts, &items), words))
            }
            _ => {
                // exactWhy: STEP and the assembly drawing need the exact kernel, which 0.5's mesh build does not have
                self.note = trv("kernel.not_built", &[("what", tr("kernel.what.asm_step"))]);
                None
            }
        }
    }

    /// The new step (asm-ui.js `st.draft ||= …`).
    fn asm_draft(&mut self) -> Option<&mut StepDraft> {
        let st = self.asm.as_mut()?;
        Some(st.tools.draft.get_or_insert_with(|| StepDraft { insts: Vec::new(), dir: "y".into(), dist: 50.0, face_dir: None }))
    }

    /// The assembly tools' commands: exports, exploded views, the new step.
    pub fn asm_tool_cmd(&mut self, k: &str, v: &str) -> Result<Option<(String, Vec<u8>, String)>, String> {
        match k {
            "export" => {
                let (kind, ms) = v.split_once('@').unwrap_or((v, ""));
                let ms = ms.trim().parse::<f64>().ok().filter(|x| x.is_finite()).or(self.clock_ms).unwrap_or_else(crate::partview::now_ms);
                return Ok(self.asm_export(kind, ms));
            }
            "bom" => self.note = self.asm_bom_words(),
            "steps" => self.note = self.asm_steps_words(),
            "explode-view" => {
                let known = v.is_empty()
                    || self.asm.as_ref().is_some_and(|s| jv::arr(s.asm.get("views")).iter().any(|x| jv::is_str(x.get("id"), v)));
                if !known {
                    return Err(format!("part.asm: no exploded view {v}"));
                }
                if let Some(st) = self.asm.as_mut() {
                    st.tools.view_id = (!v.is_empty()).then(|| v.to_string());
                    st.tools.t = 1.0;
                    st.tools.draft = None;
                }
                self.asm_scene();
            }
            "explode-new" => {
                let nv = self.asm_commit(&tr("ui.asm.undo.new_view"), |a| cc_part::explode::new_view(a, None));
                if let (Some(nv), Some(st)) = (nv, self.asm.as_mut()) {
                    st.tools.view_id = Some(jv::s(&nv, "id").to_string());
                    st.tools.t = 1.0;
                    st.tools.draft = Some(StepDraft { insts: Vec::new(), dir: "y".into(), dist: 50.0, face_dir: None });
                }
                self.asm_scene();
            }
            "explode-auto" => {
                let st = self.asm.as_ref().ok_or("no assembly")?;
                if jv::arr(st.asm.get("instances")).len() < 2 {
                    self.note = tr("ui.asm.explode.need_two");
                    return Ok(None);
                }
                let tree: Vec<(String, String, [f64; 3])> = if v == "joints" {
                    A::joint_tree(&st.asm, &st.builds_map(), &st.world)
                        .iter()
                        .map(|x| (js::str_of(x.get("parent")), js::str_of(x.get("child")), crate::partasm::p3(x.get("normal"))))
                        .collect()
                } else {
                    Vec::new()
                };
                let steps = cc_part::explode::auto_steps(&self.asm_explode_items(), v, &tree, 1.25)?;
                if steps.is_empty() {
                    self.note = tr(if v == "joints" { "ui.asm.explode.no_joints" } else { "ui.asm.explode.nothing" });
                    return Ok(None);
                }
                let n = jv::arr(st.asm.get("views")).len() + 1;
                let vname =
                    trv(if v == "joints" { "ui.asm.explode.name_joints" } else { "ui.asm.explode.name_radial" }, &[("n", n.to_string())]);
                let nv = self.asm_commit(&tr("ui.asm.undo.auto"), |a| {
                    let w = cc_part::explode::new_view(a, Some(&vname));
                    let wid = jv::s(&w, "id").to_string();
                    for s in &steps {
                        let _ = cc_part::explode::add_step(a, &wid, &s.insts, s.dir, s.dist);
                    }
                    a.get("views")
                        .and_then(Value::as_array)
                        .and_then(|l| l.iter().find(|x| jv::is_str(x.get("id"), &wid)).cloned())
                        .unwrap_or(w)
                });
                if let (Some(nv), Some(st)) = (nv, self.asm.as_mut()) {
                    st.tools.view_id = Some(jv::s(&nv, "id").to_string());
                    st.tools.t = 1.0;
                    self.note = trv("ui.asm.explode.made", &[("name", jv::s(&nv, "name").to_string()), ("n", steps.len().to_string())]);
                }
                self.asm_scene();
            }
            "explode-name" => {
                let nm: String = v.chars().filter(|c| (*c as u32) >= 0x20).collect::<String>().trim().chars().take(60).collect();
                if !nm.is_empty() {
                    self.asm_set_view(&tr("ui.asm.undo.rename_view"), |a, vid| {
                        if let Some(x) = a
                            .get_mut("views")
                            .and_then(Value::as_array_mut)
                            .and_then(|l| l.iter_mut().find(|x| jv::is_str(x.get("id"), vid)))
                        {
                            jv::set(x, "name", nm.as_str().into());
                        }
                        Ok(())
                    });
                }
            }
            "explode-t" => {
                let x = cc_part::js::str_num(v);
                if let Some(st) = self.asm.as_mut() {
                    st.tools.t = if x.is_finite() { x.clamp(0.0, 1.0) } else { 1.0 };
                }
                self.asm_scene();
            }
            "step-remove" => {
                let sid = v.to_string();
                self.asm_set_view(&tr("ui.asm.undo.remove_step"), |a, vid| {
                    if let Some(x) =
                        a.get_mut("views").and_then(Value::as_array_mut).and_then(|l| l.iter_mut().find(|x| jv::is_str(x.get("id"), vid)))
                        && let Some(steps) = x.get_mut("steps").and_then(Value::as_array_mut)
                    {
                        steps.retain(|s| !jv::is_str(s.get("id"), &sid));
                    }
                    Ok(())
                });
            }
            "step-inst" => {
                let (id, on) = v.split_once('=').unwrap_or((v, ""));
                if let Some(d) = self.asm_draft() {
                    d.insts.retain(|x| x != id);
                    if on == "on" {
                        d.insts.push(id.to_string());
                    }
                }
            }
            "step-dir" => {
                if !(dir_of(v).is_some() || v == "face" || v == "drag") {
                    return Err(format!("part.asm: step direction {v}"));
                }
                if let Some(d) = self.asm_draft() {
                    d.dir = v.to_string();
                    d.face_dir = None;
                }
                if v == "face"
                    && let Some(st) = self.asm.as_mut()
                {
                    st.tools.await_face = true;
                }
            }
            "step-dist" => {
                let x = cc_part::js::str_num(v);
                if v.trim().is_empty() || !x.is_finite() {
                    self.note = tr("app.input.number");
                } else if let Some(d) = self.asm_draft() {
                    d.dist = x;
                }
            }
            "step-add" => {
                let Some(d) = self.asm_draft().cloned() else { return Ok(None) };
                // the boxes are ticked in the copies' order (a Set in the JS keeps the order they were ticked)
                if d.insts.is_empty() {
                    self.note = tr("ui.asm.step.tick");
                    return Ok(None);
                }
                if d.dir == "drag" {
                    self.asm_start_step_drag(d.insts.clone());
                    return Ok(None);
                }
                let dir = if d.dir == "face" { d.face_dir } else { dir_of(&d.dir) };
                let Some(dir) = dir else {
                    self.note = tr("ui.asm.step.face_first");
                    if let Some(st) = self.asm.as_mut() {
                        st.tools.await_face = true;
                    }
                    return Ok(None);
                };
                self.asm_set_view(&tr("ui.asm.undo.step"), |a, vid| cc_part::explode::add_step(a, vid, &d.insts, dir, d.dist).map(|_| ()));
                if let Some(st) = self.asm.as_mut() {
                    st.tools.t = 1.0;
                }
                self.asm_scene();
            }
            // one gesture with the gizmo, in view pixels (partgizmo.rs)
            "gizmo" => self.asm_gizmo_gesture(v)?,
            "explode-delete" => {
                self.asm_set_view(&tr("ui.asm.undo.delete_view"), |a, vid| {
                    if let Some(l) = a.get_mut("views").and_then(Value::as_array_mut) {
                        l.retain(|x| !jv::is_str(x.get("id"), vid));
                    }
                    Ok(())
                });
                if let Some(st) = self.asm.as_mut() {
                    st.tools.view_id = None;
                    st.tools.draft = None;
                }
                self.asm_scene();
            }
            _ => return Err(format!("part.asm: unknown {k}")),
        }
        Ok(None)
    }

    /// asm-ui.js `pick(h)`: a face for the new step's direction, when one is awaited (the face's normal in the world).
    pub fn asm_tools_pick(&mut self, hit: Option<&(String, usize)>) -> bool {
        let Some(st) = self.asm.as_ref() else { return false };
        let waiting = st.tools.await_face && st.tools.draft.as_ref().is_some_and(|d| d.dir == "face") && self.asm_active_view().is_some();
        if !waiting {
            return false;
        }
        let n = hit.and_then(|(iid, fi)| {
            let pid = st.inst(iid).map(|i| js::str_of(i.get("part")))?;
            let f = st.builds.get(&pid)?.regen.faces.as_ref()?.get(*fi)?.clone();
            let w = st.world_of(iid)?;
            f.get("n").map(|n| V::apply_dir(&w, crate::partasm::p3(Some(n))))
        });
        let Some(n) = n else {
            self.note = tr("ui.asm.pick.face");
            return true;
        };
        let n = V::norm(n);
        let r12 = n.map(|c| cc_part::js::str_num(&cc_part::js::to_fixed(c, 12)));
        if let Some(st) = self.asm.as_mut() {
            if let Some(d) = st.tools.draft.as_mut() {
                d.face_dir = Some(r12);
            }
            st.tools.await_face = false;
        }
        self.note = trv("ui.asm.pick.done", &[("dir", n.iter().map(|c| V::fmt(*c, 3)).collect::<Vec<_>>().join(", "))]);
        true
    }

    /// The BOM in words (`part.asm=bom`).
    pub fn asm_bom_words(&self) -> String {
        let rows = self.asm_bom_rows();
        let total: Option<f64> = if !rows.is_empty() && rows.iter().all(|r| jv::as_num(r.get("massTotal")).is_some()) {
            Some(rows.iter().map(|r| jv::n(r, "massTotal")).sum())
        } else {
            None
        };
        let line = |r: &Value| {
            let each = jv::as_num(r.get("massEach")).map_or_else(String::new, |m| trv("ui.asm.mass", &[("mass", V::fmt(m, 3))]));
            [
                js::string_of(r.get("item")),
                js::string_of(r.get("qty")),
                js::string_of(r.get("number")),
                js::string_of(r.get("name")),
                js::string_of(r.get("material")),
                each,
            ]
            .join(" / ")
        };
        let qty: f64 = rows.iter().map(|r| jv::n(r, "qty")).sum();
        let tail = match total {
            None => tr("ui.asm.bom.no_mass"),
            Some(m) => trv(
                if rows.iter().all(|r| jv::truthy(r.get("exact"))) { "ui.asm.bom.total_exact" } else { "ui.asm.bom.total" },
                &[("mass", V::fmt(m, 3)), ("n", js::num_str(qty))],
            ),
        };
        rows.iter().map(line).collect::<Vec<_>>().join(" | ") + " | " + &tail
    }

    /// The exploded view's steps in words (`part.asm=steps`).
    pub fn asm_steps_words(&self) -> String {
        let Some(v) = self.asm_active_view() else { return tr("ui.asm.explode.note") };
        let Some(st) = self.asm.as_ref() else { return String::new() };
        let steps = jv::arr(v.get("steps"));
        let words: Vec<String> = steps.iter().enumerate().map(|(i, s)| step_words(st, i, s)).collect();
        format!("{}: {}", jv::s(&v, "name"), if words.is_empty() { tr("ui.asm.explode.no_steps") } else { words.join(" | ") })
    }
}

fn step_words(st: &crate::partasm::AsmSt, i: usize, s: &Value) -> String {
    let parts: Vec<String> = jv::arr(s.get("insts")).iter().map(|x| st.inst_name(&js::str_of(Some(x)))).collect();
    let dir: Vec<String> = jv::arr(s.get("dir")).iter().map(|c| V::fmt(c.as_f64().unwrap_or(f64::NAN), 3)).collect();
    trv(
        "ui.asm.explode.step",
        &[("n", (i + 1).to_string()), ("parts", parts.join(", ")), ("dist", V::fmt(jv::n(s, "dist"), 3)), ("dir", dir.join(", "))],
    )
}

/// The left panel in the assembly: renderAsmTree + renderAsmFacts.
pub fn tree(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    ui.label(RichText::new(jv::s(&st.asm, "name")).strong().family(crate::theme::bold_family()).color(t.text_strong));
    ui.separator();
    crate::shell::note(ui, t, &tr("app.asm.tree_parts"));
    let problem = |kind: &str, id: &str| st.problems.iter().find(|p| jv::is_str(p.get(kind), id)).map(|p| jv::s(p, "message").to_string());
    for (n, i) in jv::arr(st.asm.get("instances")).iter().enumerate() {
        let id = jv::s(i, "id").to_string();
        let pr = problem("inst", &id);
        let on = st.sel.as_ref().is_some_and(|(k, s)| k == "inst" && *s == id);
        ui.horizontal(|ui| {
            let r = ui.selectable_label(on, RichText::new(jv::s(i, "name")).color(t.text));
            let r = match &pr {
                Some(m) => r.on_hover_text(m),
                None => r,
            };
            if r.double_clicked() {
                *act = cmd("part.asm", format!("edit-inst:{id}"));
            } else if r.clicked() {
                *act = cmd("part.asm", format!("sel:inst:{id}"));
            }
            let flag = if n == 0 {
                tr("app.asm.flag_grounded")
            } else if pr.is_some() {
                tr("app.asm.flag_free")
            } else {
                String::new()
            };
            if !flag.is_empty() {
                ui.label(RichText::new(flag).small().color(t.text_dim));
            }
        });
    }
    crate::shell::note(ui, t, &tr("app.asm.tree_joints"));
    let joints = jv::arr(st.asm.get("joints"));
    for j in joints {
        let id = jv::s(j, "id").to_string();
        let pr = problem("joint", &id);
        let on = st.sel.as_ref().is_some_and(|(k, s)| k == "joint" && *s == id);
        let ty = tr(&format!("asm.joint.type.{}", jv::s(j, "type")));
        ui.horizontal(|ui| {
            let r = ui.selectable_label(on, RichText::new(jv::s(j, "name")).color(if pr.is_some() { t.text_strong } else { t.text }));
            let r = r.on_hover_text(pr.clone().unwrap_or_else(|| ty.clone()));
            if r.clicked() {
                *act = cmd("part.asm", format!("sel:joint:{id}"));
            }
            ui.label(RichText::new(if pr.is_some() { "⚠".to_string() } else { ty.to_lowercase() }).small().color(t.text_dim));
        });
    }
    if joints.is_empty() {
        ui.label(RichText::new(tr("app.asm.no_joints")).color(t.text_dim));
    }
    crate::shell::note(ui, t, &cc_part::i18n::t("ui.left.asm_note", &[("key", cc_part::i18n::F::S("J".into()))]));
    ui.separator();
    ui.label(RichText::new(tr("app.facts.h_assembly")).strong().family(crate::theme::bold_family()).color(t.text_strong));
    egui::Grid::new("pc-asm-facts").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        for (k, v) in facts(pv) {
            ui.label(RichText::new(k).color(t.text_dim));
            ui.label(RichText::new(v).color(t.text));
            ui.end_row();
        }
    });
}

/// A number field that sends `part.asm.set=<field>=<text>` when it is left (partcraft.js `numInput`).
pub fn num_field(ui: &mut egui::Ui, pv: &mut PartView, key: &str, shown: f64, field: &str, act: &mut Cmd) {
    let Some(st) = pv.asm.as_mut() else { return };
    let shown = V::fmt(shown, 6);
    let mut text = st.edits.get(key).cloned().unwrap_or_else(|| shown.clone());
    let r = ui.add_sized([120.0, 26.0], crate::shell::field(&mut text).id_salt(("pc-asm", key)));
    if r.has_focus() {
        st.edits.insert(key.to_string(), text.clone());
    }
    if r.lost_focus() {
        st.edits.remove(key);
        if text.trim() != shown && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *act = cmd("part.asm.set", format!("{field}={}", text.trim()));
        }
    }
}

/// A text field (`textInput`), sent when it is left.
pub fn text_field(ui: &mut egui::Ui, pv: &mut PartView, key: &str, shown: &str, field: &str, act: &mut Cmd) {
    let Some(st) = pv.asm.as_mut() else { return };
    let mut text = st.edits.get(key).cloned().unwrap_or_else(|| shown.to_string());
    let r = ui.add_sized([170.0, 26.0], crate::shell::field(&mut text).id_salt(("pc-asm", key)));
    if r.has_focus() {
        st.edits.insert(key.to_string(), text.clone());
    }
    if r.lost_focus() {
        st.edits.remove(key);
        if text != shown && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *act = cmd("part.asm.set", format!("{field}={text}"));
        }
    }
}

/// A choice (`selectInput`) sending `part.asm.set=<field>=<value>`.
pub fn choice(ui: &mut egui::Ui, id: &str, cur: &str, opts: &[(String, String)], field: &str, act: &mut Cmd) {
    let shown = opts.iter().find(|(v, _)| v == cur).map_or_else(|| cur.to_string(), |(_, l)| l.clone());
    egui::ComboBox::new(id, "").selected_text(shown).width(170.0).show_ui(ui, |ui| {
        for (v, l) in opts {
            if ui.selectable_label(v == cur, l).clicked() && v != cur {
                *act = cmd("part.asm.set", format!("{field}={v}"));
            }
        }
    });
}

/// The words of a joint's end (`endLabel`).
fn end_label(pv: &PartView, e: &Value) -> String {
    let Some(st) = pv.asm.as_ref() else { return String::new() };
    let iid = jv::s(e, "inst");
    let pid = st.inst(iid).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
    let pt = st.part_of(&pid).and_then(|p| p.get("part")).cloned().unwrap_or(Value::Null);
    let pl = e.get("plane").unwrap_or(&Value::Null);
    let plane = match jv::s(pl, "kind") {
        "base" => crate::partview::plane_name(jv::s(pl, "name")),
        "face" => match cc_part::part::feature_by_id(&pt, jv::s(pl, "feature")) {
            Some(f) => trv("app.plane.face_of", &[("name", jv::s(f, "name").to_string())]),
            None => tr("app.plane.fixed"),
        },
        _ => tr("app.plane.fixed"),
    };
    trv("app.joint.end_label", &[("name", st.inst_name(iid)), ("plane", plane)])
}

/// The right panel in the assembly: renderAsmProps.
pub fn props(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    let msg = |ui: &mut egui::Ui, text: &str, bad: bool| {
        ui.label(RichText::new(text).color(if bad { t.text_strong } else { t.text }));
    };
    if let Some(jp) = &st.joint_pick {
        ui.label(RichText::new(tr("app.joint.new")).strong().family(crate::theme::bold_family()).color(t.text_strong));
        msg(ui, &tr(if jp.is_some() { "app.joint.pick_second_face" } else { "app.joint.pick_first_face" }), false);
        crate::shell::note(ui, t, &tr("app.joint.new_note"));
        return;
    }
    let sel = st.sel.clone();
    match sel {
        Some((k, id)) if k == "joint" && st.joint(&id).is_some() => joint_props(ui, pv, t, act, &id),
        Some((k, id)) if k == "inst" && st.inst(&id).is_some() => inst_props(ui, pv, t, act, &id),
        _ => {
            let name = jv::s(&st.asm, "name").to_string();
            ui.label(RichText::new(tr("app.facts.h_assembly")).strong().family(crate::theme::bold_family()).color(t.text_strong));
            ui.horizontal(|ui| {
                ui.label(tr("app.props.name"));
                text_field(ui, pv, "asm-name", &name, "name", act);
            });
            crate::shell::note(ui, t, &tr("app.asm.note"));
            let Some(st) = pv.asm.as_ref() else { return };
            for p in &st.problems {
                msg(ui, jv::s(p, "message"), !jv::truthy(p.get("free")));
            }
            if st.clashes.is_some() || st.asm.get("clashFilter").is_some_and(|c| !c.is_null()) {
                clash_block(ui, pv, t, act);
            }
            root_tools(ui, pv, t, act);
        }
    }
}

fn joint_props(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd, jid: &str) {
    let Some(st) = pv.asm.as_ref() else { return };
    let Some(j) = st.joint(jid).cloned() else { return };
    let ty = jv::s(&j, "type").to_string();
    ui.label(RichText::new(jv::s(&j, "name")).strong().family(crate::theme::bold_family()).color(t.text_strong));
    if let Some(pr) = st.problems.iter().find(|p| jv::is_str(p.get("joint"), jid)) {
        ui.label(RichText::new(jv::s(pr, "message")).color(t.text_strong));
    }
    let types: Vec<(String, String)> = A::JOINT_TYPES.iter().map(|(k, _)| ((*k).to_string(), tr(&format!("asm.joint.type.{k}")))).collect();
    let dof: Vec<&str> = A::joint_dof(&ty).unwrap_or(&[]).to_vec();
    let setup: &[&str] = match ty.as_str() {
        "rigid" => &["angle", "offset"],
        "revolute" => &["offset"],
        "slider" => &["angle"],
        "pinslot" => &["slotDir"],
        _ => &[],
    };
    let label = |k: &str| tr(&format!("asm.joint.value.{}", k.to_lowercase()));
    egui::Grid::new("pc-asm-joint").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(tr("asm.joint.type"));
        choice(ui, "pc-asm-jtype", &ty, &types, "type", act);
        ui.end_row();
        for k in dof.iter().chain(setup) {
            ui.label(label(k));
            ui.vertical(|ui| {
                let v = jv::as_num(j.get(*k)).unwrap_or(0.0);
                num_field(ui, pv, &format!("{jid}:{k}"), v, &format!("value:{k}"), act);
                if dof.contains(k) {
                    let (lo, hi) = pv.joint_span(&j, k);
                    let mut x = v;
                    ui.spacing_mut().slider_width = 150.0;
                    let r = ui
                        .add(egui::Slider::new(&mut x, lo..=hi.max(lo)).show_value(false))
                        .on_hover_text(trv("asm.joint.drag", &[("label", label(k))]));
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        *act = cmd("part.asm.set", format!("value:{k}={}", js::num_str(x)));
                    }
                }
            });
            ui.end_row();
        }
        for (side, key) in [("a", "app.joint.first"), ("b", "app.joint.second")] {
            let e = j.get(side).cloned().unwrap_or(Value::Null);
            ui.label("");
            ui.label(RichText::new(trv(key, &[("end", end_label(pv, &e))])).small().color(t.text_dim));
            ui.end_row();
            let Some(st) = pv.asm.as_ref() else { return };
            let pid = st.inst(jv::s(&e, "inst")).map(|i| js::str_of(i.get("part"))).unwrap_or_default();
            let pt = st.part_of(&pid).and_then(|p| p.get("part")).cloned().unwrap_or(Value::Null);
            let axes = A::axes_of(st.builds.get(&pid).map(|b| &b.build), &pt);
            let mut opts = vec![(String::new(), tr("app.joint.face_origin"))];
            opts.extend(axes.iter().map(|a| (a.key.clone(), a.label.clone())));
            let cur = e.get("axis").filter(|a| !a.is_null()).map_or_else(String::new, |a| match jv::as_num(a.get("index")) {
                Some(i) => format!("{}:{}", jv::s(a, "feature"), js::num_str(i)),
                None => jv::s(a, "feature").to_string(),
            });
            ui.label(tr("app.joint.centre_on"));
            choice(ui, &format!("pc-asm-centre-{side}"), &cur, &opts, &format!("centre:{side}"), act);
            ui.end_row();
        }
    });
    // asm-ui.js jointSections: the limits
    if !dof.is_empty() {
        crate::shell::note(ui, t, &tr("asm.limits.title"));
        for k in &dof {
            let lim = A::limit_of(&j, Some(k));
            let mut on = lim.is_some();
            if ui.checkbox(&mut on, trv("asm.limits.hold", &[("value", label(k))])).changed() {
                *act = cmd("part.asm.set", format!("limit:{k}={}", if on { "on" } else { "off" }));
            }
            if let Some((lo, hi)) = lim {
                let unit = if matches!(*k, "angle" | "tiltX" | "tiltY" | "slotDir") { "°" } else { "mm" };
                egui::Grid::new(format!("pc-asm-lim-{k}")).num_columns(2).show(ui, |ui| {
                    ui.label(trv("asm.limits.min", &[("value", label(k)), ("unit", unit.into())]));
                    num_field(ui, pv, &format!("{jid}:min:{k}"), lo, &format!("limit-min:{k}"), act);
                    ui.end_row();
                    ui.label(trv("asm.limits.max", &[("value", label(k)), ("unit", unit.into())]));
                    num_field(ui, pv, &format!("{jid}:max:{k}"), hi, &format!("limit-max:{k}"), act);
                    ui.end_row();
                });
            }
        }
    }
    crate::shell::note(ui, t, &tr(&format!("asm.joint.note.{ty}")));
    if dof.len() > 1 {
        let sweep = pv
            .asm
            .as_ref()
            .and_then(|s| s.sweep.clone())
            .filter(|s| dof.contains(&s.as_str()))
            .unwrap_or_else(|| dof.first().copied().unwrap_or("").to_string());
        ui.horizontal(|ui| {
            ui.label(tr("asm.joint.sweep"));
            let opts: Vec<(String, String)> = dof.iter().map(|k| ((*k).to_string(), label(k))).collect();
            choice(ui, "pc-asm-sweep", &sweep, &opts, "sweep", act);
        });
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!dof.is_empty(), crate::shell::Btn::new(tr("asm.joint.check"))).clicked() {
            *act = cmd("part.asm", "motion");
        }
        if ui.add(crate::shell::Btn::new(tr("app.undo.remove_joint"))).clicked() {
            *act = cmd("part.asm", "delete");
        }
    });
}

fn inst_props(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd, iid: &str) {
    let Some(st) = pv.asm.as_ref() else { return };
    let Some(i) = st.inst(iid).cloned() else { return };
    let pid = js::str_of(i.get("part"));
    let n = jv::arr(st.asm.get("instances")).iter().position(|x| jv::is_str(x.get("id"), iid)).unwrap_or(0);
    ui.label(RichText::new(jv::s(&i, "name")).strong().family(crate::theme::bold_family()).color(t.text_strong));
    let copies = jv::arr(st.asm.get("instances")).iter().filter(|x| jv::is_str(x.get("part"), &pid)).count();
    let vol = st.stats_of(&pid).map(|s| format!("{} mm³", f3(jv::n(s, "volume")))).unwrap_or_default();
    let nj = jv::arr(st.asm.get("joints"))
        .iter()
        .filter(|j| jv::is_str(j.get("a").and_then(|e| e.get("inst")), iid) || jv::is_str(j.get("b").and_then(|e| e.get("inst")), iid))
        .count();
    let pos = if n == 0 {
        tr("app.inst.grounded")
    } else if st.problems.iter().any(|p| jv::is_str(p.get("inst"), iid)) {
        tr("app.inst.not_joined")
    } else {
        tr("app.inst.by_joints")
    };
    let pname = st.part_of(&pid).map(|p| jv::s(p, "name").to_string()).unwrap_or_default();
    egui::Grid::new("pc-asm-inst").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        for (k, v) in [
            (tr("app.props.part"), pname),
            (tr("app.inst.copies"), copies.to_string()),
            (tr("app.facts.volume"), vol),
            (tr("app.asm.tree_joints"), nj.to_string()),
            (tr("app.inst.position"), pos),
        ] {
            ui.label(RichText::new(k).color(t.text_dim));
            ui.label(RichText::new(v).color(t.text));
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr("app.props.name"));
        text_field(ui, pv, &format!("{iid}:name"), jv::s(&i, "name"), "name", act);
    });
    crate::partasmui::inst_tools(ui, pv, t, act, iid);
    ui.horizontal_wrapped(|ui| {
        if ui.add(crate::shell::Btn::primary(tr("app.inst.edit"))).clicked() {
            *act = cmd("part.asm", "edit");
        }
        if ui.add(crate::shell::Btn::new(tr("app.inst.insert_copy"))).clicked() {
            *act = cmd("part.asm", "insert-copy");
        }
        if n > 0 && ui.add(crate::shell::Btn::new(tr("app.inst.make_grounded"))).clicked() {
            *act = cmd("part.asm", "ground");
        }
        if ui.add(crate::shell::Btn::new(tr("app.btn.remove"))).clicked() {
            *act = cmd("part.asm", "delete");
        }
    });
}

/// asm-ui.js `instSections(inst)`: the part's properties (shared by its copies) and moving it by hand.
pub fn inst_tools(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd, iid: &str) {
    let Some(st) = pv.asm.as_ref() else { return };
    let Some(inst) = st.inst(iid).cloned() else { return };
    let pid = js::str_of(inst.get("part"));
    let p = st.part_of(&pid).cloned().unwrap_or(Value::Null);
    let free = A::free_ids(&st.asm).iter().any(|x| x == iid);
    let grounded = jv::arr(st.asm.get("instances")).first().is_some_and(|f| jv::is_str(f.get("id"), iid));
    let vol = st.stats_of(&pid).map(|s| jv::n(s, "volume"));
    ui.separator();
    ui.label(RichText::new(trv("ui.asm.part.props", &[("name", jv::s(&p, "name").to_string())])).small().color(t.text_dim));
    let known = BOM::preset_of(p.get("material"));
    let mats = BOM::materials();
    egui::Grid::new("pc-asm-partprops").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(tr("ui.asm.part.number"));
        text_field(ui, pv, &format!("{pid}:number"), jv::s(&p, "number"), "part:number", act);
        ui.end_row();
        ui.label(tr("ui.asm.part.description"));
        text_field(ui, pv, &format!("{pid}:desc"), jv::s(&p, "description"), "part:description", act);
        ui.end_row();
        ui.label(tr("part.material"));
        let mut opts = vec![(String::new(), tr("part.material.none"))];
        opts.extend(
            mats.iter()
                .enumerate()
                .map(|(i, (n, d))| (i.to_string(), trv("ui.asm.material.option", &[("name", n.clone()), ("density", js::num_str(*d))]))),
        );
        opts.push(("custom".into(), tr("ui.asm.material.custom")));
        let cur = if p.get("material").is_some_and(|m| !m.is_null()) {
            if known >= 0 { known.to_string() } else { "custom".into() }
        } else {
            String::new()
        };
        choice(ui, "pc-asm-material", &cur, &opts, "part:material", act);
        ui.end_row();
        // asm-ui.js shows the density field when the stored material is the first preset (`!known`)
        if let Some(m) = p.get("material").filter(|m| !m.is_null() && known == 0) {
            ui.label(tr("ui.asm.part.density"));
            num_field(ui, pv, &format!("{pid}:density"), jv::n(m, "density"), "part:density", act);
            ui.end_row();
        }
        if let (Some(m), Some(v)) = (p.get("material").filter(|m| !m.is_null()), vol) {
            ui.label(tr("ui.asm.part.mass"));
            ui.label(trv("ui.asm.mass_facets", &[("mass", f3(BOM::mass_of(v, jv::n(m, "density"))))]));
            ui.end_row();
        }
    });
    if !free {
        crate::shell::note(ui, t, &tr(if grounded { "ui.asm.move.grounded" } else { "ui.asm.move.joined" }));
        return;
    }
    crate::shell::note(ui, t, &tr("ui.asm.move.title"));
    let Some(st) = pv.asm.as_mut() else { return };
    let mv = st.edits.entry(format!("{iid}:move")).or_insert_with(|| "0,0,0".into()).clone();
    let mut d: Vec<String> = mv.split(',').map(str::to_string).collect();
    d.resize(3, "0".into());
    let mut changed = false;
    egui::Grid::new("pc-asm-move").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (k, ax) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(trv("ui.asm.move.delta", &[("axis", (*ax).to_string())]));
            if let Some(x) = d.get_mut(k) {
                changed |= ui.add_sized([120.0, 26.0], crate::shell::field(x).id_salt(("pc-move", k))).changed();
            }
            ui.end_row();
        }
    });
    if changed && let Some(st) = pv.asm.as_mut() {
        st.edits.insert(format!("{iid}:move"), d.join(","));
    }
    if ui.add(crate::shell::Btn::new(tr("ui.asm.move.go"))).clicked() {
        *act = cmd("part.asm.set", format!("move={}", d.join(",")));
    }
    let Some(st) = pv.asm.as_mut() else { return };
    let turn = st.edits.entry(format!("{iid}:turn")).or_insert_with(|| "y,90".into()).clone();
    let (mut ax, mut ang) = turn.split_once(',').map_or(("y".to_string(), "90".to_string()), |(a, b)| (a.to_string(), b.to_string()));
    let mut changed = false;
    egui::Grid::new("pc-asm-turn").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(tr("ui.asm.turn.about"));
        egui::ComboBox::new("pc-asm-turn-axis", "").selected_text(ax.to_uppercase()).width(80.0).show_ui(ui, |ui| {
            for a in ["x", "y", "z"] {
                if ui.selectable_label(ax == a, a.to_uppercase()).clicked() {
                    ax = a.to_string();
                    changed = true;
                }
            }
        });
        ui.end_row();
        ui.label(tr("ui.asm.turn.angle"));
        changed |= ui.add_sized([120.0, 26.0], crate::shell::field(&mut ang).id_salt("pc-turn-angle")).changed();
        ui.end_row();
    });
    if changed && let Some(st) = pv.asm.as_mut() {
        st.edits.insert(format!("{iid}:turn"), format!("{ax},{ang}"));
    }
    if ui.add(crate::shell::Btn::new(tr("ui.asm.turn.go"))).clicked() {
        *act = cmd("part.asm.set", format!("turn={ax},{}", ang.trim()));
    }
    // Drag to move / Drag to turn (the gizmo; filled while it is up on this copy)
    let g = pv.gizmo().filter(|g| g.purpose == "move" && g.insts.first().is_some_and(|x| x.as_str() == iid)).map(|g| g.tc.mode);
    ui.horizontal_wrapped(|ui| {
        for (mode, value, stop, start) in [
            (crate::gizmo3::Mode::Translate, "drag=translate", "ui.asm.drag.stop_move", "ui.asm.drag.move"),
            (crate::gizmo3::Mode::Rotate, "drag=rotate", "ui.asm.drag.stop_turn", "ui.asm.drag.turn"),
        ] {
            let clicked = if g == Some(mode) {
                ui.add(crate::shell::Btn::primary(tr(stop))).clicked()
            } else {
                ui.add(crate::shell::Btn::new(tr(start))).clicked()
            };
            if clicked {
                *act = cmd("part.asm.set", value);
            }
        }
    });
}

/// asm-ui.js `rootSections()`: the BOM (+ CSV) and the exploded views.
pub fn root_tools(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let rows = pv.asm_bom_rows();
    ui.separator();
    crate::shell::note(ui, t, &tr("ui.asm.bom.title"));
    egui::Grid::new("pc-asm-bom").num_columns(6).striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
        for h in ["ui.asm.bom.item", "ui.asm.bom.qty", "ui.asm.part.number", "ui.asm.bom.name", "part.material", "ui.asm.bom.mass_each"] {
            ui.label(RichText::new(tr(h)).small().strong().family(crate::theme::bold_family()));
        }
        ui.end_row();
        for r in &rows {
            for k in ["item", "qty", "number", "name", "material"] {
                ui.label(RichText::new(js::string_of(r.get(k))).small());
            }
            ui.label(
                RichText::new(jv::as_num(r.get("massEach")).map_or_else(String::new, |m| trv("ui.asm.mass", &[("mass", f3(m))]))).small(),
            );
            ui.end_row();
        }
    });
    let words = pv.asm_bom_words();
    if let Some(tail) = words.rsplit(" | ").next() {
        ui.label(RichText::new(tail).small().color(t.text_dim));
    }
    ui.horizontal_wrapped(|ui| {
        if ui.add(crate::shell::Btn::new(tr("ui.asm.bom.csv"))).clicked() {
            *act = cmd("part.asm", "export:bom");
        }
        if ui.add(crate::shell::Btn::new(tr("ui.asm.bom.drawing"))).clicked() {
            *act = cmd("part.asm", "export:drawing");
        }
    });
    // exploded views
    ui.separator();
    crate::shell::note(ui, t, &tr("ui.asm.explode.title"));
    let Some(st) = pv.asm.as_ref() else { return };
    let views: Vec<(String, String)> =
        jv::arr(st.asm.get("views")).iter().map(|v| (jv::s(v, "id").to_string(), jv::s(v, "name").to_string())).collect();
    let cur = st.tools.view_id.clone().unwrap_or_default();
    let has_joints = !jv::arr(st.asm.get("joints")).is_empty();
    ui.horizontal(|ui| {
        ui.label(tr("ui.asm.explode.view"));
        let mut opts = vec![(String::new(), tr("ui.asm.explode.none"))];
        opts.extend(views.iter().cloned());
        let shown = opts.iter().find(|(v, _)| *v == cur).map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::new("pc-asm-views", "").selected_text(shown).width(170.0).show_ui(ui, |ui| {
            for (v, l) in &opts {
                if ui.selectable_label(*v == cur, l).clicked() && *v != cur {
                    *act = cmd("part.asm", format!("explode-view:{v}"));
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add(crate::shell::Btn::new(tr("ui.asm.explode.new"))).clicked() {
            *act = cmd("part.asm", "explode-new");
        }
        if ui.add(crate::shell::Btn::new(tr("ui.asm.explode.auto_radial"))).clicked() {
            *act = cmd("part.asm", "explode-auto:radial");
        }
        if ui.add_enabled(has_joints, crate::shell::Btn::new(tr("ui.asm.explode.auto_joints"))).clicked() {
            *act = cmd("part.asm", "explode-auto:joints");
        }
    });
    let Some(v) = pv.asm_active_view() else {
        crate::shell::note(ui, t, &tr("ui.asm.explode.note"));
        return;
    };
    let vid = jv::s(&v, "id").to_string();
    ui.horizontal(|ui| {
        ui.label(tr("ui.asm.bom.name"));
        let Some(st) = pv.asm.as_mut() else { return };
        let key = format!("{vid}:name");
        let shown = jv::s(&v, "name").to_string();
        let mut text = st.edits.get(&key).cloned().unwrap_or_else(|| shown.clone());
        let r = ui.add_sized([170.0, 26.0], crate::shell::field(&mut text).id_salt(("pc-asm", key.as_str())));
        if r.has_focus() {
            st.edits.insert(key.clone(), text.clone());
        }
        if r.lost_focus() {
            st.edits.remove(&key);
            if text != shown {
                *act = cmd("part.asm", format!("explode-name:{text}"));
            }
        }
    });
    let mut tt = pv.asm.as_ref().map_or(1.0, |s| s.tools.t);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("ui.asm.explode.assembled")).small());
        ui.spacing_mut().slider_width = 140.0;
        if ui
            .add(egui::Slider::new(&mut tt, 0.0..=1.0).step_by(0.01).show_value(false))
            .on_hover_text(tr("ui.asm.explode.slider"))
            .changed()
        {
            *act = cmd("part.asm", format!("explode-t:{tt}"));
        }
        ui.label(RichText::new(tr("ui.asm.explode.exploded")).small());
    });
    let Some(st) = pv.asm.as_ref() else { return };
    let steps = jv::arr(v.get("steps")).to_vec();
    for (i, s) in steps.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(step_words(st, i, s)).small());
            if ui.add(crate::shell::Btn::new(tr("ui.asm.explode.remove"))).clicked() {
                *act = cmd("part.asm", format!("step-remove:{}", jv::s(s, "id")));
            }
        });
    }
    if steps.is_empty() {
        crate::shell::note(ui, t, &tr("ui.asm.explode.no_steps"));
    }
    // a new step
    crate::shell::note(ui, t, &tr("ui.asm.step.title"));
    let draft = st.tools.draft.clone().unwrap_or(StepDraft { insts: Vec::new(), dir: "y".into(), dist: 50.0, face_dir: None });
    for i in jv::arr(st.asm.get("instances")) {
        let id = jv::s(i, "id").to_string();
        let mut on = draft.insts.contains(&id);
        if ui.checkbox(&mut on, jv::s(i, "name")).changed() {
            *act = cmd("part.asm", format!("step-inst:{id}={}", if on { "on" } else { "off" }));
        }
    }
    ui.horizontal(|ui| {
        ui.label(tr("ui.asm.step.direction"));
        let opts = dir_labels();
        let shown = opts.iter().find(|(k, _)| *k == draft.dir).map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::new("pc-asm-stepdir", "").selected_text(shown).width(120.0).show_ui(ui, |ui| {
            for (k, l) in &opts {
                if ui.selectable_label(*k == draft.dir, l).clicked() {
                    *act = cmd("part.asm", format!("step-dir:{k}"));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        if draft.dir == "drag" {
            return;
        }
        ui.label(tr("ui.asm.step.distance"));
        let Some(st) = pv.asm.as_mut() else { return };
        let key = format!("{vid}:dist");
        let shown = V::fmt(draft.dist, 6);
        let mut text = st.edits.get(&key).cloned().unwrap_or_else(|| shown.clone());
        let r = ui.add_sized([100.0, 26.0], crate::shell::field(&mut text).id_salt(("pc-asm", key.as_str())));
        if r.has_focus() {
            st.edits.insert(key.clone(), text.clone());
        }
        if r.lost_focus() {
            st.edits.remove(&key);
            if text.trim() != shown {
                *act = cmd("part.asm", format!("step-dist:{}", text.trim()));
            }
        }
    });
    if draft.dir == "face" {
        let words = match draft.face_dir {
            Some(d) => trv("ui.asm.step.face_normal", &[("dir", d.iter().map(|c| f3(*c)).collect::<Vec<_>>().join(", "))]),
            None => tr("ui.asm.step.click_face"),
        };
        ui.label(RichText::new(words).small().color(t.text_dim));
    }
    ui.horizontal_wrapped(|ui| {
        if ui.add(crate::shell::Btn::primary(tr(if draft.dir == "drag" { "ui.asm.step.drag_add" } else { "ui.asm.step.add" }))).clicked() {
            *act = cmd("part.asm", "step-add");
        }
        if ui.add(crate::shell::Btn::new(tr("ui.asm.explode.delete"))).clicked() {
            *act = cmd("part.asm", "explode-delete");
        }
    });
}

/// The interference results and their filters (`clashBlock`).
fn clash_block(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    let list = st.clashes.clone().unwrap_or_default();
    let (shown, hidden) = A::filter_clashes(&st.asm, &list);
    let filter = st.asm.get("clashFilter").cloned().unwrap_or(Value::Null);
    let hide: Vec<String> = jv::arr(filter.get("hide")).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
    let pairs = jv::arr(filter.get("pairs")).len();
    ui.separator();
    crate::shell::note(ui, t, &tr("asm.clash.title"));
    for c in &shown {
        let bad = matches!(jv::s(c, "kind"), "overlap" | "thread-depth");
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(pv.clash_words(c)).color(if bad { t.text_strong } else { t.text }));
            if ui.add(crate::shell::Btn::new(tr("asm.clash.hide_pair"))).clicked() {
                *act = cmd("part.asm", format!("clash-hide:{}|{}", jv::s(c, "a"), jv::s(c, "b")));
            }
        });
    }
    ui.label(RichText::new(tr("asm.clash.show")).small());
    for k in A::CLASH_KINDS {
        let mut on = !hide.iter().any(|h| h == k);
        if ui.checkbox(&mut on, tr(&format!("asm.clash.kind.{k}"))).changed() {
            *act = cmd("part.asm", format!("clash-kind:{k}={}", if on { "on" } else { "off" }));
        }
    }
    if !hidden.is_empty() || pairs > 0 {
        ui.horizontal_wrapped(|ui| {
            if !hidden.is_empty() {
                ui.label(
                    RichText::new(cc_part::i18n::t("asm.clash.hidden", &[("n", cc_part::i18n::F::from(hidden.len()))]))
                        .small()
                        .color(t.text_dim),
                );
            }
            if pairs > 0
                && ui
                    .add(crate::shell::Btn::new(cc_part::i18n::t("asm.clash.show_pairs", &[("n", cc_part::i18n::F::from(pairs))])))
                    .clicked()
            {
                *act = cmd("part.asm", "clash-show-pairs");
            }
        });
    }
}

/// The assembly's questions (`ask`): remove a part that joints hold; close with changes not saved.
pub fn ask_dialog(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    let Some(q) = st.ask else { return };
    let (title, text, buttons): (String, String, Vec<(&str, String)>) = if q == "remove" {
        let id = st.sel.clone().map(|(_, id)| id).unwrap_or_default();
        let nj = jv::arr(st.asm.get("joints"))
            .iter()
            .filter(|j| jv::is_str(j.get("a").and_then(|e| e.get("inst")), &id) || jv::is_str(j.get("b").and_then(|e| e.get("inst")), &id))
            .count();
        (
            trv("app.asm.remove_title", &[("name", st.inst_name(&id))]),
            cc_part::i18n::t("app.asm.remove_joints", &[("n", cc_part::i18n::F::from(nj))]),
            vec![("cancel", tr("app.btn.cancel")), ("remove", tr("app.btn.remove"))],
        )
    } else {
        (
            tr("app.asm.unsaved_title"),
            trv("app.asm.unsaved_text", &[("name", jv::s(&st.asm, "name").to_string())]),
            vec![("cancel", tr("app.btn.cancel")), ("discard", tr("app.asm.discard")), ("save", tr("app.btn.save_first"))],
        )
    };
    crate::shell::Dialog::new(title).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        ui.label(RichText::new(text).color(t.text));
        ui.horizontal(|ui| {
            let last = buttons.len().saturating_sub(1);
            for (i, (k, words)) in buttons.iter().enumerate() {
                let clicked = if i == last {
                    ui.add(crate::shell::Btn::primary(words.clone())).clicked()
                } else {
                    ui.add(crate::shell::Btn::new(words.clone())).clicked()
                };
                if clicked {
                    *act = cmd("part.asm", format!("answer:{k}"));
                }
            }
        });
    });
}

/// The view in the assembly: hover names a part copy, a click picks one (or a joint's face), the keys.
pub fn stage_input(ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, pv: &mut PartView, act: &mut Cmd) {
    // the gizmo first: a press on a handle drags it (the view's click still picks, as in the 0.5 page)
    crate::partgizmo::stage_pointer(ui, resp, rect, pv);
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let px = resp.hover_pos().map(|p| (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp));
    if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
        let h = px.and_then(|(x, y)| pv.asm_hit(x, y)).map(|(i, _, _)| i);
        if let Some(st) = pv.asm.as_mut()
            && st.hover != h
        {
            st.hover = h;
            pv.asm_retint();
        }
    }
    if resp.clicked_by(egui::PointerButton::Primary)
        && let Some((x, y)) = px
    {
        let hit = pv.asm_hit(x, y);
        *act = cmd(
            "part.asm",
            match hit {
                Some((iid, f, _)) => {
                    let face = f
                        .and_then(|f| {
                            pv.asm
                                .as_ref()
                                .and_then(|s| s.inst(&iid).map(|i| js::str_of(i.get("part"))))
                                .and_then(|pid| pv.asm.as_ref()?.builds.get(&pid)?.regen.faces.as_ref()?.get(f).cloned())
                        })
                        .map(|f| jv::s(&f, "name").to_string())
                        .unwrap_or_default();
                    format!("pick:{iid}:{face}")
                }
                None => "pick:".into(),
            },
        );
    }
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    if !typing {
        let key = ui.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                Some("Escape")
            } else if i.key_pressed(egui::Key::Delete) {
                Some("Delete")
            } else if i.key_pressed(egui::Key::J) && !i.modifiers.any() {
                Some("j")
            } else if i.key_pressed(egui::Key::F) && !i.modifiers.any() {
                Some("f")
            } else {
                None
            }
        });
        if let Some(k) = key {
            *act = cmd("part.asm", format!("key:{k}"));
        }
    }
}
