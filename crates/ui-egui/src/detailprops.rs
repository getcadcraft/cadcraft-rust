//! DetailCraft's modelling panels (M14.3–M14.7), labelled from DetailCraft's catalog as detailcraft.js renders them:
//! the tool options and "Add by grid" (`toolOptions`, `addByGridForm`, `footingForm`, `stairForm`), the paste tool's
//! forms (`pasteOptions`), several selected (`multiProps`, `manyEditor`, `xfBlock`), a connection's parameters
//! (`connProps`), footings and stairs (`footingProps`, `stairProps`), holes and copes (`holeEditor`, `copeEditor`), the
//! left panel's grids, levels, phases and custom profiles, and the Settings window (settings.inc). Panels only read
//! the view and hand back [`Act`]s; every change is a `detail.*` command.

use cc_detail::i18n::{F, t, t0};
use cc_detail::js::{arr, num_to_string, round, string_of};
use egui::{RichText, Ui};
use serde_json::Value;

use crate::detail3d::Ref;
use crate::detailpanel::{Act, row, text_field};
use crate::detailtools::{param_choices, param_help_key, param_label};
use crate::detailview::{DetailView, type_name};
use crate::theme::Tokens;

/// The panels' form fields that live until the selection, the tool or the model changes (detailcraft.js renders its
/// forms again then, back to their starting values).
#[derive(Clone, Debug, Default)]
pub struct Forms {
    /// what the per-selection forms below belong to
    pub key: String,
    pub hole: [String; 4],
    pub cope: [String; 4],
    pub xf_keep: bool,
    pub xf_off: [String; 3],
    pub xf_deg: String,
    pub xf_about: String,
    pub xf_line: String,
    pub paste_off: [String; 3],
    pub paste_at: [String; 3],
    pub by_grid: [String; 6],
    /// left panel: the grid form while edited, the levels form while edited, the custom profile form
    pub grids: Option<[String; 4]>,
    pub levels: Option<Vec<(String, String)>>,
    pub pname: String,
    pub pkind: String,
    pub pdims: Vec<(String, String)>,
}

fn mm(v: f64) -> String {
    num_to_string(round(v))
}
fn r1(v: f64) -> String {
    num_to_string(round(v * 10.0) / 10.0)
}

/// A select (ComboBox) of (value, label) options; hands back a new value.
fn combo(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    lid: egui::Id,
    cur: &str,
    options: &[(String, String)],
    width: f32,
) -> Option<String> {
    let shown = options.iter().find(|(v, _)| v == cur).map_or_else(|| cur.to_string(), |(_, l)| l.clone());
    let mut pick = cur.to_string();
    egui::ComboBox::new(id, "")
        .selected_text(shown)
        .width(width)
        .height(360.0)
        .show_ui(ui, |ui| {
            for (v, l) in options {
                ui.selectable_value(&mut pick, v.clone(), l);
            }
        })
        .response
        .labelled_by(lid);
    (pick != cur).then_some(pick)
}

/// A plain text box bound to a form field (no command until a button).
fn form_text(ui: &mut Ui, v: &mut String, width: f32, lid: egui::Id) {
    ui.add(egui::TextEdit::singleline(v).desired_width(width)).labelled_by(lid);
}

fn sub(ui: &mut Ui, tk: &Tokens, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong().color(tk.text_strong));
}
fn note(ui: &mut Ui, tk: &Tokens, text: &str) {
    ui.label(RichText::new(text).small().color(tk.text_dim));
}
fn kv(ui: &mut Ui, tk: &Tokens, id: &str, rows: &[(String, String)]) {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
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
fn run(acts: &mut Vec<Act>, id: &'static str, name: String) {
    acts.push(Act::Run(id, name));
}

fn profile_options(dv: &DetailView, only: Option<&[&str]>) -> Vec<(String, String)> {
    let custom = arr(dv.model.state.get("profiles")).to_vec();
    let mut out = Vec::new();
    for g in cc_detail::profiles::catalog_groups(&custom) {
        for n in &g.names {
            if let Some(kinds) = only {
                let k = cc_detail::profiles::get_profile_str(n, &custom).map(|p| p.kind.clone()).unwrap_or_default();
                if !kinds.contains(&k.as_str()) {
                    continue;
                }
            }
            out.push((n.clone(), n.clone()));
        }
    }
    out
}
fn material_options() -> Vec<(String, String)> {
    cc_detail::model::MATERIALS.iter().map(|m| (m.to_string(), m.to_string())).collect()
}
fn position_options() -> Vec<(String, String)> {
    ["top", "middle", "bottom"].iter().map(|p| (p.to_string(), t0(&format!("app.pos.{p}")))).collect()
}
fn bar_options(naming: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for set in ["astm", "bs"] {
        for b in cc_detail::rebar::bar_set(set) {
            let db = cc_detail::rebar::bar(&b).map_or(f64::NAN, |x| x.db);
            out.push((b.clone(), format!("{} ({} mm)", cc_detail::rebar::bar_label(&b, naming), r1(db))));
        }
    }
    out
}
fn grid_labels(dv: &DetailView, axis: &str) -> Vec<String> {
    arr(dv.model.state.get("grids").and_then(|g| g.get(axis))).iter().map(|g| string_of(g.get("label"))).collect()
}
fn level_names(dv: &DetailView) -> Vec<String> {
    arr(dv.model.state.get("levels")).iter().map(|l| string_of(l.get("name"))).collect()
}
fn opts_of(list: &[String]) -> Vec<(String, String)> {
    list.iter().map(|x| (x.clone(), x.clone())).collect()
}
fn phase_options(dv: &DetailView) -> Vec<(String, String)> {
    arr(dv.model.state.get("phases"))
        .iter()
        .filter_map(|p| p.get("n").and_then(Value::as_f64))
        .map(|n| (num_to_string(n), dv.phase_name(n)))
        .collect()
}

/// Resets the per-selection forms when what they belong to changed.
fn sync_forms(dv: &mut DetailView) {
    let key = format!("{}|{:?}|{:?}|{}", dv.tools.tool, dv.sel, dv.multi, dv.model.version);
    if dv.forms.key == key {
        return;
    }
    let s = &dv.model.state;
    let gy = grid_labels(dv, "y");
    let gx = grid_labels(dv, "x");
    let lv = level_names(dv);
    let first = |v: &[String]| v.first().cloned().unwrap_or_default();
    let top = lv.last().cloned().unwrap_or_default();
    let tool = dv.tools.tool;
    let l1 = if matches!(tool, "beam" | "plate") { top.clone() } else { first(&lv) };
    let l2 = if matches!(tool, "beam" | "plate" | "brace") { top } else { first(&lv) };
    let gx2 = gx.get(1.min(gx.len().saturating_sub(1))).cloned().unwrap_or_default();
    let off = dv.settings.offset;
    let line = arr(s.get("grids").and_then(|g| g.get("y")))
        .first()
        .map(|g| format!("y:{}", string_of(g.get("label"))))
        .or_else(|| arr(s.get("grids").and_then(|g| g.get("x"))).first().map(|g| format!("x:{}", string_of(g.get("label")))))
        .unwrap_or_default();
    let face = match &dv.sel {
        Some(Ref::Member(id)) => {
            let custom = arr(s.get("profiles")).to_vec();
            let p = dv.member(id).and_then(|m| cc_detail::profiles::get_profile(m.get("profile").unwrap_or(&Value::Null), &custom));
            cc_detail::profiles::hole_faces(p.as_deref()).first().map(|f| f.to_string()).unwrap_or_default()
        }
        _ => String::new(),
    };
    let f = &mut dv.forms;
    f.key = key;
    f.hole = [face, "100".into(), "0".into(), "22".into()];
    f.cope = ["start".into(), "top".into(), "100".into(), "40".into()];
    f.xf_keep = false;
    f.xf_off = ["0".into(), "0".into(), "0".into()];
    f.xf_deg = "90".into();
    f.xf_about = String::new();
    f.xf_line = line;
    f.paste_off = off.map(num_to_string);
    f.paste_at = [first(&gy), first(&gx), first(&lv)];
    f.by_grid = [first(&gy), first(&gx), l1, first(&gy), gx2, l2];
}

/// The right panel (detailcraft.js `renderProps`).
pub fn right(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    sync_forms(dv);
    let head = |ui: &mut Ui, s: String| {
        ui.label(RichText::new(s).strong().color(tk.text_strong));
        ui.add_space(4.0);
    };
    let tool = dv.tools.tool;
    if tool == "paste" {
        head(ui, t0("app.props.paste"));
        paste_options(dv, ui, tk, acts);
    } else if tool == "move" {
        head(ui, t0("xf.move.title"));
        note(ui, tk, &t0("xf.move.help"));
    } else if tool != "select" {
        head(ui, t0(&format!("app.props.place.{tool}")));
        tool_options(dv, ui, tk, acts);
    } else if !dv.multi.is_empty() {
        head(ui, t("app.props.selected", &[("n", F::N(dv.multi.len() as f64))]));
        multi_props(dv, ui, tk, acts);
    } else {
        match dv.sel.clone() {
            Some(Ref::Member(id)) => {
                crate::detailpanel::member_props(dv, ui, tk, acts, &id);
                member_extras(dv, ui, tk, acts, &id);
            }
            Some(Ref::Conn(id)) => conn_props(dv, ui, tk, acts, &id),
            Some(Ref::Footing(id)) => footing_props(dv, ui, tk, acts, &id),
            Some(Ref::Stair(id)) => stair_props(dv, ui, tk, acts, &id),
            None => {
                crate::detailpanel::model_summary(dv, ui, tk);
                let nb = arr(dv.model.state.get("bolts")).len();
                if nb > 0 {
                    ui.add_space(6.0);
                    note(ui, tk, &t("app.bolts.loose", &[("n", F::N(nb as f64))]));
                    if ui.button(t0("app.btn.bolts-remove")).clicked() {
                        run(acts, "detail.act", "remove-bolts".into());
                    }
                }
            }
        }
    }
}

/// detailcraft.js `toolOptions()`.
fn tool_options(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let tool = dv.tools.tool;
    if tool == "footing" {
        let f = dv.tools.opts.footing.clone();
        footing_form(dv, ui, tk, acts, &f, None);
        let bar = string_of(f.get("mat").and_then(|m| m.get("barX")));
        let leg = cc_detail::rebar::bar(&bar).map_or(f64::NAN, cc_detail::rebar::hook_leg);
        note(
            ui,
            tk,
            &t("app.foot.hook-leg", &[("bar", F::S(cc_detail::rebar::bar_label(&bar, &dv.settings.bar_names))), ("leg", F::S(mm(leg)))]),
        );
        by_grid_form(dv, ui, tk, acts);
        return;
    }
    if tool == "stair" {
        let x = dv.tools.opts.stair.clone();
        stair_form(dv, ui, tk, acts, &x, None);
        note(ui, tk, &t0("app.stair.note"));
        by_grid_form(dv, ui, tk, acts);
        return;
    }
    let o = dv.tools.opts.clone();
    let opt = |acts: &mut Vec<Act>, f: &str, v: String| run(acts, "detail.opt", format!("{tool}.{f}={v}"));
    let (material, position, rotation) = match tool {
        "plate" => (o.plate_material.clone(), o.plate_position.clone(), o.plate_rotation),
        _ => match o.member(tool) {
            Some(m) => (m.material.clone(), m.position.clone(), m.rotation),
            None => return,
        },
    };
    if tool == "plate" {
        for (f, label, v) in [("t", "app.f.thickness", o.plate_t), ("w", "app.f.width", o.plate_w)] {
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(x) = text_field(dv, ui, &format!("opt.plate.{f}"), &num_to_string(v), 80.0, lid) {
                    opt(acts, f, x);
                }
            });
        }
    } else if let Some(m) = o.member(tool) {
        let opts = profile_options(dv, None);
        row(ui, tk, &t0("app.f.profile"), |ui, lid| {
            if let Some(v) = combo(ui, ("dc_opt_profile", tool), lid, &m.profile, &opts, 150.0) {
                opt(acts, "profile", v);
            }
        });
    }
    row(ui, tk, &t0("app.f.material"), |ui, lid| {
        if let Some(v) = combo(ui, ("dc_opt_mat", tool), lid, &material, &material_options(), 150.0) {
            opt(acts, "material", v);
        }
    });
    if tool != "plate" {
        row(ui, tk, &t0("app.f.position"), |ui, lid| {
            if let Some(v) = combo(ui, ("dc_opt_pos", tool), lid, &position, &position_options(), 150.0) {
                opt(acts, "position", v);
            }
        });
    }
    row(ui, tk, &t0("app.f.rotation"), |ui, lid| {
        if let Some(x) = text_field(dv, ui, &format!("opt.{tool}.rotation"), &num_to_string(rotation), 80.0, lid) {
            opt(acts, "rotation", x);
        }
    });
    if tool == "column" {
        let mut lv = vec![(String::new(), t0("app.f.up-to.next"))];
        lv.extend(opts_of(&level_names(dv)));
        row(ui, tk, &t0("app.f.up-to"), |ui, lid| {
            if let Some(v) = combo(ui, "dc_opt_to", lid, &o.column_to, &lv, 150.0) {
                opt(acts, "to", v);
            }
        });
    }
    let custom = arr(dv.model.state.get("profiles")).to_vec();
    let prof = if tool == "plate" {
        cc_detail::profiles::get_profile_str(&cc_detail::profiles::plate_name(o.plate_t, o.plate_w), &[])
    } else {
        o.member(tool).and_then(|m| cc_detail::profiles::get_profile_str(&m.profile, &custom))
    };
    if let Some(p) = prof {
        sub(ui, tk, &t0("app.f.profile"));
        let rows: Vec<(String, String)> = cc_detail::profiles::describe(&p).into_iter().map(|[k, v]| (k, v)).collect();
        kv(ui, tk, "dc_opt_desc", &rows);
    }
    by_grid_form(dv, ui, tk, acts);
}

/// detailcraft.js `addByGridForm()`: placing by keyboard from lists of grid lines and levels.
fn by_grid_form(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let (gy, gx, lv) = (opts_of(&grid_labels(dv, "y")), opts_of(&grid_labels(dv, "x")), opts_of(&level_names(dv)));
    sub(ui, tk, &t0("app.grid.add.title"));
    if gy.is_empty() || gx.is_empty() {
        note(ui, tk, &t0("app.grid.add.none"));
        return;
    }
    let two = matches!(dv.tools.tool, "beam" | "brace" | "plate");
    let labels: [&str; 6] = if two {
        [
            "app.grid.from.letter",
            "app.grid.from.number",
            "app.grid.from.level",
            "app.grid.to.letter",
            "app.grid.to.number",
            "app.grid.to.level",
        ]
    } else {
        ["app.grid.letter", "app.grid.number", "app.grid.level", "", "", ""]
    };
    let n = if two { 6 } else { 3 };
    for i in 0..n {
        let list = match i % 3 {
            0 => &gy,
            1 => &gx,
            _ => &lv,
        };
        let cur = dv.forms.by_grid.get(i).cloned().unwrap_or_default();
        row(ui, tk, &t0(labels.get(i).copied().unwrap_or("")), |ui, lid| {
            if let Some(v) = combo(ui, ("dc_bg", i), lid, &cur, list, 120.0)
                && let Some(slot) = dv.forms.by_grid.get_mut(i)
            {
                *slot = v;
            }
        });
    }
    if ui.button(t0("app.grid.add")).clicked() {
        let b = &dv.forms.by_grid;
        let a = format!("{}/{}/{}", b[0], b[1], b[2]);
        run(acts, "detail.bygrid", if two { format!("{a};{}/{}/{}", b[3], b[4], b[5]) } else { a });
    }
}

/// detailcraft.js `footingForm(f, set, placing)`: the tool's options (`id` None) or a footing's fields.
fn footing_form(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, f: &Value, id: Option<&str>) {
    let set = |acts: &mut Vec<Act>, field: &str, v: String| match id {
        Some(id) => run(acts, "detail.set", format!("{id}.{field}={v}")),
        None => run(acts, "detail.opt", format!("footing.{field}={v}")),
    };
    let g = |path: &str| -> Value {
        let mut v = f;
        for k in path.split('.') {
            v = v.get(k).unwrap_or(&Value::Null);
        }
        v.clone()
    };
    let key = id.unwrap_or("opt.footing").to_string();
    let num_row = |dv: &mut DetailView, ui: &mut Ui, acts: &mut Vec<Act>, field: &str, label: &str| {
        let cur = g(field);
        row(ui, tk, &t0(label), |ui, lid| {
            let text = cur.as_f64().map_or_else(|| string_of(Some(&cur)), num_to_string);
            if let Some(x) = text_field(dv, ui, &format!("{key}.{field}"), &text, 80.0, lid) {
                set(acts, field, x);
            }
        });
    };
    if id.is_some() {
        for (field, label) in [("x", "app.foot.cx"), ("y", "app.foot.cy"), ("top", "app.foot.top"), ("rotation", "app.f.rotation")] {
            num_row(dv, ui, acts, field, label);
        }
    }
    for (field, label) in [("L", "app.foot.l"), ("B", "app.foot.b"), ("H", "app.foot.h")] {
        num_row(dv, ui, acts, field, label);
    }
    let concrete = string_of(f.get("concrete"));
    row(ui, tk, &t0("app.foot.concrete"), |ui, lid| {
        if let Some(x) = text_field(dv, ui, &format!("{key}.concrete"), &concrete, 120.0, lid) {
            set(acts, "concrete", x);
        }
    });
    let bars = bar_options(&dv.settings.bar_names);
    let check = |ui: &mut Ui, acts: &mut Vec<Act>, field: &str, label: &str| {
        let mut on = g(field).as_bool().unwrap_or(false);
        if ui.checkbox(&mut on, t0(label)).changed() {
            set(acts, field, on.to_string());
        }
    };
    sub(ui, tk, &t0("app.foot.mat"));
    for (field, label, is_bar) in [
        ("mat.barX", "app.foot.long", true),
        ("mat.sX", "app.foot.spacing.x", false),
        ("mat.barY", "app.foot.short", true),
        ("mat.sY", "app.foot.spacing.y", false),
        ("mat.cover", "app.foot.cover", false),
    ] {
        if is_bar {
            let cur = string_of(Some(&g(field)));
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(v) = combo(ui, (&key, field), lid, &cur, &bars, 130.0) {
                    set(acts, field, v);
                }
            });
        } else {
            num_row(dv, ui, acts, field, label);
        }
    }
    check(ui, acts, "mat.hooks", "app.foot.hooks");
    sub(ui, tk, &t0("app.foot.starters"));
    check(ui, acts, "starters.on", "app.foot.starters");
    if g("starters.on").as_bool() == Some(true) {
        let cur = string_of(Some(&g("starters.bar")));
        row(ui, tk, &t0("app.foot.bar"), |ui, lid| {
            if let Some(v) = combo(ui, (&key, "starters.bar"), lid, &cur, &bars, 130.0) {
                set(acts, "starters.bar", v);
            }
        });
        for (field, label) in [
            ("starters.nx", "app.foot.nx"),
            ("starters.ny", "app.foot.ny"),
            ("starters.sx", "app.foot.sx"),
            ("starters.sy", "app.foot.sy"),
            ("starters.projection", "app.foot.projection"),
        ] {
            num_row(dv, ui, acts, field, label);
        }
        check(ui, acts, "starters.hook", "app.foot.hook");
    }
}

/// detailcraft.js `stairForm(x, set, placing)`.
fn stair_form(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, x: &Value, id: Option<&str>) {
    let set = |acts: &mut Vec<Act>, field: &str, v: String| match id {
        Some(id) => run(acts, "detail.set", format!("{id}.{field}={v}")),
        None => run(acts, "detail.opt", format!("stair.{field}={v}")),
    };
    let key = id.unwrap_or("opt.stair").to_string();
    let g = |path: &str| -> f64 {
        let mut v = x;
        for k in path.split('.') {
            v = v.get(k).unwrap_or(&Value::Null);
        }
        v.as_f64().unwrap_or(f64::NAN)
    };
    let mut fields: Vec<(&str, String)> = Vec::new();
    if id.is_some() {
        fields.extend([("x", t0("app.foot.cx")), ("y", t0("app.foot.cy")), ("z", t0("app.stair.z"))]);
    }
    fields.extend([
        ("dir", t0("app.stair.dir")),
        ("rise", t0(if id.is_none() { "app.stair.rise.tool" } else { "app.stair.rise" })),
        ("going", t0("app.stair.going")),
        ("width", t0("app.stair.width")),
        ("maxRiser", t0("app.stair.max-riser")),
    ]);
    for (field, label) in &fields {
        let cur = num_to_string(g(field));
        row(ui, tk, label, |ui, lid| {
            if let Some(v) = text_field(dv, ui, &format!("{key}.{field}"), &cur, 80.0, lid) {
                set(acts, field, v);
            }
        });
    }
    let strs = profile_options(dv, Some(&["C", "PL"]));
    let cur = string_of(x.get("stringer"));
    row(ui, tk, &t0("app.stair.stringer"), |ui, lid| {
        if let Some(v) = combo(ui, (&key, "stringer"), lid, &cur, &strs, 130.0) {
            set(acts, "stringer", v);
        }
    });
    for (field, label) in [("tread.t", "app.stair.tread-t"), ("tread.nosing", "app.stair.nosing")] {
        let cur = num_to_string(g(field));
        row(ui, tk, &t0(label), |ui, lid| {
            if let Some(v) = text_field(dv, ui, &format!("{key}.{field}"), &cur, 80.0, lid) {
                set(acts, field, v);
            }
        });
    }
    let cur = string_of(x.get("material"));
    row(ui, tk, &t0("app.f.material"), |ui, lid| {
        if let Some(v) = combo(ui, (&key, "material"), lid, &cur, &material_options(), 130.0) {
            set(acts, "material", v);
        }
    });
}

/// detailcraft.js `pasteOptions()`.
fn paste_options(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let Some(c) = dv.tools.paste_clip.clone() else { return };
    let n = |k: &str| {
        let l = arr(c.get(k)).len();
        if l > 0 { l.to_string() } else { String::new() }
    };
    let base = cc_detail::js::p3_of(c.get("base"));
    kv(
        ui,
        tk,
        "dc_paste_info",
        &[
            (t0("app.paste.k.from"), string_of(c.get("from"))),
            (t0("app.k.members"), n("members")),
            (t0("app.k.connections"), n("connections")),
            (t0("app.k.footings"), n("footings")),
            (t0("app.paste.k.base"), t("app.unit.mm", &[("v", F::S(base.map(mm).join(", ")))])),
        ],
    );
    note(ui, tk, &t0("app.paste.note"));
    sub(ui, tk, &t0("app.paste.offset.title"));
    ui.horizontal(|ui| {
        let l = ui.label(RichText::new(t0("app.paste.offset.axes")).color(tk.text_dim));
        for v in dv.forms.paste_off.iter_mut() {
            form_text(ui, v, 56.0, l.id);
        }
    });
    ui.horizontal(|ui| {
        if ui.button(t0("app.paste.offset.go")).clicked() {
            run(acts, "detail.paste", format!("offset:{}", dv.forms.paste_off.join(",")));
        }
        if ui.button(t0("app.paste.in-place")).clicked() {
            run(acts, "detail.paste", "in-place".into());
        }
    });
    let (gy, gx, lv) = (opts_of(&grid_labels(dv, "y")), opts_of(&grid_labels(dv, "x")), opts_of(&level_names(dv)));
    if gy.is_empty() || gx.is_empty() {
        return;
    }
    sub(ui, tk, &t0("app.paste.grid.title"));
    for (i, (label, list)) in [("app.grid.letter", &gy), ("app.grid.number", &gx), ("app.grid.level", &lv)].into_iter().enumerate() {
        let cur = dv.forms.paste_at.get(i).cloned().unwrap_or_default();
        row(ui, tk, &t0(label), |ui, lid| {
            if let Some(v) = combo(ui, ("dc_pat", i), lid, &cur, list, 120.0)
                && let Some(slot) = dv.forms.paste_at.get_mut(i)
            {
                *slot = v;
            }
        });
    }
    if ui.button(t0("app.paste.grid.go")).clicked() {
        run(acts, "detail.paste", format!("at:{}", dv.forms.paste_at.join("/")));
    }
}

/// The phase select of Properties (detailcraft.js `phaseField`): one value, or "Varies".
fn phase_field(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, value: Option<f64>) {
    let mut opts = phase_options(dv);
    let cur = value.map(num_to_string).unwrap_or_default();
    if value.is_none() {
        opts.insert(0, (String::new(), t0("many.varies")));
    }
    row(ui, tk, &t0("phase.prop"), |ui, lid| {
        if let Some(v) = combo(ui, "dc_phase_of", lid, &cur, &opts, 150.0)
            && !v.is_empty()
        {
            run(acts, "detail.phases", format!("of:{v}"));
        }
    });
}

/// detailcraft.js `multiProps()`.
fn multi_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let multi = dv.multi.clone();
    let mem: Vec<Value> = multi
        .iter()
        .filter_map(|r| match r {
            Ref::Member(i) => dv.member(i).cloned(),
            _ => None,
        })
        .collect();
    let conns: Vec<&str> = multi.iter().filter(|r| matches!(r, Ref::Conn(_))).map(Ref::id).collect();
    let foots: Vec<&str> = multi.iter().filter(|r| matches!(r, Ref::Footing(_))).map(Ref::id).collect();
    let mut types: Vec<(String, usize)> = Vec::new();
    for m in &mem {
        let ty = string_of(m.get("type"));
        match types.iter_mut().find(|x| x.0 == ty) {
            Some(x) => x.1 += 1,
            None => types.push((ty, 1)),
        }
    }
    let type_text: Vec<String> = types.iter().map(|(ty, n)| t(&format!("app.count.type.{ty}"), &[("n", F::N(*n as f64))])).collect();
    let steel: f64 = mem
        .iter()
        .filter_map(|m| dv.built.get(&string_of(m.get("id"))))
        .chain(conns.iter().filter_map(|id| dv.built.get(&format!("{id}.plate"))))
        .map(|p| p.weight)
        .sum();
    kv(
        ui,
        tk,
        "dc_multi_info",
        &[
            (
                t0("app.k.members"),
                if mem.is_empty() { String::new() } else { format!("{} ({})", mem.len(), type_text.join(&t0("app.count.sep"))) },
            ),
            (t0("app.k.connections"), if conns.is_empty() { String::new() } else { conns.len().to_string() }),
            (t0("app.k.footings"), if foots.is_empty() { String::new() } else { foots.len().to_string() }),
            (t0("app.k.steel"), if steel != 0.0 { t("app.multi.steel", &[("kg", F::S(format!("{steel:.1}")))]) } else { String::new() }),
        ],
    );
    if mem.len() + foots.len() > 0 {
        let mut phs: Vec<f64> = mem.iter().map(|m| cc_detail::js::or(cc_detail::js::f(m.get("phase")), 1.0)).collect();
        for id in &foots {
            let f = arr(dv.model.state.get("footings")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(*id)).cloned();
            phs.push(f.map_or(1.0, |f| cc_detail::js::or(cc_detail::js::f(f.get("phase")), 1.0)));
        }
        phs.dedup();
        let one = phs.first().copied().filter(|p| phs.iter().all(|x| x == p));
        phase_field(dv, ui, tk, acts, one);
    }
    let ids: Vec<Value> = mem.iter().filter_map(|m| m.get("id").cloned()).collect();
    if ids.len() >= 2 {
        many_editor(dv, ui, tk, acts, &ids);
    }
    xf_block(dv, ui, tk, acts);
    let copyable = mem.len() + foots.len() > 0;
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(copyable, egui::Button::new(t0("app.btn.copy"))).clicked() {
            run(acts, "detail.act", "copy".into());
        }
        if ui.add_enabled(copyable, egui::Button::new(t0("app.btn.duplicate"))).clicked() {
            run(acts, "detail.act", "duplicate".into());
        }
        if ui.button(RichText::new(t0("app.btn.delete")).color(tk.accent)).clicked() {
            run(acts, "detail.act", "delete".into());
        }
    });
    note(ui, tk, &t0("app.multi.note"));
}

/// detailcraft.js `manyEditor(ids)`: "Change on all of them".
fn many_editor(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, ids: &[Value]) {
    let st = cc_detail::transform::many_patchable(&dv.model.state, ids);
    sub(ui, tk, &t0("many.sub"));
    let profiles = profile_options(dv, None);
    for key in cc_detail::transform::MANY_FIELDS {
        let f = st.get(key).cloned().unwrap_or(Value::Null);
        if f.get("applies") != Some(&Value::Bool(true)) {
            continue;
        }
        let varies = f.get("varies") == Some(&Value::Bool(true));
        let label = t0(&crate::detailtools::many_key(key));
        let value = f.get("value").cloned().unwrap_or(Value::Null);
        let set = |acts: &mut Vec<Act>, v: String| run(acts, "detail.many", format!("{key}={v}"));
        match key {
            "profile" | "material" | "position" => {
                let mut opts = match key {
                    "profile" => profiles.clone(),
                    "material" => material_options(),
                    _ => position_options(),
                };
                let cur = if varies { String::new() } else { string_of(Some(&value)) };
                if varies {
                    opts.insert(0, (String::new(), t0("many.varies")));
                }
                row(ui, tk, &label, |ui, lid| {
                    if let Some(v) = combo(ui, ("dc_many", key), lid, &cur, &opts, 150.0)
                        && !v.is_empty()
                    {
                        set(acts, v);
                    }
                });
            }
            _ => {
                let cur = if varies { String::new() } else { value.as_f64().map(num_to_string).unwrap_or_default() };
                row(ui, tk, &label, |ui, lid| {
                    if let Some(v) = text_field(dv, ui, &format!("many.{key}"), &cur, 80.0, lid) {
                        set(acts, v);
                    }
                });
            }
        }
    }
    note(ui, tk, &t0("many.note"));
}

/// detailcraft.js `xfBlock(refs)`: "Move, rotate, mirror" for the selection's members, footings and stairs.
fn xf_block(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    if dv.selected().iter().all(|r| matches!(r, Ref::Conn(_))) {
        return;
    }
    sub(ui, tk, &t0("xf.title"));
    ui.checkbox(&mut dv.forms.xf_keep, t0("xf.copy"));
    let keep = if dv.forms.xf_keep { ":copy" } else { "" };
    ui.label(RichText::new(t0("xf.move.sub")).color(tk.text_dim));
    ui.horizontal(|ui| {
        for (k, v) in dv.forms.xf_off.iter_mut().enumerate() {
            let l = ui.label(["x", "y", "z"].get(k).copied().unwrap_or(""));
            form_text(ui, v, 52.0, l.id);
        }
    });
    ui.horizontal(|ui| {
        if ui.button(t0("xf.move.go")).clicked() {
            run(acts, "detail.xf", format!("move:{}{keep}", dv.forms.xf_off.join(",")));
        }
        if ui.button(t0("xf.move.pick")).on_hover_text(t0("xf.move.pick.title")).clicked() {
            run(acts, "detail.tool", "move".into());
        }
    });
    ui.label(RichText::new(t0("xf.rotate.sub")).color(tk.text_dim));
    let mut nodes = vec![(String::new(), t0("xf.rotate.middle"))];
    for gy in grid_labels(dv, "y") {
        for gx in grid_labels(dv, "x") {
            nodes.push((format!("{gy}/{gx}"), format!("{gy}/{gx}")));
        }
    }
    row(ui, tk, &t0("xf.rotate.angle"), |ui, lid| form_text(ui, &mut dv.forms.xf_deg, 60.0, lid));
    let about = dv.forms.xf_about.clone();
    row(ui, tk, &t0("xf.rotate.about"), |ui, lid| {
        if let Some(v) = combo(ui, "dc_xf_about", lid, &about, &nodes, 150.0) {
            dv.forms.xf_about = v;
        }
    });
    if ui.button(t0("xf.rotate.go")).clicked() {
        let at = if dv.forms.xf_about.is_empty() { String::new() } else { format!("@{}", dv.forms.xf_about) };
        run(acts, "detail.xf", format!("rotate:{}{at}{keep}", dv.forms.xf_deg));
    }
    let mut lines: Vec<(String, String)> =
        grid_labels(dv, "y").into_iter().map(|l| (format!("y:{l}"), t("xf.mirror.grid", &[("label", F::S(l))]))).collect();
    lines.extend(grid_labels(dv, "x").into_iter().map(|l| (format!("x:{l}"), t("xf.mirror.grid", &[("label", F::S(l))]))));
    if !lines.is_empty() {
        ui.label(RichText::new(t0("xf.mirror.sub")).color(tk.text_dim));
        let cur = dv.forms.xf_line.clone();
        row(ui, tk, &t0("xf.mirror.line"), |ui, lid| {
            if let Some(v) = combo(ui, "dc_xf_line", lid, &cur, &lines, 150.0) {
                dv.forms.xf_line = v;
            }
        });
        if ui.button(t0("xf.mirror.go")).clicked() {
            run(acts, "detail.xf", format!("mirror:{}{keep}", dv.forms.xf_line));
        }
    }
    note(ui, tk, &t0("xf.note"));
}

/// The rest of a member's Properties: holes, copes, "Move, rotate, mirror" and the actions.
fn member_extras(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
    let Some(m) = dv.member(id).cloned() else { return };
    let custom = arr(dv.model.state.get("profiles")).to_vec();
    let prof = cc_detail::profiles::get_profile(m.get("profile").unwrap_or(&Value::Null), &custom);
    let face_name = |f: &str| t0(&format!("app.face.{f}"));
    // holes (detailcraft.js holeEditor)
    sub(ui, tk, &t0("app.holes.title"));
    let holes = arr(m.get("holes")).to_vec();
    for h in &holes {
        ui.horizontal(|ui| {
            let g = |k: &str| r1(cc_detail::js::num(h.get(k)));
            ui.label(t(
                "app.holes.item",
                &[("face", F::S(face_name(&string_of(h.get("face"))))), ("d", F::S(g("d"))), ("x", F::S(g("x"))), ("y", F::S(g("y")))],
            ));
            if ui.small_button("×").on_hover_text(t0("app.holes.remove")).clicked() {
                run(acts, "detail.hole", format!("{id}:remove:{}", string_of(h.get("id"))));
            }
        });
    }
    let from_conn = dv.built.get(id).map_or(0, |p| p.holes.iter().filter(|h| h.conn.is_some()).count());
    if from_conn > 0 {
        note(ui, tk, &t("app.holes.from-conn", &[("n", F::N(from_conn as f64))]));
    }
    if holes.is_empty() && from_conn == 0 {
        note(ui, tk, &t0("app.holes.none"));
    }
    let faces = cc_detail::profiles::hole_faces(prof.as_deref());
    if faces.is_empty() {
        note(ui, tk, &t0("app.holes.where"));
    } else {
        let fo: Vec<(String, String)> = faces.iter().map(|f| (f.to_string(), face_name(f))).collect();
        let cur = dv.forms.hole[0].clone();
        row(ui, tk, &t0("app.holes.face"), |ui, lid| {
            if let Some(v) = combo(ui, "dc_hole_face", lid, &cur, &fo, 120.0) {
                dv.forms.hole[0] = v;
            }
        });
        for (i, label) in [(1, "app.holes.x"), (2, "app.holes.y"), (3, "app.holes.d")] {
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(v) = dv.forms.hole.get_mut(i) {
                    form_text(ui, v, 70.0, lid);
                }
            });
        }
        if ui.button(t0("app.holes.add")).clicked() {
            run(acts, "detail.hole", format!("{id}:add:{}", dv.forms.hole.join(",")));
        }
    }
    // copes (detailcraft.js copeEditor): I-shapes only
    if prof.as_ref().is_some_and(|p| p.kind == "I") {
        sub(ui, tk, &t0("app.copes.title"));
        let cuts = arr(m.get("cuts")).to_vec();
        for c in &cuts {
            ui.horizontal(|ui| {
                let g = |k: &str| r1(cc_detail::js::num(c.get(k)));
                ui.label(t(
                    "app.copes.item",
                    &[
                        ("side", F::S(t0(&format!("app.side.{}", string_of(c.get("side")))))),
                        ("end", F::S(t0(&format!("app.word.{}", string_of(c.get("end")))))),
                        ("length", F::S(g("length"))),
                        ("depth", F::S(g("depth"))),
                    ],
                ));
                if ui.small_button("×").on_hover_text(t0("app.copes.remove")).clicked() {
                    run(acts, "detail.cope", format!("{id}:remove:{}", string_of(c.get("id"))));
                }
            });
        }
        if cuts.is_empty() {
            note(ui, tk, &t0("app.copes.none"));
        }
        let ends = vec![("start".to_string(), t0("app.member.start")), ("end".to_string(), t0("app.member.end"))];
        let sides = vec![("top".to_string(), t0("app.side.top")), ("bottom".to_string(), t0("app.side.bottom"))];
        for (i, label, list) in [(0, "app.copes.end", &ends), (1, "app.copes.side", &sides)] {
            let cur = dv.forms.cope.get(i).cloned().unwrap_or_default();
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(v) = combo(ui, ("dc_cope", i), lid, &cur, list, 120.0)
                    && let Some(slot) = dv.forms.cope.get_mut(i)
                {
                    *slot = v;
                }
            });
        }
        for (i, label) in [(2, "app.copes.length"), (3, "app.copes.depth")] {
            row(ui, tk, &t0(label), |ui, lid| {
                if let Some(v) = dv.forms.cope.get_mut(i) {
                    form_text(ui, v, 70.0, lid);
                }
            });
        }
        if ui.button(t0("app.copes.add")).clicked() {
            run(acts, "detail.cope", format!("{id}:add:{}", dv.forms.cope.join(",")));
        }
    }
    xf_block(dv, ui, tk, acts);
    let kind = string_of(m.get("type"));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if kind == "beam" {
            if ui.button(t0("app.btn.endplates")).clicked() {
                run(acts, "detail.act", "endplates".into());
            }
            if ui.button(t0("app.btn.sheartabs")).clicked() {
                run(acts, "detail.act", "sheartabs".into());
            }
        }
        if kind == "column" && ui.button(t0("app.btn.baseplate")).clicked() {
            run(acts, "detail.act", "baseplate".into());
        }
        if ui.button(RichText::new(t0("app.btn.delete")).color(tk.accent)).clicked() {
            run(acts, "detail.act", "delete".into());
        }
    });
}

/// detailcraft.js `partLabel(id)`.
fn part_label(dv: &DetailView, id: &str) -> String {
    match dv.member(id) {
        Some(m) => format!(
            "{} {} ({})",
            string_of(m.get("profile")),
            dv.marks.part_mark(id).unwrap_or(""),
            cc_detail::model::node_name(&dv.model.state, cc_detail::js::p3_of(m.get("start")))
        ),
        None => t0("app.conn.missing"),
    }
}

/// detailcraft.js `connProps(c)`.
fn conn_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
    let Some(c) = arr(dv.model.state.get("connections")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)).cloned() else {
        return;
    };
    let kind = string_of(c.get("type"));
    ui.label(RichText::new(t0(&format!("app.props.{kind}"))).strong().color(tk.text_strong));
    ui.add_space(4.0);
    let s = |k: &str| string_of(c.get(k));
    let end_word = |e: &str| {
        let k = format!("app.word.{e}");
        if cc_detail::i18n::has(&k) { t0(&k) } else { e.to_string() }
    };
    let joined: Vec<(String, String)> = match kind.as_str() {
        "splice" => vec![(t0("app.k.piece-a"), part_label(dv, &s("a"))), (t0("app.k.piece-b"), part_label(dv, &s("b")))],
        "gusset" => vec![
            (t0("app.k.brace"), part_label(dv, &s("brace"))),
            (t0("app.k.end"), end_word(&s("end"))),
            (t0("app.k.support"), part_label(dv, &s("support"))),
        ],
        "endplate" | "sheartab" => vec![
            (t0("app.k.beam"), part_label(dv, &s("beam"))),
            (t0("app.k.end"), end_word(&s("end"))),
            (t0("app.k.column"), part_label(dv, &s("column"))),
        ],
        _ => vec![(t0("app.k.column"), part_label(dv, &s("column")))],
    };
    kv(ui, tk, "dc_conn_joined", &joined);
    sub(ui, tk, &t0("app.conn.params"));
    let params = c.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, v) in &params {
        let label = param_label(k);
        let help = param_help_key(k).map(t0);
        let set = |acts: &mut Vec<Act>, val: String| run(acts, "detail.set", format!("{id}.{k}={val}"));
        row(ui, tk, &label, |ui, lid| {
            if let Some(choices) = param_choices(k) {
                let opts: Vec<(String, String)> =
                    choices.iter().map(|n| (num_to_string(*n), t0(&format!("app.conn.choice.{k}.{}", num_to_string(*n))))).collect();
                let cur = cc_detail::js::to_js_string(v);
                let r = combo(ui, ("dc_conn_choice", k), lid, &cur, &opts, 150.0);
                if let Some(val) = r {
                    set(acts, val);
                }
            } else {
                let cur = cc_detail::js::to_js_string(v);
                if let Some(val) = text_field(dv, ui, &format!("{id}.{k}"), &cur, 90.0, lid) {
                    set(acts, val);
                }
            }
        });
        if let Some(h) = help {
            ui.label(RichText::new(h).small().color(tk.text_dim)).on_hover_text(label);
        }
    }
    let cc = cc_detail::state::Conn::from_value(&c);
    if kind == "baseplate" && cc.p("washers") != 0.0 && !cc.p("washers").is_nan() && !(cc.p("washerSize") > 0.0 && cc.p("washerT") > 0.0) {
        let txt = match cc_detail::connections::dg1_row(cc.p("rodD")) {
            Some(r) => t(
                "app.conn.washer-dg1",
                &[
                    ("size", F::S(r1(if cc.p("washerSize") > 0.0 { cc.p("washerSize") } else { r.washer }))),
                    ("t", F::S(r1(if cc.p("washerT") > 0.0 { cc.p("washerT") } else { r.washer_t }))),
                    ("rod", F::S(r1(r.rod))),
                ],
            ),
            None => t0("conn.warn.washer-size"),
        };
        note(ui, tk, &txt);
    }
    // bolts by length, the weld, the plate
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut groups: Vec<(String, Vec<&cc_detail::connections::Bolt>)> = Vec::new();
    for b in dv.built.bolts.iter().filter(|b| b.conn.as_deref() == Some(id)) {
        let k = format!("{}|{}", num_to_string(b.length), r1(b.grip));
        match groups.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(b),
            None => groups.push((k, vec![b])),
        }
    }
    for (_, g) in &groups {
        let Some(b0) = g.first() else { continue };
        rows.push((
            t0(if kind != "baseplate" { "app.k.bolts" } else { "app.k.anchor-rods" }),
            t(
                "app.conn.bolts",
                &[
                    ("n", F::N(g.len() as f64)),
                    ("d", F::N(b0.d)),
                    ("length", F::N(b0.length)),
                    ("grade", F::S(b0.grade.clone())),
                    ("grip", F::S(r1(b0.grip))),
                ],
            ),
        ));
    }
    if let Some(w) = dv.built.welds.iter().find(|w| w.conn.as_deref() == Some(id)) {
        rows.push((
            t0("app.k.weld"),
            t("app.conn.weld", &[("size", F::N(w.size)), ("where", F::S(w.r#where.to_lowercase())), ("length", F::S(mm(w.length)))]),
        ));
    }
    let plate = dv.built.get(&format!("{id}.plate"));
    if let Some(p) = plate {
        rows.push((t0("app.k.plate"), format!("{} x {} — {}", p.profile_name, r1(p.length), dv.marks.part_mark(&p.id).unwrap_or(""))));
    }
    if !rows.is_empty() {
        ui.add_space(4.0);
        kv(ui, tk, "dc_conn_info", &rows);
    }
    note(ui, tk, &t0("app.conn.note"));
    for p in dv.built.problems.iter().filter(|x| x.r#ref.as_deref() == Some(id)) {
        ui.label(RichText::new(&p.text).color(if p.hard == Some(true) { tk.accent } else { tk.text_dim }));
    }
    let n = dv.similar_count(&c);
    let shop = plate.is_some() && cc_detail::shop::shop_refusal(plate).is_none();
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(n > 0, egui::Button::new(t("app.conn.similar", &[("n", F::N(n as f64))]))).clicked() {
            run(acts, "detail.act", "similar".into());
        }
        if shop && ui.button(t0("app.btn.plate-drawing")).clicked() {
            run(acts, "detail.export", "shop".into());
        }
        if ui.button(RichText::new(t0("app.btn.delete")).color(tk.accent)).clicked() {
            run(acts, "detail.act", "delete".into());
        }
    });
}

/// detailcraft.js `footingProps(f)`.
fn footing_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
    let Some(f) = arr(dv.model.state.get("footings")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)).cloned() else {
        return;
    };
    let mark = dv.footing_marks.iter().find(|(k, _)| k == id).map_or("", |(_, v)| v.as_str()).to_string();
    ui.label(RichText::new(t("app.props.footing", &[("mark", F::S(mark))])).strong().color(tk.text_strong));
    ui.add_space(4.0);
    let ph = cc_detail::js::or(cc_detail::js::f(f.get("phase")), 1.0);
    row(ui, tk, &t0("phase.prop"), |ui, lid| {
        let opts = phase_options(dv);
        if let Some(v) = combo(ui, "dc_foot_phase", lid, &num_to_string(ph), &opts, 150.0) {
            run(acts, "detail.set", format!("{id}.phase={v}"));
        }
    });
    footing_form(dv, ui, tk, acts, &f, Some(id));
    sub(ui, tk, &t0("app.bars.title"));
    let list: Vec<(String, String, String, String)> = dv
        .schedule
        .by_footing
        .iter()
        .filter(|(fid, _)| fid == id)
        .flat_map(|(_, rows)| rows.iter())
        .map(|(g, mark)| {
            (
                mark.clone(),
                format!("{} {}", cc_detail::rebar::bar_label(&g.bar, &dv.settings.bar_names), g.role.to_lowercase()),
                g.count.to_string(),
                mm(g.length),
            )
        })
        .collect();
    egui::Grid::new("dc_foot_bars").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
        for h in ["app.bars.mark", "app.bars.bars", "app.bars.no", "app.bars.length"] {
            ui.label(RichText::new(t0(h)).strong().size(11.0));
        }
        ui.end_row();
        for (a, b, c, d) in &list {
            ui.label(a);
            ui.label(b);
            ui.label(c);
            ui.label(d);
            ui.end_row();
        }
    });
    let typed = cc_detail::rebar::Footing::from_value(&f);
    let bs = cc_detail::rebar::bar(&typed.mat.bar_x).is_some_and(|b| b.set == "bs");
    note(
        ui,
        tk,
        &t(
            if bs { "app.bars.note.bs" } else { "app.bars.note" },
            &[
                ("m3", F::S(format!("{:.2}", cc_detail::rebar::footing_volume(&typed)))),
                ("source", F::S(if bs { cc_detail::rebar::BAR_SOURCE_BS } else { cc_detail::rebar::BAR_SOURCE }.into())),
            ],
        ),
    );
    if let Some(ft) = dv.built.footings.iter().find(|x| x.f.id == id) {
        for p in &ft.problems {
            ui.label(RichText::new(&p.text).color(tk.text_dim));
        }
    }
    xf_block(dv, ui, tk, acts);
    if ui.button(RichText::new(t0("app.btn.delete")).color(tk.accent)).clicked() {
        run(acts, "detail.act", "delete".into());
    }
}

/// detailcraft.js `stairProps(x)`.
fn stair_props(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>, id: &str) {
    let Some(x) = arr(dv.model.state.get("stairs")).iter().find(|v| v.get("id").and_then(Value::as_str) == Some(id)).cloned() else {
        return;
    };
    let mark = dv.marks.asm_mark(&format!("{id}.stringerL")).unwrap_or("").to_string();
    ui.label(RichText::new(t("app.props.stair", &[("mark", F::S(mark))])).strong().color(tk.text_strong));
    ui.add_space(4.0);
    let ph = cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0);
    row(ui, tk, &t0("phase.prop"), |ui, lid| {
        let opts = phase_options(dv);
        if let Some(v) = combo(ui, "dc_stair_phase", lid, &num_to_string(ph), &opts, 150.0) {
            run(acts, "detail.set", format!("{id}.phase={v}"));
        }
    });
    stair_form(dv, ui, tk, acts, &x, Some(id));
    let custom = arr(dv.model.state.get("profiles")).to_vec();
    let g = cc_detail::stair::stair_geometry(&cc_detail::stair::Stair::from_value(&x), &|nm: &str| {
        cc_detail::profiles::get_profile_str(nm, &custom)
    });
    ui.add_space(4.0);
    kv(
        ui,
        tk,
        "dc_stair_geo",
        &[
            (t0("app.stair.risers"), t("app.stair.risers.v", &[("n", F::N(g.n)), ("r", F::S(r1(g.riser)))])),
            (t0("app.stair.treads"), t("app.stair.treads.v", &[("n", F::N(g.treads)), ("run", F::S(mm(g.run)))])),
            (t0("app.stair.pitch"), format!("{}°", r1(g.pitch))),
            (t0("app.stair.blondel"), mm(g.blondel)),
        ],
    );
    for p in dv.built.problems.iter().filter(|q| q.r#ref.as_deref() == Some(id)) {
        ui.label(RichText::new(&p.text).color(if p.hard == Some(true) { tk.accent } else { tk.text_dim }));
    }
    note(ui, tk, &t0("app.stair.props-note"));
    xf_block(dv, ui, tk, acts);
    if ui.button(RichText::new(t0("app.btn.delete")).color(tk.accent)).clicked() {
        run(acts, "detail.act", "delete".into());
    }
}

/// The left panel's setup sections: grids, levels, phases and custom profiles (detailcraft.js `renderLeft`).
pub fn left_setup(dv: &mut DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let s = dv.model.state.clone();
    egui::CollapsingHeader::new(RichText::new(t0("page.left.grids.title")).strong()).id_salt("dc_grids").default_open(true).show(
        ui,
        |ui| {
            let gx = arr(s.get("grids").and_then(|g| g.get("x")));
            let gy = arr(s.get("grids").and_then(|g| g.get("y")));
            let labels = |l: &[Value]| l.iter().map(|g| string_of(g.get("label"))).collect::<Vec<_>>().join(" ");
            let cur = [labels(gx), cc_detail::model::spacings_of(gx), labels(gy), cc_detail::model::spacings_of(gy)];
            let mut form = dv.forms.grids.clone().unwrap_or_else(|| cur.clone());
            let mut touched = false;
            for (i, label) in
                ["page.left.grids.numbered", "page.left.grids.spacings", "page.left.grids.lettered", "page.left.grids.spacings"]
                    .iter()
                    .enumerate()
            {
                row(ui, tk, &t0(label), |ui, lid| {
                    if let Some(v) = form.get_mut(i)
                        && ui.add(egui::TextEdit::singleline(v).desired_width(130.0)).labelled_by(lid).changed()
                    {
                        touched = true;
                    }
                });
            }
            if touched || dv.forms.grids.is_some() {
                dv.forms.grids = Some(form.clone());
            }
            if ui.button(t0("page.left.grids.apply")).clicked() {
                run(acts, "detail.grids", form.join("|"));
                dv.forms.grids = None;
            }
            note(
                ui,
                tk,
                &format!(
                    "{} 1 2 3 {} 6000 6000. {}",
                    t0("page.left.grids.note.a"),
                    t0("page.left.grids.note.and"),
                    t0("page.left.grids.note.b")
                ),
            );
        },
    );
    egui::CollapsingHeader::new(RichText::new(t0("page.left.levels.title")).strong()).id_salt("dc_levels").default_open(true).show(
        ui,
        |ui| {
            let cur: Vec<(String, String)> =
                arr(s.get("levels")).iter().map(|l| (string_of(l.get("name")), num_to_string(cc_detail::js::num(l.get("z"))))).collect();
            let mut rows = dv.forms.levels.clone().unwrap_or_else(|| cur.clone());
            let mut remove = None;
            let mut touched = false;
            for (i, (n, z)) in rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let l = ui.label(RichText::new((i + 1).to_string()).color(tk.text_dim)).on_hover_text(t0("app.levels.name"));
                    touched |= ui
                        .add(egui::TextEdit::singleline(n).desired_width(104.0))
                        .labelled_by(l.id)
                        .on_hover_text(t0("app.levels.name"))
                        .changed();
                    touched |= ui
                        .add(egui::TextEdit::singleline(z).desired_width(64.0))
                        .labelled_by(l.id)
                        .on_hover_text(t0("app.levels.z"))
                        .changed();
                    if ui.small_button("×").on_hover_text(t("app.levels.remove", &[("n", F::N((i + 1) as f64))])).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                rows.remove(i);
                touched = true;
            }
            ui.horizontal(|ui| {
                if ui.link(format!("+ {}", t0("page.left.levels.add"))).clicked() {
                    let zs: Vec<f64> =
                        rows.iter().map(|r| cc_detail::js::str_to_number(&r.1)).map(|z| if z.is_finite() { z } else { 0.0 }).collect();
                    let z = if zs.is_empty() { 0.0 } else { zs.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 4000.0 };
                    rows.push((t("app.levels.default", &[("n", F::N((zs.len() + 1) as f64))]), num_to_string(z)));
                    touched = true;
                }
                if ui.button(t0("page.left.levels.apply")).clicked() {
                    let spec: Vec<String> = rows.iter().map(|(n, z)| format!("{n}={z}")).collect();
                    run(acts, "detail.levels", spec.join(";"));
                    dv.forms.levels = None;
                    touched = false;
                }
            });
            if touched {
                dv.forms.levels = Some(rows);
            }
        },
    );
    egui::CollapsingHeader::new(RichText::new(t0("phase.title")).strong()).id_salt("dc_phases").default_open(true).show(ui, |ui| {
        let count = |n: f64| -> usize {
            let of = |k: &str| arr(s.get(k)).iter().filter(|x| cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0) == n).count();
            of("members") + of("footings")
        };
        for p in arr(s.get("phases")) {
            let n = cc_detail::js::num(p.get("n"));
            let nm = string_of(p.get("name"));
            ui.horizontal(|ui| {
                let mut on = !dv.hidden_phases.contains(&n);
                if ui.checkbox(&mut on, "").on_hover_text(t("phase.show", &[("phase", F::S(dv.phase_name(n)))])).changed() {
                    run(acts, "detail.phase", num_to_string(n));
                }
                // the phase's number names its name field (phase.name)
                let l = ui.label(RichText::new(num_to_string(n)).monospace().strong()).on_hover_text(t("phase.name", &[("n", F::N(n))]));
                if let Some(v) = text_field(dv, ui, &format!("phase.{n}"), &nm, 100.0, l.id) {
                    run(acts, "detail.phases", format!("rename:{}={v}", num_to_string(n)));
                }
                ui.label(RichText::new(t("phase.count", &[("n", F::N(count(n) as f64))])).color(tk.text_dim));
                if ui.small_button("×").on_hover_text(t("phase.remove", &[("phase", F::S(dv.phase_name(n)))])).clicked() {
                    run(acts, "detail.phases", format!("remove:{}", num_to_string(n)));
                }
            });
        }
        if ui.link(format!("+ {}", t0("phase.add"))).clicked() {
            run(acts, "detail.phases", "add".into());
        }
        let opts = phase_options(dv);
        row(ui, tk, &t0("phase.current"), |ui, lid| {
            if let Some(v) = combo(ui, "dc_phase_cur", lid, &num_to_string(dv.tools.cur_phase), &opts, 130.0) {
                run(acts, "detail.phases", format!("current:{v}"));
            }
        });
        let mut per = s.get("numbering").and_then(|n| n.get("perPhase")) != Some(&Value::Bool(false));
        if ui.checkbox(&mut per, t0("phase.per")).changed() {
            run(acts, "detail.phases", format!("per:{per}"));
        }
        note(ui, tk, &t0("phase.per.hint"));
    });
    egui::CollapsingHeader::new(RichText::new(t0("page.left.profile.title")).strong()).id_salt("dc_profile").default_open(false).show(
        ui,
        |ui| {
            if dv.forms.pkind.is_empty() {
                dv.forms.pkind = "I".into();
            }
            row(ui, tk, &t0("page.left.profile.name"), |ui, lid| form_text(ui, &mut dv.forms.pname, 120.0, lid));
            let kinds: Vec<(String, String)> = [("I", "i"), ("C", "c"), ("L", "l"), ("RHS", "rhs"), ("CHS", "chs"), ("FLAT", "flat")]
                .iter()
                .map(|(k, l)| (k.to_string(), t0(&format!("page.left.profile.shape.{l}"))))
                .collect();
            let cur = dv.forms.pkind.clone();
            row(ui, tk, &t0("page.left.profile.shape"), |ui, lid| {
                if let Some(v) = combo(ui, "dc_pkind", lid, &cur, &kinds, 130.0) {
                    dv.forms.pkind = v;
                }
            });
            let keys = cc_detail::profiles::custom_kind_keys(&dv.forms.pkind).unwrap_or(&[]);
            dv.forms.pdims.retain(|(k, _)| keys.contains(&k.as_str()));
            for k in keys {
                if !dv.forms.pdims.iter().any(|(a, _)| a == k) {
                    dv.forms.pdims.push((k.to_string(), String::new()));
                }
                let label = t0(&format!(
                    "app.dim.{}",
                    match *k {
                        "d" => "depth",
                        "bf" => "flange-width",
                        "tw" => "web",
                        "tf" => "flange",
                        "r" => "fillet",
                        "b" => "width",
                        "t" => "thickness",
                        "h" => "height",
                        _ => "diameter",
                    }
                ));
                row(ui, tk, &label, |ui, lid| {
                    if let Some((_, v)) = dv.forms.pdims.iter_mut().find(|(a, _)| a == k) {
                        form_text(ui, v, 70.0, lid);
                    }
                });
            }
            if ui.button(t0("page.left.profile.add")).clicked() {
                let dims: Vec<String> = dv.forms.pdims.iter().map(|(k, v)| format!("{k}={v}")).collect();
                run(acts, "detail.profile", format!("{};{};{}", dv.forms.pname, dv.forms.pkind, dims.join(";")));
            }
            for p in arr(s.get("profiles")) {
                let kind = string_of(p.get("kind"));
                let key = format!("app.kind.{}", kind.to_lowercase());
                ui.horizontal(|ui| {
                    ui.label(string_of(p.get("name")));
                    ui.label(RichText::new(if cc_detail::i18n::has(&key) { t0(&key) } else { kind.clone() }).color(tk.text_dim));
                });
            }
        },
    );
    ui.add_space(6.0);
}

/// The toolbar's modelling groups: Place (the tools), Connect, Edit (detailcraft.js `renderBar`'s enabled states).
pub fn tool_bar(dv: &DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let tool = dv.tools.tool;
    for (t_id, key, letter) in [
        ("select", "page.tool.select", "V"),
        ("beam", "page.tool.beam", "B"),
        ("column", "page.tool.column", "C"),
        ("brace", "page.tool.brace", "R"),
        ("plate", "page.tool.plate", "P"),
        ("footing", "page.tool.footing", "F"),
        ("stair", "page.tool.stair", "S"),
    ] {
        if ui.selectable_label(tool == t_id, t0(key)).on_hover_text(t(&format!("{key}.title"), &[("key", F::S(letter.into()))])).clicked() {
            run(acts, "detail.tool", t_id.into());
        }
    }
    ui.separator();
    let picked = dv.selected();
    let sel_type = match &dv.sel {
        Some(Ref::Member(id)) => dv.member(id).map(|m| string_of(m.get("type"))).unwrap_or_default(),
        _ => String::new(),
    };
    let has_type = |tys: &[&str]| {
        picked.iter().any(|r| match r {
            Ref::Member(i) => dv.member(i).is_some_and(|m| tys.contains(&m.get("type").and_then(Value::as_str).unwrap_or(""))),
            _ => false,
        })
    };
    let btn =
        |ui: &mut Ui, on: bool, label: &str, tip: &str| ui.add_enabled(on, egui::Button::new(t0(label))).on_hover_text(t0(tip)).clicked();
    if btn(ui, sel_type == "beam", "page.tool.endplates", "page.tool.endplates.title") {
        run(acts, "detail.act", "endplates".into());
    }
    if btn(ui, sel_type == "column", "page.tool.baseplate", "page.tool.baseplate.title") {
        run(acts, "detail.act", "baseplate".into());
    }
    ui.menu_button(t0("tool.out.more"), |ui| {
        for (a, label, tip, on) in [
            ("sheartabs", "page.tool.sheartabs", "page.tool.sheartabs.title", sel_type == "beam"),
            ("splice", "page.tool.splice", "page.tool.splice.title", has_type(&["beam", "column"])),
            ("gusset", "page.tool.gusset", "page.tool.gusset.title", has_type(&["brace"])),
            ("similar", "page.tool.similar", "page.tool.similar.title", matches!(dv.sel, Some(Ref::Conn(_)))),
        ] {
            if btn(ui, on, label, tip) {
                run(acts, "detail.act", a.into());
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(t0("tool.conn.more.title"));
    ui.separator();
    let copyable = picked.iter().any(|r| !matches!(r, Ref::Conn(_)));
    let can_paste = dv.tools.clip.is_some();
    if ui
        .add_enabled(copyable, egui::Button::new(t0("page.tool.copy")))
        .on_hover_text(t("page.tool.copy.title", &[("key", F::S("Ctrl+C".into()))]))
        .clicked()
    {
        run(acts, "detail.act", "copy".into());
    }
    if ui
        .add_enabled(can_paste, egui::Button::new(t0("page.tool.paste")))
        .on_hover_text(t("page.tool.paste.title", &[("key", F::S("Ctrl+V".into()))]))
        .clicked()
    {
        run(acts, "detail.act", "paste".into());
    }
    let any = arr(dv.model.state.get("members")).len() + arr(dv.model.state.get("footings")).len() > 0;
    ui.menu_button(t0("page.edit.more"), |ui| {
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
    })
    .response
    .on_hover_text(t0("page.edit.more.title"));
    let _ = tk;
}

/// The view bar above the stage: 3D / Plan / Elevation, the plan's level or the elevation's grid line, Fit.
pub fn view_bar(dv: &DetailView, ui: &mut Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    use crate::detailview::ViewMode;
    ui.horizontal(|ui| {
        for (m, key, spec) in [
            (ViewMode::D3, "page.view.3d", "3d"),
            (ViewMode::Plan, "page.view.plan", "plan"),
            (ViewMode::Elevation, "page.view.elevation", "elevation"),
        ] {
            if ui.selectable_label(dv.mode == m, t0(key)).clicked() {
                run(acts, "detail.view", spec.into());
            }
        }
        if dv.mode == ViewMode::Plan {
            let lv = opts_of(&level_names(dv));
            let l = ui.label(RichText::new(t0("page.view.level")).color(tk.text_dim));
            if let Some(v) = combo(ui, "dc_vlevel", l.id, &dv.vlevel, &lv, 120.0) {
                run(acts, "detail.view", format!("plan:{v}"));
            }
        }
        if dv.mode == ViewMode::Elevation {
            let gy: Vec<(String, String)> =
                grid_labels(dv, "y").into_iter().map(|g| (g.clone(), t("app.view.grid", &[("label", F::S(g))]))).collect();
            let l = ui.label(RichText::new(t0("page.view.grid")).color(tk.text_dim));
            if let Some(v) = combo(ui, "dc_vgrid", l.id, &dv.vgrid, &gy, 120.0) {
                run(acts, "detail.view", format!("elevation:{v}"));
            }
        }
        if ui.button(t0("page.view.fit")).on_hover_text(t0("page.view.fit.title")).clicked() {
            acts.push(Act::Fit);
        }
    });
}

/// The Settings window's DetailCraft panel (settings.inc).
pub fn settings_window(dv: &mut DetailView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<Act>) {
    if !dv.settings_open {
        return;
    }
    let mut open = true;
    let s = dv.settings.clone();
    let set = |acts: &mut Vec<Act>, kv: String| run(acts, "detail.setting", kv);
    egui::Window::new(RichText::new(t0("input.cmd.app.settings")).strong())
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                let radio = |ui: &mut Ui, acts: &mut Vec<Act>, key: &str, cur: &str, opts: &[(&str, String)]| {
                    ui.horizontal_wrapped(|ui| {
                        for (v, l) in opts {
                            if ui.radio(cur == *v, l).clicked() && cur != *v {
                                set(acts, format!("{key}={v}"));
                            }
                        }
                    });
                };
                sub(ui, tk, &t0("settings.steel.legend"));
                radio(ui, acts, "steel", &s.steel, &[("us", t0("settings.steel.us")), ("eu", t0("settings.steel.eu"))]);
                note(ui, tk, &t0("settings.steel.hint"));
                sub(ui, tk, &t0("settings.offset.legend"));
                ui.horizontal(|ui| {
                    let l = ui.label(t0("settings.offset.label"));
                    for k in 0..3 {
                        let cur = num_to_string(s.offset.get(k).copied().unwrap_or(0.0));
                        ui.label(["x", "y", "z"].get(k).copied().unwrap_or(""));
                        if let Some(v) = text_field(dv, ui, &format!("set.off{k}"), &cur, 60.0, l.id) {
                            let mut o = s.offset.map(num_to_string);
                            if let Some(slot) = o.get_mut(k) {
                                *slot = v;
                            }
                            set(acts, format!("offset={}", o.join(",")));
                        }
                    }
                });
                note(ui, tk, &t("settings.offset.hint", &[("key", F::S("Ctrl+D".into()))]));
                let mut conns = s.conns;
                if ui.checkbox(&mut conns, t0("settings.conns.label")).changed() {
                    set(acts, format!("conns={conns}"));
                }
                note(ui, tk, &t0("settings.conns.hint"));
                sub(ui, tk, &t0("settings.view.legend"));
                let mut bolts = s.bolts;
                if ui.checkbox(&mut bolts, t0("settings.bolts.label")).changed() {
                    set(acts, format!("bolts={bolts}"));
                }
                note(ui, tk, &t0("settings.bolts.hint"));
                sub(ui, tk, &t0("set.drag.legend"));
                radio(ui, acts, "dragSel", &s.drag_sel, &[("rect", t0("set.drag.rect")), ("lasso", t0("set.drag.lasso"))]);
                note(ui, tk, &t0("set.drag.hint"));
                sub(ui, tk, &t0("set.paper.legend"));
                let papers: Vec<(String, String)> =
                    cc_detail::paper::PAPER_KEYS.iter().map(|k| (k.to_string(), cc_detail::paper::paper_label(k))).collect();
                row(ui, tk, &t0("set.paper.label"), |ui, lid| {
                    if let Some(v) = combo(ui, "dc_set_paper", lid, &s.paper, &papers, 150.0) {
                        set(acts, format!("paper={v}"));
                    }
                });
                note(ui, tk, &t0("set.paper.hint"));
                sub(ui, tk, &t0("set.kiss.legend"));
                radio(
                    ui,
                    acts,
                    "kiss",
                    &s.kiss,
                    &[("auto", t0("set.kiss.auto")), ("imperial", t0("set.kiss.imperial")), ("metric", t0("set.kiss.metric"))],
                );
                note(ui, tk, &t0("set.kiss.hint"));
                sub(ui, tk, &t0("set.rebar.legend"));
                ui.label(t0("set.rebar.names"));
                radio(
                    ui,
                    acts,
                    "barNames",
                    &s.bar_names,
                    &[("imperial", t0("set.rebar.names.imperial")), ("metric", t0("set.rebar.names.metric"))],
                );
                ui.label(t0("set.rebar.codes"));
                radio(
                    ui,
                    acts,
                    "shapeCodes",
                    &s.shape_codes,
                    &[("dc", t0("app.codes.dc")), ("aci", t0("app.codes.aci")), ("bs8666", t0("app.codes.bs8666"))],
                );
                note(ui, tk, &t0("set.rebar.hint"));
            });
        });
    if !open {
        run(acts, "detail.settings", "close".into());
    }
}

/// A member type's counted name for the left panel's lists.
pub fn kind_title(kind: &str, n: usize) -> String {
    format!("{} ({n})", type_name(kind))
}
