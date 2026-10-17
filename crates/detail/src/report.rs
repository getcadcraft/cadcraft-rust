//! report.js — the material and weight report, phase by phase (0.4 D14): rows by phase × part mark (gross weight,
//! paint = outside perimeter × cut length), bolts and rebar per phase; the tables, the CSV, and PDF pages carried over.

use crate::build::{Built, Part};
use crate::display::{Item, List};
use crate::i18n::{F, t, t0};
use crate::js::{Obj, locale_compare_num, max_of, num_to_string, round, to_fixed};
use crate::marks::{BoltRow, Marks, bolt_list};
use crate::paper::paper_of;
use crate::pdf::text_width;
use crate::rebar::Schedule;
use serde_json::Value;

fn kg1(v: f64) -> String {
    to_fixed(round(v * 10.0) / 10.0, 1)
}
fn m2(v: f64) -> String {
    to_fixed(round(v * 100.0) / 100.0, 2)
}
/// `paintArea(part)` (m²).
pub fn paint_area(p: &Part) -> f64 {
    p.profile.perimeter * p.length / 1e6
}

/// A report row.
#[derive(Clone, Debug)]
pub struct Row {
    pub mark: String,
    pub kind: String,
    pub profile: String,
    pub material: String,
    pub length: f64,
    pub count: usize,
    pub unit_weight: f64,
    pub weight: f64,
    pub paint: f64,
}
/// Totals.
#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub parts: f64,
    pub weight: f64,
    pub paint: f64,
    pub bolts: f64,
    pub rebar: f64,
}
/// One phase.
#[derive(Clone, Debug)]
pub struct Phase {
    pub n: Value,
    pub name: Value,
    pub rows: Vec<Row>,
    pub bolts: Vec<BoltRow>,
    pub rebar: f64,
    pub totals: Totals,
}
/// `phaseReport(...)`.
#[derive(Clone, Debug)]
pub struct Report {
    pub phases: Vec<Phase>,
    pub totals: Totals,
}

pub fn phase_report(state: &Value, built: &Built, marks: &Marks, schedule: &Schedule) -> Report {
    let src = crate::js::arr(state.get("phases"));
    let list: Vec<(Value, Value)> = if src.is_empty() {
        vec![(Value::from(1), Value::from(""))]
    } else {
        src.iter().map(|p| (p.get("n").cloned().unwrap_or(Value::Null), p.get("name").cloned().unwrap_or(Value::Null))).collect()
    };
    let mut phases: Vec<Phase> =
        list.into_iter().map(|(n, name)| Phase { n, name, rows: vec![], bolts: vec![], rebar: 0.0, totals: Totals::default() }).collect();
    // byN: a later phase of the same number takes the key
    let of = |phases: &Vec<Phase>, n: f64| -> usize {
        let key = crate::js::or(n, 1.0);
        phases.iter().rposition(|p| p.n.as_f64() == Some(key)).unwrap_or(0)
    };
    let mut keys: Vec<(String, usize, usize)> = Vec::new();
    for part in &built.parts {
        let pi = of(&phases, part.phase);
        let mark = marks.part_mark(&part.id).filter(|m| !m.is_empty()).unwrap_or(&part.id).to_string();
        let key = format!("{}|{mark}", phases.get(pi).map_or_else(String::new, |p| crate::js::to_js_string(&p.n)));
        let (pi, ri) = match keys.iter().find(|k| k.0 == key) {
            Some(k) => (k.1, k.2),
            None => {
                let Some(ph) = phases.get_mut(pi) else { continue };
                ph.rows.push(Row {
                    mark,
                    kind: part.kind.clone(),
                    profile: part.profile_name.clone(),
                    material: part.material.clone(),
                    length: part.length,
                    count: 0,
                    unit_weight: part.weight,
                    weight: 0.0,
                    paint: 0.0,
                });
                let ri = ph.rows.len().saturating_sub(1);
                keys.push((key, pi, ri));
                (pi, ri)
            }
        };
        if let Some(r) = phases.get_mut(pi).and_then(|ph| ph.rows.get_mut(ri)) {
            r.count = r.count.saturating_add(1);
            r.weight += part.weight;
            r.paint += paint_area(part);
        }
    }
    let order = |k: &str| match k {
        "column" => 0.0,
        "beam" => 1.0,
        "brace" => 2.0,
        "plate" => 3.0,
        _ => f64::NAN,
    };
    let footings = crate::js::arr(state.get("footings"));
    let nph = phases.len();
    for i in 0..nph {
        let bolts: Vec<crate::connections::Bolt> = built.bolts.iter().filter(|b| of(&phases, b.phase) == i).cloned().collect();
        let mut rebar = 0.0;
        for f in footings {
            if of(&phases, crate::js::f(f.get("phase"))) != i {
                continue;
            }
            let id = crate::js::string_of(f.get("id"));
            let groups = schedule.by_footing.iter().find(|x| x.0 == id).map_or(&[][..], |x| x.1.as_slice());
            let mut s2 = 0.0;
            for (g, _) in groups {
                let mass = crate::rebar::bar(&g.bar).map_or(f64::NAN, |b| b.mass);
                s2 += g.count * g.length * mass / 1000.0;
            }
            rebar += s2;
        }
        let Some(ph) = phases.get_mut(i) else { continue };
        ph.rows.sort_by(|a, b| {
            let c = order(&a.kind) - order(&b.kind);
            crate::js::ord(if c == 0.0 || c.is_nan() { locale_compare_num(&a.mark, &b.mark) } else { c })
        });
        ph.bolts = bolt_list(&bolts);
        ph.rebar = rebar;
        ph.totals = Totals {
            parts: ph.rows.iter().fold(0.0, |s, r| s + r.count as f64),
            weight: ph.rows.iter().fold(0.0, |s, r| s + r.weight),
            paint: ph.rows.iter().fold(0.0, |s, r| s + r.paint),
            bolts: ph.bolts.iter().fold(0.0, |s, b| s + b.count as f64),
            rebar,
        };
    }
    let sum = |f: &dyn Fn(&Totals) -> f64| phases.iter().fold(0.0, |s, p| s + f(&p.totals));
    let totals = Totals {
        parts: sum(&|t| t.parts),
        weight: sum(&|t| t.weight),
        paint: sum(&|t| t.paint),
        bolts: sum(&|t| t.bolts),
        rebar: sum(&|t| t.rebar),
    };
    Report { phases, totals }
}

/// `phaseTitle(ph)`.
pub fn phase_title(ph: &Phase) -> String {
    let n = F::S(crate::js::to_js_string(&ph.n));
    if crate::js::truthy(Some(&ph.name)) {
        t("rep.phase.named", &[("n", n), ("name", F::S(crate::js::to_js_string(&ph.name)))])
    } else {
        t("phase.default", &[("n", n)])
    }
}

/// The tables of one phase.
pub struct PhaseTables {
    pub title: String,
    pub parts: Vec<Vec<String>>,
    pub bolts: Vec<Vec<String>>,
    pub rebar: f64,
}
/// `reportTables(rep)` → (summary, phases).
pub fn report_tables(rep: &Report) -> (Vec<Vec<String>>, Vec<PhaseTables>) {
    let tot =
        |p: &Totals, first: String| vec![first, num_to_string(p.parts), kg1(p.weight), m2(p.paint), num_to_string(p.bolts), kg1(p.rebar)];
    let mut summary =
        vec![["phase", "parts", "steel", "paint", "bolts", "rebar"].iter().map(|k| t0(&format!("rep.col.{k}"))).collect::<Vec<_>>()];
    for p in &rep.phases {
        summary.push(tot(&p.totals, phase_title(p)));
    }
    summary.push(tot(&rep.totals, t0("rep.total")));
    let phases = rep
        .phases
        .iter()
        .map(|p| {
            let mut parts = vec![
                ["mark", "type", "profile", "material", "length", "count", "each", "kg", "paint"]
                    .iter()
                    .map(|k| t0(&format!("rep.col.{k}")))
                    .collect::<Vec<_>>(),
            ];
            for r in &p.rows {
                parts.push(vec![
                    r.mark.clone(),
                    t0(&format!("rep.type.{}", r.kind)),
                    r.profile.clone(),
                    r.material.clone(),
                    num_to_string(round(r.length)),
                    r.count.to_string(),
                    kg1(r.unit_weight),
                    kg1(r.weight),
                    m2(r.paint),
                ]);
            }
            parts.push(vec![
                t0("rep.total"),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                num_to_string(p.totals.parts),
                String::new(),
                kg1(p.totals.weight),
                m2(p.totals.paint),
            ]);
            let mut bolts =
                vec![["fastener", "dia", "blen", "grade", "count"].iter().map(|k| t0(&format!("rep.col.{k}"))).collect::<Vec<_>>()];
            for b in &p.bolts {
                bolts.push(vec![
                    t0(if b.kind == "ANCHORBOLT" { "rep.rod" } else { "rep.bolt" }),
                    num_to_string(b.d),
                    num_to_string(b.length),
                    b.grade.clone(),
                    b.count.to_string(),
                ]);
            }
            PhaseTables { title: phase_title(p), parts, bolts, rebar: p.rebar }
        })
        .collect();
    (summary, phases)
}

/// `reportCsv(rep)`.
pub fn report_csv(rep: &Report) -> String {
    let (summary, phases) = report_tables(rep);
    let with_title = |title: Vec<String>, rows: &Vec<Vec<String>>| {
        let mut all = vec![title];
        all.extend(rows.iter().cloned());
        crate::csv::to_csv(&all)
    };
    let mut out = vec![with_title(vec![t0("rep.csv.summary")], &summary)];
    for p in &phases {
        out.push(with_title(vec![p.title.clone()], &p.parts));
        if p.bolts.len() > 1 {
            out.push(with_title(vec![t("rep.csv.bolts", &[("phase", F::S(p.title.clone()))])], &p.bolts));
        }
        out.push(crate::csv::to_csv(&[vec![t("rep.csv.rebar", &[("phase", F::S(p.title.clone()))]), kg1(p.rebar)]]));
    }
    out.join("\r\n")
}

struct Pager {
    pages: Vec<List>,
    y: f64,
    w: f64,
    h: f64,
    model: String,
    date: String,
}
impl Pager {
    fn push(&mut self, it: Item) {
        if let Some(p) = self.pages.last_mut() {
            p.items.push(it);
        }
    }
    fn new_page(&mut self) {
        let (w, h) = (self.w, self.h);
        let mut items = vec![
            Item::Path {
                pts: vec![[10.0, 10.0], [w - 10.0, 10.0], [w - 10.0, h - 10.0], [10.0, h - 10.0]],
                w: 0.5,
                dash: None,
                closed: true,
                fill: false,
            },
            Item::Text { s: t0("rep.title"), x: 20.0, y: h - 20.0, h: 4.2, align: "left", rot: 0.0, bold: true },
        ];
        let md: Vec<&str> = [self.model.as_str(), self.date.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        items.push(Item::Text { s: md.join("   "), x: w - 20.0, y: h - 20.0, h: 2.6, align: "right", rot: 0.0, bold: false });
        self.pages.push(List { w, h, items });
        self.y = h - 32.0;
    }
    fn text(&mut self, s: &str, x: f64, y: f64, h: f64, align: &'static str, bold: bool) {
        self.push(Item::Text { s: s.to_string(), x, y, h, align, rot: 0.0, bold });
    }
    fn rule(&mut self, x0: f64, x1: f64, y: f64) {
        self.push(Item::Path { pts: vec![[x0, y], [x1, y]], w: 0.18, dash: None, closed: false, fill: false });
    }
    fn table(&mut self, title0: &str, rows: &[Vec<String>], numeric: &[usize]) {
        const LINE: f64 = 4.6;
        const H: f64 = 2.4;
        const BOTTOM: f64 = 22.0;
        let mut title = title0.to_string();
        let n = rows.first().map_or(0, Vec::len);
        let last = rows.len().saturating_sub(1);
        let widths: Vec<f64> = (0..n)
            .map(|c| {
                max_of(rows.iter().enumerate().map(|(i, r)| text_width(r.get(c).map_or("", String::as_str), H, i == 0 || i == last))) + 4.0
            })
            .collect();
        let mut xs = Vec::new();
        let mut x = 20.0;
        for w in &widths {
            xs.push(x);
            x += w;
        }
        let right = x - 4.0;
        let at = |i: usize| -> (f64, &'static str) {
            let (x0, w) = (xs.get(i).copied().unwrap_or(f64::NAN), widths.get(i).copied().unwrap_or(f64::NAN));
            if numeric.contains(&i) { (x0 + w - 4.0, "right") } else { (x0, "left") }
        };
        let head = |pg: &mut Pager, title: &str| {
            if !title.is_empty() {
                let y = pg.y;
                pg.text(title, 20.0, y, 3.0, "left", true);
            }
            pg.y -= LINE + 1.0;
            if let Some(r0) = rows.first() {
                for (i, c) in r0.iter().enumerate() {
                    let (xx, al) = at(i);
                    let y = pg.y;
                    pg.text(c, xx, y, H, al, true);
                }
            }
            let y = pg.y;
            pg.rule(18.0, right + 2.0, y - 1.4);
            pg.y -= LINE;
        };
        if self.y - 3.0 * LINE < BOTTOM {
            self.new_page();
        }
        head(self, &title);
        let total = t0("rep.total");
        for (ri, r) in rows.iter().skip(1).enumerate() {
            if self.y < BOTTOM {
                self.new_page();
                if !title.is_empty() {
                    title = format!("{title} {}", t0("rep.continued"));
                }
                head(self, &title);
            }
            let is_last = ri + 2 == rows.len() && r.first() == Some(&total);
            if is_last {
                let y = self.y;
                self.rule(18.0, right + 2.0, y + LINE - 1.4);
            }
            for (i, c) in r.iter().enumerate() {
                let (xx, al) = at(i);
                let y = self.y;
                self.text(c, xx, y, H, al, is_last);
            }
            self.y -= LINE;
        }
        self.y -= LINE;
    }
}

/// `reportSheets(rep, { paper, model, date })`.
pub fn report_sheets(rep: &Report, paper: &str, model: &str, date: &str) -> Vec<List> {
    const LINE: f64 = 4.6;
    const BOTTOM: f64 = 22.0;
    let p = paper_of(paper);
    let (summary, phases) = report_tables(rep);
    let mut pg = Pager { pages: vec![], y: 0.0, w: p.w, h: p.h, model: model.into(), date: date.into() };
    pg.new_page();
    pg.table(&t0("rep.summary"), &summary, &[1, 2, 3, 4, 5]);
    let y = pg.y;
    pg.text(&t0("rep.note"), 20.0, y, 2.1, "left", false);
    pg.y -= LINE * 2.0;
    for ph in &phases {
        if pg.y - 6.0 * LINE < BOTTOM {
            pg.new_page();
        }
        pg.table(&ph.title, &ph.parts, &[4, 5, 6, 7, 8]);
        if ph.bolts.len() > 1 {
            pg.table(&t0("rep.boltsof"), &ph.bolts, &[1, 2, 4]);
        }
        if pg.y < BOTTOM {
            pg.new_page();
        }
        let y = pg.y;
        pg.text(&t("rep.rebarof", &[("kg", F::S(kg1(ph.rebar)))]), 20.0, y, 2.4, "left", false);
        pg.y -= LINE * 2.0;
    }
    let n = pg.pages.len();
    for (i, page) in pg.pages.iter_mut().enumerate() {
        page.items.push(Item::Text {
            s: t("rep.page", &[("n", F::N((i + 1) as f64)), ("of", F::N(n as f64))]),
            x: p.w - 20.0,
            y: 14.0,
            h: 2.2,
            align: "right",
            rot: 0.0,
            bold: false,
        });
    }
    pg.pages
}

/// The JSON of a report.
pub fn report_json(rep: &Report) -> Value {
    let tj = |t: &Totals| {
        Obj::new().set("parts", t.parts).set("weight", t.weight).set("paint", t.paint).set("bolts", t.bolts).set("rebar", t.rebar).done()
    };
    let phases: Vec<Value> = rep
        .phases
        .iter()
        .map(|p| {
            let rows: Vec<Value> = p
                .rows
                .iter()
                .map(|r| {
                    Obj::new()
                        .set("mark", r.mark.as_str())
                        .set("type", r.kind.as_str())
                        .set("profile", r.profile.as_str())
                        .set("material", r.material.as_str())
                        .set("length", r.length)
                        .set("count", r.count)
                        .set("unitWeight", r.unit_weight)
                        .set("weight", r.weight)
                        .set("paint", r.paint)
                        .done()
                })
                .collect();
            let bolts: Vec<Value> = p
                .bolts
                .iter()
                .map(|b| {
                    Obj::new()
                        .set("kind", b.kind.as_str())
                        .set("d", b.d)
                        .set("length", b.length)
                        .set("grade", b.grade.as_str())
                        .set("count", b.count)
                        .done()
                })
                .collect();
            Obj::new()
                .set("n", p.n.clone())
                .set("name", p.name.clone())
                .set("rows", Value::Array(rows))
                .set("bolts", Value::Array(bolts))
                .set("rebar", p.rebar)
                .set("totals", tj(&p.totals))
                .done()
        })
        .collect();
    Obj::new().set("phases", Value::Array(phases)).set("totals", tj(&rep.totals)).done()
}
