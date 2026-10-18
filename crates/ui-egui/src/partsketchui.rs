//! The sketch editor's panels and stage (M13.3): the sketch tool bar (Exit sketch, the tools, More, Construction, the
//! sketch's name and state), the sketch panel on the right (partcraft.js `sketchProps`: state, legend, the selection's
//! relations and dimensions, construction / delete, the relations list, numeric entry, Exit / Fit) and the 2D stage
//! (the sketch drawn over the model's edges projected onto its plane, pointer and keys to the editor, the value box).

use cc_part::sketch::{self as S, Ent};
use cc_part::{jv, vec as V};
use egui::{Color32, Pos2, Rect, RichText, Stroke};
use serde_json::Value;

use crate::partsketch::{self as SK, P2};
use crate::partview::{Mode, PartView, sketch_key, tr, trv};
use crate::theme::Tokens;
use crate::{Params, view3d};

type Cmd = Option<(&'static str, Params)>;

fn cmd(id: &'static str, name: impl Into<String>) -> Cmd {
    Some((id, Params { name: Some(name.into()), ..Default::default() }))
}

/// The sketch's state colours (0.5's: under-defined blue, full ink, over red, construction grey, selection orange,
/// hover cyan, relations green), each preset's own.
#[derive(Clone, Copy)]
pub struct Pal {
    pub full: Color32,
    pub under: Color32,
    pub over: Color32,
    pub cons: Color32,
    pub sel: Color32,
    pub hover: Color32,
    pub dim: Color32,
    pub glyph: Color32,
    pub paper: Color32,
    pub axis: Color32,
    pub origin: Color32,
    pub model: Color32,
}

pub fn pal(t: &Tokens, dark: bool) -> Pal {
    if dark {
        Pal {
            full: t.text_strong,
            under: Color32::from_rgb(0x5b, 0x9b, 0xf0),
            over: Color32::from_rgb(0xf0, 0x5d, 0x4e),
            cons: t.text_dim,
            sel: Color32::from_rgb(0xf0, 0x9a, 0x3e),
            hover: Color32::from_rgb(0x2e, 0xc4, 0xd8),
            dim: t.text,
            glyph: Color32::from_rgb(0x4c, 0xc2, 0x7c),
            paper: t.panel_darker,
            axis: t.divider,
            origin: Color32::from_rgb(0xf0, 0x5d, 0x4e),
            model: t.viewport_frame,
        }
    } else {
        Pal {
            full: Color32::from_rgb(0x1d, 0x23, 0x2b),
            under: Color32::from_rgb(0x2f, 0x6f, 0xd6),
            over: Color32::from_rgb(0xc0, 0x39, 0x2b),
            cons: Color32::from_rgb(0x5b, 0x65, 0x70),
            sel: Color32::from_rgb(0xd9, 0x82, 0x2b),
            hover: Color32::from_rgb(0x1a, 0x9f, 0xb5),
            dim: Color32::from_rgb(0x1d, 0x23, 0x2b),
            glyph: Color32::from_rgb(0x2e, 0x9e, 0x5b),
            paper: Color32::WHITE,
            axis: Color32::from_rgb(0xb8, 0xc2, 0xcc),
            origin: Color32::from_rgb(0xc0, 0x39, 0x2b),
            model: Color32::from_rgb(0xa0, 0xa8, 0xb2),
        }
    }
}

fn glyph(ty: &str) -> &'static str {
    match ty {
        "coincident" => "•",
        "horizontal" => "H",
        "vertical" => "V",
        "parallel" => "∥",
        "perpendicular" => "⊥",
        "tangent" => "T",
        "equal" => "=",
        "concentric" => "◎",
        "midpoint" => "M",
        "fix" => "F",
        "symmetric" => "↔",
        "offsetlink" => "O",
        _ => "?",
    }
}

/// The sketch tool bar (shown in place of the feature tools while a sketch is open).
pub fn sketch_bar(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    if ui
        .add(egui::Button::new(RichText::new(tr("ui.tb.sketch.exit_sketch")).strong()))
        .on_hover_text(tr("ui.tb.sketch.exit_sketch.title"))
        .clicked()
    {
        *act = cmd("part.sketch.exit", "");
    }
    ui.separator();
    for (tool, key) in SK::TOOLS.iter().take(11).skip(1) {
        let on = pv.sk.tool == *tool;
        let title = format!("{key}.title");
        let tip = tr(&title);
        let b = egui::Button::selectable(on, tr(key));
        if ui.add(b).on_hover_text(if tip == title { tr(key) } else { tip }).clicked() {
            *act = cmd("part.sk.tool", *tool);
        }
    }
    ui.menu_button(tr("ui.tb.more"), |ui| {
        for (tool, key) in SK::TOOLS.iter().skip(11) {
            if ui.add(egui::Button::selectable(pv.sk.tool == *tool, tr(key))).clicked() {
                *act = cmd("part.sk.tool", *tool);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(tr("ui.tb.sketch.more.title"));
    if ui.button(tr("ui.tb.sketch.construction")).on_hover_text(tr("ui.tb.sketch.construction.title")).clicked() {
        *act = cmd("part.sk.construction", "");
    }
    ui.separator();
    ui.label(RichText::new(pv.sketch_title()).color(t.text_strong));
    if let Some(s) = &pv.last_solve {
        let words = match s.status {
            "empty" => tr("app.sketch.state_empty"),
            "under" => trv("app.sketch.state_under", &[("n", s.dof.to_string())]),
            "full" => tr("app.sketch.state_full"),
            _ => tr("app.sketch.state_over"),
        };
        ui.label(RichText::new(words).small().color(t.text_dim));
    }
}

fn entity_name(sk: &Value, id: &str) -> String {
    if id == S::ORIGIN {
        return tr("app.entity.origin");
    }
    let Some(e) = S::by_id(sk, id) else { return id.to_string() };
    let kind = match e.ty() {
        "point" => tr("app.entity.point"),
        "line" if e.construction() => tr("app.entity.centerline"),
        "line" => tr("app.entity.line"),
        "circle" => tr("app.entity.circle"),
        "arc" => tr("app.entity.arc"),
        "spline" => tr("app.entity.spline"),
        other => other.to_string(),
    };
    trv("app.entity.named", &[("kind", kind), ("id", id.to_string())])
}

/// partcraft.js `sketchProps()`.
pub fn sketch_props(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd, num: &mut (String, String, String)) {
    let sk = pv.sketch().clone();
    let bad = pv.bad_ids();
    if let Some(s) = &pv.last_solve {
        let list: Vec<String> = bad.iter().map(|id| describe(&sk, id)).collect();
        let text = match s.status {
            "over" => trv(
                if s.conflicts.is_empty() { "app.sketch.msg_over_redundant" } else { "app.sketch.msg_over_conflict" },
                &[("list", list.join(", "))],
            ),
            "full" => tr("app.sketch.msg_full"),
            "empty" => tr("app.sketch.msg_empty"),
            _ => trv("app.sketch.msg_under", &[("n", s.dof.to_string())]),
        };
        ui.label(RichText::new(text).color(t.text_strong));
    }
    let p = pal(t, t.text.r() > 128);
    ui.horizontal_wrapped(|ui| {
        for (c, k) in [
            (p.under, "app.sketch.legend_under"),
            (p.full, "app.sketch.state_full"),
            (p.over, "app.sketch.state_over"),
            (p.sel, "app.sketch.legend_selected"),
        ] {
            ui.label(RichText::new("■").color(c));
            ui.label(RichText::new(tr(k)).small().color(t.text_dim));
        }
    });
    ui.separator();
    let ents: Vec<String> = pv.sk.sel.iter().filter(|id| S::by_id(&sk, id).is_some()).cloned().collect();
    let cons: Vec<String> =
        pv.sk.sel.iter().filter(|id| S::constraints(&sk).iter().any(|c| jv::is_str(c.get("id"), id))).cloned().collect();
    if !ents.is_empty() {
        let names: Vec<String> = ents.iter().map(|id| entity_name(&sk, id)).collect();
        ui.label(RichText::new(trv("app.sketch.selected", &[("names", names.join(", "))])).strong().color(t.text_strong));
        let app = S::applicable(&sk, &ents);
        let rel: Vec<&str> = app.iter().copied().filter(|ty| S::RELATIONS.contains(ty)).collect();
        let dims: Vec<&str> = app.iter().copied().filter(|ty| S::DIMENSIONS.contains(ty)).collect();
        ui.horizontal_wrapped(|ui| {
            for ty in &rel {
                if ui.button(format!("{} {}", glyph(ty), S::label_of(ty).unwrap_or_default())).clicked() {
                    *act = cmd("part.sk.rel", *ty);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for ty in &dims {
                if ui.button(S::label_of(ty).unwrap_or_default()).clicked() {
                    *act = cmd("part.sk.rel", *ty);
                }
            }
        });
        if rel.is_empty() && dims.is_empty() {
            ui.label(RichText::new(tr("app.sketch.no_relation_fits")).small().color(t.text_dim));
        }
        ui.horizontal_wrapped(|ui| {
            if ents.iter().any(|id| S::by_id(&sk, id).is_some_and(|e| e.ty() != "point"))
                && ui.button(tr("app.sketch.construction_toggle")).clicked()
            {
                *act = cmd("part.sk.construction", "");
            }
            if ui.button(tr("app.btn.delete")).clicked() {
                *act = cmd("part.delete", "");
            }
        });
        let mine: Vec<Value> = S::constraints(&sk)
            .iter()
            .filter(|c| ents.iter().any(|id| jv::arr(c.get("refs")).iter().any(|r| r.as_str() == Some(id))))
            .cloned()
            .collect();
        if !mine.is_empty() {
            constraint_list(ui, &sk, &mine, &tr("app.sketch.relations_on_selection"), &bad, t, act);
        }
    } else if !cons.is_empty() {
        ui.label(RichText::new(tr("app.sketch.selected_relations")).strong().color(t.text_strong));
        if ui.button(tr("app.btn.delete")).clicked() {
            *act = cmd("part.delete", "");
        }
        let list: Vec<Value> = S::constraints(&sk).iter().filter(|c| cons.iter().any(|id| jv::is_str(c.get("id"), id))).cloned().collect();
        constraint_list(ui, &sk, &list, "", &bad, t, act);
    } else {
        ui.label(RichText::new(tr("app.sketch.panel_note")).small().color(t.text_dim));
        if !S::constraints(&sk).is_empty() {
            constraint_list(ui, &sk, S::constraints(&sk), &tr("app.sketch.relations_dims"), &bad, t, act);
        }
    }
    ui.separator();
    // numeric entry (partcraft.js numericEntry): geometry by typed coordinates
    ui.label(RichText::new(tr("app.num.title")).strong().color(t.text_strong));
    let kinds = [
        ("line", "app.num.line"),
        ("centerline", "app.num.centerline"),
        ("rect", "app.num.rect"),
        ("circle", "app.num.circle"),
        ("point", "app.num.point"),
    ];
    let cur = kinds.iter().find(|(k, _)| *k == num.0).map_or_else(|| tr("app.num.line"), |(_, l)| tr(l));
    egui::ComboBox::new("pc-num-kind", tr("app.num.add")).selected_text(cur).show_ui(ui, |ui| {
        for (k, l) in kinds {
            if ui.selectable_label(num.0 == k, tr(l)).clicked() {
                num.0 = k.to_string();
            }
        }
    });
    let labels: (&str, Option<&str>) = match num.0.as_str() {
        "point" => ("app.num.at", None),
        "rect" => ("app.num.corner", Some("app.num.opposite")),
        "circle" => ("app.num.centre", Some("app.num.radius")),
        _ => ("app.num.from", Some("app.num.to")),
    };
    egui::Grid::new("pc-num").num_columns(2).show(ui, |ui| {
        ui.label(tr(labels.0));
        ui.add(egui::TextEdit::singleline(&mut num.1).hint_text("0, 0").desired_width(110.0));
        ui.end_row();
        if let Some(l2) = labels.1 {
            ui.label(tr(l2));
            ui.add(egui::TextEdit::singleline(&mut num.2).hint_text(if num.0 == "circle" { "10" } else { "0, 0" }).desired_width(110.0));
            ui.end_row();
        }
    });
    if ui.button(tr("app.num.add")).clicked() {
        let kind = if num.0.is_empty() { "line".to_string() } else { num.0.clone() };
        *act = cmd("part.sk.num", format!("{kind}:{};{}", num.1, num.2));
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if ui.button(RichText::new(tr("app.sketch.exit")).strong()).clicked() {
            *act = cmd("part.sketch.exit", "");
        }
        if ui.button(tr("app.sketch.fit")).clicked() {
            *act = cmd("part.sk.fit", "");
        }
    });
}

fn describe(sk: &Value, id: &str) -> String {
    S::constraints(sk).iter().find(|c| jv::is_str(c.get("id"), id)).map_or_else(|| id.to_string(), SK::describe3)
}

fn constraint_list(ui: &mut egui::Ui, sk: &Value, list: &[Value], title: &str, bad: &[String], t: &Tokens, act: &mut Cmd) {
    if !title.is_empty() {
        ui.label(RichText::new(title).strong().color(t.text_strong));
    }
    for c in list.iter().take(200) {
        let id = jv::s(c, "id").to_string();
        let ty = jv::s(c, "type");
        let refs: Vec<String> = jv::arr(c.get("refs")).iter().map(|r| entity_name(sk, r.as_str().unwrap_or(""))).collect();
        ui.horizontal(|ui| {
            let mut text = RichText::new(SK::describe3(c)).color(t.text);
            if bad.contains(&id) {
                text = text.color(t.text_strong).strikethrough();
            }
            ui.label(text).on_hover_text(refs.join(", "));
            if S::is_dim(ty) && ui.small_button(tr("app.btn.edit")).clicked() {
                *act = cmd("part.sk.edit", id.clone());
            }
            if ui.small_button(tr("app.btn.select")).clicked() {
                *act = cmd("part.sk.select", id.clone());
            }
            if ui.small_button(tr("app.btn.delete")).clicked() {
                *act = cmd("part.sk.unrel", id.clone());
            }
        });
    }
}

/// The sketch stage: the model's edges projected onto the sketch plane, the sketch, the tool preview; pointer, wheel
/// and keys go to the editor (sketchview.js events); the value box floats where the editor asked.
pub fn stage(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView, t: &Tokens, dark: bool, act: &mut Cmd) {
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    pv.sk.size = pv.sk.size_pin.unwrap_or((f64::from(rect.width()), f64::from(rect.height())));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.canvas_bg);
    let p = pal(t, dark);
    let to = |q: P2, ed: &SK::SketchEd| -> Pos2 {
        let s = ed.to_screen(q);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    // the model behind: its edges projected onto the plane
    if let Some(frame) = pv.current_frame() {
        let segs = jv::arr(pv.regen.mesh.as_ref().and_then(|m| m.get("edgeSegs")));
        let vals: Vec<f64> = segs.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let stroke = Stroke::new(1.0, p.model);
        for seg in vals.as_chunks::<6>().0.iter().take(200_000) {
            let a = V::to_plane(&frame, [seg[0], seg[1], seg[2]]);
            let b = V::to_plane(&frame, [seg[3], seg[4], seg[5]]);
            painter.line_segment([to(a, &pv.sk), to(b, &pv.sk)], stroke);
        }
    }
    draw_sketch(&painter, rect, pv, &p);
    // input: raw pointer events in order (down / move / up, as the editor's pointer handlers get them)
    let (events, mods, hover) = ui.input(|i| (i.events.clone(), i.modifiers, i.pointer.hover_pos()));
    let local = |pos: Pos2| (f64::from(pos.x - rect.min.x), f64::from(pos.y - rect.min.y));
    let m = SK::Mods { shift: mods.shift, ctrl: mods.command };
    let typing = ui.ctx().egui_wants_keyboard_input();
    let mut ed = std::mem::take(&mut pv.sk);
    for e in &events {
        match e {
            egui::Event::PointerMoved(pos) if rect.contains(*pos) || ui.ctx().input(|i| i.pointer.any_down()) => {
                let (x, y) = local(*pos);
                ed.move_to(pv, x, y);
            }
            egui::Event::PointerButton { pos, button, pressed, .. } => {
                let b = match button {
                    egui::PointerButton::Primary => 0,
                    egui::PointerButton::Middle => 1,
                    egui::PointerButton::Secondary => 2,
                    _ => continue,
                };
                let (x, y) = local(*pos);
                if *pressed {
                    if rect.contains(*pos) && ed.ask.is_none() {
                        ed.down(pv, x, y, b, m);
                    }
                } else {
                    ed.up(pv, x, y, b, m);
                }
            }
            egui::Event::MouseWheel { delta, .. } if hover.is_some_and(|h| rect.contains(h)) => {
                if let Some(h) = hover {
                    let (x, y) = local(h);
                    ed.zoom_at(x, y, (-f64::from(delta.y) * 0.15).exp());
                }
            }
            _ => {}
        }
    }
    if resp.double_clicked()
        && let Some(h) = hover
    {
        let (x, y) = local(h);
        ed.dbl(pv, x, y);
    }
    pv.sk = ed;
    if !typing && pv.sk.ask.is_none() {
        for e in &events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                if modifiers.command || modifiers.alt {
                    if modifiers.command && *key == egui::Key::Z {
                        *act = cmd(if modifiers.shift { "part.redo" } else { "part.undo" }, "");
                    } else if modifiers.command && *key == egui::Key::Y {
                        *act = cmd("part.redo", "");
                    } else if modifiers.command && *key == egui::Key::A {
                        *act = cmd("part.sk.all", "");
                    }
                    continue;
                }
                let name = key.name();
                sketch_key(pv, name, modifiers.shift);
                if pv.mode != Mode::Sketch {
                    break;
                }
            }
        }
    }
    value_box(ui, rect, pv);
    // the pointer's sketch coordinates in the corner
    if let Some(c) = pv.coords {
        let txt = format!("{}, {}", V::fmt(c[0], 3), V::fmt(c[1], 3));
        painter.text(rect.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, txt, egui::FontId::monospace(11.0), t.text_dim);
    }
    let _ = view3d::paint;
}

fn value_box(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView) {
    let Some(ask) = pv.sk.ask.as_mut() else { return };
    let w = f64::from(rect.width());
    let x = rect.min.x + (ask.at[0] - 45.0).clamp(4.0, (w - 100.0).max(4.0)) as f32;
    let y = rect.min.y + (ask.at[1] - 14.0).max(4.0) as f32;
    let mut done: Option<bool> = None;
    egui::Area::new(egui::Id::unique("pc-sk-value")).fixed_pos(Pos2::new(x, y)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        ui.horizontal(|ui| {
            let r = ui
                .add(egui::TextEdit::singleline(&mut ask.text).desired_width(90.0).id_salt("pc-sk-value-text"))
                .on_hover_text(ask.label.clone());
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            ui.label(RichText::new(&ask.label).small());
            if r.lost_focus() {
                done = Some(!ui.input(|i| i.key_pressed(egui::Key::Escape)));
            }
        });
    });
    if let Some(apply) = done {
        let mut ed = std::mem::take(&mut pv.sk);
        ed.input_done(pv, apply);
        pv.sk = ed;
    }
}

fn draw_sketch(painter: &egui::Painter, rect: Rect, pv: &mut PartView, p: &Pal) {
    let sk = pv.sketch().clone();
    let ed = &pv.sk;
    let to = |q: P2| -> Pos2 {
        let s = ed.to_screen(q);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let bad = pv.bad_ids();
    let mut bad_ents: Vec<String> = Vec::new();
    for c in S::constraints(&sk) {
        if bad.iter().any(|b| jv::is_str(c.get("id"), b)) {
            bad_ents.extend(jv::arr(c.get("refs")).iter().filter_map(|r| r.as_str().map(str::to_string)));
        }
    }
    let determined: Vec<String> = pv.last_solve.as_ref().map(|s| s.determined.clone()).unwrap_or_default();
    let hover = ed.hover.as_ref().map(|h| h.id.clone());
    let color_of = |id: &str, cons: bool| -> Color32 {
        if ed.sel.iter().any(|s| s == id) {
            p.sel
        } else if hover.as_deref() == Some(id) {
            p.hover
        } else if bad_ents.iter().any(|b| b == id) {
            p.over
        } else if cons {
            p.cons
        } else if determined.iter().any(|d| d == id) {
            p.full
        } else {
            p.under
        }
    };
    // axes through the origin
    let o = to([0.0, 0.0]);
    let ax = Stroke::new(1.0, p.axis);
    painter.extend(egui::Shape::dashed_line(&[Pos2::new(rect.min.x, o.y), Pos2::new(rect.max.x, o.y)], ax, 6.0, 4.0));
    painter.extend(egui::Shape::dashed_line(&[Pos2::new(o.x, rect.min.y), Pos2::new(o.x, rect.max.y)], ax, 6.0, 4.0));
    for e in S::entities(&sk) {
        let en = Ent::E(e);
        if en.ty() == "point" {
            continue;
        }
        let id = en.id();
        let hot = ed.sel.iter().any(|s| s == id) || hover.as_deref() == Some(id);
        let st = Stroke::new(if hot { 2.5 } else { 1.75 }, color_of(id, en.construction()));
        let pts: Vec<Pos2> = SK::entity_polyline(&sk, &en).into_iter().map(to).collect();
        if en.construction() {
            painter.extend(egui::Shape::dashed_line(&pts, st, 8.0, 4.0));
        } else {
            painter.add(egui::Shape::line(pts, st));
        }
        if en.ty() == "spline" && en.str("kind") == "control" {
            let mut q: Vec<Pos2> = S::points_of(&en).iter().map(|pid| to(S::pxy(&sk, pid))).collect();
            if jv::truthy(en.val("closed"))
                && let Some(f) = q.first().copied()
            {
                q.push(f);
            }
            painter.extend(egui::Shape::dashed_line(&q, Stroke::new(1.0, st.color.gamma_multiply(0.6)), 3.0, 3.0));
        }
    }
    let loose: Vec<String> = S::loose_points(&sk).iter().map(|q| jv::s(q, "id").to_string()).collect();
    for e in S::entities(&sk) {
        if jv::s(e, "type") != "point" {
            continue;
        }
        let id = jv::s(e, "id");
        let at = to([jv::n(e, "x"), jv::n(e, "y")]);
        let c = color_of(id, false);
        let is_loose = loose.iter().any(|l| l == id);
        let r = if is_loose { 3.5 } else { 2.5 };
        let hot = ed.sel.iter().any(|s| s == id) || hover.as_deref() == Some(id);
        painter.circle_filled(at, if hot { r + 1.5 } else { r }, c);
        if is_loose {
            painter.line_segment([at - egui::vec2(7.0, 0.0), at + egui::vec2(7.0, 0.0)], Stroke::new(1.0, c));
            painter.line_segment([at - egui::vec2(0.0, 7.0), at + egui::vec2(0.0, 7.0)], Stroke::new(1.0, c));
        }
    }
    let oc = if ed.sel.iter().any(|s| s == S::ORIGIN) { p.sel } else { p.origin };
    painter.circle_stroke(o, 5.0, Stroke::new(1.5, oc));
    painter.line_segment([o - egui::vec2(9.0, 0.0), o + egui::vec2(9.0, 0.0)], Stroke::new(1.5, oc));
    painter.line_segment([o - egui::vec2(0.0, 9.0), o + egui::vec2(0.0, 9.0)], Stroke::new(1.5, oc));
    // relations and dimensions
    let (dims, glyphs) = ed.label_boxes(&sk, &bad);
    for c in S::constraints(&sk) {
        let id = jv::s(c, "id");
        let ty = jv::s(c, "type");
        let col = if ed.sel.iter().any(|s| s == id) {
            Some(p.sel)
        } else if hover.as_deref() == Some(id) {
            Some(p.hover)
        } else if bad.iter().any(|b| b == id) {
            Some(p.over)
        } else {
            None
        };
        if S::is_dim(ty) {
            let Some(g) = SK::dim_geometry(&sk, c) else { continue };
            let col = col.unwrap_or(p.dim);
            let st = Stroke::new(1.0, col);
            for [a, b] in &g.lines {
                painter.line_segment([to(*a), to(*b)], st);
            }
            for (c0, r, a0, a1) in &g.arcs {
                let pts: Vec<Pos2> = (0..=32)
                    .map(|i| a0 + (a1 - a0) * f64::from(i) / 32.0)
                    .map(|a| to([c0[0] + r * a.cos(), c0[1] + r * a.sin()]))
                    .collect();
                painter.add(egui::Shape::line(pts, st));
            }
            for (tip, dir) in &g.arrows {
                let at = to(*tip);
                let d = [dir[0], -dir[1]];
                let l = d[0].hypot(d[1]);
                let l = if l > 0.0 { l } else { 1.0 };
                let u = [(d[0] / l) as f32, (d[1] / l) as f32];
                let a = at + egui::vec2(-u[0] * 9.0 - u[1] * 3.0, -u[1] * 9.0 + u[0] * 3.0);
                let b = at + egui::vec2(-u[0] * 9.0 + u[1] * 3.0, -u[1] * 9.0 - u[0] * 3.0);
                painter.add(egui::Shape::convex_polygon(vec![at, a, b], col, Stroke::NONE));
            }
            if let Some(bx) = dims.iter().find(|b| b.id == id) {
                let r = Rect::from_min_size(
                    Pos2::new(rect.min.x + bx.x as f32, rect.min.y + bx.y as f32),
                    egui::vec2(bx.w as f32, bx.h as f32),
                );
                painter.rect_filled(r, 0.0, p.paper);
                painter.rect_stroke(r, 0.0, st, egui::StrokeKind::Inside);
                painter.text(r.center(), egui::Align2::CENTER_CENTER, &g.text, egui::FontId::proportional(12.0), col);
            }
            continue;
        }
        if let Some(bx) = glyphs.iter().find(|b| b.id == id) {
            let r = Rect::from_min_size(Pos2::new(rect.min.x + bx.x as f32, rect.min.y + bx.y as f32), egui::vec2(15.0, 15.0));
            let col = col.unwrap_or(p.glyph);
            painter.rect_filled(r, 0.0, p.paper);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, glyph(ty), egui::FontId::proportional(11.0), col);
        }
    }
    // the tool's preview
    let mut ed2 = pv.sk.clone();
    let pre = ed2.preview(&sk);
    pv.sk = ed2;
    let ed = &pv.sk;
    let to = |q: P2| -> Pos2 {
        let s = ed.to_screen(q);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    for id in &pre.outline {
        if let Some(e) = S::by_id(&sk, id) {
            outline(painter, &sk, &e, p.sel, &to);
        }
    }
    if let Some(e) = pre.hover.as_deref().and_then(|id| S::by_id(&sk, id)) {
        outline(painter, &sk, &e, p.hover, &to);
    }
    let dash = Stroke::new(1.5, p.sel);
    for l in &pre.lines {
        let pts: Vec<Pos2> = l.iter().map(|q| to(*q)).collect();
        painter.extend(egui::Shape::dashed_line(&pts, dash, 5.0, 4.0));
    }
    for l in &pre.thin {
        let pts: Vec<Pos2> = l.iter().map(|q| to(*q)).collect();
        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, p.sel), 3.0, 3.0));
    }
    for d in &pre.dots {
        painter.circle_filled(to(*d), 3.0, p.sel);
    }
    if let Some(tr) = &pre.trim {
        painter.add(egui::Shape::line(tr.iter().map(|q| to(*q)).collect(), Stroke::new(3.5, p.over)));
    }
    if let Some((q, point)) = pre.snap {
        let c = to(q);
        let st = Stroke::new(2.0, p.hover);
        if point {
            painter.rect_stroke(Rect::from_center_size(c, egui::vec2(12.0, 12.0)), 0.0, st, egui::StrokeKind::Middle);
        } else {
            painter.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], st);
            painter.line_segment([c + egui::vec2(-6.0, -6.0), c + egui::vec2(6.0, 6.0)], st);
        }
    }
    if let Some((q, text)) = &pre.tag {
        let at = to(*q) + egui::vec2(10.0, 8.0);
        let galley = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(12.0), p.full);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + egui::vec2(8.0, 4.0)), 0.0, p.paper);
        painter.galley(at + egui::vec2(4.0, 2.0), galley, p.full);
    }
    if let Some((a, b)) = ed.window() {
        let r = Rect::from_two_pos(
            Pos2::new(rect.min.x + a[0] as f32, rect.min.y + a[1] as f32),
            Pos2::new(rect.min.x + b[0] as f32, rect.min.y + b[1] as f32),
        );
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, p.sel), 4.0, 3.0));
    }
}

fn outline(painter: &egui::Painter, sk: &Value, e: &Ent, col: Color32, to: &dyn Fn(P2) -> Pos2) {
    if e.ty() == "point" {
        painter.circle_filled(to([e.num("x"), e.num("y")]), 5.0, col);
        return;
    }
    let pts: Vec<Pos2> = SK::entity_polyline(sk, e).into_iter().map(to).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(3.0, col)));
}
