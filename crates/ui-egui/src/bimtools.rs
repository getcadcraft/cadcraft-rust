//! BimCraft's Find and Quantity totals panels on the IFC view, through cc-bim (ifc-find.js, ifc-qto.js at parity).
//! The property index is built the first time a panel needs it (bimcraft.js does the same after a model loads: it costs
//! a second read of the file, so a viewer that never searches never pays it).

use cc_bim::find::{OPS, Rule, Want};
use cc_bim::props::Index;
use cc_bim::qto::{Cell, CsvWords, Totals};
use egui::RichText;

use crate::theme::Tokens;

/// A word of BimCraft's own catalog (in the language now): `{name}` fills from `vars`; a plural entry picks `=0` / `one` / `other`
/// by `n`; the key itself when missing.
pub fn bt(key: &str, vars: &[(&str, &str)]) -> String {
    let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| *v);
    let mut s = match crate::lang::BIMCRAFT.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(o)) => {
            let form = match n {
                Some("0") if o.contains_key("=0") => "=0",
                Some("1") => "one",
                _ => "other",
            };
            o.get(form).or_else(|| o.get("other")).and_then(serde_json::Value::as_str).unwrap_or(key).to_string()
        }
        _ => key.to_string(),
    };
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// What the panels ask the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum BimAct {
    /// show only these products (expressIDs)
    Isolate(Vec<f64>),
    /// hide these products
    Hide(Vec<f64>),
    /// everything visible again
    ShowAll,
    /// save text under a suggested file name
    Save(String, Vec<u8>),
}

/// One rule as typed.
#[derive(Debug, Clone)]
struct RuleUi {
    field: String,
    op: String,
    value: String,
}

impl Default for RuleUi {
    fn default() -> Self {
        RuleUi { field: "cls".into(), op: "is".into(), value: String::new() }
    }
}

/// The panels' state for one open IFC model.
#[derive(Default)]
pub struct BimTools {
    index: Option<Result<Index, String>>,
    rules: Vec<RuleUi>,
    any: bool,
    found: Option<Vec<f64>>,
    message: String,
    group_by: Vec<String>,
    sum: Vec<String>,
    totals: Option<Totals>,
    /// a panel to open on the next frame (a command ran it): 0 none, 1 Find, 2 Quantity totals
    open: u8,
}

impl std::fmt::Debug for BimTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BimTools").field("rules", &self.rules.len()).field("found", &self.found.as_ref().map(Vec::len)).finish()
    }
}

/// The property index of a file (ifc-model.js load + ifc-props.js propertyIndex), elements only (spaces have their own
/// schedule in BimCraft).
pub fn build_index(bytes: &[u8]) -> Result<Index, String> {
    let f = cc_io_ifc::model::IfcFile::open(bytes).map_err(|e| e.to_string())?;
    let model = cc_bim::model::load(&f, &cc_io_ifc::geo::bool_ops::ManifoldOps, 0.0).map_err(|e| e.0)?;
    cc_bim::props::property_index(&f, &model, false).map_err(|e| e.0)
}

/// A field's name as the panels show it: a built-in by its catalog word, a property as "name (set)".
pub fn field_label(ix: &Index, id: &str) -> String {
    let Some(f) = ix.field(id) else { return id.to_string() };
    let label = f.label.as_ref().map(|l| l.text()).unwrap_or_default();
    if label.is_empty() {
        return bt(&format!("color.field.{id}"), &[]);
    }
    match f.group.as_ref().map(|g| g.text()).filter(|g| !g.is_empty()) {
        Some(g) => format!("{label} ({g})"),
        None => label,
    }
}

/// A dropdown with its label above it (the panel is narrow), the label naming it for accessibility.
fn labelled_combo(ui: &mut egui::Ui, id: &str, label: &str, selected: String, add: impl FnOnce(&mut egui::Ui)) {
    let l = ui.label(RichText::new(label).small());
    let w = ui.available_width().clamp(80.0, 220.0);
    let _ = egui::ComboBox::from_id_salt(id).selected_text(selected).width(w).show_ui(ui, add).response.labelled_by(l.id);
}

/// A field to choose: (id, label).
type Choice = (String, String);

/// How many rows of totals the panel draws (the CSV has them all), as bimcraft.js.
const MAX_SHOWN: usize = 200;

impl BimTools {
    /// The index, built on first use.
    fn index(&mut self, bytes: &[u8]) -> Option<&Index> {
        if self.index.is_none() {
            self.index = Some(build_index(bytes));
        }
        self.index.as_ref().and_then(|r| r.as_ref().ok())
    }

    /// `bim.find`: one rule "field:op:value" (value may hold ':'), found and opened in the panel.
    pub fn find_rule(&mut self, bytes: &[u8], spec: &str) -> Result<Vec<f64>, String> {
        let mut it = spec.splitn(3, ':');
        let (field, op, value) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        if !OPS.contains(&op) {
            return Err(format!("bim.find: unknown operator {op} (one of {})", OPS.join(", ")));
        }
        self.rules = vec![RuleUi { field: field.into(), op: op.into(), value: value.into() }];
        self.any = false;
        self.open = 1;
        self.run_find(bytes).ok_or_else(|| self.message.clone())
    }

    /// `bim.totals`: "groupField[:sumField]", counted and opened in the panel.
    pub fn totals_by(&mut self, bytes: &[u8], spec: &str) {
        let (g, s) = spec.split_once(':').unwrap_or((spec, ""));
        self.group_by = vec![g.to_string()];
        self.sum = if s.is_empty() { Vec::new() } else { vec![s.to_string()] };
        self.open = 2;
        self.run_totals(bytes);
    }

    /// ifc-find.js find() with the typed rules; keys are expressIDs (keyBase 0).
    pub fn run_find(&mut self, bytes: &[u8]) -> Option<Vec<f64>> {
        let rules: Vec<Rule> = self
            .rules
            .iter()
            .map(|r| Rule {
                field: Some(r.field.clone()),
                field_truthy: !r.field.is_empty(),
                op: r.op.clone(),
                value: Want::Str(r.value.clone()),
            })
            .collect();
        let any = self.any;
        let incomplete = self.rules.iter().any(|r| !matches!(r.op.as_str(), "has" | "hasnot") && r.value.trim().is_empty());
        let ix = self.index(bytes)?;
        if incomplete {
            self.message = bt("find.incomplete", &[]);
            return None;
        }
        let keys = cc_bim::find::find(ix, &rules, any);
        self.message = bt("find.found", &[("n", &keys.len().to_string())]);
        self.found = Some(keys.clone());
        Some(keys)
    }

    /// ifc-qto.js totals() over every element.
    pub fn run_totals(&mut self, bytes: &[u8]) {
        let (g, s) = (self.group_by.clone(), self.sum.clone());
        if let Some(ix) = self.index(bytes) {
            self.totals = Some(cc_bim::qto::totals(ix, None, &g, &s));
        }
    }

    /// The totals as BimCraft's CSV (totalsCsv), with its words.
    fn totals_csv(&self) -> Option<String> {
        let res = self.totals.as_ref()?;
        let ix = self.index.as_ref()?.as_ref().ok()?;
        let labels = res.group_by.iter().chain(res.sum.iter()).map(|f| (f.clone(), field_label(ix, f))).collect();
        let words = CsvWords {
            labels,
            count: Some(bt("qto.count", &[])),
            total: Some(bt("qto.total", &[])),
            // "{name} (left out)": what follows the header
            left_out_suffix: Some(bt("qto.leftout", &[]).replace("{name}", "")),
        };
        Some(cc_bim::qto::totals_csv(res, &words))
    }

    /// The Find and Quantity totals panels (inside the Model panel's scroll area).
    pub fn panels(&mut self, ui: &mut egui::Ui, t: &Tokens, bytes: &[u8], name: &str) -> Option<BimAct> {
        let mut act = None;
        let open = std::mem::take(&mut self.open);
        egui::CollapsingHeader::new(RichText::new(bt("find.title", &[])).color(t.text_strong))
            .default_open(false)
            .open((open == 1).then_some(true))
            .show(ui, |ui| {
                act = self.find_panel(ui, t, bytes);
            });
        egui::CollapsingHeader::new(RichText::new(bt("qto.title", &[])).color(t.text_strong))
            .default_open(false)
            .open((open == 2).then_some(true))
            .show(ui, |ui| {
                if let Some(a) = self.qto_panel(ui, t, bytes, name) {
                    act = Some(a);
                }
            });
        act
    }

    fn find_panel(&mut self, ui: &mut egui::Ui, t: &Tokens, bytes: &[u8]) -> Option<BimAct> {
        if self.rules.is_empty() {
            self.rules.push(RuleUi::default());
        }
        let fields: Vec<(String, String)> = match self.index(bytes) {
            Some(ix) => ix.fields.iter().map(|f| (f.id.clone(), field_label(ix, &f.id))).collect(),
            None => {
                let e = self.index.as_ref().and_then(|r| r.as_ref().err()).cloned().unwrap_or_default();
                ui.label(RichText::new(e).color(t.text_dim));
                return None;
            }
        };
        let mut act = None;
        let mut remove: Option<usize> = None;
        let many = self.rules.len() > 1;
        for (i, r) in self.rules.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                let cur = fields.iter().find(|f| f.0 == r.field).map_or_else(|| r.field.clone(), |f| f.1.clone());
                labelled_combo(ui, "field", &bt("find.field", &[]), cur, |ui| {
                    for (id, label) in &fields {
                        ui.selectable_value(&mut r.field, id.clone(), label);
                    }
                });
                ui.vertical(|ui| {
                    labelled_combo(ui, "op", &bt("find.op", &[]), bt(&format!("find.op.{}", r.op), &[]), |ui| {
                        for op in OPS {
                            ui.selectable_value(&mut r.op, op.to_string(), bt(&format!("find.op.{op}"), &[]));
                        }
                    });
                    if many && ui.small_button("Remove").on_hover_text(bt("find.remove", &[])).clicked() {
                        remove = Some(i);
                    }
                });
                if !matches!(r.op.as_str(), "has" | "hasnot") {
                    ui.add(egui::TextEdit::singleline(&mut r.value).hint_text(bt("find.value", &[])).desired_width(170.0));
                }
            });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            self.rules.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.small_button(bt("find.add", &[])).clicked() {
                self.rules.push(RuleUi::default());
            }
            if self.rules.len() > 1 {
                ui.radio_value(&mut self.any, false, bt("find.mode.all", &[]));
                ui.radio_value(&mut self.any, true, bt("find.mode.any", &[]));
            }
        });
        ui.horizontal(|ui| {
            if ui.button(bt("find.go", &[])).clicked()
                && let Some(keys) = self.run_find(bytes)
            {
                act = Some(BimAct::Isolate(keys));
            }
            let has = self.found.as_ref().is_some_and(|f| !f.is_empty());
            if ui.add_enabled(has, egui::Button::new("Hide them")).clicked() {
                act = self.found.clone().map(BimAct::Hide);
            }
            if ui.small_button("Show all").clicked() {
                act = Some(BimAct::ShowAll);
            }
        });
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).color(t.text_dim));
        }
        act
    }

    fn qto_panel(&mut self, ui: &mut egui::Ui, t: &Tokens, bytes: &[u8], name: &str) -> Option<BimAct> {
        let ix = self.index(bytes)?;
        let fields: Vec<Choice> = ix.fields.iter().filter(|f| !f.number).map(|f| (f.id.clone(), field_label(ix, &f.id))).collect();
        let numbers: Vec<Choice> = cc_bim::qto::quantity_fields(ix).iter().map(|f| (f.id.clone(), field_label(ix, &f.id))).collect();
        if self.group_by.is_empty() {
            self.group_by.push("cls".into());
        }
        let mut changed = self.totals.is_none();
        let label_of =
            |list: &[(String, String)], id: &str| list.iter().find(|f| f.0 == id).map_or_else(|| id.to_string(), |f| f.1.clone());
        let mut g = self.group_by.first().cloned().unwrap_or_default();
        labelled_combo(ui, "qto_group", &bt("qto.group", &[]), label_of(&fields, &g), |ui| {
            for (id, label) in &fields {
                ui.selectable_value(&mut g, id.clone(), label);
            }
        });
        if self.group_by.first() != Some(&g) {
            self.group_by = vec![g];
            changed = true;
        }
        if numbers.is_empty() {
            ui.label(RichText::new(bt("qto.sum.none", &[])).color(t.text_dim));
        } else {
            let mut s = self.sum.first().cloned().unwrap_or_default();
            let shown = if s.is_empty() { "—".to_string() } else { label_of(&numbers, &s) };
            labelled_combo(ui, "qto_sum", &bt("qto.sum", &[]), shown, |ui| {
                ui.selectable_value(&mut s, String::new(), "—");
                for (id, label) in &numbers {
                    ui.selectable_value(&mut s, id.clone(), label);
                }
            });
            let want: Vec<String> = if s.is_empty() { Vec::new() } else { vec![s] };
            if want != self.sum {
                self.sum = want;
                changed = true;
            }
        }
        if changed {
            self.run_totals(bytes);
        }
        let mut act = None;
        if let Some(res) = &self.totals {
            egui::Grid::new("qto_rows").striped(true).num_columns(2 + res.sum.len()).show(ui, |ui| {
                ui.label(RichText::new(label_of(&fields, res.group_by.first().map_or("", String::as_str))).strong());
                ui.label(RichText::new(bt("qto.count", &[])).strong());
                for f in &res.sum {
                    ui.label(RichText::new(label_of(&numbers, f)).strong());
                }
                ui.end_row();
                for r in res.rows.iter().take(MAX_SHOWN) {
                    let v = r.values.first().map(Cell::text).unwrap_or_default();
                    ui.label(if v.is_empty() { bt("qto.empty", &[]) } else { v });
                    ui.label(cc_bim::js::num_str(r.count));
                    for x in &r.sums {
                        ui.label(cc_bim::js::num_str(*x));
                    }
                    ui.end_row();
                }
                ui.label(RichText::new(bt("qto.total", &[])).strong());
                ui.label(RichText::new(cc_bim::js::num_str(res.total.count)).strong());
                for x in &res.total.sums {
                    ui.label(RichText::new(cc_bim::js::num_str(*x)).strong());
                }
                ui.end_row();
            });
            let n = res.rows.len();
            ui.label(RichText::new(bt("qto.rows", &[("n", &n.to_string())])).color(t.text_dim));
            if n > MAX_SHOWN {
                ui.label(RichText::new(bt("qto.more", &[("shown", &MAX_SHOWN.to_string()), ("n", &n.to_string())])).color(t.text_dim));
            }
            if ui.small_button(bt("qto.csv", &[])).clicked()
                && let Some(csv) = self.totals_csv()
            {
                let stem = name.rsplit_once('.').map_or(name, |(a, _)| a);
                // the page adds a BOM so spreadsheet programs read UTF-8 (ifc-qto.js leaves it to the page)
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(csv.as_bytes());
                act = Some(BimAct::Save(format!("{stem}-quantities.csv"), bytes));
            }
        }
        act
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_pick_their_form() {
        assert_eq!(bt("find.found", &[("n", "0")]), "Found nothing.");
        assert_eq!(bt("find.found", &[("n", "1")]), "Found 1 element: it is picked.");
        assert!(bt("find.found", &[("n", "7")]).starts_with("Found 7 elements"));
        assert_eq!(bt("color.field.cls", &[]), "Class");
    }

    #[test]
    fn find_and_totals_on_the_sample_house() {
        let bytes = include_bytes!("../../../assets/samples/bimcraft-house.ifc");
        let mut b =
            BimTools { rules: vec![RuleUi { field: "cls".into(), op: "is".into(), value: "IfcWall".into() }], ..Default::default() };
        let walls = b.run_find(bytes).unwrap();
        assert!(!walls.is_empty(), "{}", b.message);
        b.group_by = vec!["cls".into()];
        b.run_totals(bytes);
        let res = b.totals.as_ref().unwrap();
        let wall_row = res.rows.iter().find(|r| r.values.first().map(Cell::text).as_deref() == Some("IfcWall")).unwrap();
        assert_eq!(wall_row.count as usize, walls.len());
        let csv = b.totals_csv().unwrap();
        assert!(csv.starts_with("Class,Count\r\n"), "{csv}");
    }
}
