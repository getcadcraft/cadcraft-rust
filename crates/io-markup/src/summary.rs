//! summary.js `summaryPdf` — the markup summary as a PDF report (0.4 K6): a landscape Letter report with a title
//! block, a table (No., Sheet, Type, Subject, Text or comment, Value, Author, Status, Replies) with wrapped cells,
//! repeated headers and page numbers. Helvetica WinAnsi as pdf-lib's standard font encodes it: characters outside it
//! print "?" and are counted. The pages are written the way pdf-lib's `drawText` / `drawLine` write them.

use crate::libdoc::{LibDoc, Ref, deflate};
use crate::obj::{Dict, Obj, Stream, number_to_string};
use crate::pages::{create, info_mut, insert_page, new_page};
use cc_markup::csv::SheetRef;
use cc_markup::font::{cp1252, text_width, wrap_text};
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, STATUSES, is_type, status_name, type_name};
use std::fmt::Write as _;

/// What pdf-lib's standard Helvetica can encode (WinAnsi): the byte, or None (drawText would throw).
pub fn helv_byte(c: char) -> Option<u8> {
    let u = u32::from(c);
    if (0x20..0x7f).contains(&u) || (0xa0..=0xff).contains(&u) {
        return u8::try_from(u).ok();
    }
    cp1252(c)
}

/// `safe(font, text, count)`.
fn safe(text: &str, count: &mut usize) -> String {
    text.chars()
        .map(|c| {
            if helv_byte(c).is_some() {
                c
            } else {
                *count = count.saturating_add(1);
                '?'
            }
        })
        .collect()
}

struct Page {
    r: Ref,
    ops: String,
}

struct Doc {
    lib: LibDoc,
    pages: Vec<Page>,
    helv: Ref,
    bold: Ref,
}

const W: f64 = 792.0;
const H: f64 = 612.0;
const M: f64 = 36.0;

impl Doc {
    fn add_page(&mut self) -> usize {
        let n = self.lib.pages().len();
        let r = new_page(&mut self.lib, W, H);
        let _ = insert_page(&mut self.lib, n, r);
        self.pages.push(Page { r, ops: String::new() });
        self.pages.len().saturating_sub(1)
    }
    /// `page.drawText(text, {x, y, size, font, color})` (one line: the text never holds a line break here).
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, page: usize, s: &str, x: f64, y: f64, size: f64, bold: bool, color: [f64; 3]) {
        let Some(p) = self.pages.get_mut(page) else { return };
        let mut hex = String::new();
        for c in s.chars() {
            let _ = write!(hex, "{:02X}", helv_byte(c).unwrap_or(b'?'));
        }
        let f = if bold { "Helvetica-Bold-1" } else { "Helvetica-1" };
        let n = number_to_string;
        let _ = write!(
            p.ops,
            "q\nBT\n{} {} {} rg\n/{f} {} Tf\n24 TL\n1 0 0 1 {} {} Tm\n<{hex}> Tj\nT*\nET\nQ\n",
            n(color[0]),
            n(color[1]),
            n(color[2]),
            n(size),
            n(x),
            n(y)
        );
    }
    /// `page.drawLine({start, end, thickness, color})`.
    fn line(&mut self, page: usize, a: [f64; 2], b: [f64; 2], w: f64, color: [f64; 3]) {
        let Some(p) = self.pages.get_mut(page) else { return };
        let n = number_to_string;
        let _ = write!(
            p.ops,
            "q\n{} {} {} RG\n{} w\n[] 0 d\n{} {} m\n{} {} m\n{} {} l\nS\nQ\n",
            n(color[0]),
            n(color[1]),
            n(color[2]),
            n(w),
            n(a[0]),
            n(a[1]),
            n(a[0]),
            n(a[1]),
            n(b[0]),
            n(b[1])
        );
    }
    fn finish(mut self) -> LibDoc {
        let fonts = Dict::new()
            .with("Helvetica-1", Obj::Ref(self.helv.0, self.helv.1))
            .with("Helvetica-Bold-1", Obj::Ref(self.bold.0, self.bold.1));
        for p in std::mem::take(&mut self.pages) {
            let used_bold = p.ops.contains("/Helvetica-Bold-1 ");
            let used_helv = p.ops.contains("/Helvetica-1 ");
            let cs = self.lib.register(Obj::Stream(Box::new(Stream {
                dict: Dict::new().with("Filter", Obj::name("FlateDecode")),
                data: deflate(p.ops.as_bytes()),
            })));
            let mut f = Dict::new();
            for (k, v) in &fonts.0 {
                if (k == "Helvetica-1" && used_helv) || (k == "Helvetica-Bold-1" && used_bold) {
                    f.set(k, v.clone());
                }
            }
            if let Some(d) = self.lib.get_mut(p.r).and_then(Obj::as_dict_mut) {
                d.set(
                    "Resources",
                    Obj::Dict(
                        Dict::new()
                            .with("Font", Obj::Dict(f))
                            .with("XObject", Obj::Dict(Dict::new()))
                            .with("ExtGState", Obj::Dict(Dict::new())),
                    ),
                );
                d.set("Annots", Obj::Arr(Vec::new()));
                d.set("Contents", Obj::Arr(vec![Obj::Ref(cs.0, cs.1)]));
            }
        }
        for (r, base) in [(self.helv, "Helvetica"), (self.bold, "Helvetica-Bold")] {
            self.lib.put(
                r,
                Obj::Dict(
                    Dict::new()
                        .with("Type", Obj::name("Font"))
                        .with("Subtype", Obj::name("Type1"))
                        .with("BaseFont", Obj::name(base))
                        .with("Encoding", Obj::name("WinAnsiEncoding")),
                ),
            );
        }
        self.lib
    }
}

/// What [`summary_pdf`] gives.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryPdf {
    pub bytes: Vec<u8>,
    pub missing: usize,
    pub pages: usize,
}

/// `summaryPdf(lib, markups, {sheetOf, measureOf, title, date})` at time `now` (the file's dates).
pub fn summary_pdf(
    markups: &[&Markup],
    sheet_of: &dyn Fn(&Markup) -> Option<SheetRef>,
    measure_of: &dyn Fn(&Markup) -> String,
    title: &str,
    date_ms: f64,
    now: f64,
) -> SummaryPdf {
    let mut lib = create(now);
    let helv = lib.next_ref();
    let bold = lib.next_ref();
    let mut d = Doc { lib, pages: Vec::new(), helv, bold };
    let mut count = 0usize;
    let size = 8.0;
    let lh = size * 1.25;
    let cols: [(String, f64); 9] = [
        (t0("csv.no"), 28.0),
        (t0("csv.sheet"), 90.0),
        (t0("csv.type"), 62.0),
        (t0("csv.subject"), 74.0),
        (t0("sum.col.text"), 190.0),
        (t0("sum.col.value"), 80.0),
        (t0("csv.author"), 70.0),
        (t0("csv.status"), 56.0),
        (t0("csv.replies"), 70.0),
    ];
    let ink = [0.09, 0.13, 0.17];
    let soft = [0.45, 0.48, 0.52];
    let line = [0.8, 0.82, 0.85];
    let rows: Vec<[String; 9]> = markups
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let s = sheet_of(m).unwrap_or_default();
            let first = if matches!(m.kind.as_str(), "stamp" | "image" | "count") { m.label.clone() } else { m.text.clone() };
            let text = [first, m.comment.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" — ");
            let ty = if m.kind == "foreign" {
                m.src.as_ref().map(|s| s.subtype.clone()).filter(|x| !x.is_empty()).unwrap_or_else(|| type_name("foreign"))
            } else if is_type(&m.kind) {
                type_name(&m.kind)
            } else {
                m.kind.clone()
            };
            let status = if STATUSES.contains(&m.status.as_str()) { status_name(&m.status) } else { m.status.clone() };
            let replies = match m.replies.last() {
                Some(last) => {
                    let l = format!("{}{}", if last.author.is_empty() { String::new() } else { format!("{}: ", last.author) }, last.text);
                    t("sum.replies", &[("n", Fill::N(m.replies.len() as f64)), ("last", Fill::S(l))])
                }
                None => String::new(),
            };
            let no = m.punch.as_ref().map_or_else(|| (i.saturating_add(1)) as f64, |p| p.n);
            [cc_markup::js::num_to_string(no), s.title, ty, m.subject.clone(), text, measure_of(m), m.author.clone(), status, replies]
        })
        .collect();
    let mut page = 0usize;
    let mut y = 0.0;
    let mut page_no = 0usize;
    let mut header = |d: &mut Doc, page: &mut usize, y: &mut f64, count: &mut usize| {
        *page = d.add_page();
        page_no = page_no.saturating_add(1);
        *y = H - M;
        if page_no == 1 {
            let tt = if title.is_empty() { t0("sum.title") } else { title.to_string() };
            let s = safe(&tt, count);
            d.text(*page, &s, M, *y - 14.0, 16.0, true, ink);
            *y -= 22.0;
            let date = cc_markup::js::to_iso(date_ms).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default();
            let meta = t("sum.meta", &[("date", Fill::S(date)), ("n", Fill::N(markups.len() as f64))]);
            let s = safe(&meta, count);
            d.text(*page, &s, M, *y - 10.0, 9.0, false, soft);
            *y -= 22.0;
        }
        let mut x = M;
        for (h, w) in &cols {
            let s = safe(h, count);
            d.text(*page, &s, x + 2.0, *y - size, size, true, ink);
            x += w;
        }
        *y -= lh + 3.0;
        d.line(*page, [M, *y + 1.5], [W - M, *y + 1.5], 1.0, ink);
        *y -= 2.0;
    };
    header(&mut d, &mut page, &mut y, &mut count);
    for r in &rows {
        let cells: Vec<Vec<String>> = r
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let w = cols.get(i).map_or(0.0, |c| c.1);
                let lines = wrap_text(&safe(v, &mut count), size, w - 4.0, false);
                if lines.len() > 6 {
                    let mut l: Vec<String> = lines.iter().take(5).cloned().collect();
                    l.push(format!("{} …", lines.get(5).cloned().unwrap_or_default()));
                    l
                } else {
                    lines
                }
            })
            .collect();
        let hh = (cells.iter().map(Vec::len).max().unwrap_or(0).max(1)) as f64 * lh + 3.0;
        if y - hh < M + 14.0 {
            header(&mut d, &mut page, &mut y, &mut count);
        }
        let mut x = M;
        for (i, c) in cells.iter().enumerate() {
            for (k, ln) in c.iter().enumerate() {
                d.text(page, ln, x + 2.0, y - size - k as f64 * lh, size, i == 0, ink);
            }
            x += cols.get(i).map_or(0.0, |c| c.1);
        }
        y -= hh;
        d.line(page, [M, y + 1.0], [W - M, y + 1.0], 0.4, line);
    }
    if rows.is_empty() {
        let s = safe(&t0("sum.empty"), &mut count);
        d.text(page, &s, M, y - size - 4.0, 10.0, false, soft);
    }
    let np = d.pages.len();
    for i in 0..np {
        let s = safe(&t("sum.page", &[("n", Fill::N(i.saturating_add(1) as f64)), ("of", Fill::N(np as f64))]), &mut count);
        d.text(i, &s, W - M - text_width(&s, 8.0, false), M - 16.0, 8.0, false, soft);
    }
    let mut lib = d.finish();
    if let Some(info) = info_mut(&mut lib) {
        info.set("Title", Obj::hex_text(&if title.is_empty() { t0("sum.title") } else { title.to_string() }));
        info.set("Producer", Obj::hex_text("MarkupCraft"));
    }
    SummaryPdf { bytes: lib.save(), missing: count, pages: np }
}
