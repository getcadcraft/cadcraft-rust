//! GIS › Export (gis-export.js exportKinds / exportGroups / exportProject, gis-vector-ui.js exportDialog): the project's
//! own objects (survey points, contours, breaklines, the boundary, alignments, pads, daylight and corridor lines, pipes,
//! structures) and its GIS layers as feature layers in grid coordinates, written as a zip of shapefiles, a GeoPackage,
//! GeoJSON, KML or KMZ by cc-io-civil's writers. Local dates in the files (the .dbf header, the zip entries) are the
//! clock in the zone the host passes (cc_clock, as 0.5's `new Date()` getters). `civil.gis.export` drives it.

use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::i18n::t;
use cc_civil::js::{arr, numf, string_of};
use cc_civil::tr;
use cc_io_civil::gis_export::{EGroup, safe_name};
use serde_json::{Map, Value, json};

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// [key, label key, geometry, file stem] (gis-export.js KINDS).
const KINDS: [(&str, &str, &str, &str); 11] = [
    ("points", "gexp.kind.points", "pt", "survey_points"),
    ("contours-eg", "gexp.kind.contours-eg", "ln", "contours_existing"),
    ("contours-fg", "gexp.kind.contours-fg", "ln", "contours_graded"),
    ("breaklines", "gexp.kind.breaklines", "ln", "breaklines"),
    ("boundary", "gexp.kind.boundary", "pg", "boundary"),
    ("alignments", "gexp.kind.alignments", "ln", "alignments"),
    ("pads", "gexp.kind.pads", "pg", "pads"),
    ("daylight", "gexp.kind.daylight", "ln", "daylight_lines"),
    ("corridor", "gexp.kind.corridor", "ln", "corridor_lines"),
    ("pipes", "gexp.kind.pipes", "ln", "pipes"),
    ("structures", "gexp.kind.structures", "pt", "structures"),
];

/// The export formats (value, label key).
pub const FORMATS: [(&str, &str); 5] = [
    ("shp", "vec.x.fmt.shp"),
    ("gpkg", "vec.x.fmt.gpkg"),
    ("geojson", "vec.x.fmt.geojson"),
    ("kml", "kml.fmt.kml"),
    ("kmz", "kml.fmt.kmz"),
];

/// One kind: key, label, geometry, file stem, how many features.
pub struct Kind {
    pub key: String,
    pub label: String,
    pub geom: String,
    pub file: String,
    pub count: usize,
}

struct Built {
    has_z: bool,
    fields: Vec<Field>,
    features: Vec<Feature>,
    coord_fields: bool,
}

fn fld(name: &str, ty: &str) -> Field {
    Field { name: name.into(), ty: ty.into() }
}
fn fnum(v: f64) -> Value {
    cc_civil::js::jnum(v)
}
fn props(kv: Vec<(&str, Value)>) -> Map<String, Value> {
    kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
/// `close(r)`: the first position again at the end when it is not there.
fn close(mut r: Vec<Pos>) -> Vec<Pos> {
    if let (Some(a), Some(b)) = (r.first(), r.last())
        && (a.first() != b.first() || a.get(1) != b.get(1))
    {
        let f = a.clone();
        r.push(f);
    }
    r
}
fn p3(p: &Value, z: Option<f64>) -> Pos {
    vec![numf(p.get("x")), numf(p.get("y")), z.unwrap_or_else(|| p.get("z").and_then(Value::as_f64).unwrap_or(0.0))]
}
fn opt_num(v: Option<f64>) -> Value {
    v.filter(|x| x.is_finite()).map_or(Value::Null, fnum)
}

impl CivilView {
    /// gis-export.js `build(project, key)`.
    fn gis_build(&mut self, key: &str) -> Option<Built> {
        let units = self.project.units().to_string();
        let st = self.project.state.clone();
        let feat = |g: Option<Geom>, p: Map<String, Value>| Feature { g, p };
        Some(match key {
            "points" => Built {
                has_z: self.project.points.iter().any(|q| q.z.is_some()),
                fields: vec![fld("P", "text"), fld("N", "number"), fld("E", "number"), fld("Z", "number"), fld("D", "text")],
                features: self
                    .project
                    .points
                    .iter()
                    .map(|q| {
                        let c = match q.z {
                            Some(z) => vec![q.x, q.y, z],
                            None => vec![q.x, q.y],
                        };
                        feat(
                            Some(Geom::Pt(c)),
                            props(vec![
                                ("P", Value::String(q.n.clone())),
                                ("N", fnum(q.y)),
                                ("E", fnum(q.x)),
                                ("Z", q.z.map_or(Value::Null, fnum)),
                                ("D", Value::String(q.d.clone())),
                            ]),
                        )
                    })
                    .collect(),
                coord_fields: true,
            },
            "contours-eg" | "contours-fg" => {
                let list = if key == "contours-eg" { self.project.contours() } else { self.project.contours_fg() };
                Built {
                    has_z: true,
                    fields: vec![fld("ELEV", "number")],
                    features: list
                        .iter()
                        .map(|c| {
                            let mut r: Vec<Pos> = c.pts.iter().map(|p| vec![p.x, p.y]).collect();
                            if c.closed {
                                r = close(r);
                            }
                            let line: Vec<Pos> = r
                                .into_iter()
                                .map(|p| vec![p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0), c.z])
                                .collect();
                            feat(Some(Geom::Ln(vec![line])), props(vec![("ELEV", fnum(c.z))]))
                        })
                        .collect(),
                    coord_fields: false,
                }
            }
            "breaklines" => Built {
                has_z: true,
                fields: vec![fld("NAME", "text")],
                features: self
                    .project
                    .eg_breaklines()
                    .iter()
                    .filter(|b| b.pts.len() >= 2)
                    .map(|b| {
                        let line: Vec<Pos> = b.pts.iter().map(|p| vec![p.x, p.y, if p.z.is_finite() { p.z } else { 0.0 }]).collect();
                        let name = if b.name.is_empty() { "Breakline".to_string() } else { b.name.clone() };
                        feat(Some(Geom::Ln(vec![line])), props(vec![("NAME", Value::String(name))]))
                    })
                    .collect(),
                coord_fields: false,
            },
            "boundary" => {
                let b = arr(st.get("surface").and_then(|s| s.get("boundary")));
                if b.len() >= 3 {
                    let r: Vec<Pos> = b.iter().map(|p| vec![numf(p.get("x")), numf(p.get("y"))]).collect();
                    Built {
                        has_z: false,
                        fields: vec![fld("NAME", "text")],
                        features: vec![feat(Some(Geom::Pg(vec![vec![close(r)]])), props(vec![("NAME", json!("Boundary"))]))],
                        coord_fields: false,
                    }
                } else {
                    let eg = self.project.eg();
                    let loops: Vec<Vec<(f64, f64)>> = eg
                        .surface
                        .as_ref()
                        .map(|s| s.outline().iter().map(|l| l.iter().map(|p| (p.x, p.y)).collect()).collect())
                        .unwrap_or_default();
                    let polys = cc_io_civil::gis_export::nest(&loops);
                    Built {
                        has_z: false,
                        fields: vec![fld("NAME", "text")],
                        features: if polys.is_empty() {
                            Vec::new()
                        } else {
                            vec![feat(Some(Geom::Pg(polys)), props(vec![("NAME", json!("Surface edge"))]))]
                        },
                        coord_fields: false,
                    }
                }
            }
            "alignments" => {
                let mut fs = Vec::new();
                for a in arr(st.get("alignments")) {
                    let al = self.project.alignment(a);
                    if !al.ok() {
                        continue;
                    }
                    let line: Vec<Pos> = al.polyline(f64::INFINITY, std::f64::consts::PI / 360.0).iter().map(|p| vec![p.0, p.1]).collect();
                    fs.push(feat(
                        Some(Geom::Ln(vec![line])),
                        props(vec![
                            ("NAME", a.get("name").cloned().unwrap_or(Value::Null)),
                            ("LENGTH", fnum(al.length)),
                            ("STA_START", fnum(al.start)),
                            ("STA_END", fnum(al.end)),
                        ]),
                    ));
                }
                Built {
                    has_z: false,
                    fields: vec![fld("NAME", "text"), fld("LENGTH", "number"), fld("STA_START", "number"), fld("STA_END", "number")],
                    features: fs,
                    coord_fields: false,
                }
            }
            "pads" => Built {
                has_z: true,
                fields: vec![fld("NAME", "text"), fld("ELEV", "number"), fld("AREA", "number")],
                features: arr(st.get("pads"))
                    .iter()
                    .filter(|d| arr(d.get("pts")).len() >= 3)
                    .map(|d| {
                        let z = cc_civil::js::to_number(d.get("z").unwrap_or(&Value::Null));
                        let pts = arr(d.get("pts"));
                        let r: Vec<Pos> = pts.iter().map(|p| vec![numf(p.get("x")), numf(p.get("y")), z]).collect();
                        let r2: Vec<Pos> = pts.iter().map(|p| vec![numf(p.get("x")), numf(p.get("y"))]).collect();
                        let area = cc_io_civil::gis_geom::ring_area(&r2).abs();
                        feat(
                            Some(Geom::Pg(vec![vec![close(r)]])),
                            props(vec![("NAME", d.get("name").cloned().unwrap_or(Value::Null)), ("ELEV", fnum(z)), ("AREA", fnum(area))]),
                        )
                    })
                    .collect(),
                coord_fields: false,
            },
            "daylight" | "corridor" => {
                let g = self.project.graded();
                let mut out = Vec::new();
                for it in &g.items {
                    let name = if it.name.is_empty() { None } else { Some(it.name.clone()) };
                    let line_of = |k: &str| it.lines.as_ref().and_then(|ls| ls.iter().find(|(n, _)| n == k)).map(|(_, v)| v.clone());
                    if key == "daylight" {
                        let dl = arr(it.info.get("daylight"));
                        if it.kind == "pad" && dl.len() >= 3 && it.problems.is_empty() {
                            let r: Vec<Pos> = dl.iter().map(|p| p3(p, None)).collect();
                            out.push(feat(
                                Some(Geom::Ln(vec![close(r)])),
                                props(vec![("NAME", Value::String(format!("{} daylight", name.clone().unwrap_or_else(|| "Pad".into()))))]),
                            ));
                        }
                        if it.kind == "corridor" {
                            for k in ["DL_L", "DL_R"] {
                                if let Some(l) = line_of(k).filter(|l| l.len() >= 2) {
                                    let r: Vec<Pos> = l.iter().map(|p| vec![p.x, p.y, p.z]).collect();
                                    out.push(feat(
                                        Some(Geom::Ln(vec![r])),
                                        props(vec![(
                                            "NAME",
                                            Value::String(format!("{} {k}", name.clone().unwrap_or_else(|| "Corridor".into()))),
                                        )]),
                                    ));
                                }
                            }
                        }
                    } else if it.kind == "corridor" {
                        for k in ["CL", "EOP_L", "EOP_R", "SH_L", "SH_R"] {
                            if let Some(l) = line_of(k).filter(|l| l.len() >= 2) {
                                let r: Vec<Pos> = l.iter().map(|p| vec![p.x, p.y, p.z]).collect();
                                out.push(feat(
                                    Some(Geom::Ln(vec![r])),
                                    props(vec![
                                        ("NAME", Value::String(name.clone().unwrap_or_else(|| "Corridor".into()))),
                                        ("CODE", Value::String(k.into())),
                                    ]),
                                ));
                            }
                        }
                    }
                }
                Built {
                    has_z: true,
                    fields: if key == "daylight" { vec![fld("NAME", "text")] } else { vec![fld("NAME", "text"), fld("CODE", "text")] },
                    features: out,
                    coord_fields: false,
                }
            }
            "pipes" | "structures" => {
                let Some(n) = st.get("network").filter(|n| n.is_object()) else {
                    return Some(Built { has_z: true, fields: Vec::new(), features: Vec::new(), coord_fields: false });
                };
                let chk = self.project.network_check();
                let du = if units == "m" { "mm" } else { "in" };
                let structs = arr(n.get("structures"));
                let pipes = arr(n.get("pipes"));
                let fin = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|x| x.is_finite());
                if key == "pipes" {
                    let mut out = Vec::new();
                    for p in pipes {
                        let by = |k: &str| structs.iter().find(|x| x.get("id") == p.get(k));
                        let (Some(a), Some(b)) = (by("from"), by("to")) else { continue };
                        let id = string_of(p.get("id"));
                        let info = chk.as_ref().and_then(|c| c.pipes.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone()));
                        let (ax, ay, bx, by_) = (numf(a.get("x")), numf(a.get("y")), numf(b.get("x")), numf(b.get("y")));
                        let up = fin(p.get("invUp"));
                        let dn = fin(p.get("invDn"));
                        let slope = info.as_ref().and_then(|i| i.slope).filter(|s| s.is_finite()).map(|s| s * 100.0);
                        let len = info.as_ref().map_or_else(|| (bx - ax).hypot(by_ - ay), |i| i.len);
                        out.push(feat(
                            Some(Geom::Ln(vec![vec![vec![ax, ay, up.unwrap_or(0.0)], vec![bx, by_, dn.unwrap_or(0.0)]]])),
                            props(vec![
                                ("NAME", p.get("name").cloned().unwrap_or(Value::Null)),
                                ("FROM", a.get("name").cloned().unwrap_or(Value::Null)),
                                ("TO", b.get("name").cloned().unwrap_or(Value::Null)),
                                ("DIA", p.get("dia").cloned().unwrap_or(Value::Null)),
                                ("DIA_UNIT", json!(du)),
                                ("INV_UP", opt_num(up)),
                                ("INV_DN", opt_num(dn)),
                                ("SLOPE", opt_num(slope)),
                                ("LENGTH", fnum(len)),
                                ("NETWORK", Value::String(format!("{} ({})", string_of(n.get("name")), string_of(n.get("type"))))),
                            ]),
                        ));
                    }
                    Built {
                        has_z: true,
                        fields: vec![
                            fld("NAME", "text"),
                            fld("FROM", "text"),
                            fld("TO", "text"),
                            fld("DIA", "number"),
                            fld("DIA_UNIT", "text"),
                            fld("INV_UP", "number"),
                            fld("INV_DN", "number"),
                            fld("SLOPE", "number"),
                            fld("LENGTH", "number"),
                            fld("NETWORK", "text"),
                        ],
                        features: out,
                        coord_fields: false,
                    }
                } else {
                    let out = structs
                        .iter()
                        .map(|x| {
                            let id = string_of(x.get("id"));
                            let info = chk.as_ref().and_then(|c| c.structs.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone()));
                            let rim = info.as_ref().and_then(|i| i.rim).or_else(|| fin(x.get("rim")));
                            let mut inv: Vec<f64> =
                                pipes.iter().filter(|p| p.get("to") == x.get("id")).filter_map(|p| fin(p.get("invDn"))).collect();
                            inv.extend(pipes.iter().filter(|p| p.get("from") == x.get("id")).filter_map(|p| fin(p.get("invUp"))));
                            let (xx, yy) = (numf(x.get("x")), numf(x.get("y")));
                            feat(
                                Some(Geom::Pt(match rim {
                                    Some(r) => vec![xx, yy, r],
                                    None => vec![xx, yy],
                                })),
                                props(vec![
                                    ("NAME", x.get("name").cloned().unwrap_or(Value::Null)),
                                    ("RIM", opt_num(rim)),
                                    (
                                        "INVERT",
                                        if inv.is_empty() { Value::Null } else { fnum(inv.iter().fold(f64::INFINITY, |m, v| m.min(*v))) },
                                    ),
                                    ("SUMP", opt_num(info.as_ref().and_then(|i| i.sump))),
                                    ("DIA", x.get("dia").cloned().unwrap_or(Value::Null)),
                                    ("DIA_UNIT", json!(du)),
                                ]),
                            )
                        })
                        .collect();
                    Built {
                        has_z: true,
                        fields: vec![
                            fld("NAME", "text"),
                            fld("RIM", "number"),
                            fld("INVERT", "number"),
                            fld("SUMP", "number"),
                            fld("DIA", "number"),
                            fld("DIA_UNIT", "text"),
                        ],
                        features: out,
                        coord_fields: false,
                    }
                }
            }
            k => {
                let id = k.strip_prefix("gis:")?;
                let l = crate::civilgis::layers_of(&st).into_iter().find(|l| l.id == id)?;
                Built {
                    has_z: l.has_z,
                    fields: l.fields.clone(),
                    features: cc_io_civil::gis_layers::features_of(&self.project.gis_features, id).to_vec(),
                    coord_fields: false,
                }
            }
        })
    }

    /// gis-export.js `exportKinds(project)`.
    pub fn gis_export_kinds(&mut self) -> Vec<Kind> {
        let mut out: Vec<Kind> = KINDS
            .iter()
            .map(|(k, lk, g, f)| Kind {
                key: (*k).into(),
                label: t(lk, &[]),
                geom: (*g).into(),
                file: (*f).into(),
                count: self.gis_build(k).map_or(0, |b| b.features.len()),
            })
            .collect();
        for l in crate::civilgis::layers_of(&self.project.state) {
            let n = cc_io_civil::gis_layers::features_of(&self.project.gis_features, &l.id).len();
            out.push(Kind {
                key: format!("gis:{}", l.id),
                label: tr!("gexp.kind.gis", layer = l.name.as_str()),
                geom: l.geom.clone(),
                file: l.name.clone(),
                count: n,
            });
        }
        out
    }

    /// gis-export.js `exportGroups(project, keys, { toGrid })`.
    fn gis_export_groups(&mut self, keys: &[String], to_grid: &dyn Fn(f64, f64) -> [f64; 2]) -> Vec<EGroup> {
        let kinds = self.gis_export_kinds();
        let mut used: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for key in keys {
            let (Some(k), Some(b)) = (kinds.iter().find(|x| x.key == *key), self.gis_build(key)) else { continue };
            if b.features.is_empty() {
                continue;
            }
            let features: Vec<Feature> = b
                .features
                .iter()
                .map(|f| {
                    let g = f.g.as_ref().map(|g| {
                        cc_io_civil::gis_geom::map_xy(g, &mut |x, y| {
                            let q = to_grid(x, y);
                            (q[0], q[1])
                        })
                    });
                    let mut p = f.p.clone();
                    if b.coord_fields
                        && let Some(Geom::Pt(c)) = &g
                    {
                        p.insert("E".into(), fnum(c.first().copied().unwrap_or(f64::NAN)));
                        p.insert("N".into(), fnum(c.get(1).copied().unwrap_or(f64::NAN)));
                    }
                    Feature { g, p }
                })
                .collect();
            let has_z = b.has_z
                && features.iter().any(|f| match &f.g {
                    Some(Geom::Pt(c)) => c.len() > 2,
                    Some(_) => true,
                    None => false,
                });
            let kind = if key.starts_with("gis:") { safe_name(&k.file, &mut Vec::new()) } else { k.file.clone() };
            out.push(EGroup {
                key: key.clone(),
                label: k.label.clone(),
                kind,
                file: safe_name(&k.file, &mut used),
                geom: k.geom.clone(),
                has_z,
                fields: b.fields.clone(),
                features,
            });
        }
        out
    }

    /// gis-export.js `exportProject(project, keys, format, { stem })` → (file, bytes, what is said).
    pub fn gis_export(
        &mut self,
        keys: &[String],
        format: &str,
        now_ms: f64,
        zone: cc_clock::Zone,
    ) -> Result<(String, Vec<u8>, String), String> {
        crate::civilgis::ensure_catalog();
        let crs = self.project.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c))).cloned();
        if format == "geojson" && crs.is_none() {
            return Err(t("gexp.geojson.need", &[]));
        }
        if (format == "kml" || format == "kmz") && crs.is_none() {
            return Err(t("kml.need.crs", &[]));
        }
        let fr = match &crs {
            Some(c) => cc_io_civil::crs::frame(c)?,
            None => None,
        };
        let to_grid = |x: f64, y: f64| fr.as_ref().map_or([x, y], |f| f.to_grid(x, y));
        let groups = self.gis_export_groups(keys, &to_grid);
        let prj = crs.as_ref().and_then(|c| {
            cc_io_civil::crs::set_wkt_catalog();
            cc_io_civil::crs::prj_for(c)
        });
        let to_ll = |x: f64, y: f64| -> (f64, f64) {
            let c = crs.as_ref().map_or(Value::Null, Clone::clone);
            match cc_io_civil::crs::transform(&c, &Value::Null, x, y) {
                Ok(v) => {
                    let [a, b] = cc_io_civil::crs::nn(v);
                    (a, b)
                }
                Err(_) => (f64::NAN, f64::NAN),
            }
        };
        let k = crs.as_ref().and_then(|c| c.get("unit")).and_then(Value::as_str).and_then(cc_io_civil::crs::metres_per).unwrap_or(1.0);
        let stem = crate::civilexport::file_stem(self.project.state.get("name").and_then(Value::as_str).unwrap_or(""));
        let srs = crs.as_ref().and_then(|c| cc_io_civil::gis_export::srs_for(c, prj.as_deref()));
        let date = dos_date(zone.local_ms(now_ms));
        let to_ll_ref: Option<&dyn Fn(f64, f64) -> (f64, f64)> = if crs.is_some() { Some(&to_ll) } else { None };
        let mut r =
            cc_io_civil::gis_export::write_export(&groups, format, prj.as_deref(), to_ll_ref, &|z| z * k, &stem, srs.as_ref(), date)?;
        match &crs {
            None => r.notes.insert(0, t("gexp.nocrs", &[])),
            Some(c) => {
                if !["geojson", "kml", "kmz"].contains(&format) && c.get("code").is_none_or(Value::is_null) && prj.is_none() {
                    r.notes.insert(0, t("gexp.nocode", &[]));
                }
                if cc_civil::js::truthy(c.get("local")) && !["geojson", "kml", "kmz"].contains(&format) {
                    r.notes.push(tr!("gexp.grid", crs = string_of(c.get("name"))));
                }
            }
        }
        let list: Vec<String> = groups
            .iter()
            .map(|g| format!("{} ({})", g.label.to_lowercase(), cc_civil::units::grouped(g.features.len() as f64, 0)))
            .collect();
        let mut text = tr!("vec.x.done", file = r.name.as_str(), list = list.join(", "));
        if !r.notes.is_empty() {
            text.push(' ');
            text.push_str(&r.notes.join(" "));
        }
        Ok((r.name, r.bytes, text))
    }

    /// `civil.gis.export.info`: every kind with its count (0 = nothing yet).
    pub fn gis_export_info(&mut self) -> String {
        self.gis_export_kinds().iter().map(|k| format!("{} {}", k.key, k.count)).collect::<Vec<_>>().join(" | ")
    }
}

/// A ms on the local wall clock → the zip / dbf date fields (JS Date getters).
pub fn dos_date(local_ms: f64) -> cc_io_civil::zipfile::DosDate {
    let ms = if local_ms.is_finite() { local_ms } else { 0.0 };
    let days = (ms / 86_400_000.0).floor();
    let rem = ms - days * 86_400_000.0;
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let secs = (rem / 1000.0).floor() as i64;
    cc_io_civil::zipfile::DosDate {
        year: i32::try_from(if m <= 2 { y + 1 } else { y }).unwrap_or(1980),
        month: u32::try_from(m).unwrap_or(1),
        day: u32::try_from(d).unwrap_or(1),
        hour: u32::try_from(secs / 3600).unwrap_or(0),
        minute: u32::try_from((secs / 60) % 60).unwrap_or(0),
        second: u32::try_from(secs % 60).unwrap_or(0),
    }
}

/// The GIS › Export dialog state: the kinds ticked and the format.
#[derive(Clone, Debug, Default)]
pub struct GisExportDlg {
    pub keys: Vec<String>,
    pub format: String,
    pub said: String,
}

/// The dialog (gis-vector-ui.js exportDialog) as a window.
pub fn export_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = cv.gis_export_dlg.clone() else { return };
    let kinds = cv.gis_export_kinds();
    let crs = cv.project.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c))).cloned();
    let mut open = true;
    egui::Window::new(t0("vec.x.title")).id(egui::Id::unique("cv_gis_export")).open(&mut open).collapsible(false).show(ctx, |ui| {
        ui.label(egui::RichText::new(t0("vec.x.what")).strong().color(tk.text_strong));
        for k in &kinds {
            let mut on = d.keys.contains(&k.key);
            ui.add_enabled_ui(k.count > 0, |ui| {
                let label =
                    format!("{}  {}", k.label, if k.count > 0 { cc_civil::units::grouped(k.count as f64, 0) } else { t0("vec.x.none") });
                if ui.checkbox(&mut on, label).changed() {
                    acts.push(("civil.gis.export", format!("{}={}", if on { "tick" } else { "untick" }, k.key)));
                }
            });
        }
        ui.label(egui::RichText::new(t0("vec.x.as")).strong().color(tk.text_strong));
        for (v, lk) in FORMATS {
            let off = (v == "geojson" || v == "kml" || v == "kmz") && crs.is_none();
            ui.add_enabled_ui(!(v == "geojson" && off), |ui| {
                if ui.radio(d.format == v, t0(lk)).clicked() {
                    acts.push(("civil.gis.export", format!("format={v}")));
                }
            });
        }
        let coords = match &crs {
            Some(c) => {
                let label = format!(
                    "{}{}",
                    string_of(c.get("name")),
                    c.get("code").filter(|x| !x.is_null()).map_or(String::new(), |x| format!(" (EPSG:{})", cc_civil::js::to_js_string(x)))
                );
                tr!("vec.x.coords", crs = label)
            }
            None => t0("vec.x.coords.none"),
        };
        ui.label(egui::RichText::new(coords).small().color(tk.text_dim));
        ui.label(
            egui::RichText::new(t0(if cv.project.units() == "m" { "vec.x.units.m" } else { "vec.x.units.ft" })).small().color(tk.text_dim),
        );
        if !d.said.is_empty() {
            ui.label(egui::RichText::new(&d.said).small().color(tk.text));
        }
        ui.horizontal(|ui| {
            if ui.button(t0("vec.close")).clicked() {
                acts.push(("civil.gis.export", "close".into()));
            }
            let go = match d.format.as_str() {
                "kml" => "kml.go.kml",
                "kmz" => "kml.go.kmz",
                "gpkg" => "vec.x.go.gpkg",
                "geojson" => "vec.x.go.geojson",
                _ => "vec.x.go.shp",
            };
            if ui.add_enabled(!d.keys.is_empty(), egui::Button::new(t0(go))).clicked() {
                acts.push(("civil.gis.export", "go".into()));
            }
        });
    });
    if !open {
        acts.push(("civil.gis.export", "close".into()));
    }
}

impl CivilView {
    /// The dialog opened as 0.5 opens it: every kind that has something ticked (the graded contours aside), the last
    /// format (GeoJSON needs a system).
    pub fn gis_export_open(&mut self) {
        let kinds = self.gis_export_kinds();
        let keys = kinds.iter().filter(|k| k.count > 0 && !k.key.starts_with("contours-fg")).map(|k| k.key.clone()).collect();
        let crs = self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c)));
        let mut format = self.gis_export_format.clone();
        if !FORMATS.iter().any(|(f, _)| *f == format) || (format == "geojson" && !crs) {
            format = "shp".into();
        }
        self.gis_export_dlg = Some(GisExportDlg { keys, format, said: String::new() });
    }

    /// `civil.gis.export=` choices: tick=<key> | untick=<key> | format=<fmt> | kinds=<a+b+…> (comma-separated).
    pub fn gis_export_set(&mut self, spec: &str) -> Result<(), String> {
        if self.gis_export_dlg.is_none() {
            self.gis_export_open();
        }
        let kinds: Vec<String> = self.gis_export_kinds().into_iter().filter(|k| k.count > 0).map(|k| k.key).collect();
        let crs = self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c)));
        let d = self.gis_export_dlg.as_mut().ok_or("civil.gis.export: no dialog")?;
        for kv in spec.split(',').map(str::trim).filter(|x| !x.is_empty()) {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("civil.gis.export: {kv}"))?;
            match k {
                "tick" | "untick" => {
                    d.keys.retain(|x| x != v);
                    if k == "tick" {
                        if !kinds.iter().any(|x| x == v) {
                            return Err(format!("civil.gis.export: nothing to export as {v}"));
                        }
                        d.keys.push(v.to_string());
                    }
                }
                "kinds" => {
                    d.keys = v.split('+').filter(|x| kinds.iter().any(|y| y == x)).map(str::to_string).collect();
                }
                "format" => {
                    if !FORMATS.iter().any(|(f, _)| *f == v) || (v == "geojson" && !crs) {
                        return Err(format!("civil.gis.export: format {v} is not available"));
                    }
                    d.format = v.to_string();
                }
                o => return Err(format!("civil.gis.export: tick, untick, kinds or format (not {o})")),
            }
        }
        Ok(())
    }
}
