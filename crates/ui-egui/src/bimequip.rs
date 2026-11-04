//! BimCraft's IFC equipment schedule and library credits (bimcraft.js `toggleIfcEquipment` / `renderIfcEquipment` /
//! `showCredits`; ui.inc's library tools Equipment and Credits): the placed library items as an equipment schedule —
//! pick a storey, filter by any word, save the CSV, jump to an item (it is picked, its Properties shown) — and every
//! library item in the model with the credit its license asks for (copy the lines, or a CSV of every placement).
//! The schedule is `cc_bim`'s `Placer::schedule` (bmrestooldiff.mjs: at parity), drawn with the drawing viewer's words.
//!
//! Commands: `bim.equip` (on / off), `bim.equip.pick {name}`, `bim.equip.filter {name}`, `bim.equip.csv`,
//! `bim.equip.show {index}`, `bim.credits` (open), `bim.credits.copy`, `bim.credits.csv`, `bim.credits.close`.

use cc_dwgview::equipment::{Elec, EquipList, Place, Row, SheetCount, elec_text, equipment_csv};
use cc_dwgview::page;
use egui::RichText;

use crate::bimtools::bt;
use crate::{Params, UiApp};

pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("bim.equip", "Equipment schedule", "", "{} → the placed library items as an equipment schedule (on / off)"),
    ("bim.equip.pick", "Which equipment", "", "{name: \"all\" | \"off\" | \"storey:<id>\"} → the schedule lists that storey's"),
    ("bim.equip.filter", "Filter the schedule", "", "{name: words} → only rows holding every word"),
    ("bim.equip.csv", "Download CSV", "", "{} → saves the rows shown as CSV (\"<model> equipment.csv\")"),
    ("bim.equip.show", "Show an item", "", "{index: row shown} → picks its first placement and shows its Properties"),
    ("bim.credits", "Library credits", "", "{} → every library item in the model and the credit its license asks for"),
    ("bim.credits.copy", "Copy credits", "", "{} → the credit lines to the clipboard"),
    ("bim.credits.csv", "Credits CSV", "", "{} → saves every placement with its library, version, license and attribution"),
    ("bim.credits.close", "Close the credits", "", "{}"),
];

/// The schedule's state: shown, the choice, the filter, the list (with each row's placements' product ids).
#[derive(Clone, Debug, Default)]
pub struct EquipState {
    pub on: bool,
    pub pick: String,
    pub query: String,
    pub credits: bool,
    /// credit lines waiting for the clipboard (the next frame copies them)
    pub copy: Option<String>,
}

/// `editor.placer().schedule()` as the equipment list (and each row's products).
fn list(app: &mut UiApp) -> Option<(EquipList, Vec<Vec<f64>>)> {
    let doc = app.bimh.ed.as_mut()?;
    let s = cc_bim::draw::place::Placer::new(&mut doc.ed.api).ok()?.schedule().ok()??;
    let mut ids = Vec::new();
    let rows = s
        .rows
        .iter()
        .map(|r| {
            ids.push(r.places.iter().map(|p| p.id).collect());
            let elec: Vec<Elec> = r
                .elec
                .iter()
                .map(|e| Elec {
                    amps: e.amps.clone(),
                    kw: e.kw.clone(),
                    hp: e.hp.clone(),
                    volts: e.volts.clone(),
                    phase: e.phase.clone(),
                    hz: e.hz.clone(),
                    conn: e.conn,
                    nema: e.nema.clone(),
                })
                .collect();
            let elec_text = elec.iter().map(elec_text).collect::<Vec<_>>().join(" + ");
            Row {
                no: r.no.clone(),
                qty: r.qty.clone(),
                qtys: r.qtys.clone(),
                category: r.category.clone(),
                mfr: r.mfr.clone(),
                model: r.model.clone(),
                remark: r.remark.clone(),
                elec,
                elec_text,
                plumb_text: r.plumb_text.clone(),
                el_remark: String::new(),
                pl_remark: String::new(),
                places: r.places.iter().map(|p| Place { x: p.x, y: p.y, qty: p.qty.clone(), sheets: p.sheets.clone() }).collect(),
                sheets: r.sheets.clone(),
                clash: r.clash,
            }
        })
        .collect();
    let l = EquipList {
        rows,
        tags: s.tags,
        sheets: s.sheets.iter().map(|(k, n, c)| SheetCount { key: k.clone(), name: n.clone(), count: *c }).collect(),
        off_sheet: s.off_sheet,
    };
    Some((l, ids))
}

fn intro(l: &EquipList) -> String {
    let n = l.tags.to_string();
    bt("ifcequip.intro", &[("n", &n), ("count", &n)])
}
fn head(l: &EquipList) -> page::EquipHead {
    page::equip_head_with(l, true, Some(intro(l)))
}
fn pick_of(app: &UiApp, h: &page::EquipHead) -> String {
    let p = &app.bimh.equip.pick;
    if h.choices.iter().any(|c| c.value == *p) { p.clone() } else { h.choices.first().map(|c| c.value.clone()).unwrap_or_default() }
}
fn storey_name(l: &EquipList, key: &str) -> String {
    l.sheets.iter().find(|s| s.key == key).map_or_else(|| key.to_string(), |s| s.name.clone())
}
fn model_base(app: &UiApp) -> String {
    let n = app.ifc_name().to_string();
    if n.to_ascii_lowercase().ends_with(".ifc") { n.get(..n.len().saturating_sub(4)).unwrap_or(&n).to_string() } else { n }
}

pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.equip" => {
            if app.bimh.equip.on {
                app.bimh.equip.on = false;
            } else if list(app).is_none() {
                app.message = bt("ifcequip.none", &[]);
            } else {
                app.bimh.equip.on = true;
            }
        }
        "bim.equip.pick" => {
            let (l, _) = list(app).ok_or_else(|| bt("ifcequip.none", &[]))?;
            if !head(&l).choices.iter().any(|c| c.value == name) {
                return Err(format!("bim.equip.pick: no choice {name}"));
            }
            app.bimh.equip.pick = name;
        }
        "bim.equip.filter" => {
            if name.chars().count() > 500 {
                return Err("bim.equip.filter: at most 500 characters".into());
            }
            app.bimh.equip.query = cc_cad::js::trim(&name).to_string();
        }
        "bim.equip.csv" => {
            let (l, _) = list(app).ok_or_else(|| bt("ifcequip.none", &[]))?;
            let h = head(&l);
            let rows = page::equip_shown(&l, &pick_of(app, &h), &app.bimh.equip.query);
            if rows.is_empty() {
                return Err("bim.equip.csv: no rows to save".into());
            }
            let file = format!("{} equipment.csv", model_base(app));
            let mut bytes = "\u{feff}".as_bytes().to_vec();
            bytes.extend(equipment_csv(&rows, &|k| storey_name(&l, k)).into_bytes());
            let n = rows.len();
            let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
            if save(&file, bytes)?.is_some() {
                app.message = page::downloaded(&file, n);
            }
        }
        "bim.equip.show" => {
            let i = p.index.or_else(|| name.trim().parse().ok()).ok_or("bim.equip.show: index is required")?;
            let (l, ids) = list(app).ok_or_else(|| bt("ifcequip.none", &[]))?;
            let h = head(&l);
            let rows = page::equip_shown(&l, &pick_of(app, &h), &app.bimh.equip.query);
            let r = rows.get(i).ok_or_else(|| format!("bim.equip.show: no row {i}"))?;
            let k = l.rows.iter().position(|x| std::ptr::eq(x, *r)).ok_or("bim.equip.show: no row")?;
            let id = ids.get(k).and_then(|v| v.first()).copied().ok_or("bim.equip.show: the item has no place")?;
            app.bimh.equip.on = false;
            app.select_product(id);
        }
        "bim.credits" => app.bimh.equip.credits = true,
        "bim.credits.close" => app.bimh.equip.credits = false,
        "bim.credits.copy" => {
            let lines = credits(app)?.iter().map(|g| g.line.clone()).collect::<Vec<_>>().join("\n");
            app.bimh.equip.copy = Some(lines);
            app.message = bt("credits.copied", &[]);
        }
        "bim.credits.csv" => credits_csv(app)?,
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// One library item of the credits: name, version, license, its credit line, the item numbers it is placed as.
#[derive(Clone, Debug)]
struct Credit {
    name: String,
    version: String,
    license: String,
    line: String,
    numbers: Vec<String>,
}

fn placements(app: &mut UiApp) -> Result<Vec<cc_bim::draw::place::PlacementRow>, String> {
    let doc = app.bimh.ed.as_mut().ok_or_else(|| bt("model.needs.ifc", &[]))?;
    cc_bim::draw::place::Placer::new(&mut doc.ed.api).and_then(|mut p| p.placements()).map_err(|e| e.0)
}

/// `showCredits()`'s groups: one per item (uuid@version, else its file's sha256), in the order first placed.
fn credits(app: &mut UiApp) -> Result<Vec<Credit>, String> {
    let pls = placements(app)?;
    let mut keys: Vec<String> = Vec::new();
    let mut out: Vec<Credit> = Vec::new();
    for p in &pls {
        let k = if p.uuid.is_empty() { format!("sha:{}", p.sha256) } else { format!("{}@{}", p.uuid, p.version.string()) };
        let name = p.equip("Name").filter(|v| v.truthy()).map_or_else(|| p.name.clone(), cc_bim::wapi::P::string);
        match keys.iter().position(|x| *x == k) {
            Some(i) => {
                if let Some(c) = out.get_mut(i) {
                    c.numbers.push(p.item_number.clone());
                }
            }
            None => {
                let line = if !p.attribution.is_empty() {
                    p.attribution.clone()
                } else if !p.license.is_empty() {
                    bt("credits.line.license", &[("name", &name), ("license", &crate::bimlib::license_label(&p.license))])
                } else if !p.source_file.is_empty() {
                    bt("credits.line.file.src", &[("name", &name), ("file", &p.source_file)])
                } else {
                    bt("credits.line.file", &[("name", &name)])
                };
                let version = if p.version.truthy() { bt("lib.version", &[("version", &p.version.string())]) } else { "—".into() };
                let license = if p.license.is_empty() { bt("credits.fromfile", &[]) } else { crate::bimlib::license_label(&p.license) };
                keys.push(k);
                out.push(Credit { name, version, license, line, numbers: vec![p.item_number.clone()] });
            }
        }
    }
    Ok(out)
}

/// `csvCell(v)`: formulas made text, quoted when needed.
fn csv_cell(v: &str) -> String {
    let s = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{v}") } else { v.to_string() };
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

fn credits_csv(app: &mut UiApp) -> Result<(), String> {
    let pls = placements(app)?;
    let head = ["Item", "Qty", "Name", "Maker", "Model", "Library", "Item id", "Version", "License", "Attribution", "Notes"];
    let mut lines = vec![head.iter().map(|h| csv_cell(h)).collect::<Vec<_>>().join(",")];
    for p in &pls {
        let e = |k: &str| p.equip(k).filter(|v| v.truthy()).map(cc_bim::wapi::P::string).unwrap_or_default();
        let name = if e("Name").is_empty() { p.name.clone() } else { e("Name") };
        let ver = if p.version.truthy() { p.version.string() } else { String::new() };
        let row = [
            p.item_number.clone(),
            p.quantity.string(),
            name,
            e("Maker"),
            e("Model"),
            p.library.clone(),
            p.uuid.clone(),
            ver,
            p.license.clone(),
            p.attribution.clone(),
            p.notes.clone(),
        ];
        lines.push(row.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
    }
    let text = format!("\u{feff}{}\r\n", lines.join("\r\n"));
    let file = format!("{} library credits.csv", model_base(app));
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    if save(&file, text.into_bytes())?.is_some() {
        app.message = bt("credits.downloaded", &[]);
    }
    Ok(())
}

/// The schedule and the credits, as windows over the model.
pub fn windows(app: &mut UiApp, ctx: &egui::Context) {
    if let Some(s) = app.bimh.equip.copy.take() {
        ctx.copy_text(s);
    }
    let t = app.tokens;
    let mut act: Option<(&'static str, Params)> = None;
    if app.bimh.equip.on
        && let Some((l, _)) = list(app)
    {
        let h = head(&l);
        let pick = pick_of(app, &h);
        let query = app.bimh.equip.query.clone();
        let rows = page::equip_shown(&l, &pick, &query);
        let mut open = true;
        crate::shell::Dialog::new(h.title.clone())
            .modeless()
            .id(egui::Id::unique("bim_equip"))
            .open(&mut open)
            .resizable(true)
            .default_size([900.0, 520.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(&h.kind).small().color(t.text_dim));
                ui.label(RichText::new(&h.intro).small().color(t.text_dim));
                ui.horizontal_wrapped(|ui| {
                    let sel = h.choices.iter().find(|c| c.value == pick).map(|c| c.text.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("bim_equip_which").selected_text(sel).show_ui(ui, |ui| {
                        for c in &h.choices {
                            if ui.selectable_label(c.value == pick, &c.text).clicked() {
                                act = Some(("bim.equip.pick", Params { name: Some(c.value.clone()), ..Default::default() }));
                            }
                        }
                    });
                    let mut q = query.clone();
                    if ui.add(crate::shell::field(&mut q).hint_text(&h.filter_placeholder).desired_width(240.0)).changed() {
                        act = Some(("bim.equip.filter", Params { name: Some(q), ..Default::default() }));
                    }
                    if ui.add_enabled(!rows.is_empty(), crate::shell::Btn::new(&h.csv)).clicked() {
                        act = Some(("bim.equip.csv", Params::default()));
                    }
                });
                ui.label(RichText::new(page::count_line(rows.len(), &query)).small().color(t.text_dim));
                egui::ScrollArea::both().id_salt("bim_equip_rows").show(ui, |ui| {
                    egui::Grid::new("bim_equip_grid").striped(true).show(ui, |ui| {
                        for hd in &h.headers {
                            ui.label(RichText::new(hd).small().strong());
                        }
                        ui.end_row();
                        for (i, r) in rows.iter().enumerate() {
                            let c = page::row_cells(r, &|k| storey_name(&l, k));
                            ui.label(RichText::new(&c.item).small().strong());
                            ui.label(
                                RichText::new(c.qty_sub.as_ref().map_or_else(|| c.qty.clone(), |s| format!("{} ({s})", c.qty))).small(),
                            );
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&c.category).small());
                                if let Some(p) = &c.clash {
                                    ui.label(RichText::new(p).small().color(t.accent));
                                }
                                if let Some(s) = &c.remark {
                                    ui.label(RichText::new(s).small().color(t.text_dim));
                                }
                            });
                            ui.label(RichText::new(&c.mfr).small());
                            ui.label(RichText::new(&c.model).small());
                            ui.label(RichText::new(&c.elec).small());
                            ui.label(RichText::new(&c.plumb).small());
                            if ui.link(RichText::new(&c.where_).small()).on_hover_text(&c.where_title).clicked() {
                                act = Some(("bim.equip.show", Params { index: Some(i), ..Default::default() }));
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.label(RichText::new(&h.note).small().color(t.text_dim));
            });
        if !open {
            act = Some(("bim.equip", Params::default()));
        }
    }
    if app.bimh.equip.credits {
        let list = credits(app).unwrap_or_default();
        let mut open = true;
        crate::shell::Dialog::new(bt("credits.title", &[]))
            .id(egui::Id::unique("bim_credits"))
            .open(&mut open)
            .default_size([720.0, 360.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(bt("credits.intro", &[])).small().color(t.text_dim));
                if list.is_empty() {
                    ui.label(RichText::new(bt("credits.none", &[])).color(t.text_dim));
                    return;
                }
                egui::Grid::new("bim_credits_grid").striped(true).show(ui, |ui| {
                    for k in
                        ["credits.col.item", "credits.col.version", "credits.col.license", "credits.col.attribution", "credits.col.usedas"]
                    {
                        ui.label(RichText::new(bt(k, &[])).small().strong());
                    }
                    ui.end_row();
                    for g in &list {
                        ui.label(RichText::new(&g.name).small().strong());
                        ui.label(RichText::new(&g.version).small());
                        ui.label(RichText::new(&g.license).small());
                        ui.label(RichText::new(&g.line).small());
                        ui.label(RichText::new(g.numbers.join(", ")).small());
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add(crate::shell::Btn::primary(bt("credits.copy", &[]))).clicked() {
                        act = Some(("bim.credits.copy", Params::default()));
                    }
                    if ui.add(crate::shell::Btn::new(bt("credits.csv", &[]))).clicked() {
                        act = Some(("bim.credits.csv", Params::default()));
                    }
                });
            });
        if !open {
            act = Some(("bim.credits.close", Params::default()));
        }
    }
    if let Some((id, p)) = act
        && let Err(e) = app.run(id, &p)
    {
        app.error = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::{Params, Services, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: (!name.is_empty()).then(|| name.to_string()), ..Default::default() })
    }

    #[test]
    fn placed_items_make_a_schedule_and_credits() {
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
        for (id, n) in [("bim.clock", "1791979200000"), ("bim.new", "Kitchen|1|3000 mm"), ("bim.units", "mm")] {
            run(&mut app, id, n).unwrap();
        }
        run(&mut app, "bim.equip", "").unwrap();
        assert!(!app.bimh.equip.on, "nothing placed: no schedule");
        let item = br#"{"format":"cadcraft-library-item","format_version":1,"name":"Range","maker":"Acme","model":"R-36",
            "license":{"id":"CC0-1.0"},"ifc":{"class":"IfcFurniture"},
            "body":{"type":"box","x":-400,"y":-800,"z":0,"w":800,"d":800,"h":900},
            "params":{"width":800,"depth":800,"height":900,"elec":{"volts":"208","amps":"40","direct":"yes"}},
            "plan":{"entities":[{"type":"poly","closed":true,"pts":[[-400,-800],[400,-800],[400,0],[-400,0]]}]}}"#;
        for at in ["1000,1000", "3000,1000"] {
            app.open_bytes("range.item.json", item);
            run(&mut app, "bim.point", at).unwrap();
            run(&mut app, "bim.place.ok", "7").unwrap();
            run(&mut app, "bim.place.stop", "").unwrap();
        }
        run(&mut app, "bim.equip", "").unwrap();
        assert!(app.bimh.equip.on);
        let (l, ids) = super::list(&mut app).unwrap();
        assert_eq!(l.rows.len(), 1, "two of one item: one row");
        assert_eq!(l.rows[0].qty, "2");
        assert!(l.rows[0].elec_text.contains("208"), "{}", l.rows[0].elec_text);
        run(&mut app, "bim.equip.csv", "").unwrap();
        app.run("bim.equip.show", &Params { index: Some(0), ..Default::default() }).unwrap();
        assert_eq!(app.picked_product(), ids[0].first().copied());
        run(&mut app, "bim.credits", "").unwrap();
        run(&mut app, "bim.credits.copy", "").unwrap();
        assert!(app.bimh.equip.copy.as_ref().is_some_and(|s| s.contains("Range")));
        run(&mut app, "bim.credits.csv", "").unwrap();
        let files = saved.lock().unwrap().clone();
        assert_eq!(files.len(), 2);
        assert!(files[0].0.ends_with("equipment.csv"));
        assert!(String::from_utf8_lossy(&files[1].1).contains("CC0-1.0"));
    }
}
