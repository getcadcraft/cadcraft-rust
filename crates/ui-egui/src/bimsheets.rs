//! BimCraft's Sheets (res-ui.js `openSheets` / `makeSet`, 0.5 part 4 RC.5): the drawing set made from the model as it
//! is now — plans of each level, the four exterior elevations, the section lines' sections, the schedules, framing
//! elevations — on sheets of the paper chosen; a preview of each and the whole set as one PDF. The drawing is
//! `cc_bim::draw::resviews` and `ressheets` (bmrestooldiff.mjs compares them with the JS at parity).
//!
//! Commands: `bim.res` with `sheets` (open), `sheets.paper:<letter|tabloid|archd|a3|a1>`, `sheets.show:<n>`,
//! `sheets.pdf`, `sheets.close`.

use std::collections::HashSet;

use cc_bim::draw::plan::{Scene, viewer_to_plan};
use cc_bim::draw::ressheets::{Align, FrameBox, Item, PAPERS, SetInput, Sheet, build_set, paper, set_pdf, text_w};
use cc_bim::draw::resviews::{CutAt, ELEVATION_DIRS, View, model_triangles, project_view};
use cc_bim::draw::schedule::Schedule;
use cc_bim::draw::writer::W;
use egui::{Color32, Pos2, RichText, Stroke};

use crate::bimtools::bt;
use crate::{Params, UiApp};

/// Drawings with their titles.
type Titled = Vec<(String, View)>;

/// The dialog: the paper, the sheets drawn, the one shown.
#[derive(Clone, Debug)]
pub struct SheetsDlg {
    pub paper: String,
    pub sheets: Vec<Sheet>,
    pub shown: usize,
}

fn base_name(app: &UiApp) -> String {
    let name = app.ifc_name().to_string();
    if name.to_ascii_lowercase().ends_with(".ifc") { name.get(..name.len().saturating_sub(4)).unwrap_or(&name).to_string() } else { name }
}

/// `sceneFor(level)`: the plan scene of a level (the plan made for it, then the level shown again).
fn scene_for(app: &mut UiApp, level: f64) -> Option<Scene> {
    let keep = app.bimh.draw.level;
    if keep == Some(level) {
        return Some(app.bimh.draw.scene.clone());
    }
    app.bimh.draw.level = Some(level);
    crate::bimplan::refresh(app, false);
    let sc = app.bimh.draw.scene.clone();
    app.bimh.draw.level = keep;
    crate::bimplan::refresh(app, false);
    Some(sc)
}

/// `makeSet(paper)`: every drawing of the set, from the model as it is now.
fn make_set(app: &mut UiApp, paper_name: &str) -> Vec<Sheet> {
    let Some(sn) = app.bimh.draw.data.clone() else { return Vec::new() };
    if sn.levels.is_empty() || app.bimh.draw.model.is_none() {
        return Vec::new();
    }
    let units = app.bimh.draw.units.clone();
    let ground = sn.levels.iter().find(|l| l.elevation.abs() < 1.0).or(sn.levels.first()).map(|l| l.id).unwrap_or(0.0);
    // framing has its own sheets; inside walls it only clutters
    let mut skip = HashSet::new();
    for f in &app.bimh.draw.res.frames {
        skip.extend(f.f.parts.iter().map(|p| cc_bim::draw::plan::key_u64(*p)));
    }
    // plans of the levels with walls, and the ground's
    let mut plans: Vec<(String, Scene)> = Vec::new();
    for lv in &sn.levels {
        if !(sn.walls.iter().any(|w| w.storey == Some(lv.id)) || lv.id == ground) {
            continue;
        }
        if let Some(sc) = scene_for(app, lv.id) {
            plans.push((bt("res.sh.plan", &[("level", &lv.name)]), sc));
        }
    }
    // elevations and sections from the model's triangles
    let secs = app.bimh.draw.res.secs.clone();
    let storey_m = |app: &mut UiApp, st: f64| -> Option<cc_bim::util::M4> {
        let doc = app.bimh.ed.as_mut()?;
        W::new(&mut doc.ed.api).ok()?.storey_world(st).ok()
    };
    let gm = storey_m(app, ground);
    let mut sec_m = Vec::new();
    for sl in &secs {
        sec_m.push(sl.storey.and_then(|s| storey_m(app, s)));
    }
    let (els, sections): (Titled, Titled) = {
        let Some((_, model)) = app.bimh.draw.model.as_ref() else { return Vec::new() };
        let tris = |sw: Option<cc_bim::util::M4>| {
            sw.and_then(|sw| viewer_to_plan(&model.coord, &sw)).map(|mm| model_triangles(model, &mm, &skip)).unwrap_or_default()
        };
        let g = tris(gm);
        let els = ELEVATION_DIRS
            .iter()
            .map(|(side, dir)| (bt(&format!("res.sh.elev.{side}"), &[]), project_view(&g, *dir, None, 1600.0, [3000.0, 1500.0, 500.0])))
            .collect();
        let sections = secs
            .iter()
            .zip(sec_m)
            .map(|(sl, mw)| {
                let (o, dir, _) = cc_bim::draw::resops::section_dir(sl);
                let v = project_view(&tris(mw), dir, Some(CutAt { o, far: sl.depth }), 1600.0, [3000.0, 1500.0, 500.0]);
                (bt("res.sec.name", &[("name", &sl.name)]), v)
            })
            .collect();
        (els, sections)
    };
    // schedules: doors, windows, rooms, devices, circuits, lumber (those with rows)
    let mut schedules: Vec<(String, Schedule)> = Vec::new();
    let all = crate::bimplan::schedule_data_pub(app);
    for kind in ["doors", "windows", "rooms", "devices", "circuits", "lumber"] {
        let sc = match kind {
            "devices" | "circuits" | "lumber" => crate::bimres::schedule(app, kind),
            _ => all.as_ref().and_then(|a| a.get(kind)).cloned(),
        };
        let title = match kind {
            "lumber" => bt("res.fr.sch.lumber", &[]),
            "devices" => bt("res.el.sch.devicesched", &[]),
            "circuits" => bt("res.el.sch.circuitsched", &[]),
            k => bt(&format!("model.sch.{k}"), &[]),
        };
        if let Some(sc) = sc.filter(|s| !s.rows.is_empty()) {
            schedules.push((title, sc));
        }
    }
    let framing: Vec<(String, Vec<FrameBox>)> = app
        .bimh
        .draw
        .res
        .frames
        .iter()
        .filter(|f| f.f.kind == "wall" && !f.boxes.is_empty())
        .map(|f| {
            let len = sn.walls.iter().find(|w| w.guid == f.f.host).map_or(0.0, |w| w.len);
            let level = sn.levels.iter().find(|l| Some(l.id) == f.f.storey).map(|l| l.name.clone()).unwrap_or_default();
            (bt("res.sh.framewall", &[("len", &cc_bim::draw::units::fmt_len(len, &units)), ("level", &level)]), f.boxes.clone())
        })
        .collect();
    let input = SetInput {
        plans: plans.iter().map(|(t, s)| (t.clone(), s)).collect(),
        elevations: els.iter().map(|(t, v)| (t.clone(), v)).collect(),
        sections: sections.iter().map(|(t, v)| (t.clone(), v)).collect(),
        schedules: schedules.iter().map(|(t, s)| (t.clone(), s)).collect(),
        framing: framing.iter().map(|(t, b)| (t.clone(), b.as_slice())).collect(),
    };
    let date = cc_engine::cmd::layout::iso_date(cc_clock::now_ms());
    build_set(&input, paper_name, &units, &base_name(app), &date)
}

fn fill(app: &mut UiApp) {
    let paper = app.bimh.draw.res.sheets.as_ref().map(|d| d.paper.clone()).unwrap_or_default();
    let sheets = make_set(app, &paper);
    if sheets.is_empty() {
        app.message = bt("res.sh.none", &[]);
    }
    if let Some(d) = app.bimh.draw.res.sheets.as_mut() {
        d.sheets = sheets;
        d.shown = 0;
    }
}

/// `bim.res=sheets…`
pub fn run(app: &mut UiApp, name: &str) -> Result<(), String> {
    match name {
        "sheets" => {
            if !app.bimh.draw.on {
                return Err(bt("model.needs.ifc", &[]));
            }
            let paper = app
                .bimh
                .draw
                .res
                .sheets
                .as_ref()
                .map_or_else(|| if app.bimh.draw.units == "mm" { "a3".into() } else { "tabloid".into() }, |d| d.paper.clone());
            app.bimh.draw.res.sheets = Some(SheetsDlg { paper, sheets: Vec::new(), shown: 0 });
            fill(app);
        }
        "sheets.close" => app.bimh.draw.res.sheets = None,
        "sheets.pdf" => {
            let Some(d) = app.bimh.draw.res.sheets.clone() else { return Err("bim.res: the sheets are not open".into()) };
            if d.sheets.is_empty() {
                return Ok(());
            }
            let base = base_name(app);
            let base = if base.is_empty() { "model".to_string() } else { base };
            let pdf = set_pdf(&d.sheets, &d.paper, &base, None);
            let file = format!("{base} — {}.pdf", bt("res.sh.set", &[]));
            let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
            if save(&file, pdf)?.is_some() {
                app.message = bt("res.sh.done", &[("n", &d.sheets.len().to_string())]);
            }
        }
        _ => {
            if let Some(p) = name.strip_prefix("sheets.paper:") {
                if !PAPERS.iter().any(|x| x.0 == p) {
                    return Err("bim.res: sheets.paper letter, tabloid, archd, a3 or a1".into());
                }
                if let Some(d) = app.bimh.draw.res.sheets.as_mut() {
                    d.paper = p.into();
                }
                fill(app);
            } else if let Some(i) = name.strip_prefix("sheets.show:") {
                let i = i.parse::<usize>().map_err(|_| "bim.res: sheets.show:<n>")?;
                if let Some(d) = app.bimh.draw.res.sheets.as_mut() {
                    d.shown = i.min(d.sheets.len().saturating_sub(1));
                }
            } else {
                return Err("bim.res: sheets, sheets.paper:<paper>, sheets.show:<n>, sheets.pdf or sheets.close".into());
            }
        }
    }
    Ok(())
}

/// Paint a sheet's items into `rect` (paper white, ink black: a print preview).
fn paint_sheet(ui: &egui::Ui, rect: egui::Rect, sh: &Sheet, paper_name: &str) {
    let [pw, ph] = paper(paper_name);
    let k = (f64::from(rect.width()) / pw).min(f64::from(rect.height()) / ph);
    let ox = f64::from(rect.left()) + (f64::from(rect.width()) - pw * k) / 2.0;
    let oy = f64::from(rect.top()) + (f64::from(rect.height()) - ph * k) / 2.0;
    #[allow(clippy::cast_possible_truncation)]
    let p = |q: [f64; 2]| Pos2::new((ox + q[0] * k) as f32, (oy + (ph - q[1]) * k) as f32);
    let painter = ui.painter_at(rect);
    painter.rect_filled(egui::Rect::from_min_max(p([0.0, ph]), p([pw, 0.0])), 0.0, Color32::WHITE);
    let grey = |g: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        Color32::from_gray(v)
    };
    #[allow(clippy::cast_possible_truncation)]
    let px = |mm: f64| ((mm * k) as f32).max(0.5);
    for it in &sh.items {
        match it {
            Item::Fill { polys, g } => {
                for pl in polys {
                    let pts: Vec<Pos2> = pl.iter().map(|q| p(*q)).collect();
                    crate::bimplan::fill_poly(&painter, &pts, grey(*g));
                }
            }
            Item::Line { pts, w, g, dash } => {
                let pts: Vec<Pos2> = pts.iter().map(|q| p(*q)).collect();
                let st = Stroke::new(px(*w), grey(*g));
                match dash {
                    Some(d) => {
                        painter.extend(egui::Shape::dashed_line(&pts, st, px(d[0]).max(1.0), px(d[1]).max(1.0)));
                    }
                    None => {
                        painter.add(egui::Shape::line(pts, st));
                    }
                }
            }
            Item::Text { s, x, y, size, align, bold, rot, g } => {
                // pt → sheet mm → screen
                let mm = size * 25.4 / 72.0;
                #[allow(clippy::cast_possible_truncation)]
                let font = egui::FontId::proportional(((mm * k) as f32).max(1.0));
                let w = text_w(s, *size) * 25.4 / 72.0;
                let dx = match align {
                    Align::Left => 0.0,
                    Align::Right => -w,
                    Align::Center => -w / 2.0,
                };
                let at = [x + dx * rot.cos(), y + dx * rot.sin()];
                let galley = painter.layout_no_wrap(s.clone(), font, grey(*g));
                let _ = bold;
                #[allow(clippy::cast_possible_truncation)]
                let mut shape = egui::epaint::TextShape::new(p(at) - egui::vec2(0.0, galley.size().y * 0.8), galley, grey(*g));
                #[allow(clippy::cast_possible_truncation)]
                {
                    shape = shape.with_angle(-(*rot as f32));
                }
                painter.add(shape);
            }
        }
    }
}

/// The Sheets dialog (ui.inc `[data-res-sheets-dlg]`).
pub fn dialog(app: &mut UiApp, ctx: &egui::Context) {
    let Some(d) = app.bimh.draw.res.sheets.clone() else { return };
    let t = app.tokens;
    let mut open = true;
    let mut act: Option<String> = None;
    crate::shell::Dialog::new(bt("res.sh.title", &[]))
        .id(egui::Id::unique("bim_sheets"))
        .open(&mut open)
        .resizable(true)
        .default_size([960.0, 640.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                crate::shell::note(ui, &t, &bt("res.sh.paper", &[]));
                let label = |p: &str| match p {
                    "letter" | "tabloid" | "archd" => bt(&format!("res.sh.paper.{p}"), &[]),
                    other => other.to_ascii_uppercase(),
                };
                egui::ComboBox::from_id_salt("bim_sheets_paper").selected_text(label(&d.paper)).show_ui(ui, |ui| {
                    for (p, _) in PAPERS {
                        if ui.selectable_label(d.paper == p, label(p)).clicked() {
                            act = Some(format!("sheets.paper:{p}"));
                        }
                    }
                });
                if ui.add_enabled(!d.sheets.is_empty(), crate::shell::Btn::primary(bt("res.sh.pdf", &[]))).clicked() {
                    act = Some("sheets.pdf".into());
                }
                if ui.add(crate::shell::Btn::new(bt("res.sh.close", &[]))).clicked() {
                    act = Some("sheets.close".into());
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    if d.sheets.is_empty() {
                        crate::shell::note(ui, &t, &bt("res.sh.none", &[]));
                    }
                    egui::ScrollArea::vertical().id_salt("bim_sheets_list").show(ui, |ui| {
                        for (i, s) in d.sheets.iter().enumerate() {
                            let text = RichText::new(format!("{}  {}", s.number, s.title));
                            if ui.selectable_label(d.shown == i, text).clicked() {
                                act = Some(format!("sheets.show:{i}"));
                            }
                        }
                    });
                });
                let size = ui.available_size().max(egui::vec2(200.0, 200.0));
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                if let Some(sh) = d.sheets.get(d.shown) {
                    paint_sheet(ui, rect, sh, &d.paper);
                }
            });
        });
    if !open {
        act = Some("sheets.close".into());
    }
    if let Some(a) = act {
        app.run_ui("bim.res", &Params { name: Some(a), ..Default::default() });
    }
}

/// The model bar's Takeoff (res-ui.js `takeoffClick`): every quantity marked on a plan of each level, the takeoff
/// inside the PDF (TakeoffCraft opens it), saved or downloaded.
pub fn takeoff(app: &mut UiApp) -> Result<(), String> {
    let Some(sn) = app.bimh.draw.data.clone() else { return Err(bt("model.needs.ifc", &[])) };
    let units = app.bimh.draw.units.clone();
    let mut scenes = Vec::new();
    for lv in &sn.levels {
        if let Some(sc) = scene_for(app, lv.id) {
            scenes.push((lv.id, sc));
        }
    }
    let base = base_name(app);
    let base = if base.is_empty() { "model".to_string() } else { base };
    let doc = app.bimh.ed.as_mut().ok_or_else(|| bt("model.needs.ifc", &[]))?;
    let mut d = W::new(&mut doc.ed.api)
        .and_then(|mut w| cc_bim::draw::restakeoff::gather(&mut w, &sn, &units))
        .map_err(|e| bt("res.to.failed", &[("why", &e.0)]))?;
    d.scenes = scenes;
    d.date = cc_engine::cmd::layout::iso_date(cc_clock::now_ms());
    d.name = format!("{base} — {}", bt("res.to.button", &[]).to_lowercase());
    let mut input = cc_bim::draw::restakeoff::model_quantities(&d);
    input.paper = app.bimh.draw.res.sheets.as_ref().map(|s| s.paper.clone());
    let Some(b) = cc_bim::draw::restakeoff::build_takeoff(&input) else { return Err(bt("res.to.none", &[])) };
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    if let Some(to) = save(&b.name, b.pdf)? {
        app.message = bt("res.to.saved", &[("file", &to)]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::{Params, Services, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: (!name.is_empty()).then(|| name.to_string()), ..Default::default() })
    }

    #[test]
    fn a_house_gets_its_sheets_and_a_takeoff_takeoffcraft_reads() {
        type Saved = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
        let saved: Saved = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&saved);
        let services = Services {
            save: Some(Box::new(move |name, bytes| {
                sink.lock().unwrap().push((name.to_string(), bytes));
                Ok(Some(name.to_string()))
            })),
            ..Default::default()
        };
        let mut app = UiApp::new(services);
        for (id, n) in [("bim.clock", "1791979200000"), ("bim.new", "House|2|9'"), ("bim.draw", "on"), ("bim.tool", "wall")] {
            run(&mut app, id, n).unwrap();
        }
        for p in ["0,0", "9144,0", "9144,6096", "0,6096", "0,0"] {
            run(&mut app, "bim.point", p).unwrap();
        }
        for (id, n) in [("bim.tool", "foundation"), ("bim.point", "4572,3048"), ("bim.tool", "electrical"), ("bim.point", "3000,200")] {
            run(&mut app, id, n).unwrap();
        }
        assert_eq!(app.bimh.draw.res.founds.len(), 1);
        assert_eq!(app.bimh.draw.res.devs.len(), 1);
        run(&mut app, "bim.res", "sheets").unwrap();
        let n = app.bimh.draw.res.sheets.as_ref().map_or(0, |d| d.sheets.len());
        assert!(n >= 3, "{n} sheets");
        run(&mut app, "bim.res", "sheets.pdf").unwrap();
        run(&mut app, "bim.res", "takeoff").unwrap();
        let files = saved.lock().unwrap().clone();
        assert_eq!(files.len(), 2);
        assert!(files[0].1.starts_with(b"%PDF-1.4"));
        // the takeoff is inside the PDF, as TakeoffCraft reads it
        let doc = cc_io_takeoff::pdfread::open(&files[1].1).unwrap();
        let (stands_for, tk) = cc_io_takeoff::embed::read_embedded(&doc).expect("an embedded takeoff");
        assert_eq!(stands_for.len(), 64);
        assert!(tk.get("marks").and_then(|m| m.as_array()).is_some_and(|m| !m.is_empty()));
    }
}
