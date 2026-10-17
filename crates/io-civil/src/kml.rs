//! kml.js — KML (OGC KML 2.2) read and written. Untrusted: no entity expansion beyond XML's five (xml.rs), at most
//! `maxFeatures` placemarks, bad coordinates drop that geometry, at most 1 M vertices.

use crate::gis_geom::{ReadLayer, orient};
use crate::xml::{El, esc, parse_xml};
use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::js::{G, num_to_string, round, slice16, str_to_number};
use cc_civil::tr;
use serde_json::{Map, Value};

/// The JS KmlError message.
#[derive(Clone, Debug)]
pub struct KmlError(pub String);

const MAX_VERTS: usize = 1_000_000;

fn first<'a>(el: Option<&'a El>, name: &str) -> Option<&'a El> {
    el.and_then(|e| e.children.iter().find(|c| c.name == name))
}
fn named<'a>(el: Option<&'a El>, name: &str) -> Vec<&'a El> {
    el.map_or_else(Vec::new, |e| e.children.iter().filter(|c| c.name == name).collect())
}
fn text_in(el: Option<&El>) -> String {
    el.map_or_else(String::new, |e| cc_civil::js::trim(&e.text).to_string())
}

fn coords(text: &str, counter: &mut usize) -> Option<Vec<Pos>> {
    let mut out = Vec::new();
    for tok in split_ws(text) {
        if tok.is_empty() {
            continue;
        }
        let v: Vec<f64> = tok.split(',').map(str_to_number).collect();
        if v.len() < 2 || !v.g(0).is_finite() || !v.g(1).is_finite() || v.g(0).abs() > 180.0 || v.g(1).abs() > 90.0 {
            return None;
        }
        *counter = counter.saturating_add(1);
        if *counter > MAX_VERTS {
            return None;
        }
        out.push(if v.len() >= 3 && v.g(2).is_finite() { vec![v.g(0), v.g(1), v.g(2)] } else { vec![v.g(0), v.g(1)] });
    }
    Some(out)
}

/// `String(t).trim().split(/\s+/)`.
pub fn split_ws(s: &str) -> Vec<String> {
    cc_civil::points::split_ws(s)
}

struct Group {
    folder: String,
    kind: &'static str,
    features: Vec<Feature>,
    fields: Vec<String>,
}

fn is_num_text(v: &str) -> bool {
    cc_civil::units::is_num_str(v) && !v.chars().any(cc_civil::js::is_ws) && v == cc_civil::js::trim(v)
}

/// `readKml(text, name, { maxFeatures })` → (layers, notes).
pub fn read_kml(text: &str, name: &str, max_features: usize) -> Result<(Vec<ReadLayer>, Vec<String>), KmlError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let root = parse_xml(text, 256).map_err(|e| KmlError(tr!("kml.bad", name = name, why = e.0)))?;
    // parseXml returns the document element; `root.children.find(kml) || root`
    let kml_el = root.children.iter().find(|c| c.name == "kml").unwrap_or(&root);
    let mut notes = Vec::new();
    let mut counter = 0usize;
    let mut groups: Vec<Group> = Vec::new();
    let mut field_types: std::collections::HashMap<String, &'static str> = std::collections::HashMap::new();
    let mut bad = 0usize;
    let mut count = 0usize;
    let mut cut = false;
    let mut multi = 0usize;

    fn geoms(g: &El, out: &mut Vec<Geom>, bad: &mut usize, counter: &mut usize) {
        match g.name.as_str() {
            "Point" => match coords(&text_in(first(Some(g), "coordinates")), counter) {
                Some(c) if !c.is_empty() => out.push(Geom::Pt(c.into_iter().next().unwrap_or_default())),
                _ => *bad = bad.saturating_add(1),
            },
            "LineString" | "LinearRing" => match coords(&text_in(first(Some(g), "coordinates")), counter) {
                Some(c) if c.len() >= 2 => out.push(Geom::Ln(vec![c])),
                _ => *bad = bad.saturating_add(1),
            },
            "Polygon" => {
                let ring = |b: Option<&El>, counter: &mut usize| coords(&text_in(first(first(b, "LinearRing"), "coordinates")), counter);
                let outer = ring(first(Some(g), "outerBoundaryIs"), counter);
                match outer {
                    Some(o) if o.len() >= 4 => {
                        let mut poly = vec![o];
                        for h in named(Some(g), "innerBoundaryIs") {
                            if let Some(r) = ring(Some(h), counter)
                                && r.len() >= 4
                            {
                                poly.push(r);
                            }
                        }
                        out.push(Geom::Pg(vec![poly]));
                    }
                    _ => *bad = bad.saturating_add(1),
                }
            }
            "MultiGeometry" => {
                for c in &g.children {
                    geoms(c, out, bad, counter);
                }
            }
            "Track" => {
                let c: Vec<Pos> = named(Some(g), "coord")
                    .into_iter()
                    .map(|x| split_ws(&text_in(Some(x))).iter().map(|t| str_to_number(t)).collect::<Vec<f64>>())
                    .filter(|v| v.len() >= 2 && v.iter().all(|x| x.is_finite()))
                    .map(|v| if v.len() >= 3 { vec![v.g(0), v.g(1), v.g(2)] } else { vec![v.g(0), v.g(1)] })
                    .collect();
                if c.len() >= 2 {
                    out.push(Geom::Ln(vec![c]));
                } else {
                    *bad = bad.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    let mut placemark = |pm: &El,
                         folder: &str,
                         groups: &mut Vec<Group>,
                         bad: &mut usize,
                         counter: &mut usize,
                         cut: &mut bool,
                         count: &mut usize,
                         multi: &mut usize| {
        if *cut {
            return;
        }
        let mut props: Vec<(String, String)> = Vec::new();
        let set = |props: &mut Vec<(String, String)>, k: String, v: String| match props.iter_mut().find(|(a, _)| *a == k) {
            Some(slot) => slot.1 = v,
            None => props.push((k, v)),
        };
        let nm = text_in(first(Some(pm), "name"));
        if !nm.is_empty() {
            set(&mut props, "name".into(), nm);
        }
        let ds = strip_markup(&text_in(first(Some(pm), "description")));
        if !ds.is_empty() {
            set(&mut props, "description".into(), slice16(&ds, 10000));
        }
        let ext = first(Some(pm), "ExtendedData");
        for d in named(ext, "Data") {
            let k = slice16(d.attr("name").unwrap_or(""), 64);
            if !k.is_empty() && k != "__proto__" {
                set(&mut props, k, slice16(&text_in(first(Some(d), "value")), 10000));
            }
        }
        for sd in named(ext, "SchemaData") {
            for d in named(Some(sd), "SimpleData") {
                let k = slice16(d.attr("name").unwrap_or(""), 64);
                if !k.is_empty() && k != "__proto__" {
                    set(&mut props, k, slice16(&text_in(Some(d)), 10000));
                }
            }
        }
        let mut out = Vec::new();
        for c in &pm.children {
            geoms(c, &mut out, bad, counter);
        }
        let n_out = out.len();
        for g in out {
            *count = count.saturating_add(1);
            if *count > max_features {
                *cut = true;
                return;
            }
            let flat2d = match &g {
                Geom::Pt(c) => c.len() < 3 || c.g(2) == 0.0,
                Geom::Ln(c) => c.first().is_none_or(|r| r.iter().all(|p| p.len() < 3 || p.g(2) == 0.0)),
                // the JS tests the rings, not their points (`g.c.flat()`): a polygon keeps its z as read
                Geom::Pg(c) => c.iter().flatten().all(|r| r.len() < 3),
            };
            let kind = g.kind();
            let geom = if flat2d { strip_z(&g) } else { g };
            let gi = match groups.iter().position(|x| x.folder == folder && x.kind == kind) {
                Some(i) => i,
                None => {
                    groups.push(Group { folder: folder.to_string(), kind, features: Vec::new(), fields: Vec::new() });
                    groups.len().saturating_sub(1)
                }
            };
            let mut p = Map::new();
            // JS object key order: array-index keys first
            for (k, v) in &props {
                p.insert(k.clone(), Value::String(v.clone()));
            }
            if let Some(gr) = groups.get_mut(gi) {
                gr.features.push(Feature { g: Some(geom), p });
                let ordered = order_keys(&props);
                for (k, v) in ordered {
                    if !gr.fields.contains(&k) {
                        gr.fields.push(k.clone());
                    }
                    let key = format!("{}|{}|{}", gr.folder, gr.kind, k);
                    let ty = field_types.get(&key).copied();
                    field_types.insert(
                        key,
                        if ty == Some("text") {
                            "text"
                        } else if is_num_text(&v) {
                            "number"
                        } else {
                            "text"
                        },
                    );
                }
            }
        }
        if pm.children.iter().any(|c| c.name == "MultiGeometry") && n_out > 1 {
            *multi = multi.saturating_add(1);
        }
    };

    // walk with an explicit stack (the XML depth is capped at 256 already)
    #[allow(clippy::type_complexity)]
    fn walk(
        el: &El,
        folder: &str,
        f: &mut dyn FnMut(&El, &str, &mut Vec<Group>, &mut usize, &mut usize, &mut bool, &mut usize, &mut usize),
        st: &mut (Vec<Group>, usize, usize, bool, usize, usize),
    ) {
        for c in &el.children {
            if st.3 {
                return;
            }
            if c.name == "Placemark" {
                f(c, folder, &mut st.0, &mut st.1, &mut st.2, &mut st.3, &mut st.4, &mut st.5);
            } else if c.name == "Folder" || c.name == "Document" {
                let n = text_in(first(Some(c), "name"));
                let next = if n.is_empty() { folder.to_string() } else { n };
                walk(c, &next, f, st);
            }
        }
    }
    let mut st = (std::mem::take(&mut groups), bad, counter, cut, count, multi);
    walk(kml_el, name, &mut placemark, &mut st);
    (groups, bad, counter, cut, count, multi) = st;
    let _ = (counter, count);

    let kind_word = |k: &str| match k {
        "pt" => tr!("kml.kind.pt"),
        "ln" => tr!("kml.kind.ln"),
        _ => tr!("kml.kind.pg"),
    };
    let folders: Vec<String> = groups.iter().map(|g| g.folder.clone()).collect();
    let mut layers = Vec::new();
    for mut l in groups {
        let same_folder = folders.iter().filter(|f| **f == l.folder).count() > 1;
        let fields: Vec<Field> = l
            .fields
            .iter()
            .map(|k| Field {
                name: k.clone(),
                ty: field_types.get(&format!("{}|{}|{}", l.folder, l.kind, k)).copied().unwrap_or("text").to_string(),
            })
            .collect();
        for f in &mut l.features {
            for fd in &fields {
                if fd.ty == "number"
                    && let Some(Value::String(s)) = f.p.get(&fd.name)
                {
                    let x = str_to_number(s);
                    f.p.insert(fd.name.clone(), crate::gjs::num_keep(x));
                }
            }
        }
        let has = l.features.iter().any(|f| f.g.as_ref().is_some_and(crate::gis_geom::has_z));
        layers.push(ReadLayer {
            name: if same_folder { tr!("kml.layer", folder = l.folder.as_str(), kind = kind_word(l.kind)) } else { l.folder.clone() },
            geom: l.kind.into(),
            has_z: has,
            fields,
            features: l.features,
            notes: Vec::new(),
            extra: Vec::new(),
        });
    }
    if bad > 0 {
        notes.push(tr!("kml.bad.geom", n = bad));
    }
    if cut {
        notes.push(tr!("kml.cut", n = cc_civil::units::grouped(max_features as f64, 0)));
    }
    if multi > 0 {
        notes.push(tr!("kml.multi", n = multi));
    }
    Ok((layers, notes))
}

/// The props in JS object key order (array-index keys first, ascending).
fn order_keys(props: &[(String, String)]) -> Vec<(String, String)> {
    let mut idx: Vec<(String, String)> = props.iter().filter(|(k, _)| crate::gjs::is_index(k)).cloned().collect();
    idx.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(0));
    idx.extend(props.iter().filter(|(k, _)| !crate::gjs::is_index(k)).cloned());
    idx
}

/// `.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim()`.
fn strip_markup(s: &str) -> String {
    let mut a = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        a.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        match tail.find('>') {
            Some(j) => {
                a.push(' ');
                rest = tail.get(j.saturating_add(1)..).unwrap_or("");
            }
            None => {
                a.push_str(tail);
                rest = "";
            }
        }
    }
    a.push_str(rest);
    let mut out = String::with_capacity(a.len());
    let mut ws = false;
    for c in a.chars() {
        if cc_civil::js::is_ws(c) {
            if !ws {
                out.push(' ');
            }
            ws = true;
        } else {
            out.push(c);
            ws = false;
        }
    }
    cc_civil::js::trim(&out).to_string()
}

fn strip_z(g: &Geom) -> Geom {
    let s = |p: &Pos| p.iter().take(2).copied().collect::<Pos>();
    match g {
        Geom::Pt(c) => Geom::Pt(s(c)),
        Geom::Ln(c) => Geom::Ln(c.iter().map(|r| r.iter().map(s).collect()).collect()),
        Geom::Pg(c) => Geom::Pg(c.iter().map(|poly| poly.iter().map(|r| r.iter().map(s).collect()).collect()).collect()),
    }
}

// ---------------------------------------------------------------------------- writing

fn f9(v: f64) -> String {
    num_to_string(round(v * 1e9) / 1e9)
}

/// A KML group `{kind, hasZ, features}`.
pub struct KGroup<'a> {
    pub kind: &'a str,
    pub has_z: bool,
    pub features: &'a [Feature],
}

/// `writeKml(groups, { toLonLat, zToMetres, name })`.
pub fn write_kml(groups: &[KGroup], to_lon_lat: &dyn Fn(f64, f64) -> (f64, f64), zm: &dyn Fn(f64) -> f64, name: &str) -> String {
    let mut out: Vec<String> = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(),
        "<kml xmlns=\"http://www.opengis.net/kml/2.2\">".into(),
        "<Document>".into(),
        format!("<name>{}</name>", esc(name)),
    ];
    let pos = |p: &Pos, z: bool| {
        let (lon, lat) = to_lon_lat(p.g(0), p.g(1));
        let mut s = format!("{},{}", f9(lon), f9(lat));
        if z && p.len() > 2 && p.g(2).is_finite() {
            s.push(',');
            s.push_str(&num_to_string(round(zm(p.g(2)) * 1e4) / 1e4));
        }
        s
    };
    let ring = |r: &Vec<Pos>, z: bool| r.iter().map(|p| pos(p, z)).collect::<Vec<_>>().join(" ");
    for g in groups {
        out.push("<Folder>".into());
        out.push(format!("<name>{}</name>", esc(g.kind)));
        let z = g.has_z;
        for (i, f) in g.features.iter().enumerate() {
            let Some(geo) = &f.g else { continue };
            let keys = crate::gjs::js_keys(&f.p);
            let name_field = keys.iter().find(|k| k.eq_ignore_ascii_case("name"));
            let label = match name_field {
                Some(k) => match f.p.get(k.as_str()) {
                    None | Some(Value::Null) => String::new(),
                    Some(v) => cc_civil::js::to_js_string(v),
                },
                None => format!("{} {}", g.kind, i.saturating_add(1)),
            };
            out.push("<Placemark>".into());
            out.push(format!("<name>{}</name>", esc(&label)));
            let ext: Vec<(&String, &Value)> =
                keys.iter().filter_map(|k| f.p.get(k.as_str()).map(|v| (*k, v))).filter(|(_, v)| !v.is_null()).collect();
            if !ext.is_empty() {
                out.push("<ExtendedData>".into());
                for (k, v) in ext {
                    out.push(format!("<Data name=\"{}\"><value>{}</value></Data>", esc(k), esc(&cc_civil::js::to_js_string(v))));
                }
                out.push("</ExtendedData>".into());
            }
            let alt = if z { "<altitudeMode>absolute</altitudeMode>" } else { "" };
            match geo {
                Geom::Pt(c) => out.push(format!("<Point>{alt}<coordinates>{}</coordinates></Point>", pos(c, z))),
                Geom::Ln(c) => {
                    let parts: Vec<String> =
                        c.iter().map(|r| format!("<LineString>{alt}<coordinates>{}</coordinates></LineString>", ring(r, z))).collect();
                    out.push(if parts.len() > 1 {
                        format!("<MultiGeometry>{}</MultiGeometry>", parts.join(""))
                    } else {
                        parts.into_iter().next().unwrap_or_else(|| "undefined".into())
                    });
                }
                Geom::Pg(_) => {
                    let Geom::Pg(o) = orient(geo, true) else { continue };
                    let polys: Vec<String> = o
                        .iter()
                        .map(|poly| {
                            let outer = poly.first().map(|r| ring(r, z)).unwrap_or_default();
                            let holes: String = poly
                                .iter()
                                .skip(1)
                                .map(|h| format!("<innerBoundaryIs><LinearRing><coordinates>{}</coordinates></LinearRing></innerBoundaryIs>", ring(h, z)))
                                .collect();
                            format!("<Polygon>{alt}<outerBoundaryIs><LinearRing><coordinates>{outer}</coordinates></LinearRing></outerBoundaryIs>{holes}</Polygon>")
                        })
                        .collect();
                    out.push(if polys.len() > 1 {
                        format!("<MultiGeometry>{}</MultiGeometry>", polys.join(""))
                    } else {
                        polys.into_iter().next().unwrap_or_else(|| "undefined".into())
                    });
                }
            }
            out.push("</Placemark>".into());
        }
        out.push("</Folder>".into());
    }
    out.push("</Document>".into());
    out.push("</kml>".into());
    format!("{}\n", out.join("\n"))
}
