//! MarkupCraft's BCF 2.1 exchange (M9.19), ported from markupcraft.js exportBcf / importBcf (bcf.js, bcf-read.js): the
//! Markups list as filtered becomes one topic per markup, each with a snapshot of its part of the sheet (the page
//! rendered by hayro with that markup drawn into it, at most 900 px — view.js `snapshot`'s crop); a BCF file from a BIM
//! tool brings replies and statuses back onto the markups exported from here (matched by topic and comment GUIDs) and
//! its other topics in as notes, one undo step. The status lines are 0.5's.

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::Markup;
use serde_json::Value;

use crate::pdfview::PdfView;

/// A topic's sheet title, file name, value, box and snapshot.
type TopicFacts = (String, String, String, Option<[f64; 4]>, Option<Vec<u8>>);

/// ui-util.js `bytes(n)`.
pub fn fmt_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{} KB", cc_markup::js::to_fixed(n as f64 / 1024.0, 0))
    } else {
        format!("{} MB", cc_markup::js::to_fixed(n as f64 / 1_048_576.0, 1))
    }
}

impl PdfView {
    /// view.js `snapshot`: the markup's area of its sheet with 25 % (at least 36 pt) round it, at most 900 px, the markup
    /// drawn into the page; None when the page cannot be drawn.
    fn snapshot(&self, m: &Markup, b: [f64; 4]) -> Option<Vec<u8>> {
        let (f, i) = self.locate(m.sheet())?;
        let page = cc_io_markup::write::WritePage {
            index: i,
            markups: vec![m.clone()],
            badge: f.badge_of(i),
            cal: self.store.calibrations.get(m.sheet()).cloned(),
            legends: if m.kind == "legend" { vec![(m.id.clone(), crate::markupedit::legend_of(&self.store, m))] } else { Vec::new() },
            ..Default::default()
        };
        let opts = cc_io_markup::write::WriteOpts {
            pages: vec![page],
            flatten: cc_io_markup::write::Flatten::All,
            now_ms: self.now(),
            ..Default::default()
        };
        let (pdf, _) = cc_io_markup::write::write_marked_up(&f.bytes, &opts).ok()?;
        let doc = cc_io_takeoff::pdfread::open(&pdf).ok()?;
        let info = cc_io_takeoff::pdfread::pages(&doc).into_iter().nth(i)?;
        let tr = info.transform;
        let a = cc_io_takeoff::pdfread::to_viewport(&tr, b[0], b[1]);
        let c = cc_io_takeoff::pdfread::to_viewport(&tr, b[2], b[3]);
        let (mut x0, mut y0, mut w, mut h) = (a.0.min(c.0), a.1.min(c.1), (c.0 - a.0).abs(), (c.1 - a.1).abs());
        let mg = 36f64.max(w.max(h) * 0.25);
        x0 = (x0 - mg).max(0.0);
        y0 = (y0 - mg).max(0.0);
        let (pw, ph) = (info.width, info.height);
        let (vw, vh) = if info.rotate % 180.0 != 0.0 { (ph, pw) } else { (pw, ph) };
        w = (vw - x0).min(w + 2.0 * mg);
        h = (vh - y0).min(h + 2.0 * mg);
        // the whole page is drawn and cropped: keep the page under the renderer's pixel cap (a small markup on a big sheet
        // gets a smaller snapshot than the JS's 900 px)
        let cap = (cc_takeoff::matcher::MAX_PIXELS as f64 * 0.9 / (vw * vh).max(1.0)).sqrt();
        let sc = (900.0 / w.max(h).max(1.0)).min(6.0).min(cap);
        let annots = !self.locate(m.sheet()).is_some_and(|(f, i)| f.annots_drawn.get(i).copied().unwrap_or(false));
        let (rgba, rw, rh) = cc_io_takeoff::pdfread::render_rgba(&doc, i, sc, annots).ok()?;
        let cx = ((x0 * sc).floor().max(0.0) as usize).min(rw);
        let cy = ((y0 * sc).floor().max(0.0) as usize).min(rh);
        let cw = ((w * sc).ceil().max(1.0) as usize).min(rw - cx).max(1);
        let ch = ((h * sc).ceil().max(1.0) as usize).min(rh - cy).max(1);
        let mut out = Vec::with_capacity(cw * ch * 4);
        for y in cy..cy + ch {
            let row = rgba.get((y * rw + cx) * 4..(y * rw + cx + cw) * 4)?;
            out.extend_from_slice(row);
        }
        cc_io_markup::picture::encode_png(u32::try_from(cw).ok()?, u32::try_from(ch).ok()?, &out)
    }

    /// exportBcf: the listed markups as a .bcfzip (to the host's save).
    pub fn export_bcf(&mut self) {
        let keys: Vec<String> = self.sheet_keys();
        let order = |m: &Markup| keys.iter().position(|k| k == m.sheet()).map_or(-1.0, |i| i as f64);
        let mut list: Vec<Markup> = self.filtered().into_iter().cloned().collect();
        cc_m3d::js::sort_by(&mut list, |a, b| order(a) - order(b));
        if list.is_empty() {
            self.tools.message = t0("export.none");
            return;
        }
        let f = &self.list;
        let filtered = [&f.sheet, &f.author, &f.status, &f.kind, &f.layer, &f.space, &f.field].iter().any(|v| !v.is_empty());
        let now = self.now();
        let pre: Vec<TopicFacts> = list
            .iter()
            .map(|m| {
                let (title, file) = self.locate(m.sheet()).map(|(f, i)| (f.title(&self.store, i), f.name.clone())).unwrap_or_default();
                let bx = cc_markup::geom::bounds_of(m, &self.env_of(m)).or(m.rect);
                let png = match (&bx, self.locate(m.sheet())) {
                    (Some(b), Some(_)) => self.snapshot(m, *b),
                    _ => None,
                };
                (title, file, self.value_of(m), bx, png)
            })
            .collect();
        let topics: Vec<cc_markup::bcf::Topic<'_>> = list
            .iter()
            .zip(pre)
            .map(|(m, (sheet_title, file_name, value, bbox, png))| cc_markup::bcf::Topic {
                markup: m,
                sheet_title,
                file_name,
                value,
                bbox,
                png,
            })
            .collect();
        let (bytes, count) = cc_markup::bcf::build_bcf(&topics, &self.set_name(), now, now);
        let name = format!("{}.bcfzip", t("export.list.file", &[("set", Fill::S(self.set_name()))]));
        let size = fmt_bytes(bytes.len());
        self.outbox.push((name, bytes));
        self.tools.message =
            t(if filtered { "bcf.done.listed" } else { "bcf.done" }, &[("n", Fill::N(count as f64)), ("size", Fill::S(size))]);
    }

    /// importBcf: replies and statuses back onto the markups exported from here, other topics as notes; one undo step.
    pub fn import_bcf(&mut self, file: &str, bytes: &[u8]) {
        self.tools.cancel();
        let bcf = match cc_markup::bcfread::read_bcf(bytes) {
            Ok(b) => b,
            Err(_) => {
                self.tools.message = t("bcfin.bad", &[("file", Fill::S(file.to_string()))]);
                return;
            }
        };
        if bcf.topics.is_empty() {
            self.tools.message = t("bcfin.empty", &[("file", Fill::S(file.to_string()))]);
            return;
        }
        self.tick();
        let all: Vec<Markup> = self.store.all().into_iter().cloned().collect();
        let mut guid_of = Vec::new();
        let mut reply_guids = Vec::new();
        for m in &all {
            guid_of.push((cc_markup::bcf::guid_for(&format!("topic:{}", m.id)), m.id.clone()));
            for r in &m.replies {
                reply_guids.push(cc_markup::bcf::guid_for(&format!("comment:{}:{}", m.id, r.id)));
                if let Some(g) = r.id.strip_prefix("bcf-") {
                    reply_guids.push(g.to_string());
                }
            }
        }
        let sheets: Vec<cc_markup::bcfread::PlanSheet> = self
            .sheet_list()
            .into_iter()
            .map(|s| cc_markup::bcfread::PlanSheet {
                view: s.file.page_views.get(s.index).map_or([0.0, 0.0, 612.0, 792.0], |v| v.0),
                key: s.key,
                title: s.info.title,
                sheet_no: s.info.sheet_no,
                file: s.file.name.clone(),
            })
            .collect();
        let refs: Vec<&Markup> = all.iter().collect();
        let plan = cc_markup::bcfread::plan_import(&bcf, &refs, &guid_of, &reply_guids, &sheets, &self.sheet_key(), &self.tools.author);
        let ups = plan.get("updates").and_then(Value::as_array).cloned().unwrap_or_default();
        let adds = plan.get("adds").and_then(Value::as_array).cloned().unwrap_or_default();
        let n = |k: &str| plan.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        if ups.is_empty() && adds.is_empty() {
            self.tools.message = t("bcfin.nothing", &[("n", Fill::N(bcf.topics.len() as f64))]);
            return;
        }
        let note_style = self.tools.style_for("note");
        let r = self.store.change(&t0("undo.bcfin"), |tx| {
            for u in &ups {
                if let (Some(id), Some(p)) = (u.get("id").and_then(Value::as_str), u.get("patch").and_then(Value::as_object)) {
                    tx.update(id, p, true);
                }
            }
            let mut made = Vec::new();
            for a in &adds {
                if let Some(o) = a.as_object() {
                    let mut m = cc_markup::model::make_markup(tx.env(), o);
                    m.style = note_style.clone();
                    made.push(tx.add(m)?);
                }
            }
            Ok(made)
        });
        match r {
            Ok(made) => {
                let key = self.sheet_key();
                self.tools.sel = made.iter().filter(|m| m.sheet() == key).map(|m| m.id.clone()).collect();
                let mut parts = vec![t("bcfin.done", &[("file", Fill::S(file.to_string())), ("n", Fill::N(bcf.topics.len() as f64))])];
                for k in ["matched", "added", "replies", "unplaced"] {
                    if n(k) > 0.0 {
                        parts.push(t(&format!("bcfin.{k}"), &[("n", Fill::N(n(k)))]));
                    }
                }
                if bcf.skipped > 0 {
                    parts.push(t("bcfin.skipped", &[("n", Fill::N(bcf.skipped as f64))]));
                }
                self.tools.message = parts.join(" ");
            }
            Err(e) => self.tools.message = e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcf_goes_out_and_comes_back_and_compare_lines_up() {
        let blank =
            |grid: &str| cc_io_markup::pages::blank_pdf(Some(&serde_json::json!({ "paper": "letter", "grid": grid, "pages": 1 })), "", 0.0);
        let mut v = PdfView::open("a.pdf", &blank("square")).unwrap();
        v.set_zoom(1.0);
        v.clock = Some(1_791_810_000_000.0);
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "rect");
        v.gesture_down([100.0, 100.0], v.screen_of([100.0, 100.0]), false, false);
        v.tools.mv(&v.store, [200.0, 160.0], v.screen_of([200.0, 160.0]), false);
        v.gesture_up([200.0, 160.0], v.screen_of([200.0, 160.0]), false);
        v.export_bcf();
        let (name, bytes) = v.outbox.pop().unwrap();
        assert_eq!(name, "a markups.bcfzip");
        let bcf = cc_markup::bcfread::read_bcf(&bytes).unwrap();
        assert_eq!(bcf.topics.len(), 1);
        // our own file back: nothing new
        v.import_bcf(&name, &bytes);
        assert_eq!(v.tools.message, t("bcfin.nothing", &[("n", Fill::N(1.0))]));
        v.import_bcf("x.bcf", b"not a zip");
        assert_eq!(v.tools.message, t("bcfin.bad", &[("file", Fill::S("x.bcf".into()))]));
        // compare against another grid, nudged
        v.add_file("b.pdf", &blank("dot")).unwrap();
        v.compare_start("f1:0").unwrap();
        v.compare_mode("lines").unwrap();
        v.cmp.step = 5.0;
        v.compare_nudge(1.0, 0.0).unwrap();
        assert_eq!(v.cmp.status, t("compare.moved", &[("dx", Fill::S("5.00".into())), ("dy", Fill::S("0.00".into()))]));
        v.compare_align();
        for p in [[10.0, 10.0], [300.0, 10.0], [12.0, 10.0], [302.0, 10.0]] {
            v.gesture_down(p, v.screen_of(p), false, false);
        }
        assert!(v.cmp.aligning.is_none());
        assert_eq!(v.tools.message, t0("compare.aligned"));
    }
}
