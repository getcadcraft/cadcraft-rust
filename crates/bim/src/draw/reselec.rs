//! res-elec.js (0.5 part 4, RC.6), the half that follows the walls: devices read back from CadCraft_Electrical, and
//! the ones on a wall placed again where their stored side and offset put them now (`syncElectrical`).

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::m::{self, V2, hypot};
use crate::draw::res::{Ps, s_or};
use crate::draw::tr::t0;
use crate::draw::walls::{Wall, read_walls};
use crate::draw::writer::{W, round_d, round6, row};
use crate::wapi::P;

pub const ELEC_PSET: &str = "CadCraft_Electrical";

/// (kind, wall-mounted, body width × depth × height)
const DEVICES: [(&str, bool, [f64; 3]); 16] = [
    ("socket", true, [70.0, 12.0, 115.0]),
    ("socket.double", true, [120.0, 12.0, 115.0]),
    ("socket.earth", true, [70.0, 12.0, 115.0]),
    ("socket.3ph", true, [100.0, 15.0, 130.0]),
    ("switch", true, [70.0, 10.0, 115.0]),
    ("switch.2way", true, [70.0, 10.0, 115.0]),
    ("switch.dimmer", true, [70.0, 12.0, 115.0]),
    ("pushbutton", true, [70.0, 10.0, 70.0]),
    ("sensor", false, [110.0, 110.0, 40.0]),
    ("luminaire", false, [300.0, 300.0, 80.0]),
    ("downlight", false, [150.0, 150.0, 30.0]),
    ("luminaire.linear", false, [1200.0, 150.0, 60.0]),
    ("luminaire.emergency", false, [300.0, 120.0, 60.0]),
    ("junction", false, [100.0, 100.0, 55.0]),
    ("appliance", true, [110.0, 18.0, 130.0]),
    ("board", true, [360.0, 100.0, 760.0]),
];
/// The classes devices are written as, in res-elec.js's order.
const CLASSES: [&str; 9] = [
    "IFCOUTLET",
    "IFCFLOWTERMINAL",
    "IFCSWITCHINGDEVICE",
    "IFCFLOWCONTROLLER",
    "IFCSENSOR",
    "IFCLIGHTFIXTURE",
    "IFCJUNCTIONBOX",
    "IFCFLOWFITTING",
    "IFCELECTRICDISTRIBUTIONBOARD",
];

fn device(kind: &str) -> Option<&'static (&'static str, bool, [f64; 3])> {
    DEVICES.iter().find(|d| d.0 == kind)
}
pub fn is_light(kind: &str) -> bool {
    kind.starts_with("luminaire") || kind.starts_with("downlight")
}

/// A device read back (`readDevices`).
#[derive(Clone, Debug)]
pub struct Device {
    pub id: Id,
    pub storey: Option<Id>,
    pub kind: String,
    pub tag: String,
    pub height: f64,
    pub wall: String,
    pub side: f64,
    pub offset: f64,
    pub at: V2,
    pub angle: f64,
}

/// res-elec.js `num(v, d)`: Number(v) when finite, else d.
fn numd(ps: &Ps, k: &str, d: f64) -> f64 {
    let v = ps.get(k).map_or(f64::NAN, P::number);
    if v.is_finite() { v } else { d }
}

/// `/^([A-Za-z]*)(\d*)(.*)$/` of a tag
fn tag_parts(s: &str) -> (&str, &str, &str) {
    let a = s.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (p, rest) = s.split_at(a);
    let b = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (d, r) = rest.split_at(b);
    (p, d, r)
}
/// `cmpTag(a, b)`
pub fn cmp_tag(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (tag_parts(a), tag_parts(b));
    let n = |s: &str| {
        let v = crate::js::str_num(s);
        if v != 0.0 && !v.is_nan() { v } else { 0.0 }
    };
    crate::js::locale_cmp_plain(x.0, y.0)
        .then_with(|| crate::js::sign_ord(n(x.1) - n(y.1)))
        .then_with(|| crate::js::locale_cmp_plain(x.2, y.2))
}

pub fn read_devices(w: &mut W<'_>) -> R<Vec<Device>> {
    let mut out = Vec::new();
    let so = w.storey_of()?;
    let mut seen = std::collections::HashSet::new();
    let mm = w.unit_mm;
    for cls in CLASSES {
        for id in w.ids(cls) {
            if !seen.insert(id_key(id)) {
                continue;
            }
            let Some(ps) = w.read_pset(id, ELEC_PSET)? else { continue };
            let kind = match ps.get("Kind") {
                Some(v) if !matches!(v, P::Undef | P::Null) => v.string(),
                _ => continue,
            };
            if device(&kind).is_none() {
                continue;
            }
            let l = w.must(id)?;
            out.push(Device {
                id,
                storey: so.get(&id_key(id)).copied(),
                kind,
                tag: l.a("Tag").text_or_empty(),
                height: numd(&ps, "MountingHeight", 0.0) * mm,
                wall: s_or(&ps, "Wall"),
                side: if numd(&ps, "Side", 1.0) < 0.0 { -1.0 } else { 1.0 },
                offset: numd(&ps, "Offset", 0.0) * mm,
                at: [numd(&ps, "X", 0.0) * mm, numd(&ps, "Y", 0.0) * mm],
                angle: numd(&ps, "Angle", 0.0) * std::f64::consts::PI / 180.0,
            });
        }
    }
    out.sort_by(|a, b| cmp_tag(&a.tag, &b.tag));
    Ok(out)
}

/// `spotOn(w, s, side)`: the point on the wall's face s along it.
fn spot_on(wall: &Wall, s: f64, side: f64) -> V2 {
    let d = [wall.b[0] - wall.a[0], wall.b[1] - wall.a[1]];
    let l = hypot(d[0], d[1]);
    let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
    let u = [d[0] / l, d[1] / l];
    let n = [-u[1], u[0]];
    [wall.a[0] + u[0] * s + n[0] * side * wall.t / 2.0, wall.a[1] + u[1] * s + n[1] * side * wall.t / 2.0]
}

/// `writeDevice(w, d, walls)`
fn write_device(w: &mut W<'_>, d: &Device, walls: &[Wall]) -> R<()> {
    let Some(dv) = device(&d.kind) else { return Ok(()) };
    let mut at = d.at;
    let mut ang = d.angle;
    let wall = if d.wall.is_empty() { None } else { walls.iter().find(|x| x.guid == d.wall) };
    if let Some(wall) = wall {
        let l = hypot(wall.b[0] - wall.a[0], wall.b[1] - wall.a[1]);
        let s = m::min(m::max(0.0, d.offset), l);
        at = spot_on(wall, s, d.side);
        let n = [-(wall.b[1] - wall.a[1]) / l, (wall.b[0] - wall.a[0]) / l];
        ang = m::atan2(n[1] * d.side, n[0] * d.side);
    }
    let deg = (ang * 180.0) / std::f64::consts::PI - 90.0;
    let l = w.must(d.id)?;
    if let Some(lp) = l.a("ObjectPlacement").href() {
        w.move_placement(lp, at[0], at[1], 0.0, deg)?;
    }
    let [bw, bd, bh] = dv.2;
    let h = d.height;
    let solid = if dv.1 {
        w.boxx(-bw / 2.0, 0.0, h - bh / 2.0, bw / 2.0, bd, h + bh / 2.0)?
    } else {
        w.boxx(-bw / 2.0, -bd / 2.0, h - bh, bw / 2.0, bd / 2.0, h)?
    };
    let color = if d.kind == "board" {
        [0.55, 0.57, 0.6]
    } else if is_light(&d.kind) {
        [0.98, 0.95, 0.75]
    } else {
        [0.95, 0.95, 0.93]
    };
    let st = w.surface_style(&t0(if d.kind == "board" { "res.style.panel" } else { "res.style.device" }), color, 0.0)?;
    w.style_item(solid, st)?;
    let rep = w.rep("body", "Body", "SweptSolid", &[solid])?;
    w.set_shape(d.id, &[rep])?;
    w.upsert_pset(
        d.id,
        ELEC_PSET,
        &[
            row("X", "IFCLENGTHMEASURE", P::Num(round6(w.u(at[0])))),
            row("Y", "IFCLENGTHMEASURE", P::Num(round6(w.u(at[1])))),
            row("Angle", "IFCREAL", P::Num(round_d((ang * 180.0) / std::f64::consts::PI, 4))),
        ],
    )?;
    Ok(())
}

/// `syncElectrical(w, storey)`: devices on walls follow their wall.
pub fn sync_electrical(w: &mut W<'_>, storey: Id) -> R<usize> {
    let devs: Vec<Device> = read_devices(w)?.into_iter().filter(|d| d.storey == Some(storey) && !d.wall.is_empty()).collect();
    if devs.is_empty() {
        return Ok(0);
    }
    let walls = read_walls(w, Some(storey))?;
    let mut n = 0usize;
    for d in &devs {
        let Some(wall) = walls.iter().find(|x| x.guid == d.wall) else { continue };
        let l = hypot(wall.b[0] - wall.a[0], wall.b[1] - wall.a[1]);
        let at = spot_on(wall, m::min(m::max(0.0, d.offset), l), d.side);
        if hypot(at[0] - d.at[0], at[1] - d.at[1]) < 0.5 {
            continue;
        }
        write_device(w, d, &walls)?;
        n = n.saturating_add(1);
    }
    Ok(n)
}
