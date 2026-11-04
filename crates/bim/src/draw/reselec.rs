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

pub(crate) fn device(kind: &str) -> Option<&'static (&'static str, bool, [f64; 3])> {
    DEVICES.iter().find(|d| d.0 == kind)
}
pub fn is_light(kind: &str) -> bool {
    kind.starts_with("luminaire") || kind.starts_with("downlight")
}

/// A device read back (`readDevices`).
#[derive(Clone, Debug)]
pub struct Device {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub level: String,
    pub circuit: String,
    pub load: f64,
    pub controls: Vec<String>,
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
            let guid = l.a("GlobalId").val_str().unwrap_or_default();
            let circuit = match ps.get("Circuit") {
                Some(v) if !matches!(v, P::Undef | P::Null) => v.string(),
                _ => String::new(),
            };
            let controls = match ps.get("Controls") {
                Some(v) if v.truthy() => v.string().split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                _ => Vec::new(),
            };
            out.push(Device {
                id,
                guid,
                storey: so.get(&id_key(id)).copied(),
                level: s_or(&ps, "Level"),
                circuit,
                load: numd(&ps, "Load", 0.0),
                controls,
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
pub(crate) fn spot_on(wall: &Wall, s: f64, side: f64) -> V2 {
    let d = [wall.b[0] - wall.a[0], wall.b[1] - wall.a[1]];
    let l = hypot(d[0], d[1]);
    let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
    let u = [d[0] / l, d[1] / l];
    let n = [-u[1], u[0]];
    [wall.a[0] + u[0] * s + n[0] * side * wall.t / 2.0, wall.a[1] + u[1] * s + n[1] * side * wall.t / 2.0]
}

/// `writeDevice(w, d, walls)`
pub(crate) fn write_device(w: &mut W<'_>, d: &Device, walls: &[Wall]) -> R<()> {
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

// ------------------------------------------------------------------------------------------------------------ symbols
/// One drawing element of a device symbol (res-elec-symbols.js; paper mm, base point on the wall, +y into the room).
#[derive(Clone, Copy, Debug)]
enum Ent {
    Line(f64, f64, f64, f64),
    Circle(f64, f64, f64),
    Arc(f64, f64, f64, f64, f64),
    Poly(&'static [[f64; 2]]),
    Hatch(&'static [f64]),
}

/// res-elec-symbols.js SYMBOLS: (key, group, entities). Generated there from the Electrical add-on's symbols.
const SYMBOLS: [(&str, &str, &[Ent]); 16] = [
    (
        "socket",
        "outlets",
        &[Ent::Arc(0.0, 0.0, 3.0, 0.0, std::f64::consts::PI), Ent::Line(-3.0, 0.0, 3.0, 0.0), Ent::Line(0.0, 3.0, 0.0, 4.5)],
    ),
    (
        "socket.double",
        "outlets",
        &[
            Ent::Arc(0.0, 0.0, 3.0, 0.0, std::f64::consts::PI),
            Ent::Line(-3.0, 0.0, 3.0, 0.0),
            Ent::Line(-1.0, 3.0, -1.0, 4.5),
            Ent::Line(1.0, 3.0, 1.0, 4.5),
        ],
    ),
    (
        "socket.earth",
        "outlets",
        &[
            Ent::Arc(0.0, 0.0, 3.0, 0.0, std::f64::consts::PI),
            Ent::Line(-3.0, 0.0, 3.0, 0.0),
            Ent::Line(0.0, 3.0, 0.0, 4.5),
            Ent::Line(-3.0, 3.0, 3.0, 3.0),
        ],
    ),
    (
        "socket.3ph",
        "outlets",
        &[
            Ent::Arc(0.0, 0.0, 3.0, 0.0, std::f64::consts::PI),
            Ent::Line(-3.0, 0.0, 3.0, 0.0),
            Ent::Line(-1.2, 2.7, -1.6, 4.4),
            Ent::Line(0.0, 3.0, 0.0, 4.5),
            Ent::Line(1.2, 2.7, 1.6, 4.4),
        ],
    ),
    ("switch", "switches", &[Ent::Circle(0.0, 0.0, 1.2), Ent::Line(0.85, 0.85, 3.4, 3.4), Ent::Line(3.4, 3.4, 4.3, 2.5)]),
    (
        "switch.2way",
        "switches",
        &[
            Ent::Circle(0.0, 0.0, 1.2),
            Ent::Line(0.85, 0.85, 3.4, 3.4),
            Ent::Line(3.4, 3.4, 4.3, 2.5),
            Ent::Line(-0.85, -0.85, -3.4, -3.4),
            Ent::Line(-3.4, -3.4, -4.3, -2.5),
        ],
    ),
    (
        "switch.dimmer",
        "switches",
        &[
            Ent::Circle(0.0, 0.0, 1.2),
            Ent::Line(0.85, 0.85, 3.4, 3.4),
            Ent::Line(3.4, 3.4, 4.3, 2.5),
            Ent::Line(-0.6, -2.2, 0.6, -2.2),
            Ent::Line(0.6, -2.2, 0.6, -1.6),
        ],
    ),
    ("pushbutton", "switches", &[Ent::Circle(0.0, 0.0, 1.6), Ent::Circle(0.0, 0.0, 0.6)]),
    (
        "sensor",
        "switches",
        &[
            Ent::Circle(0.0, 0.0, 2.0),
            Ent::Arc(0.0, 0.0, 3.0, std::f64::consts::FRAC_PI_6, 2.617994),
            Ent::Arc(0.0, 0.0, 3.8, std::f64::consts::FRAC_PI_4, 2.356194),
        ],
    ),
    ("luminaire", "lights", &[Ent::Circle(0.0, 0.0, 3.0), Ent::Line(-2.12, -2.12, 2.12, 2.12), Ent::Line(-2.12, 2.12, 2.12, -2.12)]),
    ("downlight", "lights", &[Ent::Circle(0.0, 0.0, 2.0), Ent::Circle(0.0, 0.0, 0.8)]),
    ("luminaire.linear", "lights", &[Ent::Line(-5.0, 0.0, 5.0, 0.0), Ent::Line(-5.0, -1.2, -5.0, 1.2), Ent::Line(5.0, -1.2, 5.0, 1.2)]),
    (
        "luminaire.emergency",
        "lights",
        &[
            Ent::Circle(0.0, 0.0, 3.0),
            Ent::Line(-2.12, -2.12, 2.12, 2.12),
            Ent::Line(-2.12, 2.12, 2.12, -2.12),
            Ent::Hatch(&[-3.0, -0.5, 3.0, -0.5, -3.0, 0.5]),
        ],
    ),
    (
        "junction",
        "other",
        &[
            Ent::Circle(0.0, 0.0, 1.5),
            Ent::Hatch(&[
                0.7, 0.0, 0.606218, 0.35, 0.35, 0.606218, 0.0, 0.7, -0.35, 0.606218, -0.606218, 0.35, -0.7, 0.0, -0.606218, -0.35, -0.35,
                -0.606218, 0.0, -0.7, 0.35, -0.606218, 0.606218, -0.35,
            ]),
        ],
    ),
    ("appliance", "other", &[Ent::Poly(&[[-3.0, -3.0], [3.0, -3.0], [3.0, 3.0], [-3.0, 3.0]])]),
    (
        "board",
        "boards",
        &[Ent::Poly(&[[-5.0, -2.0], [5.0, -2.0], [5.0, 2.0], [-5.0, 2.0]]), Ent::Hatch(&[-5.0, -2.0, 5.0, -2.0, -5.0, 2.0])],
    ),
];
/// res-elec-symbols.js GROUPS
pub const GROUPS: [&str; 5] = ["outlets", "switches", "lights", "other", "boards"];

/// The device kinds of a symbol group, in the catalog's order.
pub fn kinds_in(group: &str) -> Vec<&'static str> {
    SYMBOLS.iter().filter(|s| s.1 == group).map(|s| s.0).collect()
}
/// Mounted on a wall (else on the ceiling).
pub fn wall_mounted(kind: &str) -> bool {
    device(kind).is_some_and(|d| d.1)
}
/// `symbolScale(units)`: plan mm per paper mm.
pub fn symbol_scale(units: &str) -> f64 {
    if units == "mm" { 50.0 } else { 48.0 }
}

/// A circle or an arc of a symbol (plan mm, radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymArc {
    pub c: V2,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
}
/// `symbolPlan(kind, at, angle, scale)`: lines, arcs, thin lines (hatches) and the picking box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymPlan {
    pub lines: Vec<[f64; 4]>,
    pub arcs: Vec<SymArc>,
    pub thin: Vec<[f64; 4]>,
    pub poly: Vec<V2>,
}

pub fn symbol_plan(kind: &str, at: V2, angle: f64, scale: f64) -> Option<SymPlan> {
    let s = SYMBOLS.iter().find(|x| x.0 == kind)?;
    let (fx, fy) = (m::cos(angle), m::sin(angle));
    let (rx, ry) = (fy, -fx);
    let pp = |x: f64, y: f64| -> V2 { [at[0] + (x * rx + y * fx) * scale, at[1] + (x * ry + y * fy) * scale] };
    let rot = m::atan2(ry, rx);
    let mut out = SymPlan::default();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut grow = |q: V2, r: f64| {
        x0 = m::min(x0, q[0] - r);
        y0 = m::min(y0, q[1] - r);
        x1 = m::max(x1, q[0] + r);
        y1 = m::max(y1, q[1] + r);
    };
    for e in s.2 {
        match *e {
            Ent::Line(a, b, c, d) => {
                let (p, q) = (pp(a, b), pp(c, d));
                out.lines.push([p[0], p[1], q[0], q[1]]);
                grow(p, 0.0);
                grow(q, 0.0);
            }
            Ent::Circle(cx, cy, r) => {
                let c = pp(cx, cy);
                out.arcs.push(SymArc { c, r: r * scale, a0: 0.0, a1: std::f64::consts::PI * 2.0 });
                grow(c, r * scale);
            }
            Ent::Arc(cx, cy, r, a0, a1) => {
                let c = pp(cx, cy);
                let mirror = rx * fy - ry * fx < 0.0;
                let (b0, b1) = if mirror { (rot - a1, rot - a0) } else { (rot + a0, rot + a1) };
                out.arcs.push(SymArc { c, r: r * scale, a0: b0, a1: b1 });
                grow(c, r * scale);
            }
            Ent::Poly(pts) => {
                let q: Vec<V2> = pts.iter().map(|v| pp(v[0], v[1])).collect();
                for i in 0..q.len() {
                    let (a, b) = (m::at(&q, i), m::at(&q, m::nx(i, q.len())));
                    if i.saturating_add(1) < q.len() || q.len() > 2 {
                        out.lines.push([a[0], a[1], b[0], b[1]]);
                        grow(a, 0.0);
                        grow(b, 0.0);
                    }
                }
            }
            Ent::Hatch(l) => {
                let q: Vec<V2> = l.as_chunks::<2>().0.iter().map(|v| pp(v[0], v[1])).collect();
                for i in 0..q.len() {
                    let (a, b) = (m::at(&q, i), m::at(&q, m::nx(i, q.len())));
                    out.thin.push([a[0], a[1], b[0], b[1]]);
                    grow(a, 0.0);
                    grow(b, 0.0);
                }
            }
        }
    }
    if !x0.is_finite() {
        return None;
    }
    let pad = scale * 0.8;
    out.poly = vec![[x0 - pad, y0 - pad], [x1 + pad, y0 - pad], [x1 + pad, y1 + pad], [x0 - pad, y1 + pad]];
    Some(out)
}

/// `wallSpot(walls, p, reach)`: the nearest wall face within reach → (wall guid, side, offset along, the spot, angle).
pub fn wall_spot(walls: &[Wall], p: V2, reach: f64) -> Option<(String, f64, f64, V2, f64)> {
    let mut best: Option<(String, f64, f64, f64, V2, f64)> = None;
    for w in walls {
        let d = [w.b[0] - w.a[0], w.b[1] - w.a[1]];
        let l = hypot(d[0], d[1]);
        if l < 1.0 {
            continue;
        }
        let u = [d[0] / l, d[1] / l];
        let n = [-u[1], u[0]];
        let s = m::max(0.0, m::min(l, (p[0] - w.a[0]) * u[0] + (p[1] - w.a[1]) * u[1]));
        let off = (p[0] - w.a[0]) * n[0] + (p[1] - w.a[1]) * n[1];
        let side = if off >= 0.0 { 1.0 } else { -1.0 };
        let dist = (off.abs() - w.t / 2.0).abs();
        let along = (p[0] - w.a[0]) * u[0] + (p[1] - w.a[1]) * u[1];
        if along < -w.t || along > l + w.t || dist > reach {
            continue;
        }
        if best.as_ref().is_none_or(|b| dist < b.3) {
            best = Some((w.guid.clone(), side, s, dist, spot_on(w, s, side), m::atan2(n[1] * side, n[0] * side)));
        }
    }
    best.map(|(g, side, s, _, at, ang)| (g, side, s, at, ang))
}
