//! DetailCraft's panels in the app, labelled from DetailCraft's catalog: the bar (file, edit, exports, drawings,
//! views), the left panel (the model, its phases, part marks, members by type), the right panel (Properties of the
//! selected member or the model summary: detailcraft.js `memberProps` / `modelSummary`) and the reports at the bottom
//! (problems, bill of materials, clashes, weight report by phase, bending schedule, cut list). Panels only read the
//! view and hand back [`Act`]s; every change goes through the `detail.*` commands.

use cc_detail::i18n::{F, t, t0};
use egui::{RichText, Ui};
use serde_json::Value;

use crate::detail3d::Ref;
use crate::detailview::{DetailView, Tab, type_name};
use crate::theme::Tokens;

/// What a panel asks for.
pub enum Act {
    Select(Option<Ref>),
    Fit,
    CloseReport,
    /// a `detail.*` command with its name parameter
    Run(&'static str, String),
}

fn r1(v: f64) -> String {
    cc_detail::js::num_to_string((v * 10.0).round() / 10.0)
}
fn mm(v: f64) -> String {
    cc_detail::js::num_to_string(v.round())
}

/// The bar above the view.
pub fn bar(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            let name = dv.model.state.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let dirty = if dv.model.dirty() { "• " } else { "" };
            ui.label(RichText::new(format!("{dirty}{name}")).strong().color(tk.text_strong));
            ui.separator();
            if ui.button(t0("page.tool.save")).on_hover_text(t("page.tool.save.title", &[("key", F::S("Ctrl+S".into()))])).clicked() {
                acts.push(Act::Run("detail.save", String::new()));
            }
            let last = |s: &[cc_detail::model::Step]| s.last().map(|x| x.label.clone()).unwrap_or_default();
            let undo_tip = if dv.model.can_undo() {
                t("app.bar.undo.label", &[("label", F::S(last(&dv.model.undo_stack)))])
            } else {
                t0("app.bar.undo")
            };
            if ui.add_enabled(dv.model.can_undo(), egui::Button::new(t0("page.tool.undo"))).on_hover_text(undo_tip).clicked() {
                acts.push(Act::Run("detail.undo", String::new()));
            }
            let redo_tip = if dv.model.can_redo() {
                t("app.bar.redo.label", &[("label", F::S(last(&dv.model.redo_stack)))])
            } else {
                t0("app.bar.redo")
            };
            if ui.add_enabled(dv.model.can_redo(), egui::Button::new(t0("page.tool.redo"))).on_hover_text(redo_tip).clicked() {
                acts.push(Act::Run("detail.redo", String::new()));
            }
            ui.separator();
            ui.menu_button(t0("input.m.export"), |ui| {
                for (kind, label, tip) in [
                    ("ifc", "page.tool.ifc", "page.tool.ifc.title"),
                    ("nc", "page.tool.nc", "page.tool.nc.title"),
                    ("bom", "page.tool.bom", "page.tool.bom.title"),
                    ("bending", "page.tool.bending", "page.tool.bending.title"),
                    ("kiss", "tool.kiss", "tool.kiss.title"),
                    ("cutlist", "nest.tab", "nest.file"),
                    ("report-csv", "tool.report", "rep.csv"),
                    ("report-pdf", "tool.report", "rep.pdf"),
                ] {
                    let text = if kind.starts_with("report") { format!("{} ({})", t0(label), t0(tip)) } else { t0(label) };
                    if ui.button(text).on_hover_text(t0(tip)).clicked() {
                        acts.push(Act::Run("detail.export", kind.into()));
                        ui.close();
                    }
                }
            });
            ui.menu_button(t0("page.tool.drawings"), |ui| {
                ui.label(RichText::new(t0("dwg.format.label")).color(tk.text_dim));
                for f in ["pdf", "dwg-AC1032", "dwg-AC1027", "dwg-AC1024", "dwg-AC1021", "dwg-AC1018", "dwg-AC1015", "dxf"] {
                    let label = crate::detailexport::format_name(f).unwrap_or_default();
                    ui.radio_value(&mut dv.dwg_format, f.to_string(), label);
                }
                ui.separator();
                let pl = ui.label(RichText::new(t0("dwg.paper.label")).color(tk.text_dim));
                let mut paper = dv.settings.paper.clone();
                egui::ComboBox::new("detail_paper", "")
                    .selected_text(cc_detail::paper::paper_label(&paper))
                    .show_ui(ui, |ui| {
                        for k in cc_detail::paper::PAPER_KEYS {
                            ui.selectable_value(&mut paper, k.to_string(), cc_detail::paper::paper_label(k));
                        }
                    })
                    .response
                    .labelled_by(pl.id);
                if paper != dv.settings.paper {
                    acts.push(Act::Run("detail.setting", format!("paper={paper}")));
                }
                ui.separator();
                let fname = crate::detailexport::format_name(&dv.dwg_format).unwrap_or_default();
                if ui.button(t("dwg.all", &[("format", F::S(fname))])).on_hover_text(t0("page.tool.drawings.title")).clicked() {
                    acts.push(Act::Run("detail.export", format!("drawings:{}", dv.dwg_format)));
                    ui.close();
                }
            })
            .response
            .on_hover_text(t0("page.tool.drawings.title"));
            let part = dv.selected_part();
            let why = cc_detail::shop::shop_refusal(part);
            if ui
                .add_enabled(why.is_none(), egui::Button::new(t0("page.tool.shop")))
                .on_hover_text(why.unwrap_or_else(|| t0("page.tool.shop.title")))
                .clicked()
            {
                acts.push(Act::Run("detail.export", "shop".into()));
            }
            ui.separator();
            if ui.button(t0("page.view.fit")).on_hover_text(t0("page.view.fit.title")).clicked() {
                acts.push(Act::Fit);
            }
            // plain English: the 0.5 app turns the view with its view cube, which has no words
            for (v, label) in [("iso", "Iso"), ("top", "Top"), ("front", "Front"), ("right", "Right")] {
                if ui.small_button(label).clicked() {
                    acts.push(Act::Run("detail.camera", v.into()));
                }
            }
            let mut bolts = dv.settings.bolts;
            if ui.checkbox(&mut bolts, t0("app.k.bolts")).changed() {
                acts.push(Act::Run("detail.setting", format!("bolts={bolts}")));
            }
        });
    });
}

/// The left panel: the model, phases, part marks and the members by type.
pub fn left(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let st = &dv.model.state;
    let head = ui.label(RichText::new(t0("page.left.model")).strong().color(tk.text_strong));
    let name = st.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let key = "model.name".to_string();
    if let Some(v) = text_field(dv, ui, &key, &name, 200.0, head.id) {
        acts.push(Act::Run("detail.set", format!("model.name={v}")));
    }
    let st = &dv.model.state;
    ui.label(RichText::new(t0("page.left.model.note")).small().color(tk.text_dim));
    ui.add_space(6.0);
    // phases: show / hide
    let phases: Vec<(f64, String)> = cc_detail::js::arr(st.get("phases"))
        .iter()
        .filter_map(|p| {
            let n = p.get("n").and_then(Value::as_f64)?;
            let nm = p
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map_or_else(|| t("phase.default", &[("n", F::N(n))]), str::to_string);
            Some((n, nm))
        })
        .collect();
    if phases.len() > 1 {
        ui.label(RichText::new(t0("phase.title")).strong().color(tk.text_strong));
        for (n, nm) in &phases {
            let mut on = !dv.hidden_phases.contains(n);
            if ui.checkbox(&mut on, nm).on_hover_text(t("phase.show", &[("phase", F::S(nm.clone()))])).changed() {
                acts.push(Act::Run("detail.phase", cc_detail::js::num_to_string(*n)));
            }
        }
        ui.add_space(6.0);
    }
    ui.label(RichText::new(t0("page.left.marks.title")).strong().color(tk.text_strong));
    if dv.marks.parts.is_empty() {
        ui.label(RichText::new(t0("app.marks.none")).color(tk.text_dim));
    }
    let sel_key = dv.sel.as_ref().map(Ref::key);
    for p in &dv.marks.parts {
        let first = p.ids.first().and_then(|id| dv.built.get(id));
        let r = first.and_then(Ref::of_part);
        let on = r.as_ref().map(Ref::key) == sel_key && sel_key.is_some();
        let text = format!("{}  {} x {}  × {}", p.mark, p.profile, mm(p.length), p.count);
        if ui
            .selectable_label(on, RichText::new(text).monospace().size(11.0))
            .on_hover_text(t("app.marks.select-first", &[("mark", F::S(p.mark.clone()))]))
            .clicked()
        {
            acts.push(Act::Select(r));
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new(t0("app.k.members")).strong().color(tk.text_strong));
    let members = cc_detail::js::arr(dv.model.state.get("members"));
    for kind in cc_detail::model::MEMBER_TYPES {
        let list: Vec<&Value> = members.iter().filter(|m| m.get("type").and_then(Value::as_str) == Some(kind)).collect();
        if list.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(format!("{} ({})", type_name(kind), list.len()))
            .id_salt(("detail_kind", kind))
            .default_open(list.len() <= 40)
            .show(ui, |ui| {
                for m in list {
                    let id = cc_detail::js::string_of(m.get("id"));
                    let mark = dv.marks.part_mark(&id).unwrap_or("").to_string();
                    let profile = cc_detail::js::string_of(m.get("profile"));
                    let r = Ref::Member(id.clone());
                    let on = sel_key.as_deref() == Some(r.key().as_str());
                    if ui.selectable_label(on, RichText::new(format!("{id}  {mark}  {profile}")).size(11.0)).clicked() {
                        acts.push(Act::Select(Some(r)));
                    }
                }
            });
    }
    let footings = cc_detail::js::arr(dv.model.state.get("footings"));
    if !footings.is_empty() {
        egui::CollapsingHeader::new(format!("{} ({})", t0("app.k.footings"), footings.len())).id_salt("detail_footings").show(ui, |ui| {
            for f in footings {
                let id = cc_detail::js::string_of(f.get("id"));
                let mark = dv.footing_marks.iter().find(|(k, _)| *k == id).map_or("", |(_, v)| v.as_str()).to_string();
                let r = Ref::Footing(id.clone());
                let on = sel_key.as_deref() == Some(r.key().as_str());
                if ui.selectable_label(on, RichText::new(format!("{id}  {mark}")).size(11.0)).clicked() {
                    acts.push(Act::Select(Some(r)));
                }
            }
        });
    }
}

/// A one-line text field that hands back its text when Enter is pressed or it loses focus with a change.
fn text_field(dv: &mut DetailView, ui: &mut Ui, key: &str, current: &str, width: f32, label: egui::Id) -> Option<String> {
    let mut text = match &dv.editing {
        Some((k, v)) if k == key => v.clone(),
        _ => current.to_string(),
    };
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width).id_salt(("detail_field", key))).labelled_by(label);
    if r.has_focus() || r.changed() {
        dv.editing = Some((key.to_string(), text.clone()));
    }
    if r.lost_focus() {
        let done = dv.editing.take().filter(|(k, _)| k == key).map(|(_, v)| v);
        return done.filter(|v| v != current);
    }
    None
}

/// A labelled row: label on the left, the widget on the right.
fn row(ui: &mut Ui, tk: &Tokens, label: &str, add: impl FnOnce(&mut Ui, egui::Id)) {
    ui.horizontal(|ui| {
        let l = ui.add_sized([96.0, 18.0], egui::Label::new(RichText::new(label).color(tk.text_dim)).truncate());
        add(ui, l.id);
    });
}

/// The right panel: Properties.
pub fn right(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    match dv.sel.clone() {
        Some(Ref::Member(id)) => member_props(dv, ui, tk, acts, &id),
        Some(r) => {
            // detailcraft.js renderProps' headings
            let head = match &r {
                Ref::Conn(i) => {
                    let c = cc_detail::js::arr(dv.model.state.get("connections"))
                        .iter()
                        .find(|x| x.get("id").and_then(Value::as_str) == Some(i));
                    t0(&format!("app.props.{}", c.and_then(|c| c.get("type")).and_then(Value::as_str).unwrap_or("")))
                }
                Ref::Stair(i) => t("app.props.stair", &[("mark", F::S(dv.marks.asm_mark(&format!("{i}.stringerL")).unwrap_or("").into()))]),
                Ref::Footing(i) => {
                    let mark = dv.footing_marks.iter().find(|(k, _)| k == i).map_or("", |(_, v)| v.as_str());
                    t("app.props.footing", &[("mark", F::S(mark.into()))])
                }
                Ref::Member(_) => t0("page.right.props"),
            };
            ui.label(RichText::new(head).strong().color(tk.text_strong));
            ui.label(dv.describe(&r));
            let probs: Vec<&cc_detail::build::BProblem> = dv.built.problems.iter().filter(|x| x.r#ref.as_deref() == Some(r.id())).collect();
            for p in probs {
                ui.label(RichText::new(&p.text).color(if p.hard == Some(true) { tk.accent } else { tk.text_dim }));
            }
            if matches!(r, Ref::Conn(_)) {
                let why = cc_detail::shop::shop_refusal(dv.selected_part());
                if ui.add_enabled(why.is_none(), egui::Button::new(t0("app.btn.shop"))).clicked() {
                    acts.push(Act::Run("detail.export", "shop".into()));
                }
            }
        }
        None => model_summary(dv, ui, tk),
    }
}

fn member_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
    let Some(m) = dv.member(id).cloned() else { return };
    let kind = cc_detail::js::string_of(m.get("type"));
    let mark = dv.marks.part_mark(id).map(|x| format!(" {x}")).unwrap_or_default();
    ui.label(RichText::new(format!("{}{mark}", type_name(&kind))).strong().color(tk.text_strong));
    ui.add_space(4.0);
    let s = |k: &str| cc_detail::js::string_of(m.get(k));
    let n = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let set = |acts: &mut Vec<Act>, field: &str, v: String| acts.push(Act::Run("detail.set", format!("{id}.{field}={v}")));
    row(ui, tk, &t0("app.f.name"), |ui, lid| {
        if let Some(v) = text_field(dv, ui, &format!("{id}.name"), &s("name"), 160.0, lid) {
            set(acts, "name", v);
        }
    });
    let custom = cc_detail::js::arr(dv.model.state.get("profiles")).to_vec();
    if kind == "plate" {
        let pr = cc_detail::profiles::get_profile(m.get("profile").unwrap_or(&Value::Null), &custom);
        let (thick, w) = (pr.as_ref().and_then(|p| p.t).unwrap_or(20.0), pr.as_ref().and_then(|p| p.b).unwrap_or(200.0));
        for (field, label, v) in [("thickness", "app.f.thickness", thick), ("width", "app.f.width", w)] {
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(x) = text_field(dv, ui, &format!("{id}.{field}"), &cc_detail::js::num_to_string(v), 80.0, lid) {
                    set(acts, field, x);
                }
            });
        }
    } else {
        let cur = s("profile");
        row(ui, tk, &t0("app.f.profile"), |ui, lid| {
            let mut pick = cur.clone();
            egui::ComboBox::new(("detail_profile", id), "")
                .selected_text(&cur)
                .width(160.0)
                .height(360.0)
                .show_ui(ui, |ui| {
                    for g in cc_detail::profiles::catalog_groups(&custom) {
                        ui.label(RichText::new(format!("{} — {}", g.label, g.source)).small().color(tk.text_dim));
                        for nm in &g.names {
                            ui.selectable_value(&mut pick, nm.clone(), nm);
                        }
                    }
                })
                .response
                .labelled_by(lid);
            if pick != cur {
                set(acts, "profile", pick);
            }
        });
    }
    let cur = s("material");
    row(ui, tk, &t0("app.f.material"), |ui, lid| {
        let mut pick = cur.clone();
        egui::ComboBox::new(("detail_material", id), "")
            .selected_text(&cur)
            .width(160.0)
            .show_ui(ui, |ui| {
                for mat in cc_detail::model::MATERIALS {
                    ui.selectable_value(&mut pick, mat.to_string(), mat);
                }
            })
            .response
            .labelled_by(lid);
        if pick != cur {
            set(acts, "material", pick);
        }
    });
    if kind != "plate" {
        let cur = s("position");
        row(ui, tk, &t0("app.f.position"), |ui, lid| {
            let mut pick = cur.clone();
            egui::ComboBox::new(("detail_position", id), "")
                .selected_text(t0(&format!("app.pos.{cur}")))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for p in ["top", "middle", "bottom"] {
                        ui.selectable_value(&mut pick, p.to_string(), t0(&format!("app.pos.{p}")));
                    }
                })
                .response
                .labelled_by(lid);
            if pick != cur {
                set(acts, "position", pick);
            }
        });
    }
    row(ui, tk, &t0("app.f.rotation"), |ui, lid| {
        if let Some(v) = text_field(dv, ui, &format!("{id}.rotation"), &cc_detail::js::num_to_string(n("rotation")), 80.0, lid) {
            set(acts, "rotation", v);
        }
    });
    let phases: Vec<f64> =
        cc_detail::js::arr(dv.model.state.get("phases")).iter().filter_map(|p| p.get("n").and_then(Value::as_f64)).collect();
    if phases.len() > 1 {
        let cur = if n("phase") > 0.0 { n("phase") } else { 1.0 };
        row(ui, tk, &t0("phase.prop"), |ui, lid| {
            let mut pick = cur;
            egui::ComboBox::new(("detail_phase", id), "")
                .selected_text(t("phase.default", &[("n", F::N(cur))]))
                .show_ui(ui, |ui| {
                    for p in &phases {
                        ui.selectable_value(&mut pick, *p, t("phase.default", &[("n", F::N(*p))]));
                    }
                })
                .response
                .labelled_by(lid);
            if pick != cur {
                set(acts, "phase", cc_detail::js::num_to_string(pick));
            }
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new(t0("app.member.ends")).color(tk.text_dim));
    for end in ["start", "end"] {
        let p = cc_detail::js::p3_of(m.get(end));
        row(ui, tk, &t0(&format!("app.member.{end}")), |ui, lid| {
            let cur = format!(
                "{},{},{}",
                cc_detail::js::num_to_string(p[0]),
                cc_detail::js::num_to_string(p[1]),
                cc_detail::js::num_to_string(p[2])
            );
            if let Some(v) = text_field(dv, ui, &format!("{id}.{end}"), &cur, 160.0, lid) {
                set(acts, end, v);
            }
        });
        ui.label(RichText::new(cc_detail::model::node_name(&dv.model.state, p)).small().color(tk.text_dim));
    }
    if let Some(part) = dv.built.get(id) {
        ui.add_space(4.0);
        let mut rows: Vec<(String, String)> = vec![
            (t0("app.k.part-mark"), dv.marks.part_mark(id).unwrap_or("").to_string()),
            (t0("app.k.assembly"), dv.marks.asm_of(id).and_then(|a| dv.marks.asm_mark(a)).unwrap_or("").to_string()),
            (t0("app.k.cut-length"), t("app.unit.mm", &[("v", F::S(r1(part.length)))])),
            (
                t0("app.k.weight"),
                t("app.member.weight", &[("kg", F::S(format!("{:.1}", part.weight))), ("mass", F::S(format!("{:.2}", part.profile.mass)))]),
            ),
        ];
        if part.setback[0] != 0.0 || part.setback[1] != 0.0 {
            rows.push((
                t0("app.k.cut-back"),
                t("app.member.cut-back", &[("start", F::S(r1(part.setback[0]))), ("end", F::S(r1(part.setback[1])))]),
            ));
        }
        egui::Grid::new(("detail_info", id)).num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
            for (k, v) in rows {
                if v.is_empty() {
                    continue;
                }
                ui.label(RichText::new(k).color(tk.text_dim));
                ui.label(v);
                ui.end_row();
            }
        });
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        let why = cc_detail::shop::shop_refusal(dv.built.get(id));
        if why.is_none() && ui.button(t0("app.btn.shop")).clicked() {
            acts.push(Act::Run("detail.export", format!("shop:{id}")));
        }
    });
    for p in dv.built.problems.iter().filter(|x| x.r#ref.as_deref() == Some(id)) {
        ui.label(RichText::new(&p.text).color(if p.hard == Some(true) { tk.accent } else { tk.text_dim }));
    }
}

/// detailcraft.js `modelSummary()`.
fn model_summary(dv: &DetailView, ui: &mut Ui, tk: &Tokens) {
    ui.label(RichText::new(t0("app.props.model")).strong().color(tk.text_strong));
    let st = &dv.model.state;
    let len = |k: &str| cc_detail::js::arr(st.get(k)).len();
    let steel: f64 = dv.built.parts.iter().map(|p| p.weight).sum();
    let bars: f64 = dv.schedule.rows.iter().map(|r| r.total_weight).sum();
    let gx = cc_detail::js::arr(st.get("grids").and_then(|g| g.get("x"))).len();
    let gy = cc_detail::js::arr(st.get("grids").and_then(|g| g.get("y"))).len();
    let rows = [
        (t0("app.k.members"), len("members").to_string()),
        (t0("app.k.connections"), len("connections").to_string()),
        (t0("app.k.footings"), len("footings").to_string()),
        (t0("app.k.parts"), dv.built.parts.len().to_string()),
        (t0("app.k.part-marks"), dv.marks.parts.len().to_string()),
        (t0("app.k.assemblies"), dv.marks.assemblies.len().to_string()),
        (t0("app.k.bolts-rods"), dv.built.bolts.len().to_string()),
        (t0("app.k.steel"), t("app.unit.kg", &[("v", F::S(format!("{steel:.1}")))])),
        (t0("app.k.rebar"), t("app.unit.kg", &[("v", F::S(format!("{bars:.1}")))])),
        (t0("app.k.grids"), format!("{gx} × {gy}")),
        (t0("app.k.levels"), len("levels").to_string()),
    ];
    egui::Grid::new("detail_summary").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).color(tk.text_dim));
            ui.label(v);
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new(t0("app.model.note")).small().color(tk.text_dim));
}

/// A report table: the first row is the head; numeric columns right-aligned; a click on a first cell selects.
fn table(ui: &mut Ui, tk: &Tokens, id: &str, rows: &[Vec<String>], numeric: &[usize], mut on_first: impl FnMut(&str)) {
    let Some((head, body)) = rows.split_first() else { return };
    egui::Grid::new(("detail_table", id)).striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
        for h in head {
            ui.label(RichText::new(h).strong().size(11.0).color(tk.text_strong));
        }
        ui.end_row();
        for r in body {
            for (i, c) in r.iter().enumerate() {
                let text = RichText::new(c).size(11.0);
                if i == 0 {
                    if ui.link(text).clicked() {
                        on_first(c);
                    }
                } else if numeric.contains(&i) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(text.monospace()));
                } else {
                    ui.label(text);
                }
            }
            ui.end_row();
        }
    });
}

/// The reports (detailcraft.js `renderReport`).
pub fn reports(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t0("page.rep.label")).strong().color(tk.text_strong));
        ui.separator();
        let np = dv.built.problems.len();
        let nc = dv.clash.as_ref().map_or(0, |c| c.clashes.len());
        let count = |n: usize| if n > 0 { format!(" ({n})") } else { String::new() };
        for (tab, label) in [
            (Tab::Problems, format!("{}{}", t0("page.rep.problems"), count(np))),
            (Tab::Bom, t0("page.rep.bom")),
            (Tab::Clashes, format!("{}{}", t0("clash.tab"), count(nc))),
            (Tab::Phases, t0("rep.tab")),
            (Tab::Bending, t0("page.rep.bending")),
            (Tab::Nest, t0("nest.tab")),
        ] {
            if ui.selectable_label(dv.tab == tab, label).clicked() {
                dv.tab = tab;
            }
        }
    });
    ui.separator();
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match dv.tab {
        Tab::Problems => {
            if dv.built.problems.is_empty() {
                ui.label(RichText::new(t0("app.rep.problems.none")).color(tk.text_dim));
            }
            for p in &dv.built.problems {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&p.text).color(if p.hard == Some(true) { tk.accent } else { tk.text }));
                    let r = p.r#ref.as_deref().and_then(|x| match x.chars().next() {
                        Some('m') => Some(Ref::Member(x.into())),
                        Some('j') => Some(Ref::Conn(x.into())),
                        Some('f') => Some(Ref::Footing(x.into())),
                        Some('s') => Some(Ref::Stair(x.into())),
                        _ => None,
                    });
                    if let Some(r) = r
                        && ui.small_button(t0("app.rep.select")).clicked()
                    {
                        acts.push(Act::Select(Some(r)));
                    }
                });
            }
        }
        Tab::Bom => {
            let b = cc_detail::marks::bom_tables(&dv.marks, &dv.built.bolts);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("app.rep.parts", &[("kg", F::S(format!("{:.1}", b.total_weight)))])).strong());
                if ui.link(t0("app.rep.csv")).clicked() {
                    acts.push(Act::Run("detail.export", "bom".into()));
                }
            });
            let marks = &dv.marks;
            let built = &dv.built;
            table(ui, tk, "bom_parts", &b.parts, &[4, 5, 6, 7], |mark| {
                let r = marks
                    .parts
                    .iter()
                    .find(|p| p.mark == mark)
                    .and_then(|p| p.ids.first())
                    .and_then(|id| built.get(id))
                    .and_then(Ref::of_part);
                if r.is_some() {
                    acts.push(Act::Select(r));
                }
            });
            ui.label(RichText::new(t0("app.rep.assemblies")).strong());
            table(ui, tk, "bom_asm", &b.assemblies, &[3, 4, 5], |_| {});
            ui.label(RichText::new(t0("app.rep.bolts")).strong());
            table(ui, tk, "bom_bolts", &b.bolts, &[1, 2, 4], |_| {});
        }
        Tab::Clashes => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t0("clash.title")).strong());
                ui.label(RichText::new(t0("clash.tol")).color(tk.text_dim));
                let mut tol = dv.clash_tol;
                ui.add(egui::DragValue::new(&mut tol).range(0.0..=50.0).speed(0.5));
                dv.clash_tol = tol;
                if ui.button(t0("clash.run")).clicked() {
                    acts.push(Act::Run("detail.clash", cc_detail::js::num_to_string(tol)));
                }
            });
            match &dv.clash {
                None => {
                    ui.label(RichText::new(t0("clash.intro")).color(tk.text_dim));
                }
                Some(c) => {
                    if c.version != dv.model.version {
                        ui.label(RichText::new(t0("clash.stale")).color(tk.accent));
                    }
                    if c.clashes.is_empty() {
                        ui.label(RichText::new(t("clash.clean", &[("parts", F::N(c.checked as f64))])).color(tk.text_dim));
                    } else {
                        let label = |v: &Value| -> String {
                            let id = cc_detail::js::string_of(v.get("id"));
                            match v.get("kind").and_then(Value::as_str) {
                                Some("member") => format!(
                                    "{} {}",
                                    dv.marks.part_mark(&id).unwrap_or(""),
                                    dv.member(&id).map(|m| cc_detail::js::string_of(m.get("profile"))).unwrap_or_default()
                                ),
                                Some("conn") => {
                                    let p = dv
                                        .built
                                        .get(&format!("{id}.plate"))
                                        .or_else(|| dv.built.parts.iter().find(|x| x.conn.as_deref() == Some(id.as_str())));
                                    p.map_or(id.clone(), |p| format!("{} {}", dv.marks.part_mark(&p.id).unwrap_or(""), p.profile_name))
                                }
                                _ => format!(
                                    "{} {}",
                                    dv.footing_marks.iter().find(|(k, _)| *k == id).map_or("", |(_, v)| v.as_str()),
                                    t0("clash.footing")
                                ),
                            }
                        };
                        let as_ref = |v: &Value| {
                            Ref::parse(&format!("{}:{}", cc_detail::js::string_of(v.get("kind")), cc_detail::js::string_of(v.get("id"))))
                        };
                        egui::Grid::new("detail_clashes").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                            for h in ["clash.col.a", "clash.col.b", "clash.col.depth", "clash.col.at"] {
                                ui.label(RichText::new(t0(h)).strong().size(11.0));
                            }
                            ui.end_row();
                            for c in &c.clashes {
                                ui.label(label(&c.a));
                                ui.label(label(&c.b));
                                ui.label(cc_detail::js::num_to_string(c.depth));
                                ui.label(cc_detail::model::node_name(&dv.model.state, c.at));
                                if ui.link(t0("clash.show")).clicked() {
                                    acts.push(Act::Select(as_ref(&c.a)));
                                }
                                ui.end_row();
                            }
                        });
                    }
                    let mut note = t0("clash.note");
                    if c.hidden > 0 {
                        note.push(' ');
                        note.push_str(&t0("clash.hidden"));
                    }
                    ui.label(RichText::new(note).small().color(tk.text_dim));
                }
            }
        }
        Tab::Phases => {
            let rep = cc_detail::report::phase_report(&dv.model.state, &dv.built, &dv.marks, &dv.schedule);
            let (summary, phases) = cc_detail::report::report_tables(&rep);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t0("rep.summary")).strong());
                if ui.link(t0("rep.csv")).clicked() {
                    acts.push(Act::Run("detail.export", "report-csv".into()));
                }
                if ui.link(t0("rep.pdf")).clicked() {
                    acts.push(Act::Run("detail.export", "report-pdf".into()));
                }
            });
            table(ui, tk, "rep_summary", &summary, &[1, 2, 3, 4, 5], |_| {});
            for (i, p) in phases.iter().enumerate() {
                ui.label(RichText::new(&p.title).strong());
                table(ui, tk, &format!("rep_parts{i}"), &p.parts, &[4, 5, 6, 7, 8], |_| {});
                if p.bolts.len() > 1 {
                    table(ui, tk, &format!("rep_bolts{i}"), &p.bolts, &[1, 2, 4], |_| {});
                }
            }
            ui.label(RichText::new(t0("rep.note")).small().color(tk.text_dim));
        }
        Tab::Bending => {
            let rows = cc_detail::rebar::schedule_table(&dv.schedule.rows, &dv.settings.shape_codes, &dv.settings.bar_names);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t("app.rep.bending2", &[("codes", F::S(t0(&format!("app.codes.{}", dv.settings.shape_codes))))]))
                        .strong(),
                );
                if ui.link(t0("app.rep.csv")).clicked() {
                    acts.push(Act::Run("detail.export", "bending".into()));
                }
            });
            if dv.schedule.rows.is_empty() {
                ui.label(RichText::new(t0("app.rep.bending.none")).color(tk.text_dim));
            } else {
                table(ui, tk, "bending", &rows, &[3, 4, 5, 6, 7, 8, 9, 10, 11], |_| {});
            }
        }
        Tab::Nest => {
            let s = &dv.settings;
            let r = cc_detail::nest::nest_bars(
                &cc_detail::nest::pieces_from_marks(&dv.marks),
                &cc_detail::nest::Stock::All(s.nest_stock.clone()),
                s.nest_kerf,
                s.nest_trim,
                s.nest_keep,
            );
            let (summary, bars) = cc_detail::nest::cut_list_tables(&r);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t("nest.title", &[("bars", F::N(r.bars as f64)), ("waste", F::S(format!("{:.1}", r.waste * 100.0)))]))
                        .strong(),
                );
                if ui.link(t0("app.rep.csv")).clicked() {
                    acts.push(Act::Run("detail.export", "cutlist".into()));
                }
            });
            ui.label(
                RichText::new(format!(
                    "{}: {} · {}: {} · {}: {} · {}: {}",
                    t0("nest.stock"),
                    s.nest_stock.iter().map(|x| cc_detail::js::num_to_string(*x)).collect::<Vec<_>>().join(" "),
                    t0("nest.kerf"),
                    cc_detail::js::num_to_string(s.nest_kerf),
                    t0("nest.trim"),
                    cc_detail::js::num_to_string(s.nest_trim),
                    t0("nest.keep-from"),
                    cc_detail::js::num_to_string(s.nest_keep)
                ))
                .small()
                .color(tk.text_dim),
            );
            if r.groups.is_empty() {
                ui.label(RichText::new(t0("nest.none")).color(tk.text_dim));
            } else {
                table(ui, tk, "nest_summary", &summary, &[2, 4, 5, 6], |_| {});
                ui.label(RichText::new(t0("nest.bars")).strong());
                table(ui, tk, "nest_bars", &bars, &[1, 2, 4], |_| {});
                ui.label(RichText::new(t0("nest.note")).small().color(tk.text_dim));
            }
        }
    });
}
