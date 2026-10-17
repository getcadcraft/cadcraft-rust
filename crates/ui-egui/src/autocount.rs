//! TakeoffCraft's Auto count on the sheet view (takeoffcraft.js `runAutoCount` / `acceptAutoCount`, this sheet): box one
//! symbol, the sheet is rendered so the symbol is about 40 px, the template is cut and trimmed from it, the matcher
//! (cc-takeoff matcher.rs, at parity with match.js) searches the same raster at the right angles (or every 15°), and the
//! matches are reviewed (a slider for the match threshold, a click rejects or restores one) before they are added as one
//! undo step. Matches over a count mark the item already has start out rejected, as in the JS.

use cc_io_takeoff::pdfread::{self, Doc, PageInfo};
use cc_takeoff::geom::{P, point_in_polygon};
use cc_takeoff::i18n::{F, t, tf, tn};
use cc_takeoff::matcher;
use cc_takeoff::model::Takeoff;
use serde_json::{Value, json};

/// The matcher's floor (AC_FLOOR): nothing under it is reported, the slider starts above it.
pub const FLOOR: f64 = 0.5;
/// The slider's start (Settings → threshold, 75 % unless changed).
pub const THRESHOLD: f64 = 0.75;
/// A sheet whose raster at the symbol's size would pass this many pixels is not searched (as the JS).
const MAX_PIXELS: f64 = 40e6;
/// Most matches a search keeps for review.
const MAX_CANDS: usize = 20_000;

/// One match, in sheet (PDF user) units.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub score: f64,
    /// x, y, w, h of the box around its corners
    pub bx: [f64; 4],
    pub center: [f64; 2],
    pub corners: [[f64; 2]; 4],
    /// already counted: a mark of the item sits inside it
    pub dup: bool,
    pub rejected: bool,
}

/// A finished search waiting for review.
#[derive(Debug, Clone)]
pub struct AutoCount {
    pub item: String,
    pub sheet: String,
    pub threshold: f64,
    pub matches: Vec<Match>,
    pub skipped: usize,
    /// how long the search took (ms), for the status line
    pub ms: f64,
}

fn iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let ix = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let iy = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    let i = ix.max(0.0) * iy.max(0.0);
    let u = a[2] * a[3] + b[2] * b[3] - i;
    if u > 0.0 { i / u } else { 0.0 }
}

/// Search `page` for the symbol boxed by the corners `bx` (PDF user space). `own` are the item's count marks on the sheet.
pub fn run(
    doc: &Doc,
    info: &PageInfo,
    item: &str,
    sheet: &str,
    bx: ([f64; 2], [f64; 2]),
    any_angle: bool,
    own: &[[f64; 2]],
) -> Result<AutoCount, String> {
    let started = web_time::Instant::now();
    let (a, b) = bx;
    let tr = &info.transform;
    let ds = [a, b, [a[0], b[1]], [b[0], a[1]]].map(|p| pdfread::to_viewport(tr, p[0], p[1]));
    let (min_x, max_x) = ds.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let (min_y, max_y) = ds.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let w = matcher::work_scale(max_x - min_x, max_y - min_y, 40.0);
    let mut out = AutoCount { item: item.into(), sheet: sheet.into(), threshold: THRESHOLD, matches: Vec::new(), skipped: 0, ms: 0.0 };
    if info.width * info.height * w * w > MAX_PIXELS {
        out.skipped = 1;
        return Ok(out);
    }
    let src = pdfread::render_ink(doc, info.index, w * info.user_unit)?;
    let tpl = matcher::trim(&matcher::crop(&src, min_x * w, min_y * w, (max_x - min_x) * w, (max_y - min_y) * w), 0.08).img;
    if let Some(why) = matcher::template_problem(&tpl) {
        return Err(why);
    }
    let rotations: Vec<f64> = if any_angle { (0..24).map(|i| f64::from(i) * 15.0).collect() } else { vec![0.0, 90.0, 180.0, 270.0] };
    let found = matcher::find_symbol(&src, &tpl, &rotations, FLOOR, matcher::BLUR, None, None).unwrap_or_default();
    let to_user = |x: f64, y: f64| {
        let (u, v) = pdfread::to_pdf(tr, x / w, y / w);
        [u, v]
    };
    for c in found.into_iter().take(MAX_CANDS) {
        let corners = [to_user(c.x, c.y), to_user(c.x + c.w, c.y), to_user(c.x + c.w, c.y + c.h), to_user(c.x, c.y + c.h)];
        let poly: Vec<P> = corners.iter().map(|q| P::new(q[0], q[1])).collect();
        let dup = own.iter().any(|m| point_in_polygon(&P::new(m[0], m[1]), &poly));
        let (bx0, by0) = corners.iter().fold((f64::INFINITY, f64::INFINITY), |(x, y), q| (x.min(q[0]), y.min(q[1])));
        let (bx1, by1) = corners.iter().fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |(x, y), q| (x.max(q[0]), y.max(q[1])));
        out.matches.push(Match {
            score: c.score,
            bx: [bx0, by0, bx1 - bx0, by1 - by0],
            center: to_user(c.x + c.w / 2.0, c.y + c.h / 2.0),
            corners,
            dup,
            rejected: dup,
        });
    }
    out.ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(out)
}

impl AutoCount {
    /// `acVisible`: the matches at or over the threshold, best first, none overlapping a better one by more than 25 %
    /// (match.js `suppress`). Indices into `matches`.
    pub fn visible(&self) -> Vec<usize> {
        let mut list: Vec<usize> =
            (0..self.matches.len()).filter(|&i| self.matches.get(i).is_some_and(|m| m.score >= self.threshold)).collect();
        // a stable sort, best first, as Array.prototype.sort
        list.sort_by(|&i, &j| {
            let (a, b) = (self.matches.get(i).map_or(0.0, |m| m.score), self.matches.get(j).map_or(0.0, |m| m.score));
            b.total_cmp(&a)
        });
        let mut keep: Vec<usize> = Vec::new();
        for i in list {
            let Some(m) = self.matches.get(i) else { continue };
            if !keep.iter().any(|&k| self.matches.get(k).is_some_and(|q| iou(&q.bx, &m.bx) > 0.25)) {
                keep.push(i);
            }
        }
        keep
    }

    /// The matches that will be added.
    pub fn adding(&self) -> Vec<usize> {
        self.visible().into_iter().filter(|&i| self.matches.get(i).is_some_and(|m| !m.rejected)).collect()
    }

    /// A click at `p` (PDF user space) rejects or restores the visible match under it (`toggleCandidate`).
    pub fn toggle(&mut self, p: [f64; 2]) -> bool {
        let hit = self.visible().into_iter().find(|&i| {
            self.matches.get(i).is_some_and(|m| {
                let poly: Vec<P> = m.corners.iter().map(|q| P::new(q[0], q[1])).collect();
                point_in_polygon(&P::new(p[0], p[1]), &poly)
            })
        });
        match hit.and_then(|i| self.matches.get_mut(i)) {
            Some(m) => {
                m.rejected = !m.rejected;
                true
            }
            None => false,
        }
    }

    /// The review line (`renderAcBar`'s summary, this sheet).
    pub fn summary(&self) -> String {
        let vis = self.visible();
        let on = self.adding().len();
        let dups = vis.iter().filter(|&&i| self.matches.get(i).is_some_and(|m| m.dup && m.rejected)).count();
        let adding = if dups > 0 {
            tf("ac.adding.dups", &[("n", F::N(on as f64)), ("dups", F::N(dups as f64))])
        } else {
            tf("ac.adding", &[("n", F::N(on as f64))])
        };
        let mut s = format!("{}, {adding} {}", tn("ac.found", vis.len() as f64), t("ac.review"));
        if self.skipped > 0 {
            s = format!("{s} {}", tn("ac.skipped", self.skipped as f64));
        }
        s
    }

    /// `acceptAutoCount`: the kept matches become count marks of the item, one undo step ("Auto count: n item").
    pub fn accept(&self, tk: &mut Takeoff) -> Result<usize, String> {
        let on = self.adding();
        if on.is_empty() {
            return Ok(0);
        }
        let name = tk.item(&self.item).and_then(|it| it.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
        let list: Vec<Value> = on
            .iter()
            .filter_map(|&i| self.matches.get(i))
            .map(|m| json!({ "sheet": self.sheet, "pts": [{ "x": m.center[0], "y": m.center[1] }] }))
            .collect();
        let label = tf("undo.ac", &[("n", F::N(on.len() as f64)), ("item", F::S(name))]);
        tk.add_marks(&self.item, &list, Some(&label), Some("count"))?;
        Ok(on.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(score: f64, x: f64) -> Match {
        let corners = [[x, 0.0], [x + 10.0, 0.0], [x + 10.0, 10.0], [x, 10.0]];
        Match { score, bx: [x, 0.0, 10.0, 10.0], center: [x + 5.0, 5.0], corners, dup: false, rejected: false }
    }

    #[test]
    fn overlapping_matches_keep_the_best_and_the_threshold_hides_weak_ones() {
        let ac = AutoCount {
            item: "i".into(),
            sheet: "s".into(),
            threshold: 0.75,
            matches: vec![m(0.8, 0.0), m(0.9, 1.0), m(0.7, 50.0), m(0.95, 100.0)],
            skipped: 0,
            ms: 0.0,
        };
        assert_eq!(ac.visible(), vec![3, 1]);
        let mut low = ac.clone();
        low.threshold = 0.6;
        assert_eq!(low.visible(), vec![3, 1, 2]);
    }

    #[test]
    fn a_click_rejects_then_restores_and_accept_adds_one_step() {
        let mut tk = Takeoff::default();
        let id = tk.add_item(&serde_json::from_value(json!({ "name": "Outlets", "type": "count" })).unwrap()).unwrap().unwrap();
        let mut ac = AutoCount {
            item: id.clone(),
            sheet: "h:0".into(),
            threshold: 0.75,
            matches: vec![m(0.9, 0.0), m(0.9, 100.0)],
            skipped: 0,
            ms: 0.0,
        };
        assert!(ac.toggle([5.0, 5.0]));
        assert_eq!(ac.adding(), vec![1]);
        assert!(ac.toggle([5.0, 5.0]));
        assert!(!ac.toggle([500.0, 500.0]));
        assert_eq!(ac.accept(&mut tk).unwrap(), 2);
        assert_eq!(tk.marks().len(), 2);
        assert_eq!(tk.undo().as_deref(), Some("Auto count: 2 Outlets"));
        assert!(tk.marks().is_empty());
    }
}
