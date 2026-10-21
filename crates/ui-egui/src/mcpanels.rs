//! ModelCraft's side panel as 0.5 lays it out (ui.inc): Entity info (renderInfo), Outliner (renderTree), Components,
//! Colors, Materials, Tags (renderTags), Style with the sun, Scenes, and Check for 3D printing. The panel only reads the
//! model and emits actions; every action runs through [`ModelView::act`] (mcact.rs), so a click and `model.edit=…` do
//! the same thing. Words come from ModelCraft's 0.5 catalog.

use std::collections::HashMap;

use cc_m3d::model::Dim;
use egui::{Color32, RichText, Sense, Stroke};

use crate::mcact::{color_label, color_name, ns, path_str};
use crate::modelcraft::{ModelView, mt};
use crate::theme::Tokens;

/// A one-line text box that hands back what was typed when it loses focus (Enter, Tab or a click elsewhere); Esc puts
/// the old text back. `id` keeps the typed text between frames.
fn text_box(ui: &mut egui::Ui, boxes: &mut HashMap<String, String>, id: &str, current: &str, width: f32) -> Option<String> {
    let buf = boxes.entry(id.to_string()).or_insert_with(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(buf).desired_width(width).id_salt(("mcbox", id)));
    if r.lost_focus() {
        let v = boxes.remove(id).unwrap_or_default();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return None;
        }
        return (v != current).then_some(v);
    }
    if !r.has_focus() {
        boxes.insert(id.to_string(), current.to_string());
    }
    None
}

fn hex_color(hex: &str) -> Color32 {
    match crate::mcmats::rgb(hex) {
        Some((r, g, b)) => Color32::from_rgb(r, g, b),
        None => Color32::GRAY,
    }
}

/// A swatch button: filled with `fill`, ringed when chosen. → clicked.
fn swatch(ui: &mut egui::Ui, fill: Option<Color32>, on: bool, tip: &str, t: &Tokens) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
    match fill {
        Some(c) => {
            ui.painter().rect_filled(r, 2.0, c);
        }
        None => {
            // Default: a diagonal through an empty square
            ui.painter().rect_stroke(r.shrink(1.0), 2.0, Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
            ui.painter().line_segment(
                [r.left_bottom() + egui::vec2(3.0, -3.0), r.right_top() + egui::vec2(-3.0, 3.0)],
                Stroke::new(1.0, t.text_dim),
            );
        }
    }
    if on {
        ui.painter().rect_stroke(r, 2.0, Stroke::new(2.0, t.text_strong), egui::StrokeKind::Outside);
    }
    resp.on_hover_text(tip).clicked()
}

/// Two columns of label / value rows (mc-props).
/// A library material's swatch (three-mats.js swatchPixels: the texture's lower-left corner, 40 px), drawn by code a
/// few a frame as the page draws them when idle; until then (and for materials without a texture) its colour.
fn mat_swatch(ui: &mut egui::Ui, id: &str, color: Color32, on: bool, tip: &str, t: &Tokens) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
    let key = egui::Id::unique(("mc-swatch", id));
    let ctx = ui.ctx().clone();
    let mut tex: Option<egui::TextureHandle> = ctx.data_mut(|d| d.get_temp(key));
    if tex.is_none() && cc_m3d_tools::matlib::tex_spec(id).is_some() {
        let budget = egui::Id::unique(("mc-swatch-budget", ctx.cumulative_pass_nr()));
        let made: u32 = ctx.data_mut(|d| d.get_temp(budget)).unwrap_or(0);
        if made < 3 {
            ctx.data_mut(|d| d.insert_temp(budget, made + 1));
            if let Some(px) = cc_m3d_tools::matlib::swatch_pixels(id, 40) {
                let h = ctx.load_texture(
                    format!("mc-swatch-{id}"),
                    egui::ColorImage::from_rgba_unmultiplied([40, 40], &px),
                    egui::TextureOptions::LINEAR,
                );
                ctx.data_mut(|d| d.insert_temp(key, h.clone()));
                tex = Some(h);
            }
        } else {
            ctx.request_repaint();
        }
    }
    match &tex {
        Some(h) => {
            ui.painter().image(h.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(r, 2.0, color);
        }
    }
    if on {
        ui.painter().rect_stroke(r, 2.0, Stroke::new(2.0, t.text_strong), egui::StrokeKind::Outside);
    }
    resp.on_hover_text(tip).clicked()
}

fn row(ui: &mut egui::Ui, t: &Tokens, k: &str, v: &str) {
    ui.label(RichText::new(k).color(t.text_dim));
    ui.label(v);
    ui.end_row();
}

impl ModelView {
    /// The right-hand panel.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let mut acts: Vec<String> = Vec::new();
        let mut boxes = std::mem::take(&mut self.boxes);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.info_section(ui, t, &mut boxes, &mut acts);
            self.tree_section(ui, t, &mut boxes, &mut acts);
            self.comps_section(ui, t, &mut acts);
            self.paint_sections(ui, t, &mut acts);
            self.tags_section(ui, t, &mut boxes, &mut acts);
            self.style_section(ui, t, &mut acts);
            self.scenes_section(ui, t, &mut boxes, &mut acts);
            self.print_section(ui, t, &mut acts);
        });
        self.boxes = boxes;
        for a in acts {
            if let Err(e) = self.act(&a) {
                self.message = e;
            }
        }
    }

    // ---- Entity info (renderInfo) ----

    fn info_section(&self, ui: &mut egui::Ui, t: &Tokens, boxes: &mut HashMap<String, String>, acts: &mut Vec<String>) {
        let mut notes: Vec<String> = Vec::new();
        let mut buttons: Vec<(String, String)> = Vec::new();
        let mut heading = String::new();
        // the heading names what is shown (infoH); it is known once the rows are laid out, so it shows from the next frame
        let title = self.info_heading.borrow().clone();
        let title = if title.is_empty() { mt("ui.info", &[]) } else { title };
        egui::CollapsingHeader::new(RichText::new(title).color(t.text_strong)).id_salt("mc-info").default_open(true).show(ui, |ui| {
            let r = ui.scope(|ui| {
                egui::Grid::new("mc-props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    heading = self.info_rows(ui, t, boxes, acts, &mut notes, &mut buttons);
                });
            });
            let _ = r;
            for n in &notes {
                ui.label(RichText::new(n).small().color(t.text_dim));
            }
            let sel = &self.tools.host.sel;
            ui.horizontal_wrapped(|ui| {
                for (label, a) in &buttons {
                    if ui.small_button(label).clicked() {
                        acts.push(a.clone());
                    }
                }
                // Edit › More: hide, isolate (0.4 M17)
                if sel.iter().any(|s| matches!(s.t.as_str(), "e" | "f" | "i")) {
                    if ui.small_button(mt("ui.editmenu.hide", &[])).clicked() {
                        acts.push("hide".into());
                    }
                    if ui.small_button(mt("ui.editmenu.isolate", &[])).clicked() {
                        acts.push("isolate".into());
                    }
                }
            });
        });
        if *self.info_heading.borrow() != heading {
            self.info_heading.replace(heading);
            ui.ctx().request_repaint();
        }
    }

    /// renderInfo's rows (in a two-column grid); → the heading (infoH).
    #[allow(clippy::too_many_lines)]
    fn info_rows(
        &self,
        ui: &mut egui::Ui,
        t: &Tokens,
        boxes: &mut HashMap<String, String>,
        acts: &mut Vec<String>,
        notes: &mut Vec<String>,
        buttons: &mut Vec<(String, String)>,
    ) -> String {
        let model = &self.tools.host.model;
        let u = model.units.clone();
        let fl = |m: f64| cc_m3d::units::format_length(m, &u);
        let ctxd = self.ctx_def();
        let Some(def) = model.def(&ctxd) else { return String::new() };
        let sel = &self.tools.host.sel;
        let count = |k: &str| sel.iter().filter(|s| s.t == k).count();
        let (nf, ne, ni, ng, nd, ns_) = (count("f"), count("e"), count("i"), count("g"), count("d"), count("s"));
        let mut heading = String::new();
        let curve = self.sel_curve();
        if sel.is_empty() {
            heading = mt("app.info.model", &[]);
            let (mut faces, mut edges) = (0usize, 0usize);
            model.walk("root", &cc_m3d::vec::IDENT, &mut |d, _, _, _| {
                faces += d.faces.len();
                edges += d.edges.values().filter(|e| !e.soft).count();
                true
            });
            let counts = model.instance_counts();
            let comps: Vec<&cc_m3d::def::Def> = model.defs.values().filter(|d| d.kind == "component").collect();
            let placed: f64 = comps.iter().map(|d| counts.get(&d.id).copied().unwrap_or(0.0)).sum();
            row(ui, t, &mt("app.info.faces", &[]), &faces.to_string());
            row(ui, t, &mt("app.info.edges", &[]), &edges.to_string());
            row(ui, t, &mt("app.info.groups", &[]), &model.defs.values().filter(|d| d.kind == "group").count().to_string());
            row(
                ui,
                t,
                &mt("app.info.components", &[]),
                &mt("app.info.components.n", &[("n", &comps.len().to_string()), ("placed", &ns(placed))]),
            );
            row(ui, t, &mt("app.info.guides", &[]), &model.guides.len().to_string());
            if !model.dims.is_empty() {
                row(ui, t, &mt("app.info.dims", &[]), &model.dims.len().to_string());
            }
            let nh = cc_m3d::model::hidden_count(model);
            if nh > 0.0 {
                row(ui, t, &mt("app.info.hidden", &[]), &ns(nh));
                buttons.push((mt("ui.editmenu.unhide_all", &[]), "unhide-all".into()));
            }
            if let Some(bb) = model.bbox("root", &cc_m3d::vec::IDENT) {
                let s = bb.max.iter().zip(bb.min).map(|(a, b)| fl(a - b)).collect::<Vec<_>>().join(" × ");
                row(ui, t, &mt("app.info.size", &[]), &s);
            }
            if !model.guides.is_empty() {
                buttons.push((mt("app.info.del_guides", &[]), "del-guides".into()));
            }
            if !model.dims.is_empty() {
                buttons.push((mt("app.info.del_dims", &[]), "del-dims".into()));
            }
            let unit = mt(&format!("app.unit.{}", u.replace('-', "_")), &[]);
            notes.push(mt("app.info.note", &[("unit", &unit)]));
            return heading;
        }
        if let Some(ci) = &curve {
            let kind = if ["arc", "circle", "polygon"].contains(&ci.kind.as_str()) { ci.kind.as_str() } else { "curve" };
            heading = mt(&format!("app.info.curve.{kind}"), &[]);
            if ci.round && ci.flat {
                len_box(ui, t, boxes, acts, &u, &mt("app.info.radius", &[]), "radius", ci.radius, "radius:");
            } else if ci.round {
                row(ui, t, &mt("app.info.radius", &[]), &fl(ci.radius));
            }
            row(ui, t, &mt(if ci.kind == "polygon" { "app.info.sides" } else { "app.info.segments" }, &[]), &ns(ci.segs));
            if ci.kind == "arc" && ci.round {
                row(
                    ui,
                    t,
                    &mt("app.info.angle", &[]),
                    &format!("{}°", ns((ci.sweep * 180.0 / std::f64::consts::PI * 100.0).round() / 100.0)),
                );
            }
            row(ui, t, &mt("app.info.length", &[]), &fl(ci.length));
            if ci.kind == "curve"
                && let Some(sp) = def.splines.get(&ci.id)
            {
                row(ui, t, &mt("app.info.points", &[]), &sp.pts.len().to_string());
            }
            if ci.round && !ci.flat {
                notes.push(mt("app.info.radius.fixed", &[]));
            }
            return heading;
        }
        let one = sel.len() == 1;
        if one && nf == 1 {
            heading = mt("app.info.face", &[]);
            if let Some(f) = sel.first().and_then(|s| def.faces.get(s.id)) {
                row(ui, t, &mt("app.info.area", &[]), &cc_m3d::units::format_area(cc_m3d::def::face_area(def, f), &u));
                row(ui, t, &mt("app.info.holes", &[]), &f.loops.len().saturating_sub(1).to_string());
                row(ui, t, &mt("app.info.color", &[]), &color_label(model, f.color.as_deref()));
            }
            buttons.push((mt("app.info.reverse", &[]), "reverse".into()));
        } else if one && ne == 1 {
            heading = mt("app.info.edge", &[]);
            if let Some(e) = sel.first().and_then(|s| def.edges.get(s.id)) {
                len_box(
                    ui,
                    t,
                    boxes,
                    acts,
                    &u,
                    &mt("app.info.length", &[]),
                    "edgelen",
                    cc_m3d::vec::dist(def.v(e.a), def.v(e.b)),
                    "edge-length:",
                );
                notes.push(mt("app.info.edge.note", &[]));
            }
        } else if one && ni == 1 {
            let Some(iid) = sel.first().map(|s| s.id) else { return heading };
            let Some((inst, d)) = def.insts.get(iid).and_then(|i| Some((i, model.def(&i.def)?))) else { return heading };
            let counts = model.instance_counts();
            heading = mt(if d.kind == "component" { "app.info.component" } else { "app.info.group" }, &[]);
            row(ui, t, &mt("app.info.color", &[]), &color_label(model, inst.color.as_deref()));
            // the Tag picker
            ui.label(RichText::new(mt("app.info.tag", &[])).color(t.text_dim));
            tag_picker(ui, model, &[inst.tag.clone().unwrap_or_default()], acts);
            ui.end_row();
            ui.label(RichText::new(mt("app.info.name", &[])).color(t.text_dim));
            if let Some(v) = text_box(ui, boxes, &format!("defname:{}", d.id), &d.name, 140.0) {
                acts.push(format!("info-name:{v}"));
            }
            ui.end_row();
            let n = counts.get(&d.id).copied().unwrap_or(0.0);
            if d.kind == "component" {
                row(ui, t, &mt("app.info.placed", &[]), &mt("app.info.placed.n", &[("n", &ns(n))]));
            }
            let s = cc_m3d::solids::solid_info(d);
            let solid = s.closed && d.insts.is_empty();
            row(ui, t, &mt("app.info.solid", &[]), &mt(if solid { "app.yes" } else { "app.no" }, &[]));
            if solid {
                let m = &inst.m;
                let det = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2]);
                let k = det.abs().cbrt();
                row(ui, t, &mt("app.info.volume", &[]), &cc_m3d::units::format_volume(s.volume.unwrap_or(0.0) * k * k * k, &u));
            }
            if let Some(bb) = model.def_bbox(&d.id) {
                for (q, key) in ["app.info.width", "app.info.depth", "app.info.height"].iter().enumerate() {
                    let size = bb.max.get(q).copied().unwrap_or(0.0) - bb.min.get(q).copied().unwrap_or(0.0);
                    if size > 1e-6 {
                        len_box(ui, t, boxes, acts, &u, &mt(key, &[]), &format!("size{q}"), size, &format!("size:{q}="));
                    } else {
                        row(ui, t, &mt(key, &[]), &fl(0.0));
                    }
                }
            }
            if !solid {
                let p = cc_m3d::solids::solid_problems(d);
                let list = crate::mcact::problem_list(&p);
                let why = if d.faces.is_empty() {
                    mt("app.check.empty", &[])
                } else if !list.is_empty() {
                    mt("app.check.why", &[("list", &list)])
                } else if !d.insts.is_empty() {
                    mt("app.check.nested", &[])
                } else {
                    String::new()
                };
                if !why.is_empty() {
                    notes.push(why);
                }
                if !list.is_empty() {
                    buttons.push((mt("app.check.show", &[]), "check-show".into()));
                    buttons.push((mt("app.check.fix", &[]), "check-fix".into()));
                }
            }
            soften_row(ui, t, boxes, self.soften_deg, acts);
            // 0.4 M26: a component made of parts stretches its parts by their rules, for this copy only
            if d.kind == "component" && (!d.insts.is_empty() || d.dyn_.is_some()) {
                buttons.push((mt("app.dyn.stretch", &[]), format!("stretch:{}", ns(iid))));
            }
            // a part inside an open component: how it follows when the component is stretched
            if !self.tools.host.path.is_empty() && def.kind == "component" {
                part_rules(ui, t, boxes, acts, model, def, iid, &u);
            }
            if let Some(dy) = &d.dyn_ {
                let src = model.def(&dy.src).map(|x| x.name.clone()).unwrap_or_default();
                notes.push(mt("app.dyn.is", &[("name", &src)]));
            }
            buttons.push((mt("app.info.edit", &[]), "edit".into()));
            buttons.push((mt("app.info.explode", &[]), "explode".into()));
            if d.kind == "component" && n > 1.0 {
                buttons.push((mt("app.info.make_unique", &[]), "make-unique".into()));
            }
            if d.kind == "group" {
                buttons.push((mt("app.undo.make_component", &[]), "to-component".into()));
            }
        } else if one && nd == 1 {
            let Some(id) = sel.first().map(|s| s.id) else { return heading };
            let Some(dm) = model.dims.get(id) else { return heading };
            let mut dm = dm.clone();
            match &mut dm {
                Dim::Rad { kind, c, .. } => {
                    let dia = kind == "dia";
                    heading = mt(if dia { "app.info.dim.dia" } else { "app.info.dim.rad" }, &[]);
                    let g = model.radial_at(c);
                    let lab = mt(if dia { "app.info.diameter" } else { "app.info.radius" }, &[]);
                    match &g {
                        Some(g) if g.live && g.flat => {
                            len_box(ui, t, boxes, acts, &u, &lab, "dimlen", if dia { 2.0 * g.r } else { g.r }, "dim-length:")
                        }
                        Some(g) => row(ui, t, &lab, &fl(if dia { 2.0 * g.r } else { g.r })),
                        None => {}
                    }
                    ui.label(RichText::new(mt("app.info.dim.show", &[])).color(t.text_dim));
                    let mut k = kind.clone();
                    egui::ComboBox::new("mc-dimkind", "")
                        .selected_text(mt(if dia { "app.info.diameter" } else { "app.info.radius" }, &[]))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut k, "rad".to_string(), mt("app.info.radius", &[]));
                            ui.selectable_value(&mut k, "dia".to_string(), mt("app.info.diameter", &[]));
                        });
                    if k != *kind {
                        acts.push(format!("dim-kind:{k}"));
                    }
                    ui.end_row();
                    notes.push(mt(
                        match &g {
                            Some(g) if !g.live => "app.info.dim.rad_gone",
                            None => "app.info.dim.rad_gone",
                            Some(g) if g.flat => "app.info.dim.rad_note",
                            _ => "app.info.dim.rad_fixed",
                        },
                        &[],
                    ));
                }
                Dim::Ang { .. } => {
                    heading = mt("app.info.dim.ang", &[]);
                    notes.push(mt("m3d.model.dim_angle_only", &[]));
                }
                Dim::Lin { a, b, mv, .. } => {
                    heading = mt("app.info.dim", &[]);
                    let attached = a.at.is_some() && b.at.is_some();
                    let (pa, pb) = (model.end_at(a), model.end_at(b));
                    len_box(ui, t, boxes, acts, &u, &mt("app.info.length", &[]), "dimlen", cc_m3d::vec::dist(pa, pb), "dim-length:");
                    ui.label(RichText::new(mt("app.info.dim.moves", &[])).color(t.text_dim));
                    let mut k = mv.clone();
                    let word = |x: &str| mt(&format!("app.info.dim.{x}"), &[]);
                    egui::ComboBox::new("mc-dimmove", "").selected_text(word(&k)).show_ui(ui, |ui| {
                        for x in ["b", "a", "both"] {
                            ui.selectable_value(&mut k, x.to_string(), word(x));
                        }
                    });
                    if k != *mv {
                        acts.push(format!("dim-move:{k}"));
                    }
                    ui.end_row();
                    notes.push(mt(if attached { "app.info.dim.note" } else { "app.info.dim.free" }, &[]));
                }
            }
            buttons.push((mt("app.info.delete", &[]), "erase".into()));
        } else if one && ns_ == 1 {
            heading = mt("app.info.section", &[]);
            let Some(id) = sel.first().map(|s| s.id) else { return heading };
            if let Some(sp) = model.sections.get(id) {
                ui.label(RichText::new(mt("app.info.name", &[])).color(t.text_dim));
                if let Some(v) = text_box(ui, boxes, "secname", &sp.name, 140.0) {
                    acts.push(format!("sec-name:{v}"));
                }
                ui.end_row();
                ui.label(RichText::new(mt("app.info.section.cut", &[])).color(t.text_dim));
                let mut on = model.active_section == Some(id);
                if ui.checkbox(&mut on, mt("app.info.section.active", &[])).changed() {
                    acts.push(format!("sec-active:{}", if on { "on" } else { "off" }));
                }
                ui.end_row();
                len_box(ui, t, boxes, acts, &u, &mt("app.info.section.at", &[]), "secat", cc_m3d::vec::dot(sp.p, sp.n), "sec-at:");
                notes.push(mt("app.info.section.note", &[]));
                buttons.push((mt("app.info.section.reverse", &[]), "sec-reverse".into()));
            }
            buttons.push((mt("app.info.delete", &[]), "erase".into()));
        } else if one && ng == 1 {
            let point = sel.first().and_then(|s| model.guides.get(s.id)).is_some_and(|g| matches!(g, cc_m3d::model::Guide::Point { .. }));
            heading = mt(if point { "app.info.guide_point" } else { "app.info.guide_line" }, &[]);
            buttons.push((mt("app.info.delete", &[]), "erase".into()));
        } else {
            heading = mt("app.info.selected", &[("n", &sel.len().to_string())]);
            if nf > 0 {
                row(ui, t, &mt("app.info.faces", &[]), &nf.to_string());
            }
            if ne > 0 {
                row(ui, t, &mt("app.info.edges", &[]), &ne.to_string());
            }
            if ni > 0 {
                row(ui, t, &mt("app.info.groups_components", &[]), &ni.to_string());
                ui.label(RichText::new(mt("app.info.tag", &[])).color(t.text_dim));
                let tags: Vec<String> = sel
                    .iter()
                    .filter(|x| x.t == "i")
                    .filter_map(|x| def.insts.get(x.id))
                    .map(|i| i.tag.clone().unwrap_or_default())
                    .collect();
                tag_picker(ui, model, &tags, acts);
                ui.end_row();
            }
            if ng > 0 {
                row(ui, t, &mt("app.info.guides", &[]), &ng.to_string());
            }
            if nd > 0 {
                row(ui, t, &mt("app.info.dims", &[]), &nd.to_string());
            }
            if nf > 0 {
                let a: f64 =
                    sel.iter().filter(|s| s.t == "f").filter_map(|s| def.faces.get(s.id)).map(|f| cc_m3d::def::face_area(def, f)).sum();
                row(ui, t, &mt("app.info.total_area", &[]), &cc_m3d::units::format_area(a, &u));
            }
            if nf > 0 || ne > 0 {
                soften_row(ui, t, boxes, self.soften_deg, acts);
            }
            buttons.push((mt("app.info.make_component_key", &[]), "make-component".into()));
            buttons.push((mt("app.info.make_group_key", &[]), "make-group".into()));
        }
        heading
    }

    // ---- Outliner (renderTree) ----

    fn tree_section(&self, ui: &mut egui::Ui, t: &Tokens, boxes: &mut HashMap<String, String>, acts: &mut Vec<String>) {
        let model = &self.tools.host.model;
        let Some(root) = model.root() else { return };
        egui::CollapsingHeader::new(RichText::new(mt("ui.outliner", &[])).color(t.text_strong)).default_open(true).show(ui, |ui| {
            let host_path = &self.tools.host.path;
            if !host_path.is_empty() {
                // the crumbs and Close group
                let mut names = Vec::new();
                let mut d = root;
                for &iid in host_path {
                    let Some(next) = d.insts.get(iid).and_then(|i| model.def(&i.def)) else { break };
                    d = next;
                    names.push(d.name.clone());
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(mt("app.tree.crumbs", &[("path", &names.join(" › "))])).small().color(t.text_dim));
                    if ui.small_button(mt("ui.outliner.close", &[])).clicked() {
                        acts.push("close-all".into());
                    }
                });
            }
            if root.insts.len() >= 2 || !self.tree_filter.is_empty() {
                let cur = self.tree_filter.clone();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⌕").color(t.text_dim)).on_hover_text(mt("ui.outliner.search", &[]));
                    let buf = boxes.entry("tree-filter".into()).or_insert_with(|| cur.clone());
                    let r = ui.add(egui::TextEdit::singleline(buf).hint_text(mt("ui.outliner.search", &[])).desired_width(170.0));
                    if r.changed() {
                        acts.push(format!("tree-filter:{buf}"));
                    }
                });
            }
            if root.insts.is_empty() {
                ui.label(RichText::new(mt("ui.outliner.empty", &[("comp", "G"), ("group", "Shift+G")])).small().color(t.text_dim));
                return;
            }
            // drop on the heading row: to the top level
            let top = ui.label(RichText::new(mt("ui.outliner.top_title", &[])).small().color(t.text_dim));
            if let Some(p) = top.dnd_release_payload::<(Vec<f64>, f64)>() {
                acts.push(format!("tree-move:{}:{}>", path_str(&p.0), ns(p.1)));
            }
            let counts = model.instance_counts();
            let off_tags = model.hidden_tags();
            let sel_ids: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "i").map(|s| s.id).collect();
            let filter = self.tree_filter.clone();
            let mut matches: HashMap<String, bool> = HashMap::new();
            let mut renaming = self.renaming.clone();
            self.tree_rows(ui, t, root, &[], 0, &counts, &off_tags, &sel_ids, &filter, &mut matches, boxes, &mut renaming, acts);
            if renaming != self.renaming {
                acts.push(format!("tree-renaming:{}", renaming.unwrap_or_default()));
            }
        });
    }

    /// With a search, does something inside def match (hit())?
    fn tree_hit(&self, def: &cc_m3d::def::Def, filter: &str, depth: u32, memo: &mut HashMap<String, bool>) -> bool {
        if depth > 12 {
            return false;
        }
        if let Some(v) = memo.get(&def.id) {
            return *v;
        }
        let mut any = false;
        for inst in def.insts.values() {
            if let Some(d) = self.tools.host.model.def(&inst.def)
                && (d.name.to_lowercase().contains(filter) || self.tree_hit(d, filter, depth + 1, memo))
            {
                any = true;
            }
        }
        memo.insert(def.id.clone(), any);
        any
    }

    #[allow(clippy::too_many_arguments)]
    fn tree_rows(
        &self,
        ui: &mut egui::Ui,
        t: &Tokens,
        def: &cc_m3d::def::Def,
        path: &[f64],
        depth: u32,
        counts: &cc_m3d::omap::OMap<String, f64>,
        off_tags: &std::collections::HashSet<String>,
        sel_ids: &[f64],
        filter: &str,
        memo: &mut HashMap<String, bool>,
        boxes: &mut HashMap<String, String>,
        renaming: &mut Option<String>,
        acts: &mut Vec<String>,
    ) {
        if depth > 12 {
            return;
        }
        let host_path = &self.tools.host.path;
        for (iid, inst) in def.insts.iter() {
            let Some(d) = self.tools.host.model.def(&inst.def) else { continue };
            let own = filter.is_empty() || d.name.to_lowercase().contains(filter);
            if !filter.is_empty() && !own && !self.tree_hit(d, filter, 0, memo) {
                continue;
            }
            let mut p = path.to_vec();
            p.push(iid);
            let open = host_path.len() >= p.len() && p.iter().zip(host_path).all(|(a, b)| a == b);
            let is_sel = sel_ids.contains(&iid) && path == host_path.as_slice();
            let by_tag = inst.tag.as_deref().is_some_and(|g| off_tags.contains(g));
            let off = inst.hidden || by_tag;
            let n = counts.get(&d.id).copied().unwrap_or(0.0);
            let name = if d.kind == "component" && n > 1.0 { format!("{} ({})", d.name, ns(n)) } else { d.name.clone() };
            let ps = path_str(path);
            ui.horizontal(|ui| {
                ui.add_space(12.0 * depth as f32);
                // the eye: hide or show this placement
                let eye = if inst.hidden { "◌" } else { "●" };
                let tip = mt(if inst.hidden { "app.tree.show" } else { "app.tree.hide" }, &[("name", &d.name)]);
                if ui.add(egui::Button::new(RichText::new(eye).small()).frame(false)).on_hover_text(tip).clicked() {
                    acts.push(format!("tree-eye:{ps}:{}", ns(iid)));
                }
                let icon = if d.kind == "component" { "◆" } else { "▢" };
                if renaming.as_deref() == Some(d.id.as_str()) {
                    ui.label(icon);
                    let r = ui.add(
                        egui::TextEdit::singleline(boxes.entry(format!("rename:{}", d.id)).or_insert_with(|| d.name.clone()))
                            .desired_width(150.0),
                    );
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        let v = boxes.remove(&format!("rename:{}", d.id)).unwrap_or_default();
                        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            acts.push(format!("tree-rename:{}={v}", d.id));
                        }
                        *renaming = None;
                    }
                    return;
                }
                let mut text = RichText::new(format!("{icon} {name}"));
                text = if off {
                    text.color(t.text_dim)
                } else if open {
                    text.color(t.text_strong)
                } else {
                    text.color(t.text)
                };
                if !filter.is_empty() && own {
                    text = text.strong();
                }
                let r =
                    ui.add(egui::Button::selectable(is_sel, text).sense(Sense::click_and_drag())).on_hover_text(mt("app.tree.title", &[]));
                if by_tag && !inst.hidden {
                    ui.label(RichText::new("⌂").small().color(t.text_dim))
                        .on_hover_text(mt("app.tree.hidden_tag", &[("name", inst.tag.as_deref().unwrap_or(""))]));
                }
                r.dnd_set_drag_payload((path.to_vec(), iid));
                if let Some(from) = r.dnd_release_payload::<(Vec<f64>, f64)>()
                    && !(from.1 == iid && from.0 == path)
                {
                    acts.push(format!("tree-move:{}:{}>{}", path_str(&from.0), ns(from.1), path_str(&p)));
                }
                if r.double_clicked() {
                    acts.push(format!("open:{}", path_str(&p)));
                } else if r.clicked() {
                    acts.push(format!("tree-select:{ps}:{}", ns(iid)));
                }
                if (r.hovered() || is_sel) && ui.input(|i| i.key_pressed(egui::Key::F2)) {
                    *renaming = Some(d.id.clone());
                }
                r.context_menu(|ui| {
                    if ui.button(mt("app.scene.rename", &[])).clicked() {
                        *renaming = Some(d.id.clone());
                        ui.close();
                    }
                });
            });
            if !d.insts.is_empty() {
                self.tree_rows(ui, t, d, &p, depth + 1, counts, off_tags, sel_ids, filter, memo, boxes, renaming, acts);
            }
        }
    }

    // ---- Components ----

    fn comps_section(&self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<String>) {
        egui::CollapsingHeader::new(RichText::new(mt("ui.comps", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
            let model = &self.tools.host.model;
            let counts = model.instance_counts();
            let mut any = false;
            for d in model.defs.values().filter(|d| d.kind == "component") {
                any = true;
                ui.horizontal(|ui| {
                    ui.label(&d.name);
                    ui.label(RichText::new(format!("{}×", ns(counts.get(&d.id).copied().unwrap_or(0.0)))).color(t.text_dim));
                    if ui.small_button(mt("app.comp.place", &[])).clicked() {
                        acts.push(format!("place:{}", d.id));
                    }
                });
            }
            if !any {
                ui.label(RichText::new(mt("ui.comps.empty", &[("key", "G")])).small().color(t.text_dim));
            }
        });
    }

    // ---- Colors and Materials ----

    fn paint_sections(&self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<String>) {
        let cur = self.tools.paint_color.clone();
        egui::CollapsingHeader::new(RichText::new(mt("ui.colors", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if swatch(ui, None, cur.is_none(), &mt("app.color.default", &[]), t) {
                    acts.push("paint:default".into());
                }
                for (name, hex) in cc_m3d::paint::PALETTE {
                    let hex: &str = hex;
                    if swatch(ui, Some(hex_color(hex)), cur.as_deref() == Some(hex), &color_name(name), t) {
                        acts.push(format!("paint:{hex}"));
                    }
                }
                // Another color
                let custom = cur.as_deref().filter(|c| cc_m3d::paint::is_color(c) && !cc_m3d::paint::PALETTE.iter().any(|(_, h)| h == c));
                let mut rgb = custom.and_then(crate::mcmats::rgb).map_or([0x5a, 0x8f, 0x6a], |(r, g, b)| [r, g, b]);
                let r = ui.color_edit_button_srgb(&mut rgb).on_hover_text(mt("app.color.other", &[]));
                if r.changed() {
                    acts.push(format!("paint:#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
                }
            });
            ui.label(RichText::new(mt("ui.colors.note", &[])).small().color(t.text_dim));
        });
        egui::CollapsingHeader::new(RichText::new(mt("ui.materials", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
            for cat in crate::mcmats::CATEGORIES {
                let list: Vec<&crate::mcmats::Mat> = crate::mcmats::MATERIALS.iter().filter(|m| m.cat == *cat).collect();
                if list.is_empty() {
                    continue;
                }
                ui.label(RichText::new(mt(&format!("render.matcat.{cat}"), &[])).small().color(t.text_dim));
                ui.horizontal_wrapped(|ui| {
                    for m in list {
                        let key = format!("mat:{}", m.id);
                        if mat_swatch(
                            ui,
                            m.id,
                            hex_color(m.color),
                            cur.as_deref() == Some(key.as_str()),
                            &mt(&format!("render.mat.{}", m.id), &[]),
                            t,
                        ) {
                            acts.push(format!("paint:{key}"));
                        }
                    }
                });
            }
            ui.label(RichText::new(mt("ui.materials.note", &[])).small().color(t.text_dim));
        });
    }

    // ---- Tags (renderTags) ----

    fn tags_section(&self, ui: &mut egui::Ui, t: &Tokens, boxes: &mut HashMap<String, String>, acts: &mut Vec<String>) {
        egui::CollapsingHeader::new(RichText::new(mt("ui.tags", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
            let model = &self.tools.host.model;
            let counts = model.tag_counts();
            let n_of = |k: &str| counts.iter().find(|c| c.0 == k).map_or(0.0, |c| c.1);
            egui::Grid::new("mc-tags").num_columns(4).spacing([6.0, 3.0]).show(ui, |ui| {
                ui.label(RichText::new("●").small().color(t.text_dim));
                ui.label(mt("app.tag.untagged", &[]));
                ui.label(RichText::new(ns(n_of(""))).color(t.text_dim));
                ui.label("");
                ui.end_row();
                for g in &model.tags {
                    let on = !g.hidden;
                    let tip = mt(if on { "app.tag.hide" } else { "app.tag.show" }, &[("name", &g.name)]);
                    if ui
                        .add(egui::Button::new(RichText::new(if on { "●" } else { "◌" }).small()).frame(false))
                        .on_hover_text(tip)
                        .clicked()
                    {
                        acts.push(format!("{}:{}", if on { "tag-hide" } else { "tag-show" }, g.name));
                    }
                    if let Some(v) = text_box(ui, boxes, &format!("tag:{}", g.name), &g.name, 120.0) {
                        acts.push(format!("tag-rename:{}={v}", g.name));
                    }
                    ui.label(RichText::new(ns(n_of(&g.name))).color(t.text_dim));
                    if ui.small_button("🗑").on_hover_text(mt("app.tag.delete", &[("name", &g.name)])).clicked() {
                        acts.push(format!("tag-delete:{}", g.name));
                    }
                    ui.end_row();
                }
            });
            if ui.small_button(format!("+ {}", mt("ui.tags.add", &[]))).clicked() {
                acts.push("tag-add".into());
            }
            ui.label(RichText::new(mt("ui.tags.note", &[])).small().color(t.text_dim));
        });
    }

    // ---- Style and the sun ----

    fn style_section(&self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<String>) {
        egui::CollapsingHeader::new(RichText::new(mt("ui.style", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
            egui::Grid::new("mc-style").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new(mt("ui.style.faces", &[])).color(t.text_dim));
                let mut mode = self.mode.clone();
                let word = |m: &str| mt(&format!("ui.style.mode.{m}"), &[]);
                egui::ComboBox::new("mc-mode", "").selected_text(word(&mode)).show_ui(ui, |ui| {
                    for m in ["shaded", "hidden", "wire", "xray"] {
                        ui.selectable_value(&mut mode, m.to_string(), word(m));
                    }
                });
                if mode != self.mode {
                    acts.push(format!("style:{mode}"));
                }
                ui.end_row();
                ui.label(RichText::new(mt("ui.style.edges", &[])).color(t.text_dim));
                let mut e = self.edges;
                if ui.checkbox(&mut e, mt("ui.style.edges.show", &[])).changed() {
                    acts.push(format!("edges:{}", if e { "on" } else { "off" }));
                }
                ui.end_row();
                ui.label(RichText::new(mt("ui.style.shadows", &[])).color(t.text_dim));
                let mut s = self.shadows;
                if ui.checkbox(&mut s, mt("ui.style.shadows.sun", &[])).changed() {
                    acts.push(format!("shadows:{}", if s { "on" } else { "off" }));
                }
                ui.end_row();
            });
            // the sun (only with shadows on, as the fieldset is disabled otherwise)
            ui.add_enabled_ui(self.shadows, |ui| {
                let mut sun = self.sun;
                egui::Grid::new("mc-sun").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new(mt("ui.sun.date", &[])).color(t.text_dim));
                    ui.horizontal(|ui| {
                        let mut month = sun[0] as i32;
                        egui::ComboBox::new("mc-sun-month", "").selected_text(mt(&format!("ui.sun.month.{month}"), &[])).show_ui(
                            ui,
                            |ui| {
                                for k in 1..=12 {
                                    ui.selectable_value(&mut month, k, mt(&format!("ui.sun.month.{k}"), &[]));
                                }
                            },
                        );
                        sun[0] = f64::from(month);
                        ui.add(egui::DragValue::new(&mut sun[1]).range(1.0..=31.0).speed(0.2).max_decimals(0));
                    });
                    ui.end_row();
                    ui.label(RichText::new(mt("ui.sun.time", &[])).color(t.text_dim));
                    let h = sun[2].floor();
                    let mi = ((sun[2] - h) * 60.0).round();
                    ui.add(egui::Slider::new(&mut sun[2], 0.0..=23.75).step_by(0.25).show_value(false).text(format!("{h}:{mi:02}")));
                    ui.end_row();
                    for (k, key, lo, hi) in
                        [(3, "ui.sun.lat", -90.0, 90.0), (4, "ui.sun.lon", -180.0, 180.0), (5, "ui.sun.tz", -14.0, 14.0)]
                    {
                        ui.label(RichText::new(mt(key, &[])).color(t.text_dim));
                        if let Some(x) = sun.get_mut(k) {
                            ui.add(egui::DragValue::new(x).range(lo..=hi).speed(0.05).max_decimals(2));
                        }
                        ui.end_row();
                    }
                });
                if sun != self.sun {
                    acts.push(format!("sun:{}", sun.iter().map(|x| ns(*x)).collect::<Vec<_>>().join(",")));
                }
                let w = cc_m3d_tools::sun::When {
                    month: sun[0],
                    day: sun[1],
                    hour: sun[2],
                    lat: sun[3],
                    lon: sun[4],
                    tz: sun[5],
                    year: 2025.0,
                };
                let s = cc_m3d_tools::sun::sun_position(&w);
                let info = if s.up {
                    mt("app.sun.info", &[("alt", &format!("{:.0}", s.altitude)), ("az", &format!("{:.0}", s.azimuth))])
                } else {
                    mt("app.sun.down", &[])
                };
                ui.label(RichText::new(info).small().color(t.text_dim));
            });
        });
    }

    // ---- Scenes ----

    fn scenes_section(&self, ui: &mut egui::Ui, t: &Tokens, boxes: &mut HashMap<String, String>, acts: &mut Vec<String>) {
        egui::CollapsingHeader::new(RichText::new(mt("ui.scenes", &[])).color(t.text_strong)).default_open(true).show(ui, |ui| {
            let scenes = &self.tools.host.model.scenes;
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(format!("+ {}", mt("ui.scenes.add", &[]))).clicked() {
                    acts.push("scene-add".into());
                }
                if !scenes.is_empty() {
                    if ui.small_button(format!("‹ {}", mt("ui.scenes.prev", &[]))).clicked() {
                        acts.push("scene-prev".into());
                    }
                    if ui.small_button(format!("{} ›", mt("ui.scenes.next", &[]))).clicked() {
                        acts.push("scene-next".into());
                    }
                }
            });
            if scenes.is_empty() {
                ui.label(RichText::new(mt("ui.scenes.empty", &[("up", "Page Up"), ("down", "Page Down")])).small().color(t.text_dim));
            }
            for (i, sc) in scenes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let key = format!("scene:{i}");
                    if boxes.contains_key(&key) {
                        let r = ui.add(egui::TextEdit::singleline(boxes.entry(key.clone()).or_default()).desired_width(140.0));
                        if !r.has_focus() && !r.lost_focus() {
                            r.request_focus();
                        }
                        if r.lost_focus() {
                            let v = boxes.remove(&key).unwrap_or_default();
                            if !ui.input(|x| x.key_pressed(egui::Key::Escape)) {
                                acts.push(format!("scene-rename:{i}={v}"));
                            }
                        }
                        return;
                    }
                    let cur = i as isize == self.scene_index;
                    if ui.selectable_label(cur, &sc.name).on_hover_text(mt("app.scene.go.title", &[])).clicked() {
                        acts.push(format!("scene-go-anim:{i}"));
                    }
                    if ui.small_button("⟳").on_hover_text(mt("app.scene.update.title", &[("name", &sc.name)])).clicked() {
                        acts.push(format!("scene-update:{i}"));
                    }
                    if ui.small_button("✎").on_hover_text(mt("app.scene.rename", &[])).clicked() {
                        boxes.insert(key, sc.name.clone());
                    }
                    if ui.small_button("🗑").on_hover_text(mt("app.scene.delete", &[])).clicked() {
                        acts.push(format!("scene-delete:{i}"));
                    }
                });
            }
        });
    }

    // ---- Check for 3D printing ----

    fn print_section(&self, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<String>) {
        egui::CollapsingHeader::new(RichText::new(mt("app.print3d.title", &[])).color(t.text_strong))
            .id_salt("mc-print3d")
            .open(self.print_report.as_ref().map(|_| true))
            .default_open(false)
            .show(ui, |ui| {
                if ui.small_button(mt("ui.export.print3d", &[])).clicked() {
                    acts.push("print-check".into());
                }
                let Some(r) = &self.print_report else { return };
                let col = if r.ok { Color32::from_rgb(0x17, 0x9a, 0x4a) } else { Color32::from_rgb(0xd0, 0x22, 0x3e) };
                ui.label(RichText::new(r.verdict()).strong().color(col));
                for l in r.problem_lines() {
                    ui.label(RichText::new(format!("• {l}")).color(t.text));
                }
                for l in r.warning_lines() {
                    ui.label(RichText::new(format!("• {l}")).color(t.text_dim));
                }
                for l in r.fact_lines() {
                    ui.label(RichText::new(l).small().color(t.text_dim));
                }
                if r.fix_hint() {
                    ui.label(RichText::new(mt("app.print3d.fixhint", &[])).small().color(t.text_dim));
                }
                ui.horizontal(|ui| {
                    if r.triangles > 0.0 {
                        if ui.small_button(mt("app.print3d.stl", &[])).clicked() {
                            acts.push("print-stl".into());
                        }
                        if ui.small_button(mt("app.print3d.3mf", &[])).clicked() {
                            acts.push("print-3mf".into());
                        }
                    }
                    if ui.small_button(mt("app.btn.close", &[])).clicked() {
                        acts.push("print-close".into());
                    }
                });
            });
    }
}

/// tagPicker: the selected placements' tag (several different: a "(several)" first choice), Untagged, the tags, New tag….
fn tag_picker(ui: &mut egui::Ui, model: &cc_m3d::model::Model, tags: &[String], acts: &mut Vec<String>) {
    let mut set: Vec<&String> = Vec::new();
    for g in tags {
        if !set.contains(&g) {
            set.push(g);
        }
    }
    let mixed = set.len() > 1;
    let cur = if mixed { None } else { set.first().map(|s| s.as_str()) };
    let shown = match cur {
        None => mt("app.info.tag.mixed", &[]),
        Some("") => mt("app.tag.untagged", &[]),
        Some(n) => n.to_string(),
    };
    egui::ComboBox::new("mc-tag", "").selected_text(shown).show_ui(ui, |ui| {
        if ui.selectable_label(cur == Some(""), mt("app.tag.untagged", &[])).clicked() {
            acts.push("tag-set:".into());
        }
        for g in &model.tags {
            let label = if g.hidden { mt("app.info.tag.hidden", &[("name", &g.name)]) } else { g.name.clone() };
            if ui.selectable_label(cur == Some(g.name.as_str()), label).clicked() {
                acts.push(format!("tag-set:{}", g.name));
            }
        }
        if ui.selectable_label(false, mt("app.info.tag.new", &[])).clicked() {
            acts.push("tag-set-new".into());
        }
    });
}

/// softenRow: the angle box with Soften and Harden all.
fn soften_row(ui: &mut egui::Ui, t: &Tokens, boxes: &mut HashMap<String, String>, deg: f64, acts: &mut Vec<String>) {
    ui.label(RichText::new(mt("app.info.soften", &[])).color(t.text_dim));
    ui.horizontal(|ui| {
        let buf = boxes.entry("soften".into()).or_insert_with(|| ns(deg));
        ui.add(egui::TextEdit::singleline(buf).desired_width(40.0));
        ui.label("°");
        if ui.small_button(mt("app.info.soften.go", &[])).clicked() {
            acts.push(format!("soften:{buf}"));
        }
        if ui.small_button(mt("app.info.harden", &[])).clicked() {
            acts.push("harden".into());
        }
    });
    ui.end_row();
}

/// lenBox: a length you can retype; the action gets what was typed.
#[allow(clippy::too_many_arguments)]
fn len_box(
    ui: &mut egui::Ui,
    t: &Tokens,
    boxes: &mut HashMap<String, String>,
    acts: &mut Vec<String>,
    u: &str,
    label: &str,
    id: &str,
    meters: f64,
    act: &str,
) {
    ui.label(RichText::new(label).color(t.text_dim));
    if let Some(v) = text_box(ui, boxes, id, &cc_m3d::units::format_length(meters, u), 110.0) {
        acts.push(format!("{act}{v}"));
    }
    ui.end_row();
}

/// partRules: a part's rule per axis inside an open component (auto shows the rule it works out), and the spacing of a
/// repeated part.
#[allow(clippy::too_many_arguments)]
fn part_rules(
    ui: &mut egui::Ui,
    t: &Tokens,
    boxes: &mut HashMap<String, String>,
    acts: &mut Vec<String>,
    model: &cc_m3d::Model,
    comp: &cc_m3d::def::Def,
    iid: f64,
    u: &str,
) {
    let Some(inst) = comp.insts.get(iid) else { return };
    let Some(bb) = model.bbox(&comp.id, &cc_m3d::vec::IDENT) else { return };
    let parts = cc_m3d_tools::dynamic::parts_of(model, &comp.id);
    let Some(part) = parts.iter().find(|p| p.iid == iid) else { return };
    // the heading in the value column (a long label in the first column would widen the whole grid)
    ui.label("");
    ui.label(RichText::new(mt("app.dyn.rules", &[])).color(t.text_strong));
    ui.end_row();
    for (k, ax) in ["x", "y", "z"].iter().enumerate() {
        let cur = inst.rule.as_ref().and_then(|r| match k {
            0 => r.x.clone(),
            1 => r.y.clone(),
            _ => r.z.clone(),
        });
        let cur = cur.unwrap_or_else(|| "auto".into());
        let mut bare = part.clone();
        bare.inst.rule = None;
        let auto =
            mt("app.dyn.rule.auto", &[("rule", &mt(&format!("app.dyn.rule.{}", cc_m3d_tools::dynamic::rule_of(&bare, k, &bb)), &[]))]);
        let word = |r: &str| if r == "auto" { auto.clone() } else { mt(&format!("app.dyn.rule.{r}"), &[]) };
        let l = ui.label(RichText::new(mt(&format!("app.dyn.axis.{ax}"), &[])).color(t.text_dim));
        egui::ComboBox::new(format!("mc-rule-{ax}"), "")
            .width(150.0)
            .truncate()
            .selected_text(word(&cur))
            .show_ui(ui, |ui| {
                for r in crate::mcstretch::RULES {
                    if ui.selectable_label(cur == r, word(r)).clicked() && cur != r {
                        acts.push(format!("rule:{}:{ax}={r}", ns(iid)));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        ui.end_row();
        if cur == "repeat" {
            let sp = inst.rule.as_ref().and_then(|r| match k {
                0 => r.sx,
                1 => r.sy,
                _ => r.sz,
            });
            let w = sp.unwrap_or((part.bx.max.get(k).copied().unwrap_or(0.0) - part.bx.min.get(k).copied().unwrap_or(0.0)) * 2.0);
            ui.label(RichText::new(mt("app.dyn.spacing", &[])).color(t.text_dim));
            if let Some(v) = text_box(ui, boxes, &format!("rule-sp:{}:{ax}", ns(iid)), &cc_m3d::units::format_length(w, u), 90.0) {
                acts.push(format!("rule-spacing:{}:{ax}={v}", ns(iid)));
            }
            ui.end_row();
        }
    }
}
