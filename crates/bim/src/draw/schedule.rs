//! model-schedule.js — the live schedules of what was drawn: doors, windows, walls by type, rooms, with their CSV
//! (ifc-qto.js `tableCsv`, as every other BimCraft table).

use std::collections::HashMap;

use cc_io_ifc::model::Id;

use crate::draw::openings::{opening_type, type_key_name};
use crate::draw::ops::Snapshot;
use crate::draw::rooms::{Loops, loop_at};
use crate::draw::tr::t;
use crate::draw::units::{fmt_area, fmt_len};
use crate::draw::walls::{Wall, type_name, wall_type};
use crate::qto::{Cell, table_csv};

/// One schedule: its head, rows of text, an optional total row, and the rows' numbers (mm / mm² / counts).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schedule {
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total: Option<Vec<String>>,
    pub nums: Vec<Vec<f64>>,
}

/// The four schedules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schedules {
    pub doors: Schedule,
    pub windows: Schedule,
    pub walls: Schedule,
    pub rooms: Schedule,
}

impl Schedules {
    pub fn get(&self, k: &str) -> Option<&Schedule> {
        match k {
            "doors" => Some(&self.doors),
            "windows" => Some(&self.windows),
            "walls" => Some(&self.walls),
            "rooms" => Some(&self.rooms),
            _ => None,
        }
    }
}

fn wall_type_name(w: Option<&Wall>) -> String {
    match w {
        Some(w) => match wall_type(&w.type_key) {
            Some(ty) => type_name(&ty),
            None => w.type_name.clone(),
        },
        None => String::new(),
    }
}

/// `schedules(snap, loops, { fmtLen, fmtArea, t })` (`loops`: storey → its rooms and outlines).
pub fn schedules(sn: &Snapshot, loops: &HashMap<u64, Loops>, units: &str) -> Schedules {
    let level_name = |id: Option<Id>| sn.levels.iter().find(|l| Some(l.id) == id).map(|l| l.name.clone()).unwrap_or_default();
    let level_ix = |id: Option<Id>| sn.levels.iter().position(|l| Some(l.id) == id).map_or(-1.0, |i| i as f64);
    let fl = |mm: f64| fmt_len(mm, units);
    let fa = |mm2: f64| fmt_area(mm2, units);
    let opening = |door: bool| -> Schedule {
        let mut list: Vec<&crate::draw::openings::Opening> = sn.openings.iter().filter(|o| o.door == door).collect();
        list.sort_by(|a, b| {
            let d = level_ix(a.storey) - level_ix(b.storey);
            if d != 0.0 && !d.is_nan() { crate::js::sign_ord(d) } else { crate::js::locale_cmp(&a.mark, &b.mark) }
        });
        let mut head = vec![t("model.sch.mark", &[]), t("model.ot.type", &[]), t("model.ot.width", &[]), t("model.ot.height", &[])];
        if !door {
            head.push(t("model.ot.sill", &[]));
        }
        head.push(t("model.level", &[]));
        head.push(t("model.sch.wall", &[]));
        let rows = list
            .iter()
            .map(|o| {
                let mut r = vec![
                    o.mark.clone(),
                    opening_type(&o.type_key)
                        .map_or_else(|| type_key_name(&o.type_key), |ty| crate::draw::openings::opening_type_name(&ty)),
                    fl(o.width),
                    fl(o.height),
                ];
                if !door {
                    r.push(fl(o.sill));
                }
                r.push(level_name(o.storey));
                r.push(wall_type_name(sn.walls.iter().find(|w| w.id == o.wall)));
                r
            })
            .collect();
        let nums = list.iter().map(|o| vec![o.width, o.height, o.sill]).collect();
        Schedule { head, rows, nums, total: Some(vec![t("model.sch.count", &[("n", &list.len().to_string())])]) }
    };
    // walls by type: how many, their length, net side area
    struct G {
        n: usize,
        len: f64,
        area: f64,
        t: f64,
        name: String,
    }
    let mut order: Vec<String> = Vec::new();
    let mut types: HashMap<String, G> = HashMap::new();
    for w in &sn.walls {
        let k = w.type_key.clone();
        if !types.contains_key(&k) {
            order.push(k.clone());
            types.insert(k.clone(), G { n: 0, len: 0.0, area: 0.0, t: w.t, name: wall_type_name(Some(w)) });
        }
        let open_area = sn.openings.iter().filter(|o| o.wall == w.id).fold(0.0, |s, o| s + o.width * o.height);
        if let Some(g) = types.get_mut(&k) {
            g.n = g.n.saturating_add(1);
            g.len += w.len;
            g.area += crate::draw::m::max(0.0, w.len * w.height - open_area);
        }
    }
    let mut wl: Vec<&G> = order.iter().filter_map(|k| types.get(k)).collect();
    wl.sort_by(|a, b| crate::js::locale_cmp_plain(&a.name, &b.name));
    let walls = Schedule {
        head: vec![
            t("model.wall.type", &[]),
            t("model.prop.thickness", &[]),
            t("model.sch.count.h", &[]),
            t("model.prop.length", &[]),
            t("model.sch.netarea", &[]),
        ],
        rows: wl.iter().map(|g| vec![g.name.clone(), fl(g.t), g.n.to_string(), fl(g.len), fa(g.area)]).collect(),
        nums: wl.iter().map(|g| vec![g.n as f64, g.len, g.area]).collect(),
        total: Some(vec![
            t("model.sch.total", &[]),
            String::new(),
            wl.iter().fold(0usize, |s, g| s.saturating_add(g.n)).to_string(),
            fl(wl.iter().fold(0.0, |s, g| s + g.len)),
            fa(wl.iter().fold(0.0, |s, g| s + g.area)),
        ]),
    };
    // rooms
    let mut rl: Vec<(&crate::draw::slabs::Space, crate::draw::rooms::Room)> = Vec::new();
    for sp in &sn.derived.spaces {
        let Some(l) = sp.storey.and_then(|s| loops.get(&crate::draw::plan::key_u64(s))) else { continue };
        let Some(lp) = loop_at(&l.rooms, sp.seed) else { continue };
        rl.push((sp, lp.clone()));
    }
    rl.sort_by(|a, b| crate::js::locale_cmp(&a.0.number, &b.0.number));
    let rooms = Schedule {
        head: vec![
            t("model.room.number", &[]),
            t("model.room.longname", &[]),
            t("model.level", &[]),
            t("model.room.area", &[]),
            t("model.room.perimeter", &[]),
        ],
        rows: rl
            .iter()
            .map(|(sp, lp)| vec![sp.number.clone(), sp.name.clone(), level_name(sp.storey), fa(lp.area), fl(lp.perimeter)])
            .collect(),
        nums: rl.iter().map(|(_, lp)| vec![lp.area, lp.perimeter]).collect(),
        total: Some(vec![
            t("model.sch.total", &[]),
            String::new(),
            String::new(),
            fa(rl.iter().fold(0.0, |s, r| s + r.1.area)),
            String::new(),
        ]),
    };
    Schedules { doors: opening(true), windows: opening(false), walls, rooms }
}

/// `scheduleCsv(s)`: the table as CSV, the total row last (padded to the head's width).
pub fn schedule_csv(s: &Schedule) -> String {
    let cell = |x: &String| Cell::Str(x.clone());
    let head: Vec<Cell> = s.head.iter().map(cell).collect();
    let mut rows: Vec<Vec<Cell>> = s.rows.iter().map(|r| r.iter().map(cell).collect()).collect();
    if let Some(tot) = &s.total {
        let mut r: Vec<Cell> = tot.iter().map(cell).collect();
        while r.len() < s.head.len() {
            r.push(Cell::Str(String::new()));
        }
        rows.push(r);
    }
    table_csv(&head, &rows)
}
