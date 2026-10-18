//! model-openings.js — doors and windows hosted in walls: starter types, the IFC they are written as (an opening cut
//! through the wall, the door / window in it with a body built from its parameters), reading them back, moving and
//! changing them, and their plan symbols.

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::m::{self, V2, sub};
use crate::draw::tr::t0;
use crate::draw::walls::{Wall, ps_num, ps_str, read_walls};
use crate::draw::writer::{QRow, W, key_err, round6, row};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;
pub const DOOR_PSET: &str = "CadCraft_Door";
pub const WINDOW_PSET: &str = "CadCraft_Window";
pub const OPENING_PSET: &str = "CadCraft_Opening";
const PROUD: f64 = 10.0;
const FRAME: f64 = 1.5 * IN;
const LEAF: f64 = 1.75 * IN;
const WFRAME: f64 = 2.0 * IN;
const WDEPTH: f64 = 4.5 * IN;
const GLASS: f64 = 6.0;

/// A starter door or window.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OType {
    pub key: &'static str,
    pub door: bool,
    pub width: f64,
    pub height: f64,
    pub sill: f64,
    pub leaves: u8,
    pub slide: bool,
    pub pocket: bool,
    pub garage: bool,
    pub bifold: bool,
    pub glazed: bool,
    pub barn: bool,
    pub sidelites: f64,
    pub res: bool,
    pub op: [&'static str; 2],
    pub part: &'static str,
    pub rail: bool,
    pub stile: bool,
    pub casement: bool,
    pub awning: bool,
}

pub fn door_types() -> Vec<OType> {
    let d = |key, width: f64, height: f64, op: [&'static str; 2]| OType { key, door: true, width, height, op, ..OType::default() };
    vec![
        OType { leaves: 1, ..d("door-single", 36.0 * IN, 80.0 * IN, ["SINGLE_SWING_LEFT", "SINGLE_SWING_RIGHT"]) },
        OType { leaves: 2, ..d("door-double", 60.0 * IN, 80.0 * IN, ["DOUBLE_DOOR_SINGLE_SWING", "DOUBLE_DOOR_SINGLE_SWING"]) },
        OType { slide: true, ..d("door-sliding", 72.0 * IN, 80.0 * IN, ["SLIDING_TO_LEFT", "SLIDING_TO_RIGHT"]) },
        OType { pocket: true, ..d("door-pocket", 32.0 * IN, 80.0 * IN, ["SLIDING_TO_LEFT", "SLIDING_TO_RIGHT"]) },
        OType { garage: true, ..d("door-garage", 16.0 * 12.0 * IN, 7.0 * 12.0 * IN, ["ROLLINGUP", "ROLLINGUP"]) },
        OType { bifold: true, res: true, ..d("door-bifold", 36.0 * IN, 80.0 * IN, ["FOLDING_TO_LEFT", "FOLDING_TO_RIGHT"]) },
        OType {
            leaves: 2,
            glazed: true,
            res: true,
            ..d("door-french", 60.0 * IN, 80.0 * IN, ["DOUBLE_DOOR_SINGLE_SWING", "DOUBLE_DOOR_SINGLE_SWING"])
        },
        OType { barn: true, res: true, ..d("door-barn", 36.0 * IN, 84.0 * IN, ["SLIDING_TO_LEFT", "SLIDING_TO_RIGHT"]) },
        OType {
            sidelites: 12.0 * IN,
            res: true,
            ..d("door-entry-sidelites", 60.0 * IN, 80.0 * IN, ["SINGLE_SWING_LEFT", "SINGLE_SWING_RIGHT"])
        },
    ]
}

pub fn window_types() -> Vec<OType> {
    let w = |key, width: f64, height: f64, sill: f64, part: &'static str| OType {
        key,
        door: false,
        width,
        height,
        sill,
        part,
        ..OType::default()
    };
    vec![
        w("win-fixed", 48.0 * IN, 48.0 * IN, 32.0 * IN, "SINGLE_PANEL"),
        OType { rail: true, ..w("win-single-hung", 36.0 * IN, 48.0 * IN, 32.0 * IN, "DOUBLE_PANEL_HORIZONTAL") },
        OType { rail: true, ..w("win-double-hung", 36.0 * IN, 60.0 * IN, 20.0 * IN, "DOUBLE_PANEL_HORIZONTAL") },
        OType { casement: true, ..w("win-casement", 30.0 * IN, 48.0 * IN, 32.0 * IN, "SINGLE_PANEL") },
        OType { stile: true, ..w("win-slider", 60.0 * IN, 48.0 * IN, 32.0 * IN, "DOUBLE_PANEL_VERTICAL") },
        OType { awning: true, res: true, ..w("win-awning", 36.0 * IN, 24.0 * IN, 56.0 * IN, "SINGLE_PANEL") },
        OType { res: true, ..w("win-picture", 72.0 * IN, 60.0 * IN, 20.0 * IN, "SINGLE_PANEL") },
        OType { res: true, ..w("win-transom", 36.0 * IN, 12.0 * IN, 82.0 * IN, "SINGLE_PANEL") },
    ]
}

/// `openingType(key)`
pub fn opening_type(key: &str) -> Option<OType> {
    door_types().into_iter().chain(window_types()).find(|x| x.key == key)
}
/// `openingTypeName(type)`
pub fn opening_type_name(ty: &OType) -> String {
    t0(&format!("{}{}", if ty.res { "res.ot." } else { "model.ot." }, ty.key))
}
/// The name of a type key that may not be a starter (`openingTypeName(openingType(k) || { key: k })`).
pub fn type_key_name(key: &str) -> String {
    match opening_type(key) {
        Some(t) => opening_type_name(&t),
        None => t0(&format!("model.ot.{key}")),
    }
}

/// A door or window the tools made.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub id: Id,
    pub guid: String,
    pub door: bool,
    pub type_key: String,
    pub type_name: String,
    pub opening_id: Id,
    pub opening: String,
    pub wall: Id,
    pub wall_guid: String,
    pub storey: Option<Id>,
    pub offset: f64,
    pub width: f64,
    pub height: f64,
    pub sill: f64,
    pub hand: String,
    pub face: String,
    pub name: String,
    pub mark: String,
}

impl Opening {
    pub fn kind(&self) -> &'static str {
        if self.door { "door" } else { "window" }
    }
}

/// `readOpenings(w, storey, walls)`
pub fn read_openings(w: &mut W<'_>, storey: Option<Id>, walls: &[Wall]) -> R<Vec<Opening>> {
    let r = w.host_index()?;
    let mut out = Vec::new();
    for cls in ["IFCDOOR", "IFCWINDOW"] {
        let door = cls == "IFCDOOR";
        for id in w.ids(cls) {
            let Some(ps) = w.read_pset(id, if door { DOOR_PSET } else { WINDOW_PSET })? else { continue };
            let Some(opening_id) = r.filled_by.get(&id_key(id)).copied() else { continue };
            let Some(wall_id) = r.void_host.get(&id_key(opening_id)).copied() else { continue };
            let Some(wall) = walls.iter().find(|x| x.id == wall_id) else { continue };
            if let Some(s) = storey
                && wall.storey != Some(s)
            {
                continue;
            }
            let line = w.must(id)?;
            let op = w.must(opening_id)?;
            let olp = op.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
            let mm = w.rel_matrix(wall.lp, olp)?;
            out.push(Opening {
                id,
                guid: line.a("GlobalId").val_str().unwrap_or_default(),
                door,
                type_key: ps_str(&ps, "TypeKey"),
                type_name: crate::draw::levels::name_or(line.a("ObjectType"), line.a("Name"), ""),
                opening_id,
                opening: op.a("GlobalId").val_str().unwrap_or_default(),
                wall: wall_id,
                wall_guid: wall.guid.clone(),
                storey: wall.storey,
                offset: mm[12],
                width: ps_num(&ps, "Width") * w.unit_mm,
                height: ps_num(&ps, "Height") * w.unit_mm,
                sill: mm[14] + wall.base_offset,
                hand: if ps.get("Hand").and_then(P::as_str).as_deref() == Some("b") { "b".into() } else { "a".into() },
                face: if ps.get("Face").and_then(P::as_str).as_deref() == Some("right") { "right".into() } else { "left".into() },
                name: line.a("Name").text_or_empty(),
                mark: line.a("Tag").text_or_empty(),
            });
        }
    }
    Ok(out)
}

/// `fitOffset(wall, width, offset, { gap })`: clamped so the opening stays in the wall; None when it can't fit.
pub fn fit_offset(wall_len: f64, width: f64, offset: f64, gap: f64) -> Option<f64> {
    let max = wall_len - width - gap;
    if max < gap - 1e-6 {
        return None;
    }
    Some(m::max(gap, m::min(max, offset)))
}

fn type_line(w: &mut W<'_>, ty: &OType) -> R<Option<Id>> {
    let (cls, pset) = if ty.door { ("IFCDOORTYPE", "CadCraft_DoorType") } else { ("IFCWINDOWTYPE", "CadCraft_WindowType") };
    for id in w.ids(cls) {
        let tl = w.must(id)?;
        for h in tl.a("HasPropertySets").hrefs() {
            let Some(ps) = w.line(h)? else { continue };
            if ps.a("Name").val_str().as_deref() != Some(pset) {
                continue;
            }
            for p in ps.a("HasProperties").hrefs() {
                if let Some(pl) = w.line(p)?
                    && pl.a("Name").val_str().as_deref() == Some("Key")
                    && pl.a("NominalValue").val_str().as_deref() == Some(ty.key)
                {
                    return Ok(Some(id));
                }
            }
        }
    }
    let ps = w.pset(pset, &[row("Key", "IFCIDENTIFIER", P::s(ty.key))])?;
    let name = w.l(&opening_type_name(ty));
    let g = w.guid();
    let oh = w.owner_history();
    let mut head = vec![g, oh, name.clone(), JV::Null, JV::Null, JV::L(vec![JV::H(ps)]), JV::Null, JV::Null, name];
    if !w.v4 {
        return Ok(None);
    }
    if ty.door {
        head.extend([JV::s("DOOR"), JV::s(ty.op[0]), JV::Bool(false), JV::Null]);
    } else {
        head.extend([JV::s("WINDOW"), JV::s(ty.part), JV::Bool(false), JV::Null]);
    }
    Ok(Some(w.make(cls, head)?))
}

/// What a new opening is (`placeOpening` spec).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NewOpening {
    pub wall: String,
    pub type_key: String,
    pub offset: Option<f64>,
    pub width: f64,
    pub height: f64,
    pub sill: Option<f64>,
    pub hand: String,
    pub face: String,
    pub guid: Option<String>,
    pub opening_guid: Option<String>,
}

/// The parameters an opening is built from.
#[derive(Clone, Debug, PartialEq)]
pub struct OpSpec {
    pub door: bool,
    pub type_key: String,
    pub width: f64,
    pub height: f64,
    pub sill: f64,
    pub hand: String,
    pub face: String,
    pub offset: f64,
}

/// `addOpening(w, spec)` → (id, guid, opening guid)
pub fn add_opening(w: &mut W<'_>, s: &NewOpening) -> R<(Id, String, String)> {
    let ty = opening_type(&s.type_key).ok_or_else(|| key_err("model.err.opentype"))?;
    let walls = read_walls(w, None)?;
    let host = walls.into_iter().find(|x| x.guid == s.wall).ok_or_else(|| key_err("model.err.nowall"))?;
    let wd = if s.width > 0.0 { s.width } else { ty.width };
    let ht = if s.height > 0.0 { s.height } else { ty.height };
    let sl = if ty.door { 0.0 } else { s.sill.filter(|v| v.is_finite()).unwrap_or(ty.sill) };
    let off = fit_offset(host.len, wd, s.offset.filter(|v| v.is_finite()).unwrap_or((host.len - wd) / 2.0), 0.0)
        .ok_or_else(|| key_err("model.err.nofit"))?;
    if sl + ht > host.height + 1.0 {
        return Err(key_err("model.err.tootall"));
    }
    let og = w.gid(s.opening_guid.as_deref());
    let olp = w.place_at(Some(host.lp), off, 0.0, sl - host.base_offset, 0.0)?;
    let oh = w.owner_history();
    let mut args = vec![w.gid_jv(&og), oh.clone(), w.l(&t0("model.opening")), JV::Null, JV::Null, JV::H(olp), JV::Null, JV::Null];
    if w.v4 {
        args.push(JV::s("OPENING"));
    }
    let opening = w.make("IFCOPENINGELEMENT", args)?;
    w.note_guid(&og, opening);
    let gg = w.guid();
    let oh = w.owner_history();
    w.make("IFCRELVOIDSELEMENT", vec![gg, oh, JV::Null, JV::Null, JV::H(host.id), JV::H(opening)])?;
    let g = w.gid(s.guid.as_deref());
    let lp = w.place_at(Some(olp), 0.0, 0.0, 0.0, 0.0)?;
    let name = w.l(&opening_type_name(&ty));
    let hand_b = s.hand == "b";
    let mut tail = vec![JV::Num(round6(w.u(ht))), JV::Num(round6(w.u(wd)))];
    if w.v4 {
        if ty.door {
            tail.extend([JV::s("DOOR"), JV::s(if hand_b { ty.op[1] } else { ty.op[0] }), JV::Null]);
        } else {
            tail.extend([JV::s("WINDOW"), JV::s(ty.part), JV::Null]);
        }
    }
    let oh = w.owner_history();
    let mark = next_mark(w, ty.door)?;
    let mut args = vec![w.gid_jv(&g), oh, name.clone(), JV::Null, name, JV::H(lp), JV::Null, w.t("IFCIDENTIFIER", P::s(&mark))];
    args.extend(tail);
    let id = w.make(if ty.door { "IFCDOOR" } else { "IFCWINDOW" }, args)?;
    w.note_guid(&g, id);
    let gg = w.guid();
    let oh = w.owner_history();
    w.make("IFCRELFILLSELEMENT", vec![gg, oh, JV::Null, JV::Null, JV::H(opening), JV::H(id)])?;
    if let Some(st) = host.storey {
        w.contain(id, st)?;
    } else {
        return Err(crate::api::JsErr("TypeError: storey".into()));
    }
    if let Some(tl) = type_line(w, &ty)? {
        w.relate_type(id, tl)?;
    }
    let op = OpSpec {
        door: ty.door,
        type_key: s.type_key.clone(),
        width: wd,
        height: ht,
        sill: sl,
        hand: if hand_b { "b".into() } else { "a".into() },
        face: if s.face == "right" { "right".into() } else { "left".into() },
        offset: off,
    };
    write_opening(w, id, opening, &op, &host)?;
    Ok((id, g, og))
}

/// The next free mark for a door ("D1" …) or a window ("W1" …).
pub fn next_mark(w: &mut W<'_>, door: bool) -> R<String> {
    let letter = if door { 'D' } else { 'W' };
    let mut hi = 0.0f64;
    for id in w.ids(if door { "IFCDOOR" } else { "IFCWINDOW" }) {
        let tag = w.line(id)?.map(|l| l.a("Tag").text_or_empty()).unwrap_or_default();
        let mut ch = tag.chars();
        if let Some(c) = ch.next()
            && c.is_ascii_uppercase()
        {
            let digits: &str = ch.as_str();
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && c == letter {
                hi = m::max(hi, crate::js::str_num(digits));
            }
        }
    }
    Ok(format!("{letter}{}", crate::js::num_str(hi + 1.0)))
}

struct Style {
    key: &'static str,
    rgb: [f64; 3],
    tr: f64,
}
fn style(k: &str) -> Style {
    match k {
        "leaf" => Style { key: "model.style.leaf", rgb: [0.72, 0.56, 0.38], tr: 0.0 },
        "glass" => Style { key: "model.style.glass", rgb: [0.62, 0.79, 0.89], tr: 0.6 },
        "garage" => Style { key: "model.style.garage", rgb: [0.86, 0.86, 0.84], tr: 0.0 },
        _ => Style { key: "model.style.frame", rgb: [0.93, 0.92, 0.89], tr: 0.0 },
    }
}

/// One box of a door or window body.
pub type PartBox = ([f64; 6], &'static str);

/// Shapes, psets and quantities of a door / window and its opening.
fn write_opening(w: &mut W<'_>, id: Id, opening: Id, op: &OpSpec, host: &Wall) -> R<()> {
    let ty = opening_type(&op.type_key).unwrap_or_else(|| if op.door { door_types().remove(0) } else { window_types().remove(0) });
    let t = host.t;
    let b = w.boxx(0.0, -t / 2.0 - PROUD, 0.0, op.width, t / 2.0 + PROUD, op.height)?;
    let r = w.rep("body", "Body", "SweptSolid", &[b])?;
    w.set_shape(opening, &[r])?;
    let parts = if op.door { door_parts(&ty, op, t) } else { window_parts(&ty, op, t) };
    let mut items = Vec::new();
    for (bx, st) in parts {
        let item = w.boxx(bx[0], bx[1], bx[2], bx[3], bx[4], bx[5])?;
        let s = style(st);
        let sid = w.surface_style(&t0(s.key), s.rgb, s.tr)?;
        w.style_item(item, sid)?;
        items.push(item);
    }
    let r = w.rep("body", "Body", "SweptSolid", &items)?;
    w.set_shape(id, &[r])?;
    let external = w.read_pset(host.id, "Pset_WallCommon")?.and_then(|p| p.get("IsExternal").cloned()).is_some_and(|v| v.truthy());
    let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
    w.upsert_pset(
        id,
        if op.door { DOOR_PSET } else { WINDOW_PSET },
        &[
            row("TypeKey", "IFCIDENTIFIER", P::s(&op.type_key)),
            row("Width", "IFCPOSITIVELENGTHMEASURE", u(w, op.width)),
            row("Height", "IFCPOSITIVELENGTHMEASURE", u(w, op.height)),
            row("Sill", "IFCLENGTHMEASURE", u(w, op.sill)),
            row("Hand", "IFCLABEL", P::s(&op.hand)),
            row("Face", "IFCLABEL", P::s(&op.face)),
        ],
    )?;
    w.upsert_pset(
        id,
        if op.door { "Pset_DoorCommon" } else { "Pset_WindowCommon" },
        &[row("Reference", "IFCIDENTIFIER", P::s(&opening_type_name(&ty))), row("IsExternal", "IFCBOOLEAN", P::Bool(external))],
    )?;
    let q: Vec<QRow> =
        vec![("Width".into(), "length", op.width), ("Height".into(), "length", op.height), ("Area".into(), "area", op.width * op.height)];
    w.upsert_qto(id, if op.door { "Qto_DoorBaseQuantities" } else { "Qto_WindowBaseQuantities" }, &q)?;
    w.upsert_pset(
        opening,
        OPENING_PSET,
        &[row("Width", "IFCPOSITIVELENGTHMEASURE", u(w, op.width)), row("Height", "IFCPOSITIVELENGTHMEASURE", u(w, op.height))],
    )?;
    let mut l = w.touch(id)?;
    l.set("OverallWidth", JV::Num(round6(w.u(op.width))));
    l.set("OverallHeight", JV::Num(round6(w.u(op.height))));
    if op.door && w.v4 {
        l.set("OperationType", JV::s(if op.hand == "b" { ty.op[1] } else { ty.op[0] }));
    }
    w.write(&mut l)?;
    Ok(())
}

/// A door's boxes in the opening's axes (x along, y across the wall, z up).
pub fn door_parts(ty: &OType, op: &OpSpec, t: f64) -> Vec<PartBox> {
    let (wd, ht) = (op.width, op.height);
    let f = m::min(FRAME, wd / 6.0);
    let mut out: Vec<PartBox> = Vec::new();
    let mut bx = |x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, st: &'static str| out.push(([x0, y0, z0, x1, y1, z1], st));
    let (y0, y1) = (-t / 2.0, t / 2.0);
    bx(0.0, y0, 0.0, f, y1, ht, "frame");
    bx(wd - f, y0, 0.0, wd, y1, ht, "frame");
    bx(f, y0, ht - f, wd - f, y1, ht, "frame");
    let s = if op.face == "right" { -1.0 } else { 1.0 };
    let ly0 = if s > 0.0 { y1 - LEAF } else { y0 };
    let ly1 = if s > 0.0 { y1 } else { y0 + LEAF };
    let inner = wd - 2.0 * f;
    let top = ht - f;
    if ty.garage {
        bx(f, -LEAF / 2.0, 0.0, wd - f, LEAF / 2.0, top, "garage");
    } else if ty.slide {
        let half = inner / 2.0 + 25.0;
        bx(f, -LEAF, 0.0, f + half, 0.0, top, "glass");
        bx(wd - f - half, 0.0, 0.0, wd - f, LEAF, top, "glass");
        bx(f, -LEAF, 0.0, f + 40.0, 0.0, top, "leaf");
        bx(wd - f - 40.0, 0.0, 0.0, wd - f, LEAF, top, "leaf");
    } else if ty.pocket || ty.bifold {
        bx(f, -LEAF / 2.0, 0.0, wd - f, LEAF / 2.0, top, "leaf");
    } else if ty.barn {
        let yb = if s > 0.0 { y1 } else { y0 - LEAF };
        let yt = if s > 0.0 { y1 + LEAF } else { y0 };
        bx(-50.0, yb, 0.0, wd + 50.0, yt, ht + 25.0, "leaf");
        bx(-50.0, if s > 0.0 { yt } else { yb - 25.0 }, ht + 40.0, 2.0 * wd, if s > 0.0 { yt + 25.0 } else { yb }, ht + 80.0, "frame");
    } else if ty.sidelites > 0.0 {
        let sl = m::min(ty.sidelites, wd / 4.0);
        bx(f, -GLASS / 2.0, 0.0, f + sl, GLASS / 2.0, top, "glass");
        bx(wd - f - sl, -GLASS / 2.0, 0.0, wd - f, GLASS / 2.0, top, "glass");
        bx(f + sl, y0, 0.0, f + sl + f, y1, top, "frame");
        bx(wd - f - sl - f, y0, 0.0, wd - f - sl, y1, top, "frame");
        bx(f + sl + f, ly0, 0.0, wd - f - sl - f, ly1, top - 3.0, "leaf");
    } else if ty.leaves == 2 {
        if ty.glazed {
            for (a, b) in [(f, f + inner / 2.0 - 2.0), (f + inner / 2.0 + 2.0, wd - f)] {
                let st = 100.0;
                bx(a, ly0, 0.0, a + st, ly1, top - 3.0, "leaf");
                bx(b - st, ly0, 0.0, b, ly1, top - 3.0, "leaf");
                bx(a + st, ly0, 0.0, b - st, ly1, 200.0, "leaf");
                bx(a + st, ly0, top - 3.0 - st, b - st, ly1, top - 3.0, "leaf");
                bx(a + st, (ly0 + ly1) / 2.0 - GLASS / 2.0, 200.0, b - st, (ly0 + ly1) / 2.0 + GLASS / 2.0, top - 3.0 - st, "glass");
            }
        } else {
            bx(f, ly0, 0.0, f + inner / 2.0 - 2.0, ly1, top - 3.0, "leaf");
            bx(f + inner / 2.0 + 2.0, ly0, 0.0, wd - f, ly1, top - 3.0, "leaf");
        }
    } else {
        bx(f, ly0, 0.0, wd - f, ly1, top - 3.0, "leaf");
    }
    out
}

/// A window's boxes: frame, sashes' rails and stiles, glass.
pub fn window_parts(ty: &OType, op: &OpSpec, t: f64) -> Vec<PartBox> {
    let (wd, ht) = (op.width, op.height);
    let f = m::min(m::min(WFRAME, wd / 6.0), ht / 6.0);
    let mut out: Vec<PartBox> = Vec::new();
    let mut bx = |x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, st: &'static str| out.push(([x0, y0, z0, x1, y1, z1], st));
    let d = m::min(WDEPTH, t);
    let s = if op.face == "right" { -1.0 } else { 1.0 };
    let y0 = if s > 0.0 { t / 2.0 - d } else { -t / 2.0 };
    let y1 = y0 + d;
    bx(0.0, y0, 0.0, f, y1, ht, "frame");
    bx(wd - f, y0, 0.0, wd, y1, ht, "frame");
    bx(f, y0, 0.0, wd - f, y1, f, "frame");
    bx(f, y0, ht - f, wd - f, y1, ht, "frame");
    let gy = (y0 + y1) / 2.0;
    if ty.rail {
        let mm = ht / 2.0;
        bx(f, y0 + d * 0.15, mm - f / 2.0, wd - f, y1 - d * 0.15, mm + f / 2.0, "frame");
    }
    if ty.stile {
        let mm = wd / 2.0;
        bx(mm - f / 2.0, y0 + d * 0.15, f, mm + f / 2.0, y1 - d * 0.15, ht - f, "frame");
    }
    bx(f, gy - GLASS / 2.0, f, wd - f, gy + GLASS / 2.0, ht - f, "glass");
    out
}

/// What an opening change asks for (`updateOpening` changes).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OpeningChange {
    pub offset: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub sill: Option<f64>,
    pub type_key: Option<String>,
    pub hand: Option<String>,
    pub face: Option<String>,
    /// another wall's GlobalId
    pub wall: Option<String>,
}

/// `updateOpening(w, guid, changes)` → the host walls (GlobalIds).
pub fn update_opening(w: &mut W<'_>, guid: &str, ch: &OpeningChange) -> R<Vec<String>> {
    let walls = read_walls(w, None)?;
    let all = read_openings(w, None, &walls)?;
    let cur = all.into_iter().find(|x| x.guid == guid).ok_or_else(|| key_err("model.err.noopening"))?;
    let mut host = walls.iter().find(|x| x.id == cur.wall).cloned().ok_or_else(|| crate::api::JsErr("TypeError: host".into()))?;
    let mut touched = vec![host.guid.clone()];
    let mut op = OpSpec {
        door: cur.door,
        type_key: cur.type_key.clone(),
        width: cur.width,
        height: cur.height,
        sill: cur.sill,
        hand: cur.hand.clone(),
        face: cur.face.clone(),
        offset: cur.offset,
    };
    if let Some(v) = ch.width.filter(|v| v.is_finite() && *v >= 0.0) {
        op.width = v;
    }
    if let Some(v) = ch.height.filter(|v| v.is_finite() && *v >= 0.0) {
        op.height = v;
    }
    if let Some(v) = ch.sill.filter(|v| v.is_finite() && *v >= 0.0) {
        op.sill = v;
    }
    if op.door {
        op.sill = 0.0;
    }
    if let Some(tk) = ch.type_key.as_ref().filter(|k| !k.is_empty())
        && opening_type(tk).is_some_and(|t| t.door == op.door)
    {
        op.type_key = tk.clone();
    }
    if let Some(h) = ch.hand.as_ref().filter(|h| *h == "a" || *h == "b") {
        op.hand = h.clone();
    }
    if let Some(f) = ch.face.as_ref().filter(|f| *f == "left" || *f == "right") {
        op.face = f.clone();
    }
    if !(op.width >= 100.0 && op.height >= 100.0) {
        return Err(key_err("model.err.small"));
    }
    if let Some(to_g) = ch.wall.as_ref().filter(|g| !g.is_empty() && **g != host.guid) {
        let to = walls.iter().find(|x| x.guid == *to_g).cloned().ok_or_else(|| key_err("model.err.nowall"))?;
        for rid in w.ids("IFCRELVOIDSELEMENT") {
            let r = w.must(rid)?;
            if r.a("RelatedOpeningElement").href() != Some(cur.opening_id) {
                continue;
            }
            let mut l = w.touch(rid)?;
            l.set("RelatingBuildingElement", JV::H(to.id));
            w.write(&mut l)?;
        }
        let olp =
            w.must(cur.opening_id)?.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
        let mut pl = w.touch(olp)?;
        pl.set("PlacementRelTo", JV::H(to.lp));
        w.write(&mut pl)?;
        if to.storey != host.storey
            && let Some(st) = to.storey
        {
            move_containment(w, cur.id, st)?;
        }
        touched.push(to.guid.clone());
        host = to;
    }
    let off = fit_offset(host.len, op.width, ch.offset.filter(|v| v.is_finite()).unwrap_or(op.offset), 0.0)
        .ok_or_else(|| key_err("model.err.nofit"))?;
    if op.sill + op.height > host.height + 1.0 {
        return Err(key_err("model.err.tootall"));
    }
    op.offset = off;
    let olp = w.must(cur.opening_id)?.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    w.move_placement(olp, off, 0.0, op.sill - host.base_offset, 0.0)?;
    if op.type_key != cur.type_key {
        let ty = opening_type(&op.type_key).ok_or_else(|| key_err("model.err.opentype"))?;
        let tl = type_line(w, &ty)?;
        for rid in w.ids("IFCRELDEFINESBYTYPE") {
            let r = w.must(rid)?;
            if !r.a("RelatedObjects").hrefs().contains(&cur.id) {
                continue;
            }
            let rest: Vec<JV> = match r.a("RelatedObjects") {
                JV::L(l) => l.iter().filter(|h| h.href() != Some(cur.id)).cloned().collect(),
                _ => Vec::new(),
            };
            let mut l = w.touch(rid)?;
            if !rest.is_empty() {
                l.set("RelatedObjects", JV::L(rest));
                w.write(&mut l)?;
            } else {
                drop_rel(w, rid)?;
            }
        }
        if let Some(tl) = tl {
            w.relate_type(cur.id, tl)?;
        }
        let nm = w.l(&opening_type_name(&ty));
        let mut l = w.touch(cur.id)?;
        l.set("Name", nm.clone());
        l.set("ObjectType", nm);
        w.write(&mut l)?;
    }
    write_opening(w, cur.id, cur.opening_id, &op, &host)?;
    Ok(touched)
}

fn move_containment(w: &mut W<'_>, id: Id, storey: Id) -> R<()> {
    for rid in w.ids("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
        let r = w.must(rid)?;
        if !r.a("RelatedElements").hrefs().contains(&id) {
            continue;
        }
        let rest: Vec<JV> = match r.a("RelatedElements") {
            JV::L(l) => l.iter().filter(|h| h.href() != Some(id)).cloned().collect(),
            _ => Vec::new(),
        };
        let mut l = w.touch(rid)?;
        if !rest.is_empty() {
            l.set("RelatedElements", JV::L(rest));
            w.write(&mut l)?;
        } else {
            drop_rel(w, rid)?;
        }
    }
    w.contain(id, storey)?;
    Ok(())
}

fn drop_rel(w: &mut W<'_>, rid: Id) -> R<()> {
    w.touch(rid)?;
    w.api.delete(rid);
    Ok(())
}

/// `deleteOpenings(w, guids)` → the host walls (GlobalIds).
pub fn delete_openings(w: &mut W<'_>, guids: &[String]) -> R<Vec<String>> {
    let walls = read_walls(w, None)?;
    let all = read_openings(w, None, &walls)?;
    let mut list = Vec::new();
    let mut hosts: Vec<String> = Vec::new();
    for g in guids {
        let Some(o) = all.iter().find(|x| x.guid == *g) else { continue };
        list.push(o.id);
        list.push(o.opening_id);
        if !hosts.contains(&o.wall_guid) {
            hosts.push(o.wall_guid.clone());
        }
    }
    if !list.is_empty() {
        w.drop_product(&list)?;
    }
    Ok(hosts)
}

// ------------------------------------------------------------------------------------------------------- plan symbols

/// A circular arc of a symbol: centre, radius, from a0 to a1 (radians).
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub c: V2,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
}

/// The plan symbol of a door or window: lines (heavy), arcs and thin lines (swing), dashed, poly (what a click picks).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Symbol {
    pub lines: Vec<[f64; 4]>,
    pub arcs: Vec<Arc>,
    pub thin: Vec<[f64; 4]>,
    pub dashed: Vec<[f64; 4]>,
    pub poly: Vec<V2>,
    pub fill: Vec<Vec<V2>>,
}

/// `openingSymbol(op, wall)` (plan mm).
pub fn opening_symbol(op: &OpSpec, wa: V2, wb: V2, wt: f64) -> Symbol {
    let ty = opening_type(&op.type_key).unwrap_or_else(|| if op.door { door_types().remove(0) } else { window_types().remove(0) });
    let l0 = m::hypot(wb[0] - wa[0], wb[1] - wa[1]);
    let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
    let u = [(wb[0] - wa[0]) / l, (wb[1] - wa[1]) / l];
    let n = [-u[1], u[0]];
    let pp = |x: f64, y: f64| -> V2 { [wa[0] + u[0] * x + n[0] * y, wa[1] + u[1] * x + n[1] * y] };
    let seg = |p: V2, q: V2| -> [f64; 4] { [p[0], p[1], q[0], q[1]] };
    let t = wt;
    let s = if op.face == "right" { -1.0 } else { 1.0 };
    let (x0, x1, wd) = (op.offset, op.offset + op.width, op.width);
    let mut out = Symbol { poly: vec![pp(x0, -t / 2.0), pp(x1, -t / 2.0), pp(x1, t / 2.0), pp(x0, t / 2.0)], ..Symbol::default() };
    out.lines.push(seg(pp(x0, -t / 2.0), pp(x0, t / 2.0)));
    out.lines.push(seg(pp(x1, -t / 2.0), pp(x1, t / 2.0)));
    let ang = |v: V2| m::atan2(v[1], v[0]);
    let short_way = |a0: f64, a1: f64| -> f64 {
        let pi = std::f64::consts::PI;
        let mut d = a1 - a0;
        let mut guard = 0u32;
        while d > pi && guard < 64 {
            d -= 2.0 * pi;
            guard = guard.saturating_add(1);
        }
        while d < -pi && guard < 128 {
            d += 2.0 * pi;
            guard = guard.saturating_add(1);
        }
        d
    };
    if op.door {
        let f = m::min(FRAME, wd / 6.0);
        let face = s * t / 2.0;
        if ty.garage {
            out.dashed.push(seg(pp(x0, 0.0), pp(x1, 0.0)));
            out.thin.push(seg(pp(x0 + f, face), pp(x1 - f, face)));
        } else if ty.slide {
            let half = (wd - 2.0 * f) / 2.0 + 25.0;
            out.lines.push(seg(pp(x0 + f, -LEAF / 2.0), pp(x0 + f + half, -LEAF / 2.0)));
            out.lines.push(seg(pp(x1 - f - half, LEAF / 2.0), pp(x1 - f, LEAF / 2.0)));
            out.thin.push(seg(pp(x0 + f, -LEAF), pp(x0 + f + half, -LEAF)));
            out.thin.push(seg(pp(x1 - f - half, LEAF), pp(x1 - f, LEAF)));
        } else if ty.pocket {
            let into = if op.hand == "b" { [x1 - f, x1 + wd - 2.0 * f] } else { [x0 + f - (wd - 2.0 * f), x0 + f] };
            out.lines.push(seg(pp(x0 + f, 0.0), pp(x1 - f, 0.0)));
            out.dashed.push(seg(pp(into[0], 0.0), pp(into[1], 0.0)));
        } else if ty.bifold {
            let pw = (wd - 2.0 * f) / 4.0;
            let a = std::f64::consts::PI / 6.0;
            for (hx, dir) in [(x0 + f, 1.0), (x1 - f, -1.0)] {
                let p0 = pp(hx, face);
                let p1 = pp(hx + dir * pw * m::cos(a), face + s * pw * m::sin(a));
                let p2 = pp(hx + dir * 2.0 * pw * m::cos(a), face);
                out.lines.push(seg(p0, p1));
                out.lines.push(seg(p1, p2));
            }
            out.poly = vec![pp(x0, -s * t / 2.0), pp(x1, -s * t / 2.0), pp(x1, face + s * pw), pp(x0, face + s * pw)];
        } else if ty.barn {
            let yb = face + s * LEAF;
            out.lines.push(seg(pp(x0 - 50.0, yb), pp(x1 + 50.0, yb)));
            let into = if op.hand == "b" { [x0 - wd, x0 - 50.0] } else { [x1 + 50.0, x1 + wd] };
            out.dashed.push(seg(pp(into[0], yb), pp(into[1], yb)));
        } else if ty.sidelites > 0.0 {
            let sl = m::min(ty.sidelites, wd / 4.0);
            let d0 = x0 + f + sl + f;
            let d1 = x1 - f - sl - f;
            let r = d1 - d0;
            out.thin.push(seg(pp(x0 + f, 0.0), pp(x0 + f + sl, 0.0)));
            out.thin.push(seg(pp(x1 - f - sl, 0.0), pp(x1 - f, 0.0)));
            out.lines.push(seg(pp(x0 + f + sl, -t / 2.0), pp(x0 + f + sl, t / 2.0)));
            out.lines.push(seg(pp(x1 - f - sl, -t / 2.0), pp(x1 - f - sl, t / 2.0)));
            let hx = if op.hand == "b" { d1 } else { d0 };
            let dir = if op.hand == "b" { -1.0 } else { 1.0 };
            let h = pp(hx, face);
            let open = pp(hx, face + s * r);
            let shut = pp(hx + dir * r, face);
            out.lines.push(seg(h, open));
            let a0 = ang(sub(open, h));
            let a1 = ang(sub(shut, h));
            out.arcs.push(Arc { c: h, r, a0, a1: a0 + short_way(a0, a1) });
            out.poly = vec![pp(x0, -s * t / 2.0), pp(x1, -s * t / 2.0), pp(x1, face + s * r), pp(x0, face + s * r)];
        } else {
            let leaves: Vec<(f64, f64, f64)> = if ty.leaves == 2 {
                vec![(x0 + f, 1.0, (wd - 2.0 * f) / 2.0), (x1 - f, -1.0, (wd - 2.0 * f) / 2.0)]
            } else {
                vec![(if op.hand == "b" { x1 - f } else { x0 + f }, if op.hand == "b" { -1.0 } else { 1.0 }, wd - 2.0 * f)]
            };
            for (hx, dir, r) in leaves {
                let h = pp(hx, face);
                let open = pp(hx, face + s * r);
                let shut = pp(hx + dir * r, face);
                out.lines.push(seg(h, open));
                let a0 = ang(sub(open, h));
                let a1 = ang(sub(shut, h));
                out.arcs.push(Arc { c: h, r, a0, a1: a0 + short_way(a0, a1) });
            }
            let reach = if ty.leaves == 2 { (wd - 2.0 * f) / 2.0 } else { wd - 2.0 * f };
            let ext = pp(x0, face + s * reach);
            let ext2 = pp(x1, face + s * reach);
            out.poly = vec![pp(x0, -s * t / 2.0), pp(x1, -s * t / 2.0), ext2, ext];
        }
    } else {
        let d = m::min(WDEPTH, t);
        let yf = if s > 0.0 { t / 2.0 - d } else { -t / 2.0 };
        let yb = yf + d;
        let gy = (yf + yb) / 2.0;
        out.lines.push(seg(pp(x0, yf), pp(x1, yf)));
        out.lines.push(seg(pp(x0, yb), pp(x1, yb)));
        if ty.stile {
            out.thin.push(seg(pp(x0, gy - 8.0), pp(x0 + wd / 2.0 + 10.0, gy - 8.0)));
            out.thin.push(seg(pp(x1 - wd / 2.0 - 10.0, gy + 8.0), pp(x1, gy + 8.0)));
        } else if ty.rail {
            out.thin.push(seg(pp(x0, gy - 8.0), pp(x1, gy - 8.0)));
            out.thin.push(seg(pp(x0, gy + 8.0), pp(x1, gy + 8.0)));
        } else {
            out.thin.push(seg(pp(x0, gy), pp(x1, gy)));
        }
        if ty.casement {
            let hx = if op.hand == "b" { x1 } else { x0 };
            let dir = if op.hand == "b" { -1.0 } else { 1.0 };
            let r = wd;
            let face = s * t / 2.0;
            let pi6 = std::f64::consts::PI / 6.0;
            let h = pp(hx, face);
            let open = pp(hx + dir * r * m::cos(pi6), face + s * r * m::sin(pi6));
            out.thin.push(seg(h, open));
        }
    }
    out
}
