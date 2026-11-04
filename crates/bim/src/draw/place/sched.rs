//! ifc-place.js `schedule()`: the placed library items as an equipment schedule — one row for the same item number,
//! name, maker, model and connections, its places and quantities, which storeys show it, a clash where one item
//! number stands for different equipment (cc-dwgview's equipment list draws it; `elecText` is its).

use crate::api::R;
use crate::draw::place::Placer;
use crate::wapi::P;

/// A row's electrical connection (the item's Elec* fields; `conn`: "direct" | "cord and plug" | "").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SElec {
    pub volts: String,
    pub phase: String,
    pub amps: String,
    pub kw: String,
    pub hp: String,
    pub hz: String,
    pub conn: &'static str,
    pub nema: String,
}
/// A place of a row: where, how many, the storey that shows it, the product.
#[derive(Clone, Debug, PartialEq)]
pub struct SPlace {
    pub x: f64,
    pub y: f64,
    pub qty: String,
    pub sheets: Vec<String>,
    pub id: f64,
}
/// One schedule row.
#[derive(Clone, Debug, PartialEq)]
pub struct SRow {
    pub no: String,
    pub category: String,
    pub mfr: String,
    pub model: String,
    pub remark: String,
    pub elec: Vec<SElec>,
    /// cw, hw, dd, id, gas
    pub plumb: [(&'static str, String); 5],
    pub places: Vec<SPlace>,
    pub sheets: Vec<String>,
    pub qtys: Vec<String>,
    pub qty: String,
    pub plumb_text: String,
    pub clash: bool,
}
/// The schedule: rows, how many placements, the storeys with a count, the rows on no storey.
#[derive(Clone, Debug, PartialEq)]
pub struct Sched {
    pub rows: Vec<SRow>,
    pub tags: usize,
    pub sheets: Vec<(String, String, usize)>,
    pub off_sheet: usize,
}

/// `x || ''` of a property value.
fn or_empty(v: Option<&P>) -> String {
    match v {
        Some(p) if p.truthy() => p.string(),
        _ => String::new(),
    }
}

impl Placer<'_> {
    /// `schedule()` → None when nothing is placed.
    pub fn schedule(&mut self) -> R<Option<Sched>> {
        let storeys = self.info()?;
        let mut rows: Vec<(String, SRow)> = Vec::new();
        let mut tags = 0usize;
        for p in self.placements()? {
            tags = tags.saturating_add(1);
            let e = |k: &str| or_empty(p.equip(k));
            let yes = |k: &str| e(k).to_ascii_lowercase().contains("yes");
            let elec = SElec {
                volts: e("ElecVolts"),
                phase: e("ElecPhase"),
                amps: e("ElecAmps"),
                kw: e("ElecKw"),
                hp: e("ElecHp"),
                hz: e("ElecHz"),
                conn: if yes("ElecDirect") {
                    "direct"
                } else if yes("ElecPlug") {
                    "cord and plug"
                } else {
                    ""
                },
                nema: e("ElecNema"),
            };
            let any = [&elec.volts, &elec.phase, &elec.amps, &elec.kw, &elec.hp, &elec.hz, &elec.nema].iter().any(|s| !s.is_empty())
                || !elec.conn.is_empty();
            let plumb = [("cw", e("PlumbCw")), ("hw", e("PlumbHw")), ("dd", e("PlumbDd")), ("id", e("PlumbId")), ("gas", e("PlumbGas"))];
            let category = {
                let n = e("Name");
                if n.is_empty() { p.name.clone() } else { n }
            };
            let row = SRow {
                no: p.item_number.clone(),
                category,
                mfr: e("Maker"),
                model: e("Model"),
                remark: e("Remarks"),
                elec: if any { vec![elec.clone()] } else { Vec::new() },
                plumb: plumb.clone(),
                places: Vec::new(),
                sheets: Vec::new(),
                qtys: Vec::new(),
                qty: String::new(),
                plumb_text: String::new(),
                clash: false,
            };
            let key = format!(
                "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{:?}\u{0}{:?}",
                row.no,
                row.category,
                row.mfr,
                row.model,
                (&elec.volts, &elec.phase, &elec.amps, &elec.kw, &elec.hp, &elec.hz, elec.conn, &elec.nema),
                plumb
            );
            let i = match rows.iter().position(|r| r.0 == key) {
                Some(i) => i,
                None => {
                    rows.push((key, row));
                    rows.len().saturating_sub(1)
                }
            };
            let qty = match &p.quantity {
                P::Undef | P::Null => "1".to_string(),
                q => q.string(),
            };
            let sheets = p.storey.map(|s| vec![format!("storey:{}", crate::js::num_str(s))]).unwrap_or_default();
            if let Some(r) = rows.get_mut(i) {
                r.1.places.push(SPlace { x: p.x, y: p.y, qty, sheets, id: p.id });
            }
        }
        let mut list: Vec<SRow> = rows.into_iter().map(|r| r.1).collect();
        for r in &mut list {
            let mut sh: Vec<String> = Vec::new();
            for s in r.places.iter().flat_map(|x| x.sheets.iter()) {
                if !sh.contains(s) {
                    sh.push(s.clone());
                }
            }
            r.sheets = sh;
            r.qtys = r.places.iter().map(|x| x.qty.clone()).collect();
            let sum: f64 = r.qtys.iter().map(|q| crate::js::str_num(q)).map(|v| if v != 0.0 && !v.is_nan() { v } else { 0.0 }).sum();
            r.qty = crate::js::num_str(sum);
            r.plumb_text = r
                .plumb
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{} {v}", k.to_uppercase()))
                .collect::<Vec<_>>()
                .join(", ");
        }
        let nos: Vec<String> = list.iter().map(|r| r.no.clone()).collect();
        for r in &mut list {
            r.clash = nos.iter().filter(|n| **n == r.no).count() > 1;
        }
        list.sort_by(|a, b| crate::js::locale_cmp(&a.no, &b.no).then_with(|| crate::js::locale_cmp_plain(&a.category, &b.category)));
        let sheets = storeys
            .iter()
            .map(|s| {
                let key = format!("storey:{}", crate::js::num_str(s.id));
                let n = list.iter().filter(|r| r.sheets.contains(&key)).count();
                (key, s.name.clone(), n)
            })
            .filter(|s| s.2 > 0)
            .collect();
        let off_sheet = list.iter().filter(|r| r.sheets.is_empty()).count();
        Ok((tags > 0).then_some(Sched { rows: list, tags, sheets, off_sheet }))
    }
}
