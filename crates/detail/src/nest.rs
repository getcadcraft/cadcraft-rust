//! nest.js — cut-list nesting (0.4 D24): the linear parts cut from stock bars, first-fit decreasing per profile and
//! material, each bar then shrunk to the shortest stock length its cuts still fit; kerf, end trim, drops kept or
//! scrapped; `piecesFromMarks`, `cutListTables`.

use crate::i18n::t0;
use crate::js::{Obj, locale_compare_num, max, num_to_string, round, to_fixed, upper};
use crate::marks::Marks;
use serde_json::Value;

/// A piece to cut.
#[derive(Clone, Debug)]
pub struct Piece {
    pub mark: String,
    pub profile: String,
    pub material: String,
    pub length: f64,
    pub count: f64,
}
/// One cut on a bar.
#[derive(Clone, Debug)]
pub struct CutP {
    pub mark: String,
    pub length: f64,
}
/// A stock bar.
#[derive(Clone, Debug)]
pub struct Bar {
    pub stock: f64,
    pub cuts: Vec<CutP>,
    pub used: f64,
    pub drop: f64,
    pub scrap: bool,
}
/// A profile's nesting.
#[derive(Clone, Debug)]
pub struct Group {
    pub profile: String,
    pub material: String,
    pub bars: Vec<Bar>,
    pub pieces: f64,
    pub bought: f64,
    pub used: f64,
    pub waste: f64,
    pub too_long: Vec<String>,
    pub stock: Vec<(f64, usize)>,
}
/// `nestBars(...)`.
#[derive(Clone, Debug)]
pub struct Nest {
    pub groups: Vec<Group>,
    pub bars: usize,
    pub bought: f64,
    pub used: f64,
    pub waste: f64,
}
/// Stock lengths: one list for all, or per profile (upper-case name, `*` = the rest).
pub enum Stock {
    All(Vec<f64>),
    Per(Vec<(String, Vec<f64>)>),
}

fn sum(a: impl IntoIterator<Item = f64>) -> f64 {
    a.into_iter().fold(0.0, |s, v| s + v)
}
/// `barNeed(cuts, kerf, trim)`.
pub fn bar_need(cuts: &[f64], kerf: f64, trim: f64) -> f64 {
    trim + sum(cuts.iter().copied()) + kerf * cuts.len() as f64
}

/// Bars per group are capped (the JS has no cap; 1e6 pieces of one profile is far past any model).
const MAX_ITEMS: f64 = 1_000_000.0;

pub fn nest_bars(pieces: &[Piece], stock: &Stock, kerf: f64, trim: f64, keep: f64) -> Nest {
    struct G {
        profile: String,
        material: String,
        items: Vec<CutP>,
    }
    let mut by: Vec<(String, G)> = Vec::new();
    let mut total = 0.0;
    for p in pieces {
        if !(p.length > 0.0) || !(p.count > 0.0) {
            continue;
        }
        let k = format!("{}|{}", upper(&p.profile), upper(&p.material));
        let i = match by.iter().position(|(kk, _)| *kk == k) {
            Some(i) => i,
            None => {
                by.push((k, G { profile: p.profile.clone(), material: p.material.clone(), items: vec![] }));
                by.len().saturating_sub(1)
            }
        };
        let mut c = 0.0;
        while c < p.count && total < MAX_ITEMS {
            if let Some((_, g)) = by.get_mut(i) {
                g.items.push(CutP { mark: p.mark.clone(), length: round(p.length * 10.0) / 10.0 });
            }
            c += 1.0;
            total += 1.0;
        }
    }
    let mut groups = Vec::new();
    for (_, mut g) in by {
        let mut lengths: Vec<f64> = match stock {
            Stock::All(v) => v.clone(),
            Stock::Per(m) => {
                let u = upper(&g.profile);
                m.iter()
                    .find(|(k, _)| *k == u)
                    .or_else(|| m.iter().find(|(k, _)| k == "*"))
                    .map_or_else(|| vec![12000.0], |(_, v)| v.clone())
            }
        };
        lengths.retain(|v| *v > 0.0);
        lengths.sort_by(|a, b| crate::js::ord(a - b));
        let mx = lengths.last().copied().unwrap_or(f64::NAN);
        g.items.sort_by(|a, b| {
            let c = b.length - a.length;
            crate::js::ord(if c == 0.0 || c.is_nan() { locale_compare_num(&a.mark, &b.mark) } else { c })
        });
        let mut bars: Vec<Bar> = Vec::new();
        let mut too_long = Vec::new();
        for it in &g.items {
            if bar_need(&[it.length], kerf, trim) - kerf > mx {
                too_long.push(it.mark.clone());
                continue;
            }
            let fit = bars.iter().position(|b| {
                let mut cs: Vec<f64> = b.cuts.iter().map(|c| c.length).collect();
                cs.push(it.length);
                bar_need(&cs, kerf, trim) <= b.stock + kerf
            });
            let i = match fit {
                Some(i) => i,
                None => {
                    bars.push(Bar { stock: mx, cuts: vec![], used: 0.0, drop: 0.0, scrap: false });
                    bars.len().saturating_sub(1)
                }
            };
            if let Some(b) = bars.get_mut(i) {
                b.cuts.push(it.clone());
            }
        }
        for b in &mut bars {
            let cs: Vec<f64> = b.cuts.iter().map(|c| c.length).collect();
            let need = bar_need(&cs, kerf, trim);
            b.stock = lengths.iter().copied().find(|l| need <= l + kerf).filter(|l| *l != 0.0).unwrap_or(b.stock);
            b.used = sum(cs.iter().copied());
            let kerfs = kerf * b.cuts.len() as f64;
            b.drop = max(0.0, round((b.stock - trim - b.used - kerfs) * 10.0) / 10.0);
            if b.drop < 0.05 {
                b.drop = 0.0;
            }
            b.scrap = b.drop < keep;
        }
        let bought = sum(bars.iter().map(|b| b.stock));
        let used = sum(bars.iter().map(|b| b.used));
        let stock_count: Vec<(f64, usize)> = lengths.iter().map(|&l| (l, bars.iter().filter(|b| b.stock == l).count())).collect();
        let pieces_n = g.items.len() as f64 - too_long.len() as f64;
        groups.push(Group {
            profile: g.profile,
            material: g.material,
            bars,
            pieces: pieces_n,
            bought,
            used,
            waste: if bought != 0.0 && !bought.is_nan() { 1.0 - used / bought } else { 0.0 },
            too_long,
            stock: stock_count,
        });
    }
    groups.sort_by(|a, b| crate::js::ord(locale_compare_num(&a.profile, &b.profile)));
    let bought = sum(groups.iter().map(|g| g.bought));
    let used = sum(groups.iter().map(|g| g.used));
    let bars = groups.iter().fold(0usize, |s, g| s.saturating_add(g.bars.len()));
    Nest { groups, bars, bought, used, waste: if bought != 0.0 && !bought.is_nan() { 1.0 - used / bought } else { 0.0 } }
}

/// `piecesFromMarks(marks)`.
pub fn pieces_from_marks(marks: &Marks) -> Vec<Piece> {
    marks
        .parts
        .iter()
        .filter(|p| p.kind != "plate")
        .map(|p| Piece {
            mark: p.mark.clone(),
            profile: p.profile.clone(),
            material: p.material.clone(),
            length: p.length,
            count: p.count as f64,
        })
        .collect()
}

/// `{ L: n }` of a group's stock counts as the JS object (integer keys in ascending order, the rest after).
fn stock_obj(st: &[(f64, usize)]) -> Value {
    let mut o = serde_json::Map::new();
    for (l, n) in st {
        o.insert(num_to_string(*l), Value::from(*n));
    }
    Value::Object(o)
}
/// The JSON of a nesting.
pub fn nest_json(r: &Nest) -> Value {
    let groups: Vec<Value> = r
        .groups
        .iter()
        .map(|g| {
            let bars: Vec<Value> = g
                .bars
                .iter()
                .map(|b| {
                    // an empty stock list leaves the bars' stock undefined (NaN never comes from the list: it keeps v > 0)
                    let o = if b.stock.is_nan() { Obj::new() } else { Obj::new().set("stock", b.stock) };
                    o.set(
                        "cuts",
                        Value::Array(
                            b.cuts.iter().map(|c| Obj::new().set("mark", c.mark.as_str()).set("length", c.length).done()).collect(),
                        ),
                    )
                    .set("used", b.used)
                    .set("drop", b.drop)
                    .set("scrap", b.scrap)
                    .done()
                })
                .collect();
            Obj::new()
                .set("profile", g.profile.as_str())
                .set("material", g.material.as_str())
                .set("bars", Value::Array(bars))
                .set("pieces", g.pieces)
                .set("bought", g.bought)
                .set("used", g.used)
                .set("waste", g.waste)
                .set("tooLong", g.too_long.clone())
                .set("stock", stock_obj(&g.stock))
                .done()
        })
        .collect();
    Obj::new()
        .set("groups", Value::Array(groups))
        .set("totals", Obj::new().set("bars", r.bars).set("bought", r.bought).set("used", r.used).set("waste", r.waste).done())
        .done()
}

/// `cutListTables(res)` → (summary, bars).
pub fn cut_list_tables(res: &Nest) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let m = |v: f64| to_fixed(v / 1000.0, 2);
    let pc = |v: f64| to_fixed(v * 100.0, 1);
    let mut summary = vec![
        ["profile", "material", "pieces", "bars", "bought", "used", "waste", "too-long"]
            .iter()
            .map(|k| t0(&format!("nest.col.{k}")))
            .collect::<Vec<_>>(),
    ];
    let mut bars =
        vec![["profile", "bar", "stock", "cuts", "drop", "keep"].iter().map(|k| t0(&format!("nest.col.{k}"))).collect::<Vec<_>>()];
    for g in &res.groups {
        // Object.entries of the stock counts: integer-like keys ascending first (stock lengths are integers in practice)
        let mut ent: Vec<(String, f64, usize)> = g.stock.iter().map(|(l, n)| (num_to_string(*l), *l, *n)).collect();
        js_key_order(&mut ent);
        let mut ent: Vec<&(String, f64, usize)> = ent.iter().filter(|e| e.2 > 0).collect();
        ent.sort_by(|a, b| crate::js::ord(crate::js::str_to_number(&b.0) - crate::js::str_to_number(&a.0)));
        let buy = ent.iter().map(|(l, _, n)| format!("{n} × {l}")).collect::<Vec<_>>().join(", ");
        summary.push(vec![
            g.profile.clone(),
            g.material.clone(),
            num_to_string(g.pieces),
            buy,
            m(g.bought),
            m(g.used),
            pc(g.waste),
            g.too_long.join(" "),
        ]);
        for (i, b) in g.bars.iter().enumerate() {
            bars.push(vec![
                g.profile.clone(),
                i.saturating_add(1).to_string(),
                if b.stock.is_nan() { "undefined".to_string() } else { num_to_string(b.stock) },
                b.cuts.iter().map(|c| format!("{} {}", c.mark, num_to_string(round(c.length)))).collect::<Vec<_>>().join(" + "),
                num_to_string(round(b.drop)),
                if b.scrap { t0("nest.scrap") } else { t0("nest.kept") },
            ]);
        }
    }
    summary.push(vec![
        t0("nest.total"),
        String::new(),
        num_to_string(sum(res.groups.iter().map(|g| g.pieces))),
        res.bars.to_string(),
        m(res.bought),
        m(res.used),
        pc(res.waste),
        String::new(),
    ]);
    (summary, bars)
}

/// The key order a JS object gives its keys: array indices (canonical integers below 2³²−1) ascending, then the rest in
/// insertion order; a later duplicate key keeps the first place.
fn js_key_order(ent: &mut Vec<(String, f64, usize)>) {
    let mut seen: Vec<(String, f64, usize)> = Vec::new();
    for e in ent.drain(..) {
        if let Some(s) = seen.iter_mut().find(|x| x.0 == e.0) {
            s.2 = e.2;
        } else {
            seen.push(e);
        }
    }
    let is_index = |k: &str| k.parse::<u64>().is_ok_and(|n| n < 4_294_967_295 && n.to_string() == k);
    let (mut idx, rest): (Vec<_>, Vec<_>) = seen.into_iter().partition(|e| is_index(&e.0));
    idx.sort_by_key(|e| e.0.parse::<u64>().unwrap_or(0));
    idx.extend(rest);
    *ent = idx;
}
