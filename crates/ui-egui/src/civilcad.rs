//! A DWG or DXF drawing as linework (gis-cad-ui.js, 0.4 V1 / 0.5 WL.19): the drawing is read here (cc-io-dwg's DWG
//! reader or DXF model), its layers that hold linework listed (lines, closed shapes, points; with or without
//! elevations), the drawing units and what they become (or another unit chosen), each layer's "Use for the surface"
//! (breaklines from the lines' own elevations or their ELEV, the boundary, survey points — layers named like contours
//! start as breaklines), and survey points from the drawing's block references (by block or by layer; number,
//! description and elevation from the insertion or an attribute). Add: the chosen layers become GIS layers (one undo
//! step), the points one more, each use one more. The maths is cc-io-civil's cad_linework.rs and block_points.rs.
//!
//! Commands: `civil.cad=<action>` (see its doc), `civil.cad.info`.

use cc_civil::gis_state::{Feature, Field, LAYER_TOKENS, Layer, Source, clean_str};
use cc_civil::js::{J, num_to_string, string_of};
use cc_civil::tr;
use cc_io_civil::block_points::{self, Refs};
use cc_io_civil::cad_linework::{self, Linework};
use cc_io_dwg::value::{Obj, V};
use egui::RichText;
use serde_json::{Value, json};

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// A drawing is read up to 200 MB (gis-cad-ui.js MAX_BYTES).
pub const MAX_BYTES: usize = 200 * 1024 * 1024;
/// UNIT_CHOICES: (key, INSUNITS) — "" = what the file says.
const UNIT_CHOICES: [(&str, Option<f64>); 7] =
    [("", None), ("ft", Some(2.0)), ("ft-us", Some(21.0)), ("m", Some(6.0)), ("mm", Some(4.0)), ("cm", Some(5.0)), ("in", Some(1.0))];

fn contour(name: &str) -> bool {
    let u = name.to_uppercase();
    ["TOPO", "CONT", "CNTR", "CTR", "CONTOUR"].iter().any(|k| u.contains(k))
}
fn guess(tag: &str, what: &str) -> bool {
    let t = tag;
    match what {
        "num" => {
            matches!(t, "PT" | "PNT" | "POINT" | "NO" | "NUM" | "NUMBER" | "ID")
                || t.strip_prefix("PT").is_some_and(|r| r == "NO" || r == "_NO")
                || t.strip_prefix("PNT").is_some_and(|r| r == "NO" || r == "_NO")
                || t.strip_prefix("POINT").is_some_and(|r| {
                    let r = r.strip_prefix('_').unwrap_or(r);
                    r == "NO" || r == "NUM" || r == "NUMBER"
                })
        }
        "desc" => {
            matches!(t, "DESC" | "DESCRIPTION" | "CODE" | "DESCR" | "NAME")
                || t.strip_prefix("RAW").is_some_and(|r| r == "DESC" || r == "_DESC")
                || t.strip_prefix("FULL").is_some_and(|r| r == "DESC" || r == "_DESC")
        }
        "z" => matches!(t, "ELEV" | "ELEVATION" | "EL" | "Z" | "HEIGHT" | "ELV" | "LEVEL" | "RL"),
        _ => t.is_empty(),
    }
}

/// The dialog (chooseDialog's state).
#[derive(Clone)]
pub struct CadDlg {
    pub file: String,
    model: Obj,
    pub unit_code: Option<f64>,
    pub r: Linework,
    pub br: Refs,
    /// CAD layers to add
    pub pick: Vec<String>,
    /// "cad layer|geom" → the use chosen (-1 = none)
    pub uses: Vec<(String, i64)>,
    pub b_pick: Vec<String>,
    pub l_pick: Vec<String>,
    pub num: String,
    pub desc: String,
    pub z: String,
    touched: [bool; 3],
}

fn sorted_tags(refs: &Refs, b: &[String], l: &[String]) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for r in refs.refs.iter().filter(|r| b.contains(&r.block) || l.contains(&r.layer)) {
        for (k, _) in &r.attrs {
            if !tags.contains(k) {
                tags.push(k.clone());
            }
        }
    }
    tags.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    tags
}

impl CadDlg {
    fn key(l: &cc_io_civil::cad_linework::CadLayer) -> String {
        format!("{}|{}", l.cad_layer, l.geom)
    }
    fn choices(l: &cc_io_civil::cad_linework::CadLayer) -> Vec<Value> {
        let layer = Layer {
            id: String::new(),
            name: l.name.clone(),
            geom: l.geom.to_string(),
            has_z: l.has_z,
            fields: vec![
                Field { name: "LAYER".into(), ty: "text".into() },
                Field { name: "TYPE".into(), ty: "text".into() },
                Field { name: "ELEV".into(), ty: "number".into() },
            ],
            visible: true,
            color: String::new(),
            source: Source::default(),
            label: None,
        };
        cc_io_civil::gis_layers::use_choices(&layer, &l.features)
    }
    fn chosen_refs(&self) -> usize {
        self.br.refs.iter().filter(|r| self.b_pick.contains(&r.block) || self.l_pick.contains(&r.layer)).count()
    }
    /// howChoices(): the number / description / elevation choices kept, or guessed from the tags.
    fn how_choices(&mut self) {
        let tags = sorted_tags(&self.br, &self.b_pick, &self.l_pick);
        let flat = self.br.refs.iter().filter(|r| self.b_pick.contains(&r.block) || self.l_pick.contains(&r.layer)).all(|r| r.z == 0.0);
        let fill = |was: &str, fixed: &[&str], what: &str, keep: bool| -> String {
            let vals: Vec<String> = fixed.iter().map(|s| s.to_string()).chain(tags.iter().map(|t| format!("attr:{t}"))).collect();
            if keep && vals.iter().any(|v| v == was) {
                return was.to_string();
            }
            match tags.iter().find(|t| guess(t, what)) {
                Some(g) => format!("attr:{g}"),
                None => fixed.first().map(|s| s.to_string()).unwrap_or_default(),
            }
        };
        self.num = fill(&self.num, &["next"], "num", self.touched[0]);
        self.desc = fill(&self.desc, &["block", "layer"], "desc", self.touched[1]);
        self.z = fill(&self.z, &["insert", "none"], if flat { "z" } else { "never" }, self.touched[2]);
    }
    /// rebuild(): the drawing read again with the units chosen.
    fn rebuild(&mut self, project_unit: &str) {
        let mut model = self.model.clone();
        if let Some(c) = self.unit_code {
            let mut h = model.get("header").and_then(V::as_obj).cloned().unwrap_or_default();
            h.set("INSUNITS", V::Num(c));
            model.set("header", V::Obj(h));
        }
        self.r = cad_linework::cad_linework(&model, &self.file, project_unit, None, None);
        let mut uses = std::mem::take(&mut self.uses);
        for l in &self.r.layers {
            let ch = CadDlg::choices(l);
            if ch.is_empty() {
                continue;
            }
            let key = CadDlg::key(l);
            if !uses.iter().any(|(k, _)| *k == key) {
                let auto = if contour(&l.cad_layer) && l.geom != "pt" {
                    ch.iter()
                        .position(|c| {
                            c.get("use").and_then(Value::as_str) == Some("breaklines") && c.get("from").and_then(Value::as_str) == Some("z")
                        })
                        .map_or(-1, |i| i as i64)
                } else {
                    -1
                };
                uses.push((key, auto));
            }
        }
        self.uses = uses;
        self.br = block_points::block_refs(&model, project_unit, Some(self.r.scale));
        self.how_choices();
    }
    fn chosen(&self) -> Vec<&cc_io_civil::cad_linework::CadLayer> {
        self.r.layers.iter().filter(|l| self.pick.contains(&l.cad_layer)).collect()
    }
    fn use_of(&self, l: &cc_io_civil::cad_linework::CadLayer) -> i64 {
        let k = CadDlg::key(l);
        self.uses.iter().find(|(x, _)| *x == k).map_or(-1, |x| x.1)
    }
    /// sum(): what Add does.
    pub fn what(&self) -> String {
        let ls = self.chosen();
        let uses = ls.iter().filter(|l| self.use_of(l) >= 0).count();
        let n = self.chosen_refs();
        let parts: Vec<String> = [
            if ls.is_empty() { String::new() } else { tr!("cad.ui.what", n = ls.len(), uses = uses) },
            if n > 0 { tr!("cad.blk.what", n = n) } else { String::new() },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        if parts.is_empty() { t0("cad.ui.what.none") } else { parts.join(" ") }
    }
}

impl CivilView {
    /// A DWG or DXF opened while CivilCraft is open (gis-cad-ui.js openFiles → readModel → chooseDialog).
    pub fn cad_open(&mut self, name: &str, bytes: &[u8]) {
        let t_open = web_time::Instant::now();
        if bytes.len() > MAX_BYTES {
            self.message = tr!("cad.ui.too.big", file = name, mb = 200);
            return;
        }
        let model = if name.to_ascii_lowercase().ends_with(".dxf") {
            let text = cc_io_dwg::dxf_model::decode_dxf_bytes(bytes);
            let head: String = text.chars().take(32).collect();
            if head.trim_start().starts_with("AutoCAD Binary DXF") {
                self.message = tr!("cad.ui.binary.dxf", file = name);
                return;
            }
            match cc_io_dwg::dxf_model::try_read_dxf_model(&text) {
                Ok(m) => m,
                Err(e) => {
                    self.message = tr!("cad.ui.read.failed", file = name, why = e);
                    return;
                }
            }
        } else {
            cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default())
        };
        let pu = if self.project.units() == "m" { "m" } else { "ft" };
        let r = cad_linework::cad_linework(&model, name, pu, None, None);
        let br = block_points::block_refs(&model, pu, Some(r.scale));
        if r.layers.is_empty() && br.refs.is_empty() {
            let mut m = vec![tr!("cad.ui.nothing", file = name)];
            m.extend(r.notes.iter().cloned());
            self.message = m.join(" ");
            return;
        }
        let pick: Vec<String> = r.cad_layers.iter().filter(|l| l.on).map(|l| l.name.clone()).collect();
        let mut d = CadDlg {
            file: name.to_string(),
            model,
            unit_code: None,
            r,
            br,
            pick,
            uses: Vec::new(),
            b_pick: Vec::new(),
            l_pick: Vec::new(),
            num: "next".into(),
            desc: "block".into(),
            z: "insert".into(),
            touched: [false; 3],
        };
        d.rebuild(pu);
        self.gis3.cad = Some(Box::new(d));
        self.timings.insert("cad open", t_open.elapsed().as_secs_f64() * 1000.0);
    }

    /// `civil.cad=<action>`: units=<"" | ft | ft-us | m | mm | cm | in> · layer=<name>:on|off · use=<layer>|<geom>:<i|-1>
    /// · block=<name>:on|off · blklayer=<name>:on|off · num=|desc=|z=<value> · go · cancel.
    pub fn cad_cmd(&mut self, spec: &str) -> Result<(), String> {
        let pu = if self.project.units() == "m" { "m" } else { "ft" };
        let d = self.gis3.cad.as_mut().ok_or("civil.cad: the drawing dialog is not open")?;
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        let (name, on) = v.rsplit_once(':').unwrap_or((v, "on"));
        let set = |l: &mut Vec<String>, n: &str, on: &str| {
            l.retain(|x| x != n);
            if on != "off" {
                l.push(n.to_string());
            }
        };
        match k {
            "cancel" | "close" => self.gis3.cad = None,
            "units" => {
                let c = UNIT_CHOICES.iter().find(|(key, _)| *key == v).ok_or("civil.cad: units=|ft|ft-us|m|mm|cm|in")?;
                d.unit_code = c.1;
                d.rebuild(pu);
            }
            "layer" => {
                if !d.r.cad_layers.iter().any(|l| l.name == name) {
                    return Err(format!("civil.cad: no layer {name}"));
                }
                set(&mut d.pick, name, on);
            }
            "use" => {
                let (key, i) = v.rsplit_once(':').ok_or("civil.cad: use=<layer>|<geom>:<index>")?;
                let i: i64 = i.parse().map_err(|_| "civil.cad: use=<layer>|<geom>:<index>")?;
                let l = d.r.layers.iter().find(|l| CadDlg::key(l) == key).ok_or("civil.cad: no such layer")?;
                let n = CadDlg::choices(l).len() as i64;
                if i < -1 || i >= n {
                    return Err("civil.cad: no such use".into());
                }
                match d.uses.iter_mut().find(|(k, _)| k == key) {
                    Some(x) => x.1 = i,
                    None => return Err("civil.cad: that layer has nothing to use".into()),
                }
            }
            "block" | "blklayer" => {
                let ok = if k == "block" { d.br.blocks.iter().any(|b| b.0 == name) } else { d.br.layers.iter().any(|b| b.0 == name) };
                if !ok {
                    return Err(format!("civil.cad: no {k} {name}"));
                }
                set(if k == "block" { &mut d.b_pick } else { &mut d.l_pick }, name, on);
                d.how_choices();
            }
            "num" | "desc" | "z" => {
                let fixed: &[&str] = match k {
                    "num" => &["next"],
                    "desc" => &["block", "layer"],
                    _ => &["insert", "none"],
                };
                let tags = sorted_tags(&d.br, &d.b_pick, &d.l_pick);
                if !fixed.contains(&v) && !v.strip_prefix("attr:").is_some_and(|t| tags.iter().any(|x| x == t)) {
                    return Err(format!("civil.cad: {k}={v} is not offered"));
                }
                let i = match k {
                    "num" => 0,
                    "desc" => 1,
                    _ => 2,
                };
                if let Some(x) = d.touched.get_mut(i) {
                    *x = true;
                }
                match k {
                    "num" => d.num = v.into(),
                    "desc" => d.desc = v.into(),
                    _ => d.z = v.into(),
                }
            }
            "go" => self.cad_go()?,
            o => return Err(format!("civil.cad: unknown action {o}")),
        }
        Ok(())
    }

    /// Add: the chosen layers as GIS layers, the block points, the uses (each one undo step).
    fn cad_go(&mut self) -> Result<(), String> {
        let d = self.gis3.cad.as_ref().ok_or("closed")?;
        let ls: Vec<cc_io_civil::cad_linework::CadLayer> = d.chosen().into_iter().cloned().collect();
        let nrefs = d.chosen_refs();
        if ls.is_empty() && nrefs == 0 {
            return Err("civil.cad: nothing chosen".into());
        }
        let d = self.gis3.cad.take().ok_or("closed")?;
        let file = d.file.clone();
        let mut msgs: Vec<String> = Vec::new();
        let ids = if ls.is_empty() { Vec::new() } else { self.add_layers(&ls, &tr!("cad.ui.undo", file = file.as_str())) };
        if !ls.is_empty() {
            msgs.push(tr!("cad.ui.added", n = ids.len(), file = file.as_str()));
        }
        if nrefs > 0 {
            let existing: Vec<String> = self.project.points.iter().map(|p| p.n.clone()).collect();
            let o = block_points::FromBlocks {
                blocks: &d.b_pick,
                layers: &d.l_pick,
                desc: &d.desc,
                z: &d.z,
                number: &d.num,
                existing: &existing,
                start: None,
            };
            let (pts, no_z, no_num, renum) = block_points::points_from_blocks(&d.br.refs, &o);
            let n = pts.len();
            let pts: Vec<cc_civil::points::Pt> = pts
                .into_iter()
                .map(|p| cc_civil::points::Pt {
                    n: p.n,
                    x: p.x,
                    y: p.y,
                    z: p.z,
                    d: p.d,
                    blk: Some(json!({ "name": p.blk.0, "layer": p.blk.1, "handle": p.blk.2 })),
                })
                .collect();
            self.project.edit_points(&tr!("cad.blk.undo", file = file.as_str()), &mut |mut all, _| {
                all.extend(pts.iter().cloned());
                Some(all)
            });
            msgs.push(tr!("cad.blk.added", n = n, file = file.as_str()));
            if renum > 0 {
                msgs.push(tr!("cad.blk.renumbered", n = renum));
            }
            if no_z > 0 {
                msgs.push(tr!("cad.blk.noz", n = no_z));
            }
            if no_num > 0 {
                msgs.push(tr!("cad.blk.nonum", n = no_num));
            }
        }
        for (i, l) in ls.iter().enumerate() {
            let k = d.use_of(l);
            let (Ok(k), Some(id)) = (usize::try_from(k), ids.get(i)) else { continue };
            let Some(c) = CadDlg::choices(l).get(k).cloned() else { continue };
            let from = c.get("from").and_then(Value::as_str).unwrap_or("z").to_string();
            let fields: Vec<String> = cc_civil::js::arr(c.get("fields")).iter().map(|x| string_of(Some(x))).collect();
            let field = (from == "field")
                .then(|| if fields.iter().any(|f| f == "ELEV") { "ELEV".to_string() } else { fields.first().cloned().unwrap_or_default() });
            let (_, m) = self.use_for_surface(id, &string_of(c.get("use")), &from, field.as_deref(), None);
            msgs.push(m);
        }
        self.fit();
        self.message = format!("{} {}", msgs.join(" "), t0("cad.ui.after"));
        Ok(())
    }

    /// addLayers(project, layers, { label }): GIS layers, one undo step → their ids.
    pub fn add_layers(&mut self, ls: &[cc_io_civil::cad_linework::CadLayer], label: &str) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let fields = vec![
            Field { name: "LAYER".into(), ty: "text".into() },
            Field { name: "TYPE".into(), ty: "text".into() },
            Field { name: "ELEV".into(), ty: "number".into() },
        ];
        self.project.edit_gis(label, &mut |mut map, s| {
            let mut seq = s.get("seq").and_then(Value::as_f64).unwrap_or(1.0);
            let Some(m) = s.as_object_mut() else { return Some(map) };
            let g = m.entry("gis").or_insert_with(|| json!({ "layers": [], "maps": [] }));
            if !g.is_object() {
                *g = json!({ "layers": [], "maps": [] });
            }
            let Some(gm) = g.as_object_mut() else { return Some(map) };
            let lsv = gm.entry("layers").or_insert_with(|| Value::Array(Vec::new()));
            let Some(lsv) = lsv.as_array_mut() else { return Some(map) };
            let k0 = lsv.len();
            for (k, l) in (k0..).zip(ls.iter()) {
                if lsv.len() >= cc_civil::gis_state::MAX_LAYERS {
                    break;
                }
                let id = format!("g{}", num_to_string(seq));
                seq += 1.0;
                ids.push(id.clone());
                let name = clean_str(&l.name, 120);
                let layer = Layer {
                    id: id.clone(),
                    name: if name.is_empty() { t0("layers.default") } else { name },
                    geom: l.geom.to_string(),
                    has_z: l.has_z,
                    fields: fields.clone(),
                    visible: true,
                    color: LAYER_TOKENS.get(k % LAYER_TOKENS.len()).copied().unwrap_or("blue").to_string(),
                    source: Source { file: clean_str(&l.file, 200), format: "CAD".into(), crs: None },
                    label: None,
                };
                lsv.push(layer.j());
                let feats: Vec<Feature> = l.features.clone();
                map.push((id, std::sync::Arc::new(feats)));
            }
            m.insert("seq".into(), cc_civil::js::jnum(seq));
            Some(map)
        });
        ids
    }

    /// `civil.cad.info`: the dialog's rows, units, notes, block rows and what Add does.
    pub fn cad_info(&self) -> Result<String, String> {
        let d = self.gis3.cad.as_ref().ok_or("civil.cad.info: the drawing dialog is not open")?;
        let pu = if self.project.units() == "m" { "m" } else { "ft" };
        let mut out = vec![tr!("cad.ui.title", file = d.file.as_str())];
        for cl in &d.r.cad_layers {
            let kinds: Vec<String> = [
                (cl.lines > 0).then(|| tr!("cad.ui.n.lines", n = cl.lines)),
                (cl.polygons > 0).then(|| tr!("cad.ui.n.closed", n = cl.polygons)),
                (cl.points > 0).then(|| tr!("cad.ui.n.points", n = cl.points)),
            ]
            .into_iter()
            .flatten()
            .collect();
            let uses: Vec<String> =
                d.r.layers
                    .iter()
                    .filter(|l| l.cad_layer == cl.name)
                    .filter_map(|l| {
                        let ch = CadDlg::choices(l);
                        if ch.is_empty() {
                            return None;
                        }
                        let u = d.use_of(l);
                        let label = usize::try_from(u)
                            .ok()
                            .and_then(|i| ch.get(i))
                            .map_or_else(|| t0("cad.ui.use.none"), |c| string_of(c.get("label")));
                        Some(format!("{}={label}", CadDlg::key(l)))
                    })
                    .collect();
            out.push(format!(
                "[{}] {}{} · {} · {} · {}",
                if d.pick.contains(&cl.name) { "x" } else { " " },
                cl.name,
                if cl.on { String::new() } else { format!(" {}", t0("cad.ui.off")) },
                kinds.join(", "),
                t0(if cl.z { "cad.ui.z.yes" } else { "cad.ui.z.no" }),
                if uses.is_empty() { t0("cad.ui.use.nothing") } else { uses.join("; ") }
            ));
        }
        let header = d.model.get("header").and_then(V::as_obj).cloned();
        let mut h = header.unwrap_or_default();
        if let Some(c) = d.unit_code {
            h.set("INSUNITS", V::Num(c));
        }
        let us = cad_linework::unit_scale(Some(&h), pu);
        let file_label = if d.r.units.label.is_empty() { t0("cad.ui.units.none") } else { d.r.units.label.clone() };
        let ukey = UNIT_CHOICES.iter().find(|c| c.1 == d.unit_code).map_or("", |c| c.0);
        out.push(format!(
            "{}: {}",
            t0("cad.ui.units"),
            if ukey.is_empty() { tr!("cad.ui.units.file", unit = file_label) } else { t0(&format!("cad.ui.unit.{ukey}")) }
        ));
        out.push(us.note.unwrap_or_else(|| tr!("cad.ui.units.same", unit = t0(&format!("cad.unit.{pu}")))));
        out.extend(d.r.notes.iter().cloned());
        if !d.br.refs.is_empty() {
            for (name, count, layers, tags) in &d.br.blocks {
                out.push(format!(
                    "[{}] {name} · {} · {} · {}",
                    if d.b_pick.contains(name) { "x" } else { " " },
                    cc_civil::units::grouped(*count as f64, 0),
                    layers.join(", "),
                    if tags.is_empty() { "—".into() } else { tags.join(", ") }
                ));
            }
            for (name, count) in &d.br.layers {
                out.push(format!(
                    "[{}] {}",
                    if d.l_pick.contains(name) { "x" } else { " " },
                    tr!("cad.blk.layer", layer = name.as_str(), n = cc_civil::units::grouped(*count as f64, 0))
                ));
            }
            out.push(format!("num={} desc={} z={}", d.num, d.desc, d.z));
            let n = d.chosen_refs();
            out.push(if n > 0 { tr!("cad.blk.what", n = n) } else { t0("cad.blk.what.none") });
        }
        out.push(d.what());
        Ok(out.join(" | "))
    }
}

/// The drawing window.
pub fn cad_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = cv.gis3.cad.as_ref() else { return };
    let mut open = true;
    let dim = |ui: &mut egui::Ui, s: &str| {
        ui.add(egui::Label::new(RichText::new(s).small().color(tk.text_dim)).wrap());
    };
    let pu = if cv.project.units() == "m" { "m" } else { "ft" };
    egui::Window::new(tr!("cad.ui.title", file = d.file.as_str()))
        .id(egui::Id::unique("cv_cad"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .open(&mut open)
        .collapsible(false)
        .default_width(640.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(ctx.content_rect().height() * 0.75)
                .min_scrolled_height(ctx.content_rect().height() * 0.6)
                .show(ui, |ui| {
                    dim(ui, &t0("cad.ui.lede"));
                    egui::Grid::new("cv_cad_layers").num_columns(5).spacing([10.0, 3.0]).striped(true).show(ui, |ui| {
                        ui.label("");
                        for h in ["cad.ui.col.layer", "cad.ui.col.holds", "cad.ui.col.z", "cad.ui.col.use"] {
                            ui.label(RichText::new(t0(h)).small().strong().color(tk.text_dim));
                        }
                        ui.end_row();
                        for cl in &d.r.cad_layers {
                            let mut on = d.pick.contains(&cl.name);
                            if ui.checkbox(&mut on, "").on_hover_text(tr!("cad.ui.add.layer", layer = cl.name.as_str())).changed() {
                                acts.push(("civil.cad", format!("layer={}:{}", cl.name, if on { "on" } else { "off" })));
                            }
                            ui.label(format!("{}{}", cl.name, if cl.on { String::new() } else { format!(" {}", t0("cad.ui.off")) }));
                            let kinds: Vec<String> = [
                                (cl.lines > 0).then(|| tr!("cad.ui.n.lines", n = cl.lines)),
                                (cl.polygons > 0).then(|| tr!("cad.ui.n.closed", n = cl.polygons)),
                                (cl.points > 0).then(|| tr!("cad.ui.n.points", n = cl.points)),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            ui.label(RichText::new(kinds.join(", ")).small());
                            ui.label(RichText::new(t0(if cl.z { "cad.ui.z.yes" } else { "cad.ui.z.no" })).small());
                            ui.vertical(|ui| {
                                let mut any = false;
                                for l in d.r.layers.iter().filter(|l| l.cad_layer == cl.name) {
                                    let ch = CadDlg::choices(l);
                                    if ch.is_empty() {
                                        continue;
                                    }
                                    any = true;
                                    let u = d.use_of(l);
                                    let key = CadDlg::key(l);
                                    let sel = usize::try_from(u)
                                        .ok()
                                        .and_then(|i| ch.get(i))
                                        .map_or_else(|| t0("cad.ui.use.none"), |c| string_of(c.get("label")));
                                    egui::ComboBox::new(("cv_cad_use", key.clone()), "").selected_text(sel).width(220.0).show_ui(
                                        ui,
                                        |ui| {
                                            if ui.selectable_label(u < 0, t0("cad.ui.use.none")).clicked() {
                                                acts.push(("civil.cad", format!("use={key}:-1")));
                                            }
                                            for (i, c) in ch.iter().enumerate() {
                                                if ui.selectable_label(u == i as i64, string_of(c.get("label"))).clicked() {
                                                    acts.push(("civil.cad", format!("use={key}:{i}")));
                                                }
                                            }
                                        },
                                    );
                                }
                                if !any {
                                    ui.label(RichText::new(t0("cad.ui.use.nothing")).small().color(tk.text_dim));
                                }
                            });
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(t0("cad.ui.units"));
                        let file_label = if d.r.units.label.is_empty() { t0("cad.ui.units.none") } else { d.r.units.label.clone() };
                        let label = |k: &str| {
                            if k.is_empty() {
                                tr!("cad.ui.units.file", unit = file_label.as_str())
                            } else {
                                t0(&format!("cad.ui.unit.{k}"))
                            }
                        };
                        let cur = UNIT_CHOICES.iter().find(|c| c.1 == d.unit_code).map_or("", |c| c.0);
                        egui::ComboBox::new("cv_cad_units", "").selected_text(label(cur)).show_ui(ui, |ui| {
                            for (k, _) in UNIT_CHOICES {
                                if ui.selectable_label(cur == k, label(k)).clicked() {
                                    acts.push(("civil.cad", format!("units={k}")));
                                }
                            }
                        });
                    });
                    let mut h = d.model.get("header").and_then(V::as_obj).cloned().unwrap_or_default();
                    if let Some(c) = d.unit_code {
                        h.set("INSUNITS", V::Num(c));
                    }
                    let us = cad_linework::unit_scale(Some(&h), pu);
                    dim(ui, &us.note.unwrap_or_else(|| tr!("cad.ui.units.same", unit = t0(&format!("cad.unit.{pu}")))));
                    if !d.r.notes.is_empty() {
                        crate::civilpanel::report_pub(ui, tk, &d.r.notes, false);
                    }
                    if !d.br.refs.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(t0("cad.blk.head")).strong().color(tk.text_strong));
                        dim(ui, &t0("cad.blk.lede"));
                        egui::Grid::new("cv_cad_blocks").num_columns(5).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
                            ui.label("");
                            for h in ["cad.blk.col.block", "cad.blk.col.count", "cad.blk.col.layers", "cad.blk.col.tags"] {
                                ui.label(RichText::new(t0(h)).small().strong().color(tk.text_dim));
                            }
                            ui.end_row();
                            for (name, count, layers, tags) in &d.br.blocks {
                                let mut on = d.b_pick.contains(name);
                                if ui.checkbox(&mut on, "").on_hover_text(tr!("cad.blk.take", block = name.as_str())).changed() {
                                    acts.push(("civil.cad", format!("block={name}:{}", if on { "on" } else { "off" })));
                                }
                                ui.label(name);
                                ui.label(cc_civil::units::grouped(*count as f64, 0));
                                ui.label(RichText::new(layers.join(", ")).small());
                                ui.label(RichText::new(if tags.is_empty() { "—".to_string() } else { tags.join(", ") }).small());
                                ui.end_row();
                            }
                        });
                        dim(ui, &t0("cad.blk.bylayer"));
                        for (name, count) in &d.br.layers {
                            let mut on = d.l_pick.contains(name);
                            if ui
                                .checkbox(
                                    &mut on,
                                    tr!("cad.blk.layer", layer = name.as_str(), n = cc_civil::units::grouped(*count as f64, 0)),
                                )
                                .changed()
                            {
                                acts.push(("civil.cad", format!("blklayer={name}:{}", if on { "on" } else { "off" })));
                            }
                        }
                        let tags = sorted_tags(&d.br, &d.b_pick, &d.l_pick);
                        for (k, label, fixed) in [
                            ("num", "cad.blk.num", vec![("next", t0("cad.blk.num.next"))]),
                            ("desc", "cad.blk.desc", vec![("block", t0("cad.blk.desc.block")), ("layer", t0("cad.blk.desc.layer"))]),
                            ("z", "cad.blk.z", vec![("insert", t0("cad.blk.z.insert")), ("none", t0("cad.blk.z.none"))]),
                        ] {
                            let cur = match k {
                                "num" => &d.num,
                                "desc" => &d.desc,
                                _ => &d.z,
                            };
                            let show = |v: &str| {
                                fixed
                                    .iter()
                                    .find(|f| f.0 == v)
                                    .map(|f| f.1.clone())
                                    .unwrap_or_else(|| tr!("cad.blk.attr", tag = v.trim_start_matches("attr:")))
                            };
                            ui.horizontal(|ui| {
                                ui.label(t0(label));
                                egui::ComboBox::new(("cv_cad_how", k), "").selected_text(show(cur)).show_ui(ui, |ui| {
                                    for (v, l) in &fixed {
                                        if ui.selectable_label(cur == v, l).clicked() {
                                            acts.push(("civil.cad", format!("{k}={v}")));
                                        }
                                    }
                                    for tg in &tags {
                                        let v = format!("attr:{tg}");
                                        if ui.selectable_label(*cur == v, tr!("cad.blk.attr", tag = tg.as_str())).clicked() {
                                            acts.push(("civil.cad", format!("{k}={v}")));
                                        }
                                    }
                                });
                            });
                        }
                        let n = d.chosen_refs();
                        dim(ui, &if n > 0 { tr!("cad.blk.what", n = n) } else { t0("cad.blk.what.none") });
                    }
                    ui.label(RichText::new(t0("cad.ui.what.head")).strong().color(tk.text_strong));
                    dim(ui, &d.what());
                    ui.horizontal(|ui| {
                        if ui.add(crate::shell::Btn::new(t0("cad.ui.cancel"))).clicked() {
                            acts.push(("civil.cad", "cancel".into()));
                        }
                        let can = !d.chosen().is_empty() || d.chosen_refs() > 0;
                        if ui.add_enabled(can, crate::shell::Btn::new(t0("cad.ui.add"))).clicked() {
                            acts.push(("civil.cad", "go".into()));
                        }
                    });
                });
        });
    if !open {
        acts.push(("civil.cad", "cancel".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf() -> Vec<u8> {
        let mut o: Vec<String> = Vec::new();
        let mut g = |c: i32, v: &str| {
            o.push(c.to_string());
            o.push(v.to_string());
        };
        g(0, "SECTION");
        g(2, "HEADER");
        g(9, "$INSUNITS");
        g(70, "2");
        g(0, "ENDSEC");
        g(0, "SECTION");
        g(2, "BLOCKS");
        g(0, "BLOCK");
        g(8, "0");
        g(2, "SPOT");
        g(70, "2");
        g(10, "0");
        g(20, "0");
        g(30, "0");
        g(0, "ATTDEF");
        g(8, "0");
        g(10, "0");
        g(20, "0");
        g(30, "0");
        g(40, "1");
        g(1, "");
        g(3, "PT");
        g(2, "PT");
        g(70, "0");
        g(0, "ENDBLK");
        g(0, "ENDSEC");
        g(0, "SECTION");
        g(2, "ENTITIES");
        for (i, z) in [(0, "250"), (1, "255")] {
            g(0, "LINE");
            g(8, "TOPO");
            g(10, &format!("{}", 6150000 + i * 30));
            g(20, "2080000");
            g(30, z);
            g(11, &format!("{}", 6150020 + i * 30));
            g(21, "2080200");
            g(31, z);
        }
        g(0, "INSERT");
        g(8, "SURVEY");
        g(66, "1");
        g(2, "SPOT");
        g(10, "6150100");
        g(20, "2080100");
        g(30, "251");
        g(0, "ATTRIB");
        g(8, "SURVEY");
        g(10, "6150100");
        g(20, "2080100");
        g(30, "0");
        g(40, "1");
        g(1, "9001");
        g(2, "PT");
        g(70, "0");
        g(0, "SEQEND");
        g(0, "ENDSEC");
        g(0, "EOF");
        (o.join("\n") + "\n").into_bytes()
    }

    #[test]
    fn a_dxf_adds_layers_breaklines_and_block_points() {
        let mut cv = CivilView::sample();
        let (n0, b0) = (cv.project.points.len(), cc_civil::js::arr(cv.project.state["surface"].get("breaklines")).len());
        cv.cad_open("site.dxf", &dxf());
        assert!(cv.gis3.cad.is_some(), "{}", cv.message);
        let info = cv.cad_info().unwrap();
        assert!(info.contains("TOPO"), "{info}");
        cv.cad_cmd("block=SPOT:on").unwrap();
        assert_eq!(cv.gis3.cad.as_ref().unwrap().num, "attr:PT", "a likely number tag is guessed");
        cv.cad_cmd("go").unwrap();
        assert!(cv.gis3.cad.is_none());
        assert_eq!(cv.project.points.len(), n0 + 1, "{}", cv.message);
        assert!(cv.project.points.iter().any(|p| p.n == "9001" && p.blk.is_some()));
        assert_eq!(
            cc_civil::js::arr(cv.project.state["surface"].get("breaklines")).len(),
            b0 + 2,
            "TOPO starts as breaklines: {}",
            cv.message
        );
        assert!(!cv.gis_layers().is_empty());
    }

    #[test]
    fn nothing_readable_is_said() {
        let mut cv = CivilView::sample();
        cv.cad_open("x.dxf", b"hello");
        assert!(cv.gis3.cad.is_none() && !cv.message.is_empty());
        cv.cad_open("x.dwg", b"AC1032 not really");
        assert!(cv.gis3.cad.is_none());
    }
}
