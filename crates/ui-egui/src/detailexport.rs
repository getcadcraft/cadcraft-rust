//! DetailCraft's exports in the app: detailcraft.js `saveModel`, `exportIfc`, `exportNc`, `exportBom`,
//! `exportBending`, `exportKiss`, `exportNest`, `exportReport`, `exportReportPdf`, the drawings dialog's "all sheets" /
//! "this sheet" (PDF, DWG 2000–2018, DXF R12) and the shop drawing's PDF, composed from the engine's writers exactly
//! as 0.5 composes them (cc-detail, cc-io-detail), named by `safeName()` and the catalog's words, with 0.5's status
//! lines. The clock comes from the caller (`now_ms`): the drawings' and reports' dates, the PDF CreationDate, the
//! zip entries' and the KISS file's date, IFC's time stamp, the DWG's created date.

use cc_detail::build::Built;
use cc_detail::i18n::{F, t, t0};
use cc_detail::marks::Marks;
use cc_detail::rebar::Schedule;
use serde_json::{Value, json};

/// Settings › DetailCraft that the exports read (detailcraft.js `SET_DEFAULTS`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub paper: String,
    /// "auto" | "imperial" | "metric"
    pub kiss: String,
    /// "imperial" | "metric"
    pub bar_names: String,
    /// "dc" | "aci" | "bs8666"
    pub shape_codes: String,
    pub nest_stock: Vec<f64>,
    pub nest_kerf: f64,
    pub nest_trim: f64,
    pub nest_keep: f64,
    /// bolts and anchor rods shown in 3D
    pub bolts: bool,
    /// "us" | "eu": the steel new members start as
    pub steel: String,
    /// how far Duplicate puts the copy (mm)
    pub offset: [f64; 3],
    /// copy connections along with their members
    pub conns: bool,
    /// "rect" | "lasso": what a drag to select draws
    pub drag_sel: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            paper: "A3".into(),
            kiss: "auto".into(),
            bar_names: "imperial".into(),
            shape_codes: "dc".into(),
            nest_stock: vec![6000.0, 12000.0],
            nest_kerf: 3.0,
            nest_trim: 0.0,
            nest_keep: 500.0,
            bolts: true,
            steel: "us".into(),
            offset: [6000.0, 0.0, 0.0],
            conns: true,
            drag_sel: "rect".into(),
        }
    }
}

impl Settings {
    /// One setting from "key=value" (detailcraft.js `readSettings` ranges); a bad value is refused.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let (k, v) = spec.split_once('=').ok_or("detail.setting: name = \"key=value\"")?;
        let (k, v) = (k.trim(), v.trim());
        let nb = |lo: f64, hi: f64| -> Result<f64, String> {
            v.parse::<f64>()
                .ok()
                .filter(|x| x.is_finite() && *x >= lo && *x <= hi)
                .ok_or_else(|| format!("detail.setting: {k} = {lo}…{hi}"))
        };
        match k {
            "paper" if cc_detail::paper::PAPER_KEYS.contains(&v) => self.paper = v.into(),
            "kiss" if ["auto", "imperial", "metric"].contains(&v) => self.kiss = v.into(),
            "barNames" if ["imperial", "metric"].contains(&v) => self.bar_names = v.into(),
            "shapeCodes" if ["dc", "aci", "bs8666"].contains(&v) => self.shape_codes = v.into(),
            "bolts" if ["true", "false"].contains(&v) => self.bolts = v == "true",
            "conns" if ["true", "false"].contains(&v) => self.conns = v == "true",
            "steel" if ["us", "eu"].contains(&v) => self.steel = v.into(),
            "dragSel" if ["rect", "lasso"].contains(&v) => self.drag_sel = v.into(),
            "offset" => {
                let o: Vec<f64> = v.split(',').map(|x| cc_detail::js::str_to_number(x.trim())).collect();
                match o.as_slice() {
                    [x, y, z] if o.iter().all(|n| n.is_finite() && n.abs() <= 1e6) && v.split(',').all(|x| !x.trim().is_empty()) => {
                        self.offset = [*x, *y, *z]
                    }
                    _ => return Err(t0("app.set.offset.bad")),
                }
            }
            "stock" => {
                let st: Vec<f64> = v
                    .split([' ', ',', ';'])
                    .filter_map(|x| x.parse::<f64>().ok())
                    .filter(|x| (100.0..=30000.0).contains(x))
                    .take(8)
                    .collect();
                if st.is_empty() {
                    return Err(t0("nest.err.stock"));
                }
                self.nest_stock = st;
            }
            "kerf" => self.nest_kerf = nb(0.0, 20.0)?,
            "trim" => self.nest_trim = nb(0.0, 500.0)?,
            "keep" => self.nest_keep = nb(0.0, 30000.0)?,
            _ => return Err(format!("detail.setting: unknown setting or value {spec}")),
        }
        Ok(())
    }
}

/// What the exports read: the model state and what `recompute()` made from it.
pub struct Ctx<'a> {
    pub state: &'a Value,
    pub built: &'a Built,
    pub marks: &'a Marks,
    /// `marks.footingMark`
    pub footing_marks: &'a [(String, String)],
    pub schedule: &'a Schedule,
    pub settings: &'a Settings,
    /// the selected part's id (shop drawings)
    pub part: Option<String>,
}

/// An export's outcome: a file to save with its status line, or only a status line.
pub enum Outcome {
    File(String, Vec<u8>, String),
    Nothing(String),
}

/// UTC fields of a time in ms (`new Date(ms)` read with getUTC… — the desktop and the harness run in UTC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Clock {
    pub fn of_ms(ms: f64) -> Clock {
        let ms = if ms.is_finite() { ms } else { 0.0 };
        let secs = (ms / 1000.0).floor() as i64;
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // civil_from_days (H. Hinnant)
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        let u = |v: i64| u32::try_from(v).unwrap_or(0);
        Clock { year: y, month: u(m), day: u(d), hour: u(rem / 3600), minute: u(rem % 3600 / 60), second: u(rem % 60) }
    }
    /// `new Date().toISOString().slice(0, 10)`
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    fn pdf(&self) -> cc_detail::pdf::Utc {
        cc_detail::pdf::Utc { year: self.year, month: self.month, day: self.day, hour: self.hour, minute: self.minute, second: self.second }
    }
    fn dos(&self) -> cc_detail::zip::DosDate {
        cc_detail::zip::DosDate {
            year: u32::try_from(self.year).unwrap_or(1980),
            month: self.month,
            day: self.day,
            hours: self.hour,
            minutes: self.minute,
            seconds: self.second,
        }
    }
    fn kiss(&self) -> cc_detail::kiss::When {
        cc_detail::kiss::When {
            year: self.year,
            month: self.month,
            day: self.day,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
        }
    }
}

/// detailcraft.js `safeName()`.
pub fn safe_name(state: &Value) -> String {
    let name = state.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("model");
    let mut out = String::new();
    let mut in_bad = false;
    for c in name.chars() {
        let bad = matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20;
        if bad {
            if !in_bad {
                out.push('-');
            }
            in_bad = true;
        } else {
            out.push(c);
            in_bad = false;
        }
    }
    // .trim().slice(0, 80): UTF-16 units, at a char boundary
    let trimmed = js_trim(&out);
    let mut units = 0usize;
    let mut cut = String::new();
    for c in trimmed.chars() {
        units += c.len_utf16();
        if units > 80 {
            break;
        }
        cut.push(c);
    }
    if cut.is_empty() { "model".into() } else { cut }
}

/// `String.prototype.trim` (JS white space and line terminators).
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn n(v: usize) -> F {
    F::N(v as f64)
}

/// The DWG / DXF / PDF choices of the drawings dialog (its `<select>`): value → shown name.
pub fn format_name(f: &str) -> Option<String> {
    Some(match f {
        "pdf" => "PDF".into(),
        "dxf" => t0("dwg.format.dxf"),
        _ => {
            let year = match f.strip_prefix("dwg-")? {
                "AC1032" => "2018",
                "AC1027" => "2013",
                "AC1024" => "2010",
                "AC1021" => "2007",
                "AC1018" => "2004",
                "AC1015" => "2000",
                _ => return None,
            };
            t("dwg.format.dwg", &[("year", F::S(year.into()))])
        }
    })
}

/// The export names `detail.export` takes.
pub const KINDS: &[&str] =
    &["project", "ifc", "nc", "bom", "bending", "kiss", "cutlist", "report-csv", "report-pdf", "drawings", "sheet", "shop"];

/// Run one export: `spec` = kind[:options]; `fixed` = the clock was given (DWG GUIDs then cc-io-dwg's fixed sequence).
pub fn run(c: &Ctx, spec: &str, now_ms: f64, fixed: bool) -> Result<Outcome, String> {
    let (kind, opt) = spec.split_once(':').map_or((spec.trim(), ""), |(a, b)| (a.trim(), b.trim()));
    let clock = Clock::of_ms(now_ms);
    let st = c.state;
    let safe = safe_name(st);
    Ok(match kind {
        "project" => {
            let name = format!("{safe}.detailcraft.json");
            Outcome::File(name.clone(), project_json(st).into_bytes(), t("app.file.saved", &[("file", F::S(name))]))
        }
        "ifc" => {
            let footings = cc_detail::js::arr(st.get("footings")).len();
            if c.built.parts.is_empty() && footings == 0 {
                return Ok(Outcome::Nothing(t0("app.export.nothing")));
            }
            let txt = cc_io_detail::ifc_out::write_ifc(st, c.built, c.marks, Some(c.footing_marks), Some(c.schedule), None, now_ms)?;
            let bars: f64 = c.built.footings.iter().map(|f| f.bars.iter().map(|g| g.count).sum::<f64>()).sum();
            let msg = t(
                "app.export.ifc",
                &[("parts", n(c.built.parts.len())), ("bolts", n(c.built.bolts.len())), ("footings", n(footings)), ("bars", F::N(bars))],
            );
            Outcome::File(format!("{safe}.ifc"), txt.into_bytes(), msg)
        }
        "nc" => {
            let order = cc_detail::js::string_of(st.get("name"));
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            let mut skipped: Vec<String> = Vec::new();
            for row in &c.marks.parts {
                let Some(part) = row.ids.first().and_then(|id| c.built.get(id)) else { continue };
                if cc_io_detail::nc1::nc_refusal(part).is_some() {
                    skipped.push(row.mark.clone());
                    continue;
                }
                let info = cc_io_detail::nc1::NcInfo {
                    order: order.clone(),
                    mark: row.mark.clone(),
                    count: if row.count > 0 { row.count.to_string() } else { "1".into() },
                    phase: cc_detail::js::num_to_string(cc_detail::js::or(part.phase, 1.0)),
                };
                files.push((format!("{}.nc1", row.mark), cc_io_detail::nc1::write_nc1(part, &info).into_bytes()));
            }
            if files.is_empty() {
                return Ok(Outcome::Nothing(t0(if c.built.parts.is_empty() { "app.export.nothing" } else { "app.export.nc.none2" })));
            }
            let mut msg = t("app.export.nc.done", &[("n", n(files.len()))]);
            if !skipped.is_empty() {
                msg.push(' ');
                msg.push_str(&t("app.export.nc.skipped2", &[("marks", F::S(skipped.join(", ")))]));
            }
            msg.push(' ');
            msg.push_str(&t0("app.export.nc.check"));
            Outcome::File(format!("{safe} NC.zip"), cc_detail::zip::zip(&files, clock.dos()), msg)
        }
        "bom" => {
            let b = cc_detail::marks::bom_tables(c.marks, &c.built.bolts);
            let csv = cc_detail::csv::sections_csv(&[
                (t0("app.k.parts"), b.parts),
                (t0("app.k.assemblies"), b.assemblies),
                (t0("app.rep.bolts"), b.bolts),
            ]);
            Outcome::File(format!("{safe} BOM.csv"), bom(&csv), t("app.export.bom", &[("n", n(c.marks.parts.len()))]))
        }
        "bending" => {
            if c.schedule.rows.is_empty() {
                return Ok(Outcome::Nothing(t0("app.export.bending.none")));
            }
            let rows = cc_detail::rebar::schedule_table(&c.schedule.rows, &c.settings.shape_codes, &c.settings.bar_names);
            let csv = cc_detail::csv::to_csv(&rows);
            Outcome::File(
                format!("{safe} {}.csv", t0("app.export.bending.file")),
                bom(&csv),
                t("app.export.bending", &[("n", n(c.schedule.rows.len()))]),
            )
        }
        "kiss" => {
            if c.built.parts.is_empty() {
                return Ok(Outcome::Nothing(t0("kiss.empty")));
            }
            let metric = match c.settings.kiss.as_str() {
                "metric" => true,
                "imperial" => false,
                _ => cc_detail::kiss::kiss_metric(c.built),
            };
            let revs = cc_detail::issues::revs_by_number(cc_detail::js::arr(st.get("issues")));
            let k = cc_detail::kiss::kiss_file(st, c.built, c.marks, Some(metric), clock.kiss(), "", Some(&revs));
            let count = |key: &str| k.counts.iter().find(|(x, _)| x == key).map_or(0, |(_, v)| *v);
            let msg = t(
                "kiss.saved",
                &[
                    ("job", F::S(k.job.clone())),
                    ("units", F::S(t0(if metric { "kiss.metric" } else { "kiss.imperial" }))),
                    ("d", n(count("D"))),
                    ("m", n(count("M"))),
                ],
            );
            Outcome::File(format!("{safe}.kss"), k.text.into_bytes(), msg)
        }
        "cutlist" => {
            let s = c.settings;
            let r = cc_detail::nest::nest_bars(
                &cc_detail::nest::pieces_from_marks(c.marks),
                &cc_detail::nest::Stock::All(s.nest_stock.clone()),
                s.nest_kerf,
                s.nest_trim,
                s.nest_keep,
            );
            let (summary, bars) = cc_detail::nest::cut_list_tables(&r);
            let csv = cc_detail::csv::sections_csv(&[(t0("nest.summary"), summary), (t0("nest.bars"), bars)]);
            Outcome::File(format!("{safe} {}.csv", t0("nest.file")), bom(&csv), t0("nest.saved"))
        }
        "report-csv" => {
            let rep = cc_detail::report::phase_report(st, c.built, c.marks, c.schedule);
            let csv = cc_detail::report::report_csv(&rep);
            Outcome::File(format!("{safe} {}.csv", t0("rep.file")), bom(&csv), t("rep.saved.csv", &[("n", n(rep.phases.len()))]))
        }
        "report-pdf" => {
            let rep = cc_detail::report::phase_report(st, c.built, c.marks, c.schedule);
            let model = cc_detail::js::string_of(st.get("name"));
            let pages = cc_detail::report::report_sheets(&rep, &c.settings.paper, &model, &clock.date());
            let (bytes, missing) = cc_detail::pdf::to_pdf(&pages, &t0("rep.title"), true, clock.pdf());
            let mut msg = t("rep.saved.pdf", &[("n", n(pages.len())), ("paper", F::S(cc_detail::paper::paper_label(&c.settings.paper)))]);
            missing_note(&mut msg, missing);
            Outcome::File(format!("{safe} {}.pdf", t0("rep.file")), bytes, msg)
        }
        "drawings" | "sheet" => {
            // drawings[:fmt] = every sheet; sheet:<index>[:fmt] = the one shown
            if c.built.parts.is_empty() {
                return Ok(Outcome::Nothing(t0("app.dwg.nothing")));
            }
            let (one, fmt) = if kind == "sheet" {
                let (i, f) = opt.split_once(':').unwrap_or((opt, "pdf"));
                (Some(i.trim().parse::<usize>().map_err(|_| "detail.export: sheet:<index>[:format]")?), f.trim())
            } else {
                (None, if opt.is_empty() { "pdf" } else { opt })
            };
            let fmt_name = format_name(fmt).ok_or_else(|| format!("detail.export: format pdf, dxf or dwg-AC1015…AC1032, not {fmt}"))?;
            let set = cc_detail::drawings::drawing_set(st, c.built, c.marks, Some(c.footing_marks), &clock.date(), &c.settings.paper, true);
            if set.sheets.is_empty() {
                return Ok(Outcome::Nothing(t0("app.dwg.empty")));
            }
            let sheets: Vec<&cc_detail::drawings::Sheet> = match one {
                Some(i) => vec![set.sheets.get(i).ok_or_else(|| format!("detail.export: there are {} sheets", set.sheets.len()))?],
                None => set.sheets.iter().collect(),
            };
            let list = |s: &cc_detail::drawings::Sheet| cc_detail::display::List { w: s.w, h: s.h, items: s.items.clone() };
            if fmt == "pdf" {
                match one {
                    None => {
                        let lists: Vec<_> = sheets.iter().map(|s| list(s)).collect();
                        let title = t("app.dwg.pdf-title", &[("name", F::S(cc_detail::js::string_of(st.get("name"))))]);
                        let (bytes, missing) = cc_detail::pdf::to_pdf(&lists, &title, true, clock.pdf());
                        let mut msg = t("app.dwg.saved.pdf", &[("n", n(lists.len())), ("ref", F::S(set.r#ref.clone()))]);
                        missing_note(&mut msg, missing);
                        Outcome::File(format!("{safe} {}.pdf", t0("app.dwg.file")), bytes, msg)
                    }
                    Some(_) => {
                        let Some(sh) = sheets.first() else { return Ok(Outcome::Nothing(t0("app.dwg.empty"))) };
                        let (bytes, _) = cc_detail::pdf::to_pdf(&[list(sh)], &format!("{} {}", sh.number, sh.title), true, clock.pdf());
                        Outcome::File(
                            format!("{safe} {}.pdf", sh.number),
                            bytes,
                            t("app.dwg.saved.sheet", &[("number", F::S(sh.number.clone()))]),
                        )
                    }
                }
            } else {
                let (files, notes) = cad_files(&safe, &sheets, fmt, now_ms, fixed)?;
                let ext = if fmt == "dxf" { "DXF" } else { "DWG" };
                let paper = cc_detail::paper::paper_label(set.paper);
                let mut msg = if one.is_none() {
                    t("dwg.saved.all", &[("n", n(files.len())), ("format", F::S(fmt_name)), ("paper", F::S(paper))])
                } else {
                    t("dwg.saved.one", &[("n", n(files.len())), ("format", F::S(fmt_name)), ("paper", F::S(paper))])
                };
                if !notes.is_empty() {
                    msg.push(' ');
                    msg.push_str(&notes.join(" "));
                }
                if files.len() == 1 {
                    let Some((name, data)) = files.into_iter().next() else { return Ok(Outcome::Nothing(msg)) };
                    Outcome::File(name, data, msg)
                } else {
                    Outcome::File(format!("{safe} {} {ext}.zip", t0("app.dwg.file")), cc_detail::zip::zip(&files, clock.dos()), msg)
                }
            }
        }
        "shop" => {
            let id = if opt.is_empty() { c.part.clone() } else { Some(opt.to_string()) };
            let part = id.as_deref().and_then(|i| c.built.get(i).or_else(|| c.built.get(&format!("{i}.plate"))));
            if let Some(why) = cc_detail::shop::shop_refusal(part) {
                return Ok(Outcome::Nothing(why));
            }
            let Some(part) = part else { return Ok(Outcome::Nothing(String::new())) };
            let row = c.marks.parts.iter().find(|p| p.ids.contains(&part.id));
            let mark = row.map_or_else(|| part.id.clone(), |r| r.mark.clone());
            let weld = c.built.welds.iter().find(|w| w.between.contains(&part.id));
            let info = cc_detail::shop::Info {
                paper: c.settings.paper.clone(),
                mark: mark.clone(),
                asm_mark: c.marks.asm_of(&part.id).and_then(|a| c.marks.asm_mark(a)).unwrap_or("").to_string(),
                count: Some(row.map_or(1.0, |r| if r.count > 0 { r.count as f64 } else { 1.0 })),
                weld: weld
                    .map_or_else(String::new, |w| t("app.shop.weld", &[("size", F::N(w.size)), ("where", F::S(w.r#where.to_lowercase()))])),
                model: cc_detail::js::string_of(st.get("name")),
                date: clock.date(),
            };
            let sd = cc_detail::shop::shop_drawing(part, &info)?;
            let list = cc_detail::display::List { w: sd.w, h: sd.h, items: sd.items };
            let (bytes, missing) =
                cc_detail::pdf::to_pdf(&[list], &t("app.shop.title", &[("mark", F::S(mark.clone()))]), true, clock.pdf());
            let mut msg = t("app.shop.saved", &[("mark", F::S(mark.clone()))]);
            missing_note(&mut msg, missing);
            Outcome::File(format!("{safe} {mark}.pdf"), bytes, msg)
        }
        other => return Err(format!("detail.export: one of {}, not {other}", KINDS.join(", "))),
    })
}

/// `model.toJSON()` of a state (`JSON.stringify(state, null, 1)`).
pub fn project_json(state: &Value) -> String {
    cc_detail::model::Model::new(state.clone()).to_json()
}

/// '\ufeff' + text
fn bom(text: &str) -> Vec<u8> {
    let mut v = "\u{feff}".as_bytes().to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn missing_note(msg: &mut String, missing: usize) {
    if missing > 0 {
        msg.push(' ');
        msg.push_str(&t("app.pdf.missing", &[("n", n(missing))]));
    }
}

/// Named files and the notes about them.
type CadFiles = (Vec<(String, Vec<u8>)>, Vec<String>);

/// detailcraft.js `cadFiles(sheets)`: DXF R12 (dwgout.js, Latin-1) or DWG (cc-dwg's writer) per sheet, and the notes.
fn cad_files(safe: &str, sheets: &[&cc_detail::drawings::Sheet], fmt: &str, now_ms: f64, fixed: bool) -> Result<CadFiles, String> {
    let mut files = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let sheet_value = |s: &cc_detail::drawings::Sheet| json!({ "w": cc_detail::js::jnum(s.w), "h": cc_detail::js::jnum(s.h), "items": s.items.iter().map(cc_detail::display::Item::j).collect::<Vec<_>>() });
    if fmt == "dxf" {
        let mut fills = 0.0;
        for s in sheets {
            let r = cc_io_detail::dwgout::sheet_dxf(&sheet_value(s), &s.title)?;
            fills += cc_detail::js::arr(r.get("notes")).iter().map(|x| x.get("n").and_then(Value::as_f64).unwrap_or(0.0)).sum::<f64>();
            let text = r.get("text").and_then(Value::as_str).unwrap_or("");
            // latin1: each UTF-16 unit & 255
            let data: Vec<u8> = text.encode_utf16().map(|u| (u & 255) as u8).collect();
            files.push((format!("{safe} {}.dxf", s.number), data));
        }
        if fills > 0.0 {
            notes.push(t("dxf.note.fills", &[("n", F::N(fills))]));
        }
        return Ok((files, notes));
    }
    let version = fmt.strip_prefix("dwg-").unwrap_or("AC1015").to_string();
    for s in sheets {
        let model = cc_io_detail::dwgout::sheet_model(&sheet_value(s), &s.title)?;
        let cc_io_dwg::V::Obj(obj) = cc_io_dwg::w_js::from_json(&model, 0) else { return Err("the sheet model is not an object".into()) };
        let opts = cc_io_dwg::WriteOpts {
            version: version.clone(),
            created_ms: Some(now_ms),
            guid: if fixed { cc_io_dwg::GuidMode::Sequence } else { cc_io_dwg::GuidMode::Random },
        };
        let (bytes, report) = cc_io_dwg::write_dwg_opts(&obj, &opts)?;
        let rep: Value = serde_json::from_str(&cc_io_dwg::V::Obj(report).to_json()).unwrap_or(Value::Null);
        notes.extend(cc_detail::js::arr(rep.get("notes")).iter().filter_map(|x| x.as_str().map(str::to_string)));
        files.push((format!("{safe} {}.dwg", s.number), bytes));
    }
    Ok((files, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_utc() {
        let c = Clock::of_ms(1_760_700_000_000.0);
        assert_eq!(c, Clock { year: 2025, month: 10, day: 17, hour: 11, minute: 20, second: 0 });
        assert_eq!(c.date(), "2025-10-17");
        assert_eq!(Clock::of_ms(0.0).date(), "1970-01-01");
        assert_eq!(Clock::of_ms(951_782_400_000.0).date(), "2000-02-29");
    }

    #[test]
    fn safe_names_are_the_js_ones() {
        assert_eq!(safe_name(&json!({ "name": "Sample frame" })), "Sample frame");
        assert_eq!(safe_name(&json!({ "name": "a/b:\\c" })), "a-b-c");
        assert_eq!(safe_name(&json!({ "name": "a//b" })), "a-b");
        assert_eq!(safe_name(&json!({ "name": "  " })), "model");
        assert_eq!(safe_name(&json!({})), "model");
        assert_eq!(safe_name(&json!({ "name": "x".repeat(100) })).len(), 80);
    }

    #[test]
    fn settings_validate() {
        let mut s = Settings::default();
        s.set("paper=A1").unwrap();
        assert!(s.set("paper=A9").is_err());
        assert!(s.set("kerf=50").is_err());
        s.set("stock=6000 9000,50").unwrap();
        assert_eq!(s.nest_stock, vec![6000.0, 9000.0]);
        assert!(s.set("stock=1").is_err());
        assert!(s.set("nonsense").is_err());
    }

    #[test]
    fn format_names_are_the_dialog_s() {
        assert_eq!(format_name("pdf").as_deref(), Some("PDF"));
        assert_eq!(format_name("dwg-AC1032").as_deref(), Some("DWG (2018 format)"));
        assert_eq!(format_name("dxf").as_deref(), Some("DXF (R12)"));
        assert_eq!(format_name("dwg-AC9999"), None);
    }
}
