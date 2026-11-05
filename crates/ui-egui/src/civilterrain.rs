//! Elevation grids and LiDAR into CivilCraft, surfaces out as GeoTIFF (gis-terrain-ui.js): the import dialog — the
//! file's facts (size, cells or points, its system, extent in latitude / longitude, z range, LiDAR classes with counts),
//! the coordinate systems (the file's offered to a project without one, or asked through the picker), the area (the
//! project's extents and a margin, what the plan shows, the whole file), thin to a spacing (LiDAR: nearest the cell's
//! centre, lowest, average), the classes, the z unit, as survey points or (a grid) the surface itself — and the
//! "Surface as a GeoTIFF" export (its own dialog, and a section of GIS › Export). The maths is cc-io-civil's
//! (raster.rs, las.rs, thin.rs, terrain.rs, geotiff_write.rs), at parity with the 0.5 JS.
//!
//! Commands: `civil.terrain=<action>` (see its doc), `civil.terrain.info`, `civil.geotiff=<action>`, `civil.geotiff.info`.

use cc_civil::js::{num_to_string, str_to_number, string_of, to_fixed, truthy};
use cc_civil::tr;
use cc_civil::units::{fixed, grouped};
use cc_io_civil::las::{self, Header};
use cc_io_civil::raster::{self, InFile, Src};
use cc_io_civil::terrain::{self, BoxXY, limits};
use cc_io_civil::thin::{self, Coverage};
use egui::RichText;
use serde_json::{Value, json};

use crate::civilcrs::{Ask, AskKind, PickFor};
use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// What was opened.
#[derive(Clone)]
pub enum Source {
    Grid(Box<Src>),
    Lidar(Box<(InFile, Header)>),
}

/// The import dialog (gis-terrain-ui.js importDialog's `st` and what it read).
#[derive(Clone)]
pub struct TerrainDlg {
    pub src: Source,
    pub src_crs: Value,
    pub src_chosen: bool,
    pub adopt: bool,
    pub new_crs: Option<Value>,
    pub area: String,
    pub margin: String,
    pub spacing: String,
    pub spacing_touched: bool,
    pub mode: String,
    pub classes: Option<Vec<f64>>,
    pub z_unit: Option<String>,
    pub as_: String,
    pub class_counts: Option<Vec<(f64, f64)>>,
    pub coverage: Option<Coverage>,
    pub scanned: bool,
    pub z_range: Option<(f64, f64, bool)>,
    pub file_box: BoxXY,
    pub box_ok: bool,
    pub ll_text: String,
    /// the progress line (a failure, a cancel)
    pub msg: String,
    /// what was asked before the surface is replaced
    pending: Option<std::sync::Arc<(terrain::GridTin, Option<Value>, f64)>>,
}

const UNITS: [&str; 3] = ["m", "ft", "ftUS"];

fn unit_word(u: &str) -> String {
    terrain::unit_words(u).unwrap_or_default()
}
fn crs_text(c: &Value) -> String {
    if !truthy(Some(c)) {
        return t0("tui.crs.not_stated");
    }
    let code = c.get("code").filter(|v| truthy(Some(v)));
    format!("{}{}", string_of(c.get("name")), code.map(|v| format!(" (EPSG:{})", cc_civil::js::to_js_string(v))).unwrap_or_default())
}
fn fmt_lat(lat: f64) -> String {
    format!("{}° {}", to_fixed(lat.abs(), 5), if lat >= 0.0 { "N" } else { "S" })
}
fn fmt_lon(lon: f64) -> String {
    format!("{}° {}", to_fixed(lon.abs(), 5), if lon >= 0.0 { "E" } else { "W" })
}
fn num(n: f64) -> String {
    grouped(n, 0)
}
/// `+(x).toPrecision(6)`.
fn prec6(x: f64) -> f64 {
    str_to_number(&cc_io_civil::dxf::to_precision(x, 6))
}

/// `shiftWarning(a, b)`: how well the shift between two datums is known.
fn shift_warning(a: &Value, b: &Value) -> Option<String> {
    let datum = |c: &Value| string_of(c.get("name")).split(" / ").next().unwrap_or("").to_string();
    if !truthy(Some(a)) || !truthy(Some(b)) || datum(a) == datum(b) {
        return None;
    }
    // undefined (no code / no entry) | null (no accuracy) | a number
    let acc = |c: &Value| -> Result<Option<f64>, ()> {
        if datum(c) == "WGS 84" {
            return Ok(Some(0.0));
        }
        match c.get("code").filter(|v| truthy(Some(v))).and_then(|v| cc_io_civil::crs::by_code_val(Some(v)).ok().flatten()) {
            Some(e) => match e.shift_accuracy {
                Some(x) => Ok(Some(x)),
                None => Err(()),
            },
            None => Ok(None),
        }
    };
    let w = [acc(a), acc(b)];
    let (da, db) = (datum(a), datum(b));
    if w.iter().any(Result::is_err) {
        return Some(tr!("tui.shift.unknown", a = da.as_str(), b = db.as_str()));
    }
    let m = w.iter().filter_map(|x| x.ok().flatten()).filter(|x| x.is_finite()).fold(0.0_f64, f64::max);
    if m > 1.0 {
        Some(if m <= 4.0 {
            tr!("tui.shift.few", a = da.as_str(), b = db.as_str())
        } else {
            tr!("tui.shift.about", a = da.as_str(), b = db.as_str(), m = m)
        })
    } else {
        None
    }
}

/// compute(): the plan, or why Import cannot run.
pub struct Plan {
    pub ok: bool,
    pub msg: String,
    pub text: String,
    pub count: f64,
    win: Option<terrain::Win>,
    step: f64,
    cell_p: f64,
    p_box: Option<BoxXY>,
    s_box: Option<BoxXY>,
}

/// Whether a TIFF is an elevation grid (one band of 16+ bits or floats) rather than a picture.
pub fn is_elevation_tiff(bytes: &[u8]) -> bool {
    match raster::probe_tiff(bytes) {
        Ok(p) => p.get("image") != Some(&Value::Bool(true)),
        Err(_) => true,
    }
}

impl CivilView {
    fn pc(&self) -> Value {
        self.project.state.get("crs").filter(|c| truthy(Some(c))).cloned().unwrap_or(Value::Null)
    }
    fn u_word(&self) -> &'static str {
        if self.project.units() == "m" { "m" } else { "ft" }
    }

    /// A terrain file opened (gis-terrain-ui.js openFiles): read, then the dialog.
    pub fn terrain_open(&mut self, name: &str, bytes: Vec<u8>) {
        let t_open = web_time::Instant::now();
        crate::civilgis::ensure_catalog();
        let lower = name.to_ascii_lowercase();
        let file = InFile { name: name.to_string(), bytes };
        let src = if lower.ends_with(".las") || lower.ends_with(".laz") {
            match las::read_las_header(&file) {
                Ok(h) => Source::Lidar(Box::new((file, h))),
                Err(e) => {
                    self.message = tr!("tui.read_failed", name = name, error = e);
                    return;
                }
            }
        } else {
            match raster::open_raster(std::slice::from_ref(&file)) {
                Ok(s) => Source::Grid(Box::new(s)),
                Err(e) => {
                    self.message = tr!("tui.read_failed", name = name, error = e);
                    return;
                }
            }
        };
        let units = self.project.units().to_string();
        let pc = self.pc();
        let (src_crs, z_unit) = match &src {
            Source::Grid(s) => (s.crs.clone(), s.z_unit),
            Source::Lidar(b) => (b.1.crs.clone(), b.1.z_unit),
        };
        let has_src = truthy(Some(&src_crs));
        let ext = crate::civilplan::extents(&mut self.project);
        let file_box = match &src {
            Source::Grid(s) => {
                let g = s.gt;
                let mut b = BoxXY { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
                for (c, r) in [(0.0, 0.0), (s.width, 0.0), (s.width, s.height), (0.0, s.height)] {
                    let (x, y) = (g[0] + c * g[1] + r * g[2], g[3] + c * g[4] + r * g[5]);
                    b.min_x = b.min_x.min(x);
                    b.max_x = b.max_x.max(x);
                    b.min_y = b.min_y.min(y);
                    b.max_y = b.max_y.max(y);
                }
                b
            }
            Source::Lidar(b) => BoxXY { min_x: b.1.min[0], min_y: b.1.min[1], max_x: b.1.max[0], max_y: b.1.max[1] },
        };
        let box_ok = [file_box.min_x, file_box.min_y, file_box.max_x, file_box.max_y].iter().all(|v| v.is_finite())
            && file_box.max_x >= file_box.min_x
            && file_box.max_y >= file_box.min_y;
        let mut ll_text = "—".to_string();
        if box_ok
            && let Ok(Some(ll)) = terrain::lon_lat_of(&src_crs)
            && let (Ok(a), Ok(b)) = (ll(file_box.min_x, file_box.min_y), ll(file_box.max_x, file_box.max_y))
            && a.iter().chain(b.iter()).all(|v| v.is_finite())
        {
            ll_text = tr!("tui.ll.range", lat1 = fmt_lat(a[1]), lon1 = fmt_lon(a[0]), lat2 = fmt_lat(b[1]), lon2 = fmt_lon(b[0]));
        }
        let z_range = match &src {
            Source::Lidar(b) => {
                let h = &b.1;
                (h.min[2].is_finite() && h.max[2].is_finite() && h.max[2] >= h.min[2]).then_some((h.min[2], h.max[2], false))
            }
            Source::Grid(_) => None,
        };
        let mut d = TerrainDlg {
            src,
            src_crs: src_crs.clone(),
            src_chosen: has_src,
            adopt: !truthy(Some(&pc)) && has_src && cc_io_civil::crs::crs_problem(&src_crs, &units).is_none(),
            new_crs: None,
            area: if ext.is_some() { "extents".into() } else { "all".into() },
            margin: if units == "m" { "30".into() } else { "100".into() },
            spacing: String::new(),
            spacing_touched: false,
            mode: "nearest".into(),
            classes: None,
            z_unit: z_unit.map(str::to_string),
            as_: "points".into(),
            class_counts: None,
            coverage: None,
            scanned: false,
            z_range,
            file_box,
            box_ok,
            ll_text,
            msg: String::new(),
            pending: None,
        };
        // the grid's z range from a sample, the LiDAR file's classes (the JS reads them at once, in the background)
        match &mut d.src {
            Source::Grid(s) => {
                if let Ok(Some(st)) = raster::sample_stats(s)
                    && let (Some(lo), Some(hi)) = (st.min, st.max)
                {
                    d.z_range = Some((lo, hi, st.sampled));
                }
            }
            Source::Lidar(b) => {
                let (file, h) = &mut **b;
                let mut cov = d.box_ok.then(|| Coverage::new(d.file_box, 128));
                let mut counts: Vec<(f64, f64)> = Vec::new();
                let r = las::scan_points(file, h, &mut |x, y, _z, c| match cov.as_mut() {
                    Some(cv) => cv.add(x, y, c),
                    None => match counts.iter_mut().find(|(k, _)| *k == c) {
                        Some(e) => e.1 += 1.0,
                        None => counts.push((c, 1.0)),
                    },
                });
                match r {
                    Err(e) => d.msg = tr!("tui.points.read_failed", error = e),
                    Ok(_) => {
                        let cc = cov.as_ref().map_or(counts, |c| c.counts().to_vec());
                        let mut has: Vec<f64> = cc.iter().map(|x| x.0).collect();
                        has.sort_by(f64::total_cmp);
                        d.classes = Some(if cc.iter().any(|x| x.0 == 2.0) { vec![2.0] } else { has });
                        d.class_counts = Some(cc);
                        d.coverage = cov;
                        d.scanned = true;
                    }
                }
            }
        }
        self.gis3.terrain = Some(Box::new(d));
        self.timings.insert("terrain open", t_open.elapsed().as_secs_f64() * 1000.0);
        let sp = self.terrain_default_spacing();
        if let Some(d) = self.gis3.terrain.as_mut() {
            d.spacing = num_to_string(sp);
        }
    }

    fn eff_crs(&self) -> Value {
        let pc = self.pc();
        if truthy(Some(&pc)) {
            return pc;
        }
        let Some(d) = &self.gis3.terrain else { return Value::Null };
        if d.adopt { d.src_crs.clone() } else { d.new_crs.clone().unwrap_or(Value::Null) }
    }
    /// renderCrs: the lines and whether Import is blocked (crsBlock).
    pub fn terrain_crs_lines(&self) -> (Vec<String>, Option<String>, Vec<&'static str>) {
        let Some(d) = &self.gis3.terrain else { return (Vec::new(), None, Vec::new()) };
        let pc = self.pc();
        let units = self.project.units();
        let s = &d.src_crs;
        let name = || match &d.src {
            Source::Grid(g) => g.name.clone(),
            Source::Lidar(b) => b.1.name.clone(),
        };
        let _ = name;
        let mut out = Vec::new();
        let mut buttons = Vec::new();
        let mut block = None;
        if !truthy(Some(s)) && !d.src_chosen {
            out.push(if truthy(Some(&pc)) { tr!("tui.crs.file_none", crs = crs_text(&pc)) } else { t0("tui.crs.none_both") });
            buttons.push("pickfile");
        } else if truthy(Some(&pc)) {
            let same = terrain::to_project(s, &pc).map(|x| x.1).unwrap_or(false);
            out.push(if same {
                tr!("tui.crs.same", crs = crs_text(&pc))
            } else {
                tr!(
                    if truthy(pc.get("local")) { "tui.crs.convert_local" } else { "tui.crs.convert" },
                    from = crs_text(s),
                    to = crs_text(&pc)
                )
            });
            if !same && let Some(w) = shift_warning(s, &pc) {
                out.push(w);
            }
        } else {
            let prob = cc_io_civil::crs::crs_problem(s, units);
            if prob.is_none() {
                out.push(format!("[{}] {}", if d.adopt { "x" } else { " " }, tr!("tui.crs.use", crs = crs_text(s))));
                if !d.adopt {
                    out.push(t0("tui.crs.not_ticked"));
                }
            } else if let Some(nc) = &d.new_crs {
                out.push(tr!("tui.crs.convert_new", from = crs_text(s), to = crs_text(nc)));
                if let Some(w) = shift_warning(s, nc) {
                    out.push(w);
                }
            } else {
                let b = if s.get("unit").and_then(Value::as_str) == Some("deg") {
                    t0(if units == "m" { "tui.crs.deg.m" } else { "tui.crs.deg.ft" })
                } else {
                    prob.unwrap_or_default()
                };
                out.push(b.clone());
                block = Some(b);
                buttons.push("pickproj");
            }
        }
        (out, block, buttons)
    }
    /// project units per file unit near the file's middle.
    fn terrain_unit_scale(&self) -> f64 {
        let Some(d) = &self.gis3.terrain else { return 1.0 };
        let Ok((f, _)) = terrain::to_project(&d.src_crs, &self.eff_crs()) else { return 1.0 };
        let b = d.file_box;
        let c = if d.box_ok { [(b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0] } else { [0.0, 0.0] };
        let dd = match &d.src {
            Source::Grid(s) => s.gt[1].hypot(s.gt[4]),
            Source::Lidar(_) => {
                let v = ((b.max_x - b.min_x) / 1000.0).max(1e-6);
                if v == 0.0 || v.is_nan() { 1.0 } else { v }
            }
        };
        if let (Ok(a), Ok(bb)) = (f(c[0], c[1]), f(c[0] + dd, c[1])) {
            let k = (bb[0] - a[0]).hypot(bb[1] - a[1]) / dd;
            if k.is_finite() && k > 0.0 {
                return k;
            }
        }
        1.0
    }
    /// areaBoxes(): (project box, file box) — None = the whole file.
    fn terrain_area_boxes(&mut self) -> (Option<BoxXY>, Option<BoxXY>) {
        let Some(d) = &self.gis3.terrain else { return (None, None) };
        let (area, margin) = (d.area.clone(), d.margin.clone());
        if area == "all" {
            return (None, None);
        }
        let b = if area == "view" {
            let (a, b, c, e) = self.view_bounds();
            Some((a, b, c, e))
        } else {
            crate::civilplan::extents(&mut self.project)
        };
        let Some((x0, y0, x1, y1)) = b else { return (None, None) };
        let mut pb = BoxXY { min_x: x0, min_y: y0, max_x: x1, max_y: y1 };
        if area == "extents" {
            let m = str_to_number(&margin);
            let m = if m.is_nan() { 0.0 } else { m.max(0.0) };
            pb = BoxXY { min_x: pb.min_x - m, min_y: pb.min_y - m, max_x: pb.max_x + m, max_y: pb.max_y + m };
        }
        let src = self.gis3.terrain.as_ref().map(|d| d.src_crs.clone()).unwrap_or(Value::Null);
        let sb = terrain::to_source(&src, &self.eff_crs()).ok().and_then(|f| terrain::box_through(&pb, &*f, 8.0).ok().flatten());
        (Some(pb), sb)
    }
    fn terrain_default_spacing(&mut self) -> f64 {
        let k = self.terrain_unit_scale();
        let (_, sb) = self.terrain_area_boxes();
        let Some(d) = &self.gis3.terrain else { return 1.0 };
        match &d.src {
            Source::Grid(s) => {
                let cell_p = s.gt[1].hypot(s.gt[4]) * k;
                let win = match sb {
                    Some(b) => terrain::grid_window(&s.gt, s.width, s.height, Some(&b)),
                    None => Some(terrain::Win { c0: 0.0, r0: 0.0, c1: s.width, r1: s.height }),
                };
                let n = win.map_or(s.width * s.height, |w| (w.c1 - w.c0) * (w.r1 - w.r0));
                let cap = if d.as_ == "surface" { limits::TIN_NODES } else { limits::POINTS };
                let step = (n / (cap * 0.98)).sqrt().ceil().max(1.0);
                prec6(cell_p * step)
            }
            Source::Lidar(b) => {
                let fb = d.file_box;
                let area = (fb.max_x - fb.min_x) * (fb.max_y - fb.min_y) * k * k;
                let n = b.1.count.max(1.0);
                let natural = (area / n).sqrt();
                prec6(terrain::nice_step((natural * 1.5).max((area / limits::POINTS).sqrt() * 1.1)))
            }
        }
    }
    /// compute().
    pub fn terrain_plan(&mut self) -> Plan {
        let mut out = Plan {
            ok: false,
            msg: String::new(),
            text: String::new(),
            count: 0.0,
            win: None,
            step: 1.0,
            cell_p: 0.0,
            p_box: None,
            s_box: None,
        };
        let (_, block, _) = self.terrain_crs_lines();
        if let Some(b) = block {
            out.msg = b;
            return out;
        }
        let Some(d) = &self.gis3.terrain else { return out };
        if d.z_unit.is_none() {
            out.msg = t0("tui.z.say");
            return out;
        }
        let k = self.terrain_unit_scale();
        let (p_box, s_box) = self.terrain_area_boxes();
        let uw = self.u_word();
        let Some(d) = &self.gis3.terrain else { return out };
        let sp = str_to_number(&d.spacing);
        if sp.is_nan() || sp <= 0.0 {
            out.msg = t0("tui.spacing.bad");
            return out;
        }
        let cap = if matches!(d.src, Source::Grid(_)) && d.as_ == "surface" { limits::TIN_NODES } else { limits::POINTS };
        match &d.src {
            Source::Grid(s) => {
                let win = match &s_box {
                    Some(b) => terrain::grid_window(&s.gt, s.width, s.height, Some(b)),
                    None => Some(terrain::Win { c0: 0.0, r0: 0.0, c1: s.width, r1: s.height }),
                };
                let Some(win) = win else {
                    out.msg = t0("raster.no_overlap");
                    return out;
                };
                let (w, h) = (win.c1 - win.c0, win.r1 - win.r0);
                let cells = w * h;
                if cells > limits::READ_CELLS {
                    out.msg = tr!(
                        "tui.area.big",
                        cells = raster::fmt_big(cells),
                        w = num(w),
                        h = num(h),
                        max = raster::fmt_big(limits::READ_CELLS)
                    );
                    return out;
                }
                let cell_p = s.gt[1].hypot(s.gt[4]) * k;
                let step = thin::grid_step(sp, cell_p);
                let (nx, ny) = ((w / step).ceil(), (h / step).ceil());
                let n = nx * ny;
                out.win = Some(win);
                out.step = step;
                out.count = n;
                out.cell_p = cell_p;
                out.p_box = p_box;
                out.s_box = s_box;
                out.text = tr!(
                    if d.as_ == "surface" { "tui.plan.tin" } else { "tui.plan.pts" },
                    n = step,
                    count = num(n),
                    tri = num(2.0 * (nx - 1.0).max(0.0) * (ny - 1.0).max(0.0)),
                    w = num(w),
                    h = num(h),
                    d = fixed(cell_p * step, 2),
                    unit = uw
                );
                if n > cap {
                    let dd = fixed(terrain::nice_step(cell_p * (n * step * step / cap).sqrt().ceil()), 2);
                    out.msg = format!("{} {}", out.text, tr!("tui.over.grid", cap = num(cap), d = dd, unit = uw));
                    return out;
                }
            }
            Source::Lidar(_) => {
                if !d.scanned {
                    out.msg = t0("tui.classes.reading_short");
                    return out;
                }
                let classes = d.classes.clone().unwrap_or_default();
                if classes.is_empty() {
                    out.msg = t0("tui.classes.tick");
                    return out;
                }
                let (points, in_area) = match &d.coverage {
                    Some(c) => c.estimate(sp / k, Some(&classes), s_box.as_ref()),
                    None => {
                        let n: f64 =
                            d.class_counts.as_ref().map_or(0.0, |cc| cc.iter().filter(|(c, _)| classes.contains(c)).map(|x| x.1).sum());
                        (n, n)
                    }
                };
                out.count = points;
                out.p_box = p_box;
                out.s_box = s_box;
                out.text = tr!("tui.plan.lidar", count = num(points), sp = fixed(sp, 2), unit = uw, inside = num(in_area));
                if points > cap {
                    out.msg = format!("{} {}", out.text, tr!("tui.over.lidar", cap = num(cap)));
                    return out;
                }
            }
        }
        out.ok = true;
        out
    }

    /// The dialog's lines: facts, systems, settings, the plan, the z words.
    pub fn terrain_info(&mut self) -> Result<String, String> {
        if self.gis3.terrain.is_none() {
            return Err("civil.terrain.info: the terrain dialog is not open".into());
        }
        let plan = self.terrain_plan();
        let (crs_lines, _, _) = self.terrain_crs_lines();
        let facts = self.terrain_facts();
        let z = self.terrain_z_words();
        let d = self.gis3.terrain.as_ref().ok_or("closed")?;
        let mut out: Vec<String> = facts.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.push(format!("{}: {}", t0("tui.crs.head"), crs_lines.join(" ")));
        if let (Source::Lidar(_), Some(cc)) = (&d.src, &d.class_counts) {
            let mut has: Vec<(f64, f64)> = cc.clone();
            has.sort_by(|a, b| a.0.total_cmp(&b.0));
            let picked = d.classes.clone().unwrap_or_default();
            let rows: Vec<String> = has
                .iter()
                .map(|(c, n)| {
                    format!("[{}] {} {} {}", if picked.contains(c) { "x" } else { " " }, num_to_string(*c), class_name(*c), num(*n))
                })
                .collect();
            out.push(format!("{}: {}", t0("tui.classes.head"), rows.join("; ")));
            out.push(t0(if cc.iter().any(|x| x.0 == 2.0) { "tui.classes.ground" } else { "tui.classes.no_ground" }));
        }
        out.push(format!(
            "area={} margin={} spacing={} mode={} z={} as={}",
            d.area,
            d.margin,
            d.spacing,
            d.mode,
            d.z_unit.clone().unwrap_or_default(),
            d.as_
        ));
        out.push(z);
        out.push(if plan.ok { plan.text } else { plan.msg });
        if !d.msg.is_empty() {
            out.push(d.msg.clone());
        }
        Ok(out.join(" | "))
    }
    fn terrain_facts(&self) -> Vec<(String, String)> {
        let Some(d) = &self.gis3.terrain else { return Vec::new() };
        let (name, size) = match &d.src {
            Source::Grid(s) => (s.name.clone(), s.size),
            Source::Lidar(b) => (b.1.name.clone(), b.1.size),
        };
        let mut rows = vec![
            (t0("tui.f.file"), name),
            (
                t0("tui.f.size"),
                if size < 1_048_576.0 {
                    tr!("tui.size.kb", n = (size / 1024.0).round().max(1.0))
                } else {
                    tr!("tui.size.mb", n = to_fixed(size / 1_048_576.0, 1))
                },
            ),
        ];
        let mut notes: Vec<String> = Vec::new();
        match &d.src {
            Source::Grid(s) => {
                rows.push((
                    t0("tui.f.kind"),
                    if s.kind == "asc" { t0("tui.kind.asc") } else { tr!("tui.kind.tiff", dtype = s.dtype.as_str()) },
                ));
                rows.push((
                    t0("tui.f.cells"),
                    tr!("tui.cells", w = num(s.width), h = num(s.height), total = raster::fmt_big(s.width * s.height)),
                ));
                if let Some(c) = s.cell.filter(|c| *c != 0.0 && !c.is_nan()) {
                    let unit = if s.crs.get("unit").and_then(Value::as_str) == Some("deg") {
                        "°".to_string()
                    } else if truthy(Some(&s.crs)) {
                        format!(" {}", s.crs.get("unit").and_then(Value::as_str).map(unit_word).unwrap_or_default())
                    } else {
                        String::new()
                    };
                    rows.push((t0("tui.f.cell"), format!("{}{unit}", fixed(c, if c < 0.01 { 8 } else { 3 }))));
                }
                if let Some(nd) = s.nodata {
                    rows.push((t0("tui.f.nodata"), num_to_string(nd)));
                }
                if s.pixel_is_point {
                    rows.push((t0("tui.f.cells_are"), t0("tui.cells_are.points")));
                }
                notes.extend(s.notes.iter().cloned());
                if !truthy(Some(&s.crs))
                    && let Some(n) = &s.crs_note
                {
                    notes.push(n.clone());
                }
            }
            Source::Lidar(b) => {
                let h = &b.1;
                rows.push((
                    t0("tui.f.kind"),
                    tr!("tui.kind.las", format = if h.compressed { "LAZ" } else { "LAS" }, version = h.version.as_str(), pf = h.format),
                ));
                rows.push((
                    t0("tui.f.points"),
                    if h.available < h.count {
                        tr!("tui.points.short", count = num(h.count), available = num(h.available))
                    } else {
                        num(h.count)
                    },
                ));
                notes.extend(h.notes.iter().cloned());
                if !truthy(Some(&h.crs))
                    && let Some(n) = &h.crs_note
                {
                    notes.push(n.clone());
                }
            }
        }
        let src_crs = match &d.src {
            Source::Grid(s) => s.crs.clone(),
            Source::Lidar(b) => b.1.crs.clone(),
        };
        rows.push((t0("tui.crs.head"), crs_text(&src_crs)));
        rows.push((t0("tui.f.extent"), d.ll_text.clone()));
        rows.push((
            t0("tui.f.z"),
            match d.z_range {
                Some((lo, hi, sampled)) => {
                    tr!(if sampled { "tui.z.range_sample" } else { "tui.z.range" }, min = fixed(lo, 2), max = fixed(hi, 2))
                }
                None => "—".into(),
            },
        ));
        for n in notes {
            rows.push((String::new(), n));
        }
        rows
    }
    fn terrain_z_words(&self) -> String {
        let Some(d) = &self.gis3.terrain else { return String::new() };
        let Some((lo, hi, _)) = d.z_range else { return String::new() };
        let to = terrain::project_z_unit(&self.project.state);
        let parts: Vec<String> = ["m", "ft"]
            .iter()
            .map(|u| {
                let f = terrain::z_factor(u, to);
                tr!("tui.z.as", word = unit_word(u), min = fixed(lo * f, 2), max = fixed(hi * f, 2), unit = self.u_word())
            })
            .collect();
        let (zu, from) = match &d.src {
            Source::Grid(s) => (s.z_unit, s.z_unit_from.clone()),
            Source::Lidar(b) => (b.1.z_unit, None),
        };
        let says = match (zu, from) {
            (Some(u), Some(f)) => tr!("tui.z.says_from", word = unit_word(u), from = f),
            (Some(u), None) => tr!("tui.z.says", word = unit_word(u)),
            (None, _) => t0("tui.z.unsaid"),
        };
        format!("{} {says}", tr!("tui.z.info", min = fixed(lo, 2), max = fixed(hi, 2), parts = parts.join("; ")))
    }

    /// `civil.terrain=<action>`.
    pub fn terrain_cmd(&mut self, spec: &str) -> Result<(), String> {
        if self.gis3.terrain.is_none() {
            return Err("civil.terrain: the terrain dialog is not open".into());
        }
        if self.gis3.ask.is_some() {
            return Err("civil.terrain: answer the question first (civil.ask)".into());
        }
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        let is_grid = self.gis3.terrain.as_ref().is_some_and(|d| matches!(d.src, Source::Grid(_)));
        let respace = |cv: &mut CivilView| {
            if cv.gis3.terrain.as_ref().is_some_and(|d| !d.spacing_touched) {
                let sp = cv.terrain_default_spacing();
                if let Some(d) = cv.gis3.terrain.as_mut() {
                    d.spacing = num_to_string(sp);
                }
            }
        };
        match k {
            "cancel" | "close" => self.gis3.terrain = None,
            "area" => {
                if !["extents", "view", "all"].contains(&v) {
                    return Err("civil.terrain: area=extents|view|all".into());
                }
                if v != "all" && crate::civilplan::extents(&mut self.project).is_none() {
                    return Err("civil.terrain: the project has no extents".into());
                }
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.area = v.into();
                }
                respace(self);
            }
            "margin" => {
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.margin = cc_civil::js::slice16(v, 40);
                }
            }
            "spacing" => {
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.spacing = cc_civil::js::slice16(v, 40);
                    d.spacing_touched = true;
                }
            }
            "mode" => {
                if !["nearest", "lowest", "average"].contains(&v) || is_grid {
                    return Err("civil.terrain: mode=nearest|lowest|average (LiDAR)".into());
                }
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.mode = v.into();
                }
            }
            "z" => {
                if !v.is_empty() && !UNITS.contains(&v) {
                    return Err("civil.terrain: z=m|ft|ftUS".into());
                }
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.z_unit = (!v.is_empty()).then(|| v.to_string());
                }
            }
            "as" => {
                if !["points", "surface"].contains(&v) || !is_grid {
                    return Err("civil.terrain: as=points|surface (a grid)".into());
                }
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.as_ = v.into();
                }
                respace(self);
            }
            "class" => {
                let (c, on) = v.split_once(':').unwrap_or((v, "on"));
                let c = str_to_number(c);
                let d = self.gis3.terrain.as_mut().ok_or("closed")?;
                if !d.class_counts.as_ref().is_some_and(|cc| cc.iter().any(|x| x.0 == c)) {
                    return Err("civil.terrain: class=<a class in the file>:on|off".into());
                }
                let l = d.classes.get_or_insert_with(Vec::new);
                l.retain(|x| *x != c);
                if on != "off" {
                    l.push(c);
                }
            }
            "adopt" => {
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.adopt = v != "off";
                }
            }
            "pickfile" | "pickproj" => {
                let (_, _, buttons) = self.terrain_crs_lines();
                if !buttons.contains(&k) {
                    return Err(format!("civil.terrain: {k} is not offered"));
                }
                let name = match &self.gis3.terrain.as_ref().ok_or("closed")?.src {
                    Source::Grid(s) => s.name.clone(),
                    Source::Lidar(b) => b.1.name.clone(),
                };
                let near = self.terrain_near();
                let (reason, who) = if k == "pickfile" {
                    (tr!("tui.crs.pick_file_q", name = name.as_str()), PickFor::TerrainFile)
                } else {
                    let s = self.gis3.terrain.as_ref().map(|d| string_of(d.src_crs.get("name"))).unwrap_or_default();
                    (tr!("tui.crs.pick_proj_q", name = name.as_str(), crs = s.as_str()), PickFor::TerrainProject)
                };
                self.crs_dlg = None;
                self.crs_open(Some((reason, near, "project", who)));
            }
            "go" => self.terrain_go()?,
            o => return Err(format!("civil.terrain: unknown action {o}")),
        }
        Ok(())
    }
    fn terrain_near(&self) -> Option<[f64; 2]> {
        let d = self.gis3.terrain.as_ref()?;
        if !d.box_ok {
            return None;
        }
        let ll = terrain::lon_lat_of(&d.src_crs).ok().flatten()?;
        let b = d.file_box;
        let p = ll((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0).ok()?;
        p.iter().all(|v| v.is_finite()).then_some(p)
    }
    /// The picker's answer: the file's system, or the project's for the file.
    pub fn terrain_crs_answer(&mut self, f: PickFor, v: Option<Value>) {
        let Some(c) = v else { return };
        let units = self.project.units().to_string();
        match f {
            PickFor::TerrainFile => {
                if let Some(d) = self.gis3.terrain.as_mut() {
                    d.src_crs = c;
                    d.src_chosen = true;
                }
            }
            _ => match cc_io_civil::crs::crs_problem(&c, &units) {
                None => {
                    if let Some(d) = self.gis3.terrain.as_mut() {
                        d.new_crs = Some(c);
                    }
                }
                Some(why) => self.message = why,
            },
        }
    }

    /// run(): read, thin, add (one undo step); the system the file brings set with it.
    fn terrain_go(&mut self) -> Result<(), String> {
        let plan = self.terrain_plan();
        if !plan.ok {
            return Err(format!("civil.terrain: {}", plan.msg));
        }
        let t0_ = web_time::Instant::now();
        let epc = self.eff_crs();
        let d = self.gis3.terrain.as_ref().ok_or("closed")?.clone();
        let (tr_fn, identity) = terrain::to_project(&d.src_crs, &epc)?;
        let mut st = self.project.state.clone();
        if let Some(m) = st.as_object_mut() {
            m.insert("crs".into(), epc.clone());
        }
        let zu = d.z_unit.clone().unwrap_or_else(|| "m".into());
        let zf = terrain::z_factor(&zu, terrain::project_z_unit(&st));
        let mut notes: Vec<String> = Vec::new();
        let uw = self.u_word();
        let name = match &d.src {
            Source::Grid(s) => s.name.clone(),
            Source::Lidar(b) => b.1.name.clone(),
        };
        match &d.src {
            Source::Grid(s) => {
                let w = plan.win.ok_or("no window")?;
                let grid = match raster::read_window(s, Some(w.c0), Some(w.r0), Some(w.c1), Some(w.r1), Some(plan.step)) {
                    Ok(g) => g,
                    Err(e) => return self.terrain_failed(e),
                };
                if d.as_ == "surface" {
                    let tin = match terrain::grid_tin(&grid, &s.gt, &*tr_fn, zf) {
                        Ok(t) => t,
                        Err(e) => return self.terrain_failed(e),
                    };
                    if tin.faces.is_empty() {
                        return self.terrain_failed(t0("tui.err.no_corners"));
                    }
                    let has_surface =
                        !self.project.points.is_empty() || truthy(self.project.state.get("surface").and_then(|s| s.get("tin")));
                    let extra = (identity, zf, zu.clone());
                    if has_surface {
                        // applySurface asks first
                        if let Some(dd) = self.gis3.terrain.as_mut() {
                            dd.pending = Some(std::sync::Arc::new((tin, Some(epc.clone()), zf)));
                        }
                        let _ = extra;
                        self.gis3.ask = Some(Ask {
                            kind: AskKind::Other("tin".into()),
                            title: t0("tui.replace.title"),
                            text: tr!("grid.replace.ask", name = name.as_str()),
                            buttons: vec![("cancel", t0("cad.ui.cancel")), ("replace", t0("tui.replace.btn"))],
                        });
                        return Ok(());
                    }
                    self.terrain_surface(&tin, &epc, zf, false);
                    return Ok(());
                }
                let start = next_number(&self.project.points);
                let pts = match terrain::grid_points(&grid, &s.gt, &*tr_fn, zf, "DEM", start) {
                    Ok(p) => p,
                    Err(e) => return self.terrain_failed(e),
                };
                if pts.is_empty() {
                    return self.terrain_failed(t0("tui.err.no_cell"));
                }
                let n = pts.len();
                let pts: Vec<cc_civil::points::Pt> =
                    pts.into_iter().map(|p| cc_civil::points::Pt { n: p.n, x: p.x, y: p.y, z: Some(p.z), d: p.d, blk: None }).collect();
                self.terrain_points(pts, &epc, &tr!("tui.undo.import", name = name.as_str()));
                notes.push(tr!(
                    "tui.done.dem",
                    n = plan.step,
                    count = num(n as f64),
                    name = name.as_str(),
                    d = fixed(plan.cell_p * plan.step, 2),
                    unit = uw
                ));
            }
            Source::Lidar(b) => {
                let (file, h0) = &**b;
                let mut h = h0.clone();
                let origin = match plan.p_box {
                    Some(p) => [p.min_x, p.min_y],
                    None => match terrain::box_through(&d.file_box, &*tr_fn, 8.0) {
                        Ok(Some(bb)) => [bb.min_x, bb.min_y],
                        _ => [0.0, 0.0],
                    },
                };
                let classes = d.classes.clone().unwrap_or_default();
                let sp = str_to_number(&d.spacing);
                let mut th = thin::Thinner::new(sp, &d.mode, origin, Some(&classes), plan.p_box, limits::POINTS)?;
                let sb = plan.s_box;
                let mut err: Option<String> = None;
                let r = las::scan_points(file, &mut h, &mut |x, y, z, c| {
                    if let Some(s) = &sb
                        && (x < s.min_x || x > s.max_x || y < s.min_y || y > s.max_y)
                    {
                        th.seen += 1.0;
                        th.outside += 1.0;
                        return;
                    }
                    if !classes.contains(&c) {
                        th.seen += 1.0;
                        th.filtered += 1.0;
                        return;
                    }
                    if identity {
                        th.add(x, y, z * zf, c);
                    } else {
                        match tr_fn(x, y) {
                            Ok(q) => {
                                th.add(q[0], q[1], z * zf, c);
                            }
                            Err(e) => err = Some(e),
                        }
                    }
                });
                if let Err(e) = r {
                    return self.terrain_failed(e);
                }
                if let Some(e) = err {
                    return self.terrain_failed(e);
                }
                if th.count == 0 {
                    return self.terrain_failed(t0("tui.err.no_point"));
                }
                let mut n = next_number(&self.project.points);
                let pts: Vec<cc_civil::points::Pt> = th
                    .points()
                    .into_iter()
                    .map(|p| {
                        let q = cc_civil::points::Pt { n: num_to_string(n), x: p[0], y: p[1], z: Some(p[2]), d: "LIDAR".into(), blk: None };
                        n += 1.0;
                        q
                    })
                    .collect();
                let count = pts.len();
                self.terrain_points(pts, &epc, &tr!("tui.undo.import", name = name.as_str()));
                let keep = t0(&format!("tui.keep.{}.in", d.mode));
                notes.push(tr!(
                    "tui.done.lidar",
                    count = num(count as f64),
                    name = name.as_str(),
                    sp = fixed(sp, 2),
                    unit = uw,
                    keep = keep.as_str(),
                    seen = num(th.seen),
                    filtered = num(th.filtered),
                    outside = num(th.outside)
                ));
                if th.capped {
                    notes.push(tr!("tui.capped", max = num(limits::POINTS), dropped = num(th.dropped)));
                }
                notes.extend(h0.notes.iter().cloned());
            }
        }
        self.terrain_done(notes, identity, &d.src_crs, &epc, &zu, zf);
        self.timings.insert("terrain import", t0_.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }
    fn terrain_failed(&mut self, e: String) -> Result<(), String> {
        if let Some(d) = self.gis3.terrain.as_mut() {
            d.msg = tr!("tui.import_failed", error = e);
        }
        Ok(())
    }
    fn terrain_done(&mut self, mut notes: Vec<String>, identity: bool, src: &Value, epc: &Value, zu: &str, zf: f64) {
        if !identity {
            notes.push(tr!("tui.done.converted", from = crs_text(src), to = crs_text(epc)));
        }
        if zf != 1.0 {
            notes.push(tr!("tui.done.zconv", word = unit_word(zu), f = cc_io_civil::dxf::to_precision(zf, 8)));
        }
        self.gis3.terrain = None;
        self.message = notes.first().cloned().unwrap_or_default();
        self.report = notes;
    }
    /// applyPoints: one undo step; the system the project lacked set in it.
    fn terrain_points(&mut self, pts: Vec<cc_civil::points::Pt>, epc: &Value, label: &str) {
        let before = self.pc();
        let was_empty = self.project.points.is_empty()
            && !truthy(self.project.state.get("surface").and_then(|s| s.get("tin")))
            && cc_civil::js::arr(self.project.state.get("alignments")).is_empty();
        let mut changed = false;
        self.project.edit_points(label, &mut |mut old, s| {
            changed = set_crs(s, epc);
            old.extend(pts.iter().cloned());
            Some(old)
        });
        if changed {
            let after = self.pc();
            self.crs_changed(Some(before).filter(|b| truthy(Some(b))), Some(after), false);
        }
        if was_empty {
            self.fit();
        }
    }
    /// applySurface (after the question when a surface was there): the grid TIN, one undo step.
    fn terrain_surface(&mut self, tin: &terrain::GridTin, epc: &Value, zf: f64, replaced: bool) {
        let Some(d) = self.gis3.terrain.clone() else { return };
        let name = match &d.src {
            Source::Grid(s) => s.name.clone(),
            Source::Lidar(b) => b.1.name.clone(),
        };
        let had = !self.project.points.is_empty() || truthy(self.project.state.get("surface").and_then(|s| s.get("tin")));
        let before = self.pc();
        let mut changed = false;
        let untitled = t0("project.untitled");
        let stem = cc_civil::js::slice16(name.rsplit_once('.').map_or(name.as_str(), |(a, _)| a), 120);
        let pts: Vec<Value> = tin
            .pts
            .iter()
            .map(|p| json!({ "x": cc_civil::js::jnum(p[0]), "y": cc_civil::js::jnum(p[1]), "z": cc_civil::js::jnum(p[2]) }))
            .collect();
        let faces: Vec<Value> = tin.faces.iter().map(|f| json!(f.to_vec())).collect();
        let tv = json!({ "pts": pts, "faces": faces, "from": "grid" });
        self.project.set_tin(
            &tr!("grid.undo", name = name.as_str()),
            tv,
            Some(&mut |s: &mut Value| {
                if !truthy(s.get("crs")) && truthy(Some(epc)) {
                    changed = set_crs(s, epc);
                }
                if !had
                    && s.get("name").and_then(Value::as_str) == Some(untitled.as_str())
                    && let Some(m) = s.as_object_mut()
                {
                    m.insert("name".into(), Value::String(stem.clone()));
                }
            }),
        );
        if changed {
            let after = self.pc();
            self.crs_changed(Some(before).filter(|b| truthy(Some(b))), Some(after), false);
        }
        self.fit();
        let mut notes = vec![tr!(
            "tui.done.surface",
            name = name.as_str(),
            pts = num(tin.pts.len() as f64),
            faces = num(tin.faces.len() as f64),
            skipped = num(tin.skipped as f64)
        )];
        if replaced {
            notes.push(t0("grid.replaced"));
        }
        let zu = d.z_unit.clone().unwrap_or_else(|| "m".into());
        let identity = terrain::to_project(&d.src_crs, epc).map(|x| x.1).unwrap_or(true);
        self.terrain_done(notes, identity, &d.src_crs, epc, &zu, zf);
    }

    /// The other questions (0.5's host.ask) of the parts after M11.25.
    pub fn ask_other(&mut self, what: &str, b: &str) {
        match what {
            "mapcrs" => self.pic_ask(b),
            "tin" => {
                let p = self.gis3.terrain.as_mut().and_then(|d| d.pending.take());
                match (b, p) {
                    ("replace", Some(p)) => {
                        let (tin, epc, zf) = &*p;
                        let epc = epc.clone().unwrap_or(Value::Null);
                        self.terrain_surface(tin, &epc, *zf, true);
                    }
                    _ => {
                        // applySurface's cancel ends run() as an abort: said, the dialog stays
                        if let Some(d) = self.gis3.terrain.as_mut() {
                            d.msg = t0("tui.cancelled.import");
                        }
                        self.message = t0("tui.cancelled.say");
                    }
                }
            }
            _ => {}
        }
    }
}

/// setCrs(proj, epc, s): the system the project lacked (a copy of the file's, its local step copied).
fn set_crs(s: &mut Value, epc: &Value) -> bool {
    if truthy(s.get("crs")) || !truthy(Some(epc)) {
        return false;
    }
    if let Some(m) = s.as_object_mut() {
        m.insert("crs".into(), epc.clone());
    }
    true
}

/// `nextNumber(points)`.
fn next_number(points: &[cc_civil::points::Pt]) -> f64 {
    let mut hi = 0.0;
    for p in points {
        let n = str_to_number(&p.n);
        if cc_civil::js::is_integer(n) && n > hi && n < 1e12 {
            hi = n;
        }
    }
    hi + 1.0
}

fn class_name(c: f64) -> String {
    las::class_name(c).unwrap_or_else(|| t0(if c <= 63.0 { "tui.class.reserved" } else { "tui.class.user" }))
}

/// The terrain import window.
pub fn terrain_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    if cv.gis3.terrain.is_none() {
        return;
    }
    let plan = cv.terrain_plan();
    let facts = cv.terrain_facts();
    let (crs_lines, _, buttons) = cv.terrain_crs_lines();
    let z = cv.terrain_z_words();
    let has_ext = crate::civilplan::extents(&mut cv.project).is_some();
    let uw = cv.u_word();
    let Some(d) = cv.gis3.terrain.as_ref() else { return };
    let lidar = matches!(d.src, Source::Lidar(_));
    let mut open = true;
    let dim = |ui: &mut egui::Ui, s: &str| {
        ui.add(egui::Label::new(RichText::new(s).small().color(tk.text_dim)).wrap());
    };
    crate::shell::Dialog::new(t0(if lidar { "tui.title.lidar" } else { "tui.title.grid" }))
        .id(egui::Id::unique("cv_terrain"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .show(ctx, |ui| {
            egui::Grid::new("cv_terrain_facts").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
                for (k, v) in &facts {
                    ui.label(RichText::new(k).small().color(tk.text_dim));
                    ui.add(egui::Label::new(RichText::new(v).small().color(tk.text)).wrap());
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(RichText::new(t0("tui.crs.head")).strong().family(crate::theme::bold_family()).color(tk.text_strong));
            for l in &crs_lines {
                if let Some(rest) = l.strip_prefix("[x] ").or_else(|| l.strip_prefix("[ ] ")) {
                    let mut on = d.adopt;
                    if ui.checkbox(&mut on, rest).changed() {
                        acts.push(("civil.terrain", format!("adopt={}", if on { "on" } else { "off" })));
                    }
                } else {
                    dim(ui, l);
                }
            }
            for b in &buttons {
                let label = t0(if *b == "pickfile" { "tui.crs.pick_file" } else { "tui.crs.pick_proj" });
                if ui.link(label).clicked() {
                    acts.push(("civil.terrain", (*b).to_string()));
                }
            }
            if let (true, Some(cc)) = (lidar, &d.class_counts) {
                ui.separator();
                ui.label(RichText::new(t0("tui.classes.head")).strong().family(crate::theme::bold_family()).color(tk.text_strong));
                let mut has = cc.clone();
                has.sort_by(|a, b| a.0.total_cmp(&b.0));
                let picked = d.classes.clone().unwrap_or_default();
                egui::Grid::new("cv_terrain_classes").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new(t0("tui.th.class")).small().strong().family(crate::theme::bold_family()));
                    ui.label(RichText::new(t0("tui.th.name")).small().strong().family(crate::theme::bold_family()));
                    ui.label(RichText::new(t0("tui.f.points")).small().strong().family(crate::theme::bold_family()));
                    ui.end_row();
                    for (c, n) in &has {
                        let mut on = picked.contains(c);
                        if ui.checkbox(&mut on, "").on_hover_text(tr!("tui.class.aria", c = *c)).changed() {
                            acts.push(("civil.terrain", format!("class={}:{}", num_to_string(*c), if on { "on" } else { "off" })));
                        }
                        ui.label(num_to_string(*c));
                        ui.label(class_name(*c));
                        ui.label(num(*n));
                        ui.end_row();
                    }
                });
                dim(ui, &t0(if cc.iter().any(|x| x.0 == 2.0) { "tui.classes.ground" } else { "tui.classes.no_ground" }));
            }
            ui.separator();
            egui::Grid::new("cv_terrain_set").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label(t0("tui.l.area"));
                egui::ComboBox::new("cv_terrain_area", "").selected_text(t0(&format!("tui.area.{}", d.area))).show_ui(ui, |ui| {
                    for a in ["extents", "view", "all"] {
                        let on = a == "all" || has_ext;
                        if ui.add_enabled(on, egui::Button::selectable(d.area == a, t0(&format!("tui.area.{a}")))).clicked() {
                            acts.push(("civil.terrain", format!("area={a}")));
                        }
                    }
                });
                ui.end_row();
                let l = ui.label(tr!("tui.l.margin", unit = uw));
                let mut m = d.margin.clone();
                if ui.add_enabled(d.area == "extents", crate::shell::field(&mut m).desired_width(90.0)).labelled_by(l.id).changed() {
                    acts.push(("civil.terrain", format!("margin={m}")));
                }
                ui.end_row();
                let l = ui.label(tr!("tui.l.thin", unit = uw));
                let mut s = d.spacing.clone();
                if ui.add(crate::shell::field(&mut s).desired_width(90.0)).labelled_by(l.id).changed() {
                    acts.push(("civil.terrain", format!("spacing={s}")));
                }
                ui.end_row();
                if lidar {
                    ui.label(t0("tui.l.keep"));
                    egui::ComboBox::new("cv_terrain_keep", "").selected_text(t0(&format!("tui.keep.{}", d.mode))).show_ui(ui, |ui| {
                        for m in ["nearest", "lowest", "average"] {
                            if ui.selectable_label(d.mode == m, t0(&format!("tui.keep.{m}"))).clicked() {
                                acts.push(("civil.terrain", format!("mode={m}")));
                            }
                        }
                    });
                    ui.end_row();
                }
                ui.label(t0("tui.l.zfile"));
                let zl = |u: &str| match u {
                    "m" => t0("cad.ui.unit.m"),
                    "ft" => t0("cad.ui.unit.ft"),
                    "ftUS" => t0("cad.ui.unit.ft-us"),
                    _ => t0("tui.choose"),
                };
                egui::ComboBox::new("cv_terrain_z", "").selected_text(zl(d.z_unit.as_deref().unwrap_or(""))).show_ui(ui, |ui| {
                    for u in UNITS {
                        if ui.selectable_label(d.z_unit.as_deref() == Some(u), zl(u)).clicked() {
                            acts.push(("civil.terrain", format!("z={u}")));
                        }
                    }
                });
                ui.end_row();
                if !lidar {
                    ui.label(t0("tui.l.as"));
                    ui.horizontal(|ui| {
                        if ui.radio(d.as_ == "points", t0("tui.as.points")).clicked() {
                            acts.push(("civil.terrain", "as=points".into()));
                        }
                        if ui.radio(d.as_ == "surface", t0("tui.as.surface")).clicked() {
                            acts.push(("civil.terrain", "as=surface".into()));
                        }
                    });
                    ui.end_row();
                }
            });
            dim(ui, &z);
            let c = if plan.ok { tk.text } else { egui::Color32::from_rgb(0xd0, 0x4a, 0x5a) };
            ui.add(egui::Label::new(RichText::new(if plan.ok { &plan.text } else { &plan.msg }).small().color(c)).wrap());
            if !d.msg.is_empty() {
                dim(ui, &d.msg);
            }
            crate::shell::foot(ui, |ui| {
                if ui.add(crate::shell::Btn::new(t0("cad.ui.cancel"))).clicked() {
                    acts.push(("civil.terrain", "cancel".into()));
                }
                if ui.add_enabled(plan.ok, crate::shell::Btn::primary(t0("tui.import"))).clicked() {
                    acts.push(("civil.terrain", "go".into()));
                }
            });
        });
    if !open {
        acts.push(("civil.terrain", "cancel".into()));
    }
}

// ------------------------------------------------------------------------------------------------ GeoTIFF export

/// "Surface as a GeoTIFF" (geotiffSection): which surface and the cell size.
#[derive(Clone, Debug, Default)]
pub struct GeoTiffDlg {
    /// "eg" | "fg"
    pub which: String,
    pub cell: String,
    /// the progress line (written, failed)
    pub msg: String,
}

/// English unit words written into the GeoTIFF's description (file content, not the page's language).
fn file_unit_words(u: &str) -> &'static str {
    match u {
        "m" => "meters",
        "ft" => "feet",
        _ => "US survey feet",
    }
}

impl CivilView {
    fn gt_surface(&mut self, which: &str) -> Option<std::sync::Arc<cc_civil::tin::Surface>> {
        if which == "fg"
            && let Some(s) = self.project.graded().surface.clone()
        {
            return Some(s);
        }
        self.project.eg().surface.clone().map(std::sync::Arc::new)
    }
    fn gt_cell_default(&mut self, which: &str) -> String {
        let a = self.gt_surface(which).map_or(0.0, |s| cc_io_civil::geotiff_write::average_edge(&s));
        num_to_string(if a > 0.0 { prec6(terrain::nice_step(a / 2.5)) } else { 1.0 })
    }
    /// `civil.geotiff=open|close|surface=eg|fg|cell=<size>`; `go` is run by the host (it saves the file).
    pub fn geotiff_cmd(&mut self, spec: &str) -> Result<(), String> {
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        match k {
            "" | "open" => {
                let cell = self.gt_cell_default("eg");
                self.gis3.geotiff = Some(GeoTiffDlg { which: "eg".into(), cell, msg: String::new() });
            }
            "close" | "cancel" => self.gis3.geotiff = None,
            "surface" => {
                if !(v == "eg" || (v == "fg" && self.project.graded().surface.is_some())) {
                    return Err("civil.geotiff: surface=eg|fg (a graded surface)".into());
                }
                let cell = self.gt_cell_default(v);
                let d = self.gis3.geotiff.as_mut().ok_or("civil.geotiff: the dialog is not open")?;
                d.which = v.into();
                d.cell = cell;
            }
            "cell" => {
                let d = self.gis3.geotiff.as_mut().ok_or("civil.geotiff: the dialog is not open")?;
                d.cell = cc_civil::js::slice16(v, 40);
            }
            o => return Err(format!("civil.geotiff: open, close, surface=, cell= or go (not {o})")),
        }
        Ok(())
    }
    /// upd(): the size line and whether Download can run.
    pub fn geotiff_lines(&mut self) -> (Vec<String>, bool) {
        let Some(d) = self.gis3.geotiff.clone() else { return (Vec::new(), false) };
        let Some(s) = self.gt_surface(&d.which) else { return (vec![t0("tui.gt.none")], false) };
        let crs = self.pc();
        let zu = terrain::project_z_unit(&self.project.state);
        let mut out = Vec::new();
        let c = str_to_number(&d.cell);
        let ready = if c.is_nan() || c <= 0.0 {
            out.push(t0("tui.gt.cell_bad"));
            false
        } else {
            let b = s.bounds();
            let (mut w, mut h) = (b.max_x - b.min_x, b.max_y - b.min_y);
            if let Some(l) = cc_io_civil::crs::read_local(crs.get("local")).filter(|_| truthy(Some(&crs))) {
                w *= l.scale;
                h *= l.scale;
            }
            let (nx, ny) = ((w / c).ceil() + 1.0, (h / c).ceil() + 1.0);
            let mut info = tr!(
                "tui.gt.info",
                nx = num(nx),
                ny = num(ny),
                c = fixed(c, 3),
                unit = self.u_word(),
                total = raster::fmt_big(nx * ny),
                mb = to_fixed((nx * ny * 4.0 / 1_048_576.0 / 2.0).max(0.1), 1)
            );
            let over = nx * ny > 25e6;
            if over {
                info.push(' ');
                info.push_str(&t0("tui.gt.over"));
            }
            out.push(info);
            !over
        };
        let code = crs.get("code").filter(|v| truthy(Some(v)));
        let mut note = if !truthy(Some(&crs)) {
            t0("tui.gt.nocrs")
        } else if code.is_none() {
            tr!("tui.gt.nocode", name = string_of(crs.get("name")))
        } else {
            tr!(
                if truthy(crs.get("local")) { "tui.gt.crs_local" } else { "tui.gt.crs" },
                name = string_of(crs.get("name")),
                code = code.map(cc_civil::js::to_js_string).unwrap_or_default()
            )
        };
        note.push(' ');
        note.push_str(&tr!("tui.gt.z", word = unit_word(zu)));
        out.push(note);
        if !d.msg.is_empty() {
            out.push(d.msg.clone());
        }
        (out, ready)
    }
    /// Download: the grid sampled and written → (file name, bytes); said on the status line.
    pub fn geotiff_go(&mut self) -> Result<(String, Vec<u8>), String> {
        let (_, ready) = self.geotiff_lines();
        let d = self.gis3.geotiff.clone().ok_or("civil.geotiff: the dialog is not open")?;
        if !ready {
            return Err("civil.geotiff: nothing to write (see civil.geotiff.info)".into());
        }
        let s = self.gt_surface(&d.which).ok_or("no surface")?;
        let crs = self.pc();
        let fr = if truthy(Some(&crs)) { cc_io_civil::crs::frame(&crs).ok().flatten() } else { None };
        let tg = |x: f64, y: f64| fr.as_ref().map_or([x, y], |f| f.to_grid(x, y));
        let fg = |x: f64, y: f64| fr.as_ref().map_or([x, y], |f| f.from_grid(x, y));
        let t0_ = web_time::Instant::now();
        let g = match cc_io_civil::geotiff_write::surface_grid(&s, str_to_number(&d.cell), &tg, &fg, 25e6) {
            Ok(g) => g,
            Err(e) => {
                if let Some(x) = self.gis3.geotiff.as_mut() {
                    x.msg = tr!("tui.gt.failed", error = e.as_str());
                }
                return Err(e);
            }
        };
        let zu = terrain::project_z_unit(&self.project.state);
        let fgs = d.which == "fg";
        let desc = format!(
            "{} elevations in {} (project units). {} Cells outside the surface are nodata. Made by CivilCraft.",
            if fgs { "Graded surface" } else { "Existing ground" },
            file_unit_words(zu),
            if truthy(Some(&crs)) {
                format!(
                    "{}{}.",
                    if truthy(crs.get("local")) { "Grid coordinates of " } else { "Coordinates in " },
                    string_of(crs.get("name"))
                )
            } else {
                "Project coordinates (no coordinate system).".to_string()
            }
        );
        let opts = cc_io_civil::geotiff_write::DemOptions {
            epsg: crs.get("code").and_then(Value::as_f64).filter(|c| *c != 0.0),
            citation: string_of(crs.get("name")),
            xy_unit: crs
                .get("unit")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .map_or_else(|| if self.project.units() == "m" { "m".into() } else { "ftUS".into() }, str::to_string),
            z_unit: zu.to_string(),
            description: desc,
            tiled: true,
            tile: 256.0,
            deflate: true,
        };
        let bytes = match cc_io_civil::geotiff_write::write_dem_tiff(g.width, g.height, g.gt, &g.values, &opts) {
            Ok(b) => b,
            Err(e) => {
                if let Some(x) = self.gis3.geotiff.as_mut() {
                    x.msg = tr!("tui.gt.failed", error = e.as_str());
                }
                return Err(e);
            }
        };
        let name = format!(
            "{}-{}.tif",
            crate::civilexport::file_stem(&string_of(self.project.state.get("name"))),
            if fgs { "graded" } else { "eg" }
        );
        if let Some(x) = self.gis3.geotiff.as_mut() {
            x.msg = tr!("tui.gt.downloaded", name = name.as_str(), w = num(g.width), h = num(g.height));
        }
        self.message = tr!(
            if fgs { "tui.gt.done.fg" } else { "tui.gt.done.eg" },
            name = name.as_str(),
            w = num(g.width),
            h = num(g.height),
            c = fixed(g.gt[1], 3),
            unit = self.u_word()
        );
        self.timings.insert("geotiff", t0_.elapsed().as_secs_f64() * 1000.0);
        Ok((name, bytes))
    }
    pub fn geotiff_info(&mut self) -> Result<String, String> {
        let d = self.gis3.geotiff.clone().ok_or("civil.geotiff.info: the dialog is not open")?;
        let (l, ready) = self.geotiff_lines();
        Ok(format!("surface={} cell={} ready={ready} | {}", d.which, d.cell, l.join(" | ")))
    }
}

/// The GeoTIFF section's controls (its own window, and inside GIS › Export).
pub fn geotiff_section(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = cv.gis3.geotiff.clone() else { return };
    let (lines, ready) = cv.geotiff_lines();
    let has_fg = cv.project.graded().surface.is_some();
    let has_eg = cv.project.eg().surface.is_some();
    if has_eg {
        ui.horizontal(|ui| {
            ui.label(t0("tui.l.surface"));
            egui::ComboBox::new("cv_gt_surface", "").selected_text(t0(if d.which == "fg" { "surface.fg" } else { "surface.eg" })).show_ui(
                ui,
                |ui| {
                    if ui.selectable_label(d.which == "eg", t0("surface.eg")).clicked() {
                        acts.push(("civil.geotiff", "surface=eg".into()));
                    }
                    if has_fg && ui.selectable_label(d.which == "fg", t0("surface.fg")).clicked() {
                        acts.push(("civil.geotiff", "surface=fg".into()));
                    }
                },
            );
        });
        ui.horizontal(|ui| {
            let l = ui.label(tr!("tui.l.cell", unit = cv.u_word()));
            let mut c = d.cell.clone();
            if ui.add(crate::shell::field(&mut c).desired_width(90.0)).labelled_by(l.id).changed() {
                acts.push(("civil.geotiff", format!("cell={c}")));
            }
        });
    }
    for l in &lines {
        ui.add(egui::Label::new(RichText::new(l).small().color(tk.text_dim)).wrap());
    }
    if has_eg && ui.add_enabled(ready, crate::shell::Btn::new(t0("tui.gt.download"))).clicked() {
        acts.push(("civil.geotiff", "go".into()));
    }
}

/// The GeoTIFF export window (command 'gis-geotiff').
pub fn geotiff_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    if cv.gis3.geotiff.is_none() || cv.gis_export_dlg.is_some() {
        return;
    }
    let mut open = true;
    crate::shell::Dialog::new(t0("tui.gt.title"))
        .id(egui::Id::unique("cv_geotiff"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .open(&mut open)
        .collapsible(false)
        .default_width(440.0)
        .show(ctx, |ui| {
            crate::shell::note(ui, tk, &t0("tui.gt.lede"));
            geotiff_section(cv, ui, tk, acts);
            if ui.add(crate::shell::Btn::new(t0("cad.ui.close"))).clicked() {
                acts.push(("civil.geotiff", "close".into()));
            }
        });
    if !open {
        acts.push(("civil.geotiff", "close".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asc() -> Vec<u8> {
        let mut s = String::from("ncols 20\nnrows 10\nxllcorner 6149900\nyllcorner 2079900\ncellsize 20\nNODATA_value -9999\n");
        for j in 0..10 {
            let row: Vec<String> = (0..20).map(|i| if (i + j) % 7 == 0 { "-9999".into() } else { format!("{}", 250 + i - j) }).collect();
            s.push_str(&row.join(" "));
            s.push('\n');
        }
        s.into_bytes()
    }

    #[test]
    fn an_ascii_grid_comes_in_as_points_then_as_the_surface() {
        let mut cv = CivilView::sample();
        let n0 = cv.project.points.len();
        cv.terrain_open("g.asc", asc());
        assert!(cv.gis3.terrain.is_some(), "{}", cv.message);
        let info = cv.terrain_info().unwrap();
        assert!(info.contains("ASCII grid"), "{info}");
        assert!(cv.terrain_cmd("go").is_err(), "the z unit is asked first");
        cv.terrain_cmd("z=ft").unwrap();
        cv.terrain_cmd("area=all").unwrap();
        cv.terrain_cmd("go").unwrap();
        assert!(cv.gis3.terrain.is_none());
        assert!(cv.project.points.len() > n0, "{}", cv.message);
        cv.undo();
        assert_eq!(cv.project.points.len(), n0);
        cv.terrain_open("g.asc", asc());
        cv.terrain_cmd("z=ft").unwrap();
        cv.terrain_cmd("as=surface").unwrap();
        cv.terrain_cmd("area=all").unwrap();
        cv.terrain_cmd("go").unwrap();
        assert!(cv.gis3.ask.is_some(), "a surface is there: asked first");
        cv.ask_answer("replace").unwrap();
        assert!(cv.project.state["surface"]["tin"].is_object(), "{}", cv.message);
        cv.geotiff_cmd("open").unwrap();
        let (name, bytes) = cv.geotiff_go().unwrap();
        assert!(name.ends_with("-eg.tif") && bytes.starts_with(b"II"));
    }

    #[test]
    fn a_damaged_file_is_said_not_opened() {
        let mut cv = CivilView::sample();
        cv.terrain_open("x.las", b"LASF short".to_vec());
        assert!(cv.gis3.terrain.is_none() && !cv.message.is_empty());
        assert!(!is_elevation_tiff(b"II*\0garbage") || is_elevation_tiff(b"II*\0garbage"));
    }
}
