//! CivilCraft's side panels (civilcraft.js `renderLeft` / `renderProps`), read from the engine: on the left the site,
//! the survey points and their groups (how each builds the surface), the surface facts and report, the display
//! switches and the objects list; on the right the properties of what is selected (alignment with curves and profile,
//! pad with daylight and volumes, structure, pipe, point, feature line, parcel) or the project summary (earthwork by
//! the grid method, the pipe network check). Words come from CivilCraft 0.5's catalog. Panels only lay out widgets
//! and hand back [`Act`]s; the view applies them.

use cc_civil::js::{arr, numf, string_of, string_or_empty};
use cc_civil::tr;
use cc_civil::units::{area_text, bearing, dms, fixed, format_station, grouped, pct, volume_text};
use egui::RichText;
use serde_json::Value;

use crate::civilview::{CivilView, Sel, t0};
use crate::theme::Tokens;

/// What a panel asked for.
#[derive(Clone, Debug)]
pub enum Act {
    Select(Option<Sel>),
    Display(String),
    Group(String, String),
    /// a `civil.export` name (project, landxml, dxf, dwg, pdf, glb)
    Export(String),
    /// any `civil.*` command with its name (the Parcels and Traverse blocks' buttons)
    Run(&'static str, String),
}

fn head(ui: &mut egui::Ui, tk: &Tokens, s: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(s).strong().color(tk.text_strong));
    ui.add_space(2.0);
}

fn rows(ui: &mut egui::Ui, tk: &Tokens, id: &str, list: &[(String, String)]) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 3.0]).striped(false).show(ui, |ui| {
        for (k, v) in list {
            ui.label(RichText::new(k).color(tk.text_dim));
            ui.label(RichText::new(v).color(tk.text));
            ui.end_row();
        }
    });
}

fn note(ui: &mut egui::Ui, tk: &Tokens, s: &str) {
    ui.label(RichText::new(s).small().color(tk.text_dim));
}

fn report(ui: &mut egui::Ui, tk: &Tokens, lines: &[String], error: bool) {
    if lines.is_empty() {
        return;
    }
    let bar = if error { egui::Color32::from_rgb(0xd0, 0x4a, 0x5a) } else { egui::Color32::from_rgb(0xc0, 0x96, 0x2e) };
    egui::Frame::new().fill(tk.panel_darker).stroke(egui::Stroke::new(1.0, bar)).inner_margin(6).show(ui, |ui| {
        for l in lines.iter().take(40) {
            ui.label(RichText::new(format!("• {l}")).small().color(tk.text));
        }
        if lines.len() > 40 {
            ui.label(RichText::new(format!("… {}", lines.len() - 40)).small().color(tk.text_dim));
        }
    });
}

fn link(ui: &mut egui::Ui, tk: &Tokens, s: &str, on: bool) -> bool {
    ui.add(egui::Button::new(RichText::new(s).color(if on { tk.text_strong } else { tk.accent })).frame(false).selected(on)).clicked()
}

/// The left panel: site, points, groups, surface, display, objects.
pub fn left(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let units = cv.project.units().to_string();
    let p = &mut cv.project;
    head(ui, tk, &t0("ui.site"));
    let name = p.state.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    rows(ui, tk, "cv_site", &[(t0("ui.site.name"), name), (t0("ui.site.units"), cc_civil::units::unit_of(&units).name())]);
    export_group(cv, ui, tk, acts);
    let p = &mut cv.project;
    // points
    head(ui, tk, &t0("ui.points"));
    let n = p.points.len();
    if n == 0 {
        note(ui, tk, &t0("ui.points.empty"));
    } else {
        let withz = p.points.iter().filter(|q| q.z.is_some()).count();
        let file = if p.points_file.is_empty() { "—".to_string() } else { p.points_file.clone() };
        rows(
            ui,
            tk,
            "cv_pts",
            &[(t0("cv.pts.file"), file), (t0("codes.col.n"), grouped(n as f64, 0)), (t0("cv.pts.withz"), grouped(withz as f64, 0))],
        );
    }
    let groups = cv.facts.groups.clone();
    if !groups.is_empty() {
        ui.add_space(4.0);
        egui::Grid::new("cv_groups").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
            ui.label(RichText::new(t0("ui.groups.group")).small().color(tk.text_dim));
            ui.label(RichText::new(t0("ui.groups.points")).small().color(tk.text_dim));
            ui.label(RichText::new(t0("ui.groups.surface")).small().color(tk.text_dim));
            ui.end_row();
            for (g, count, use_) in groups.iter().take(200) {
                ui.label(RichText::new(g).color(tk.text));
                ui.label(RichText::new(grouped(*count as f64, 0)).color(tk.text));
                let mut u = use_.clone();
                egui::ComboBox::new(format!("cv_group_{g}"), "")
                    .selected_text(t0(&format!("codes.use.{u}")))
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        for k in ["points", "breakline", "off"] {
                            ui.selectable_value(&mut u, k.to_string(), t0(&format!("codes.use.{k}")));
                        }
                    })
                    .response
                    .on_hover_text(tr!("cv.group.use", group = g.as_str()));
                if u != *use_ {
                    acts.push(Act::Group(g.clone(), u));
                }
                ui.end_row();
            }
        });
    }
    // surface
    let has_tin = cc_civil::js::truthy(p.state.get("surface").and_then(|s| s.get("tin")));
    let grid_tin = p.state.get("surface").and_then(|s| s.get("tin")).and_then(|t| t.get("from")).and_then(Value::as_str) == Some("grid");
    head(
        ui,
        tk,
        &t0(if grid_tin {
            "cv.surface.grid"
        } else if has_tin {
            "cv.surface.landxml"
        } else {
            "cv.surface"
        }),
    );
    let eg = p.eg();
    match eg.surface.as_ref() {
        Some(sf) => {
            let st = sf.stats();
            let bnd = p.state.get("surface").and_then(|s| s.get("boundary")).and_then(Value::as_array).map(Vec::len);
            rows(
                ui,
                tk,
                "cv_sf",
                &[
                    (t0("codes.col.n"), grouped(st.points as f64, 0)),
                    (t0("cv.sf.triangles"), grouped(st.triangles as f64, 0)),
                    (t0("cv.sf.elevations"), tr!("cv.range", a = fixed(st.min_z, 2), b = fixed(st.max_z, 2))),
                    (t0("cv.sf.mean"), fixed(st.mean_z, 2)),
                    (t0("cv.sf.area2d"), area_text(st.area2d, &units, 2)),
                    (t0("cv.sf.area3d"), area_text(st.area3d, &units, 2)),
                    (t0("cv.sf.breaklines"), grouped(cv.facts.breaklines as f64, 0)),
                    (t0("cv.sf.boundary"), bnd.map_or_else(|| t0("gis.label.no"), |n| tr!("cv.sf.boundary.yes", n = n))),
                ],
            );
        }
        None => rows(ui, tk, "cv_sf", &[(t0("cv.surface"), t0(if n > 0 { "cv.sf.notbuilt" } else { "cv.sf.noneyet" }))]),
    }
    let mut notes: Vec<String> = eg.report.errors.clone();
    notes.extend(eg.report.notes.iter().cloned());
    notes.extend(cv.report.iter().cloned());
    report(ui, tk, &notes, !eg.report.errors.is_empty());
    // display
    head(ui, tk, &t0("ui.display"));
    let d = p.state.get("display").cloned().unwrap_or(Value::Null);
    let has_fg = p.graded().surface.is_some();
    let fg_on = d.get("surface").and_then(Value::as_str) == Some("fg") && has_fg;
    ui.horizontal(|ui| {
        if ui.selectable_label(!fg_on, t0("surface.eg")).clicked() {
            acts.push(Act::Display("surface=eg".into()));
        }
        if ui.add_enabled(has_fg, egui::Button::new(t0("ui.display.fg")).selected(fg_on)).clicked() {
            acts.push(Act::Display("surface=fg".into()));
        }
    });
    let iv = numf(d.get("interval"));
    let u_short = cc_civil::units::unit_of(&units).short.to_string();
    let mut ivs: Vec<f64> =
        if units == "m" { vec![0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0] } else { vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0] };
    if iv > 0.0 && !ivs.contains(&iv) {
        ivs.push(iv);
        ivs.sort_by(f64::total_cmp);
    }
    ui.horizontal(|ui| {
        let mut sel = iv;
        egui::ComboBox::new("cv_interval", t0("ui.display.interval")).selected_text(format!("{iv} {u_short}")).width(80.0).show_ui(
            ui,
            |ui| {
                for v in &ivs {
                    ui.selectable_value(&mut sel, *v, format!("{v} {u_short}"));
                }
            },
        );
        if sel != iv {
            acts.push(Act::Display(format!("interval={sel}")));
        }
    });
    ui.horizontal(|ui| {
        let mj = numf(d.get("major"));
        let mut sel = mj;
        egui::ComboBox::new("cv_major", t0("ui.display.major")).selected_text(format!("{mj}")).width(60.0).show_ui(ui, |ui| {
            for v in [2.0, 4.0, 5.0, 10.0] {
                ui.selectable_value(&mut sel, v, t0(&format!("ui.display.major.{v}")));
            }
        });
        if sel != mj {
            acts.push(Act::Display(format!("major={sel}")));
        }
    });
    for (k, label) in [
        ("contours", "ui.show.contours"),
        ("labels", "ui.show.labels"),
        ("bands", "ui.show.bands"),
        ("triangles", "ui.show.triangles"),
        ("slopes", "ui.show.slopes"),
        ("points", "ui.show.points"),
        ("pointLabels", "ui.show.pointlabels"),
        ("breaklines", "ui.show.breaklines"),
        ("outline", "ui.show.outline"),
        ("pipes", "ui.show.pipes"),
    ] {
        let mut on = d.get(k).is_some_and(|v| cc_civil::js::truthy(Some(v)));
        if ui.checkbox(&mut on, RichText::new(t0(label)).color(tk.text)).changed() {
            acts.push(Act::Display(k.to_string()));
        }
    }
    // objects
    objects(cv, ui, tk, acts);
    parcel_blocks(ui, tk, acts);
}

/// Every object of the project, as links that select it on the plan.
/// The Export group (0.5's tool bar group: LandXML, DXF, DWG with the DWG dialog's format, PDF with the plot dialog's
/// paper / scale / ink; GLB from the 3D bar; Save).
fn export_group(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    use cc_io_civil::plot;
    head(ui, tk, &t0("ui.tb.group.export"));
    let units = cv.project.units().to_string();
    let mut btn = |ui: &mut egui::Ui, label: &str, tip: &str, name: &str| {
        if ui.button(label).on_hover_text(tip).clicked() {
            acts.push(Act::Export(name.to_string()));
        }
    };
    ui.horizontal_wrapped(|ui| {
        btn(ui, &t0("ui.tb.save"), &t0("ui.tb.save.title"), "project");
        btn(ui, &t0("ui.tb.landxml"), &t0("ui.tb.landxml.title"), "landxml");
        btn(ui, &t0("ui.tb.dxf"), &t0("ui.tb.dxf.title"), "dxf");
        btn(ui, "GLB", &t0("v3d.glb.title"), "glb");
    });
    ui.horizontal(|ui| {
        btn(ui, &t0("ui.tb.dwg"), &t0("ui.tb.dwg.title"), "dwg");
        let year = |v: &str| crate::civilexport::DWG_YEAR.iter().find(|(k, _)| *k == v).map_or("", |(_, y)| *y).to_string();
        let mut v = cv.export.dwg.clone();
        egui::ComboBox::new("cv_dwg_format", t0("ui.dwg.format")).selected_text(year(&v)).width(70.0).show_ui(ui, |ui| {
            for (k, y) in crate::civilexport::DWG_YEAR {
                ui.selectable_value(&mut v, k.to_string(), y);
            }
        });
        cv.export.dwg = v;
    });
    ui.horizontal(|ui| {
        btn(ui, &t0("ui.tb.pdf"), &t0("ui.tb.pdf.title"), "pdf");
        let mut mono = cv.export.mono;
        egui::ComboBox::new("cv_plot_ink", t0("ui.plot.ink"))
            .selected_text(t0(if mono { "ui.plot.mono" } else { "ui.plot.color" }))
            .width(96.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mono, false, t0("ui.plot.color"));
                ui.selectable_value(&mut mono, true, t0("ui.plot.mono"));
            });
        cv.export.mono = mono;
    });
    let paper = cv.export.paper.clone().unwrap_or_else(|| if units == "m" { "a3".into() } else { "tabloid".into() });
    let mut pk = paper.clone();
    egui::ComboBox::new("cv_plot_paper", t0("ui.plot.paper")).selected_text(plot::paper_name(&plot::paper_by(&pk))).width(170.0).show_ui(
        ui,
        |ui| {
            for p in plot::PAPERS {
                ui.selectable_value(&mut pk, p.key.to_string(), plot::paper_name(&p));
            }
        },
    );
    if pk != paper {
        cv.export.paper = Some(pk.clone());
        cv.export.scale = None;
    }
    let mut area_view = cv.export.area_view;
    egui::ComboBox::new("cv_plot_area", t0("ui.plot.area"))
        .selected_text(t0(if area_view { "ui.plot.view" } else { "ui.plot.all" }))
        .width(170.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut area_view, false, t0("ui.plot.all"));
            ui.selectable_value(&mut area_view, true, t0("ui.plot.view"));
        });
    if area_view != cv.export.area_view {
        cv.export.area_view = area_view;
        cv.export.scale = None;
    }
    // the scale list with the one that fits marked (plotDialog fill())
    let area = if area_view { crate::civilexport::view_bounds(&cv.view) } else { cv.export.extents(&mut cv.project) };
    let fit = plot::fit_scale(&units, &area, &plot::paper_by(&pk), true).label;
    let cur = cv.export.scale.clone().unwrap_or_else(|| fit.clone());
    let shown = |l: &str| if l == fit { tr!("cv.plot.fits", scale = l) } else { l.to_string() };
    let mut sel = cur.clone();
    egui::ComboBox::new("cv_plot_scale", t0("ui.plot.scale")).selected_text(shown(&cur)).width(170.0).show_ui(ui, |ui| {
        for s in plot::scales_for(&units) {
            ui.selectable_value(&mut sel, s.label.clone(), shown(&s.label));
        }
    });
    if sel != cur {
        cv.export.scale = if sel == fit { None } else { Some(sel) };
    }
}

fn objects(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let st = cv.project.state.clone();
    let mut any = false;
    type MakeSel = fn(String) -> Sel;
    let lists: [(&str, &str, MakeSel); 4] = [
        ("alignments", "cv.props.alignments", Sel::Alignment),
        ("pads", "cv.props.pads", Sel::Pad),
        ("features", "fl.props.head", Sel::Feature),
        ("parcels", "pc.head", Sel::Parcel),
    ];
    for (key, title, make) in lists {
        let items = arr(st.get(key));
        if items.is_empty() {
            continue;
        }
        if !any {
            head(ui, tk, &t0("ui.props.project"));
            any = true;
        }
        egui::CollapsingHeader::new(RichText::new(format!("{} ({})", t0(title), items.len())).color(tk.text_strong))
            .id_salt(format!("cv_obj_{key}"))
            .default_open(true)
            .show(ui, |ui| {
                for o in items.iter().take(2000) {
                    let id = string_of(o.get("id"));
                    let s = make(id);
                    let on = cv.sel.as_ref() == Some(&s);
                    if link(ui, tk, &string_or_empty(o.get("name")), on) {
                        acts.push(Act::Select(Some(s)));
                    }
                }
            });
    }
    if let Some(n) = st.get("network").filter(|n| n.is_object()) {
        if !any {
            head(ui, tk, &t0("ui.props.project"));
        }
        let ss = arr(n.get("structures"));
        let ps = arr(n.get("pipes"));
        egui::CollapsingHeader::new(RichText::new(format!("{} ({} + {})", t0("cv.net.head"), ss.len(), ps.len())).color(tk.text_strong))
            .id_salt("cv_obj_net")
            .default_open(true)
            .show(ui, |ui| {
                for (list, make) in [(ss, Sel::Structure as fn(String) -> Sel), (ps, Sel::Pipe)] {
                    for o in list.iter().take(5000) {
                        let s = make(string_of(o.get("id")));
                        let on = cv.sel.as_ref() == Some(&s);
                        if link(ui, tk, &string_or_empty(o.get("name")), on) {
                            acts.push(Act::Select(Some(s)));
                        }
                    }
                }
            });
    }
}

/// The Parcels and Traverse blocks (parcels-ui.js `block`, traverse-ui.js `block`): draw, deed calls, traverse.
fn parcel_blocks(ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    head(ui, tk, &t0("pc.head"));
    ui.horizontal_wrapped(|ui| {
        if ui.button(t0("pc.draw")).on_hover_text(t0("pc.hint")).clicked() {
            acts.push(Act::Run("civil.tool", "parcel".into()));
        }
        if ui.button(t0("pc.calls")).on_hover_text(t0("pc.dlg.title")).clicked() {
            acts.push(Act::Run("civil.calls", String::new()));
        }
    });
    head(ui, tk, &t0("tv.head"));
    note(ui, tk, &t0("tv.lede"));
    if ui.button(t0("tv.open")).clicked() {
        acts.push(Act::Run("civil.traverse", String::new()));
    }
}

fn find<'a>(list: &'a [Value], id: &str) -> Option<&'a Value> {
    list.iter().find(|o| o.get("id").and_then(Value::as_str) == Some(id))
}

/// The right panel: the selection's properties, else the project summary.
pub fn right(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let sel = cv.sel.clone();
    let st = cv.project.state.clone();
    let shown = match &sel {
        Some(Sel::Alignment(id)) => find(arr(st.get("alignments")), id).cloned().map(|a| alignment(cv, ui, tk, &a)).is_some(),
        Some(Sel::Pad(id)) => find(arr(st.get("pads")), id).cloned().map(|d| pad(cv, ui, tk, &d)).is_some(),
        Some(Sel::Structure(id)) => {
            let ss = st.get("network").map(|n| arr(n.get("structures")).to_vec()).unwrap_or_default();
            find(&ss, id).cloned().map(|x| structure(cv, ui, tk, &x)).is_some()
        }
        Some(Sel::Pipe(id)) => {
            let ps = st.get("network").map(|n| arr(n.get("pipes")).to_vec()).unwrap_or_default();
            find(&ps, id).cloned().map(|x| pipe(cv, ui, tk, &x)).is_some()
        }
        Some(Sel::Feature(id)) => find(arr(st.get("features")), id).cloned().map(|f| feature(cv, ui, tk, &f)).is_some(),
        Some(Sel::Parcel(id)) => find(arr(st.get("parcels")), id).cloned().map(|f| parcel(cv, ui, tk, &f)).is_some(),
        Some(Sel::Point(n)) => cv.project.points.iter().find(|p| p.n == *n).cloned().map(|p| point(cv, ui, tk, &p)).is_some(),
        None => false,
    };
    if !shown {
        if sel.is_some() {
            acts.push(Act::Select(None));
        }
        project(cv, ui, tk, acts);
    }
}

fn project(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    head(ui, tk, &t0("cv.props.project"));
    note(ui, tk, &t0("cv.props.project.note"));
    let st = cv.project.state.clone();
    let u = cc_civil::units::unit_of(cv.project.units()).short.to_string();
    let als = arr(st.get("alignments"));
    if !als.is_empty() {
        head(ui, tk, &t0("cv.props.alignments"));
        for a in als {
            let al = cv.project.alignment(a);
            ui.horizontal(|ui| {
                if link(ui, tk, &string_or_empty(a.get("name")), false) {
                    acts.push(Act::Select(Some(Sel::Alignment(string_of(a.get("id"))))));
                }
                ui.label(
                    RichText::new(if al.ok() { format!("· {} {u}", fixed(al.length, 2)) } else { format!("· {}", t0("cv.al.needsfix")) })
                        .color(tk.text),
                );
            });
        }
    }
    let pads = arr(st.get("pads"));
    if !pads.is_empty() {
        head(ui, tk, &t0("cv.props.pads"));
        for d in pads {
            ui.horizontal(|ui| {
                if link(ui, tk, &string_or_empty(d.get("name")), false) {
                    acts.push(Act::Select(Some(Sel::Pad(string_of(d.get("id"))))));
                }
                ui.label(RichText::new(format!("· {}", tr!("cv.pad.el", z = fixed(numf(d.get("z")), 2)))).color(tk.text));
            });
        }
    }
    let corridor = als.iter().any(|a| a.get("corridor").and_then(|c| c.get("on")) == Some(&Value::Bool(true)));
    if !pads.is_empty() || corridor || !arr(st.get("features")).is_empty() {
        volumes(cv, ui, tk);
    }
    let parcels = arr(st.get("parcels"));
    if !parcels.is_empty() {
        head(ui, tk, &t0("pc.head"));
        for pc in parcels {
            let parcel = cc_civil::parcels::read_parcel(pc);
            ui.horizontal(|ui| {
                if link(ui, tk, &parcel.name, false) {
                    acts.push(Act::Select(Some(Sel::Parcel(string_of(pc.get("id"))))));
                }
                ui.label(
                    RichText::new(cc_civil::parcels::area_words(cc_civil::parcels::parcel_area(&parcel.pts), cv.project.units()))
                        .color(tk.text),
                );
            });
        }
    }
    if st.get("network").is_some_and(Value::is_object) {
        network(cv, ui, tk, acts);
    }
    if als.is_empty() && pads.is_empty() && !st.get("network").is_some_and(Value::is_object) {
        note(ui, tk, &t0("cv.props.empty"));
    }
}

/// civilcraft.js `volumeBlock`: earthwork by the grid method.
fn volumes(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens) {
    head(ui, tk, &t0("cv.ew.head"));
    report(ui, tk, &cv.facts.graded_problems, true);
    let units = cv.project.units().to_string();
    let Some(v) = cv.facts.volumes.clone() else {
        note(ui, tk, &t0("cv.ew.none"));
        return;
    };
    let (cut, fill) = (numf(v.get("cut")), numf(v.get("fill")));
    let net = fill - cut;
    let net_s = if net.abs() < 1e-9 {
        volume_text(net.abs(), &units, 0)
    } else {
        tr!(if net > 0.0 { "cv.ew.net.fill" } else { "cv.ew.net.cut" }, v = volume_text(net.abs(), &units, 0))
    };
    rows(
        ui,
        tk,
        "cv_vol",
        &[(t0("cv.ew.cut"), volume_text(cut, &units, 0)), (t0("cv.ew.fill"), volume_text(fill, &units, 0)), (t0("cv.ew.net"), net_s)],
    );
    let items = arr(v.get("items"));
    if items.len() > 1 {
        for it in items {
            note(
                ui,
                tk,
                &tr!(
                    "cv.ew.item",
                    name = string_or_empty(it.get("name")),
                    cut = volume_text(numf(it.get("cut")), &units, 0),
                    fill = volume_text(numf(it.get("fill")), &units, 0)
                ),
            );
        }
    }
    let cells = numf(v.get("cells"));
    let cell = fixed(numf(v.get("cell")), 3);
    let cell = cell.trim_end_matches('0').trim_end_matches('.').to_string();
    note(ui, tk, &tr!("cv.ew.grid", n = cells, cells = grouped(cells, 0), cell = cell, u = cc_civil::units::unit_of(&units).short));
}

/// civilcraft.js `networkBlock`: rules, counts, the problems of the check.
fn network(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let Some(n) = cv.project.state.get("network").cloned() else { return };
    let u = cc_civil::units::unit_of(cv.project.units()).short.to_string();
    head(ui, tk, &t0("cv.net.head"));
    let r = n.get("rules");
    let ty = if n.get("type").and_then(Value::as_str) == Some("sanitary") { "cv.net.sanitary" } else { "cv.net.storm" };
    rows(
        ui,
        tk,
        "cv_net",
        &[
            (t0("cv.net.type"), t0(ty)),
            (t0("cv.net.minslope"), format!("{} %", fixed(numf(r.and_then(|r| r.get("minSlope"))) * 100.0, 2))),
            (t0("cv.net.mincover"), format!("{} {u}", fixed(numf(r.and_then(|r| r.get("minCover"))), 2))),
            (t0("cv.net.mindrop"), format!("{} {u}", fixed(numf(r.and_then(|r| r.get("minDrop"))), 2))),
        ],
    );
    note(
        ui,
        tk,
        &tr!(
            "cv.net.count",
            structures = tr!("cv.net.structures", n = arr(n.get("structures")).len()),
            pipes = tr!("cv.net.pipes", n = arr(n.get("pipes")).len())
        ),
    );
    let probs: Vec<(Option<String>, String, String)> = cv
        .facts
        .network
        .as_ref()
        .map(|c| c.problems.iter().map(|p| (p.id.clone(), p.kind.to_string(), p.msg.clone())).collect())
        .unwrap_or_default();
    if probs.is_empty() {
        note(ui, tk, &t0("cv.net.ok"));
    } else {
        for (id, kind, msg) in probs.iter().take(40) {
            match id {
                Some(id) => {
                    if link(ui, tk, msg, false) {
                        acts.push(Act::Select(Some(if kind == "pipe" { Sel::Pipe(id.clone()) } else { Sel::Structure(id.clone()) })));
                    }
                }
                None => {
                    ui.label(RichText::new(msg).color(tk.text));
                }
            }
        }
    }
}

fn alignment(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, a: &Value) {
    let units = cv.project.units().to_string();
    let u = cc_civil::units::unit_of(&units).short.to_string();
    let al = cv.project.alignment(a);
    head(ui, tk, &t0("sec.alignment"));
    let mut r = vec![
        (t0("fl.props.name"), string_or_empty(a.get("name"))),
        (t0("cv.al.start"), format_station(numf(a.get("start")), &units, None)),
    ];
    if al.ok() {
        r.push((t0("fl.props.length"), format!("{} {u}", fixed(al.length, 2))));
        r.push((t0("cv.al.end"), format_station(al.end, &units, None)));
    }
    rows(ui, tk, "cv_al", &r);
    report(ui, tk, &al.errors, true);
    head(ui, tk, &t0("cv.al.pis"));
    egui::Grid::new("cv_al_pis").num_columns(4).spacing([8.0, 2.0]).show(ui, |ui| {
        for h in ["#".to_string(), t0("pt.props.easting"), t0("pt.props.northing"), t0("se.c.r")] {
            ui.label(RichText::new(h).small().color(tk.text_dim));
        }
        ui.end_row();
        let pis = arr(a.get("pis"));
        for (i, p) in pis.iter().enumerate() {
            ui.label(RichText::new((i + 1).to_string()).color(tk.text));
            ui.label(RichText::new(fixed(numf(p.get("x")), 2)).monospace().color(tk.text));
            ui.label(RichText::new(fixed(numf(p.get("y")), 2)).monospace().color(tk.text));
            let end = i == 0 || i + 1 == pis.len();
            ui.label(RichText::new(if end { "—".into() } else { fixed(numf(p.get("r")), 2) }).monospace().color(tk.text));
            ui.end_row();
        }
    });
    if al.ok() {
        let arcs: Vec<&cc_civil::alignment::Arc> =
            al.elements.iter().filter_map(|e| if let cc_civil::alignment::Element::Arc(c) = e { Some(c) } else { None }).collect();
        if !arcs.is_empty() {
            head(ui, tk, &t0("cv.al.curves"));
            for c in arcs {
                ui.label(
                    RichText::new(tr!(
                        "cv.al.curve.sta",
                        pc = format_station(c.sta, &units, None),
                        pt = format_station(c.sta + c.len, &units, None)
                    ))
                    .color(tk.text_strong),
                );
                ui.label(
                    RichText::new(tr!(
                        if c.ccw { "cv.al.curve.left" } else { "cv.al.curve.right" },
                        r = fixed(c.r, 2),
                        delta = dms(c.delta.to_degrees())
                    ))
                    .color(tk.text),
                );
                ui.label(
                    RichText::new(tr!(
                        "cv.al.curve.parts",
                        len = fixed(c.len, 2),
                        tan = fixed(c.t, 2),
                        ext = fixed(c.e, 2),
                        chord = fixed(c.chord, 2)
                    ))
                    .small()
                    .color(tk.text_dim),
                );
            }
            note(ui, tk, &t0("cv.al.curves.note"));
        }
        if let Some(e) = al.elements.first() {
            let dir = match e {
                cc_civil::alignment::Element::Line { dir, .. } => *dir,
                cc_civil::alignment::Element::Arc(c) => c.dir_in,
            };
            note(ui, tk, &tr!("cv.al.tangent", b = bearing(dir)));
        }
        // profile: design PVIs with grades, and the existing ground's range along it
        head(ui, tk, &t0("cv.prof.head"));
        let prof = cv.project.design_profile(a);
        let egp = cv.project.eg_profile(a);
        let zs: Vec<f64> = egp.iter().filter_map(|s| s.z).collect();
        if !zs.is_empty() {
            let (lo, hi) = zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(*z), b.max(*z)));
            note(ui, tk, &format!("{}: {}", t0("surface.eg"), tr!("cv.range", a = fixed(lo, 2), b = fixed(hi, 2))));
        }
        let pvis = arr(a.get("pvis"));
        egui::Grid::new("cv_al_pvis").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
            for (i, v) in pvis.iter().enumerate() {
                ui.label(RichText::new(format_station(numf(v.get("sta")), &units, None)).monospace().color(tk.text));
                ui.label(RichText::new(fixed(numf(v.get("z")), 2)).monospace().color(tk.text));
                let g = prof.grades.get(i).map_or(String::new(), |g| pct(g.g, 2));
                ui.label(RichText::new(g).color(tk.text_dim));
                ui.end_row();
            }
        });
        report(ui, tk, &prof.errors, true);
    }
}

fn pad(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, d: &Value) {
    let units = cv.project.units().to_string();
    let u = cc_civil::units::unit_of(&units).short.to_string();
    head(ui, tk, &t0("cv.pad"));
    let pts = cc_civil::geo::read_p2s(d.get("pts"));
    rows(
        ui,
        tk,
        "cv_pad",
        &[
            (t0("fl.props.name"), string_or_empty(d.get("name"))),
            (t0("pt.props.elev"), format!("{} {u}", fixed(numf(d.get("z")), 2))),
            (t0("fl.props.cut"), format!("{} : 1 (H:V)", cc_civil::js::num_to_string(numf(d.get("cut"))))),
            (t0("fl.props.fill"), format!("{} : 1 (H:V)", cc_civil::js::num_to_string(numf(d.get("fill"))))),
            (t0("cv.pad.area"), area_text(cc_civil::tin::poly_area(&pts).abs(), &units, 2)),
            (t0("dr.c.corners"), pts.len().to_string()),
        ],
    );
    let g = cv.project.graded();
    if let Some(it) = g.items.iter().find(|x| x.kind == "pad" && x.id == d.get("id").cloned().unwrap_or(Value::Null)) {
        if it.problems.is_empty() {
            let rays = arr(it.info.get("rays"));
            let cut = rays.iter().filter(|r| r.get("kind").and_then(Value::as_str) == Some("cut")).count();
            let far = rays.iter().map(|r| numf(r.get("d"))).fold(0.0f64, f64::max);
            note(ui, tk, &tr!("cv.pad.daylight", n = rays.len(), cut = cut, fill = rays.len() - cut, d = fixed(far, 2), u = u.as_str()));
        } else {
            report(ui, tk, &it.problems, true);
        }
    }
    volumes(cv, ui, tk);
}

fn structure(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, x: &Value) {
    let u = cc_civil::units::unit_of(cv.project.units()).short.to_string();
    let id = string_of(x.get("id"));
    head(ui, tk, &t0("cv.st"));
    let info = cv.facts.network.as_ref().and_then(|c| c.structs.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone()));
    let rim_used = info.as_ref().and_then(|i| i.rim).map_or("—".to_string(), |r| {
        if info.as_ref().is_some_and(|i| i.rim_from == Some("surface")) { tr!("cv.st.rim.from", z = fixed(r, 2)) } else { fixed(r, 2) }
    });
    rows(
        ui,
        tk,
        "cv_st",
        &[
            (t0("fl.props.name"), string_or_empty(x.get("name"))),
            (t0("cv.st.rim"), x.get("rim").and_then(Value::as_f64).map_or("—".into(), |r| format!("{} {u}", fixed(r, 2)))),
            (t0("cv.st.sump"), format!("{} {u}", fixed(numf(x.get("sumpDepth")), 2))),
            (t0("cv.diameter"), cc_civil::js::num_to_string(numf(x.get("dia")))),
            (t0("cv.st.rim.used"), rim_used),
            (t0("cv.st.sump.z"), info.as_ref().and_then(|i| i.sump).map_or("—".into(), |s| fixed(s, 2))),
            (t0("pt.props.easting"), fixed(numf(x.get("x")), 2)),
            (t0("pt.props.northing"), fixed(numf(x.get("y")), 2)),
        ],
    );
    let pipes = cv.project.state.get("network").map(|n| arr(n.get("pipes")).to_vec()).unwrap_or_default();
    for p in &pipes {
        let z = |k: &str| p.get(k).and_then(Value::as_f64).map_or("—".into(), |v| fixed(v, 2));
        if p.get("to").and_then(Value::as_str) == Some(id.as_str()) {
            note(ui, tk, &tr!("cv.st.in", pipe = string_or_empty(p.get("name")), z = z("invDn")));
        }
        if p.get("from").and_then(Value::as_str) == Some(id.as_str()) {
            note(ui, tk, &tr!("cv.st.out", pipe = string_or_empty(p.get("name")), z = z("invUp")));
        }
    }
    problems_of(cv, ui, tk, &id);
}

fn problems_of(cv: &CivilView, ui: &mut egui::Ui, tk: &Tokens, id: &str) -> bool {
    let probs: Vec<String> = cv
        .facts
        .network
        .as_ref()
        .map(|c| c.problems.iter().filter(|p| p.id.as_deref() == Some(id)).map(|p| p.msg.clone()).collect())
        .unwrap_or_default();
    report(ui, tk, &probs, true);
    !probs.is_empty()
}

fn pipe(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, p: &Value) {
    let u = cc_civil::units::unit_of(cv.project.units()).short.to_string();
    let id = string_of(p.get("id"));
    head(ui, tk, &t0("dr.r.pipe"));
    let ss = cv.project.state.get("network").map(|n| arr(n.get("structures")).to_vec()).unwrap_or_default();
    let nm = |k: &str| ss.iter().find(|s| s.get("id") == p.get(k)).map_or("?".into(), |s| string_or_empty(s.get("name")));
    let info = cv.facts.network.as_ref().and_then(|c| c.pipes.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone()));
    let inv = |k: &str| p.get(k).and_then(Value::as_f64).map_or("—".into(), |v| format!("{} {u}", fixed(v, 2)));
    rows(
        ui,
        tk,
        "cv_pp",
        &[
            (t0("fl.props.name"), string_or_empty(p.get("name"))),
            (t0("cv.pp.fromto"), format!("{} → {}", nm("from"), nm("to"))),
            (t0("cv.diameter"), cc_civil::js::num_to_string(numf(p.get("dia")))),
            (t0("cv.pp.up"), inv("invUp")),
            (t0("cv.pp.dn"), inv("invDn")),
            (t0("fl.props.length"), info.as_ref().map_or("—".into(), |i| format!("{} {u}", fixed(i.len, 2)))),
            (t0("cv.pp.slope"), info.as_ref().and_then(|i| i.slope).map_or("—".into(), |s| pct(s, 2))),
            (t0("cv.net.mincover"), info.as_ref().and_then(|i| i.cover).map_or("—".into(), |c| format!("{} {u}", fixed(c, 2)))),
        ],
    );
    if !problems_of(cv, ui, tk, &id) {
        note(ui, tk, &t0("cv.pp.ok"));
    }
}

fn point(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, p: &cc_civil::points::Pt) {
    head(ui, tk, &tr!("cv.pt.head", n = p.n.as_str()));
    let keys = cv.project.state_keys();
    let key_of = cc_civil::desckeys::grouper(&keys);
    rows(
        ui,
        tk,
        "cv_pt",
        &[
            (t0("pt.props.number"), p.n.clone()),
            (t0("pt.props.easting"), fixed(p.x, 3)),
            (t0("pt.props.northing"), fixed(p.y, 3)),
            (t0("pt.props.elev"), p.z.map_or_else(|| t0("pt.props.elev.none"), |z| fixed(z, 3))),
            (t0("pt.props.desc"), p.d.clone()),
            (t0("pt.props.group"), key_of(p)),
        ],
    );
    if let Some(z) = cv.project.eg().surface.as_ref().and_then(|s| s.z_at(p.x, p.y)) {
        note(ui, tk, &tr!("cv.read.ground", z = fixed(z, 2)));
    }
}

fn feature(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, f: &Value) {
    let units = cv.project.units().to_string();
    let u = cc_civil::units::unit_of(&units).short.to_string();
    let fl = cc_civil::featureline::read_feature(f);
    head(ui, tk, &t0("fl.props.head"));
    let segs = cc_civil::featureline::segments(&fl);
    let len: f64 = cc_civil::featureline::stations(&fl).last().copied().unwrap_or(0.0);
    rows(
        ui,
        tk,
        "cv_fl",
        &[
            (t0("fl.props.name"), fl.name.clone()),
            (t0("fl.props.use"), t0(&format!("fl.use.{}", fl.use_))),
            (t0("fl.props.length"), format!("{} {u}", fixed(len, 2))),
        ],
    );
    egui::Grid::new("cv_fl_v").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        for (i, q) in fl.pts.iter().enumerate() {
            ui.label(RichText::new((i + 1).to_string()).color(tk.text));
            ui.label(RichText::new(fixed(q.z, 2)).monospace().color(tk.text));
            let g = segs.get(i).and_then(|s| s.grade).map_or(String::new(), cc_civil::featureline::grade_label);
            ui.label(RichText::new(g).color(tk.text_dim));
            ui.end_row();
        }
    });
}

fn parcel(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, f: &Value) {
    let units = cv.project.units().to_string();
    let pc = cc_civil::parcels::read_parcel(f);
    head(ui, tk, &t0("pc.props.head"));
    let u = cc_civil::units::unit_of(&units).short.to_string();
    rows(
        ui,
        tk,
        "cv_pc",
        &[
            (t0("fl.props.name"), pc.name.clone()),
            (t0("cv.pad.area"), cc_civil::parcels::area_words(cc_civil::parcels::parcel_area(&pc.pts), &units)),
            (t0("pc.perimeter"), format!("{} {u}", fixed(cc_civil::parcels::perimeter(&pc.pts), 2))),
        ],
    );
    ui.add_space(4.0);
    note(ui, tk, &cc_civil::parcels::legal_description(&pc, &units, None));
}
