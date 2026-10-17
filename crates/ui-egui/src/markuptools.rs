//! MarkupCraft's drawing tools on the sheet view: a gesture becomes one `Store::change` step (cc-markup model.js:
//! makeMarkup through `tx.add`, so ids, styles and undo labels are the 0.5 app's), drawn while it is dragged from the
//! same display list (shapes.js) as the finished markup. Select picks with `listDistance`, as the JS hit test does.

use cc_markup::model::{Markup, Store};
use cc_markup::shapes::{ShapeEnv, list_distance, shapes};
use serde_json::{Map, Value, json};

/// The tools, in palette order: (id, label). `select` and `pan` make nothing.
pub const TOOLS: &[(&str, &str)] = &[
    ("select", "Select"),
    ("pan", "Pan"),
    ("pen", "Pen"),
    ("highlight", "Highlighter"),
    ("line", "Line"),
    ("arrow", "Arrow"),
    ("rect", "Rectangle"),
    ("ellipse", "Ellipse"),
    ("cloudrect", "Cloud"),
    ("text", "Text"),
];

/// The colours offered (MarkupCraft's swatches: red is its default).
pub const COLORS: &[&str] = &["#e5322d", "#f08c00", "#2da44e", "#1f6feb", "#8250df", "#111111"];

/// How far (screen px) a click may land from a markup and still pick it.
const PICK_PX: f64 = 6.0;
/// Points a pen stroke keeps at most (a hostile or endless drag cannot grow it without end).
const MAX_PEN: usize = 20_000;

/// The tools' state for one open plan set.
#[derive(Debug, Clone)]
pub struct MarkupTools {
    pub tool: String,
    pub color: String,
    pub text: String,
    /// the gesture under way: its points in PDF user space
    drag: Vec<[f64; 2]>,
    pub selected: Option<String>,
    pub message: String,
}

impl Default for MarkupTools {
    fn default() -> Self {
        MarkupTools {
            tool: "select".into(),
            color: COLORS.first().copied().unwrap_or("#e5322d").into(),
            text: "Note".into(),
            drag: Vec::new(),
            selected: None,
            message: String::new(),
        }
    }
}

/// The markup fields a gesture from `a` to `b` (and the pen's whole path) makes.
fn fields(tool: &str, sheet: &str, pts: &[[f64; 2]], color: &str, text: &str) -> Option<Map<String, Value>> {
    let (a, b) = (pts.first()?, pts.last()?);
    // a click with a shape tool makes nothing (the engine would keep a zero-size shape nobody can see)
    let tiny = (a[0] - b[0]).abs() < 1.0 && (a[1] - b[1]).abs() < 1.0;
    if tiny && tool != "text" {
        return None;
    }
    let rect = json!([a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]);
    let mut f = Map::new();
    f.insert("type".into(), json!(tool));
    f.insert("sheet".into(), json!(sheet));
    f.insert("style".into(), json!({ "color": color }));
    match tool {
        "pen" | "highlight" => {
            if pts.len() < 2 {
                return None;
            }
            f.insert("pts".into(), json!(pts));
        }
        "line" | "arrow" => {
            f.insert("pts".into(), json!([a, b]));
        }
        "rect" | "ellipse" | "cloudrect" => {
            f.insert("rect".into(), rect);
        }
        "text" => {
            // a click places a box of the default size; a drag sizes it
            let small = (a[0] - b[0]).abs() < 4.0 || (a[1] - b[1]).abs() < 4.0;
            let r = if small { json!([a[0], a[1] - 30.0, a[0] + 180.0, a[1]]) } else { rect };
            f.insert("rect".into(), r);
            f.insert("text".into(), json!(text));
        }
        _ => return None,
    }
    Some(f)
}

impl MarkupTools {
    /// A markup for the gesture so far (drawn while dragging; not in the store).
    pub fn preview(&self, sheet: &str) -> Option<Markup> {
        if self.drag.is_empty() {
            return None;
        }
        let f = fields(&self.tool, sheet, &self.drag, &self.color, &self.text)?;
        let mut env = cc_markup::model::Env::default();
        let m = cc_markup::model::make_markup(&mut env, &f);
        cc_markup::model::geometry_ok(&m).then_some(m)
    }

    /// Whether the tool draws (a drag with it makes a markup rather than panning).
    pub fn draws(&self) -> bool {
        !matches!(self.tool.as_str(), "select" | "pan")
    }

    /// The gesture: pressed at `p` (PDF user space).
    pub fn begin(&mut self, p: [f64; 2]) {
        self.drag.clear();
        self.drag.push(p);
    }

    /// The gesture moved to `p`.
    pub fn moved(&mut self, p: [f64; 2]) {
        if self.drag.is_empty() {
            return;
        }
        if matches!(self.tool.as_str(), "pen" | "highlight") {
            if self.drag.len() < MAX_PEN {
                self.drag.push(p);
            }
        } else {
            self.drag.truncate(1);
            self.drag.push(p);
        }
    }

    /// The gesture ended: one undo step adding the markup (none when the shape is degenerate).
    pub fn end(&mut self, store: &mut Store, sheet: &str) -> Option<String> {
        let pts = std::mem::take(&mut self.drag);
        let f = fields(&self.tool, sheet, &pts, &self.color, &self.text)?;
        let probe = cc_markup::model::make_markup(&mut cc_markup::model::Env::default(), &f);
        if !cc_markup::model::geometry_ok(&probe) {
            return None;
        }
        let label = format!("Add {}", cc_markup::model::type_name(&self.tool).to_lowercase());
        match store.change(&label, |tx| tx.add_fields(&f)) {
            Ok(m) => {
                self.message = label;
                self.selected = Some(m.id.clone());
                Some(m.id)
            }
            Err(e) => {
                self.message = e.to_string();
                None
            }
        }
    }

    /// Select: the topmost shown markup of the sheet within `PICK_PX` of `p` (`px` = PDF units per screen pixel).
    pub fn pick(&mut self, store: &Store, sheet: &str, p: [f64; 2], px: f64) -> Option<String> {
        let env = ShapeEnv { cal: store.calibrations.get(sheet).cloned(), badge: 0.0, legend: None };
        let best = store
            .on_sheet(sheet)
            .into_iter()
            .rev()
            .filter(|m| store.shown(m))
            .map(|m| (list_distance(&shapes(m, &env), p), m.id.clone()))
            .filter(|(d, _)| d.is_finite() && *d <= PICK_PX * px)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id);
        self.selected = best.clone();
        best
    }

    /// Delete the selected markup (one undo step).
    pub fn delete_selected(&mut self, store: &mut Store) {
        let Some(id) = self.selected.take() else { return };
        let label = store.get(&id).map(|m| format!("Delete {}", cc_markup::model::type_name(&m.kind).to_lowercase()));
        if let Some(label) = label {
            let _ = store.change(&label, |tx| {
                tx.remove(&id);
                Ok(())
            });
            self.message = label;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_adds_one_markup_and_undo_removes_it() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "rect".into(), ..MarkupTools::default() };
        t.begin([100.0, 100.0]);
        t.moved([200.0, 160.0]);
        let id = t.end(&mut store, "f1:0").unwrap();
        let m = store.get(&id).unwrap();
        assert_eq!(m.rect, Some([100.0, 100.0, 200.0, 160.0]));
        assert_eq!(m.style.color, "#e5322d");
        assert_eq!(store.on_sheet("f1:0").len(), 1);
        // a click on its edge picks it; far away picks nothing
        assert_eq!(t.pick(&store, "f1:0", [100.5, 130.0], 1.0), Some(id.clone()));
        assert_eq!(t.pick(&store, "f1:0", [500.0, 500.0], 1.0), None);
        assert!(store.undo().is_some());
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn a_click_is_not_a_rectangle() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "rect".into(), ..MarkupTools::default() };
        t.begin([100.0, 100.0]);
        assert!(t.end(&mut store, "f1:0").is_none());
        assert!(store.on_sheet("f1:0").is_empty());
    }
}
