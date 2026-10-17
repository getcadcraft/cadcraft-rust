//! MarkupCraft's drawing tools on the sheet view: a gesture becomes one `Store::change` step (cc-markup model.js:
//! makeMarkup through `tx.add`, so ids, styles and undo labels are the 0.5 app's), drawn while it is dragged from the
//! same display list (shapes.js) as the finished markup. Select picks with `listDistance`, as the JS hit test does.

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, Store};
use cc_markup::shapes::{ShapeEnv, list_distance, shapes};
use cc_markup::units::{Cal, calibration_from, parse_length};
use serde_json::{Map, Value, json};

/// The tools, in palette order (ids; the label is the catalog's `tool.<id>`). `select` and `pan` make nothing;
/// `calibrate` sets the sheet's scale instead of making a markup.
pub const TOOLS: &[&str] = &[
    "select",
    "pan",
    "pen",
    "highlight",
    "line",
    "arrow",
    "rect",
    "ellipse",
    "cloudrect",
    "callout",
    "text",
    "measure",
    "area",
    "calibrate",
];

/// A tool's label from MarkupCraft's catalog.
pub fn tool_label(id: &str) -> String {
    t0(&format!("tool.{id}"))
}

/// What the status bar asks for while the tool is active (the 0.5 hints).
pub fn tool_hint(id: &str) -> String {
    match id {
        "calibrate" | "callout" | "area" => t0(&format!("hint.{id}")),
        _ => String::new(),
    }
}

/// The colours offered (MarkupCraft's swatches: red is its default).
pub const COLORS: &[&str] = &["#e5322d", "#f08c00", "#2da44e", "#1f6feb", "#8250df", "#111111"];

/// How far (screen px) a click may land from a markup and still pick it.
const PICK_PX: f64 = 6.0;
/// Points a pen stroke keeps at most (a hostile or endless drag cannot grow it without end).
const MAX_PEN: usize = 20_000;
/// Corners an area outline keeps at most.
const MAX_POLY: usize = 5_000;

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
    /// PDF units per screen pixel (set by the view each frame): sizes a callout box as the JS does with `view.z`
    pub px: f64,
    /// the pointer while an area outline is open (its rubber-band corner)
    pub hover: Option<[f64; 2]>,
    /// the calibrate line just drawn: its length in PDF units, waiting for the real distance
    pub cal_len: Option<f64>,
    /// the real distance typed for it, and the unit it is in
    pub cal_text: String,
    pub cal_unit: String,
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
            px: 1.0,
            hover: None,
            cal_len: None,
            cal_text: String::new(),
            cal_unit: "ft-in".into(),
        }
    }
}

/// The markup fields a gesture from `a` to `b` (and the pen's whole path) makes.
fn fields(tool: &str, sheet: &str, pts: &[[f64; 2]], color: &str, text: &str, px: f64) -> Option<Map<String, Value>> {
    let (a, b) = (pts.first()?, pts.last()?);
    // a click with a shape tool makes nothing (the engine would keep a zero-size shape nobody can see)
    let tiny = (a[0] - b[0]).abs() < 1.0 && (a[1] - b[1]).abs() < 1.0;
    if tiny && !matches!(tool, "text" | "callout" | "area") {
        return None;
    }
    let rect = json!([a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]);
    let mut f = Map::new();
    f.insert("type".into(), json!(tool));
    f.insert("sheet".into(), json!(sheet));
    // the measuring types keep their own blue (model.js styles), as the JS app does: colour applies to COLOR_TYPES
    if !matches!(tool, "measure" | "area") {
        f.insert("style".into(), json!({ "color": color }));
    }
    match tool {
        "pen" | "highlight" => {
            if pts.len() < 2 {
                return None;
            }
            f.insert("pts".into(), json!(pts));
        }
        "line" | "arrow" | "measure" => {
            f.insert("pts".into(), json!([a, b]));
        }
        "area" => {
            if pts.len() < 3 {
                return None;
            }
            f.insert("pts".into(), json!(pts));
        }
        "callout" => {
            // finishCallout: a click puts the knee up and to the right of the tip; the box sits past the knee,
            // 190 screen px wide (clamped to 120–300 pt) and one line of the callout's 12 pt type tall
            let px = if px.is_finite() && px > 0.0 { px } else { 1.0 };
            let knee = if tiny { [a[0] + 50.0 * px, a[1] + 40.0 * px] } else { *b };
            let w = (190.0 * px).clamp(120.0, 300.0);
            let h = 12.0 * 1.2 + 10.0;
            let gap = 6.0 * px;
            let sx = if knee[0] >= a[0] { knee[0] + gap } else { knee[0] - gap - w };
            f.insert("rect".into(), json!([sx, knee[1] - h / 2.0, sx + w, knee[1] + h / 2.0]));
            f.insert("pts".into(), json!([a, knee]));
            f.insert("text".into(), json!(text));
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
        let (kind, pts) = match self.tool.as_str() {
            // the calibrate line is drawn as a dimension while it is dragged
            "calibrate" => ("measure", self.drag.clone()),
            "area" => ("area", self.drag.iter().copied().chain(self.hover).collect()),
            k => (k, self.drag.clone()),
        };
        let f = fields(kind, sheet, &pts, &self.color, &self.text, self.px)?;
        let mut env = cc_markup::model::Env::default();
        let m = cc_markup::model::make_markup(&mut env, &f);
        cc_markup::model::geometry_ok(&m).then_some(m)
    }

    /// Whether the tool draws (a drag with it makes a markup rather than panning).
    pub fn draws(&self) -> bool {
        !matches!(self.tool.as_str(), "select" | "pan")
    }

    /// Whether the tool is clicked corner by corner (an area outline) rather than dragged.
    pub fn clicks(&self) -> bool {
        self.tool == "area"
    }

    /// An outline tool's click: one more corner.
    pub fn click(&mut self, p: [f64; 2]) {
        if self.drag.len() < MAX_POLY {
            self.drag.push(p);
        }
    }

    /// The outline is finished (double-click or Enter): one undo step, or nothing when it has under 3 corners.
    pub fn finish(&mut self, store: &mut Store, sheet: &str) -> Option<String> {
        // a double-click lands its second press on the last corner: drop the repeat
        if let [.., p, q] = self.drag.as_slice()
            && (p[0] - q[0]).abs() < 1e-9
            && (p[1] - q[1]).abs() < 1e-9
        {
            self.drag.pop();
        }
        self.hover = None;
        self.end(store, sheet)
    }

    /// Drop the gesture under way (Esc).
    pub fn cancel(&mut self) {
        self.drag.clear();
        self.hover = None;
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
        if self.tool == "calibrate" {
            let (a, b) = (pts.first()?, pts.last()?);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if len < 1.0 {
                return None;
            }
            self.cal_len = Some(len);
            self.message = t("cal.two.text", &[("d", Fill::S(cc_markup::js::to_fixed(len / 72.0, 3)))]);
            return None;
        }
        let f = fields(&self.tool, sheet, &pts, &self.color, &self.text, self.px)?;
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

    /// Set the sheet's scale from the calibrate line and the typed distance (cal.set, one undo step).
    pub fn apply_calibration(&mut self, store: &mut Store, sheet: &str) -> bool {
        let Some(len) = self.cal_len else { return false };
        let Some(mm) = parse_length(&self.cal_text, &self.cal_unit).filter(|v| *v > 0.0) else {
            self.message = t0("cal.bad_length");
            return false;
        };
        let Some(cal) = calibration_from(len, mm, &self.cal_unit) else {
            self.message = t0("cal.bad_length");
            return false;
        };
        self.set_scale(store, sheet, Some(cal));
        self.cal_len = None;
        self.cal_text.clear();
        // as the JS: after calibrating, the Length tool is next
        self.tool = "measure".into();
        true
    }

    /// Set (or with `None` remove) the sheet's scale: one undo step labelled as the JS labels it.
    pub fn set_scale(&mut self, store: &mut Store, sheet: &str, cal: Option<Cal>) {
        let label = if cal.is_some() { t0("cal.set") } else { t0("cal.remove") };
        let done = cal.as_ref().map(|c| t("cal.done", &[("scale", Fill::S(c.label.clone()))]));
        let r = store.change(&label, |tx| {
            tx.set_calibration(sheet, cal);
            Ok(())
        });
        self.message = match r {
            Ok(()) => done.unwrap_or_else(|| t0("cal.removed")),
            Err(e) => e.to_string(),
        };
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

    #[test]
    fn calibrate_then_measure_reads_the_real_length() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "calibrate".into(), ..MarkupTools::default() };
        // 144 pt on the sheet is 10 ft
        t.begin([0.0, 0.0]);
        t.moved([144.0, 0.0]);
        assert!(t.end(&mut store, "f1:0").is_none());
        assert_eq!(t.cal_len, Some(144.0));
        t.cal_text = "10'".into();
        assert!(t.apply_calibration(&mut store, "f1:0"));
        assert_eq!(t.tool, "measure");
        let cal = store.calibrations.get("f1:0").cloned().unwrap();
        assert!((cal.mm_per_pt - 3048.0 / 144.0).abs() < 1e-9);
        // a 72 pt dimension now reads 5 ft
        t.begin([0.0, 10.0]);
        t.moved([72.0, 10.0]);
        let id = t.end(&mut store, "f1:0").unwrap();
        let m = store.get(&id).unwrap();
        assert_eq!(m.kind, "measure");
        assert_eq!(cc_markup::units::measure(72.0, Some(&cal)), "5'-0\"");
        // undo takes the dimension, then the scale
        store.undo();
        store.undo();
        assert!(store.calibrations.get("f1:0").is_none());
    }

    #[test]
    fn a_bad_distance_sets_no_scale() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "calibrate".into(), cal_len: Some(100.0), cal_text: "banana".into(), ..MarkupTools::default() };
        assert!(!t.apply_calibration(&mut store, "f1:0"));
        assert!(store.calibrations.is_empty());
        assert!(store.undo_stack.is_empty());
    }

    #[test]
    fn an_area_is_clicked_corner_by_corner() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "area".into(), ..MarkupTools::default() };
        assert!(t.clicks());
        for p in [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [100.0, 50.0]] {
            t.click(p);
        }
        let id = t.finish(&mut store, "f1:0").unwrap();
        let m = store.get(&id).unwrap();
        assert_eq!(m.kind, "area");
        assert_eq!(m.pts.as_ref().map(Vec::len), Some(3));
        // two corners are not an area
        t.click([0.0, 0.0]);
        t.click([10.0, 0.0]);
        assert!(t.finish(&mut store, "f1:0").is_none());
    }

    #[test]
    fn a_callout_points_at_the_press_and_boxes_past_the_release() {
        let mut store = Store::new();
        let mut t = MarkupTools { tool: "callout".into(), text: "Check".into(), ..MarkupTools::default() };
        t.begin([100.0, 100.0]);
        t.moved([200.0, 150.0]);
        let id = t.end(&mut store, "f1:0").unwrap();
        let m = store.get(&id).unwrap();
        assert_eq!(m.pts, Some(vec![[100.0, 100.0], [200.0, 150.0]]));
        let r = m.rect.unwrap();
        assert!(r[0] > 200.0 && r[1] < 150.0 && r[3] > 150.0);
        assert_eq!(m.text, "Check");
    }
}
