//! CivilCraft's GIS vector layers (gis-vector-ui.js, gis-layers.js) and the project's coordinate system
//! (gis-crs-ui.js's set / remove, the latitude / longitude readout): the layers drawn under the design linework on the
//! plan and in DXF / DWG / PDF (`gisScene`), the left panel's GIS layers section (show, colour, zoom to, rename, label
//! with a field, remove), a picked feature's properties, and GIS files (GeoJSON, shapefile sets and zips, KML / KMZ,
//! GeoPackage) brought in through the import dialog — which layers, what system the project takes (the project's, the
//! file's offered, one picked), unknown systems taken as the project's — as one undo step. The numbers and readers are
//! cc-io-civil's (gis_read.rs, gis_layers.rs, crs.rs). Every action is a `civil.gis.*` / `civil.crs` command.

use cc_civil::gis_state::{Feature, Field, Geom, LAYER_TOKENS, Layer, Source, clean_str};
use cc_civil::i18n::t;
use cc_civil::js::{J, arr, string_of};
use cc_civil::tr;
use cc_io_civil::gis_read::{self, Src};
use egui::RichText;
use serde_json::{Value, json};

use crate::civilpanel::{Act, head, link, note, report_pub};
use crate::civilview::{CivilView, Sel, t0};
use crate::theme::Tokens;

/// Most bytes of GIS files read at once (0.5: 200 MB a file).
pub const MAX_GIS_BYTES: usize = 200 * 1024 * 1024;

/// The project's GIS layers (state.gis.layers), as saved (already checked when the project was read).
pub fn layers_of(state: &Value) -> Vec<Layer> {
    arr(state.get("gis").and_then(|g| g.get("layers")))
        .iter()
        .map(|l| {
            let s = l.get("source");
            Layer {
                id: string_of(l.get("id")),
                name: string_of(l.get("name")),
                geom: string_of(l.get("geom")),
                has_z: l.get("hasZ") == Some(&Value::Bool(true)),
                fields: arr(l.get("fields"))
                    .iter()
                    .map(|f| Field { name: string_of(f.get("name")), ty: string_of(f.get("type")) })
                    .collect(),
                visible: l.get("visible") != Some(&Value::Bool(false)),
                color: string_of(l.get("color")),
                source: Source {
                    file: s.map(|s| string_of(s.get("file"))).unwrap_or_default(),
                    format: s.map(|s| string_of(s.get("format"))).unwrap_or_default(),
                    crs: s.and_then(|s| s.get("crs")).and_then(Value::as_str).map(str::to_string),
                },
                label: l.get("label").and_then(|x| x.get("field")).and_then(Value::as_str).map(str::to_string),
            }
        })
        .collect()
}

/// The GIS import dialog: the files read, what each layer is, the system plan and the person's choices.
pub struct GisImport {
    pub files: Vec<(String, Vec<u8>)>,
    pub sources: Vec<gis_read::Source>,
    pub notes: Vec<String>,
    pub plan: Value,
    /// "si:li" of the layers to bring in
    pub pick: Vec<(usize, usize)>,
    /// the system the layers go into (None: the project's own coordinates)
    pub target: Option<Value>,
    pub set_crs: bool,
    /// unknown sources: taken as the project's ("same") or in a picked system
    pub overrides: Vec<(usize, Src)>,
}

impl GisImport {
    fn chosen(&self) -> Vec<(usize, usize)> {
        let mut v = self.pick.clone();
        v.sort_unstable();
        v
    }
    fn src_of(&self, si: usize) -> Option<Src> {
        self.overrides.iter().find(|(i, _)| *i == si).map(|(_, s)| s.clone()).or_else(|| self.sources.get(si).map(|s| s.src.clone()))
    }
    /// upd(): a system is needed (layers in a known system, no target yet).
    pub fn needs(&self) -> bool {
        self.target.is_none()
            && self.chosen().iter().any(|(si, _)| {
                matches!(self.src_of(*si), Some(Src::Crs { .. })) || matches!(self.sources.get(*si).map(|s| &s.src), Some(Src::Wgs84))
            })
    }
}

fn kind_word(geom: &str, z: bool, with: bool) -> String {
    let k = match geom {
        "pt" | "ln" | "pg" => geom,
        o => return o.to_string(),
    };
    t(
        &format!(
            "vec.kind.{k}{}",
            if with {
                ".withz"
            } else if z {
                ".z"
            } else {
                ""
            }
        ),
        &[],
    )
}

fn num(n: usize) -> String {
    cc_civil::units::grouped(n as f64, 0)
}

impl CivilView {
    pub fn gis_layers(&self) -> Vec<Layer> {
        layers_of(&self.project.state)
    }

    /// gis-vector-ui.js's scene part: `gisScene(project, mm)` → items (layers GIS-<id>) and each layer's colour token.
    pub fn gis_scene(&self, mm: f64) -> (Vec<Value>, Vec<(String, String)>) {
        let layers = self.gis_layers();
        if layers.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let g = cc_io_civil::gis_layers::gis_scene(&layers, &self.project.gis_features, mm);
        let toks = arr(g.get("layers")).iter().map(|l| (string_of(l.get("name")), string_of(l.get("token")))).collect();
        (arr(g.get("items")).to_vec(), toks)
    }

    /// `gisExtents(project)`: the visible layers' box.
    pub fn gis_extents(&self) -> Option<(f64, f64, f64, f64)> {
        let b = cc_io_civil::gis_layers::gis_extents(&self.gis_layers(), &self.project.gis_features)?;
        b.min_x.is_finite().then_some((b.min_x, b.min_y, b.max_x, b.max_y))
    }

    /// `pickFeature(project, x, y, tol)`.
    pub fn gis_pick(&self, x: f64, y: f64, tol: f64) -> Option<Sel> {
        let (l, i) = cc_io_civil::gis_layers::pick_feature(&self.gis_layers(), &self.project.gis_features, x, y, tol)?;
        Some(Sel::Gis(l, i))
    }

    /// `civil.gis.layer=show|color|rename|label|remove|zoom=<id>[:value]` (gis-layers.js setLayer / removeLayer).
    pub fn gis_layer_cmd(&mut self, spec: &str) -> Result<(), String> {
        let (k, rest) = spec.split_once('=').ok_or("civil.gis.layer: name = \"show|color|rename|label|remove|zoom=<id>[:value]\"")?;
        let (id, v) = rest.split_once(':').map_or((rest, ""), |(a, b)| (a, b));
        let layers = self.gis_layers();
        let l = layers.iter().find(|l| l.id == id).cloned().ok_or_else(|| format!("civil.gis.layer: no layer {id}"))?;
        let id = id.to_string();
        let set = |cv: &mut CivilView, label: &str, f: &dyn Fn(&mut serde_json::Map<String, Value>)| {
            cv.project.change(label, &mut |s: &mut Value| {
                if let Some(x) = s
                    .get_mut("gis")
                    .and_then(|g| g.get_mut("layers"))
                    .and_then(Value::as_array_mut)
                    .and_then(|ls| ls.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id.as_str())))
                    .and_then(Value::as_object_mut)
                {
                    f(x);
                }
            });
        };
        match k {
            "show" => {
                let on = v != "off";
                set(self, &tr!(if on { "layers.undo.show" } else { "layers.undo.hide" }, layer = l.name.as_str()), &|x| {
                    x.insert("visible".into(), Value::Bool(on));
                });
            }
            "color" => {
                if !LAYER_TOKENS.contains(&v) {
                    return Err(format!("civil.gis.layer: colour {} (not {v})", LAYER_TOKENS.join(", ")));
                }
                let c = v.to_string();
                set(self, &t("layers.undo.color", &[]), &|x| {
                    x.insert("color".into(), Value::String(c.clone()));
                });
            }
            "rename" => {
                let n = clean_str(v, 120);
                let name = if n.is_empty() { l.name.clone() } else { n };
                set(self, &t("layers.undo.rename", &[]), &|x| {
                    x.insert("name".into(), Value::String(name.clone()));
                });
            }
            "label" => {
                if !v.is_empty() && !l.fields.iter().any(|f| f.name == v) {
                    return Err(format!("civil.gis.layer: {} has no field {v}", l.name));
                }
                let field = v.to_string();
                set(self, &tr!("gis.label.undo", layer = l.name.as_str()), &|x| {
                    if field.is_empty() {
                        x.remove("label");
                    } else {
                        x.insert("label".into(), json!({ "field": field }));
                    }
                });
                self.message = if v.is_empty() {
                    tr!("gis.label.off", layer = l.name.as_str())
                } else {
                    tr!("gis.label.on", layer = l.name.as_str(), field = v)
                };
            }
            "remove" => {
                self.project.edit_gis(&tr!("layers.undo.remove", layer = l.name.as_str()), &mut |mut map, s| {
                    if let Some(ls) = s.get_mut("gis").and_then(|g| g.get_mut("layers")).and_then(Value::as_array_mut) {
                        ls.retain(|x| x.get("id").and_then(Value::as_str) != Some(id.as_str()));
                    }
                    map.retain(|(k, _)| *k != id);
                    Some(map)
                });
                if matches!(&self.sel, Some(Sel::Gis(lid, _)) if *lid == id) {
                    self.sel = None;
                }
                self.message = tr!("vec.layer.removed", layer = l.name.as_str());
            }
            "zoom" => {
                let fs = cc_io_civil::gis_layers::features_of(&self.project.gis_features, &id);
                if let Some(b) = cc_io_civil::gis_layers::layer_bounds(fs) {
                    self.zoom_to(b.min_x, b.min_y, b.max_x, b.max_y);
                }
            }
            o => return Err(format!("civil.gis.layer: show, color, rename, label, remove or zoom (not {o})")),
        }
        Ok(())
    }

    /// gis-vector-ui.js `zoomTo(b)`: the box with 15 % around it.
    pub fn zoom_to(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return;
        }
        let pad = (x1 - x0).max(y1 - y0).max(10.0) * 0.15;
        self.fit_box(x0 - pad, y0 - pad, x1 + pad, y1 + pad);
    }

    // ------------------------------------------------------------------------------------------- import

    /// A GIS file opened or dropped: read (with the files already waiting, so a shapefile's parts come together) and
    /// the import dialog opened (gis-vector-ui.js importFiles → importDialog). Nothing readable: said.
    pub fn gis_open(&mut self, name: &str, bytes: Vec<u8>) {
        ensure_catalog();
        let mut files = self.gis_import.take().map(|d| d.files).unwrap_or_default();
        files.retain(|(n, _)| n != name);
        files.push((name.to_string(), bytes));
        let t0 = web_time::Instant::now();
        let (sources, notes) = gis_read::read_gis_files(&files, cc_io_civil::gis_layers::MAX_LAYER_FEATURES);
        self.timings.insert("gis read", t0.elapsed().as_secs_f64() * 1000.0);
        if sources.is_empty() {
            let names: Vec<String> = files.iter().map(|f| f.0.clone()).collect();
            self.message = if notes.is_empty() { tr!("vec.import.nothing", files = names.join(", ")) } else { notes.join(" ") };
            // a shapefile's .shp may come after its .dbf: keep the parts that cannot be read alone a moment
            if files.iter().any(|(n, _)| {
                let l = n.to_ascii_lowercase();
                l.ends_with(".dbf") || l.ends_with(".shx") || l.ends_with(".prj") || l.ends_with(".cpg")
            }) {
                self.gis_pending = files;
            }
            return;
        }
        let units = self.project.units().to_string();
        let crs = self.project.state.get("crs").cloned().unwrap_or(Value::Null);
        let plan = gis_read::plan_import(&crs, &units, &sources);
        let case = plan.get("case").and_then(Value::as_str).unwrap_or("none").to_string();
        let target = match case.as_str() {
            "project" => Some(crs.clone()),
            "offer" => plan.get("offer").cloned(),
            _ => None,
        };
        let pick = sources.iter().enumerate().flat_map(|(si, s)| (0..s.layers.len()).map(move |li| (si, li))).collect();
        // renderCrs: an unknown system is taken as the project's (unless there is no system at all)
        let overrides = if case == "none" {
            Vec::new()
        } else {
            sources.iter().enumerate().filter(|(_, s)| matches!(s.src, Src::None)).map(|(i, _)| (i, Src::Same)).collect()
        };
        self.gis_import = Some(GisImport { files, sources, notes, plan, pick, target, set_crs: case == "offer", overrides });
    }

    /// `civil.gis.import=ok|cancel|pick=si:li:on|off|crs=<EPSG code>|crs=offer|unknown=si:same|<code>`.
    pub fn gis_import_cmd(&mut self, spec: &str) -> Result<(), String> {
        let units = self.project.units().to_string();
        let (k, v) = spec.split_once('=').map_or((spec, ""), |(a, b)| (a, b));
        match k {
            "cancel" => {
                self.gis_import = None;
                Ok(())
            }
            "ok" => self.gis_import_go(),
            "pick" => {
                let d = self.gis_import.as_mut().ok_or("civil.gis.import: the import dialog is not open")?;
                let mut it = v.split(':');
                let si: usize = it.next().and_then(|x| x.parse().ok()).ok_or("civil.gis.import: pick=si:li:on|off")?;
                let li: usize = it.next().and_then(|x| x.parse().ok()).ok_or("civil.gis.import: pick=si:li:on|off")?;
                let on = it.next() != Some("off");
                d.pick.retain(|p| *p != (si, li));
                if on && d.sources.get(si).is_some_and(|s| li < s.layers.len()) {
                    d.pick.push((si, li));
                }
                Ok(())
            }
            "crs" => {
                let has_project = self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c)));
                let d = self.gis_import.as_mut().ok_or("civil.gis.import: the import dialog is not open")?;
                if has_project {
                    return Err("civil.gis.import: the layers go into the project's system".into());
                }
                if v == "offer" {
                    let offer = d.plan.get("offer").filter(|o| !o.is_null()).cloned().ok_or("civil.gis.import: no system is offered")?;
                    d.target = Some(offer);
                    d.set_crs = true;
                    return Ok(());
                }
                let c = crs_of_code(v)?;
                if let Some(why) = cc_io_civil::crs::crs_problem(&c, &units) {
                    self.message = why;
                    return Ok(());
                }
                d.target = Some(c);
                d.set_crs = true;
                Ok(())
            }
            "unknown" => {
                let d = self.gis_import.as_mut().ok_or("civil.gis.import: the import dialog is not open")?;
                let (si, w) = v.split_once(':').ok_or("civil.gis.import: unknown=si:same|<code>")?;
                let si: usize = si.parse().map_err(|_| "civil.gis.import: unknown=si:same|<code>")?;
                let src = if w == "same" {
                    Src::Same
                } else {
                    let c = crs_of_code(w)?;
                    let name = string_of(c.get("name"));
                    let geographic = c.get("unit").and_then(Value::as_str) == Some("deg");
                    Src::Crs { crs: c, name, matched: None, geographic, notes: None }
                };
                d.overrides.retain(|(i, _)| *i != si);
                d.overrides.push((si, src));
                Ok(())
            }
            o => Err(format!("civil.gis.import: ok, cancel, pick=, crs= or unknown= (not {o})")),
        }
    }

    /// The dialog's Import (`importInto` + addLayers): one undo step; the system set in it when chosen.
    fn gis_import_go(&mut self) -> Result<(), String> {
        let d = self.gis_import.take().ok_or("civil.gis.import: the import dialog is not open")?;
        if d.pick.is_empty() || d.needs() {
            self.gis_import = Some(d);
            return Err("civil.gis.import: nothing to import yet (choose layers, or a system)".into());
        }
        let t0 = web_time::Instant::now();
        let units = self.project.units().to_string();
        let had_crs = self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c)));
        let setting = d.set_crs && !had_crs;
        let target = d.target.clone().unwrap_or(Value::Null);
        let chosen = d.chosen();
        // the dialog is done with: its sources are taken (the chosen layers moved, not copied)
        let GisImport { sources: all, overrides, .. } = d;
        let sources: Vec<gis_read::Source> = all
            .into_iter()
            .enumerate()
            .map(|(si, mut s)| {
                s.layers = std::mem::take(&mut s.layers)
                    .into_iter()
                    .enumerate()
                    .filter(|(li, _)| chosen.contains(&(si, *li)))
                    .map(|(_, l)| l)
                    .collect();
                s
            })
            .collect();
        let (layers, notes) = gis_read::build_layers_owned(sources, &target, &units, &overrides);
        let mut keep: Vec<gis_read::BuiltLayer> = layers.into_iter().filter(|l| l.features.iter().any(|f| f.g.is_some())).collect();
        let mut files: Vec<String> = Vec::new();
        for l in &keep {
            if !files.contains(&l.source.0) {
                files.push(l.source.0.clone());
            }
        }
        if keep.is_empty() {
            self.message = format!("{} {}", t("vec.imported.none", &[]), notes.join(" "));
            return Ok(());
        }
        let label = match files.as_slice() {
            [one] => tr!("gread.undo", file = one.as_str()),
            _ => tr!("gread.undo.many", n = files.len()),
        };
        let crs_set = setting.then(|| target.clone());
        let mut ids: Vec<String> = Vec::new();
        let count: usize = keep.iter().map(|l| l.features.len()).sum();
        self.project.edit_gis(&label, &mut |mut map, s| {
            if let Some(c) = &crs_set
                && let Some(m) = s.as_object_mut()
            {
                m.insert("crs".into(), c.clone());
            }
            let seq0 = s.get("seq").and_then(Value::as_f64).unwrap_or(1.0);
            let mut seq = seq0;
            let Some(m) = s.as_object_mut() else { return Some(map) };
            let g = m.entry("gis").or_insert_with(|| json!({ "layers": [], "maps": [] }));
            if !g.is_object() {
                *g = json!({ "layers": [], "maps": [] });
            }
            let Some(gm) = g.as_object_mut() else { return Some(map) };
            let ls = gm.entry("layers").or_insert_with(|| Value::Array(Vec::new()));
            let Some(ls) = ls.as_array_mut() else { return Some(map) };
            let k0 = ls.len();
            for (k, l) in (k0..).zip(keep.iter_mut()) {
                if ls.len() >= cc_civil::gis_state::MAX_LAYERS {
                    break;
                }
                let id = format!("g{}", cc_civil::js::num_to_string(seq));
                seq += 1.0;
                ids.push(id.clone());
                let name = clean_str(&l.name, 120);
                let layer = Layer {
                    id: id.clone(),
                    name: if name.is_empty() { t("layers.default", &[]) } else { name },
                    geom: l.geom.clone(),
                    has_z: l.has_z,
                    fields: l.fields.clone(),
                    visible: true,
                    color: LAYER_TOKENS.get(k % LAYER_TOKENS.len()).copied().unwrap_or("blue").to_string(),
                    source: Source {
                        file: clean_str(&l.source.0, 200),
                        format: clean_str(&l.source.1, 40),
                        crs: l.source.2.as_deref().map(|c| clean_str(c, 200)).filter(|c| !c.is_empty()),
                    },
                    label: None,
                };
                ls.push(layer.j());
                // edit_gis runs this once: the layer's features move into the project
                map.push((id, std::sync::Arc::new(std::mem::take(&mut l.features))));
            }
            m.insert("seq".into(), cc_civil::js::jnum(seq));
            Some(map)
        });
        let first =
            self.project.points.is_empty() && arr(self.project.state.get("alignments")).is_empty() && self.gis_layers().len() == ids.len();
        self.report = notes;
        if first {
            self.fit();
        }
        self.message = tr!("vec.imported", n = ids.len(), count = num(ids.len()), features = num(count));
        self.timings.insert("gis import", t0.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    /// `civil.gis.import.info`: the dialog's layer rows, notes, system and what Import does (" | " between lines).
    pub fn gis_import_info(&self) -> Result<String, String> {
        let d = self.gis_import.as_ref().ok_or("civil.gis.import.info: the import dialog is not open")?;
        let mut out = Vec::new();
        out.push(tr!(
            if d.files.len() == 1 { "vec.files.one" } else { "vec.files.many" },
            files = d.files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>().join(", ")
        ));
        for (si, s) in d.sources.iter().enumerate() {
            for (li, l) in s.layers.iter().enumerate() {
                let empty = l.features.iter().filter(|f| f.g.is_none()).count();
                let n = if empty > 0 {
                    tr!("vec.features.empty", count = num(l.features.len()), empty = num(empty))
                } else {
                    num(l.features.len())
                };
                let on = if d.pick.contains(&(si, li)) { "[x]" } else { "[ ]" };
                out.push(format!(
                    "{on} {}  {}  {}  {}  {} · {}",
                    l.name,
                    kind_word(&l.geom, false, l.has_z),
                    n,
                    l.fields.len(),
                    s.format,
                    gis_read::src_name(&s.src)
                ));
            }
        }
        out.extend(self.gis_import_notes());
        out.extend(self.gis_import_crs_lines());
        out.push(self.gis_import_what());
        Ok(out.join(" | "))
    }

    fn gis_import_notes(&self) -> Vec<String> {
        let Some(d) = &self.gis_import else { return Vec::new() };
        let mut n = d.notes.clone();
        for s in &d.sources {
            n.extend(s.notes.iter().cloned());
            if let Src::Crs { notes: Some(sn), .. } = &s.src {
                n.extend(sn.iter().cloned());
            }
            if let Src::Bad(why) = &s.src {
                n.push(tr!("vec.import.badcrs", file = s.file.as_str(), why = why.as_str()));
            }
        }
        n.truncate(30);
        n
    }

    /// renderCrs's words: what system the layers go into.
    fn gis_import_crs_lines(&self) -> Vec<String> {
        let Some(d) = &self.gis_import else { return Vec::new() };
        let case = d.plan.get("case").and_then(Value::as_str).unwrap_or("none");
        let name = |c: &Value| string_of(c.get("name"));
        let mut out = Vec::new();
        match case {
            "project" => {
                let c = self.project.state.get("crs").cloned().unwrap_or(Value::Null);
                let key = if cc_civil::js::truthy(c.get("local")) { "vec.import.project.local" } else { "vec.import.project" };
                out.push(tr!(key, crs = name(&c)));
            }
            "offer" => {
                let offer = d.plan.get("offer").cloned().unwrap_or(Value::Null);
                let using = d.target.as_ref() == Some(&offer);
                out.push(format!("{} {}", if using { "(•)" } else { "( )" }, t("vec.offer.use", &[]).replace("{crs}", &name(&offer))));
                out.push(format!("{} {}", if using { "( )" } else { "(•)" }, t("vec.offer.pick", &[])));
                if let Some(c) = d.target.as_ref().filter(|_| !using) {
                    out.push(t("vec.offer.chosen", &[]).replace("{crs}", &name(c)));
                }
            }
            "pick" => match &d.target {
                Some(c) => out.push(t("vec.offer.will", &[]).replace("{crs}", &name(c))),
                None => out.push(string_of(d.plan.get("why"))),
            },
            _ => out.push(t("vec.import.nocrs", &[])),
        }
        for (si, s) in d.sources.iter().enumerate() {
            if !matches!(s.src, Src::None) || (case == "none" && d.target.is_none()) {
                continue;
            }
            let ov = d.overrides.iter().find(|(i, _)| *i == si).map(|(_, o)| o);
            out.push(tr!("vec.unknown.legend", file = s.file.as_str()));
            out.push(match ov {
                Some(Src::Crs { name, .. }) => tr!("vec.unknown.in", crs = name.as_str()),
                _ => t("vec.unknown.same", &[]),
            });
        }
        out
    }

    /// upd(): what Import does.
    fn gis_import_what(&self) -> String {
        let Some(d) = &self.gis_import else { return String::new() };
        let chosen = d.chosen();
        let mut bits = Vec::new();
        if chosen.is_empty() {
            bits.push(t("vec.what.none", &[]));
        } else {
            let feats: usize =
                chosen.iter().filter_map(|(si, li)| d.sources.get(*si).and_then(|s| s.layers.get(*li))).map(|l| l.features.len()).sum();
            bits.push(tr!("vec.what.layers", n = chosen.len(), count = num(chosen.len()), features = num(feats)));
            if d.needs() {
                bits.push(t("vec.what.needs", &[]));
            } else if let Some(target) = &d.target {
                let local = cc_civil::js::truthy(target.get("local"));
                let mut conv: Vec<String> = Vec::new();
                for (si, _) in &chosen {
                    let x = d.src_of(*si);
                    let differs = match &x {
                        Some(Src::Wgs84) => true,
                        Some(Src::Crs { crs, .. }) => crs.get("proj4") != target.get("proj4") || local,
                        _ => false,
                    };
                    if differs && let Some(x) = x {
                        let n = gis_read::src_name(&x);
                        if !conv.contains(&n) {
                            conv.push(n);
                        }
                    }
                }
                if !conv.is_empty() {
                    bits.push(tr!(
                        if local { "vec.what.convert.local" } else { "vec.what.convert" },
                        from = conv.join(", "),
                        to = string_of(target.get("name"))
                    ));
                }
                if d.set_crs && !self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c))) {
                    bits.push(tr!("vec.what.becomes", crs = string_of(target.get("name"))));
                }
            }
            bits.push(t("vec.what.undo", &[]));
        }
        bits.join(" ")
    }

    // ------------------------------------------------------------------------------------------- the coordinate system

    /// `civil.crs=<EPSG code>|none` (gis-crs-ui.js apply / remove; the project's coordinates are not converted).
    pub fn crs_set(&mut self, v: &str) -> Result<(), String> {
        let v = v.trim();
        if v == "none" || v.is_empty() {
            if !self.project.state.get("crs").is_some_and(|c| cc_civil::js::truthy(Some(c))) {
                return Ok(());
            }
            self.project.change(&t("crs.d.undo.remove", &[]), &mut |s: &mut Value| {
                if let Some(m) = s.as_object_mut() {
                    m.insert("crs".into(), Value::Null);
                }
            });
            self.message = t("crs.d.removed", &[]);
            return Ok(());
        }
        let next = crs_of_code(v)?;
        if let Some(why) = cc_io_civil::crs::crs_problem(&next, self.project.units()) {
            return Err(why);
        }
        let same =
            self.project.state.get("crs").is_some_and(|c| cc_civil::project::js_stringify(c) == cc_civil::project::js_stringify(&next));
        if same {
            return Ok(());
        }
        let name = string_of(next.get("name"));
        self.project.change(&tr!("crs.d.undo.set", crs = name.as_str()), &mut |s: &mut Value| {
            if let Some(m) = s.as_object_mut() {
                m.insert("crs".into(), next.clone());
            }
        });
        self.message = tr!("crs.d.set", crs = name.as_str());
        Ok(())
    }

    /// The status bar's latitude / longitude (gis-crs-ui.js showLatLon): None without a usable system.
    pub fn lat_lon_at(&self, x: f64, y: f64) -> Option<String> {
        let c = self.project.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c)))?;
        ensure_catalog();
        if cc_io_civil::crs::crs_problem(c, self.project.units()).is_some() {
            return None;
        }
        let f = cc_io_civil::crs::frame(c).ok()??;
        Some(match f.to_lon_lat(x, y) {
            Ok(ll) => {
                let [lon, lat] = cc_io_civil::crs::nn(ll);
                cc_io_civil::crs_pick::lat_lon_text(lon, lat, &self.pt_settings.latlon)
            }
            Err(_) => t("crs.d.latlon.off", &[]),
        })
    }
}

/// gis-ui.js install: `await loadCatalog()` (the EPSG list compiled in) before any GIS part runs.
pub fn ensure_catalog() {
    if !cc_io_civil::crs::catalog_loaded() {
        cc_io_civil::crs::set_catalog();
    }
}

/// An EPSG code → a project system object (crs.js crsFromEntry).
pub fn crs_of_code(v: &str) -> Result<Value, String> {
    ensure_catalog();
    let code: f64 = v.trim().trim_start_matches("EPSG:").parse().map_err(|_| format!("not an EPSG code: {v}"))?;
    let e = cc_io_civil::crs::by_code(code)?.ok_or_else(|| tr!("gread.epsg.unknown", code = code))?;
    Ok(cc_io_civil::crs::crs_from_entry(e))
}

// ------------------------------------------------------------------------------------------------ panels

/// The left panel's GIS layers section (gis-vector-ui.js renderPanel): top layer first.
pub fn layers_section(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    let layers = cv.gis_layers();
    if layers.is_empty() {
        return;
    }
    head(ui, tk, &t0("v3d.show.gis"));
    let pal = crate::civilplan::Palette::of(tk, cv.dark);
    for l in layers.iter().rev() {
        let n = cc_io_civil::gis_layers::features_of(&cv.project.gis_features, &l.id).len();
        ui.horizontal(|ui| {
            let mut vis = l.visible;
            if ui.checkbox(&mut vis, "").on_hover_text(tr!("vec.layer.show", layer = l.name.as_str())).changed() {
                acts.push(Act::Run("civil.gis.layer", format!("show={}:{}", l.id, if vis { "on" } else { "off" })));
            }
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 1.0, pal.token(&l.color));
            ui.label(RichText::new(&l.name).color(tk.text)).on_hover_text(format!("{} — {}", l.name, l.source.file));
            let count = match l.geom.as_str() {
                "pt" | "ln" | "pg" => tr!(&format!("vec.count.{}", l.geom), n = n, count = num(n)),
                _ => num(n),
            };
            ui.label(RichText::new(count).small().color(tk.text_dim)).on_hover_text(kind_word(&l.geom, l.has_z, false));
            ui.menu_button("⋯", |ui| {
                if ui.button(t0("vec.zoomto")).clicked() {
                    acts.push(Act::Run("civil.gis.layer", format!("zoom={}", l.id)));
                    ui.close();
                }
                ui.label(RichText::new(t0("vec.layer.color.title")).small());
                ui.horizontal_wrapped(|ui| {
                    for tok in LAYER_TOKENS {
                        if ui.selectable_label(l.color == tok, cc_io_civil::gis_layers::token_label(tok)).clicked() {
                            acts.push(Act::Run("civil.gis.layer", format!("color={}:{tok}", l.id)));
                        }
                    }
                });
                if !l.fields.is_empty() {
                    ui.label(RichText::new(t0("gis.label.with")).small());
                    if ui.selectable_label(l.label.is_none(), t0("gis.label.none")).clicked() {
                        acts.push(Act::Run("civil.gis.layer", format!("label={}:", l.id)));
                    }
                    for f in &l.fields {
                        if ui.selectable_label(l.label.as_deref() == Some(f.name.as_str()), &f.name).clicked() {
                            acts.push(Act::Run("civil.gis.layer", format!("label={}:{}", l.id, f.name)));
                        }
                    }
                }
                ui.separator();
                if ui.button(t0("vec.layer.remove")).clicked() {
                    acts.push(Act::Run("civil.gis.layer", format!("remove={}", l.id)));
                    ui.close();
                }
            });
        });
    }
}

/// A picked feature's properties (gis-vector-ui.js featureProps): read-only facts and attributes.
pub fn feature_props(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, lid: &str, i: usize, acts: &mut Vec<Act>) -> bool {
    let Some(l) = cv.gis_layers().into_iter().find(|l| l.id == lid) else { return false };
    let fs = cc_io_civil::gis_layers::features_of(&cv.project.gis_features, lid);
    let Some(f) = fs.get(i).cloned() else { return false };
    head(ui, tk, &t0("vec.feat.head"));
    let rows = feature_rows(&l, &f, i, fs.len());
    egui::Grid::new(("cv_gis_feat", lid, i)).num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
        for (k, v) in &rows {
            ui.label(RichText::new(k).color(tk.text_dim));
            ui.label(RichText::new(v).color(tk.text));
            ui.end_row();
        }
    });
    head(ui, tk, &t0("vec.f.attrs"));
    if l.fields.is_empty() {
        note(ui, tk, &t0("vec.f.noattrs"));
    } else {
        egui::Grid::new(("cv_gis_attrs", lid, i)).num_columns(2).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
            for (k, v) in attr_rows(&l, &f) {
                ui.label(RichText::new(k).small().strong().color(tk.text_dim));
                ui.label(RichText::new(v).small().color(tk.text));
                ui.end_row();
            }
        });
    }
    note(ui, tk, &t0("vec.f.note"));
    if f.g.is_some() && link(ui, tk, &t0("vec.zoomto"), false) {
        let b = cc_io_civil::gis_geom::bounds(f.g.as_ref());
        acts.push(Act::Run("civil.gis.zoom", format!("{},{},{},{}", b.min_x, b.min_y, b.max_x, b.max_y)));
    }
    true
}

fn feature_rows(l: &Layer, f: &Feature, i: usize, total: usize) -> Vec<(String, String)> {
    let g = f.g.as_ref();
    let mut zs: Vec<f64> = Vec::new();
    let verts = g.map_or(0, |g| {
        let mut n = 0usize;
        let mut add = |p: &Vec<f64>| {
            n += 1;
            if let Some(z) = p.get(2) {
                zs.push(*z);
            }
        };
        match g {
            Geom::Pt(c) => add(c),
            Geom::Ln(parts) => parts.iter().flatten().for_each(&mut add),
            Geom::Pg(polys) => polys.iter().flatten().flatten().for_each(&mut add),
        }
        n
    });
    let vs = num(verts);
    let shape = match g {
        None => t("vec.shape.none", &[]),
        Some(Geom::Pt(_)) => t("vec.shape.pt", &[]),
        Some(Geom::Ln(c)) if c.len() > 1 => tr!("vec.shape.lns", n = c.len(), verts = vs.as_str()),
        Some(Geom::Ln(_)) => tr!("vec.shape.ln", verts = vs.as_str()),
        Some(Geom::Pg(c)) => {
            let holes = c.iter().any(|p| p.len() > 1);
            if c.len() > 1 {
                tr!(if holes { "vec.shape.pgs.holes" } else { "vec.shape.pgs" }, n = c.len(), verts = vs.as_str())
            } else {
                tr!(if holes { "vec.shape.pg.holes" } else { "vec.shape.pg" }, verts = vs.as_str())
            }
        }
    };
    let mut rows = vec![
        (t("vec.f.layer", &[]), l.name.clone()),
        (t("vec.f.feature", &[]), tr!("vec.f.of", i = num(i + 1), total = num(total))),
        (t("vec.f.shape", &[]), shape),
    ];
    if let Some(Geom::Pt(c)) = g {
        rows.push((t("vec.f.easting", &[]), cc_civil::js::to_fixed(c.first().copied().unwrap_or(f64::NAN), 3)));
        rows.push((t("vec.f.northing", &[]), cc_civil::js::to_fixed(c.get(1).copied().unwrap_or(f64::NAN), 3)));
    }
    if !zs.is_empty() {
        let v = if zs.len() == 1 {
            cc_civil::js::to_fixed(zs.first().copied().unwrap_or(0.0), 3)
        } else {
            let (mn, mx) = zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(*z), b.max(*z)));
            tr!("vec.f.range", min = cc_civil::js::to_fixed(mn, 2), max = cc_civil::js::to_fixed(mx, 2))
        };
        rows.push((t("vec.f.elevation", &[]), v));
    }
    let file = if l.source.file.is_empty() { "—".to_string() } else { l.source.file.clone() };
    rows.push((
        t("vec.f.from", &[]),
        if l.source.format.is_empty() { file } else { tr!("vec.f.from.format", file = file.as_str(), format = l.source.format.as_str()) },
    ));
    rows.push((t("vec.f.system", &[]), l.source.crs.clone().unwrap_or_else(|| t("vec.f.system.none", &[]))));
    rows
}

fn attr_rows(l: &Layer, f: &Feature) -> Vec<(String, String)> {
    l.fields
        .iter()
        .map(|fd| {
            let v = f.p.get(&fd.name);
            let s = match v {
                None | Some(Value::Null) => "—".to_string(),
                Some(Value::String(s)) if s.is_empty() => "—".to_string(),
                Some(x) if fd.ty == "bool" => t(if cc_civil::js::truthy(Some(x)) { "gis.label.yes" } else { "gis.label.no" }, &[]),
                Some(Value::String(s)) => s.clone(),
                Some(x) => cc_civil::js::to_js_string(x),
            };
            (fd.name.clone(), s)
        })
        .collect()
}

impl CivilView {
    /// `civil.gis.feature.info`: the picked feature's rows and attributes (" | " between lines).
    pub fn gis_feature_info(&self) -> Result<String, String> {
        let Some(Sel::Gis(lid, i)) = &self.sel else { return Err("civil.gis.feature.info: pick a GIS feature first".into()) };
        let l = self.gis_layers().into_iter().find(|l| l.id == *lid).ok_or("civil.gis.feature.info: no such layer")?;
        let fs = cc_io_civil::gis_layers::features_of(&self.project.gis_features, lid);
        let f = fs.get(*i).ok_or("civil.gis.feature.info: no such feature")?;
        let mut out: Vec<String> = feature_rows(&l, f, *i, fs.len()).into_iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.extend(attr_rows(&l, f).into_iter().map(|(k, v)| format!("{k}={v}")));
        Ok(out.join(" | "))
    }
}

/// The GIS import dialog (gis-vector-ui.js importDialog) as a window.
pub fn import_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    if cv.gis_import.is_none() {
        return;
    }
    let mut open = true;
    let info = cv.gis_import.as_ref().map(|d| {
        let rows: Vec<(usize, usize, bool, [String; 5])> = d
            .sources
            .iter()
            .enumerate()
            .flat_map(|(si, s)| {
                s.layers.iter().enumerate().map(move |(li, l)| {
                    let empty = l.features.iter().filter(|f| f.g.is_none()).count();
                    let n = if empty > 0 {
                        tr!("vec.features.empty", count = num(l.features.len()), empty = num(empty))
                    } else {
                        num(l.features.len())
                    };
                    (
                        si,
                        li,
                        d.pick.contains(&(si, li)),
                        [
                            l.name.clone(),
                            kind_word(&l.geom, false, l.has_z),
                            n,
                            l.fields.len().to_string(),
                            format!("{} · {}", s.format, gis_read::src_name(&s.src)),
                        ],
                    )
                })
            })
            .collect();
        let names = d.files.iter().map(|f| f.0.clone()).collect::<Vec<_>>().join(", ");
        let case = d.plan.get("case").and_then(Value::as_str).unwrap_or("none").to_string();
        let offer = d.plan.get("offer").cloned().unwrap_or(Value::Null);
        let using_offer = d.target.as_ref() == Some(&offer);
        let disabled = d.pick.is_empty() || d.needs();
        let unknown: Vec<(usize, String, bool)> = d
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.src, Src::None))
            .map(|(si, s)| (si, s.file.clone(), !matches!(d.overrides.iter().find(|(i, _)| *i == si).map(|x| &x.1), Some(Src::Crs { .. }))))
            .collect();
        (rows, names, case, offer, using_offer, disabled, d.plan.get("near").cloned(), unknown, d.files.len())
    });
    let Some((rows, names, case, offer, using_offer, disabled, near, unknown, nfiles)) = info else { return };
    let notes = cv.gis_import_notes();
    let crs_lines = cv.gis_import_crs_lines();
    let what = cv.gis_import_what();
    egui::Window::new(t0("layers.undo.import"))
        .id(egui::Id::unique("cv_gis_import"))
        .open(&mut open)
        .collapsible(false)
        .default_width(620.0)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(tr!(if nfiles == 1 { "vec.files.one" } else { "vec.files.many" }, files = names.as_str()))
                    .small()
                    .color(tk.text_dim),
            );
            egui::Grid::new("cv_gis_imp_rows").num_columns(6).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
                ui.label("");
                for h in [t0("vec.f.layer"), t0("vec.col.kind"), t0("vec.col.features"), t0("vec.col.fields"), t0("vec.col.format")] {
                    ui.label(RichText::new(h).small().strong().color(tk.text_dim));
                }
                ui.end_row();
                for (si, li, on, r) in &rows {
                    let mut v = *on;
                    if ui.checkbox(&mut v, "").on_hover_text(tr!("vec.import.layer", layer = r[0].as_str())).changed() {
                        acts.push(("civil.gis.import", format!("pick={si}:{li}:{}", if v { "on" } else { "off" })));
                    }
                    for c in r {
                        ui.label(RichText::new(c).small().color(tk.text));
                    }
                    ui.end_row();
                }
            });
            report_pub(ui, tk, &notes, false);
            ui.label(RichText::new(t0("vec.import.crs")).strong().color(tk.text_strong));
            if case == "offer" {
                if ui.radio(using_offer, t0("vec.offer.use").replace("{crs}", &string_of(offer.get("name")))).clicked() {
                    acts.push(("civil.gis.import", "crs=offer".into()));
                }
            } else {
                for l in &crs_lines {
                    ui.label(RichText::new(l).small().color(tk.text));
                }
            }
            if matches!(case.as_str(), "offer" | "pick") {
                // the systems near the layers (crs.js suggest), as 0.5's askCrs lists them
                let list = near
                    .as_ref()
                    .and_then(|n| Some((n.get(0)?.as_f64()?, n.get(1)?.as_f64()?)))
                    .and_then(|(lon, lat)| cc_io_civil::crs::suggest(lon, lat, Some(cv.project.units()), 30.0).ok())
                    .unwrap_or_default();
                egui::ComboBox::new("cv_gis_imp_crs", t0("vec.ask.system")).selected_text(t0("vec.choose")).width(360.0).show_ui(
                    ui,
                    |ui| {
                        for e in &list {
                            if ui.selectable_label(false, format!("EPSG:{} — {}", cc_civil::js::num_to_string(e.code), e.name)).clicked()
                            {
                                acts.push(("civil.gis.import", format!("crs={}", cc_civil::js::num_to_string(e.code))));
                            }
                        }
                    },
                );
            }
            for (si, file, same) in &unknown {
                if case == "none" {
                    break;
                }
                ui.label(RichText::new(tr!("vec.unknown.legend", file = file.as_str())).small().color(tk.text_dim));
                if ui.radio(*same, t0("vec.unknown.same")).clicked() {
                    acts.push(("civil.gis.import", format!("unknown={si}:same")));
                }
            }
            ui.label(RichText::new(t0("vec.import.what")).strong().color(tk.text_strong));
            ui.add(egui::Label::new(RichText::new(&what).small().color(tk.text)).wrap());
            ui.horizontal(|ui| {
                if ui.button(t0("vec.cancel")).clicked() {
                    acts.push(("civil.gis.import", "cancel".into()));
                }
                if ui.add_enabled(!disabled, egui::Button::new(t0("vec.import.go"))).clicked() {
                    acts.push(("civil.gis.import", "ok".into()));
                }
            });
        });
    if !open {
        acts.push(("civil.gis.import", "cancel".into()));
    }
}

// ------------------------------------------------------------------------------------------------ GIS menu, Coordinates

/// The Coordinates window (gis-crs-ui.js runDialog, project mode, Grid only): search, the systems the site's
/// coordinates fit, Apply / Remove.
#[derive(Clone, Debug, Default)]
pub struct CrsDlg {
    pub query: String,
    pub list: Vec<(f64, String)>,
    pub chosen: Option<f64>,
    pub said: String,
}

impl CivilView {
    /// "Systems these coordinates fit" (crs-pick.js systemsThatFit at the site's middle).
    pub fn crs_fits(&mut self) -> Vec<(f64, String)> {
        ensure_catalog();
        let Some((x0, y0, x1, y1)) = crate::civilplan::extents(&mut self.project) else { return Vec::new() };
        let units = self.project.units().to_string();
        cc_io_civil::crs_pick::systems_that_fit(Some((x0 + x1) / 2.0), Some((y0 + y1) / 2.0), &units, 40.0)
            .map(|(l, _)| l.iter().map(|f| (f.entry.code, f.entry.name.clone())).collect())
            .unwrap_or_default()
    }
    pub fn crs_search(&mut self, q: &str) -> Vec<(f64, String)> {
        ensure_catalog();
        cc_io_civil::crs::search(q, None, 40.0).map(|l| l.iter().map(|e| (e.code, e.name.clone())).collect()).unwrap_or_default()
    }
}

/// The toolbar's GIS group (ui.inc 'GIS'): Coordinates, Import, Export, Map.
pub fn gis_menu(ui: &mut egui::Ui, acts: &mut Vec<(&'static str, String)>) {
    ui.menu_button("GIS", |ui| {
        for (cmd, key) in [
            ("civil.crs.open", "ui.tb.crs"),
            ("civil.gis.pick", "ui.tb.gis-import"),
            ("civil.gis.export", "ui.tb.gis-export"),
            ("civil.maps.open", "ui.tb.basemap"),
        ] {
            if ui.button(t0(key)).on_hover_text(t0(&format!("{key}.title"))).clicked() {
                acts.push((cmd, String::new()));
                ui.close();
            }
        }
    });
}

/// The Coordinates window.
pub fn crs_window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(mut d) = cv.crs_dlg.clone() else { return };
    let now = cv.project.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c))).cloned();
    let mut open = true;
    egui::Window::new(t0("ui.tb.crs")).id(egui::Id::unique("cv_crs")).open(&mut open).collapsible(false).default_width(460.0).show(
        ctx,
        |ui| {
            ui.label(RichText::new(t0("crs.d.now")).strong().color(tk.text_strong));
            match &now {
                Some(c) => {
                    let code = c
                        .get("code")
                        .filter(|x| !x.is_null())
                        .map_or(String::new(), |x| format!(" (EPSG:{})", cc_civil::js::to_js_string(x)));
                    ui.label(RichText::new(format!("{}{code}", string_of(c.get("name")))).color(tk.text));
                    if ui.button(t0("crs.d.remove")).clicked() {
                        acts.push(("civil.crs", "none".into()));
                    }
                }
                None => {
                    ui.label(RichText::new(t0("crs.d.none").replace("{none}", &t0("crs.d.none.word"))).small().color(tk.text_dim));
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t0(if now.is_some() { "crs.d.change.head" } else { "crs.d.pick.head" })).strong().color(tk.text_strong));
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut d.query).hint_text(t0("crs.d.search.ph")).desired_width(280.0));
                if ui.button(t0("crs.d.search")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    d.list = cv.crs_search(&d.query);
                    d.said = if d.list.is_empty() {
                        t0("crs.d.nomatch").replace("{q}", &d.query)
                    } else {
                        t0("crs.d.match").replace("{count}", &d.list.len().to_string()).replace("{q}", &d.query)
                    };
                }
            });
            if ui.button(t0("crs.d.fits")).clicked() {
                d.list = cv.crs_fits();
                d.said.clear();
            }
            if !d.said.is_empty() {
                ui.label(RichText::new(&d.said).small().color(tk.text_dim));
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for (code, name) in &d.list {
                    if ui
                        .selectable_label(d.chosen == Some(*code), format!("EPSG:{} — {name}", cc_civil::js::num_to_string(*code)))
                        .clicked()
                    {
                        d.chosen = Some(*code);
                    }
                }
            });
            if let Some(c) = d.chosen {
                if let Ok(v) = crs_of_code(&cc_civil::js::num_to_string(c))
                    && let Some(why) = cc_io_civil::crs::crs_problem(&v, cv.project.units())
                {
                    report_pub(ui, tk, &[why], true);
                }
                if ui.button(t0("crs.d.apply")).clicked() {
                    acts.push(("civil.crs", cc_civil::js::num_to_string(c)));
                }
            }
            ui.label(RichText::new(t0("crs.d.grid.note")).small().color(tk.text_dim));
        },
    );
    cv.crs_dlg = if open { Some(d) } else { None };
}

#[cfg(test)]
mod tests {
    use super::*;

    const GJ: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"NAME":"A"},"geometry":{"type":"LineString","coordinates":[[-104.99,39.74],[-104.989,39.741]]}},
        {"type":"Feature","properties":{"NAME":"B"},"geometry":{"type":"Point","coordinates":[-104.99,39.742]}}]}"#;

    #[test]
    fn a_geojson_comes_in_through_the_dialog_and_draws_and_exports() {
        let mut cv = CivilView::sample();
        cv.gis_open("x.geojson", GJ.as_bytes().to_vec());
        assert!(cv.gis_import.is_some());
        assert!(cv.gis_import_cmd("ok").is_err(), "latitude / longitude need a system first");
        cv.gis_import_cmd("crs=2232").unwrap();
        cv.gis_import_cmd("ok").unwrap();
        let ls = cv.gis_layers();
        assert_eq!(ls.len(), 2, "{ls:?}");
        assert!(cv.project.state["crs"].is_object(), "the system was set with the layers");
        let (items, toks) = cv.gis_scene(1.0);
        assert!(!items.is_empty() && toks.len() == 2);
        let id = ls[0].id.clone();
        cv.gis_layer_cmd(&format!("color={id}:red")).unwrap();
        assert!(cv.gis_layer_cmd(&format!("color={id}:pink")).is_err());
        cv.gis_layer_cmd(&format!("label={id}:NAME")).unwrap();
        assert!(cv.gis_layers()[0].label.is_some());
        let (name, bytes, said) = cv.gis_export(&["points".into(), format!("gis:{id}")], "geojson", 0.0, cc_clock::Zone::Utc).unwrap();
        assert!(name.ends_with(".geojson") && !bytes.is_empty() && !said.is_empty());
        cv.gis_layer_cmd(&format!("remove={id}")).unwrap();
        assert_eq!(cv.gis_layers().len(), 1);
        cv.undo();
        assert_eq!(cv.gis_layers().len(), 2);
        assert!(cv.lat_lon_at(3_140_000.0, 1_700_000.0).is_some());
        cv.crs_set("none").unwrap();
        assert!(cv.lat_lon_at(0.0, 0.0).is_none());
    }

    #[test]
    fn dos_dates_follow_the_local_clock() {
        let d = crate::civilgisx::dos_date(cc_clock::Zone::Fixed(-360).local_ms(1_760_745_600_000.0));
        assert_eq!((d.year, d.month, d.day, d.hour), (2025, 10, 17, 18));
    }
}
