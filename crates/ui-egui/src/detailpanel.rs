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
    /// bring a model point (mm) to the middle of the view, about 2.5 m across (view3d.js `focusPoint`)
    Focus([f64; 3]),
    CloseReport,
    /// Esc: cancel the window being dragged, the placement, the tool or the selection
    Escape,
    /// a `detail.*` command with its name parameter
    Run(&'static str, String),
}

fn r1(v: f64) -> String {
    cc_detail::js::num_to_string((v * 10.0).round() / 10.0)
}
fn mm(v: f64) -> String {
    cc_detail::js::num_to_string(v.round())
}

/// The bar's groups as 0.7's toolbar (DetailCraft ui.inc: File, Place, Connect, Edit, Output, and the view's turns):
/// each button's id is what [`bar`] runs.
pub fn bar_groups(dv: &DetailView) -> Vec<crate::shell::ToolGroup> {
    use crate::shell::{Tool, ToolGroup};
    let tool = dv.tools.tool;
    let picked = dv.selected();
    let sel_type = match &dv.sel {
        Some(Ref::Member(id)) => dv.member(id).map(|m| m.get("type").and_then(Value::as_str).unwrap_or("").to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    let copyable = picked.iter().any(|r| !matches!(r, Ref::Conn(_)));
    let last = |s: &[cc_detail::model::Step]| s.last().map(|x| x.label.clone()).unwrap_or_default();
    let undo_tip =
        if dv.model.can_undo() { t("app.bar.undo.label", &[("label", F::S(last(&dv.model.undo_stack)))]) } else { t0("app.bar.undo") };
    let redo_tip =
        if dv.model.can_redo() { t("app.bar.redo.label", &[("label", F::S(last(&dv.model.redo_stack)))]) } else { t0("app.bar.redo") };
    let place = [
        ("select", "page.tool.select", "V", "cursor"),
        ("beam", "page.tool.beam", "B", "dash-lg"),
        ("column", "page.tool.column", "C", "grip-vertical"),
        ("brace", "page.tool.brace", "R", "slash-lg"),
        ("plate", "page.tool.plate", "P", "square"),
        ("footing", "page.tool.footing", "F", "grid-3x3"),
        ("stair", "page.tool.stair", "S", "bar-chart-steps"),
    ];
    let why_shop = cc_detail::shop::shop_refusal(dv.selected_part());
    let view = |v: &str, key: &str| if v == "iso" { crate::lang::mc(key) } else { crate::lang::t0(&cc_detail::i18n::CATALOG, key) };
    vec![
        ToolGroup {
            label: t0("page.group.file"),
            tools: vec![
                Tool::new("save", "download", t0("page.tool.save")).tip(t("page.tool.save.title", &[("key", F::S("Ctrl+S".into()))])),
            ],
        },
        ToolGroup {
            label: t0("page.group.place"),
            tools: place
                .iter()
                .map(|(id, key, letter, icon)| {
                    Tool::new(format!("tool:{id}"), icon, t0(key))
                        .tip(t(&format!("{key}.title"), &[("key", F::S((*letter).into()))]))
                        .on(tool == *id)
                })
                .collect(),
        },
        ToolGroup {
            label: t0("page.group.connect"),
            tools: vec![
                Tool::new("act:endplates", "layout-split", t0("page.tool.endplates"))
                    .tip(t0("page.tool.endplates.title"))
                    .enabled(sel_type == "beam"),
                Tool::new("act:baseplate", "bounding-box", t0("page.tool.baseplate"))
                    .tip(t0("page.tool.baseplate.title"))
                    .enabled(sel_type == "column"),
                Tool::new("menu:conn", "three-dots", t0("tool.out.more")).tip(t0("tool.conn.more.title")),
            ],
        },
        ToolGroup {
            label: t0("page.group.edit"),
            tools: vec![
                Tool::new("undo", "arrow-counterclockwise", t0("page.tool.undo")).tip(undo_tip).enabled(dv.model.can_undo()),
                Tool::new("redo", "arrow-clockwise", t0("page.tool.redo")).tip(redo_tip).enabled(dv.model.can_redo()),
                Tool::new("act:copy", "copy", t0("page.tool.copy"))
                    .tip(t("page.tool.copy.title", &[("key", F::S("Ctrl+C".into()))]))
                    .enabled(copyable),
                Tool::new("act:paste", "clipboard-plus", t0("page.tool.paste"))
                    .tip(t("page.tool.paste.title", &[("key", F::S("Ctrl+V".into()))]))
                    .enabled(dv.tools.clip.is_some()),
                Tool::new("menu:edit", "three-dots", t0("page.edit.more")).tip(t0("page.edit.more.title")),
            ],
        },
        ToolGroup {
            label: t0("page.group.output"),
            tools: vec![
                Tool::new("menu:export", "box-arrow-up-right", t0("input.m.export")),
                Tool::new("menu:drawings", "files", t0("page.tool.drawings")).tip(t0("page.tool.drawings.title")),
                Tool::new("shop", "file-earmark-arrow-down", t0("page.tool.shop"))
                    .tip(why_shop.clone().unwrap_or_else(|| t0("page.tool.shop.title")))
                    .enabled(why_shop.is_none()),
            ],
        },
        ToolGroup {
            label: t0("input.grp.view"),
            tools: vec![
                Tool::new("cam:iso", "box", view("iso", "ui.views.iso")),
                Tool::new("cam:top", "square", view("top", "viewcube.face.top")),
                Tool::new("cam:front", "layout-sidebar", view("front", "viewcube.face.front")),
                Tool::new("cam:right", "layout-sidebar-inset", view("right", "viewcube.face.right")),
                Tool::new("bolts", "record-circle", t0("app.k.bolts")).on(dv.settings.bolts),
                Tool::new("settings", "gear", t0("input.cmd.app.settings")),
            ],
        },
    ]
}

/// The bar over DetailCraft's view (0.7's toolbar from [`bar_groups`]): the buttons' commands, the Connect, Edit,
/// Export and Drawings menus under their buttons.
pub fn bar(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, groups: &[crate::shell::ToolGroup]) {
    let picked = dv.selected();
    let sel_type = match &dv.sel {
        Some(Ref::Member(id)) => dv.member(id).map(|m| m.get("type").and_then(Value::as_str).unwrap_or("").to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    let has_type = |tys: &[&str]| {
        picked.iter().any(|r| match r {
            Ref::Member(i) => dv.member(i).is_some_and(|m| tys.contains(&m.get("type").and_then(Value::as_str).unwrap_or(""))),
            _ => false,
        })
    };
    let (splice_ok, gusset_ok) = (has_type(&["beam", "column"]), has_type(&["brace"]));
    let sel_conn = matches!(dv.sel, Some(Ref::Conn(_)));
    let copyable = picked.iter().any(|r| !matches!(r, Ref::Conn(_)));
    let can_paste = dv.tools.clip.is_some();
    let any = dv.model.state.get("members").and_then(Value::as_array).map_or(0, Vec::len)
        + dv.model.state.get("footings").and_then(Value::as_array).map_or(0, Vec::len)
        > 0;
    let app_c = tk.app("detailcraft");
    let run = |acts: &mut Vec<Act>, id: &'static str, name: String| acts.push(Act::Run(id, name));
    for (id, resp) in crate::shell::toolbar(ui, tk, app_c, "dc-tb", groups) {
        match id.as_str() {
            "menu:conn" => {
                egui::Popup::menu(&resp).show(|ui| {
                    for (a, label, tip, on) in [
                        ("sheartabs", "page.tool.sheartabs", "page.tool.sheartabs.title", sel_type == "beam"),
                        ("splice", "page.tool.splice", "page.tool.splice.title", splice_ok),
                        ("gusset", "page.tool.gusset", "page.tool.gusset.title", gusset_ok),
                        ("similar", "page.tool.similar", "page.tool.similar.title", sel_conn),
                    ] {
                        if ui.add_enabled(on, egui::Button::new(t0(label))).on_hover_text(t0(tip)).clicked() {
                            run(acts, "detail.act", a.into());
                            ui.close();
                        }
                    }
                });
            }
            "menu:edit" => {
                egui::Popup::menu(&resp).show(|ui| {
                    for (a, label, key, on) in [
                        ("cut", "page.edit.cut", "Ctrl+X", copyable),
                        ("copy", "page.edit.copy", "Ctrl+C", copyable),
                        ("paste", "page.edit.paste", "Ctrl+V", can_paste),
                        ("paste-in-place", "page.edit.paste-in-place", "Ctrl+Shift+V", can_paste),
                        ("duplicate", "page.edit.duplicate", "Ctrl+D", copyable),
                        ("move", "page.edit.move", "M", copyable),
                        ("delete", "page.edit.delete", "Delete", !picked.is_empty()),
                        ("select-all", "page.edit.select-all", "Ctrl+A", any),
                        ("select-none", "page.edit.select-none", "Esc", !picked.is_empty()),
                    ] {
                        if ui.add_enabled(on, egui::Button::new(format!("{}    {key}", t0(label)))).clicked() {
                            run(acts, "detail.act", a.into());
                            ui.close();
                        }
                    }
                });
            }
            "menu:export" => {
                egui::Popup::menu(&resp).show(|ui| {
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
                            run(acts, "detail.export", kind.into());
                            ui.close();
                        }
                    }
                });
            }
            "menu:drawings" => {
                egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
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
                        run(acts, "detail.setting", format!("paper={paper}"));
                    }
                    ui.separator();
                    let fname = crate::detailexport::format_name(&dv.dwg_format).unwrap_or_default();
                    if ui.button(t("dwg.all", &[("format", F::S(fname))])).on_hover_text(t0("page.tool.drawings.title")).clicked() {
                        run(acts, "detail.export", format!("drawings:{}", dv.dwg_format));
                        ui.close();
                    }
                });
            }
            _ if !resp.clicked() => {}
            "save" => run(acts, "detail.save", String::new()),
            "undo" => run(acts, "detail.undo", String::new()),
            "redo" => run(acts, "detail.redo", String::new()),
            "shop" => run(acts, "detail.export", "shop".into()),
            "bolts" => run(acts, "detail.setting", format!("bolts={}", !dv.settings.bolts)),
            "settings" => run(acts, "detail.settings", "open".into()),
            x => {
                if let Some(v) = x.strip_prefix("tool:") {
                    run(acts, "detail.tool", v.into());
                } else if let Some(v) = x.strip_prefix("act:") {
                    run(acts, "detail.act", v.into());
                } else if let Some(v) = x.strip_prefix("cam:") {
                    run(acts, "detail.camera", v.into());
                }
            }
        }
    }
}

/// The left panel's head: the model's name.
pub fn left_head(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let st = &dv.model.state;
    let head = ui.label(RichText::new(t0("page.left.model")).strong().color(tk.text_strong));
    let name = st.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let key = "model.name".to_string();
    if let Some(v) = text_field(dv, ui, &key, &name, 200.0, head.id) {
        acts.push(Act::Run("detail.set", format!("model.name={v}")));
    }
    ui.label(RichText::new(t0("page.left.model.note")).small().color(tk.text_dim));
    ui.add_space(6.0);
}

/// The left panel: part marks and the members by type.
pub fn left(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
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
pub(crate) fn text_field(dv: &mut DetailView, ui: &mut Ui, key: &str, current: &str, width: f32, label: egui::Id) -> Option<String> {
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
pub(crate) fn row(ui: &mut Ui, tk: &Tokens, label: &str, add: impl FnOnce(&mut Ui, egui::Id)) {
    ui.horizontal(|ui| {
        let l = ui.add_sized([112.0, 18.0], egui::Label::new(RichText::new(label).color(tk.text_dim)).truncate());
        add(ui, l.id);
    });
}

pub(crate) fn member_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
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
pub(crate) fn model_summary(dv: &DetailView, ui: &mut Ui, tk: &Tokens) {
    ui.label(RichText::new(t0("app.props.model")).strong().color(tk.text_strong));
    let st = &dv.model.state;
    let len = |k: &str| cc_detail::js::arr(st.get(k)).len();
    let steel: f64 = dv.built.parts.iter().fold(0.0, |s, p| s + p.weight);
    let bars: f64 = dv.schedule.rows.iter().fold(0.0, |s, r| s + r.total_weight);
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
        Tab::Bom | Tab::Phases | Tab::Bending | Tab::Nest => {
            // made once per model version and settings (the cut list nests every bar: not every frame)
            let key = format!("{:?}|{}|{:?}", dv.tab, dv.model.version, dv.settings);
            if dv.tables.as_ref().is_none_or(|x| x.key != key) {
                dv.tables = Some(tab_tables(dv, key));
            }
            let Some(tt) = dv.tables.as_ref() else { return };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&tt.title).strong());
                for (label, kind) in &tt.links {
                    if ui.link(t0(label)).clicked() {
                        acts.push(Act::Run("detail.export", (*kind).into()));
                    }
                }
            });
            if let Some(sub) = &tt.sub {
                ui.label(RichText::new(sub).small().color(tk.text_dim));
            }
            if let Some(e) = &tt.empty {
                ui.label(RichText::new(e).color(tk.text_dim));
            }
            let marks = &dv.marks;
            let built = &dv.built;
            for (i, (head, rows, numeric)) in tt.blocks.iter().enumerate() {
                if let Some(h) = head {
                    ui.label(RichText::new(h).strong());
                }
                let links = dv.tab == Tab::Bom && i == 0;
                table(ui, tk, &format!("detail_tab{i}"), rows, numeric, |mark| {
                    if !links {
                        return;
                    }
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
            }
            if let Some(n) = &tt.note {
                ui.label(RichText::new(n).small().color(tk.text_dim));
            }
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
                                    acts.push(Act::Focus(c.at));
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
    });
}

/// One table of a report tab: (heading, rows with the head first, numeric columns).
pub type Block = (Option<String>, Vec<Vec<String>>, Vec<usize>);

/// The tables of a report tab, made once per model version and settings.
pub struct TabTables {
    pub key: String,
    pub title: String,
    /// (catalog label, export kind) links after the title
    pub links: Vec<(&'static str, &'static str)>,
    pub sub: Option<String>,
    pub empty: Option<String>,
    pub blocks: Vec<Block>,
    pub note: Option<String>,
}

/// detailcraft.js `renderReport` for the BOM, phases, bending and cut-list tabs.
fn tab_tables(dv: &DetailView, key: String) -> TabTables {
    let mut tt = TabTables { key, title: String::new(), links: Vec::new(), sub: None, empty: None, blocks: Vec::new(), note: None };
    match dv.tab {
        Tab::Bom => {
            let b = cc_detail::marks::bom_tables(&dv.marks, &dv.built.bolts);
            tt.title = t("app.rep.parts", &[("kg", F::S(format!("{:.1}", b.total_weight)))]);
            tt.links.push(("app.rep.csv", "bom"));
            tt.blocks.push((None, b.parts, vec![4, 5, 6, 7]));
            tt.blocks.push((Some(t0("app.rep.assemblies")), b.assemblies, vec![3, 4, 5]));
            tt.blocks.push((Some(t0("app.rep.bolts")), b.bolts, vec![1, 2, 4]));
        }
        Tab::Phases => {
            let rep = cc_detail::report::phase_report(&dv.model.state, &dv.built, &dv.marks, &dv.schedule);
            let (summary, phases) = cc_detail::report::report_tables(&rep);
            tt.title = t0("rep.summary");
            tt.links.push(("rep.csv", "report-csv"));
            tt.links.push(("rep.pdf", "report-pdf"));
            tt.blocks.push((None, summary, vec![1, 2, 3, 4, 5]));
            for p in phases {
                let bolts = p.bolts;
                tt.blocks.push((Some(p.title), p.parts, vec![4, 5, 6, 7, 8]));
                if bolts.len() > 1 {
                    tt.blocks.push((None, bolts, vec![1, 2, 4]));
                }
            }
            tt.note = Some(t0("rep.note"));
        }
        Tab::Bending => {
            tt.title = t("app.rep.bending2", &[("codes", F::S(t0(&format!("app.codes.{}", dv.settings.shape_codes))))]);
            tt.links.push(("app.rep.csv", "bending"));
            if dv.schedule.rows.is_empty() {
                tt.empty = Some(t0("app.rep.bending.none"));
            } else {
                let rows = cc_detail::rebar::schedule_table(&dv.schedule.rows, &dv.settings.shape_codes, &dv.settings.bar_names);
                tt.blocks.push((None, rows, vec![3, 4, 5, 6, 7, 8, 9, 10, 11]));
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
            tt.title = t("nest.title", &[("bars", F::N(r.bars as f64)), ("waste", F::S(format!("{:.1}", r.waste * 100.0)))]);
            tt.links.push(("app.rep.csv", "cutlist"));
            tt.sub = Some(format!(
                "{}: {} · {}: {} · {}: {} · {}: {}",
                t0("nest.stock"),
                s.nest_stock.iter().map(|x| cc_detail::js::num_to_string(*x)).collect::<Vec<_>>().join(" "),
                t0("nest.kerf"),
                cc_detail::js::num_to_string(s.nest_kerf),
                t0("nest.trim"),
                cc_detail::js::num_to_string(s.nest_trim),
                t0("nest.keep-from"),
                cc_detail::js::num_to_string(s.nest_keep)
            ));
            if r.groups.is_empty() {
                tt.empty = Some(t0("nest.none"));
            } else {
                tt.blocks.push((None, summary, vec![2, 4, 5, 6]));
                tt.blocks.push((Some(t0("nest.bars")), bars, vec![1, 2, 4]));
                tt.note = Some(t0("nest.note"));
            }
        }
        Tab::Problems | Tab::Clashes => {}
    }
    tt
}
