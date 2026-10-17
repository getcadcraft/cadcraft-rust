//! landxml.js — LandXML 1.2 in and out: units, coordinate system, CgPoints, surfaces (TIN definition, breaklines,
//! outer boundary), alignments (lines and arcs; spirals named and skipped), profiles (PVIs, parabolic curves, surface
//! profiles), pipe networks. LandXML writes northing first ("N E Z"); here x = easting, y = northing. The reader takes
//! hostile input: nesting is capped by the XML reader, counts by the input's own size, and anything not handled is
//! named in `notes`, never guessed.

use crate::xml::{El, esc, kid, kids, parse_xml, text_of};
use cc_civil::alignment::{Alignment, Element};
use cc_civil::geo::P2;
use cc_civil::i18n::t;
use cc_civil::js::{J, JsMath, hypot, max, min_of, num_to_string, pad_start, slice16, str_to_number, to_fixed};
use cc_civil::pipes::Pipe;
use cc_civil::points::{Pt, group_key};
use cc_civil::profile::{Profile, StaZ};
use cc_civil::tin::{Breakline, Surface};
use cc_civil::tr;
use cc_civil::units::metres_per;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::f64::consts::PI;

const NS: &str = "http://www.landxml.org/schema/LandXML-1.2";

/// `num(v, d)`: fixed decimals, trailing zeros dropped, tiny values 0, non-finite '0'.
pub fn num(v: f64, d: usize) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let lim = 0.5 * 10f64.jpow(-(d as f64));
    let s = to_fixed(if v.abs() < lim { 0.0 } else { v }, d);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}
fn ne(p: P2, d: usize) -> String {
    format!("{} {}", num(p.y, d), num(p.x, d))
}
fn nez(x: f64, y: f64, z: f64, d: usize) -> String {
    format!("{} {} {}", num(y, d), num(x, d), num(z, d))
}

/// A surface to write.
pub struct WSurface<'a> {
    pub name: String,
    pub desc: String,
    pub surface: Option<&'a Surface>,
    pub breaklines: Vec<Breakline>,
    pub boundary: Option<Vec<P2>>,
}
/// An alignment to write.
pub struct WAlignment<'a> {
    pub name: String,
    pub alignment: Option<&'a Alignment>,
    pub eg: Option<(String, Vec<StaZ>)>,
    pub profile: Option<(String, &'a Profile)>,
}
/// A structure to write (pipes.js shape plus what the page shows).
#[derive(Clone, Debug)]
pub struct WStruct {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub dia: f64,
    pub rim_shown: Option<f64>,
    pub sump: Option<f64>,
}
/// A network to write.
pub struct WNetwork {
    pub name: String,
    pub ty: String,
    pub structures: Vec<WStruct>,
    pub pipes: Vec<Pipe>,
}

/// Civil date parts (UTC) of a time value in ms: (year, month 1–12, day, hours, minutes, seconds).
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
pub fn date_parts(ms: f64) -> (i64, i64, i64, i64, i64, i64) {
    let ms = ms.trunc();
    let days = (ms / 86_400_000.0).floor();
    let rem = ms - days * 86_400_000.0;
    let secs = (rem / 1000.0).floor();
    let h = (secs / 3600.0).floor();
    let mi = ((secs - h * 3600.0) / 60.0).floor();
    let s = secs - h * 3600.0 - mi * 60.0;
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm)
    let z = cc_civil::js::i64_of(days).unwrap_or(0).saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let f = |v: f64| cc_civil::js::i64_of(v).unwrap_or(0);
    (y, m, d, f(h), f(mi), f(s))
}

/// `writeLandXml({ name, units, points, surfaces, alignments, networks, date, crs })`.
#[allow(clippy::too_many_arguments, clippy::arithmetic_side_effects)]
pub fn write_land_xml(
    name: &str,
    units: &str,
    points: &[Pt],
    surfaces: &[WSurface],
    alignments: &[WAlignment],
    networks: &[WNetwork],
    date_ms: f64,
    crs: Option<&Value>,
) -> String {
    let mut l: Vec<String> = Vec::new();
    let pad = |x: i64| pad_start(&x.to_string(), 2, '0');
    let (yy, mo, dd, hh, mi, ss) = date_parts(date_ms);
    let ds = format!("{}-{}-{}", yy, pad(mo), pad(dd));
    let ts = format!("{}:{}:{}", pad(hh), pad(mi), pad(ss));
    l.push("<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into());
    l.push(format!(
        "<LandXML xmlns=\"{NS}\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"{NS} http://www.landxml.org/schema/LandXML-1.2/LandXML-1.2.xsd\" version=\"1.2\" date=\"{ds}\" time=\"{ts}\">"
    ));
    l.push("  <Units>".into());
    if units == "m" {
        l.push("    <Metric areaUnit=\"squareMeter\" linearUnit=\"meter\" volumeUnit=\"cubicMeter\" temperatureUnit=\"celsius\" pressureUnit=\"milliBars\" diameterUnit=\"millimeter\" angularUnit=\"decimal degrees\" directionUnit=\"decimal degrees\"/>".into());
    } else {
        l.push("    <Imperial areaUnit=\"squareFoot\" linearUnit=\"USSurveyFoot\" volumeUnit=\"cubicYard\" temperatureUnit=\"fahrenheit\" pressureUnit=\"inHG\" diameterUnit=\"inch\" angularUnit=\"decimal degrees\" directionUnit=\"decimal degrees\"/>".into());
    }
    l.push("  </Units>".into());
    if let Some(crs) = crs.filter(|c| cc_civil::js::truthy(c.get("name"))) {
        let lo = crs.get("local").filter(|v| cc_civil::js::truthy(Some(v)));
        let nn = |v: Option<&Value>, d: usize| num(cc_civil::js::numf(v), d);
        let desc = match lo {
            Some(lo) => {
                let rot = cc_civil::js::numf(lo.get("rotation"));
                format!(
                    "Coordinates are {} coordinates, not grid: grid = ({}, {}) + {} × rotate {}° × (point − ({}, {}))",
                    if rot != 0.0 && !rot.is_nan() { "local (assumed)" } else { "ground" },
                    nn(lo.get("to").and_then(|a| a.get(0)), 4),
                    nn(lo.get("to").and_then(|a| a.get(1)), 4),
                    nn(lo.get("scale"), 10),
                    nn(lo.get("rotation"), 8),
                    nn(lo.get("from").and_then(|a| a.get(0)), 4),
                    nn(lo.get("from").and_then(|a| a.get(1)), 4)
                )
            }
            None => String::new(),
        };
        let code = crs.get("code").filter(|c| !c.is_null());
        let proj4 = crs.get("proj4").and_then(Value::as_str).unwrap_or("");
        let up = proj4.get(..7).map(str::to_ascii_uppercase).unwrap_or_default();
        let wkt = if code.is_none() && (up.starts_with("PROJCS[") || up.starts_with("GEOGCS[")) { proj4 } else { "" };
        let mut s = format!("  <CoordinateSystem name=\"{}\"", esc(&cc_civil::js::string_or_empty(crs.get("name"))));
        if let Some(c) = code.filter(|c| cc_civil::js::truthy(Some(c))) {
            s.push_str(&format!(" epsgCode=\"{}\"", esc(&cc_civil::js::to_js_string(c))));
        }
        if !wkt.is_empty() {
            s.push_str(&format!(" ogcWktCode=\"{}\"", esc(wkt)));
        }
        if !desc.is_empty() {
            s.push_str(&format!(" desc=\"{}\"", esc(&desc)));
        }
        s.push_str("/>");
        l.push(s);
    }
    l.push(format!("  <Project name=\"{}\"/>", esc(name)));
    l.push(
        "  <Application name=\"CivilCraft\" manufacturer=\"CadCraft\" version=\"lab\" manufacturerURL=\"https://getcadcraft.com\"/>".into(),
    );

    if !points.is_empty() {
        l.push("  <CgPoints>".into());
        for p in points {
            let code = group_key(&p.d);
            let mut attrs = format!("name=\"{}\"", esc(&p.n));
            if !p.d.is_empty() {
                attrs.push_str(&format!(" desc=\"{}\" code=\"{}\"", esc(&p.d), esc(&code)));
            }
            let c = match p.z {
                None => ne(P2 { x: p.x, y: p.y }, 6),
                Some(z) => nez(p.x, p.y, z, 6),
            };
            l.push(format!("    <CgPoint {attrs}>{c}</CgPoint>"));
        }
        l.push("  </CgPoints>".into());
    }

    if !alignments.is_empty() {
        l.push(format!("  <Alignments name=\"{}\">", esc(name)));
        for a in alignments {
            let Some(al) = a.alignment.filter(|al| al.ok()) else { continue };
            l.push(format!("    <Alignment name=\"{}\" length=\"{}\" staStart=\"{}\">", esc(&a.name), num(al.length, 6), num(al.start, 6)));
            l.push("      <CoordGeom>".into());
            for e in &al.elements {
                match e {
                    Element::Line { p0, p1, sta, len, .. } => l.push(format!(
                        "        <Line staStart=\"{}\" length=\"{}\"><Start>{}</Start><End>{}</End></Line>",
                        num(*sta, 6),
                        num(*len, 6),
                        ne(*p0, 6),
                        ne(*p1, 6)
                    )),
                    Element::Arc(e) => l.push(format!(
                        "        <Curve rot=\"{}\" crvType=\"arc\" radius=\"{}\" length=\"{}\" staStart=\"{}\" chord=\"{}\" delta=\"{}\" tangent=\"{}\" external=\"{}\" midOrd=\"{}\"><Start>{}</Start><Center>{}</Center><End>{}</End><PI>{}</PI></Curve>",
                        if e.ccw { "ccw" } else { "cw" },
                        num(e.r, 6),
                        num(e.len, 6),
                        num(e.sta, 6),
                        num(e.chord, 6),
                        num(e.delta * 180.0 / PI, 8),
                        num(e.t, 6),
                        num(e.e, 6),
                        num(e.m, 6),
                        ne(e.p0, 6),
                        ne(e.c, 6),
                        ne(e.p1, 6),
                        ne(e.pi, 6)
                    )),
                }
            }
            l.push("      </CoordGeom>".into());
            let eg_ok = a.eg.as_ref().is_some_and(|(_, p)| !p.is_empty());
            let fg_ok = a.profile.as_ref().is_some_and(|(_, p)| !p.pvis.is_empty());
            if eg_ok || fg_ok {
                l.push(format!("      <Profile name=\"{}\">", esc(&a.name)));
                if let Some((egname, pts)) = a.eg.as_ref().filter(|_| eg_ok) {
                    let mut runs: Vec<Vec<StaZ>> = Vec::new();
                    let mut cur: Vec<StaZ> = Vec::new();
                    for p in pts {
                        if p.z.is_none() {
                            if !cur.is_empty() {
                                runs.push(std::mem::take(&mut cur));
                            }
                        } else {
                            cur.push(*p);
                        }
                    }
                    if !cur.is_empty() {
                        runs.push(cur);
                    }
                    for r in runs.iter().filter(|r| r.len() >= 2) {
                        let list: Vec<String> =
                            r.iter().map(|p| format!("{} {}", num(p.sta, 4), num(p.z.unwrap_or(f64::NAN), 4))).collect();
                        l.push(format!("        <ProfSurf name=\"{}\"><PntList2D>{}</PntList2D></ProfSurf>", esc(egname), list.join(" ")));
                    }
                }
                if let Some((fname, prof)) = a.profile.as_ref().filter(|_| fg_ok) {
                    l.push(format!("        <ProfAlign name=\"{}\">", esc(fname)));
                    let n = prof.pvis.len();
                    for (i, v) in prof.pvis.iter().enumerate() {
                        let lv = v.l.unwrap_or(f64::NAN);
                        let curve = i > 0 && i + 1 < n && lv > 0.0;
                        l.push(if curve {
                            format!("          <ParaCurve length=\"{}\">{} {}</ParaCurve>", num(lv, 6), num(v.sta, 6), num(v.z, 6))
                        } else {
                            format!("          <PVI>{} {}</PVI>", num(v.sta, 6), num(v.z, 6))
                        });
                    }
                    l.push("        </ProfAlign>".into());
                }
                l.push("      </Profile>".into());
            }
            l.push("    </Alignment>".into());
        }
        l.push("  </Alignments>".into());
    }

    if !surfaces.is_empty() {
        l.push(format!("  <Surfaces name=\"{}\">", esc(name)));
        for s in surfaces {
            let Some(sf) = s.surface else { continue };
            let st = sf.stats();
            l.push(format!(
                "    <Surface name=\"{}\"{}>",
                esc(&s.name),
                if s.desc.is_empty() { String::new() } else { format!(" desc=\"{}\"", esc(&s.desc)) }
            ));
            let bls: Vec<&Breakline> = s.breaklines.iter().filter(|b| b.pts.len() >= 2).collect();
            let bnd = s.boundary.as_ref().filter(|b| b.len() >= 3);
            if !bls.is_empty() || bnd.is_some() {
                l.push("      <SourceData>".into());
                if !bls.is_empty() {
                    l.push("        <Breaklines>".into());
                    for b in &bls {
                        let list: Vec<String> = b.pts.iter().map(|p| nez(p.x, p.y, p.z, 6)).collect();
                        l.push(format!(
                            "          <Breakline brkType=\"standard\" name=\"{}\"><PntList3D>{}</PntList3D></Breakline>",
                            esc(&b.name),
                            list.join(" ")
                        ));
                    }
                    l.push("        </Breaklines>".into());
                }
                if let Some(bnd) = bnd {
                    l.push("        <Boundaries>".into());
                    let mut ring: Vec<String> = bnd.iter().map(|p| ne(*p, 6)).collect();
                    if let Some(p0) = bnd.first() {
                        ring.push(ne(*p0, 6));
                    }
                    l.push(format!(
                        "          <Boundary name=\"Outer\" bndType=\"outer\" edgeTrim=\"true\"><PntList2D>{}</PntList2D></Boundary>",
                        ring.join(" ")
                    ));
                    l.push("        </Boundaries>".into());
                }
                l.push("      </SourceData>".into());
            }
            let (pts, faces) = sf.to_faces();
            l.push(format!(
                "      <Definition surfType=\"TIN\" area2DSurf=\"{}\" area3DSurf=\"{}\" elevMax=\"{}\" elevMin=\"{}\">",
                num(st.area2d, 4),
                num(st.area3d, 4),
                num(st.max_z, 6),
                num(st.min_z, 6)
            ));
            l.push("        <Pnts>".into());
            for (i, p) in pts.iter().enumerate() {
                l.push(format!("          <P id=\"{}\">{}</P>", i.saturating_add(1), nez(p.x, p.y, p.z, 6)));
            }
            l.push("        </Pnts>".into());
            l.push("        <Faces>".into());
            for f in &faces {
                l.push(format!("          <F>{} {} {}</F>", u64::from(f[0]) + 1, u64::from(f[1]) + 1, u64::from(f[2]) + 1));
            }
            l.push("        </Faces>".into());
            l.push("      </Definition>".into());
            l.push("    </Surface>".into());
        }
        l.push("  </Surfaces>".into());
    }
    for n in networks {
        let used: Vec<&str> = n.pipes.iter().flat_map(|p| [p.from.as_str(), p.to.as_str()]).collect();
        let structs: Vec<&WStruct> = n.structures.iter().filter(|x| used.contains(&x.id.as_str())).collect();
        let pipes: Vec<&Pipe> =
            n.pipes.iter().filter(|p| p.inv_up.is_some_and(f64::is_finite) && p.inv_dn.is_some_and(f64::is_finite)).collect();
        if structs.len() < 2 || pipes.is_empty() {
            continue;
        }
        // Map(id → name): a later structure with the same id wins
        let mut s_name: HashMap<&str, &str> = HashMap::new();
        for x in &structs {
            s_name.insert(x.id.as_str(), x.name.as_str());
        }
        l.push(format!("  <PipeNetworks name=\"{}\">", esc(name)));
        l.push(format!(
            "    <PipeNetwork name=\"{}\" pipeNetType=\"{}\">",
            esc(&n.name),
            if n.ty == "sanitary" { "sanitary" } else { "storm" }
        ));
        l.push("      <Structs>".into());
        for x in &structs {
            let mut inv = Vec::new();
            for p in &pipes {
                if p.from == x.id {
                    inv.push(format!(
                        "<Invert elev=\"{}\" flowDir=\"out\" refPipe=\"{}\"/>",
                        num(p.inv_up.unwrap_or(f64::NAN), 6),
                        esc(&p.name)
                    ));
                }
                if p.to == x.id {
                    inv.push(format!(
                        "<Invert elev=\"{}\" flowDir=\"in\" refPipe=\"{}\"/>",
                        num(p.inv_dn.unwrap_or(f64::NAN), 6),
                        esc(&p.name)
                    ));
                }
            }
            if inv.is_empty() {
                inv.push(format!("<Invert elev=\"{}\" flowDir=\"both\" refPipe=\"\"/>", num(x.sump.unwrap_or(0.0), 6)));
            }
            let rim = x.rim_shown.filter(|v| v.is_finite()).map_or(String::new(), |r| format!(" elevRim=\"{}\"", num(r, 6)));
            let sump = x.sump.filter(|v| v.is_finite()).map_or(String::new(), |s| format!(" elevSump=\"{}\"", num(s, 6)));
            l.push(format!(
                "        <Struct name=\"{}\"{rim}{sump}><Center>{}</Center><CircStruct diameter=\"{}\"/>{}</Struct>",
                esc(&x.name),
                ne(P2 { x: x.x, y: x.y }, 6),
                num(x.dia, 6),
                inv.join("")
            ));
        }
        l.push("      </Structs>".into());
        l.push("      <Pipes>".into());
        for p in &pipes {
            let a = structs.iter().find(|x| x.id == p.from);
            let b = structs.iter().find(|x| x.id == p.to);
            let len = match (a, b) {
                (Some(a), Some(b)) => hypot(b.x - a.x, b.y - a.y),
                _ => f64::NAN,
            };
            let slope = if len != 0.0 && !len.is_nan() { (p.inv_up.unwrap_or(f64::NAN) - p.inv_dn.unwrap_or(f64::NAN)) / len } else { 0.0 };
            l.push(format!(
                "        <Pipe name=\"{}\" refStart=\"{}\" refEnd=\"{}\" length=\"{}\" slope=\"{}\"><CircPipe diameter=\"{}\"/></Pipe>",
                esc(&p.name),
                esc(s_name.get(p.from.as_str()).copied().unwrap_or("")),
                esc(s_name.get(p.to.as_str()).copied().unwrap_or("")),
                num(len, 6),
                num(slope, 8),
                num(p.dia, 6)
            ));
        }
        l.push("      </Pipes>".into());
        l.push("    </PipeNetwork>".into());
        l.push("  </PipeNetworks>".into());
    }
    l.push("</LandXML>".into());
    format!("{}\n", l.join("\n"))
}

// ---------------------------------------------------------------------------

/// `nums(s)`: numbers split on white space and commas (`Number` of each piece).
fn nums(s: &str) -> Vec<f64> {
    let t = cc_civil::js::trim(s);
    if t.is_empty() {
        return Vec::new();
    }
    t.split(|c: char| cc_civil::js::is_ws(c) || c == ',').map(str_to_number).collect()
}

/// The reader's result (JSON as the JS objects).
pub type Read = Value;

/// The safety caps of the reader (counts are also bounded by the text itself).
pub const MAX_TEXT: usize = 512 * 1024 * 1024;

/// `readLandXml(text)` → the data, or the error message the JS throws.
#[allow(clippy::arithmetic_side_effects)]
pub fn read_land_xml(text: &str) -> Result<Read, String> {
    if text.len() > MAX_TEXT {
        return Err(t("xml.err.none", &[]));
    }
    let root = parse_xml(text, 256).map_err(|e| e.0)?;
    if root.name != "LandXML" {
        return Err(tr!("lx.err.root", tag = format!("<{}>", root.name)));
    }
    let mut notes: Vec<String> = Vec::new();
    let note = |notes: &mut Vec<String>, m: String| {
        if !notes.contains(&m) {
            notes.push(m);
        }
    };
    let r = Some(&root);
    let mut units = Value::Null;
    let mut linear = Value::Null;
    if let Some(u) = kid(r, "Units")
        && let Some(ue) = kid(Some(u), "Imperial").or_else(|| kid(Some(u), "Metric"))
    {
        let lu = ue
            .attr("linearUnit")
            .filter(|s| !s.is_empty())
            .map_or_else(|| if ue.name == "Metric" { "meter".to_string() } else { "foot".to_string() }, str::to_string);
        let mp = metres_per(&lu);
        let un = if mp.is_some_and(|m| m > 0.5) { "m" } else { "ft" };
        if mp.is_none() {
            note(&mut notes, tr!(if un == "m" { "lx.unit.m" } else { "lx.unit.ft" }, unit = lu.as_str()));
        }
        units = Value::String(un.into());
        linear = Value::String(lu);
    }
    let name = kid(r, "Project").and_then(|p| p.attr("name")).filter(|s| !s.is_empty()).unwrap_or("").to_string();
    let mut cs_v = Value::Null;
    if let Some(cs) = kid(r, "CoordinateSystem") {
        let a = |k: &str| cs.attr(k).filter(|s| !s.is_empty());
        let code = epsg_code(a("epsgCode").unwrap_or(""));
        let wkt = cc_civil::js::trim(a("ogcWktCode").unwrap_or("")).to_string();
        let nm = slice16(
            a("name").or_else(|| a("projectedCoordinateSystemName")).or_else(|| a("horizontalCoordinateSystemName")).unwrap_or(""),
            200,
        );
        let desc = slice16(a("desc").unwrap_or(""), 500);
        let wkt_v = if !wkt.is_empty() && wkt.encode_utf16().count() <= 20000 { Value::String(wkt) } else { Value::Null };
        if code.is_some() || !wkt_v.is_null() || !nm.is_empty() {
            cs_v = json!({ "name": nm, "epsgCode": code.map(|c| c.j()), "ogcWktCode": wkt_v, "desc": desc });
        }
    }
    // points (CgPoints nest)
    let mut points: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stack: Vec<&El> = kids(r, "CgPoints").into_iter().rev().collect();
    // depth-first in document order: a CgPoints is walked where it appears
    fn walk<'a>(el: &'a El, out: &mut Vec<&'a El>, depth: usize) {
        if depth > 300 {
            return;
        }
        for c in &el.children {
            if c.name == "CgPoints" {
                walk(c, out, depth + 1);
            } else if c.name == "CgPoint" {
                out.push(c);
            }
        }
    }
    let mut cg: Vec<&El> = Vec::new();
    while let Some(el) = stack.pop() {
        walk(el, &mut cg, 0);
    }
    for c in cg {
        if c.attr("pntRef").is_some_and(|s| !s.is_empty()) && text_of(Some(c)).is_empty() {
            note(&mut notes, t("lx.pntref", &[]));
            continue;
        }
        let v = nums(&text_of(Some(c)));
        if v.len() < 2 || v.iter().any(|x| !x.is_finite()) {
            note(&mut notes, t("lx.pts.bad", &[]));
            continue;
        }
        let n = c
            .attr("name")
            .filter(|s| !s.is_empty())
            .or_else(|| c.attr("oID").filter(|s| !s.is_empty()))
            .map_or_else(|| (points.len() + 1).to_string(), str::to_string);
        if seen.contains(&n) {
            note(&mut notes, t("lx.pts.twice", &[]));
            continue;
        }
        seen.insert(n.clone());
        let d = slice16(c.attr("desc").filter(|s| !s.is_empty()).or_else(|| c.attr("code").filter(|s| !s.is_empty())).unwrap_or(""), 200);
        let g = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        points.push(json!({ "n": n, "x": g(1).j(), "y": g(0).j(), "z": if v.len() >= 3 { g(2).j() } else { Value::Null }, "d": d }));
    }
    // surfaces
    let mut surfaces: Vec<Value> = Vec::new();
    for ss in kids(r, "Surfaces") {
        for s in kids(Some(ss), "Surface") {
            let def = kid(Some(s), "Definition");
            let sname = s.attr("name").filter(|x| !x.is_empty()).map_or_else(|| t("lx.def.surface", &[]), str::to_string);
            let sdesc = s.attr("desc").unwrap_or("").to_string();
            if let Some(st) = def.and_then(|d| d.attr("surfType")).filter(|x| !x.is_empty() && *x != "TIN") {
                note(&mut notes, tr!("lx.surf.type", name = sname.as_str(), r#type = st));
                continue;
            }
            let src = kid(Some(s), "SourceData");
            let mut bls: Vec<Value> = Vec::new();
            for bs in kids(src, "Breaklines") {
                for b in kids(Some(bs), "Breakline") {
                    let Some(l3) = kid(Some(b), "PntList3D") else {
                        note(&mut notes, t("lx.bl.no3d", &[]));
                        continue;
                    };
                    let v = nums(&text_of(Some(l3)));
                    let mut pts = Vec::new();
                    let mut i = 0usize;
                    while i + 2 < v.len() {
                        let g = |k: usize| v.get(k).copied().unwrap_or(f64::NAN);
                        pts.push(json!({ "x": g(i + 1).j(), "y": g(i).j(), "z": g(i + 2).j() }));
                        i += 3;
                    }
                    if pts.len() >= 2 {
                        let bn = b
                            .attr("name")
                            .filter(|x| !x.is_empty())
                            .or_else(|| b.attr("desc").filter(|x| !x.is_empty()))
                            .map_or_else(|| t("lx.def.breakline", &[]), str::to_string);
                        bls.push(json!({ "name": bn, "pts": pts }));
                    }
                }
            }
            let mut boundary = Value::Null;
            for bs in kids(src, "Boundaries") {
                for b in kids(Some(bs), "Boundary") {
                    if b.attr("bndType").filter(|x| !x.is_empty()).unwrap_or("outer") != "outer" {
                        note(&mut notes, t("lx.bndy.outer", &[]));
                        continue;
                    }
                    let l2 = kid(Some(b), "PntList2D");
                    let l3 = kid(Some(b), "PntList3D");
                    let v = nums(&text_of(l2.or(l3)));
                    let k = if l2.is_some() { 2 } else { 3 };
                    let mut pts: Vec<(f64, f64)> = Vec::new();
                    let mut i = 0usize;
                    while i + k - 1 < v.len() {
                        pts.push((v.get(i + 1).copied().unwrap_or(f64::NAN), v.get(i).copied().unwrap_or(f64::NAN)));
                        i += k;
                    }
                    if pts.len() > 1 {
                        let (f0, fl) = (pts.first().copied().unwrap_or_default(), pts.last().copied().unwrap_or_default());
                        if hypot(f0.0 - fl.0, f0.1 - fl.1) < 1e-9 {
                            pts.pop();
                        }
                    }
                    if pts.len() >= 3 {
                        boundary = Value::Array(pts.iter().map(|(x, y)| json!({ "x": x.j(), "y": y.j() })).collect());
                    }
                }
            }
            let mut pts: Vec<Value> = Vec::new();
            let mut faces: Vec<Value> = Vec::new();
            if let Some(def) = def {
                let mut id: HashMap<String, usize> = HashMap::new();
                for pnts in kids(Some(def), "Pnts") {
                    for p in kids(Some(pnts), "P") {
                        let v = nums(&text_of(Some(p)));
                        if v.len() < 3 || v.iter().any(|x| !x.is_finite()) {
                            continue;
                        }
                        // a P without an id is keyed by `undefined` in the JS Map: no face token can name it
                        id.insert(p.attr("id").map_or_else(|| "\u{0}undefined".to_string(), str::to_string), pts.len());
                        let g = |k: usize| v.get(k).copied().unwrap_or(f64::NAN);
                        pts.push(json!({ "x": g(1).j(), "y": g(0).j(), "z": g(2).j() }));
                    }
                }
                let mut hidden = 0usize;
                for fs in kids(Some(def), "Faces") {
                    for f in kids(Some(fs), "F") {
                        if f.attr("i") == Some("1") {
                            hidden += 1;
                            continue;
                        }
                        let tx = text_of(Some(f));
                        let ids = cc_civil::points::split_ws(&tx);
                        if ids.len() < 3 {
                            continue;
                        }
                        let tri: Vec<Value> = ids.iter().take(3).map(|k| id.get(k).map_or(json!(-1), |&i| json!(i))).collect();
                        faces.push(Value::Array(tri));
                    }
                }
                if hidden > 0 {
                    note(&mut notes, tr!("lx.faces.hidden", n = hidden));
                }
            }
            if faces.is_empty() && bls.is_empty() {
                note(&mut notes, tr!("lx.surf.nofaces", name = sname.as_str()));
                continue;
            }
            surfaces.push(json!({ "name": sname, "desc": sdesc, "pts": pts, "faces": faces, "breaklines": bls, "boundary": boundary }));
        }
    }
    // alignments
    let mut aligns: Vec<Value> = Vec::new();
    for als in kids(r, "Alignments") {
        for a in kids(Some(als), "Alignment") {
            let aname = a.attr("name").filter(|x| !x.is_empty()).map_or_else(|| t("scene.al.default", &[]), str::to_string);
            let Some(cg) = kid(Some(a), "CoordGeom") else {
                note(&mut notes, tr!("lx.al.nogeom", name = aname.as_str()));
                continue;
            };
            let els: Vec<&El> =
                cg.children.iter().filter(|c| ["Line", "Curve", "Spiral", "IrregularLine", "Chain"].contains(&c.name.as_str())).collect();
            if els.iter().any(|e| e.name != "Line" && e.name != "Curve") {
                note(&mut notes, tr!("lx.al.spirals", name = aname.as_str()));
                continue;
            }
            let pt = |el: &El, tag: &str| -> Option<P2> {
                let v = nums(&text_of(kid(Some(el), tag)));
                (v.len() >= 2).then(|| P2 { x: v.get(1).copied().unwrap_or(f64::NAN), y: v.first().copied().unwrap_or(f64::NAN) })
            };
            let mut pis: Vec<(f64, f64, f64)> = Vec::new();
            let mut ok = true;
            for (i, e) in els.iter().enumerate() {
                let (Some(s), Some(en)) = (pt(e, "Start"), pt(e, "End")) else {
                    ok = false;
                    continue;
                };
                if i == 0 {
                    pis.push((s.x, s.y, 0.0));
                }
                if e.name == "Curve" {
                    let c = pt(e, "Center");
                    let mut pi = pt(e, "PI");
                    let rr = cc_civil::js::str_to_number(e.attr("radius").unwrap_or(""));
                    let rr = if e.attr("radius").is_none() { f64::NAN } else { rr };
                    let r = if rr != 0.0 && !rr.is_nan() { rr } else { c.map_or(0.0, |c| hypot(s.x - c.x, s.y - c.y)) };
                    if pi.is_none()
                        && let Some(c) = c
                    {
                        pi = tangents_meet(s, en, c);
                    }
                    let Some(pi) = pi else {
                        ok = false;
                        continue;
                    };
                    pis.push((pi.x, pi.y, r));
                } else if els.get(i + 1).is_some_and(|n| n.name == "Line") {
                    pis.push((en.x, en.y, 0.0));
                }
                if i + 1 == els.len() {
                    pis.push((en.x, en.y, 0.0));
                }
            }
            if !ok || pis.len() < 2 {
                note(&mut notes, tr!("lx.al.unread", name = aname.as_str()));
                continue;
            }
            let start = cc_civil::js::number_or0(a.attr("staStart").map(|s| Value::String(s.to_string())).as_ref());
            let mut profiles: Vec<Value> = Vec::new();
            let mut surf_profiles: Vec<Value> = Vec::new();
            for pr in kids(Some(a), "Profile") {
                for pa in kids(Some(pr), "ProfAlign") {
                    let mut pvis: Vec<Value> = Vec::new();
                    for c in &pa.children {
                        let v = nums(&text_of(Some(c)));
                        let g = |k: usize| v.get(k).copied().unwrap_or(f64::NAN);
                        if c.name == "PVI" && v.len() >= 2 {
                            pvis.push(json!({ "sta": g(0).j(), "z": g(1).j(), "L": 0 }));
                        } else if c.name == "ParaCurve" && v.len() >= 2 {
                            let lv = cc_civil::js::number_or0(c.attr("length").map(|s| Value::String(s.to_string())).as_ref());
                            pvis.push(json!({ "sta": g(0).j(), "z": g(1).j(), "L": lv.j() }));
                        } else if c.name == "CircCurve" || c.name == "UnsymParaCurve" {
                            note(&mut notes, t("lx.vc.other", &[]));
                            if v.len() >= 2 {
                                pvis.push(json!({ "sta": g(0).j(), "z": g(1).j(), "L": 0 }));
                            }
                        }
                    }
                    if pvis.len() >= 2 {
                        let pn = pa.attr("name").filter(|x| !x.is_empty()).map_or_else(|| t("lx.def.profile", &[]), str::to_string);
                        profiles.push(json!({ "name": pn, "pvis": pvis }));
                    }
                }
                for ps in kids(Some(pr), "ProfSurf") {
                    let v = nums(&text_of(kid(Some(ps), "PntList2D")));
                    let mut pts = Vec::new();
                    let mut i = 0usize;
                    while i + 1 < v.len() {
                        pts.push(
                            json!({ "sta": v.get(i).copied().unwrap_or(f64::NAN).j(), "z": v.get(i + 1).copied().unwrap_or(f64::NAN).j() }),
                        );
                        i += 2;
                    }
                    if !pts.is_empty() {
                        let pn = ps.attr("name").filter(|x| !x.is_empty()).map_or_else(|| t("lx.def.surface", &[]), str::to_string);
                        surf_profiles.push(json!({ "name": pn, "pts": pts }));
                    }
                }
            }
            let pis_v: Vec<Value> = pis.iter().map(|(x, y, r)| json!({ "x": x.j(), "y": y.j(), "r": r.j() })).collect();
            aligns.push(json!({ "name": aname, "start": start.j(), "pis": pis_v, "profiles": profiles, "surfProfiles": surf_profiles }));
        }
    }
    // pipe networks
    let mut networks: Vec<Value> = Vec::new();
    for pns in kids(r, "PipeNetworks") {
        for pn in kids(Some(pns), "PipeNetwork") {
            let nname = pn.attr("name").filter(|x| !x.is_empty()).map_or_else(|| t("lx.def.network", &[]), str::to_string);
            let ptype = pn.attr("pipeNetType");
            if !matches!(ptype, Some("storm" | "sanitary")) {
                match ptype.filter(|x| !x.is_empty()) {
                    Some(ty) => note(&mut notes, tr!("lx.net.type", name = nname.as_str(), r#type = ty)),
                    None => note(&mut notes, tr!("lx.net.untyped", name = nname.as_str())),
                }
            }
            struct S {
                id: String,
                name: String,
                x: f64,
                y: f64,
                rim: Option<f64>,
                sump_elev: Option<f64>,
                dia: f64,
                sump_depth: f64,
            }
            let mut structs: Vec<S> = Vec::new();
            let mut by_name: HashMap<String, usize> = HashMap::new();
            let mut inv: HashMap<String, f64> = HashMap::new();
            let mut k = 0usize;
            for sts in kids(Some(pn), "Structs") {
                for st in kids(Some(sts), "Struct") {
                    let c = nums(&text_of(kid(Some(st), "Center")));
                    if c.len() < 2 {
                        note(&mut notes, t("lx.st.nocentre", &[]));
                        continue;
                    }
                    let shape = kid(Some(st), "CircStruct")
                        .or_else(|| kid(Some(st), "RectStruct"))
                        .or_else(|| kid(Some(st), "InletStruct"))
                        .or_else(|| kid(Some(st), "OutletStruct"));
                    k += 1;
                    let sname = st.attr("name").filter(|x| !x.is_empty()).map_or_else(|| format!("S{k}"), str::to_string);
                    let rim = st.attr("elevRim").map(str_to_number).filter(|v| v.is_finite());
                    let sump_elev = st.attr("elevSump").map(str_to_number);
                    let dia = shape.and_then(|s| s.attr("diameter")).map_or(f64::NAN, str_to_number);
                    let rec = S {
                        id: format!("s{k}"),
                        name: sname.clone(),
                        x: c.get(1).copied().unwrap_or(f64::NAN),
                        y: c.first().copied().unwrap_or(f64::NAN),
                        rim,
                        sump_elev,
                        dia: if dia != 0.0 && !dia.is_nan() { dia } else { 0.0 },
                        sump_depth: 0.0,
                    };
                    if shape.is_some_and(|s| s.name != "CircStruct") {
                        note(&mut notes, t("lx.st.round", &[]));
                    }
                    by_name.insert(sname.clone(), structs.len());
                    structs.push(rec);
                    for iv in kids(Some(st), "Invert") {
                        let key = format!(
                            "{}|{}|{}",
                            sname,
                            iv.attr("refPipe").unwrap_or("undefined"),
                            iv.attr("flowDir").unwrap_or("undefined")
                        );
                        inv.insert(key, iv.attr("elev").map_or(f64::NAN, str_to_number));
                    }
                }
            }
            let mut pipes: Vec<Value> = Vec::new();
            let mut pipe_inv: Vec<(String, String, Option<f64>, Option<f64>)> = Vec::new();
            let mut q = 0usize;
            for ps in kids(Some(pn), "Pipes") {
                for p in kids(Some(ps), "Pipe") {
                    let a = p.attr("refStart").and_then(|n| by_name.get(n)).and_then(|&i| structs.get(i));
                    let b = p.attr("refEnd").and_then(|n| by_name.get(n)).and_then(|&i| structs.get(i));
                    let (Some(a), Some(b)) = (a, b) else {
                        note(&mut notes, t("lx.pipe.missing", &[]));
                        continue;
                    };
                    let circ = kid(Some(p), "CircPipe");
                    if circ.is_none() {
                        note(&mut notes, t("lx.pipe.round", &[]));
                    }
                    let pname = p.attr("name").filter(|x| !x.is_empty()).map_or_else(|| format!("P{}", q + 1), str::to_string);
                    let up =
                        inv.get(&format!("{}|{}|out", a.name, pname)).or_else(|| inv.get(&format!("{}|{}|both", a.name, pname))).copied();
                    let dn =
                        inv.get(&format!("{}|{}|in", b.name, pname)).or_else(|| inv.get(&format!("{}|{}|both", b.name, pname))).copied();
                    q += 1;
                    let dia = circ.and_then(|c| c.attr("diameter")).map_or(f64::NAN, str_to_number);
                    let up = up.filter(|v| v.is_finite());
                    let dn = dn.filter(|v| v.is_finite());
                    pipe_inv.push((a.id.clone(), b.id.clone(), up, dn));
                    pipes.push(json!({
                        "id": format!("q{q}"), "name": pname, "from": a.id, "to": b.id, "dia": if dia != 0.0 && !dia.is_nan() { dia.j() } else { json!(0) },
                        "invUp": up.map(|v| v.j()), "invDn": dn.map(|v| v.j()),
                    }));
                }
            }
            for st in &mut structs {
                let mut v = Vec::new();
                for (from, to, up, dn) in &pipe_inv {
                    if *from == st.id
                        && let Some(u) = up
                    {
                        v.push(*u);
                    }
                    if *to == st.id
                        && let Some(d) = dn
                    {
                        v.push(*d);
                    }
                }
                if let Some(se) = st.sump_elev.filter(|x| x.is_finite())
                    && !v.is_empty()
                {
                    st.sump_depth = max(0.0, min_of(v.iter().copied()) - se);
                }
            }
            if !pipes.is_empty() {
                let ss: Vec<Value> = structs
                    .iter()
                    .map(|s| json!({ "id": s.id, "name": s.name, "x": s.x.j(), "y": s.y.j(), "rim": s.rim.map(|r| r.j()), "dia": s.dia.j(), "sumpDepth": s.sump_depth.j() }))
                    .collect();
                networks.push(json!({ "name": nname, "type": if ptype == Some("sanitary") { "sanitary" } else { "storm" }, "structures": ss, "pipes": pipes }));
            }
        }
    }
    for k in ["Parcels", "PlanFeatures", "Roadways", "GradeModel", "Survey"] {
        if kid(r, k).is_some() {
            note(&mut notes, tr!("lx.notyet", what = k));
        }
    }
    let mut out = Map::new();
    out.insert("units".into(), units);
    out.insert("linearUnit".into(), linear);
    out.insert("name".into(), Value::String(name));
    out.insert("points".into(), Value::Array(points));
    out.insert("surfaces".into(), Value::Array(surfaces));
    out.insert("alignments".into(), Value::Array(aligns));
    out.insert("notes".into(), Value::Array(notes.into_iter().map(Value::String).collect()));
    out.insert("coordinateSystem".into(), cs_v);
    out.insert("networks".into(), Value::Array(networks));
    let _ = num_to_string;
    Ok(Value::Object(out))
}

/// `/^\s*(?:EPSG:)?(\d{3,6})\s*$/i` → the code.
fn epsg_code(s: &str) -> Option<f64> {
    let t = cc_civil::js::trim_start(s);
    let t = match t.get(..5) {
        Some(p) if p.eq_ignore_ascii_case("EPSG:") => t.get(5..).unwrap_or(""),
        _ => t,
    };
    let n = t.bytes().take_while(u8::is_ascii_digit).count();
    let rest = t.get(n..).unwrap_or("");
    ((3..=6).contains(&n) && cc_civil::js::trim(rest).is_empty()).then(|| str_to_number(t.get(..n).unwrap_or("")))
}

/// The point where the tangents at a curve's start and end meet (from its centre).
fn tangents_meet(s: P2, e: P2, c: P2) -> Option<P2> {
    let a = P2 { x: -(s.y - c.y), y: s.x - c.x };
    let b = P2 { x: -(e.y - c.y), y: e.x - c.x };
    let den = a.x * b.y - a.y * b.x;
    if !(den.abs() >= 1e-12) {
        return None;
    }
    let t = ((e.x - s.x) * b.y - (e.y - s.y) * b.x) / den;
    Some(P2 { x: s.x + a.x * t, y: s.y + a.y * t })
}

/// `scaleImport(d, k)`: every length of a read file times k (a file in the other unit, converted on import).
#[allow(clippy::arithmetic_side_effects)]
pub fn scale_import(mut d: Read, k: f64) -> Read {
    use cc_civil::js::{jnum, number};
    if k == 1.0 {
        return d;
    }
    let mul = |v: Option<&Value>| jnum(number(v) * k);
    let p = |q: &Value| -> Value {
        if !cc_civil::js::truthy(Some(q)) {
            return q.clone();
        }
        let mut m = q.as_object().cloned().unwrap_or_default();
        m.insert("x".into(), mul(q.get("x")));
        m.insert("y".into(), mul(q.get("y")));
        if q.get("z").is_some_and(|z| !z.is_null()) {
            m.insert("z".into(), mul(q.get("z")));
        }
        Value::Object(m)
    };
    let map_arr = |v: Option<&mut Value>, f: &dyn Fn(&Value) -> Value| {
        if let Some(Value::Array(a)) = v {
            for x in a.iter_mut() {
                *x = f(x);
            }
        }
    };
    map_arr(d.get_mut("points"), &p);
    if let Some(Value::Array(ss)) = d.get_mut("surfaces") {
        for s in ss.iter_mut() {
            map_arr(s.get_mut("pts"), &p);
            map_arr(s.get_mut("breaklines"), &|b: &Value| {
                let mut b = b.clone();
                map_arr(b.get_mut("pts"), &p);
                b
            });
            if let Some(bd) = s.get_mut("boundary")
                && cc_civil::js::truthy(Some(bd))
            {
                map_arr(Some(bd), &p);
            }
        }
    }
    if let Some(Value::Array(als)) = d.get_mut("alignments") {
        for a in als.iter_mut() {
            let st = mul(a.get("start"));
            if let Some(m) = a.as_object_mut() {
                m.insert("start".into(), st);
            }
            map_arr(a.get_mut("pis"), &|q: &Value| {
                let mut o = p(q);
                if let Some(m) = o.as_object_mut() {
                    m.insert("r".into(), mul(q.get("r")));
                }
                o
            });
            if let Some(Value::Array(prs)) = a.get_mut("profiles") {
                for pr in prs.iter_mut() {
                    map_arr(
                        pr.get_mut("pvis"),
                        &|v: &Value| serde_json::json!({ "sta": mul(v.get("sta")), "z": mul(v.get("z")), "L": mul(v.get("L")) }),
                    );
                }
            }
            if let Some(Value::Array(ps)) = a.get_mut("surfProfiles") {
                for pr in ps.iter_mut() {
                    map_arr(pr.get_mut("pts"), &|v: &Value| serde_json::json!({ "sta": mul(v.get("sta")), "z": mul(v.get("z")) }));
                }
            }
        }
    }
    if let Some(Value::Array(ns)) = d.get_mut("networks") {
        for n in ns.iter_mut() {
            let or_null = |v: Option<&Value>| if v.is_some_and(Value::is_null) { Value::Null } else { mul(v) };
            map_arr(n.get_mut("structures"), &|x: &Value| {
                let mut m = x.as_object().cloned().unwrap_or_default();
                m.insert("x".into(), mul(x.get("x")));
                m.insert("y".into(), mul(x.get("y")));
                m.insert("rim".into(), or_null(x.get("rim")));
                m.insert("sumpDepth".into(), mul(x.get("sumpDepth")));
                Value::Object(m)
            });
            map_arr(n.get_mut("pipes"), &|x: &Value| {
                let mut m = x.as_object().cloned().unwrap_or_default();
                m.insert("invUp".into(), or_null(x.get("invUp")));
                m.insert("invDn".into(), or_null(x.get("invDn")));
                Value::Object(m)
            });
        }
    }
    d
}
