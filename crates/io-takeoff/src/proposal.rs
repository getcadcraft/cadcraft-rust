//! `bid.js` `proposalPdf` — the proposal PDF: heading, project / to / from / date, the price block (each markup and
//! the bid), alternates with their markups, the scope (items and quantities, no prices), inclusions, exclusions, terms
//! and the acceptance lines. pdf-lib's operators and Helvetica metrics, so pdf.js reads the same pages as from the JS's.

use serde_json::Value;

use cc_takeoff::bid::{self, bid_summary, normal_bid};
use cc_takeoff::i18n::{F, t, tf};
use cc_takeoff::jv;
use cc_takeoff::model;
use cc_takeoff::units::format_cents;

use crate::pdfmarks::{Canvas, Font, Rgb, win_ansi, win_ansi_nl};
use crate::pdfw::{PO, Writer};
use crate::xlsx::Stamp;

const W: f64 = 612.0;
const H: f64 = 792.0;
const M: f64 = 54.0;
const INK: Rgb = (0.1, 0.1, 0.12);
const GREY: Rgb = (0.38, 0.38, 0.42);
const RULE: Rgb = (0.75, 0.75, 0.78);

/// Words → lines no wider than `w` points.
fn wrap(s: &str, font: Font, size: f64, w: f64) -> Vec<String> {
    let mut out = Vec::new();
    for para in win_ansi_nl(s).split('\n') {
        let mut line = String::new();
        for word in para.split(jv::is_space).filter(|x| !x.is_empty()) {
            let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if font.width(&next, size) <= w || line.is_empty() {
                line = next;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            let mut guard: u32 = 0;
            while font.width(&line, size) > w && line.chars().count() > 1 && guard < 10_000 {
                guard = guard.saturating_add(1);
                let chars: Vec<char> = line.chars().collect();
                let mut k = chars.len().saturating_sub(1);
                while k > 1 && font.width(&chars.iter().take(k).collect::<String>(), size) > w {
                    k = k.saturating_sub(1);
                }
                out.push(chars.iter().take(k).collect());
                line = chars.iter().skip(k).collect();
            }
        }
        out.push(line);
    }
    out
}

fn pct_text(bp: f64) -> String {
    let s = cc_m3d::js::to_fixed(bp / 100.0, 2);
    format!("{} %", cc_takeoff::units::strip_zeros(&s))
}

/// `toLocaleString('en-US', { maximumFractionDigits: 2 })` of a value already rounded to two decimals.
fn locale2(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "NaN".into()
        } else if x > 0.0 {
            "∞".into()
        } else {
            "-∞".into()
        };
    }
    let s = cc_m3d::js::to_fixed(x.abs(), 2);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    let (i, f) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
    let g = cc_takeoff::units::group3(i);
    let neg = x < 0.0 && s != "0";
    format!("{}{g}{}{f}", if neg { "-" } else { "" }, if f.is_empty() { "" } else { "." })
}

struct Doc {
    w: Writer,
    pages: Vec<Canvas>,
    y: f64,
}
impl Doc {
    fn cv(&mut self) -> &mut Canvas {
        if self.pages.is_empty() {
            self.pages.push(Canvas::new());
        }
        let n = self.pages.len().saturating_sub(1);
        match self.pages.get_mut(n) {
            Some(c) => c,
            None => unreachable_canvas(),
        }
    }
    fn page_no(&self) -> usize {
        self.pages.len()
    }
    fn foot(&mut self) {
        let s = win_ansi(&tf("bid.pdf.page", &[("n", F::from(self.page_no()))]));
        let x = W - M - Font::Regular.width(&s, 8.0);
        self.cv().text(&s, x, M - 18.0, 8.0, Font::Regular, GREY, 0.0);
    }
    fn need(&mut self, h: f64) {
        if self.y - h < M + 24.0 {
            self.foot();
            self.pages.push(Canvas::new());
            self.y = H - M;
        }
    }
    fn text_at(&mut self, s: &str, x: f64, size: f64, f: Font, color: Rgb) {
        let y = self.y;
        self.cv().text(&win_ansi(s), x, y, size, f, color, 0.0);
    }
    fn right(&mut self, s: &str, xr: f64, size: f64, f: Font, color: Rgb) {
        let ws = win_ansi(s);
        let w = f.width(&ws, size);
        let y = self.y;
        self.cv().text(&ws, xr - w, y, size, f, color, 0.0);
    }
    fn para(&mut self, s: &str, size: f64, f: Font, color: Rgb, indent: f64) {
        for line in wrap(s, f, size, W - 2.0 * M - indent) {
            self.need(size * 1.4);
            self.text_at(&line, M + indent, size, f, color);
            self.y -= size * 1.4;
        }
    }
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, thickness: f64, color: Rgb) {
        let c = &mut self.cv().c;
        c.op("q", &[]);
        c.op("RG", &[color.0, color.1, color.2]);
        c.op("w", &[thickness]);
        c.dash(&[], 0.0);
        c.op("m", &[x0, y0]);
        c.op("m", &[x0, y0]);
        c.op("l", &[x1, y1]);
        c.op("S", &[]);
        c.op("Q", &[]);
    }
    fn hr(&mut self) {
        let y = self.y + 4.0;
        self.line(M, y, W - M, y, 0.6, RULE);
        self.y -= 8.0;
    }
    fn heading(&mut self, s: &str) {
        self.y -= 6.0;
        self.need(30.0);
        self.text_at(s, M, 12.0, Font::Bold, INK);
        self.y -= 16.0;
        self.hr();
    }
}
fn unreachable_canvas() -> &'static mut Canvas {
    Box::leak(Box::default())
}

/// The proposal for a takeoff state; `date_ms` is its date (ms since 1970, UTC).
pub fn proposal_pdf(state: &Value, date_ms: f64) -> Vec<u8> {
    let est = model::base_estimate(state);
    let alts: Vec<bid::AltIn> = model::alternates(state).iter().map(model::Alternate::bid_in).collect();
    proposal_with(state, &est, &alts, date_ms, None)
}

/// `proposalPdf(PDFLib, { state, est, alts, date, items })`.
pub fn proposal_with(state: &Value, est: &model::Estimate, alts: &[bid::AltIn], date_ms: f64, items: Option<&[Value]>) -> Vec<u8> {
    let b = normal_bid(state.get("bid"));
    let sum = bid_summary(est.mat_total, est.lab_total, est.incomplete, Some(&b), alts);
    let mut d = Doc { w: Writer::new(1), pages: vec![Canvas::new()], y: H - M };
    let bs = |k: &str| jv::to_string_or_empty(b.get(k));
    d.text_at(&t("bid.pdf.h"), M, 22.0, Font::Bold, INK);
    d.y -= 30.0;
    let date = Stamp::from_ms(date_ms);
    let when = format!("{:04}-{:02}-{:02}", date.year, date.month, date.day);
    let project =
        if !bs("project").is_empty() { bs("project") } else { jv::to_string_or_empty(state.get("name").filter(|v| jv::truthy(Some(v)))) };
    let meta = [(t("bid.pdf.project"), project), (t("bid.pdf.to"), bs("to")), (t("bid.pdf.from"), bs("by")), (t("bid.pdf.date"), when)];
    for (k, v) in &meta {
        if v.is_empty() {
            continue;
        }
        let lines = wrap(v, Font::Regular, 10.0, W - 2.0 * M - 90.0);
        d.need(lines.len() as f64 * 14.0);
        d.text_at(k, M, 10.0, Font::Bold, GREY);
        for line in &lines {
            d.text_at(line, M + 90.0, 10.0, Font::Regular, INK);
            d.y -= 14.0;
        }
    }
    d.y -= 4.0;
    d.heading(&t("bid.pdf.price"));
    for l in &sum.lines {
        d.need(16.0);
        let label = match l.pct.filter(|p| *p != 0.0) {
            Some(p) => tf("bid.l.pct", &[("label", F::S(l.label.clone())), ("pct", F::S(pct_text(p)))]),
            None => l.label.clone(),
        };
        let total = l.key == "total";
        let size = if total { 12.0 } else { 10.0 };
        let f = if l.sub { Font::Bold } else { Font::Regular };
        d.text_at(&label, M + if l.sub { 0.0 } else { 12.0 }, size, f, INK);
        d.right(&format_cents(l.cents), W - M, size, f, INK);
        d.y -= if total { 20.0 } else { 15.0 };
    }
    if sum.incomplete {
        d.para(&t("bid.pdf.incomplete"), 9.0, Font::Regular, GREY, 0.0);
    }
    if !sum.alts.is_empty() {
        d.heading(&t("bid.pdf.alts"));
        for a in &sum.alts {
            d.need(15.0);
            d.text_at(
                &tf("bid.pdf.alt", &[("name", F::S(a.name.clone())), ("kind", F::S(jv::lower(&t(&format!("alt.{}", a.alt)))))]),
                M,
                10.0,
                Font::Regular,
                INK,
            );
            d.right(
                &format!("{}{}", if a.with_markup >= 0.0 { "+" } else { "-" }, format_cents(a.with_markup.abs())),
                W - M,
                10.0,
                Font::Bold,
                INK,
            );
            d.y -= 15.0;
        }
        d.para(&t("bid.pdf.alts.note"), 9.0, Font::Regular, GREY, 0.0);
    }
    let rows: Vec<Value> = match items {
        Some(i) => i.to_vec(),
        None => est
            .rows
            .iter()
            .filter(|r| matches!(model::rstr(r, "kind"), "item" | "condition") && model::rnum(r, "marks") > 0.0)
            .cloned()
            .collect(),
    };
    if jv::truthy(b.get("items")) && !rows.is_empty() {
        d.heading(&t("bid.pdf.scope"));
        for r in &rows {
            let q = format!("{} {}", locale2(cc_m3d::js::round(model::rnum(r, "qty") * 100.0) / 100.0), jv::to_string(r.get("unit")));
            let name = if model::rstr(r, "kind") == "condition" {
                tf(
                    "qty.cond.name",
                    &[
                        ("item", F::S(jv::to_string(r.get("name")))),
                        ("type", F::S(jv::lower(&t(&format!("type.{}", model::rstr(r, "cond")))))),
                    ],
                )
            } else {
                jv::to_string(r.get("name"))
            };
            let lines = wrap(&name, Font::Regular, 10.0, W - 2.0 * M - 120.0);
            d.need(lines.len() as f64 * 14.0);
            d.right(&q, W - M, 10.0, Font::Regular, GREY);
            for line in &lines {
                d.text_at(line, M + 12.0, 10.0, Font::Regular, INK);
                d.y -= 14.0;
            }
        }
    }
    let list = |d: &mut Doc, title: &str, body: &str| {
        let items2: Vec<String> = body
            .split('\n')
            .map(|s| {
                // s.replace(/^\s*[-*•]\s*/, '').trim()
                let t0 = s.trim_start_matches(jv::is_space);
                let t1 = match t0.chars().next() {
                    Some('-' | '*' | '•') => {
                        t0.get(t0.chars().next().map_or(0, char::len_utf8)..).unwrap_or("").trim_start_matches(jv::is_space)
                    }
                    _ => s,
                };
                cc_m3d::js::trim(t1).to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        if items2.is_empty() {
            return;
        }
        d.heading(title);
        for it in &items2 {
            let lines = wrap(it, Font::Regular, 10.0, W - 2.0 * M - 16.0);
            d.need(lines.len() as f64 * 14.0);
            d.text_at("•", M + 2.0, 10.0, Font::Regular, INK);
            for line in &lines {
                d.text_at(line, M + 16.0, 10.0, Font::Regular, INK);
                d.y -= 14.0;
            }
        }
    };
    list(&mut d, &t("bid.pdf.incl"), &bs("incl"));
    list(&mut d, &t("bid.pdf.excl"), &bs("excl"));
    if !cc_m3d::js::trim(&bs("terms")).is_empty() {
        d.heading(&t("bid.pdf.terms"));
        d.para(&bs("terms"), 10.0, Font::Regular, INK, 0.0);
    }
    d.y -= 10.0;
    d.need(80.0);
    d.heading(&t("bid.pdf.accept"));
    d.para(&tf("bid.pdf.accept.text", &[("total", F::S(format_cents(sum.total)))]), 10.0, Font::Regular, INK, 0.0);
    d.y -= 26.0;
    d.need(30.0);
    for (x, label) in [(M, t("bid.pdf.sign")), (M + 300.0, t("bid.pdf.date"))] {
        let y = d.y;
        d.line(x, y + 12.0, x + 220.0, y + 12.0, 0.6, INK);
        d.cv().text(&win_ansi(&label), x, y, 8.0, Font::Regular, GREY, 0.0);
    }
    d.foot();
    // the document: catalog, page tree, pages, fonts, Info
    let mut w = d.w;
    let pages_n = w.alloc();
    let helv = w.add(&PO::dict(vec![
        ("Type", PO::name("Font")),
        ("Subtype", PO::name("Type1")),
        ("BaseFont", PO::name("Helvetica")),
        ("Encoding", PO::name("WinAnsiEncoding")),
    ]));
    let helv_b = w.add(&PO::dict(vec![
        ("Type", PO::name("Font")),
        ("Subtype", PO::name("Type1")),
        ("BaseFont", PO::name("Helvetica-Bold")),
        ("Encoding", PO::name("WinAnsiEncoding")),
    ]));
    let mut kids = Vec::new();
    for cv in &d.pages {
        let content = w.add_stream(vec![], &cv.c.bytes);
        let fonts: Vec<(String, PO)> =
            cv.fonts.iter().map(|(f, n)| (n.clone(), if *f == Font::Regular { helv.clone() } else { helv_b.clone() })).collect();
        let page = PO::dict(vec![
            ("Type", PO::name("Page")),
            ("Parent", PO::Ref(pages_n, 0)),
            ("MediaBox", PO::Arr(vec![PO::Num(0.0), PO::Num(0.0), PO::Num(W), PO::Num(H)])),
            ("Resources", PO::dict(vec![("Font", PO::Dict(fonts)), ("ExtGState", PO::Dict(cv.ext_g_states.clone()))])),
            ("Contents", PO::Arr(vec![content])),
        ]);
        kids.push(w.add(&page));
    }
    let count = kids.len() as f64;
    let mut pages_body = Vec::new();
    crate::pdfw::write(&mut pages_body, &PO::dict(vec![("Type", PO::name("Pages")), ("Kids", PO::Arr(kids)), ("Count", PO::Num(count))]));
    w.put(pages_n, 0, pages_body);
    let catalog = w.add(&PO::dict(vec![("Type", PO::name("Catalog")), ("Pages", PO::Ref(pages_n, 0))]));
    let name = state.get("name").filter(|v| jv::truthy(Some(v))).map_or_else(|| "Takeoff".to_string(), |v| jv::to_string(Some(v)));
    let title = win_ansi(&tf("bid.pdf.title", &[("name", F::S(name))]));
    let pdate = crate::pdfw::pdf_date(&date);
    let info = w.add(&PO::dict(vec![
        ("Producer", PO::Str(b"TakeoffCraft (pdf-lib)".to_vec())),
        ("Creator", PO::Str(b"TakeoffCraft".to_vec())),
        ("CreationDate", PO::Str(pdate.clone().into_bytes())),
        ("ModDate", PO::Str(pdate.into_bytes())),
        ("Title", PO::Str(title.chars().filter_map(crate::stdfont::winansi).collect())),
    ]));
    w.finish(&[], vec![("Root", catalog), ("Info", info)])
}
