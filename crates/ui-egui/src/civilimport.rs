//! The Import points dialog (civilcraft.js `importPoints`, ui.inc `data-import`): a survey point file opened or
//! dropped is read with the chosen column order (Detect, PNEZD, PENZD, PNEZ, PENZ, NEZD, ENZD, NEZ, ENZ), the dialog
//! says what it found (how the order was decided, the delimiter, rows not read) and shows the first six points; Import
//! replaces the project's points (the surface is rebuilt from them), Cancel changes nothing. Latitude / longitude
//! columns need a coordinate system: without one the points are not imported (0.5's words). `civil.import` drives it.

use std::sync::Arc;

use cc_civil::i18n::t;
use cc_civil::points::{Parsed, parse_points};
use cc_civil::tr;
use cc_civil::units::{fixed, grouped};
use egui::RichText;
use serde_json::Value;

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// The column orders the dialog offers (value, label key or the code itself).
pub const FORMATS: [&str; 9] = ["auto", "PNEZD", "PENZD", "PNEZ", "PENZ", "NEZD", "ENZD", "NEZ", "ENZ"];

/// The open dialog: the file, the chosen order and what it reads.
#[derive(Clone)]
pub struct ImportDlg {
    pub name: String,
    pub text: Arc<String>,
    pub format: String,
    pub res: Arc<Parsed>,
}

impl ImportDlg {
    pub fn new(name: &str, text: String, prefer: &str) -> ImportDlg {
        let text = Arc::new(text);
        let res = Arc::new(parse_points(&text, "auto", prefer));
        ImportDlg { name: name.to_string(), text, format: "auto".into(), res }
    }
}

fn format_label(f: &str) -> String {
    match f {
        "auto" => t0("ui.imp.detect"),
        "PNEZD" => t0("ui.imp.pnezd"),
        "PENZD" => t0("ui.imp.penzd"),
        o => o.to_string(),
    }
}

impl CivilView {
    /// `parse()`: the dialog's summary line (data-imp-summary).
    pub fn import_summary(&self) -> Option<String> {
        let d = self.import.as_ref()?;
        let res = &d.res;
        if res.points.is_empty() {
            return Some(t("cv.imp.none", &[]));
        }
        let delim = match res.delimiter.as_deref() {
            Some(",") => t("cv.imp.delim.comma", &[]),
            Some("\t") => t("cv.imp.delim.tab", &[]),
            Some(";") => t("cv.imp.delim.semi", &[]),
            Some("ws") => t("cv.imp.delim.space", &[]),
            _ => "—".into(),
        };
        let key = match res.format_from.as_str() {
            "header" => "cv.imp.read.header",
            "chosen" => "cv.imp.read.chosen",
            _ => "cv.imp.read.guessed",
        };
        let n = res.points.len();
        let mut s = tr!(
            key,
            n = n,
            count = grouped(n as f64, 0),
            format = res.format.clone().unwrap_or_default(),
            order = self.pt_settings.order.as_str(),
            delim = delim
        );
        if res.bad_count > 0 {
            s.push(' ');
            s.push_str(&tr!("cv.imp.bad", n = res.bad_count, count = grouped(res.bad_count as f64, 0)));
        }
        if res.latlon {
            s.push(' ');
            match self.project.state.get("crs").filter(|c| c.is_object()) {
                Some(c) => s.push_str(&tr!("cv.imp.latlon.crs", crs = cc_civil::js::string_of(c.get("name")))),
                None => s.push_str(&t("cv.imp.latlon.pick", &[])),
            }
        }
        Some(s)
    }

    /// The preview's rows (first six: point, northing, easting, elevation, description) and the rows not read.
    pub fn import_rows(&self) -> (Vec<[String; 5]>, Vec<String>) {
        let Some(d) = self.import.as_ref() else { return (Vec::new(), Vec::new()) };
        let rows = d
            .res
            .points
            .iter()
            .take(6)
            .map(|p| [p.n.clone(), fixed(p.y, 3), fixed(p.x, 3), p.z.map_or("—".into(), |z| fixed(z, 3)), p.d.clone()])
            .collect();
        let mut bad = Vec::new();
        if !d.res.bad.is_empty() {
            bad.push(t("cv.imp.bad.head", &[]));
            for b in d.res.bad.iter().take(25) {
                let text = cc_civil::js::slice16_range(&b.text, 0, Some(60));
                bad.push(tr!("cv.imp.bad.line", line = b.line, why = b.why.as_str(), text = text));
            }
            if d.res.bad_count > 25 {
                bad.push(tr!("cv.imp.bad.more", count = grouped((d.res.bad_count - 25) as f64, 0)));
            }
        }
        (rows, bad)
    }

    /// `civil.import=format=<order>`: the Columns choice (the file read again).
    pub fn import_format(&mut self, f: &str) -> Result<(), String> {
        if !FORMATS.contains(&f) {
            return Err(format!("civil.import: format = {} (not {f})", FORMATS.join(", ")));
        }
        let prefer = self.pt_settings.order.clone();
        let d = self.import.as_mut().ok_or("civil.import: the import dialog is not open")?;
        d.format = f.to_string();
        d.res = Arc::new(parse_points(&d.text, f, &prefer));
        Ok(())
    }

    /// Import: the project's points replaced by the file's (civilcraft.js data-imp-ok).
    pub fn import_ok(&mut self) -> Result<(), String> {
        let d = self.import.take().ok_or("civil.import: the import dialog is not open")?;
        if d.res.points.is_empty() {
            self.import = Some(d);
            return Err("civil.import: no points to import".into());
        }
        if d.res.latlon {
            // gis-vector-ui.js pointsLatLon converts through the project's system; without one nothing is imported
            self.message = tr!("cv.imp.latlon.need", file = d.name.as_str());
            return Ok(());
        }
        let pts = d.res.points.clone();
        let replacing = !self.project.points.is_empty();
        if let Some(s) = self.project.state.get_mut("surface").and_then(Value::as_object_mut) {
            s.insert("tin".into(), Value::Null);
        }
        let untitled = t("project.untitled", &[]);
        let empty = self.project.points.is_empty() && cc_civil::js::arr(self.project.state.get("alignments")).is_empty();
        if empty && self.project.state.get("name").and_then(Value::as_str) == Some(untitled.as_str()) {
            let stem = d.name.rsplit_once('.').map_or(d.name.as_str(), |(a, _)| a);
            let name = cc_civil::js::slice16_range(stem, 0, Some(120));
            if let Some(m) = self.project.state.as_object_mut() {
                m.insert("name".into(), Value::String(name));
            }
        }
        self.report.clear();
        let n = pts.len();
        self.project.set_points(pts, &d.name);
        self.sel = None;
        self.profile.sel_pvi = None;
        self.points.picked.clear();
        self.fit();
        let count = grouped(n as f64, 0);
        let tris = self.project.eg().surface.as_ref().map(|s| grouped(s.n_t as f64, 0));
        let key = match (tris.is_some(), replacing) {
            (true, true) => "cv.imp.replaced.sf",
            (true, false) => "cv.imp.imported.sf",
            (false, true) => "cv.imp.replaced",
            (false, false) => "cv.imp.imported",
        };
        let said = tr!(key, n = n, count = count.as_str(), file = d.name.as_str(), tris = tris.unwrap_or_default());
        self.message = format!("{said} {}", t("cv.imp.mark", &[]));
        Ok(())
    }

    /// `civil.import.info`: the summary, the preview rows and the rows not read (" | " between lines).
    pub fn import_info(&self) -> Result<String, String> {
        let s = self.import_summary().ok_or("civil.import.info: the import dialog is not open")?;
        let (rows, bad) = self.import_rows();
        let mut out = vec![s];
        out.extend(rows.into_iter().map(|r| r.join("  ")));
        out.extend(bad);
        Ok(out.join(" | "))
    }
}

/// The dialog as a window over the plan.
pub fn show(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = cv.import.clone() else { return };
    let mut open = true;
    egui::Window::new(t0("ui.imp.h"))
        .id(egui::Id::unique("civil_import_dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .show(ctx, |ui| {
            egui::Grid::new("cv_imp_head").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new(t0("ui.imp.file")).color(tk.text_dim));
                ui.label(RichText::new(&d.name).color(tk.text));
                ui.end_row();
                let l = ui.label(RichText::new(t0("ui.imp.columns")).color(tk.text_dim));
                egui::ComboBox::new("cv_imp_format", "")
                    .selected_text(format_label(&d.format))
                    .width(330.0)
                    .show_ui(ui, |ui| {
                        for f in FORMATS {
                            if ui.selectable_label(d.format == f, format_label(f)).clicked() && d.format != f {
                                acts.push(("civil.import", format!("format={f}")));
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
                ui.end_row();
            });
            ui.add_space(4.0);
            if let Some(s) = cv.import_summary() {
                ui.add(egui::Label::new(RichText::new(s).small().color(tk.text)).wrap());
            }
            let (rows, bad) = cv.import_rows();
            if !rows.is_empty() {
                egui::Grid::new("cv_imp_preview").num_columns(5).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
                    for h in [t0("sk.c.point"), t0("pt.props.northing"), t0("pt.props.easting"), t0("pt.props.elev"), t0("pt.props.desc")] {
                        ui.label(RichText::new(h).small().strong().color(tk.text_dim));
                    }
                    ui.end_row();
                    for r in &rows {
                        for c in r {
                            ui.label(RichText::new(c).small().color(tk.text));
                        }
                        ui.end_row();
                    }
                });
            }
            if !bad.is_empty() {
                egui::Frame::new().fill(tk.panel_darker).inner_margin(6).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        for b in &bad {
                            ui.label(RichText::new(b).small().color(tk.text));
                        }
                    });
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(t0("ui.cancel")).clicked() {
                    acts.push(("civil.import", "cancel".into()));
                }
                if ui.add_enabled(!d.res.points.is_empty(), egui::Button::new(t0("ui.imp.ok"))).clicked() {
                    acts.push(("civil.import", "ok".into()));
                }
            });
        });
    if !open {
        acts.push(("civil.import", "cancel".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_reads_then_imports_or_cancels() {
        let mut cv = CivilView::sample();
        let text = "Point,Northing,Easting,Elevation,Description\n1,2080100,6150200,250,GRD\n2,2080110,6150210,251,GRD\n3,2080090,6150230,249,GRD\nbad,x,y\n";
        cv.import = Some(ImportDlg::new("a.csv", text.into(), "PNEZD"));
        let info = cv.import_info().unwrap();
        assert!(info.contains("2080100.000") && info.contains("Line 5"), "{info}");
        cv.import_format("NOPE").unwrap_err();
        cv.import_format("PENZD").unwrap();
        cv.import_format("auto").unwrap();
        cv.import_ok().unwrap();
        assert_eq!(cv.project.points.len(), 3);
        assert!(cv.import.is_none());
        assert!(cv.import_ok().is_err(), "closed");
        cv.import = Some(ImportDlg::new("b.csv", "Point,Lat,Lon,Elevation\n1,39.7,-104.9,1600\n".into(), "PNEZD"));
        cv.import_ok().unwrap();
        assert_eq!(cv.project.points.len(), 3, "latitude / longitude need a coordinate system");
    }
}
