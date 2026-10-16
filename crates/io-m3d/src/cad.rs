//! DXF and DWG export (0.4 M2): io.js `exportDXF` (DXF R12 / AC1009 text), `toDwgModel` (the same scene as a cc-dwg
//! model, as JSON, for cc-io-dwg's writer), `nearestAci`, `insertOf`.
//!
//! One neutral scene for both: every face as 3DFACEs (its triangles; edges inside a face and soft edges invisible),
//! loose edges as LINEs, every group and component as a block (a component once, placed as often as it is used)
//! placed by an INSERT with its extrusion direction. A placement that is not a turn + scale is written out in place.
//! Coordinates are in the model's display unit: inches for ft-in and in, else mm / cm / m.

use std::collections::HashSet;
use std::sync::OnceLock;

use cc_m3d::Model;
use cc_m3d::def::{Def, edge_faces, face_tris, key};
use cc_m3d::js;
use cc_m3d::omap::FxMap;
use cc_m3d::vec::{IDENT, M4, P3, apply, cross, det3, dot, len, mmul, mul, norm};
use serde_json::{Map, Value, json};

use crate::Res;
use crate::i18n::err;
use crate::jsx::{atan2, pow};
use crate::num::precision_num;
use crate::obj::parse_hex;
use crate::util::{face_paint, or_paint, paint_hex};

/// Most definitions an export may nest (the JS stack overflows somewhere past this).
const MAX_DEPTH: u32 = 5000;

/// `CAD_UNITS[units]`: name, factor, $INSUNITS.
fn cad_units(u: &str) -> (&'static str, f64, f64) {
    match u {
        "ft-in" | "in" => ("inches", 1.0 / 0.0254, 1.0),
        "mm" => ("millimeters", 1000.0, 4.0),
        "cm" => ("centimeters", 100.0, 5.0),
        _ => ("meters", 1.0, 6.0),
    }
}

/// The AutoCAD colour index palette (the DXF reference's: 1–9 named, 10–249 hue × value × pastel, 250–255 greys).
fn aci() -> &'static [[f64; 3]; 256] {
    static T: OnceLock<[[f64; 3]; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [[f64::NAN; 3]; 256];
        let named = [
            [255.0, 0.0, 0.0],
            [255.0, 255.0, 0.0],
            [0.0, 255.0, 0.0],
            [0.0, 255.0, 255.0],
            [0.0, 0.0, 255.0],
            [255.0, 0.0, 255.0],
            [255.0, 255.0, 255.0],
            [128.0, 128.0, 128.0],
            [192.0, 192.0, 192.0],
        ];
        for (i, c) in named.iter().enumerate() {
            if let Some(s) = t.get_mut(i.saturating_add(1)) {
                *s = *c;
            }
        }
        let vv = [255.0, 204.0, 153.0, 127.0, 76.0];
        for i in 10usize..250 {
            let k = i.saturating_sub(10);
            let hue = (k / 10) as f64 * 15.0;
            let kk = k % 10;
            let v = vv.get(kk >> 1).copied().unwrap_or(0.0);
            let pastel = kk & 1 == 1;
            let h6 = hue / 60.0;
            let f = h6 - h6.floor();
            let lo = if pastel { v / 2.0 } else { 0.0 };
            let q = v - (v - lo) * f;
            let u = lo + (v - lo) * f;
            let opts = [[v, u, lo], [q, v, lo], [lo, v, u], [lo, q, v], [u, lo, v], [v, lo, q]];
            let sel = crate::num::index(h6.floor()).unwrap_or(0) % 6;
            let c = opts.get(sel).copied().unwrap_or([0.0; 3]);
            if let Some(s) = t.get_mut(i) {
                *s = [js::round(c[0]), js::round(c[1]), js::round(c[2])];
            }
        }
        for (i, g) in [51.0, 91.0, 132.0, 173.0, 214.0, 255.0].iter().enumerate() {
            if let Some(s) = t.get_mut(250usize.saturating_add(i)) {
                *s = [*g, *g, *g];
            }
        }
        t
    })
}

/// `/^#?([0-9a-f]{6})$/i.exec(String(paintHex(hex) || ''))` → the six hex digits.
fn hex6(c: Option<&str>) -> Option<String> {
    let h = c.map(paint_hex).unwrap_or_default();
    let s = h.strip_prefix('#').unwrap_or(&h);
    (s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_string())
}

/// `nearestAci(hex)`: the AutoCAD colour index nearest to a colour (7 when it is not one).
pub fn nearest_aci(c: Option<&str>) -> f64 {
    let Some(h) = hex6(c) else { return 7.0 };
    let comp = |a: usize| parse_hex(h.get(a..a.saturating_add(2)).unwrap_or(""));
    let (r, g, b) = (comp(0), comp(2), comp(4));
    let t = aci();
    let mut best = 7.0;
    let mut bd = f64::INFINITY;
    for i in 1usize..256 {
        let Some(c) = t.get(i) else { continue };
        let d = pow(c[0] - r, 2.0) + pow(c[1] - g, 2.0) + pow(c[2] - b, 2.0);
        if d < bd {
            bd = d;
            best = i as f64;
        }
    }
    best
}

/// A placement as an INSERT: insertion point (OCS), scale, rotation (radians), extrusion.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertOf {
    pub ins: P3,
    pub scale: P3,
    pub rot: f64,
    pub n: P3,
}

/// `insertOf(m, k)`: None when the placement is not a turn + scale.
pub fn insert_of(m: &M4, k: f64) -> Option<InsertOf> {
    let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
    let mut s = [len(c[0]), len(c[1]), len(c[2])];
    if s.iter().any(|x| !(*x > 1e-12)) {
        return None;
    }
    let mut r = [mul(c[0], 1.0 / s[0]), mul(c[1], 1.0 / s[1]), mul(c[2], 1.0 / s[2])];
    if dot(r[0], r[1]).abs() > 1e-9 || dot(r[0], r[2]).abs() > 1e-9 || dot(r[1], r[2]).abs() > 1e-9 {
        return None;
    }
    if dot(cross(r[0], r[1]), r[2]) < 0.0 {
        s[2] = -s[2];
        r[2] = mul(r[2], -1.0);
    }
    let n = r[2];
    // the arbitrary axis algorithm (DXF reference): the OCS of this extrusion direction
    let ax = norm(if n[0].abs() < 1.0 / 64.0 && n[1].abs() < 1.0 / 64.0 { cross([0.0, 1.0, 0.0], n) } else { cross([0.0, 0.0, 1.0], n) });
    let ay = cross(n, ax);
    let t = [m[12] * k, m[13] * k, m[14] * k];
    Some(InsertOf { ins: [dot(t, ax), dot(t, ay), dot(t, n)], scale: s, rot: atan2(dot(r[0], ay), dot(r[0], ax)), n })
}

/// One entity of the scene.
#[derive(Clone, Debug)]
pub enum CadEnt {
    Face { pts: [P3; 4], inv: u32, color: Option<String> },
    Line { a: P3, b: P3 },
    Insert { block: String, ins: InsertOf },
}

/// What `exportDXF` / `toDwgModel` report.
#[derive(Clone, Debug, Default)]
pub struct CadReport {
    pub faces: f64,
    pub triangles: f64,
    pub lines: f64,
    pub blocks: f64,
    pub inserts: f64,
    pub flattened: f64,
    pub unit: String,
}

impl CadReport {
    pub fn to_value(&self) -> Value {
        json!({ "faces": self.faces, "triangles": self.triangles, "lines": self.lines, "blocks": self.blocks,
            "inserts": self.inserts, "flattened": self.flattened, "unit": self.unit })
    }
}

/// The neutral scene: blocks (in the order they finish), model-space entities, report.
pub struct Scene {
    pub unit_name: &'static str,
    pub insunits: f64,
    pub blocks: Vec<(String, Vec<CadEnt>)>,
    pub ents: Vec<CadEnt>,
    pub report: CadReport,
}

struct Builder<'m> {
    model: &'m Model,
    k: f64,
    report: CadReport,
    names: FxMap<String, String>,
    used: HashSet<String>,
    blocks: Vec<(String, Vec<CadEnt>)>,
    done: HashSet<String>,
}

impl Builder<'_> {
    fn too_big(&self) -> bool {
        self.report.triangles + self.report.lines + self.report.inserts > 2e6
    }
    /// `blockName(def)`.
    fn block_name(&mut self, id: &str, name: &str) -> String {
        if let Some(n) = self.names.get(id) {
            return n.clone();
        }
        let src = if name.is_empty() { id } else { name };
        let up = src.to_uppercase();
        let mut cleaned = String::with_capacity(up.len());
        let mut run = false;
        for c in up.chars() {
            if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '$' || c == '-' {
                cleaned.push(c);
                run = false;
            } else {
                if !run {
                    cleaned.push('_');
                }
                run = true;
            }
        }
        let trimmed = cleaned.trim_matches('_');
        let base = js::slice16(if trimmed.is_empty() { "BLOCK" } else { trimmed }, 26);
        let mut n = base.clone();
        let mut i = 2u64;
        while self.used.contains(&n) {
            n = format!("{}_{i}", js::slice16(&base, 26));
            i = i.saturating_add(1);
        }
        self.used.insert(n.clone());
        self.names.insert(id.to_string(), n.clone());
        n
    }
    /// `entsOf(def, m, pc)`: the entities of one definition in its own coordinates.
    fn ents_of(&mut self, def: &Def, m: &M4, pc: Option<&str>, depth: u32) -> Res<Vec<CadEnt>> {
        if depth > MAX_DEPTH {
            return Err(cc_m3d::Error::Msg("$RangeError".into()));
        }
        let k = self.k;
        let mut out = Vec::new();
        let p = |v: f64| -> P3 {
            let q = apply(m, def.v(v));
            [q[0] * k, q[1] * k, q[2] * k]
        };
        let ef = edge_faces(def);
        let soft: HashSet<cc_m3d::def::EdgeKey> = def.edges.values().filter(|e| e.soft).map(|e| key(e.a, e.b)).collect();
        let mirror = det3(m) < 0.0;
        for f in def.faces.values() {
            self.report.faces += 1.0;
            let mut real: HashSet<cc_m3d::def::EdgeKey> = HashSet::new();
            for l in &f.loops {
                let n = l.len();
                for (i, &a) in l.iter().enumerate() {
                    real.insert(key(a, l.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).copied().unwrap_or(a)));
                }
            }
            let color = face_paint(f.color.as_deref(), pc);
            for mut t in face_tris(def, f) {
                if mirror {
                    t = [t[0], t[2], t[1]];
                }
                let vis = |a: f64, b: f64| real.contains(&key(a, b)) && !soft.contains(&key(a, b));
                let inv = u32::from(!vis(t[0], t[1])) | (u32::from(!vis(t[1], t[2])) << 1) | 4 | (u32::from(!vis(t[2], t[0])) << 3);
                out.push(CadEnt::Face { pts: [p(t[0]), p(t[1]), p(t[2]), p(t[2])], inv, color: color.clone() });
                self.report.triangles += 1.0;
            }
            if self.too_big() {
                return Err(err("m3d.io.this_model_is_too_big_to"));
            }
        }
        for e in def.edges.values() {
            if ef.has(&key(e.a, e.b)) {
                continue;
            }
            out.push(CadEnt::Line { a: p(e.a), b: p(e.b) });
            self.report.lines += 1.0;
        }
        let insts: Vec<cc_m3d::def::Inst> = def.insts.values().cloned().collect();
        for inst in insts {
            let model = self.model;
            let Some(child) = model.def(&inst.def) else { continue };
            let wm = mmul(m, &inst.m);
            let cc = or_paint(inst.color.as_deref(), pc).map(str::to_string);
            match insert_of(&wm, k) {
                Some(ins) => {
                    let block = self.block_of(child, cc.as_deref(), depth)?;
                    out.push(CadEnt::Insert { block, ins });
                    self.report.inserts += 1.0;
                }
                None => {
                    self.report.flattened += 1.0;
                    let more = self.ents_of(child, &wm, cc.as_deref(), depth.saturating_add(1))?;
                    out.extend(more);
                }
            }
            if self.too_big() {
                return Err(err("m3d.io.this_model_is_too_big_to"));
            }
        }
        Ok(out)
    }
    /// `blockOf(def, pc)`: the block a definition (in a placement colour) becomes, made the first time.
    fn block_of(&mut self, def: &Def, pc: Option<&str>, depth: u32) -> Res<String> {
        let key_s = format!("{}|{}", def.id, pc.unwrap_or(""));
        let nm = match pc {
            Some(c) => {
                let mut it = c.chars();
                it.next();
                format!("{} {}", def.name, it.as_str())
            }
            None => def.name.clone(),
        };
        let name = self.block_name(&key_s, &nm);
        if !self.done.contains(&key_s) {
            self.done.insert(key_s);
            self.report.blocks += 1.0;
            let ents = self.ents_of(def, &IDENT, pc, depth.saturating_add(1))?;
            self.blocks.push((name.clone(), ents));
        }
        Ok(name)
    }
}

/// `cadScene(model)`.
pub fn cad_scene(model: &Model) -> Res<Scene> {
    let (unit_name, k, insunits) = cad_units(&model.units);
    let mut b = Builder {
        model,
        k,
        report: CadReport { unit: unit_name.to_string(), ..CadReport::default() },
        names: FxMap::default(),
        used: HashSet::new(),
        blocks: Vec::new(),
        done: HashSet::new(),
    };
    let Some(root) = model.root() else { return Err(cc_m3d::Error::Type) };
    let ents = b.ents_of(root, &IDENT, None, 0)?;
    if b.too_big() {
        return Err(err("m3d.io.this_model_is_too_big_to"));
    }
    Ok(Scene { unit_name, insunits, blocks: b.blocks, ents, report: b.report })
}

/// `df(x)`: 15 significant digits (−0 and |x| < 1e-12 → 0).
pub fn df(x: f64) -> String {
    let mut v = precision_num(x, 15);
    if v == 0.0 || v.abs() < 1e-12 {
        v = 0.0;
    }
    js::num_str(v)
}

/// `exportDXF(model)` → {text, report}: DXF R12 (AC1009), CRLF.
pub fn export_dxf(model: &Model) -> Res<(String, CadReport)> {
    let sc = cad_scene(model)?;
    let mut s = String::new();
    let mut w = |c: &str, v: &str| {
        s.push_str(c);
        s.push_str("\r\n");
        s.push_str(v);
        s.push_str("\r\n");
    };
    let ent = |e: &CadEnt, w: &mut dyn FnMut(&str, &str)| match e {
        CadEnt::Face { pts, inv, color } => {
            w("0", "3DFACE");
            w("8", "0");
            if let Some(c) = color {
                w("62", &js::num_str(nearest_aci(Some(c))));
            }
            for (codes, p) in [("10", "20", "30"), ("11", "21", "31"), ("12", "22", "32"), ("13", "23", "33")].iter().zip(pts.iter()) {
                w(codes.0, &df(p[0]));
                w(codes.1, &df(p[1]));
                w(codes.2, &df(p[2]));
            }
            w("70", &inv.to_string());
        }
        CadEnt::Line { a, b } => {
            w("0", "LINE");
            w("8", "0");
            w("10", &df(a[0]));
            w("20", &df(a[1]));
            w("30", &df(a[2]));
            w("11", &df(b[0]));
            w("21", &df(b[1]));
            w("31", &df(b[2]));
        }
        CadEnt::Insert { block, ins } => {
            w("0", "INSERT");
            w("8", "0");
            w("2", block);
            w("10", &df(ins.ins[0]));
            w("20", &df(ins.ins[1]));
            w("30", &df(ins.ins[2]));
            w("41", &df(ins.scale[0]));
            w("42", &df(ins.scale[1]));
            w("43", &df(ins.scale[2]));
            w("50", &df((ins.rot * 180.0) / std::f64::consts::PI));
            w("210", &df(ins.n[0]));
            w("220", &df(ins.n[1]));
            w("230", &df(ins.n[2]));
        }
    };
    for (c, v) in [
        ("0", "SECTION"),
        ("2", "HEADER"),
        ("9", "$ACADVER"),
        ("1", "AC1009"),
        ("9", "$INSBASE"),
        ("10", "0"),
        ("20", "0"),
        ("30", "0"),
        ("0", "ENDSEC"),
        ("0", "SECTION"),
        ("2", "TABLES"),
        ("0", "TABLE"),
        ("2", "LTYPE"),
        ("70", "1"),
        ("0", "LTYPE"),
        ("2", "CONTINUOUS"),
        ("70", "0"),
        ("3", "Solid line"),
        ("72", "65"),
        ("73", "0"),
        ("40", "0"),
        ("0", "ENDTAB"),
        ("0", "TABLE"),
        ("2", "LAYER"),
        ("70", "1"),
        ("0", "LAYER"),
        ("2", "0"),
        ("70", "0"),
        ("62", "7"),
        ("6", "CONTINUOUS"),
        ("0", "ENDTAB"),
        ("0", "ENDSEC"),
        ("0", "SECTION"),
        ("2", "BLOCKS"),
    ] {
        w(c, v);
    }
    for (name, ents) in &sc.blocks {
        w("0", "BLOCK");
        w("8", "0");
        w("2", name);
        w("70", "0");
        w("10", "0");
        w("20", "0");
        w("30", "0");
        w("3", name);
        for e in ents {
            ent(e, &mut w);
        }
        w("0", "ENDBLK");
        w("8", "0");
    }
    w("0", "ENDSEC");
    w("0", "SECTION");
    w("2", "ENTITIES");
    for e in &sc.ents {
        ent(e, &mut w);
    }
    w("0", "ENDSEC");
    w("0", "EOF");
    Ok((s, sc.report))
}

fn pt(p: &P3) -> Value {
    obj(vec![("x", cc_m3d::file::jn(p[0])), ("y", cc_m3d::file::jn(p[1])), ("z", cc_m3d::file::jn(p[2]))])
}

/// An object from (key, value) pairs in order, the values moved in (`json!` would serialize each `Value` again).
fn obj(kv: Vec<(&str, Value)>) -> Value {
    let mut m = Map::with_capacity(kv.len());
    for (k, v) in kv {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn by_layer() -> Value {
    obj(vec![("index", Value::from(256))])
}

fn conv(e: &CadEnt) -> Value {
    match e {
        CadEnt::Face { pts, inv, color } => {
            let col = match hex6(color.as_deref()) {
                Some(h) => obj(vec![("rgb", cc_m3d::file::jn(parse_hex(&h))), ("index", cc_m3d::file::jn(nearest_aci(color.as_deref())))]),
                None => by_layer(),
            };
            obj(vec![
                ("type", s("3DFACE")),
                ("layer", s("0")),
                ("color", col),
                ("points", Value::Array(pts.iter().map(pt).collect())),
                ("invisibleEdges", Value::from(*inv)),
            ])
        }
        CadEnt::Line { a, b } => obj(vec![("type", s("LINE")), ("layer", s("0")), ("color", by_layer()), ("start", pt(a)), ("end", pt(b))]),
        CadEnt::Insert { block, ins } => obj(vec![
            ("type", s("INSERT")),
            ("layer", s("0")),
            ("color", by_layer()),
            ("block", s(block)),
            ("insert", pt(&ins.ins)),
            ("scale", pt(&ins.scale)),
            ("rotation", cc_m3d::file::jn(ins.rot)),
            ("extrusion", pt(&ins.n)),
        ]),
    }
}

/// `toDwgModel(model)` → {dwg, report}: the model as a cc-dwg model (docs/DWG-LIB-SPEC.md) for writeDwg, as JSON.
/// (The app passes `created`; the caller hands the creation time to the writer instead.)
pub fn to_dwg_model(model: &Model) -> Res<(Value, CadReport)> {
    let sc = cad_scene(model)?;
    let metric = sc.insunits != 1.0;
    let mut blocks = vec![
        obj(vec![("name", s("*Model_Space")), ("entities", Value::Array(sc.ents.iter().map(conv).collect()))]),
        obj(vec![("name", s("*Paper_Space")), ("entities", Value::Array(Vec::new()))]),
    ];
    for (name, ents) in &sc.blocks {
        blocks.push(obj(vec![
            ("name", s(name)),
            ("base", json!({ "x": 0, "y": 0, "z": 0 })),
            ("entities", Value::Array(ents.iter().map(conv).collect())),
        ]));
    }
    let dwg = obj(vec![
        ("version", s("AC1015")),
        ("codepage", s("ANSI_1252")),
        (
            "header",
            json!({ "INSUNITS": sc.insunits, "MEASUREMENT": if metric { 1 } else { 0 }, "LUNITS": 2, "LUPREC": if metric { 3 } else { 4 } }),
        ),
        (
            "tables",
            json!({ "layers": [{ "name": "0", "color": { "index": 7 }, "off": false, "frozen": false, "locked": false, "plot": true }] }),
        ),
        ("blocks", Value::Array(blocks)),
        ("layouts", Value::Array(Vec::new())),
        ("objects", Value::Array(Vec::new())),
    ]);
    Ok((dwg, sc.report))
}
