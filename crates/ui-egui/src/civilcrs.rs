//! GIS › Coordinates in full (gis-crs-ui.js runDialog): the project's system now, its grid shift file (0.4 V11), search
//! by code or name, the systems the site's coordinates fit, systems near a typed latitude / longitude, a .prj file (the
//! dialog's picker, or a lone .prj opened or dropped), the chosen system's facts and whether it can be the project's,
//! Grid / Ground (a combined scale factor typed or computed at a point) / Local (two control points, a check point),
//! Apply as one undo step — asking first whether the coordinates are already in the new system or should be converted
//! (crs-pick.js reprojectProject) — and Remove. The same dialog in "pick" mode is 0.5's `reg.pickCrs` / askCrs: the GIS
//! import's project or source system, a latitude / longitude point file's system.
//!
//! After the project's system changes, GIS layers are moved to where they belong in the new one (gis-vector-ui.js
//! crsChange): at once when the project's coordinates were converted, else after asking. 0.5's `host.ask` questions are
//! [`Ask`]s here: a window, and `civil.ask=<button>` for agents and tests.
//!
//! Every action is a command: `civil.crs.open`, `civil.crs.dlg=<action>`, `civil.crs.info`, `civil.ask=<button>`. Words
//! come from CivilCraft 0.5's catalog (crs.d.*, grid.*, vec.move.*).

use cc_civil::js::{num_to_string, str_to_number, string_of, to_fixed, truthy};
use cc_civil::tr;
use cc_io_civil::crs::{self, Local};
use cc_io_civil::crs_pick;
use egui::RichText;
use serde_json::{Map, Value, json};

use crate::civilgis::ensure_catalog;
use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// A .prj file larger than this is not one (gis-crs-ui.js readPrj: 64 KB).
pub const MAX_PRJ: usize = 64 * 1024;
/// A grid shift file kept in a project (gis-crs-ui.js: 20 MB).
pub const MAX_GRID: usize = 20 * 1024 * 1024;

/// `fx(v, d)`: `Number(v.toFixed(d)).toLocaleString('en-US', { maximumFractionDigits: d })`, '—' when not finite.
pub fn fx(v: f64, d: usize) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let x = str_to_number(&to_fixed(v, d));
    let mut s = cc_civil::units::grouped(x, d);
    if d > 0 && s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// `numIn(v)`: None for an empty field, NaN for one that is not a number.
fn num_in(v: &str) -> Option<f64> {
    let s: String = v.trim().chars().filter(|c| *c != ',').collect();
    if s.is_empty() {
        return None;
    }
    let n = str_to_number(&s);
    Some(if n.is_finite() { n } else { f64::NAN })
}

/// `sameBase(a, b)`: the same definition and code.
fn same_base(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.filter(|v| truthy(Some(v))), b.filter(|v| truthy(Some(v)))) {
        (Some(a), Some(b)) => {
            a.get("proj4") == b.get("proj4")
                && a.get("code").cloned().unwrap_or(Value::Null) == b.get("code").cloned().unwrap_or(Value::Null)
        }
        _ => false,
    }
}

/// `{ ...c, local: null }` (a key already there keeps its place).
fn without_local(c: &Value) -> Value {
    let mut m = c.as_object().cloned().unwrap_or_default();
    m.insert("local".into(), Value::Null);
    Value::Object(m)
}

/// Who asked for a system (0.5's pickCrs callers) — the answer goes back there.
#[derive(Clone, Debug, PartialEq)]
pub enum PickFor {
    /// the GIS import dialog's project system (gis-vector-ui.js chooseProject)
    GisProject,
    /// the system a GIS file without one is in (the import dialog's "unknown" choice)
    GisUnknown(usize),
    /// a latitude / longitude point file (gis-vector-ui.js pointsLatLon)
    PointsLatLon,
    /// an elevation grid or LiDAR file's own system (gis-terrain-ui.js "pick the file's system")
    TerrainFile,
    /// the project's system for a terrain file whose own cannot be the project's
    TerrainProject,
    /// a picture's system (gis-map-ui.js "where is this picture")
    MapPicture,
}

/// A control point of Local mode.
#[derive(Clone, Debug, Default)]
pub struct Cp {
    pub n: String,
    pub ge: String,
    pub gn: String,
    pub ll: String,
    pub xy: Option<[f64; 2]>,
    pub at: String,
    pub ge_ph: String,
    pub gn_ph: String,
}

/// The chosen system.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub crs: Value,
    pub code: Option<f64>,
    pub notes: Vec<String>,
}

/// A line of one of the dialog's boxes.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Note(String),
    Problem(String),
    Row(String, String),
}
impl Line {
    fn text(&self) -> String {
        match self {
            Line::Note(s) | Line::Problem(s) => s.clone(),
            Line::Row(k, v) => format!("{k}: {v}"),
        }
    }
}

/// "Systems these coordinates fit": (code, lon, lat) of the first ones, and how many fit.
type Fits = (Vec<(f64, f64, f64)>, usize);

/// The dialog (one at a time, as 0.5's `openNow`).
#[derive(Clone, Debug)]
pub struct CrsDlg {
    pub pick: bool,
    pub for_project: bool,
    pub reason: String,
    pub pick_for: Option<PickFor>,
    pub current: Option<Value>,
    pub chosen: Option<Chosen>,
    pub mid: Option<[f64; 2]>,
    pub has_data: bool,
    pub here: Option<[f64; 2]>,
    pub q: String,
    pub near: String,
    pub list: Vec<f64>,
    pub extra: Vec<String>,
    pub head: String,
    pub gmode: String,
    pub csf: String,
    pub be: String,
    pub bn: String,
    pub ae: String,
    pub an: String,
    pub az: String,
    pub factors: Vec<Line>,
    pub usef: Option<String>,
    pub cps: [Cp; 3],
    pub local: Vec<Line>,
    pub local_fit: Option<Local>,
    pub problem: Option<String>,
    pub grid_msg: Option<(String, bool)>,
    pub picking: Option<String>,
    fits: Option<Fits>,
    /// the files the .prj arrived with (a lone .prj may be the first part of a shapefile set)
    pub prj_file: Option<(String, Vec<u8>)>,
}

/// One of 0.5's `host.ask` questions.
#[derive(Clone, Debug)]
pub struct Ask {
    pub kind: AskKind,
    pub title: String,
    pub text: String,
    /// (button id, label); the last is the main one
    pub buttons: Vec<(&'static str, String)>,
}
#[derive(Clone, Debug)]
pub enum AskKind {
    /// Apply in the Coordinates window: are the coordinates already in the new system? (the dialog stays open)
    CrsConvert(Value),
    /// move the GIS layers from one system to another
    MoveLayers(Value, Value),
    /// another part's question (terrain, CAD linework …) with what it needs to go on
    Other(String),
}

/// What a dialog action needs from the host (the platform's picker, the network).
#[derive(Clone, Debug, PartialEq)]
pub enum Need {
    PickPrj,
    PickGrid,
    /// GIS › Import on the web: the picked files go through Open
    PickGis,
    Fetch(String),
}

/// The state the parts after M11.25 keep on the view.
#[derive(Clone, Default)]
pub struct Gis3 {
    pub ask: Option<Ask>,
    /// a latitude / longitude point file waiting for a system (the import dialog's result)
    pub latlon_wait: Option<crate::civilimport::ImportDlg>,
    /// the picker the host was asked for (web: the files arrive later)
    pub pick_wait: Option<Need>,
    /// the grid shift file being downloaded
    pub grid_fetch: Option<String>,
    /// the "Use for the surface" dialog of a GIS layer
    pub use_dlg: Option<crate::civilgis3::UseDlg>,
    /// the terrain import dialog (elevation grids, LiDAR)
    pub terrain: Option<Box<crate::civilterrain::TerrainDlg>>,
    /// the GeoTIFF export dialog (a surface as an elevation grid)
    pub geotiff: Option<crate::civilterrain::GeoTiffDlg>,
    /// the DWG / DXF linework dialog
    pub cad: Option<Box<crate::civilcad::CadDlg>>,
    /// a long press fired for the press still held (touch)
    pub long_fired: bool,
    /// the 3D view's surface colour: "" / bands, single, map (site-view3d.js state.color)
    pub color3d: String,
    /// the drape's note (no map layer to drape, the layers' attribution)
    pub drape_note: String,
    /// a picture waiting for its system (gis-map-ui.js crsForFile)
    pub pic_wait: Option<crate::civilpics::PicWait>,
}

impl CivilView {
    fn crs_now(&self) -> Option<Value> {
        self.project.state.get("crs").filter(|c| truthy(Some(c))).cloned()
    }
    fn units_now(&self) -> String {
        self.project.units().to_string()
    }

    /// Open the Coordinates window (project mode), or the pick dialog (`pick` = (reason, near lon/lat, purpose, who)).
    pub fn crs_open(&mut self, pick: Option<(String, Option<[f64; 2]>, &str, PickFor)>) {
        if self.crs_dlg.is_some() {
            return;
        }
        let t_open = web_time::Instant::now();
        ensure_catalog();
        let current = self.crs_now();
        let (is_pick, reason, near, purpose, pick_for) = match pick {
            Some((r, n, p, f)) => (true, r, n, p.to_string(), Some(f)),
            None => (false, String::new(), None, "project".to_string(), None),
        };
        let for_project = !is_pick || purpose != "source";
        let chosen = match (&current, is_pick) {
            (Some(c), false) => Some(Chosen {
                crs: without_local(c),
                code: c
                    .get("code")
                    .filter(|v| !v.is_null())
                    .and_then(Value::as_f64)
                    .filter(|code| crs::by_code(*code).ok().flatten().is_some()),
                notes: Vec::new(),
            }),
            _ => None,
        };
        let ext = crate::civilplan::extents(&mut self.project);
        let has_data = ext.is_some() && (!self.project.points.is_empty() || self.project.eg().surface.is_some());
        let mid = ext.map(|(a, b, c, d)| [(a + c) / 2.0, (b + d) / 2.0]);
        let mut here = near.filter(|n| n.iter().all(|v| v.is_finite()));
        if here.is_none()
            && let (Some(c), Some(m)) = (&current, mid)
        {
            here = crs::frame(c).ok().flatten().and_then(|f| f.to_lon_lat(m[0], m[1]).ok()).map(crs::nn);
        }
        // the current local step (Ground when it has no rotation and the same base both ways)
        let cur = current.as_ref().and_then(|c| crs::read_local(c.get("local")));
        let cur_ground = cur.is_some_and(|l| l.rotation == 0.0 && l.from == l.to);
        let gmode = match cur {
            None => "grid",
            Some(_) if cur_ground => "ground",
            Some(_) => "local",
        };
        let mut cps: [Cp; 3] = Default::default();
        if let Some(l) = cur.filter(|_| !cur_ground) {
            cps[0].xy = Some(l.from);
            cps[0].ge = num_to_string(l.to[0]);
            cps[0].gn = num_to_string(l.to[1]);
            cps[0].at = tr!("crs.d.en", e = fx(l.from[0], 3), n = fx(l.from[1], 3));
        }
        let d = CrsDlg {
            pick: is_pick,
            for_project,
            reason,
            pick_for,
            current,
            chosen,
            mid,
            has_data,
            here,
            q: String::new(),
            near: here.map(|h| format!("{}, {}", to_fixed(h[1], 5), to_fixed(h[0], 5))).unwrap_or_default(),
            list: Vec::new(),
            extra: Vec::new(),
            head: String::new(),
            gmode: gmode.into(),
            csf: cur.filter(|_| cur_ground).map(|l| num_to_string(l.scale)).unwrap_or_default(),
            be: cur.filter(|_| cur_ground).map_or_else(|| "0".into(), |l| num_to_string(l.from[0])),
            bn: cur.filter(|_| cur_ground).map_or_else(|| "0".into(), |l| num_to_string(l.from[1])),
            ae: mid.map(|m| to_fixed(m[0], 2)).unwrap_or_default(),
            an: mid.map(|m| to_fixed(m[1], 2)).unwrap_or_default(),
            az: String::new(),
            factors: Vec::new(),
            usef: None,
            cps,
            local: Vec::new(),
            local_fit: None,
            problem: None,
            grid_msg: None,
            picking: None,
            fits: None,
            prj_file: None,
        };
        self.crs_dlg = Some(d);
        self.crs_render_chosen();
        if self.crs_dlg.as_ref().is_some_and(|d| d.gmode == "local") {
            self.crs_render_local();
        }
        self.crs_default_list();
        self.timings.insert("crs open", t_open.elapsed().as_secs_f64() * 1000.0);
    }

    // ------------------------------------------------------------------------------------------------- lists

    fn crs_show_list(&mut self, head: String, codes: Vec<f64>, extra: Vec<String>) {
        if let Some(d) = self.crs_dlg.as_mut() {
            d.head = head;
            d.extra = if extra.len() == codes.len() { extra } else { vec![String::new(); codes.len()] };
            d.list = codes;
        }
    }
    fn crs_search(&mut self) {
        let Some(d) = self.crs_dlg.as_ref() else { return };
        let qt = d.q.trim().to_string();
        if qt.is_empty() {
            self.crs_default_list();
            return;
        }
        let (here, for_project) = (d.here, d.for_project);
        let hits: Vec<f64> = crs::search(&qt, here, 60.0)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !for_project || !e.geographic)
            .map(|e| e.code)
            .collect();
        let count = if hits.len() == 60 { format!("{}+", hits.len()) } else { hits.len().to_string() };
        let qs = cc_civil::js::slice16(&qt, 60);
        let head = if hits.is_empty() {
            tr!("crs.d.nomatch", q = qs.as_str())
        } else {
            tr!(if here.is_some() { "crs.d.match.near" } else { "crs.d.match" }, count = count.as_str(), q = qs.as_str())
        };
        self.crs_show_list(head, hits, Vec::new());
    }
    fn crs_fits(&mut self) {
        let Some(d) = self.crs_dlg.as_ref() else { return };
        let Some(mid) = d.mid else { return };
        let units = self.units_now();
        if d.fits.is_none() {
            let (l, total) = crs_pick::systems_that_fit(Some(mid[0]), Some(mid[1]), &units, 60.0).unwrap_or_default();
            let v: Vec<(f64, f64, f64)> = l.iter().map(|f| (f.entry.code, f.lon, f.lat)).collect();
            if let Some(d) = self.crs_dlg.as_mut() {
                d.fits = Some((v, total));
            }
        }
        let Some((l, total)) = self.crs_dlg.as_ref().and_then(|d| d.fits.clone()) else { return };
        let unit = t0(if units == "m" { "crs.unit.m" } else { "gread.unit.ft" });
        let (e, n) = (fx(mid[0], 0), fx(mid[1], 0));
        let head = if total > 0 {
            let mut h = tr!("crs.d.fit", n = total, unit = unit.as_str(), e = e.as_str(), n2 = n.as_str());
            if total > l.len() {
                h.push(' ');
                h.push_str(&tr!("crs.d.fit.first", n = l.len()));
            }
            h
        } else {
            tr!("crs.d.fit.none", unit = unit.as_str(), e = e.as_str(), n = n.as_str())
        };
        let extra =
            l.iter().map(|(_, lon, lat)| format!(" · {}", tr!("crs.d.middleat", at = crs_pick::lat_lon_text(*lon, *lat, "dd")))).collect();
        self.crs_show_list(head, l.iter().map(|x| x.0).collect(), extra);
    }
    fn crs_near(&mut self) {
        let Some(d) = self.crs_dlg.as_ref() else { return };
        let Some(ll) = crs_pick::parse_lat_lon(&d.near) else {
            if let Some(d) = self.crs_dlg.as_mut() {
                d.head = t0("crs.d.near.bad");
                d.list.clear();
                d.extra.clear();
            }
            return;
        };
        let unit = if d.for_project { Some(self.units_now()) } else { None };
        if let Some(d) = self.crs_dlg.as_mut() {
            d.here = Some(ll);
        }
        let sg: Vec<f64> = crs::suggest(ll[0], ll[1], unit.as_deref(), 40.0).unwrap_or_default().iter().map(|e| e.code).collect();
        let head =
            if sg.is_empty() { t0("crs.d.near.none") } else { tr!("crs.d.near.list", place = crs_pick::lat_lon_text(ll[0], ll[1], "dd")) };
        self.crs_show_list(head, sg, Vec::new());
    }
    fn crs_default_list(&mut self) {
        let Some(d) = self.crs_dlg.as_ref() else { return };
        if d.has_data && d.current.is_none() && !d.pick {
            self.crs_fits();
            return;
        }
        if let Some(h) = d.here {
            if let Some(d) = self.crs_dlg.as_mut() {
                d.near = format!("{}, {}", to_fixed(h[1], 5), to_fixed(h[0], 5));
            }
            self.crs_near();
            return;
        }
        if d.has_data {
            self.crs_fits();
            return;
        }
        self.crs_show_list(t0("crs.d.list.start"), Vec::new(), Vec::new());
    }

    // ------------------------------------------------------------------------------------------------- chosen

    fn crs_choose(&mut self, c: Option<Chosen>) {
        if let Some(d) = self.crs_dlg.as_mut() {
            d.chosen = c;
        }
        self.crs_render_chosen();
    }
    /// The chosen box's lines (renderChosen) and the problem under it.
    pub fn crs_chosen_lines(&self) -> Vec<Line> {
        let Some(d) = self.crs_dlg.as_ref() else { return Vec::new() };
        let Some(ch) = &d.chosen else {
            return vec![Line::Note(t0(if d.pick {
                "crs.d.chosen.none"
            } else if d.current.is_some() {
                "crs.d.chosen.keep"
            } else {
                "crs.d.chosen.none"
            }))];
        };
        let mut out = facts(&crs_pick::describe(&ch.crs));
        out.extend(ch.notes.iter().map(|n| Line::Note(n.clone())));
        if let Some(mid) = d.mid.filter(|_| d.has_data)
            && ch.crs.get("unit").and_then(Value::as_str) != Some("deg")
        {
            let ll = crs::frame(&without_local(&ch.crs)).ok().flatten().and_then(|f| f.to_lon_lat(mid[0], mid[1]).ok()).map(crs::nn);
            match ll {
                Some([lon, lat]) => {
                    let area = ch.code.and_then(|c| crs::by_code(c).ok().flatten()).and_then(|e| e.area);
                    let inside = area.map(|a| {
                        lat >= a[1] && lat <= a[3] && if a[0] <= a[2] { lon >= a[0] && lon <= a[2] } else { lon >= a[0] || lon <= a[2] }
                    });
                    let key = match inside {
                        Some(false) => "crs.d.mid.out",
                        Some(true) => "crs.d.mid.in",
                        None => "crs.d.mid",
                    };
                    out.push(Line::Note(tr!(key, at = crs_pick::lat_lon_text(lon, lat, &self.pt_settings.latlon))));
                }
                None => out.push(Line::Note(t0("crs.d.mid.cannot"))),
            }
        }
        out
    }
    fn crs_render_chosen(&mut self) {
        let units = self.units_now();
        let Some(d) = self.crs_dlg.as_mut() else { return };
        d.problem = d.chosen.as_ref().and_then(|ch| {
            if d.for_project {
                crs::crs_problem(&ch.crs, &units)
            } else if !truthy(ch.crs.get("proj4")) {
                Some(t0("crs.p.nodef"))
            } else {
                None
            }
        });
    }
    /// Apply's state in pick mode: None = on, Some(why) = off.
    pub fn crs_apply_off(&self) -> Option<String> {
        let d = self.crs_dlg.as_ref()?;
        if !d.pick {
            return None;
        }
        match &d.chosen {
            None => Some(t0("crs.d.pickfirst")),
            Some(ch) if d.for_project => crs::crs_problem(&ch.crs, self.project.units()),
            Some(_) => None,
        }
    }

    // ------------------------------------------------------------------------------------------------- grid / ground / local

    fn crs_base(&self) -> Option<Value> {
        let d = self.crs_dlg.as_ref()?;
        match (&d.chosen, &d.current) {
            (Some(ch), _) => Some(ch.crs.clone()),
            (None, Some(c)) => Some(without_local(c)),
            _ => None,
        }
    }
    fn crs_compute(&mut self) {
        let base = self.crs_base();
        let units = self.units_now();
        let latlon = self.pt_settings.latlon.clone();
        let Some(d) = self.crs_dlg.as_ref() else { return };
        let set = |cv: &mut CivilView, lines: Vec<Line>, usef: Option<String>| {
            if let Some(d) = cv.crs_dlg.as_mut() {
                d.factors = lines;
                d.usef = usef;
            }
        };
        let Some(b) = base else {
            set(self, vec![Line::Problem(t0("crs.d.pickfirst2"))], None);
            return;
        };
        let (x, y) = (num_in(&d.ae), num_in(&d.an));
        let (Some(x), Some(y)) = (x.filter(|v| v.is_finite()), y.filter(|v| v.is_finite())) else {
            set(self, vec![Line::Problem(t0("crs.d.factor.en"))], None);
            return;
        };
        let z = match num_in(&d.az) {
            Some(z) => z,
            None => {
                let g = self.project.eg().surface.as_ref().and_then(|s| s.z_at(x, y)).filter(|g| g.is_finite());
                if let Some(d) = self.crs_dlg.as_mut() {
                    d.az = g.map_or_else(|| "0".into(), |g| to_fixed(g, 2));
                }
                g.unwrap_or(0.0)
            }
        };
        if !z.is_finite() {
            set(self, vec![Line::Problem(t0("crs.d.factor.z"))], None);
            return;
        }
        let current = self.crs_dlg.as_ref().and_then(|d| d.current.clone());
        let with = match &current {
            Some(c) if same_base(Some(c), Some(&b)) && truthy(c.get("local")) => {
                let mut m = b.as_object().cloned().unwrap_or_default();
                m.insert("local".into(), c.get("local").cloned().unwrap_or(Value::Null));
                Value::Object(m)
            }
            _ => b,
        };
        let r = crs_pick::ground_factor(&with, x, y, Some(&json!(z))).ok();
        let k = r.as_ref().and_then(|r| r.get("k")).and_then(Value::as_f64).filter(|k| k.is_finite());
        let (Some(r), Some(k)) = (r, k) else {
            set(self, vec![Line::Problem(t0("crs.d.factor.out"))], None);
            return;
        };
        let g = |key: &str| r.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let ppm = |v: f64| format!("{} ppm", to_fixed((v - 1.0) * 1e6, 1));
        let unit = if units == "m" { "m" } else { "ft" };
        let comb = to_fixed(g("combined"), 9);
        let lines = vec![
            Line::Row(t0("crs.d.k"), tr!("crs.d.k.v", k = to_fixed(k, 9), ppm = ppm(k))),
            Line::Row(
                t0("crs.d.ef"),
                tr!("crs.d.ef.v", f = to_fixed(g("elevation"), 9), ppm = ppm(g("elevation")), z = fx(z, 2), unit = unit),
            ),
            Line::Row(t0("crs.d.cf"), tr!("crs.d.cf.v", f = comb.as_str())),
            Line::Row(t0("crs.d.conv"), tr!("crs.d.conv.v", deg = to_fixed(g("convergence"), 6))),
            Line::Row(t0("crs.d.latlon"), crs_pick::lat_lon_text(g("lon"), g("lat"), &latlon)),
            Line::Note(t0("crs.d.geoid")),
        ];
        set(self, lines, Some(comb));
    }
    fn crs_cp_point(&mut self, i: usize) -> Option<[f64; 2]> {
        let d = self.crs_dlg.as_ref()?;
        let c = d.cps.get(i)?;
        let v = c.n.trim().to_string();
        let (xy, at) = if !v.is_empty() {
            match self.project.points.iter().find(|p| p.n == v) {
                Some(p) => (Some([p.x, p.y]), tr!("crs.d.en", e = fx(p.x, 3), n = fx(p.y, 3))),
                None => (None, tr!("crs.d.nopoint", n = cc_civil::js::slice16(&v, 20))),
            }
        } else if let Some(xy) = c.xy {
            (Some(xy), tr!("crs.d.en", e = fx(xy[0], 3), n = fx(xy[1], 3)))
        } else {
            (None, String::new())
        };
        let c = self.crs_dlg.as_mut()?.cps.get_mut(i)?;
        c.xy = xy;
        c.at = at;
        xy
    }
    /// `cpGrid(i)` → Ok(Some(grid)) | Ok(None) | Err(why).
    fn crs_cp_grid(&mut self, i: usize) -> Result<Option<[f64; 2]>, String> {
        let b = self.crs_base();
        let Some(c) = self.crs_dlg.as_ref().and_then(|d| d.cps.get(i)).cloned() else { return Ok(None) };
        let ll = c.ll.trim();
        if !ll.is_empty() {
            let Some(l) = crs_pick::parse_lat_lon(ll) else { return Err(tr!("crs.d.cp.err.ll", n = i + 1)) };
            let Some(b) = b else { return Err(t0("crs.d.pickfirst2")) };
            return match crs_pick::grid_of(&b, l[0], l[1]) {
                Ok(g) => {
                    let g = crs::nn(g);
                    if let Some(c) = self.crs_dlg.as_mut().and_then(|d| d.cps.get_mut(i)) {
                        c.ge_ph = fx(g[0], 3);
                        c.gn_ph = fx(g[1], 3);
                    }
                    Ok(Some(g))
                }
                Err(_) => Err(tr!("crs.d.cp.err.out", n = i + 1)),
            };
        }
        let (e, n) = (num_in(&c.ge), num_in(&c.gn));
        match (e, n) {
            (None, None) => Ok(None),
            (Some(e), Some(n)) if e.is_finite() && n.is_finite() => Ok(Some([e, n])),
            _ => Err(tr!("crs.d.cp.err.num", n = i + 1)),
        }
    }
    fn crs_render_local(&mut self) {
        let units = self.units_now();
        if let Some(d) = self.crs_dlg.as_mut() {
            d.local_fit = None;
        }
        let (a, b, c3) = (self.crs_cp_point(0), self.crs_cp_point(1), self.crs_cp_point(2));
        let (ga, gb, gc) = (self.crs_cp_grid(0), self.crs_cp_grid(1), self.crs_cp_grid(2));
        let set = |cv: &mut CivilView, lines: Vec<Line>, fit: Option<Local>| {
            if let Some(d) = cv.crs_dlg.as_mut() {
                d.local = lines;
                d.local_fit = fit;
            }
        };
        let err = ga.as_ref().err().or(gb.as_ref().err()).cloned();
        if let Some(e) = err {
            set(self, vec![Line::Problem(e)], None);
            return;
        }
        let (Some(a), Some(b), Ok(Some(ga)), Ok(Some(gb))) = (a, b, ga, gb) else {
            set(self, vec![Line::Note(t0("crs.d.local.need"))], None);
            return;
        };
        let fit = match crs::local_from_pairs(&json!([[a, ga], [b, gb]])) {
            Ok(f) => f,
            Err(e) => {
                set(self, vec![Line::Problem(e)], None);
                return;
            }
        };
        let mut lines = vec![
            Line::Row(t0("crs.d.l.scale"), tr!("crs.d.l.scale.v", s = to_fixed(fit.scale, 9), ppm = to_fixed((fit.scale - 1.0) * 1e6, 1))),
            Line::Row(t0("crs.d.l.rot"), tr!("crs.d.l.rot.v", deg = to_fixed(fit.rotation, 6))),
            Line::Row(t0("crs.d.l.fit"), t0("crs.d.l.fit.v")),
        ];
        let gcv = gc.as_ref().ok().copied().flatten();
        match (c3, gcv) {
            (Some(c3), Some(g)) => {
                let m = crs_pick::misclosure(&fit, c3, g);
                let f = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let lim = 0.1 * if units == "m" { 0.3048 } else { 1.0 };
                let ppm = m.get("ppm").and_then(Value::as_f64);
                let unit = if units == "m" { "m" } else { "ft" };
                let text = tr!(
                    if ppm.is_some() { "crs.d.check.ppm" } else { "crs.d.check" },
                    d = fx(f("dist"), 3),
                    unit = unit,
                    dx = fx(f("dx"), 3),
                    dy = fx(f("dy"), 3),
                    ppm = ppm.map(|p| to_fixed(p, 0)).unwrap_or_default()
                );
                lines.push(if f("dist") > lim { Line::Problem(text) } else { Line::Note(text) });
            }
            (c3, g) if c3.is_some() || g.is_some() => lines.push(Line::Note(t0("crs.d.check.need"))),
            _ => {}
        }
        if let Err(e) = gc {
            lines.push(Line::Problem(e));
        }
        set(self, lines, Some(fit));
    }
    /// `localNow()` → Ok(local or null) | Err(why).
    fn crs_local_now(&mut self) -> Result<Value, String> {
        let Some(d) = self.crs_dlg.as_ref() else { return Ok(Value::Null) };
        match d.gmode.as_str() {
            "grid" => Ok(Value::Null),
            "ground" => {
                let k = num_in(&d.csf).unwrap_or(f64::NAN);
                if !(k > 0.9 && k < 1.1) {
                    return Err(t0("crs.d.err.csf"));
                }
                let (e, n) = (num_in(&d.be), num_in(&d.bn));
                let (Some(e), Some(n)) = (e.filter(|v| v.is_finite()), n.filter(|v| v.is_finite())) else {
                    return Err(t0("crs.d.err.base"));
                };
                Ok(cc_civil::js::J::j(&crs::ground_local(k, [e, n])))
            }
            _ => {
                self.crs_render_local();
                let fit = self.crs_dlg.as_ref().and_then(|d| d.local_fit).ok_or_else(|| t0("crs.d.err.local"))?;
                if !(fit.scale > 0.5 && fit.scale < 2.0) {
                    return Err(tr!("crs.d.err.scale", s = to_fixed(fit.scale, 4)));
                }
                Ok(cc_civil::js::J::j(&fit))
            }
        }
    }

    // ------------------------------------------------------------------------------------------------- grid shift files

    /// The grid section's lines (renderGrid), or None when it is hidden.
    pub fn crs_grid_lines(&self) -> Option<Vec<(String, Line)>> {
        let d = self.crs_dlg.as_ref()?;
        let c = d.current.as_ref()?;
        if d.pick {
            return None;
        }
        let sug = crs::suggest_grids(c).unwrap_or_default();
        let entry = c.get("code").filter(|v| !v.is_null()).and_then(|v| crs::by_code_val(Some(v)).ok().flatten());
        // acc: null for an entry without one, undefined without an entry
        let rough = entry.is_some_and(|e| e.shift_accuracy.is_none_or(|a| a > 1.0));
        let grid = c.get("grid").filter(|g| truthy(Some(g))).map(cc_civil::js::to_js_string);
        let nad27 = string_of(c.get("name")).to_ascii_uppercase().contains("NAD27");
        if grid.is_none() && sug.is_empty() && !rough && !nad27 {
            return None;
        }
        let g = grid.as_deref().and_then(crs::grid_info);
        let mut out = vec![("head".to_string(), Line::Note(t0("grid.head")))];
        out.push((
            "status".into(),
            Line::Note(match (&grid, &g) {
                (Some(_), Some(g)) => tr!(
                    "grid.using",
                    name = g.name.as_str(),
                    kind = if g.kind == "ntv2" { "NTv2" } else { "GeoTIFF" },
                    w = to_fixed(g.extent[0], 2),
                    s = to_fixed(g.extent[1], 2),
                    e = to_fixed(g.extent[2], 2),
                    n = to_fixed(g.extent[3], 2)
                ),
                (Some(n), None) => tr!("grid.shift.missing", name = n.as_str()),
                (None, _) => t0("grid.none"),
            }),
        ));
        if let (Some(g), Some(h)) = (&g, d.here) {
            let inside = h[0] >= g.extent[0] && h[0] <= g.extent[2] && h[1] >= g.extent[1] && h[1] <= g.extent[3];
            out.push(("inside".into(), if inside { Line::Note(t0("grid.inside")) } else { Line::Problem(t0("grid.outside")) }));
        }
        for q in &sug {
            let name = string_of(q.get("name"));
            if grid.as_deref() != Some(name.as_str()) || g.is_none() {
                let size = q.get("size").and_then(Value::as_f64).unwrap_or(0.0);
                let label = tr!(
                    "grid.cdn",
                    label = string_of(q.get("label")),
                    size = to_fixed(size / 1_048_576.0, if size < 1_048_576.0 { 2 } else { 1 })
                );
                out.push((format!("cdn:{name}"), Line::Note(label)));
            }
        }
        out.push(("file".into(), Line::Note(t0("grid.file"))));
        if grid.is_some() {
            out.push(("off".into(), Line::Note(t0("grid.off"))));
        }
        if let Some((m, bad)) = &d.grid_msg {
            out.push(("msg".into(), if *bad { Line::Problem(m.clone()) } else { Line::Note(m.clone()) }));
        }
        out.push(("note".into(), Line::Note(t0("grid.note"))));
        Some(out)
    }
    /// `useGrid(name, bytes)`: the grid becomes the project's datum shift (one undo step) and is kept in the project.
    pub fn crs_use_grid(&mut self, name: &str, bytes: Vec<u8>) {
        if bytes.len() > MAX_GRID {
            self.crs_grid_msg(t0("grid.big"), true);
            return;
        }
        let info = match crs::load_grid(name, &bytes) {
            Ok(i) => i,
            Err(e) => {
                self.crs_grid_msg(e, true);
                return;
            }
        };
        let before = self.crs_now();
        match self.project.crs_grids.iter_mut().find(|(n, _)| *n == info.name) {
            Some(slot) => slot.1 = bytes,
            None => self.project.crs_grids.push((info.name.clone(), bytes)),
        }
        let nm = info.name.clone();
        self.project.change(&t0("grid.undo.on"), &mut |s: &mut Value| {
            if let Some(c) = s.get_mut("crs").and_then(Value::as_object_mut) {
                c.insert("grid".into(), Value::String(nm.clone()));
            }
        });
        let after = self.crs_now();
        if let Some(d) = self.crs_dlg.as_mut() {
            d.current = after.clone();
        }
        self.crs_changed(before, after, false);
        let done = tr!("grid.done", name = info.name.as_str());
        self.crs_grid_msg(done.clone(), false);
        self.message = done;
    }
    fn crs_grid_off(&mut self) {
        let before = self.crs_now();
        self.project.change(&t0("grid.undo.off"), &mut |s: &mut Value| {
            if let Some(c) = s.get_mut("crs").and_then(Value::as_object_mut) {
                c.shift_remove("grid");
            }
        });
        let after = self.crs_now();
        if let Some(d) = self.crs_dlg.as_mut() {
            d.current = after.clone();
        }
        self.crs_changed(before, after, false);
        self.crs_grid_msg(t0("grid.offdone"), false);
    }
    fn crs_grid_msg(&mut self, m: String, bad: bool) {
        if let Some(d) = self.crs_dlg.as_mut() {
            d.grid_msg = Some((m, bad));
        }
    }
    /// A downloaded grid (or why it was not).
    pub fn crs_grid_fetched(&mut self, name: &str, r: Result<Vec<u8>, String>) {
        self.gis3.grid_fetch = None;
        match r {
            Ok(b) => self.crs_use_grid(name, b),
            Err(e) => self.crs_grid_msg(tr!("grid.fetch.fail", msg = e), true),
        }
    }

    // ------------------------------------------------------------------------------------------------- .prj

    /// `readPrj(file)`.
    pub fn crs_prj(&mut self, name: &str, bytes: &[u8]) {
        let set_head = |cv: &mut CivilView, h: String| {
            if let Some(d) = cv.crs_dlg.as_mut() {
                d.head = h;
            }
        };
        if bytes.len() > MAX_PRJ {
            set_head(self, tr!("crs.d.prj.big", file = name));
            return;
        }
        let text = String::from_utf8_lossy(bytes);
        crs::set_wkt_catalog();
        match crs::crs_from_prj(&text) {
            Ok(r) => {
                let code = r.matched.map(|e| e.code);
                let mut notes = vec![match code {
                    Some(c) => tr!("crs.d.prj.same", file = name, code = c),
                    None => tr!("crs.d.prj.from", file = name),
                }];
                notes.extend(r.notes);
                self.crs_choose(Some(Chosen { crs: r.crs, code, notes }));
                set_head(self, tr!("crs.d.prj.read", file = name));
                if let Some(c) = code {
                    self.crs_show_list(tr!("crs.d.prj.this", file = name), vec![c], Vec::new());
                }
            }
            Err(e) => {
                set_head(self, tr!("gread.file.note", file = name, note = e));
                if let Some(d) = self.crs_dlg.as_mut() {
                    d.list.clear();
                    d.extra.clear();
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------------- pick on the plan

    /// A click on the plan while the dialog steps aside (pickOnPlan): the nearest survey point within 10 px.
    pub fn crs_pick_click(&mut self, x: f64, y: f64) {
        let Some(what) = self.crs_dlg.as_ref().and_then(|d| d.picking.clone()) else { return };
        let s = self.css(x, y);
        let near = self.nearest_px(s, 10.0, false).and_then(|i| self.project.points.get(i)).cloned();
        let Some(d) = self.crs_dlg.as_mut() else { return };
        d.picking = None;
        if what == "factor" {
            let (px, py) = near.as_ref().map_or((x, y), |p| (p.x, p.y));
            d.ae = to_fixed(px, 3);
            d.an = to_fixed(py, 3);
            d.az.clear();
            self.crs_compute();
            return;
        }
        let i: usize = what.strip_prefix("cp").and_then(|n| n.parse().ok()).unwrap_or(0).min(2);
        match near {
            Some(p) => {
                if let Some(c) = d.cps.get_mut(i) {
                    c.n = p.n.clone();
                    c.xy = None;
                }
                self.crs_render_local();
            }
            None => self.message = t0("crs.d.click.miss"),
        }
    }
    pub fn crs_picking(&self) -> bool {
        self.crs_dlg.as_ref().is_some_and(|d| d.picking.is_some())
    }

    // ------------------------------------------------------------------------------------------------- apply / remove

    fn crs_finish(&mut self, v: Option<Value>) {
        let Some(d) = self.crs_dlg.take() else { return };
        if let (Some(f), true) = (d.pick_for, d.pick) {
            self.crs_answer(f, v);
        }
    }
    fn crs_apply(&mut self) -> Result<(), String> {
        let Some(d) = self.crs_dlg.as_ref() else { return Err("civil.crs.dlg: the Coordinates window is not open".into()) };
        let units = self.units_now();
        if d.pick {
            let Some(ch) = d.chosen.clone() else { return Ok(()) };
            let prob = if d.for_project { crs::crs_problem(&ch.crs, &units) } else { None };
            if let Some(p) = prob {
                if let Some(d) = self.crs_dlg.as_mut() {
                    d.problem = Some(p);
                }
                return Ok(());
            }
            self.crs_finish(Some(without_local(&ch.crs)));
            return Ok(());
        }
        let Some(base) = self.crs_base() else {
            if let Some(d) = self.crs_dlg.as_mut() {
                d.problem = Some(t0("crs.d.apply.none"));
            }
            return Ok(());
        };
        let local = match self.crs_local_now() {
            Ok(l) => l,
            Err(e) => {
                if let Some(d) = self.crs_dlg.as_mut() {
                    d.problem = Some(e);
                }
                return Ok(());
            }
        };
        let current = self.crs_dlg.as_ref().and_then(|d| d.current.clone());
        let mut m: Map<String, Value> = base.as_object().cloned().unwrap_or_default();
        m.insert("local".into(), local);
        match &current {
            Some(c) if truthy(c.get("grid")) && same_base(Some(c), Some(&base)) => {
                m.insert("grid".into(), c.get("grid").cloned().unwrap_or(Value::Null));
            }
            _ => {
                m.shift_remove("grid");
            }
        }
        let next = Value::Object(m);
        if let Some(p) = crs::crs_problem(&next, &units) {
            if let Some(d) = self.crs_dlg.as_mut() {
                d.problem = Some(p);
            }
            return Ok(());
        }
        if current.as_ref().is_some_and(|c| cc_civil::project::js_stringify(c) == cc_civil::project::js_stringify(&next)) {
            self.crs_finish(None);
            return Ok(());
        }
        let has_work = !self.project.points.is_empty()
            || !cc_civil::js::arr(self.project.state.get("alignments")).is_empty()
            || !cc_civil::js::arr(self.project.state.get("pads")).is_empty()
            || truthy(self.project.state.get("network"));
        if let Some(c) = current.as_ref().filter(|c| !same_base(Some(c), Some(&base)) && has_work) {
            let (old, new) = (string_of(c.get("name")), string_of(base.get("name")));
            self.gis3.ask = Some(Ask {
                kind: AskKind::CrsConvert(next),
                title: tr!("crs.d.ask.title", crs = new.as_str()),
                text: tr!("crs.d.ask.text", old = old.as_str(), crs = new.as_str()),
                buttons: vec![("cancel", t0("vec.cancel")), ("convert", t0("crs.d.ask.convert")), ("yes", t0("crs.d.ask.yes"))],
            });
            return Ok(());
        }
        self.crs_apply_next(next, false);
        Ok(())
    }
    /// Apply's end: `next` set, converting the project's coordinates first when asked.
    fn crs_apply_next(&mut self, next: Value, convert: bool) {
        let before = self.crs_dlg.as_ref().and_then(|d| d.current.clone());
        let name = string_of(next.get("name"));
        if convert {
            let Some(cur) = before.clone() else { return };
            let res = self.crs_convert(&cur, &next);
            match res {
                Ok(n) => self.message = tr!("crs.d.converted", count = cc_civil::units::grouped(n, 0), crs = name.as_str()),
                Err(e) => {
                    if let Some(d) = self.crs_dlg.as_mut() {
                        d.problem = Some(e);
                    }
                    return;
                }
            }
        } else {
            let nx = next.clone();
            self.project.change(&tr!("crs.d.undo.set", crs = name.as_str()), &mut |s: &mut Value| {
                if let Some(m) = s.as_object_mut() {
                    m.insert("crs".into(), nx.clone());
                }
            });
            let gmode = self.crs_dlg.as_ref().map(|d| d.gmode.clone()).unwrap_or_default();
            let f = next.get("local").and_then(|l| l.get("scale")).cloned().unwrap_or(Value::Null);
            let fs = if f.is_null() { "undefined".to_string() } else { cc_civil::js::to_js_string(&f) };
            let key = if truthy(next.get("local")) {
                if gmode == "ground" { "crs.d.set.ground" } else { "crs.d.set.local" }
            } else {
                "crs.d.set"
            };
            self.message = tr!(key, crs = name.as_str(), f = fs.as_str());
        }
        self.crs_changed(before, Some(next.clone()), convert);
        self.crs_finish(Some(next));
    }
    /// The project's coordinates converted from `cur` into `next` (crs-pick.js reprojectProject; one undo step that also
    /// sets the system). Ok(how many coordinates moved) | Err(why nothing moved).
    fn crs_convert(&mut self, cur: &Value, next: &Value) -> Result<f64, String> {
        let f_old = crs::frame(cur)?.ok_or("no system")?;
        let f_new = crs::frame(next)?.ok_or("no system")?;
        let old_base = without_local(cur);
        f_new.check_from(&old_base)?;
        let map = |x: f64, y: f64| -> Result<[f64; 2], String> {
            let g = f_old.to_grid(x, y);
            f_new.from(&old_base, g[0], g[1])
        };
        let mut out: Result<f64, String> = Ok(0.0);
        let label = tr!("crs.d.undo.convert", crs = string_of(next.get("name")));
        let nx = next.clone();
        self.project.edit_points(&label, &mut |pts, st| {
            if st.get("surface").and_then(|s| s.get("tin")).is_some_and(|v| truthy(Some(v))) {
                out = Err(t0("crs.reproject.tin"));
                return None;
            }
            let mut moved: f64 = 0.0;
            let mut np = Vec::with_capacity(pts.len());
            for p in pts {
                match map(p.x, p.y) {
                    Ok([x, y]) if x.is_finite() && y.is_finite() => {
                        moved += 1.0;
                        np.push(cc_civil::points::Pt { x, y, ..p });
                    }
                    Ok(_) => {
                        out = Err(tr!("crs.reproject.point", n = p.n.as_str()));
                        return None;
                    }
                    Err(e) => {
                        out = Err(e);
                        return None;
                    }
                }
            }
            // on a copy: 0.5 leaves a half-converted state behind when a coordinate fails part way; here nothing changes
            let mut st2 = st.clone();
            let mut m2 = |x: f64, y: f64| map(x, y);
            match crs_pick::reproject_project(&mut st2, &[], &mut m2) {
                Ok(r) => moved += r.get("moved").and_then(Value::as_f64).unwrap_or(0.0),
                Err(e) => {
                    out = Err(e);
                    return None;
                }
            }
            *st = st2;
            if let Some(m) = st.as_object_mut() {
                m.insert("crs".into(), nx.clone());
            }
            out = Ok(moved);
            Some(np)
        });
        out
    }
    fn crs_remove(&mut self) {
        let before = self.crs_dlg.as_ref().and_then(|d| d.current.clone());
        self.project.change(&t0("crs.d.undo.remove"), &mut |s: &mut Value| {
            if let Some(m) = s.as_object_mut() {
                m.insert("crs".into(), Value::Null);
            }
        });
        self.crs_changed(before, None, false);
        self.message = t0("crs.d.removed");
        self.crs_finish(None);
    }

    /// `reg.crsChange` (gis-vector-ui.js): the GIS layers follow a changed system — at once when the coordinates were
    /// converted, else after asking.
    pub fn crs_changed(&mut self, before: Option<Value>, after: Option<Value>, converted: bool) {
        let (Some(before), Some(after)) = (before, after) else { return };
        if crate::civilgis::layers_of(&self.project.state).is_empty() {
            return;
        }
        let ser = |v: Option<&Value>| cc_civil::project::js_stringify(v.unwrap_or(&Value::Null));
        if before.get("proj4") == after.get("proj4") && ser(before.get("local")) == ser(after.get("local")) {
            return;
        }
        if converted {
            self.move_layers(&before, &after);
            self.message = tr!("vec.move.converted", crs = string_of(after.get("name")));
            return;
        }
        let named = |c: &Value| {
            if truthy(c.get("local")) { tr!("vec.crs.ground", crs = string_of(c.get("name"))) } else { string_of(c.get("name")) }
        };
        self.gis3.ask = Some(Ask {
            title: t0("vec.move.title"),
            text: tr!("vec.move.text", before = named(&before), after = named(&after)),
            buttons: vec![("cancel", t0("vec.move.leave")), ("move", t0("vec.move.go"))],
            kind: AskKind::MoveLayers(before, after),
        });
    }
    /// gis-layers.js moveLayers: every vertex from where it belongs under `before` to where it belongs under `after`.
    pub fn move_layers(&mut self, before: &Value, after: &Value) -> bool {
        let (Ok(Some(fb)), Ok(Some(fa))) = (crs::frame(before), crs::frame(after)) else { return false };
        if fb.check_to(after).is_err() {
            return false;
        }
        let label = tr!("layers.undo.moveto", crs = string_of(after.get("name")));
        let f = |x: f64, y: f64| -> (f64, f64) {
            match fb.to(after, x, y) {
                Ok(g) => {
                    let g = crs::nn(g);
                    let p = fa.from_grid(g[0], g[1]);
                    (p[0], p[1])
                }
                Err(_) => (f64::NAN, f64::NAN),
            }
        };
        self.project.edit_gis(&label, &mut |mut map, _s| {
            for (_, fs) in map.iter_mut() {
                let moved: Vec<cc_civil::gis_state::Feature> = fs
                    .iter()
                    .map(|ft| cc_civil::gis_state::Feature {
                        g: ft.g.as_ref().map(|g| cc_io_civil::gis_geom::map_xy(g, &mut |x, y| f(x, y))),
                        p: ft.p.clone(),
                    })
                    .collect();
                *fs = std::sync::Arc::new(moved);
            }
            Some(map)
        })
    }

    /// The answer of a pick dialog goes back to who asked.
    fn crs_answer(&mut self, f: PickFor, v: Option<Value>) {
        match f {
            PickFor::GisProject => self.gis_import_target(v),
            PickFor::GisUnknown(si) => self.gis_import_unknown(si, v),
            PickFor::PointsLatLon => self.import_latlon_answer(v),
            PickFor::TerrainFile | PickFor::TerrainProject => self.terrain_crs_answer(f, v),
            PickFor::MapPicture => self.maps_crs_answer(v),
        }
    }

    /// `civil.ask=<button>`.
    pub fn ask_answer(&mut self, b: &str) -> Result<(), String> {
        let a = self.gis3.ask.take().ok_or("civil.ask: no question is open")?;
        if !a.buttons.iter().any(|(id, _)| *id == b) {
            let list: Vec<&str> = a.buttons.iter().map(|x| x.0).collect();
            self.gis3.ask = Some(a);
            return Err(format!("civil.ask: {} (not {b})", list.join(", ")));
        }
        match a.kind {
            AskKind::CrsConvert(next) => match b {
                "cancel" => {}
                other => self.crs_apply_next(next, other == "convert"),
            },
            AskKind::MoveLayers(before, after) => {
                if b == "move" {
                    self.move_layers(&before, &after);
                    self.message = tr!("vec.move.done", crs = string_of(after.get("name")));
                }
            }
            AskKind::Other(what) => self.ask_other(&what, b),
        }
        Ok(())
    }

    /// `civil.crs.dlg=<action>` (see the command's doc). Ok(Some(need)) when the host has to pick a file or fetch.
    pub fn crs_cmd(&mut self, spec: &str) -> Result<Option<Need>, String> {
        if self.crs_dlg.is_none() {
            return Err("civil.crs.dlg: the Coordinates window is not open (civil.crs.open)".into());
        }
        if self.gis3.ask.is_some() {
            return Err("civil.crs.dlg: answer the question first (civil.ask)".into());
        }
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        let pick = self.crs_dlg.as_ref().is_some_and(|d| d.pick);
        let set = |cv: &mut CivilView, f: &mut dyn FnMut(&mut CrsDlg)| {
            if let Some(d) = cv.crs_dlg.as_mut() {
                f(d);
            }
        };
        match k {
            "search" => {
                set(self, &mut |d| d.q = cc_civil::js::slice16(v, 200));
                self.crs_search();
            }
            "fits" => {
                if self.crs_dlg.as_ref().is_some_and(|d| d.has_data) {
                    set(self, &mut |d| d.q.clear());
                    self.crs_fits();
                }
            }
            "near" => {
                set(self, &mut |d| d.near = cc_civil::js::slice16(v, 200));
                self.crs_near();
            }
            "choose" => {
                let code = str_to_number(v);
                let e = crs::by_code(code).ok().flatten().ok_or_else(|| format!("civil.crs.dlg: no EPSG:{v}"))?;
                self.crs_choose(Some(Chosen { crs: crs::crs_from_entry(e), code: Some(e.code), notes: Vec::new() }));
            }
            "prj" => return Ok(Some(Need::PickPrj)),
            "close" | "cancel" => self.crs_finish(None),
            "apply" => self.crs_apply()?,
            "remove" => {
                if pick || self.crs_dlg.as_ref().is_some_and(|d| d.current.is_none()) {
                    return Err("civil.crs.dlg: there is no system to remove".into());
                }
                self.crs_remove();
            }
            _ if pick => return Err(format!("civil.crs.dlg: {k} is not in the pick dialog")),
            "mode" => {
                if !["grid", "ground", "local"].contains(&v) {
                    return Err("civil.crs.dlg: mode=grid|ground|local".into());
                }
                set(self, &mut |d| d.gmode = v.to_string());
                if v == "local" {
                    self.crs_render_local();
                }
            }
            "csf" | "be" | "bn" | "ae" | "an" | "az" => {
                let val = cc_civil::js::slice16(v, 60);
                set(self, &mut |d| match k {
                    "csf" => d.csf = val.clone(),
                    "be" => d.be = val.clone(),
                    "bn" => d.bn = val.clone(),
                    "ae" => d.ae = val.clone(),
                    "an" => d.an = val.clone(),
                    _ => d.az = val.clone(),
                });
            }
            "compute" => self.crs_compute(),
            "usef" => {
                let f = self.crs_dlg.as_ref().and_then(|d| d.usef.clone()).ok_or("civil.crs.dlg: compute the factor first")?;
                set(self, &mut |d| d.csf = f.clone());
            }
            "cp" => {
                // cp=<i>:<n|ge|gn|ll|xy>=<value>
                let (i, rest) = v.split_once(':').ok_or("civil.crs.dlg: cp=<0|1|2>:<n|ge|gn|ll|xy>=<value>")?;
                let i: usize = i.parse().ok().filter(|i| *i < 3).ok_or("civil.crs.dlg: control point 0, 1 or 2")?;
                let (f, val) = rest.split_once('=').unwrap_or((rest, ""));
                let val = cc_civil::js::slice16(val, 60);
                let xy = if f == "xy" {
                    let p = crate::civilview::xy_param(&val).filter(|p| p.len() >= 2).ok_or("civil.crs.dlg: cp=i:xy=x,y")?;
                    Some([p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0)])
                } else {
                    None
                };
                let mut bad = false;
                set(self, &mut |d| {
                    let Some(c) = d.cps.get_mut(i) else { return };
                    match f {
                        "n" => c.n = val.clone(),
                        "ge" => c.ge = val.clone(),
                        "gn" => c.gn = val.clone(),
                        "ll" => c.ll = val.clone(),
                        "xy" => {
                            c.n.clear();
                            c.xy = xy;
                        }
                        _ => bad = true,
                    }
                });
                if bad {
                    return Err("civil.crs.dlg: cp=i:n|ge|gn|ll|xy=…".into());
                }
                if self.crs_dlg.as_ref().is_some_and(|d| d.gmode == "local") {
                    self.crs_render_local();
                }
            }
            "pick" => {
                if !["factor", "cp0", "cp1", "cp2"].contains(&v) {
                    return Err("civil.crs.dlg: pick=factor|cp0|cp1|cp2".into());
                }
                set(self, &mut |d| d.picking = Some(v.to_string()));
                self.message = if v == "factor" {
                    t0("crs.d.click.factor")
                } else {
                    tr!("crs.d.click.cp", n = v.trim_start_matches("cp").parse::<usize>().unwrap_or(0) + 1)
                };
            }
            "grid" => {
                if self.crs_grid_lines().is_none() {
                    return Err("civil.crs.dlg: the grid section is not shown".into());
                }
                match v {
                    "file" => return Ok(Some(Need::PickGrid)),
                    "off" => {
                        if self.crs_dlg.as_ref().and_then(|d| d.current.as_ref()).is_none_or(|c| !truthy(c.get("grid"))) {
                            return Err("civil.crs.dlg: no grid is used".into());
                        }
                        self.crs_grid_off();
                    }
                    other => {
                        let name = other.strip_prefix("cdn:").ok_or("civil.crs.dlg: grid=file|off|cdn:<name>")?;
                        let ok = self.crs_grid_lines().is_some_and(|l| l.iter().any(|(k, _)| k == &format!("cdn:{name}")));
                        if !ok {
                            return Err(format!("civil.crs.dlg: {name} is not offered"));
                        }
                        if !self.pt_settings.internet {
                            self.crs_grid_msg(t0("mapl.net_first"), true);
                            return Ok(None);
                        }
                        self.gis3.grid_fetch = Some(name.to_string());
                        self.crs_grid_msg(tr!("grid.fetching", name = name), false);
                        return Ok(Some(Need::Fetch(format!("{}{name}", crs::GRID_CDN))));
                    }
                }
            }
            other => return Err(format!("civil.crs.dlg: unknown action {other}")),
        }
        Ok(None)
    }

    /// `civil.crs.info`: what the dialog shows, line by line.
    pub fn crs_info(&mut self) -> Result<String, String> {
        if self.crs_dlg.is_none() {
            return Err("civil.crs.info: the Coordinates window is not open".into());
        }
        if self.crs_dlg.as_ref().is_some_and(|d| d.gmode == "local" && !d.pick) {
            self.crs_render_local();
        }
        let units = self.units_now();
        let d = self.crs_dlg.as_ref().ok_or("closed")?;
        let mut out: Vec<String> = Vec::new();
        if !d.reason.is_empty() {
            out.push(d.reason.clone());
        }
        if !d.pick {
            match &d.current {
                None => out.push(tr!("crs.d.none", none = t0("crs.d.none.word"))),
                Some(c) => out.push(format!("{}: {}", t0("crs.d.now"), lines_text(&facts(&crs_pick::describe(c))))),
            }
        }
        if let Some(g) = self.crs_grid_lines() {
            out.push(lines_text(&g.into_iter().map(|x| x.1).collect::<Vec<_>>()));
        }
        out.push(d.head.clone());
        let chosen_code = d.chosen.as_ref().and_then(|c| c.code);
        for (i, code) in d.list.iter().enumerate() {
            out.push(list_item(*code, &units, d.for_project, d.extra.get(i).map_or("", String::as_str), chosen_code == Some(*code)));
        }
        out.push(format!("{}: {}", t0("crs.d.chosen"), lines_text(&self.crs_chosen_lines())));
        if let Some(p) = &d.problem {
            out.push(p.clone());
        }
        if d.pick {
            out.push(match self.crs_apply_off() {
                None => format!("{}: on", t0("vec.ask.go")),
                Some(w) => format!("{}: off ({w})", t0("vec.ask.go")),
            });
        } else {
            let d = self.crs_dlg.as_ref().ok_or("closed")?;
            out.push(format!("{}: {}", t0("crs.d.ggl"), d.gmode));
            match d.gmode.as_str() {
                "ground" => {
                    out.push(format!("csf={} base={},{} at={},{},{}", d.csf, d.be, d.bn, d.ae, d.an, d.az));
                    out.push(lines_text(&d.factors));
                }
                "local" => {
                    for c in &d.cps {
                        out.push(format!("{} · {} · {},{} · {} [{}, {}]", c.n, c.at, c.ge, c.gn, c.ll, c.ge_ph, c.gn_ph));
                    }
                    out.push(lines_text(&d.local));
                }
                _ => {}
            }
        }
        Ok(out.join(" | "))
    }
}

/// facts(d): the system's rows.
fn facts(d: &Value) -> Vec<Line> {
    let code = d.get("code").filter(|v| !v.is_null()).map(cc_civil::js::to_js_string).unwrap_or_else(|| t0("crs.d.f.code.none"));
    vec![
        Line::Row(t0("vec.ask.system"), string_of(d.get("name"))),
        Line::Row(t0("crs.d.f.code"), code),
        Line::Row(t0("crs.d.f.unit"), string_of(d.get("unit"))),
        Line::Row(t0("crs.d.f.shift"), string_of(d.get("shift"))),
        Line::Row(t0("crs.d.f.area"), string_of(d.get("area"))),
        Line::Row(t0("crs.d.f.coords"), string_of(d.get("local"))),
    ]
}
fn lines_text(l: &[Line]) -> String {
    l.iter().map(Line::text).collect::<Vec<_>>().join("; ")
}
/// A list row: name, then "EPSG:<code> · <unit words>[ mismatch][extra]".
fn list_item(code: f64, units: &str, for_project: bool, extra: &str, sel: bool) -> String {
    let Some(e) = crs::by_code(code).ok().flatten() else { return format!("EPSG:{}", num_to_string(code)) };
    let fam = if e.unit == "deg" { t0("crs.unit.deg") } else { crs_pick::unit_words(Some(&Value::String(e.unit.clone()))) };
    let mismatch = for_project && e.unit != "deg" && (e.unit == "m") != (units == "m");
    let mm = if mismatch { format!(" {}", t0(if units == "m" { "crs.d.mismatch.m" } else { "crs.d.mismatch.ft" })) } else { String::new() };
    format!("{}{} · EPSG:{} · {fam}{mm}{extra}", if sel { "* " } else { "" }, e.name, num_to_string(e.code))
}

/// What a dialog needs from the host: the platform's picker (desktop: now; web: later, see [`poll_host`]) or the
/// network (the web build's fetch; the desktop has no HTTP client, so a download is said to be unavailable).
pub fn host_need(app: &mut crate::UiApp, need: Need) {
    match need {
        Need::PickPrj | Need::PickGrid | Need::PickGis => {
            let kind = match need {
                Need::PickPrj => "prj",
                Need::PickGis => "gis",
                _ => "grid",
            };
            match app.services.pick_files.as_mut().map(|f| f(kind, kind == "gis")) {
                Some(Some(files)) => deliver(app, &need, files),
                Some(None) => {
                    if let Some(cv) = app.civil.as_mut() {
                        cv.gis3.pick_wait = Some(need);
                    }
                }
                None => {}
            }
        }
        Need::Fetch(url) => match app.services.fetch.as_mut() {
            Some(f) => f(&url),
            None => {
                if let Some(cv) = app.civil.as_mut() {
                    let name = cv.gis3.grid_fetch.clone().unwrap_or_default();
                    cv.crs_grid_fetched(&name, Err("downloading is not available in this version of the app".into()));
                }
            }
        },
    }
}

fn deliver(app: &mut crate::UiApp, need: &Need, files: Vec<crate::ImportedFile>) {
    if *need == Need::PickGis {
        for f in &files {
            app.open_bytes(&f.name, &f.bytes);
        }
        return;
    }
    let Some(cv) = app.civil.as_mut() else { return };
    let Some(f) = files.into_iter().next() else { return };
    match need {
        Need::PickPrj => cv.crs_prj(&f.name, &f.bytes),
        Need::PickGrid => cv.crs_use_grid(&f.name, f.bytes),
        Need::PickGis | Need::Fetch(_) => {}
    }
}

/// Every frame: a web picker's files, a downloaded grid.
pub fn poll_host(app: &mut crate::UiApp) {
    let waiting = app.civil.as_ref().and_then(|c| c.gis3.pick_wait.clone());
    if let Some(need) = waiting
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        if let Some(cv) = app.civil.as_mut() {
            cv.gis3.pick_wait = None;
        }
        deliver(app, &need, files);
    }
}

/// A fetched URL that is a grid shift file the Coordinates window asked for (true: taken).
pub fn take_fetched(cv: &mut CivilView, url: &str, r: &Result<(String, Vec<u8>), String>) -> bool {
    let Some(name) = cv.gis3.grid_fetch.clone() else { return false };
    if url != format!("{}{name}", crs::GRID_CDN) {
        return false;
    }
    cv.crs_grid_fetched(&name, r.clone().map(|(_, b)| b));
    true
}

// ------------------------------------------------------------------------------------------------------- windows

fn show_lines(ui: &mut egui::Ui, tk: &Tokens, l: &[Line]) {
    let problem = egui::Color32::from_rgb(0xd0, 0x4a, 0x5a);
    let rows: Vec<&Line> = l.iter().filter(|x| matches!(x, Line::Row(..))).collect();
    if !rows.is_empty() {
        egui::Grid::new(ui.next_auto_id()).num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
            for r in rows {
                if let Line::Row(k, v) = r {
                    ui.label(RichText::new(k).small().color(tk.text_dim));
                    ui.add(egui::Label::new(RichText::new(v).small().color(tk.text)).wrap());
                    ui.end_row();
                }
            }
        });
    }
    for x in l {
        match x {
            Line::Note(s) => {
                ui.add(egui::Label::new(RichText::new(s).small().color(tk.text_dim)).wrap());
            }
            Line::Problem(s) => {
                ui.add(egui::Label::new(RichText::new(s).small().color(problem)).wrap());
            }
            Line::Row(..) => {}
        }
    }
}

fn field(ui: &mut egui::Ui, label: &str, v: &str, hint: &str, w: f32, acts: &mut Vec<(&'static str, String)>, key: &str) {
    ui.horizontal(|ui| {
        let l = ui.label(label);
        let mut s = v.to_string();
        let r = ui.add(crate::shell::field(&mut s).hint_text(hint).desired_width(w)).labelled_by(l.id);
        if r.changed() {
            acts.push(("civil.crs.dlg", format!("{key}={s}")));
        }
    });
}

/// The Coordinates window (and the pick dialog).
pub fn crs_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    if cv.crs_dlg.as_ref().is_none_or(|d| d.picking.is_some()) {
        return;
    }
    if cv.crs_dlg.as_ref().is_some_and(|d| d.gmode == "local") {
        cv.crs_render_local();
    }
    let Some(d) = cv.crs_dlg.clone() else { return };
    let units = cv.project.units().to_string();
    let grid = cv.crs_grid_lines();
    let chosen = cv.crs_chosen_lines();
    let apply_off = cv.crs_apply_off();
    let mut open = true;
    let title = if d.pick { t0("crs.d.title.pick") } else { t0("vec.import.crs") };
    let problem = egui::Color32::from_rgb(0xd0, 0x4a, 0x5a);
    egui::Window::new(title)
        .id(egui::Id::unique("cv_crs"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(ctx.content_rect().height() * 0.8)
                .min_scrolled_height(ctx.content_rect().height() * 0.6)
                .show(ui, |ui| {
                    if !d.reason.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&d.reason).color(tk.text)).wrap());
                    }
                    if !d.pick {
                        match &d.current {
                            None => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(tr!("crs.d.none", none = t0("crs.d.none.word"))).small().color(tk.text_dim),
                                    )
                                    .wrap(),
                                );
                            }
                            Some(c) => {
                                ui.label(RichText::new(t0("crs.d.now")).strong().color(tk.text_strong));
                                show_lines(ui, tk, &facts(&crs_pick::describe(c)));
                            }
                        }
                    }
                    if let Some(g) = &grid {
                        ui.separator();
                        for (k, l) in g {
                            match k.as_str() {
                                "head" => {
                                    ui.label(RichText::new(l.text()).strong().color(tk.text_strong));
                                }
                                "file" | "off" => {
                                    if ui.add(crate::shell::Btn::new(l.text())).clicked() {
                                        acts.push(("civil.crs.dlg", format!("grid={k}")));
                                    }
                                }
                                k if k.starts_with("cdn:") => {
                                    let on = cv.pt_settings.internet;
                                    let b = ui.add_enabled(on, crate::shell::Btn::new(l.text()));
                                    let b = if on { b } else { b.on_disabled_hover_text(t0("mapl.net_first")) };
                                    if b.clicked() {
                                        acts.push(("civil.crs.dlg", format!("grid={k}")));
                                    }
                                }
                                _ => show_lines(ui, tk, std::slice::from_ref(l)),
                            }
                        }
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(t0(if d.pick || d.current.is_none() { "crs.d.pick.head" } else { "crs.d.change.head" }))
                            .strong()
                            .color(tk.text_strong),
                    );
                    ui.horizontal(|ui| {
                        let l = ui.label(t0("crs.d.search"));
                        let mut q = d.q.clone();
                        let r = ui.add(crate::shell::field(&mut q).hint_text(t0("crs.d.search.ph")).desired_width(300.0)).labelled_by(l.id);
                        if r.changed() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            acts.push(("civil.crs.dlg", format!("search={q}")));
                        }
                    });
                    ui.horizontal(|ui| {
                        let l = ui.label(t0("crs.d.near"));
                        let mut q = d.near.clone();
                        let r = ui.add(crate::shell::field(&mut q).hint_text("30.27, -97.74").desired_width(200.0)).labelled_by(l.id);
                        if r.changed()
                            && let Some(x) = cv.crs_dlg.as_mut()
                        {
                            x.near = q.clone();
                        }
                        if ui.add(crate::shell::Btn::new(t0("crs.d.suggest"))).clicked()
                            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            acts.push(("civil.crs.dlg", format!("near={q}")));
                        }
                    });
                    ui.horizontal(|ui| {
                        if d.has_data && ui.link(t0("crs.d.fits")).clicked() {
                            acts.push(("civil.crs.dlg", "fits".into()));
                        }
                        if ui.link(t0("crs.d.prj")).clicked() {
                            acts.push(("civil.crs.dlg", "prj".into()));
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(&d.head).small().color(tk.text_dim)).wrap());
                    let chosen_code = d.chosen.as_ref().and_then(|c| c.code);
                    egui::ScrollArea::vertical().id_salt("cv_crs_list").max_height(200.0).show(ui, |ui| {
                        for (i, code) in d.list.iter().enumerate() {
                            let Some(e) = crs::by_code(*code).ok().flatten() else { continue };
                            let meta = list_item(*code, &units, d.for_project, d.extra.get(i).map_or("", String::as_str), false);
                            let meta = meta.split_once(" · ").map_or(meta.as_str(), |(_, m)| m).to_string();
                            let sel = chosen_code == Some(*code);
                            let r = ui.selectable_label(sel, RichText::new(&e.name).color(tk.text));
                            ui.label(RichText::new(meta).small().color(tk.text_dim));
                            if r.clicked() {
                                acts.push(("civil.crs.dlg", format!("choose={}", num_to_string(*code))));
                            }
                        }
                    });
                    ui.separator();
                    ui.label(RichText::new(t0("crs.d.chosen")).strong().color(tk.text_strong));
                    show_lines(ui, tk, &chosen);
                    if let Some(p) = &d.problem {
                        ui.add(egui::Label::new(RichText::new(p).small().color(problem)).wrap());
                    }
                    if !d.pick {
                        ui.separator();
                        ui.label(RichText::new(t0("crs.d.ggl")).strong().color(tk.text_strong));
                        ui.horizontal(|ui| {
                            for (m, k) in [("grid", "crs.d.mode.grid"), ("ground", "crs.d.mode.ground"), ("local", "crs.d.mode.local")] {
                                if ui.radio(d.gmode == m, t0(k)).clicked() {
                                    acts.push(("civil.crs.dlg", format!("mode={m}")));
                                }
                            }
                        });
                        match d.gmode.as_str() {
                            "grid" => {
                                ui.add(egui::Label::new(RichText::new(t0("crs.d.grid.note")).small().color(tk.text_dim)).wrap());
                            }
                            "ground" => {
                                ui.add(egui::Label::new(RichText::new(t0("crs.d.ground.note")).small().color(tk.text_dim)).wrap());
                                field(ui, &t0("crs.d.csf"), &d.csf, "0.99987", 120.0, acts, "csf");
                                field(ui, &t0("crs.d.base.e"), &d.be, "", 120.0, acts, "be");
                                field(ui, &t0("crs.d.base.n"), &d.bn, "", 120.0, acts, "bn");
                                ui.add(egui::Label::new(RichText::new(t0("crs.d.base.note")).small().color(tk.text_dim)).wrap());
                                ui.label(RichText::new(t0("crs.d.factor.head")).strong().color(tk.text_strong));
                                field(ui, &t0("crs.d.e"), &d.ae, "", 120.0, acts, "ae");
                                field(ui, &t0("crs.d.n"), &d.an, "", 120.0, acts, "an");
                                field(ui, &tr!("crs.d.elev", unit = if units == "m" { "m" } else { "ft" }), &d.az, "", 120.0, acts, "az");
                                ui.horizontal(|ui| {
                                    if ui.add(crate::shell::Btn::new(format!("⌖ {}", t0("crs.d.pick.point")))).clicked() {
                                        acts.push(("civil.crs.dlg", "pick=factor".into()));
                                    }
                                    if ui.add(crate::shell::Btn::new(t0("crs.d.compute"))).clicked() {
                                        acts.push(("civil.crs.dlg", "compute".into()));
                                    }
                                });
                                show_lines(ui, tk, &d.factors);
                                if let Some(f) = &d.usef
                                    && ui.add(crate::shell::Btn::new(tr!("crs.d.usef", f = f.as_str()))).clicked()
                                {
                                    acts.push(("civil.crs.dlg", "usef".into()));
                                }
                            }
                            _ => {
                                ui.add(egui::Label::new(RichText::new(t0("crs.d.local.note")).small().color(tk.text_dim)).wrap());
                                for (i, key) in ["crs.d.cp1", "crs.d.cp2", "crs.d.cp3"].iter().enumerate() {
                                    let Some(c) = d.cps.get(i) else { continue };
                                    let title = t0(key);
                                    ui.label(RichText::new(&title).small().strong().color(tk.text_strong));
                                    ui.horizontal(|ui| {
                                        for (f, val, hint, w) in [
                                            ("n", &c.n, t0("crs.d.cp.no"), 80.0),
                                            ("ge", &c.ge, if c.ge_ph.is_empty() { t0("crs.d.cp.ge.ph") } else { c.ge_ph.clone() }, 110.0),
                                            ("gn", &c.gn, if c.gn_ph.is_empty() { t0("crs.d.cp.gn.ph") } else { c.gn_ph.clone() }, 110.0),
                                            ("ll", &c.ll, t0("crs.d.cp.ll.ph"), 130.0),
                                        ] {
                                            let mut s = val.clone();
                                            let r = ui.add(crate::shell::field(&mut s).hint_text(hint).desired_width(w));
                                            if r.changed() {
                                                acts.push(("civil.crs.dlg", format!("cp={i}:{f}={s}")));
                                            }
                                            if f == "n" && ui.add(crate::shell::Btn::new(format!("⌖ {}", t0("crs.d.pick")))).clicked() {
                                                acts.push(("civil.crs.dlg", format!("pick=cp{i}")));
                                            }
                                        }
                                    });
                                    if !c.at.is_empty() {
                                        ui.label(RichText::new(&c.at).small().color(tk.text_dim));
                                    }
                                }
                                show_lines(ui, tk, &d.local);
                            }
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if !d.pick && d.current.is_some() && ui.add(crate::shell::Btn::new(t0("crs.d.remove"))).clicked() {
                            acts.push(("civil.crs.dlg", "remove".into()));
                        }
                        if ui.add(crate::shell::Btn::new(t0("vec.cancel"))).clicked() {
                            acts.push(("civil.crs.dlg", "close".into()));
                        }
                        let label = if d.pick { t0("vec.ask.go") } else { t0("crs.d.apply") };
                        let b = ui.add_enabled(apply_off.is_none(), crate::shell::Btn::new(label));
                        let b = match &apply_off {
                            Some(w) => b.on_disabled_hover_text(w),
                            None => b,
                        };
                        if b.clicked() {
                            acts.push(("civil.crs.dlg", "apply".into()));
                        }
                    });
                });
        });
    if !open {
        acts.push(("civil.crs.dlg", "close".into()));
    }
}

/// A question (0.5's host.ask) as a small window.
pub fn ask_window(cv: &CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(a) = &cv.gis3.ask else { return };
    egui::Window::new(&a.title)
        .id(egui::Id::unique("cv_ask"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.add(egui::Label::new(RichText::new(&a.text).color(tk.text)).wrap());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (id, label) in &a.buttons {
                    if ui.add(crate::shell::Btn::new(label)).clicked() {
                        acts.push(("civil.ask", (*id).to_string()));
                    }
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && a.buttons.iter().any(|b| b.0 == "cancel") {
                acts.push(("civil.ask", "cancel".into()));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_is_the_pages_number() {
        assert_eq!(fx(1234567.891, 3), "1,234,567.891");
        assert_eq!(fx(2.5, 3), "2.5");
        assert_eq!(fx(1999.9996, 3), "2,000");
        assert_eq!(fx(f64::NAN, 3), "—");
        assert_eq!(fx(-0.0001, 3), "-0");
    }

    #[test]
    fn ground_mode_applies_and_converting_moves_the_points() {
        let mut cv = CivilView::sample();
        cv.crs_set("2277").unwrap();
        cv.crs_open(None);
        cv.crs_cmd("mode=ground").unwrap();
        cv.crs_cmd("csf=1.2").unwrap();
        cv.crs_cmd("apply").unwrap();
        assert!(cv.crs_dlg.as_ref().unwrap().problem.is_some(), "a factor out of range is refused");
        cv.crs_cmd("csf=0.99987").unwrap();
        cv.crs_cmd("apply").unwrap();
        assert!(cv.crs_dlg.is_none());
        let l = &cv.project.state["crs"]["local"];
        assert_eq!(l["scale"], json!(0.99987));
        // a different base system: asked, then converted (one undo step)
        let x0 = cv.project.points[0].x;
        cv.crs_open(None);
        cv.crs_cmd("choose=2278").unwrap();
        cv.crs_cmd("apply").unwrap();
        assert!(matches!(cv.gis3.ask.as_ref().map(|a| &a.kind), Some(AskKind::CrsConvert(_))));
        cv.ask_answer("convert").unwrap();
        assert!(cv.crs_dlg.is_none());
        assert!((cv.project.points[0].x - x0).abs() > 1000.0, "the points moved");
        assert_eq!(cv.project.state["crs"]["code"].as_f64(), Some(2278.0));
        cv.undo();
        assert_eq!(cv.project.points[0].x, x0);
        assert_eq!(cv.project.state["crs"]["code"].as_f64(), Some(2277.0));
    }

    #[test]
    fn local_mode_fits_two_control_points() {
        let mut cv = CivilView::sample();
        cv.crs_set("2277").unwrap();
        cv.crs_open(None);
        cv.crs_cmd("mode=local").unwrap();
        let (a, b) = (cv.project.points[0].clone(), cv.project.points[5].clone());
        cv.crs_cmd(&format!("cp=0:n={}", a.n)).unwrap();
        cv.crs_cmd("cp=0:ge=5000").unwrap();
        cv.crs_cmd("cp=0:gn=5000").unwrap();
        cv.crs_cmd(&format!("cp=1:n={}", b.n)).unwrap();
        cv.crs_cmd(&format!("cp=1:ge={}", 5000.0 + (b.x - a.x))).unwrap();
        cv.crs_cmd(&format!("cp=1:gn={}", 5000.0 + (b.y - a.y))).unwrap();
        let fit = cv.crs_dlg.as_ref().unwrap().local_fit.unwrap();
        assert!((fit.scale - 1.0).abs() < 1e-9 && fit.rotation.abs() < 1e-9);
        let info = cv.crs_info().unwrap();
        assert!(info.contains("Scale"), "{info}");
        cv.crs_cmd("apply").unwrap();
        assert!(cv.project.state["crs"]["local"].is_object());
    }

    #[test]
    fn a_prj_file_is_chosen_and_a_lone_prj_opens_the_dialog() {
        let mut cv = CivilView::sample();
        cv.crs_open(None);
        cv.crs_prj("site.prj", b"PROJCS[\"NAD_1983_StatePlane_Texas_Central_FIPS_4203_Feet\",GEOGCS[\"GCS_North_American_1983\",DATUM[\"D_North_American_1983\",SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Lambert_Conformal_Conic\"],PARAMETER[\"False_Easting\",2296583.333333333],PARAMETER[\"False_Northing\",9842500.0],PARAMETER[\"Central_Meridian\",-100.3333333333333],PARAMETER[\"Standard_Parallel_1\",30.11666666666667],PARAMETER[\"Standard_Parallel_2\",31.88333333333333],PARAMETER[\"Latitude_Of_Origin\",29.66666666666667],UNIT[\"Foot_US\",0.3048006096012192]]");
        let d = cv.crs_dlg.as_ref().unwrap();
        assert!(d.chosen.is_some(), "{}", d.head);
        cv.crs_prj("big.prj", &vec![b' '; MAX_PRJ + 1]);
        assert!(cv.crs_dlg.as_ref().unwrap().head.contains("big.prj"));
    }
}
