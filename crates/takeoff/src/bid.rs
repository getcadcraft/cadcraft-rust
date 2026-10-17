//! `bid.js` (the data half) — the bid settings (`normalBid`), whether a takeoff uses them, and the summary: material +
//! labor + labor burden = cost, then tax, overhead, profit and bond, each rounded to the cent. The proposal PDF is in
//! cc-io-takeoff.

use crate::i18n::t;
use crate::jv::{self, Obj};
use serde_json::{Value, json};

/// The markups after the cost, in order.
pub const BID_FIELDS: [&str; 4] = ["tax", "overhead", "profit", "bond"];
pub const BID_PCTS: [&str; 5] = ["burden", "tax", "overhead", "profit", "bond"];

fn pct(v: Option<&Value>) -> f64 {
    match jv::int(v) {
        Some(x) if (0.0..=100000.0).contains(&x) => x,
        _ => 0.0,
    }
}
/// `String(v ?? '').replace(/\r\n?/g, '\n').replace(/[\u0000-\u0009\u000b-\u001f\u007f]/g, ' ').slice(0, max)`.
pub fn text(v: Option<&Value>, max: usize) -> String {
    let s = jv::to_string_or_empty(v);
    clean_lines(&s, max)
}
/// The same cleaning for a string.
pub fn clean_lines(s: &str, max: usize) -> String {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let s: String = s.chars().map(|c| if (c as u32) <= 0x1f && c != '\n' || c == '\u{7f}' { ' ' } else { c }).collect();
    cc_m3d::js::slice16(&s, max)
}

/// The takeoff's bid settings, clean.
pub fn normal_bid(raw: Option<&Value>) -> Value {
    let empty = Obj::new();
    let b = jv::obj(raw).unwrap_or(&empty);
    let arrb = matches!(raw, Some(Value::Array(_)));
    // an array is an object in JS too: its named properties are read (there are none)
    let g = |k: &str| if arrb { None } else { b.get(k) };
    json!({
        "burden": jv::n(pct(g("burden"))), "tax": jv::n(pct(g("tax"))), "overhead": jv::n(pct(g("overhead"))),
        "profit": jv::n(pct(g("profit"))), "bond": jv::n(pct(g("bond"))),
        "to": text(g("to"), 400), "project": text(g("project"), 200), "by": text(g("by"), 400),
        "incl": text(g("incl"), 4000), "excl": text(g("excl"), 4000), "terms": text(g("terms"), 4000),
        "items": !matches!(g("items"), Some(Value::Bool(false))),
    })
}

/// `bidUsed(b)`.
pub fn bid_used(b: Option<&Value>) -> bool {
    match b {
        None | Some(Value::Null) => false,
        Some(v) if !jv::truthy(Some(v)) => false,
        Some(v) => {
            let get = |k: &str| match v {
                Value::Object(o) => o.get(k),
                _ => None,
            };
            BID_PCTS.iter().any(|k| jv::to_number(get(k)) > 0.0 && !matches!(get(k), Some(Value::Object(_)) | None))
                || ["to", "project", "by", "incl", "excl", "terms"].iter().any(|k| jv::truthy(get(k)))
        }
    }
}

fn share(cents: f64, bp: f64) -> f64 {
    cc_m3d::js::round(cents * bp / 10000.0)
}

/// The markups on a cost of `mat` + `lab` cents.
#[derive(Debug, Clone, Copy)]
pub struct Markup {
    pub burden: f64,
    pub cost: f64,
    pub tax: f64,
    pub overhead: f64,
    pub profit: f64,
    pub bond: f64,
    pub total: f64,
}
pub fn markup(mat: f64, lab: f64, b: &Value) -> Markup {
    let g = |k: &str| jv::num(b.get(k)).unwrap_or(0.0);
    let burden = share(lab, if jv::truthy(b.get("burden")) { g("burden") } else { 0.0 });
    let cost = mat + lab + burden;
    let tax = share(mat, g("tax"));
    let overhead = share(cost + tax, g("overhead"));
    let profit = share(cost + tax + overhead, g("profit"));
    let bond = share(cost + tax + overhead + profit, g("bond"));
    Markup { burden, cost, tax, overhead, profit, bond, total: cost + tax + overhead + profit + bond }
}

/// One line of the summary.
#[derive(Debug, Clone)]
pub struct Line {
    pub key: &'static str,
    pub label: String,
    pub pct: Option<f64>,
    pub cents: f64,
    pub sub: bool,
}
/// An alternate in the summary.
#[derive(Debug, Clone)]
pub struct AltLine {
    pub name: String,
    pub alt: String,
    pub amount: f64,
    pub with_markup: f64,
}
/// `bidSummary(est, bid, alts)`.
#[derive(Debug, Clone)]
pub struct Summary {
    pub lines: Vec<Line>,
    pub total: f64,
    pub alts: Vec<AltLine>,
    pub incomplete: bool,
}
impl Summary {
    pub fn line(&self, key: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.key == key)
    }
    /// As the JS returns it.
    pub fn to_json(&self) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                let mut o = Obj::new();
                o.insert("key".into(), Value::String(l.key.into()));
                o.insert("label".into(), Value::String(l.label.clone()));
                if let Some(p) = l.pct {
                    o.insert("pct".into(), jv::n(p));
                }
                o.insert("cents".into(), jv::n(l.cents));
                if l.sub {
                    o.insert("sub".into(), Value::Bool(true));
                }
                Value::Object(o)
            })
            .collect();
        let alts: Vec<Value> = self
            .alts
            .iter()
            .map(|a| json!({ "name": a.name, "alt": a.alt, "amount": jv::n(a.amount), "withMarkup": jv::n(a.with_markup) }))
            .collect();
        json!({ "lines": lines, "total": jv::n(self.total), "alts": alts, "incomplete": self.incomplete })
    }
}

/// An alternate's input to the summary: its bid area's name, 'add' | 'deduct', its amount and cost.
#[derive(Debug, Clone)]
pub struct AltIn {
    pub name: String,
    pub alt: String,
    pub amount: f64,
    pub mat_total: f64,
    pub lab_total: f64,
}

/// `bidSummary(est, bidRaw, alts)`.
pub fn bid_summary(mat_total: f64, lab_total: f64, incomplete: bool, bid_raw: Option<&Value>, alts: &[AltIn]) -> Summary {
    let b = normal_bid(bid_raw);
    let m = markup(mat_total, lab_total, &b);
    let g = |k: &str| jv::num(b.get(k)).unwrap_or(0.0);
    let mut lines = vec![
        Line { key: "mat", label: t("bid.l.mat"), pct: None, cents: mat_total, sub: false },
        Line { key: "lab", label: t("bid.l.lab"), pct: None, cents: lab_total, sub: false },
    ];
    if g("burden") > 0.0 {
        lines.push(Line { key: "burden", label: t("bid.l.burden"), pct: Some(g("burden")), cents: m.burden, sub: false });
    }
    lines.push(Line { key: "cost", label: t("bid.l.cost"), pct: None, cents: m.cost, sub: true });
    for k in BID_FIELDS {
        if g(k) > 0.0 {
            let cents = match k {
                "tax" => m.tax,
                "overhead" => m.overhead,
                "profit" => m.profit,
                _ => m.bond,
            };
            lines.push(Line { key: k, label: t(&format!("bid.l.{k}")), pct: Some(g(k)), cents, sub: false });
        }
    }
    lines.push(Line { key: "total", label: t("bid.l.total"), pct: None, cents: m.total, sub: true });
    let alt_out = alts
        .iter()
        .map(|a| {
            let am = markup(a.mat_total, a.lab_total, &b);
            AltLine {
                name: a.name.clone(),
                alt: a.alt.clone(),
                amount: a.amount,
                with_markup: if a.alt == "add" { am.total } else { -am.total },
            }
        })
        .collect();
    Summary { lines, total: m.total, alts: alt_out, incomplete }
}
