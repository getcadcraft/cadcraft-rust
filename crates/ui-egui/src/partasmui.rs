//! PartCraft's assembly panels in the app (M13.11): partcraft.js `renderAsmTree` (the part copies and the joints),
//! `renderAsmFacts`, `renderAsmProps` (the assembly, a joint with its values, centres, limits and motion check, a
//! part copy), the interference block with its filters, the Assembly group of the toolbar, the questions (remove a
//! joined part, close with changes), and the view's picking.

use cc_part::{asm as A, js, jv, vec as V};
use egui::RichText;
use serde_json::Value;

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

impl PartView {
    /// What is drawn: the placement, or the exploded view (M13.12).
    pub fn asm_display_world(&self) -> Vec<(String, V::M4)> {
        let Some(st) = self.asm.as_ref() else { return Vec::new() };
        let view = st.tools.view_id.as_deref().and_then(|id| jv::arr(st.asm.get("views")).iter().find(|v| jv::is_str(v.get("id"), id)));
        match view.filter(|v| !jv::arr(v.get("steps")).is_empty()) {
            Some(v) => cc_part::explode::exploded(&st.world, &cc_part::explode::offsets(Some(v), st.tools.t)),
            None => st.world.clone(),
        }
    }

    /// Esc with an assembly tool waiting: whether it took the key.
    pub fn asm_tools_escape(&mut self) -> bool {
        false
    }

    /// The assembly tools' fields (asm-ui.js; M13.12).
    pub fn asm_tool_set(&mut self, _inst: Option<&str>, field: &str, _value: &str) -> Result<(), String> {
        Err(format!("part.asm.set: no field {field}"))
    }

    /// The assembly tools' commands (asm-ui.js; M13.12).
    pub fn asm_tool_cmd(&mut self, k: &str, _v: &str) -> Result<Option<(String, Vec<u8>, String)>, String> {
        Err(format!("part.asm: unknown {k}"))
    }
}

/// The left panel in the assembly: renderAsmTree + renderAsmFacts.
pub fn tree(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    ui.label(RichText::new(jv::s(&st.asm, "name")).strong().color(t.text_strong));
    ui.separator();
    ui.label(RichText::new(tr("app.asm.tree_parts")).small().color(t.text_dim));
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
    ui.label(RichText::new(tr("app.asm.tree_joints")).small().color(t.text_dim));
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
    ui.label(RichText::new(cc_part::i18n::t("ui.left.asm_note", &[("key", cc_part::i18n::F::S("J".into()))])).small().color(t.text_dim));
    ui.separator();
    ui.label(RichText::new(tr("app.facts.h_assembly")).strong().color(t.text_strong));
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
        ui.label(RichText::new(tr("app.joint.new")).strong().color(t.text_strong));
        msg(ui, &tr(if jp.is_some() { "app.joint.pick_second_face" } else { "app.joint.pick_first_face" }), false);
        ui.label(RichText::new(tr("app.joint.new_note")).small().color(t.text_dim));
        return;
    }
    let sel = st.sel.clone();
    match sel {
        Some((k, id)) if k == "joint" && st.joint(&id).is_some() => joint_props(ui, pv, t, act, &id),
        Some((k, id)) if k == "inst" && st.inst(&id).is_some() => inst_props(ui, pv, t, act, &id),
        _ => {
            let name = jv::s(&st.asm, "name").to_string();
            ui.label(RichText::new(tr("app.facts.h_assembly")).strong().color(t.text_strong));
            ui.horizontal(|ui| {
                ui.label(tr("app.props.name"));
                text_field(ui, pv, "asm-name", &name, "name", act);
            });
            ui.label(RichText::new(tr("app.asm.note")).small().color(t.text_dim));
            let Some(st) = pv.asm.as_ref() else { return };
            for p in &st.problems {
                msg(ui, jv::s(p, "message"), !jv::truthy(p.get("free")));
            }
            if st.clashes.is_some() || st.asm.get("clashFilter").is_some_and(|c| !c.is_null()) {
                clash_block(ui, pv, t, act);
            }
        }
    }
}

fn joint_props(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd, jid: &str) {
    let Some(st) = pv.asm.as_ref() else { return };
    let Some(j) = st.joint(jid).cloned() else { return };
    let ty = jv::s(&j, "type").to_string();
    ui.label(RichText::new(jv::s(&j, "name")).strong().color(t.text_strong));
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
        ui.label(RichText::new(tr("asm.limits.title")).small().color(t.text_dim));
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
    ui.label(RichText::new(tr(&format!("asm.joint.note.{ty}"))).small().color(t.text_dim));
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
    ui.label(RichText::new(jv::s(&i, "name")).strong().color(t.text_strong));
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

/// asm-ui.js's sections of a part copy (M13.12).
pub fn inst_tools(_ui: &mut egui::Ui, _pv: &mut PartView, _t: &Tokens, _act: &mut Cmd, _iid: &str) {}

/// The interference results and their filters (`clashBlock`).
fn clash_block(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some(st) = pv.asm.as_ref() else { return };
    let list = st.clashes.clone().unwrap_or_default();
    let (shown, hidden) = A::filter_clashes(&st.asm, &list);
    let filter = st.asm.get("clashFilter").cloned().unwrap_or(Value::Null);
    let hide: Vec<String> = jv::arr(filter.get("hide")).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
    let pairs = jv::arr(filter.get("pairs")).len();
    ui.separator();
    ui.label(RichText::new(tr("asm.clash.title")).small().color(t.text_dim));
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
