//! DetailCraft in the app (M14): a model opened from DetailCraft 0.5's own file (`<name>.detailcraft.json`,
//! `Model.toJSON`), DetailCraft's sample frame (sample.js) or an IFC file imported into it (ifc-import.js, while a
//! DetailCraft model is open: as detailcraft.js does), built through the engine (cc-detail `build` → `markParts`,
//! `markFootings`, `bendingSchedule`: detailcraft.js `recompute`) after every change and shown as the 3D scene
//! (detail3d.rs), with the panels (detailpanel.rs) and the exports (detailexport.rs). Every edit goes through the
//! engine's own `Model::change` (one undo step, its own undo / redo stacks); this module keeps the selection, the
//! per-version caches and maps the `detail.*` UI commands.

use cc_detail::build::Built;
use cc_detail::i18n::{F, t, t0};
use cc_detail::js::str_to_number;
use cc_detail::marks::Marks;
use cc_detail::model::Model;
use cc_detail::rebar::Schedule;
use egui::{Rect, RichText};
use serde_json::{Value, json};

use crate::detail3d::{self, DetailScene, Ref};
use crate::detailexport::{self, Outcome, Settings};
use crate::theme::{Preset, Tokens};
use crate::view3d::View3;
use crate::{ImportedFile, Params, UiApp};

/// Largest model file opened (detailcraft.js: 50 MB) and IFC file imported (200 MB).
pub const MAX_MODEL_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_IFC_BYTES: usize = 200 * 1024 * 1024;
/// How near (points) a grid intersection snaps (view3d.js SNAP_PX).
pub const SNAP_PX: f64 = 18.0;

/// The `detail.*` UI commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("detail.sample", "Open DetailCraft's sample", "", "{} → sample.js: the two-bay frame with connections, footings and rebar"),
    ("detail.new", "New DetailCraft model", "", "{} → an empty model on the starter grids (starterState)"),
    (
        "detail.select",
        "Select",
        "",
        "{name: \"member:m3\" | \"conn:j2\" | \"footing:f1\" | \"stair:s1\" | \"\"} → selects it (Properties shows it); empty clears",
    ),
    (
        "detail.set",
        "Change a member",
        "",
        "{name: \"<member id>.<field>=<value>\"} → field: name, profile, material, position (top|middle|bottom), rotation, phase, start / end (\"x,y,z\" mm), thickness / width (plates); \"model.name=<text>\": the model's name. One undo step, then rebuild",
    ),
    ("detail.undo", "Undo", "Ctrl+Z", "{}"),
    ("detail.redo", "Redo", "Ctrl+Y", "{}"),
    ("detail.save", "Save", "Ctrl+S", "{} → <name>.detailcraft.json (Model.toJSON, 0.5's format) through the save service"),
    (
        "detail.export",
        "Export",
        "",
        "{name: \"project\"|\"ifc\"|\"nc\"|\"bom\"|\"bending\"|\"kiss\"|\"cutlist\"|\"report-csv\"|\"report-pdf\"|\"drawings[:pdf|dxf|dwg-AC1015…AC1032]\"|\"sheet:<index>[:format]\"|\"shop[:<member id>]\" + optional \"@<time ms>\" (a fixed clock; DWG GUIDs then a fixed sequence)} → the file 0.5 writes, through the save service",
    ),
    (
        "detail.setting",
        "DetailCraft setting",
        "",
        "{name: \"paper=A4|A3|A2|A1|ANSI-B|ANSI-C|ANSI-D\" | \"kiss=auto|imperial|metric\" | \"barNames=…\" | \"shapeCodes=dc|aci|bs8666\" | \"stock=6000 12000\" | \"kerf=…\" | \"trim=…\" | \"keep=…\" | \"bolts=true|false\"}",
    ),
    (
        "detail.clash",
        "Clash check",
        "",
        "{factor?: tolerance mm (0…50, default 1)} → clash.js on the shown phases; the Clashes tab lists them",
    ),
    ("detail.phase", "Show or hide a phase", "", "{factor: phase number} → switches it in the view (and the clash check)"),
    ("detail.tab", "Report tab", "", "{name: problems|bom|clashes|phases|bending|nest}"),
    ("detail.camera", "3D view direction", "", "{name: iso|top|front|back|left|right}"),
    ("detail.timings", "Timings", "", "{} → the status line lists the last open / build / edit / select / scene times in ms"),
    ("detail.tool", "Tool", "V B C R P F S M", "{name: select|beam|column|brace|plate|footing|stair|move|paste} → detailcraft.js setTool"),
    (
        "detail.node",
        "Click a grid intersection",
        "",
        "{name: \"gy/gx/level\" | \"x,y,z\"} → what a click on that intersection does with the tool (placeAt)",
    ),
    (
        "detail.opt",
        "Tool option",
        "",
        "{name: \"<tool>.<field>=<value>\"} → beam|column|brace .profile/.material/.position/.rotation, column.to, plate.t/.w/…, footing.L/.mat.barX/.starters.on/…, stair.rise/.tread.t/…",
    ),
    ("detail.bygrid", "Add by grid", "", "{name: \"gy/gx/level\" | \"gy1/gx1/l1;gy2/gx2/l2\"} → the tool options' Add by grid form"),
    (
        "detail.act",
        "Action",
        "",
        "{name: endplates|sheartabs|baseplate|splice|gusset|similar|copy|cut|paste|paste-in-place|duplicate|move|delete|select-all|select-none|remove-bolts}",
    ),
    ("detail.paste", "Paste by form", "", "{name: \"offset:x,y,z\" | \"at:gy/gx/level\" | \"in-place\"} → the paste tool's forms"),
    (
        "detail.xf",
        "Move, rotate, mirror",
        "",
        "{name: \"move:dx,dy,dz\" | \"rotate:deg[@gy/gx]\" | \"mirror:x|y:<label>\", \":copy\" keeps the original}",
    ),
    (
        "detail.many",
        "Change on all of them",
        "",
        "{name: \"profile|plateT|plateW|material|position|rotation=<value>\"} on the selected members",
    ),
    ("detail.grids", "Apply grids", "", "{name: \"1 2 3|6000 6000|A B|6000\"}"),
    ("detail.levels", "Apply levels", "", "{name: \"Level 1=0;Level 2=4000\"}"),
    ("detail.phases", "Phases", "", "{name: add | rename:<n>=<name> | remove:<n> | current:<n> | per:true|false | of:<n>}"),
    ("detail.hole", "Holes", "", "{name: \"<member>:add:face,x,y,d\" | \"<member>:remove:<hole id>\"}"),
    ("detail.cope", "Copes", "", "{name: \"<member>:add:end,side,length,depth\" | \"<member>:remove:<cope id>\"}"),
    ("detail.profile", "Custom profile", "", "{name: \"<name>;<kind>;<dim>=<value>;…\"}"),
    ("detail.view", "View", "", "{name: 3d | plan[:<level>] | elevation[:<lettered grid line>]}"),
    (
        "detail.window",
        "Selection window",
        "",
        "{name: \"[+]x1,y1:x2,y2\" (view points) | \"[+]plan:x1,y1:x2,y2\" (mm on the plan's level)} → to the right wholly inside, to the left touching",
    ),
    ("detail.lasso", "Selection lasso", "", "{name: \"[+]x,y;x,y;…\" (view points)}"),
    ("detail.settings", "Settings window", "", "{name: open | close}"),
];

/// The report tabs (detailcraft.js `reportTab`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Problems,
    Bom,
    Clashes,
    Phases,
    Bending,
    Nest,
}

impl Tab {
    pub fn parse(s: &str) -> Option<Tab> {
        Some(match s {
            "problems" => Tab::Problems,
            "bom" => Tab::Bom,
            "clashes" => Tab::Clashes,
            "phases" => Tab::Phases,
            "bending" => Tab::Bending,
            "nest" => Tab::Nest,
            _ => return None,
        })
    }
}

/// A clash check's result, kept until the model changes (detailcraft.js `clashRes`).
pub struct ClashRes {
    pub clashes: Vec<cc_detail::clash::Clash>,
    pub checked: usize,
    pub version: u64,
    pub hidden: usize,
    pub ms: f64,
}

/// One open DetailCraft model and its view.
pub struct DetailView {
    pub model: Model,
    /// the file it came from
    pub name: String,
    pub built: Built,
    pub marks: Marks,
    pub footing_marks: Vec<(String, String)>,
    pub schedule: Schedule,
    /// `model.version` the derived data was made for
    built_version: u64,
    pub sel: Option<Ref>,
    pub scene: Option<DetailScene>,
    scene_key: Option<(u64, Vec<u64>, bool)>,
    sel_shown: Option<(Vec<Ref>, ViewMode, String, String)>,
    pub view: View3,
    seq: u64,
    pub settings: Settings,
    pub hidden_phases: Vec<f64>,
    pub clash: Option<ClashRes>,
    pub clash_tol: f64,
    pub tab: Tab,
    pub message: String,
    /// the IFC import's report lines (detailcraft.js `showImportReport`)
    pub report: Vec<String>,
    /// last timings (ms) by step
    pub timings: Vec<(&'static str, f64)>,
    /// text being typed in Properties: (field key, text)
    pub editing: Option<(String, String)>,
    /// the drawings format chosen in the bar
    pub dwg_format: String,
    /// the report tab's tables, made once per model version and settings
    pub tables: Option<crate::detailpanel::TabTables>,
    /// two or more selected (sel is then None: detailcraft.js `multi`)
    pub multi: Vec<Ref>,
    /// the tool state (detailtools.rs)
    pub tools: crate::detailtools::Tools,
    /// 3D, plan (looking down on a level) or elevation (looking north at a lettered grid line)
    pub mode: ViewMode,
    /// the plan's level and the elevation's grid line (names)
    pub vlevel: String,
    pub vgrid: String,
    /// a selection window or lasso being dragged
    pub win: Option<Win>,
    /// the panels' form fields (detailprops.rs)
    pub forms: crate::detailprops::Forms,
    /// the Settings window is open
    pub settings_open: bool,
    /// points to pixels of the last frame (commands with view points use it)
    pub ppp: f64,
}

/// The view's mode (detailcraft.js `viewMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    D3,
    Plan,
    Elevation,
}

/// A selection window or lasso being dragged (view3d.js `win`), in view points.
#[derive(Clone, Debug)]
pub struct Win {
    pub start: egui::Pos2,
    pub cur: egui::Pos2,
    pub mode: Option<&'static str>,
    pub add: bool,
    pub moved: bool,
    pub lasso: bool,
    pub path: Vec<egui::Pos2>,
    pub fixed: bool,
}

fn ms(t0: web_time::Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// detailcraft.js `typeName(t)`.
pub fn type_name(kind: &str) -> String {
    let k = format!("app.type.{kind}");
    if cc_detail::i18n::has(&k) {
        t0(&k)
    } else {
        let mut c = kind.chars();
        c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
    }
}

impl DetailView {
    pub fn new(model: Model, name: &str) -> DetailView {
        let mut v = DetailView {
            model,
            name: name.to_string(),
            built: Built::default(),
            marks: Marks::default(),
            footing_marks: Vec::new(),
            schedule: Schedule { rows: Vec::new(), by_footing: Vec::new() },
            built_version: u64::MAX,
            sel: None,
            scene: None,
            scene_key: None,
            sel_shown: None,
            view: View3::default(),
            seq: 0,
            settings: Settings::default(),
            hidden_phases: Vec::new(),
            clash: None,
            clash_tol: 1.0,
            tab: Tab::Problems,
            message: String::new(),
            report: Vec::new(),
            timings: Vec::new(),
            editing: None,
            dwg_format: "pdf".into(),
            tables: None,
            multi: Vec::new(),
            tools: crate::detailtools::Tools::default(),
            mode: ViewMode::D3,
            vlevel: String::new(),
            vgrid: String::new(),
            win: None,
            forms: crate::detailprops::Forms::default(),
            settings_open: false,
            ppp: 1.0,
        };
        v.tools.cur_phase =
            cc_detail::js::arr(v.model.state.get("phases")).first().and_then(|p| p.get("n")).and_then(Value::as_f64).unwrap_or(1.0);
        v.recompute();
        v
    }

    /// File → Sample (sample.js `sampleModel()`).
    pub fn sample() -> Result<DetailView, String> {
        let t1 = web_time::Instant::now();
        let m = cc_detail::sample::sample_model("Sample frame")?;
        let open = ms(t1);
        let mut v = DetailView::new(m, "Sample frame");
        v.timings.insert(0, ("open", open));
        v.message = t0("app.file.sample");
        Ok(v)
    }

    /// File → New (`starterState(t('app.model.untitled'))`).
    pub fn empty() -> DetailView {
        let mut v = DetailView::new(Model::new(cc_detail::model::starter_state(&t0("app.model.untitled"))), "");
        v.message = t0("app.file.new");
        v
    }

    /// detailcraft.js `openFile` for a model file: `Model.load(text)` (untrusted: readModel checks everything).
    pub fn open_model(name: &str, bytes: &[u8]) -> Result<DetailView, String> {
        if bytes.len() > MAX_MODEL_BYTES {
            return Err(t0("app.file.too-big"));
        }
        let t1 = web_time::Instant::now();
        let text = String::from_utf8_lossy(bytes).into_owned();
        let mut m = Model::default();
        let warnings =
            m.load(&Value::String(text)).map_err(|e| t("app.file.open.error", &[("file", F::S(name.into())), ("error", F::S(e))]))?;
        let open = ms(t1);
        let mut v = DetailView::new(m, name);
        v.timings.insert(0, ("open", open));
        v.message = t("app.file.opened", &[("file", F::S(name.into()))]);
        if !warnings.is_empty() {
            v.message.push(' ');
            v.message.push_str(&warnings.join(" "));
        }
        Ok(v)
    }

    /// detailcraft.js `openIfc`: cc-ifc reads the file, ifc-import.js turns it into a model (not saved yet).
    pub fn import_ifc(name: &str, bytes: &[u8]) -> Result<DetailView, String> {
        if bytes.len() > MAX_IFC_BYTES {
            return Err(t0("app.ifc.too-big"));
        }
        let t1 = web_time::Instant::now();
        let base = name.strip_suffix(".ifc").or_else(|| name.strip_suffix(".IFC")).unwrap_or(name);
        let r = cc_io_detail::ifc_import::import_ifc(bytes, base)
            .map_err(|e| t("app.ifc.error", &[("file", F::S(name.into())), ("error", F::S(e))]))?;
        let state = r.get("state").cloned().unwrap_or(Value::Null);
        let report = r.get("report").cloned().unwrap_or(Value::Null);
        let open = ms(t1);
        let mut m = Model::new(state);
        m.version = 1; // an imported model is not saved yet
        let mut v = DetailView::new(m, name);
        v.timings.insert(0, ("open", open));
        let st = &v.model.state;
        let n = cc_detail::js::arr(st.get("members")).len();
        let levels = cc_detail::js::arr(st.get("levels")).len();
        let grids = cc_detail::js::arr(st.get("grids").and_then(|g| g.get("x"))).len()
            + cc_detail::js::arr(st.get("grids").and_then(|g| g.get("y"))).len();
        let other: f64 =
            cc_detail::js::arr(report.get("notConverted")).iter().map(|r| r.get("n").and_then(Value::as_f64).unwrap_or(0.0)).sum();
        v.message = t(
            "app.ifc.imported",
            &[
                ("file", F::S(name.into())),
                ("n", F::N(n as f64)),
                ("levels", F::N(levels as f64)),
                ("grids", F::N(grids as f64)),
                ("other", F::N(other)),
            ],
        );
        v.report = cc_io_detail::ifc_import::report_lines(&report);
        v.report.extend(cc_detail::js::arr(report.get("warnings")).iter().filter_map(|w| w.as_str().map(str::to_string)));
        Ok(v)
    }

    /// detailcraft.js `recompute()` when the model changed: build → marks → footing marks → bending schedule.
    pub fn recompute(&mut self) {
        if self.built_version == self.model.version {
            return;
        }
        let t1 = web_time::Instant::now();
        let st = &self.model.state;
        self.built = match cc_detail::build::build(st) {
            Ok(b) => b,
            Err(e) => {
                self.message = e;
                Built::default()
            }
        };
        let t2 = web_time::Instant::now();
        let per_phase = st.get("numbering").and_then(|n| n.get("perPhase")) != Some(&Value::Bool(false));
        self.marks = cc_detail::marks::mark_parts(&self.built, 1.0, per_phase);
        let footings = cc_detail::js::arr(st.get("footings"));
        self.footing_marks = cc_detail::marks::mark_footings(footings, per_phase);
        let typed: Vec<cc_detail::rebar::Footing> = footings.iter().map(cc_detail::rebar::Footing::from_value).collect();
        self.schedule = cc_detail::rebar::bending_schedule(&typed);
        let phases: Vec<f64> = cc_detail::js::arr(st.get("phases")).iter().filter_map(|p| p.get("n").and_then(Value::as_f64)).collect();
        self.built_version = self.model.version;
        self.set_timing("build", t2.duration_since(t1).as_secs_f64() * 1000.0);
        self.set_timing("marks", ms(t2));
        // detailcraft.js refresh(): drop a selection that is gone, phases that are gone
        if let Some(r) = &self.sel
            && !self.exists(r)
        {
            self.sel = None;
        }
        let multi = std::mem::take(&mut self.multi);
        self.multi = multi.into_iter().filter(|r| self.exists(r)).collect();
        if self.multi.len() == 1 {
            self.sel = self.multi.pop();
        }
        self.hidden_phases.retain(|n| phases.contains(n));
        if !phases.contains(&self.tools.cur_phase) {
            self.tools.cur_phase = phases.first().copied().unwrap_or(1.0);
        }
    }

    pub fn set_timing(&mut self, k: &'static str, v: f64) {
        match self.timings.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => self.timings.push((k, v)),
        }
    }

    pub fn timings_text(&self) -> String {
        let list: Vec<String> = self.timings.iter().map(|(k, v)| format!("{k} {v:.1}")).collect();
        format!("ms: {}", list.join(" · "))
    }

    fn list_has(&self, key: &str, id: &str) -> bool {
        cc_detail::js::arr(self.model.state.get(key)).iter().any(|x| x.get("id").and_then(Value::as_str) == Some(id))
    }

    /// detailcraft.js `exists(ref)`.
    pub fn exists(&self, r: &Ref) -> bool {
        match r {
            Ref::Member(i) => self.list_has("members", i),
            Ref::Conn(i) => self.list_has("connections", i),
            Ref::Stair(i) => self.list_has("stairs", i),
            Ref::Footing(i) => self.list_has("footings", i),
        }
    }

    pub fn member(&self, id: &str) -> Option<&Value> {
        cc_detail::js::arr(self.model.state.get("members")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id))
    }

    /// detailcraft.js `selectedPart()`.
    pub fn selected_part(&self) -> Option<&cc_detail::build::Part> {
        match self.sel.as_ref()? {
            Ref::Member(i) => self.built.get(i),
            Ref::Conn(i) => self.built.get(&format!("{i}.plate")),
            _ => None,
        }
    }

    /// detailcraft.js `describeRef(r)`.
    pub fn describe(&self, r: &Ref) -> String {
        let mm = |v: f64| cc_detail::js::num_to_string(cc_detail::js::round(v));
        match r {
            Ref::Member(i) => {
                let Some(m) = self.member(i) else { return String::new() };
                let ty = type_name(m.get("type").and_then(Value::as_str).unwrap_or(""));
                let profile = cc_detail::js::string_of(m.get("profile"));
                match self.marks.part_mark(i) {
                    Some(mk) => t(
                        "app.ref.member.marked",
                        &[
                            ("type", F::S(ty)),
                            ("profile", F::S(profile)),
                            ("part", F::S(mk.into())),
                            ("asm", F::S(self.marks.asm_mark(i).unwrap_or("").into())),
                        ],
                    ),
                    None => t("app.ref.member", &[("type", F::S(ty)), ("profile", F::S(profile))]),
                }
            }
            Ref::Conn(i) => {
                let c =
                    cc_detail::js::arr(self.model.state.get("connections")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(i));
                let ty = c.and_then(|c| c.get("type")).and_then(Value::as_str).unwrap_or("");
                t(&format!("app.ref.{ty}"), &[("id", F::S(i.clone()))])
            }
            Ref::Stair(i) => {
                let x = cc_detail::js::arr(self.model.state.get("stairs")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(i));
                let g = |k: &str| x.and_then(|x| x.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
                t(
                    "app.ref.stair",
                    &[
                        ("mark", F::S(self.marks.asm_mark(&format!("{i}.stringerL")).unwrap_or("").into())),
                        ("rise", F::S(mm(g("rise")))),
                        ("width", F::S(mm(g("width")))),
                    ],
                )
            }
            Ref::Footing(i) => {
                let f =
                    cc_detail::js::arr(self.model.state.get("footings")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(i));
                let g = |k: &str| f.and_then(|f| f.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let mark = self.footing_marks.iter().find(|(k, _)| k == i).map_or("", |(_, v)| v.as_str());
                t(
                    "app.ref.footing",
                    &[("mark", F::S(mark.into())), ("l", F::S(mm(g("L")))), ("b", F::S(mm(g("B")))), ("h", F::S(mm(g("H"))))],
                )
            }
        }
    }

    /// detailcraft.js `select(ref)`: that one only (or nothing), said in the status line.
    pub fn select(&mut self, r: Option<Ref>) {
        self.set_selection(r.into_iter().collect());
        if let Some(r) = self.sel.clone() {
            self.message = self.describe(&r);
        }
    }

    /// `detail.set`: one property of a member (or the model's name) through `Model::change`, one undo step.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let (path, value) = spec.split_once('=').ok_or("detail.set: name = \"<member id>.<field>=<value>\"")?;
        let (id, field) = path.split_once('.').ok_or("detail.set: <member id>.<field>=<value>")?;
        let (id, field, value) = (id.trim(), field.trim(), value.trim());
        if id == "model" && field == "name" {
            let v = Value::from(value);
            return self.change(&t0("app.undo.rename-model"), |tx| {
                tx.set_name(Some(&v));
                Ok(())
            });
        }
        if let Some(r) = self.set_other(id, field, value) {
            return r;
        }
        let m = self.member(id).cloned().ok_or_else(|| format!("detail.set: no member {id}"))?;
        let num = |s: &str| s.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| t0("app.num.bad"));
        let idv = Value::from(id);
        let upd = |this: &mut DetailView, label: &str, patch: Value| this.change(label, |tx| tx.update_member(Some(&idv), Some(&patch)));
        match field {
            "name" => upd(self, &t0("app.undo.rename"), json!({ "name": value })),
            "profile" => upd(self, &t0("app.undo.profile"), json!({ "profile": value })),
            "material" => upd(self, &t0("app.undo.material"), json!({ "material": value })),
            "position" => upd(self, &t0("app.undo.position"), json!({ "position": value })),
            "rotation" => upd(self, &t0("app.undo.rotate"), json!({ "rotation": num(value)? })),
            "thickness" | "width" => {
                let custom = cc_detail::js::arr(self.model.state.get("profiles")).to_vec();
                let pr = cc_detail::profiles::get_profile(m.get("profile").unwrap_or(&Value::Null), &custom);
                let (tk, w) = (pr.as_ref().and_then(|p| p.t).unwrap_or(20.0), pr.as_ref().and_then(|p| p.b).unwrap_or(200.0));
                let v = num(value)?;
                let name =
                    if field == "thickness" { cc_detail::profiles::plate_name(v, w) } else { cc_detail::profiles::plate_name(tk, v) };
                upd(self, &t0("app.undo.plate-size"), json!({ "profile": name }))
            }
            "start" | "end" => {
                let p: Vec<f64> = value.split(',').map(num).collect::<Result<_, _>>()?;
                if p.len() != 3 {
                    return Err("detail.set: start / end = \"x,y,z\"".into());
                }
                upd(self, &t0(&format!("app.undo.move.{field}")), json!({ field: p }))
            }
            "phase" => {
                let n = num(value)?;
                self.phase_of_one(Ref::Member(id.into()), n)
            }
            other => Err(format!("detail.set: unknown field {other}")),
        }
    }

    /// detailcraft.js `edit(label, fn)`: one undo step; a refused edit is said and nothing changes.
    fn change(&mut self, label: &str, f: impl FnOnce(&mut cc_detail::model::Tx) -> cc_detail::js::Res<()>) -> Result<(), String> {
        let r = self.model.change(label, f);
        self.recompute();
        r
    }

    pub fn undo(&mut self) {
        self.message = match self.model.undo() {
            Some(l) => t("app.undo.done", &[("label", F::S(l))]),
            None => t0("app.undo.nothing"),
        };
        self.recompute();
    }

    pub fn redo(&mut self) {
        self.message = match self.model.redo() {
            Some(l) => t("app.redo.done", &[("label", F::S(l))]),
            None => t0("app.redo.nothing"),
        };
        self.recompute();
    }

    /// detailcraft.js `runClashes()` on the shown phases.
    pub fn run_clashes(&mut self) {
        self.recompute();
        let t1 = web_time::Instant::now();
        let phases: Vec<f64> = cc_detail::js::arr(self.model.state.get("phases"))
            .iter()
            .filter_map(|p| p.get("n").and_then(Value::as_f64))
            .filter(|n| !self.hidden_phases.contains(n))
            .collect();
        let (clashes, _pairs, checked) = cc_detail::clash::clash_check(&self.model.state, &self.built, self.clash_tol, Some(&phases));
        let took = ms(t1);
        let n = clashes.len();
        self.message = t(
            if n > 0 { "clash.found" } else { "clash.none" },
            &[("n", F::N(n as f64)), ("parts", F::N(checked as f64)), ("ms", F::N(took.round()))],
        );
        self.clash = Some(ClashRes { clashes, checked, version: self.model.version, hidden: self.hidden_phases.len(), ms: took });
        self.tab = Tab::Clashes;
    }

    /// detailcraft.js `setSetting(k, v, say)`: what a changed setting does and says.
    pub fn setting_changed(&mut self, before: &Settings) {
        let s = &self.settings;
        if s.steel != before.steel {
            self.tools.opts.apply_steel(&s.steel);
            self.message = t0(if s.steel == "eu" { "app.set.steel.eu" } else { "app.set.steel.us" });
        } else if s.offset != before.offset {
            let f = |v: f64| F::S(cc_detail::js::num_to_string(v));
            self.message = t("app.set.offset.done", &[("x", f(s.offset[0])), ("y", f(s.offset[1])), ("z", f(s.offset[2]))]);
        } else if s.conns != before.conns {
            self.message = t0(if s.conns { "app.set.conns.on" } else { "app.set.conns.off" });
        } else if s.bolts != before.bolts {
            self.message = t0(if s.bolts { "app.set.bolts.on" } else { "app.set.bolts.off" });
        } else if s.paper != before.paper {
            self.message = t("dwg.paper.set", &[("paper", F::S(cc_detail::paper::paper_label(&s.paper)))]);
        }
    }

    /// The export context (what `recompute` made).
    pub fn ctx(&self) -> detailexport::Ctx<'_> {
        detailexport::Ctx {
            state: &self.model.state,
            built: &self.built,
            marks: &self.marks,
            footing_marks: &self.footing_marks,
            schedule: &self.schedule,
            settings: &self.settings,
            part: self.selected_part().map(|p| p.id.clone()),
        }
    }

    /// Make the scene again when the model, the hidden phases or the bolts setting changed; paint the selection.
    pub fn sync_scene(&mut self) {
        self.recompute();
        let key = (self.model.version, self.hidden_phases.iter().map(|p| p.to_bits()).collect::<Vec<u64>>(), self.settings.bolts);
        if self.scene_key.as_ref() != Some(&key) {
            let t1 = web_time::Instant::now();
            self.seq = self.seq.wrapping_add(1);
            let id = (1u64 << 42) + self.seq;
            let first = self.scene.is_none();
            self.scene = Some(detail3d::build(&self.model.state, &self.built, &self.marks, id, &self.hidden_phases, self.settings.bolts));
            if first {
                self.view.fit_pending = true;
            }
            self.scene_key = Some(key);
            self.sel_shown = None;
            self.set_timing("scene", ms(t1));
        }
        let vis = (self.shown_selection(), self.mode, self.vlevel.clone(), self.vgrid.clone());
        if self.sel_shown.as_ref() != Some(&vis) {
            let cut = self.hide_band();
            if let Some(s) = self.scene.as_mut() {
                let refs = s.refs.clone();
                let sel = vis.0.clone();
                s.scene.set_tints(&|i| match refs.get(i) {
                    Some(Some(r)) if sel.contains(r) => Some(detail3d::palette::PICK),
                    _ => None,
                });
                let boxes = s.boxes.clone();
                s.scene.set_hidden(&|i| match (boxes.get(i), cut) {
                    (Some(Some((lo, hi))), Some((axis, a, b))) => {
                        let (l, h) = (lo.get(axis).copied().unwrap_or(0.0), hi.get(axis).copied().unwrap_or(0.0));
                        h < a || l > b
                    }
                    _ => false,
                });
            }
            self.sel_shown = Some(vis);
        }
    }

    /// The plan's level (detailcraft.js setView: the chosen one, else the top level): (name, z).
    pub fn plan_level(&self) -> Option<(String, f64)> {
        let lv = cc_detail::js::arr(self.model.state.get("levels"));
        let l = lv.iter().find(|l| l.get("name").and_then(Value::as_str) == Some(self.vlevel.as_str())).or(lv.last())?;
        Some((cc_detail::js::string_of(l.get("name")), cc_detail::js::num(l.get("z"))))
    }
    /// The elevation's lettered grid line (the chosen one, else the first): (label, at).
    pub fn elev_grid(&self) -> Option<(String, f64)> {
        let gy = cc_detail::js::arr(self.model.state.get("grids").and_then(|g| g.get("y")));
        let g = gy.iter().find(|g| g.get("label").and_then(Value::as_str) == Some(self.vgrid.as_str())).or(gy.first())?;
        Some((cc_detail::js::string_of(g.get("label")), cc_detail::js::num(g.get("at"))))
    }
    /// What a plan or an elevation leaves out (metres): (axis, from, to) — a plan keeps up to 1.5 m above its level, an
    /// elevation 1 m either side of its grid line (view3d.js `applyClipping`).
    fn hide_band(&self) -> Option<(usize, f64, f64)> {
        match self.mode {
            ViewMode::D3 => None,
            ViewMode::Plan => self.plan_level().map(|(_, z)| (2, f64::NEG_INFINITY, (z + 1500.0) * 0.001)),
            ViewMode::Elevation => self.elev_grid().map(|(_, y)| (1, (y - 1000.0) * 0.001, (y + 1000.0) * 0.001)),
        }
    }
    /// The view's cut as winsel.js planes (view3d.js `cutPlanes()`).
    pub fn cut_planes(&self) -> Vec<cc_detail::winsel::Plane> {
        match self.mode {
            ViewMode::D3 => Vec::new(),
            ViewMode::Plan => {
                self.plan_level().map(|(_, z)| vec![cc_detail::winsel::Plane { n: [0.0, 0.0, -1.0], d: z + 1500.0 }]).unwrap_or_default()
            }
            ViewMode::Elevation => self
                .elev_grid()
                .map(|(_, y)| {
                    vec![
                        cc_detail::winsel::Plane { n: [0.0, -1.0, 0.0], d: y + 1000.0 },
                        cc_detail::winsel::Plane { n: [0.0, 1.0, 0.0], d: -(y - 1000.0) },
                    ]
                })
                .unwrap_or_default(),
        }
    }
    /// Grid intersections that can be picked in this view (view3d.js `activeNodes()`).
    pub fn active_nodes(&self) -> Vec<crate::detailtools::Node> {
        let all = crate::detailtools::all_nodes(&self.model.state);
        match (self.mode, self.plan_level(), self.elev_grid()) {
            (ViewMode::Plan, Some((_, z)), _) => all.into_iter().filter(|n| (n.p[2] - z).abs() < 0.5).collect(),
            (ViewMode::Elevation, _, Some((_, y))) => all.into_iter().filter(|n| (n.p[1] - y).abs() < 0.5).collect(),
            _ => all,
        }
    }

    /// `detail.view`: "3d", "plan[:<level>]" or "elevation[:<grid line>]" (detailcraft.js `setView`).
    pub fn set_view(&mut self, spec: &str) -> Result<(), String> {
        let (m, which) = spec.split_once(':').unwrap_or((spec, ""));
        self.mode = match m {
            "3d" => ViewMode::D3,
            "plan" => ViewMode::Plan,
            "elevation" => ViewMode::Elevation,
            o => return Err(format!("detail.view: 3d | plan[:level] | elevation[:grid], not {o}")),
        };
        if !which.is_empty() {
            match self.mode {
                ViewMode::Plan => self.vlevel = which.into(),
                ViewMode::Elevation => self.vgrid = which.into(),
                ViewMode::D3 => {}
            }
        }
        if let Some((l, _)) = self.plan_level()
            && self.mode == ViewMode::Plan
        {
            self.vlevel = l;
        }
        if let Some((g, _)) = self.elev_grid()
            && self.mode == ViewMode::Elevation
        {
            self.vgrid = g;
        }
        self.view.cam.persp = self.mode == ViewMode::D3;
        self.view.lock_orbit = self.mode != ViewMode::D3;
        self.view.style.section = match self.mode {
            ViewMode::D3 => None,
            ViewMode::Plan => self.plan_level().map(|(_, z)| ([0.0, 0.0, 1.0], (z + 1500.0) * 0.001)),
            ViewMode::Elevation => self.elev_grid().map(|(_, y)| ([0.0, -1.0, 0.0], -(y - 1000.0) * 0.001)),
        };
        self.win = None;
        self.fit();
        self.fit_now();
        Ok(())
    }

    pub fn fit(&mut self) {
        self.view.fit_pending = true;
    }

    /// view3d.js `fit()`: the whole model; a plan looks straight down on its level, an elevation north at its line.
    pub fn fit_now(&mut self) {
        self.sync_scene();
        let Some(s) = &self.scene else { return };
        let (mut lo, mut hi) = (s.scene.min, s.scene.max);
        let dir = match self.mode {
            ViewMode::D3 => None,
            ViewMode::Plan => {
                let z = self.plan_level().map_or(0.0, |l| l.1 * 0.001);
                lo[2] = z;
                hi[2] = z;
                Some([0.0, 0.0, 1.0])
            }
            ViewMode::Elevation => {
                let y = self.elev_grid().map_or((lo[1] + hi[1]) / 2.0, |g| g.1 * 0.001);
                lo[1] = y;
                hi[1] = y;
                Some([0.0, -1.0, 0.0])
            }
        };
        if self.view.cam.width > 0.0 {
            self.view.cam.fit(lo, hi, dir);
            self.view.fit_pending = false;
        }
    }

    /// view3d.js `focusPoint(p, span)`: a model point (mm) to the middle of the view, about `span` mm across.
    pub fn focus(&mut self, at: [f64; 3], span: f64) {
        let h = span / 2.0 * 0.001;
        let c = [at[0] * 0.001, at[1] * 0.001, at[2] * 0.001];
        if c.iter().all(|v| v.is_finite()) {
            let dir = match self.mode {
                ViewMode::D3 => None,
                ViewMode::Plan => Some([0.0, 0.0, 1.0]),
                ViewMode::Elevation => Some([0.0, -1.0, 0.0]),
            };
            self.view.cam.fit([c[0] - h, c[1] - h, c[2] - h], [c[0] + h, c[1] + h, c[2] + h], dir);
            self.view.fit_pending = false;
        }
    }

    pub fn camera(&mut self, name: &str) -> Result<(), String> {
        let dir = match name {
            "top" => [0.0, -1e-4, 1.0],
            "front" => [0.0, -1.0, 0.2],
            "back" => [0.0, 1.0, 0.2],
            "left" => [-1.0, 0.0, 0.2],
            "right" => [1.0, 0.0, 0.2],
            "iso" | "" => [0.9, -1.2, 0.8],
            o => return Err(format!("detail.camera: unknown view {o}")),
        };
        if self.mode != ViewMode::D3 {
            self.set_view("3d")?;
        }
        self.sync_scene();
        if let Some(s) = &self.scene {
            self.view.cam.fit(s.scene.min, s.scene.max, Some(dir));
            self.view.fit_pending = false;
        }
        Ok(())
    }

    /// A click at (sx, sy) physical pixels of the view: the nearest part or footing (view3d.js `pickRef`); what a
    /// plan or an elevation leaves out is not picked.
    pub fn pick_at(&mut self, sx: f64, sy: f64) -> Option<Ref> {
        let s = self.scene.as_ref()?;
        let hit = crate::view3d::pick(&s.scene, &self.view.cam, sx, sy);
        let part = hit.and_then(|h| s.refs.get(h.instance).cloned().flatten().map(|r| (r, h.t)));
        let (o, d) = self.view.cam.ray(sx, sy);
        let band = self.hide_band();
        let foot_list: Vec<detail3d::FootBox> = s
            .footings
            .iter()
            .filter(|f| match band {
                Some((axis, a, b)) => {
                    let (c, h) = (f.c.get(axis).copied().unwrap_or(0.0), f.half.get(axis).copied().unwrap_or(0.0));
                    !(c + h < a || c - h > b)
                }
                None => true,
            })
            .cloned()
            .collect();
        let foot = detail3d::pick_footing(&foot_list, o, d);
        match (part, foot) {
            (Some(a), Some(b)) => Some(if a.1 <= b.1 { a.0 } else { b.0 }),
            (Some(a), None) => Some(a.0),
            (None, Some(b)) => Some(b.0),
            (None, None) => None,
        }
    }

    /// The status line: the last message, then the counts (detailcraft.js `renderBar` counts).
    pub fn status(&self) -> String {
        let steel: f64 = self.built.parts.iter().fold(0.0, |s, p| s + p.weight);
        let bars: f64 = self.schedule.rows.iter().fold(0.0, |s, r| s + r.total_weight);
        let mut counts = t(
            "app.bar.counts",
            &[
                ("parts", F::N(self.built.parts.len() as f64)),
                ("bolts", F::N(self.built.bolts.len() as f64)),
                ("steel", F::S(format!("{steel:.1}"))),
            ],
        );
        if bars > 0.0 {
            counts.push(' ');
            counts.push_str(&t("app.bar.counts.rebar", &[("rebar", F::S(format!("{bars:.1}")))]));
        }
        if self.message.is_empty() { counts } else { format!("{}   · {counts}", self.message) }
    }

    /// A model point (mm) → view points (winsel.js's `project`), behind the eye flagged.
    pub fn screen_of(&self, p: [f64; 3], ppp: f64) -> cc_detail::transform::Screen {
        match project(&self.view.cam, [p[0] * 0.001, p[1] * 0.001, p[2] * 0.001]) {
            Some((x, y)) => cc_detail::transform::Screen { x: x / ppp, y: y / ppp, behind: false },
            None => cc_detail::transform::Screen { x: f64::NAN, y: f64::NAN, behind: true },
        }
    }

    /// view3d.js `nearestNode(px, py)`: the active grid intersection within 18 points of a view point.
    pub fn nearest_node(&self, at: egui::Vec2, ppp: f64) -> Option<crate::detailtools::Node> {
        let mut best: Option<(f64, crate::detailtools::Node)> = None;
        for n in self.active_nodes() {
            let s = self.screen_of(n.p, ppp);
            if s.behind {
                continue;
            }
            let d = (s.x - f64::from(at.x)).hypot(s.y - f64::from(at.y));
            if d < best.as_ref().map_or(SNAP_PX, |b| b.0) {
                best = Some((d, n));
            }
        }
        best.map(|b| b.1)
    }

    /// The end of a selection window or lasso (view3d.js `endWindow`): what it took, through winsel.js.
    pub fn end_window(&mut self, w: &Win, ppp: f64) {
        if !w.moved {
            return;
        }
        let pt = |p: &egui::Pos2| cc_detail::lasso_core::Pt { x: f64::from(p.x), y: f64::from(p.y) };
        let path: Vec<cc_detail::lasso_core::Pt> = if w.lasso {
            cc_detail::lasso_core::thin(&w.path.iter().map(pt).collect::<Vec<_>>(), 2.0)
        } else {
            vec![pt(&w.start), pt(&egui::pos2(w.cur.x, w.start.y)), pt(&w.cur), pt(&egui::pos2(w.start.x, w.cur.y))]
        };
        let planes = self.cut_planes();
        let cam = self.view.cam;
        let proj = move |p: [f64; 3]| match project(&cam, [p[0] * 0.001, p[1] * 0.001, p[2] * 0.001]) {
            Some((x, y)) => cc_detail::transform::Screen { x: x / ppp, y: y / ppp, behind: false },
            None => cc_detail::transform::Screen { x: f64::NAN, y: f64::NAN, behind: true },
        };
        self.window_picked(&proj, &path, w.mode.unwrap_or("window"), w.add, &planes);
    }

    /// `detail.window` / `detail.lasso`: a window "x1,y1:x2,y2" or a lasso "x,y;x,y;…" in view points, or a window
    /// "plan:x1,y1:x2,y2" between two model points (mm) on the plan's level; a leading "+" adds (Shift).
    pub fn window_cmd(&mut self, spec: &str, lasso: bool, ppp: f64) -> Result<(), String> {
        let (spec, add) = match spec.strip_prefix('+') {
            Some(s) => (s, true),
            None => (spec, false),
        };
        let xy = |s: &str| -> Option<egui::Pos2> {
            let (a, b) = s.split_once(',')?;
            let (x, y) = (str_to_number(a.trim()), str_to_number(b.trim()));
            (x.is_finite() && y.is_finite()).then(|| egui::pos2(x as f32, y as f32))
        };
        let bad = || format!("detail.{}: bad points {spec}", if lasso { "lasso" } else { "window" });
        let mut pts: Vec<egui::Pos2> = Vec::new();
        if let Some(world) = spec.strip_prefix("plan:") {
            if self.mode != ViewMode::Plan {
                self.set_view("plan")?;
            }
            let z = self.plan_level().map_or(0.0, |l| l.1);
            for p in world.split(':') {
                let q = xy(p).ok_or_else(bad)?;
                let s = self.screen_of([f64::from(q.x), f64::from(q.y), z], ppp);
                pts.push(egui::pos2(s.x as f32, s.y as f32));
            }
        } else {
            for p in spec.split(if lasso { ';' } else { ':' }) {
                pts.push(xy(p).ok_or_else(bad)?);
            }
        }
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return Err(bad()) };
        if pts.len() < 2 {
            return Err(bad());
        }
        let pt = |p: egui::Pos2| cc_detail::lasso_core::Pt { x: f64::from(p.x), y: f64::from(p.y) };
        let mode = cc_detail::lasso_core::lasso_mode(pt(a), pt(b), if lasso { 6.0 } else { 1.0 }).unwrap_or("window");
        let w = Win { start: a, cur: b, mode: Some(mode), add, moved: true, lasso, path: pts, fixed: false };
        self.end_window(&w, ppp);
        Ok(())
    }

    /// Show the 3D view in `rect`: left drag orbits (3D) or draws a selection window (plan, elevation; Shift+drag in
    /// 3D), a click selects or places on the grid intersection under the pointer.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens, dark: bool) {
        self.sync_scene();
        let c = t.canvas_bg;
        self.view.style.background = [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, 1.0];
        self.view.style.edge = if dark { [0.78, 0.8, 0.84, 1.0] } else { [0.09, 0.13, 0.17, 1.0] };
        self.view.style.back = if dark { crate::model3d::BACK_MODEL } else { crate::model3d::BACK_PAPER };
        let ppp = f64::from(ui.ctx().pixels_per_point());
        self.ppp = ppp;
        self.view.cam.width = f64::from(rect.width()) * ppp;
        self.view.cam.height = f64::from(rect.height()) * ppp;
        if self.view.fit_pending && self.mode != ViewMode::D3 {
            self.fit_now();
        }
        let Some(scene) = self.scene.as_ref() else { return };
        let scene3 = scene.scene.clone();
        let resp = crate::view3d::show(ui, rect, &scene3, &mut self.view);
        let placing = self.tools.tool != "select";
        let mods = ui.input(|i| i.modifiers);
        let hover = resp.hover_pos().map(|p| p - rect.min);
        self.tools.hover = if placing { hover.and_then(|p| self.nearest_node(p, ppp)) } else { None };
        if resp.drag_started_by(egui::PointerButton::Primary) && !placing && (self.mode != ViewMode::D3 || mods.shift) {
            let origin = ui.input(|i| i.pointer.press_origin()).map_or(egui::Vec2::ZERO, |p| p - rect.min);
            let start = origin.to_pos2();
            self.win = Some(Win {
                start,
                cur: start,
                mode: None,
                add: mods.shift,
                moved: false,
                lasso: self.settings.drag_sel == "lasso",
                path: vec![start],
                fixed: false,
            });
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            let (dx, dy) = (f64::from(d.x) * ppp, f64::from(d.y) * ppp);
            if let Some(w) = self.win.as_mut() {
                if let Some(p) = hover {
                    let p = p.to_pos2();
                    w.cur = p;
                    if !w.moved && (p - w.start).length() > 5.0 {
                        w.moved = true;
                    }
                    let pt = |p: egui::Pos2| cc_detail::lasso_core::Pt { x: f64::from(p.x), y: f64::from(p.y) };
                    if w.lasso {
                        w.path.push(p);
                        if w.moved && !w.fixed {
                            w.mode = w.mode.or_else(|| cc_detail::lasso_core::lasso_mode(pt(w.start), pt(w.cur), 6.0));
                        }
                    } else if w.moved {
                        w.mode = cc_detail::lasso_core::lasso_mode(pt(w.start), pt(w.cur), 1.0).or(w.mode).or(Some("window"));
                    }
                }
            } else if self.mode == ViewMode::D3 && !mods.shift {
                self.view.cam.orbit(dx, dy);
            } else {
                self.view.cam.pan(dx, dy);
            }
        }
        if let Some(w) = self.win.as_mut()
            && w.lasso
            && w.moved
            && ui.input(|i| i.key_pressed(egui::Key::Space))
        {
            // view3d.js: Space while lassoing steps window → crossing → fence
            w.mode = Some(cc_detail::lasso_core::next_mode(w.mode.unwrap_or("window")));
            w.fixed = true;
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary)
            && let Some(w) = self.win.take()
        {
            self.end_window(&w, ppp);
            ui.ctx().request_repaint();
        }
        if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let at = p - rect.min;
            if placing {
                if let Some(n) = self.nearest_node(at, ppp) {
                    self.place_at(&n);
                }
            } else {
                let r = self.pick_at(f64::from(at.x) * ppp, f64::from(at.y) * ppp);
                self.pick(r, mods.shift || mods.command);
            }
            ui.ctx().request_repaint();
        }
        self.overlays(ui, rect, t, ppp);
    }

    /// Grid bubbles and level names, the placing marker, rubber band and ghost, the selection window.
    fn overlays(&self, ui: &egui::Ui, rect: Rect, t: &Tokens, ppp: f64) {
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(12.0);
        let at_pt = |p_mm: [f64; 3]| -> Option<egui::Pos2> {
            let s = self.screen_of(p_mm, ppp);
            (!s.behind && s.x.is_finite()).then(|| egui::pos2(rect.min.x + s.x as f32, rect.min.y + s.y as f32))
        };
        if let Some(scene) = self.scene.as_ref() {
            let plan_z = self.plan_level().map(|l| l.1);
            for l in &scene.labels {
                // a plan names only its own level; an elevation looks along the lettered lines
                let skip = match (self.mode, l.axis) {
                    (ViewMode::Plan, 'l') => plan_z.is_none_or(|z| (z - l.z).abs() > 0.5),
                    (ViewMode::Elevation, 'y') => true,
                    _ => false,
                };
                if skip {
                    continue;
                }
                let Some((x, y)) = project(&self.view.cam, l.at) else { continue };
                let at = egui::pos2(rect.min.x + (x / ppp) as f32, rect.min.y + (y / ppp) as f32);
                if !rect.contains(at) {
                    continue;
                }
                if l.bubble {
                    painter.circle(at, 10.0, t.panel, egui::Stroke::new(1.5, t.text_dim));
                    painter.text(at, egui::Align2::CENTER_CENTER, &l.text, egui::FontId::proportional(11.0), t.text_strong);
                } else {
                    painter.text(at, egui::Align2::LEFT_CENTER, &l.text, font.clone(), t.text_dim);
                }
            }
        }
        let pick = egui::Color32::from_rgb(detail3d::palette::PICK[0], detail3d::palette::PICK[1], detail3d::palette::PICK[2]);
        let stroke = egui::Stroke::new(1.6, pick);
        if self.tools.tool != "select" {
            let hover = self.tools.hover.as_ref();
            if let (Some(from), Some(to)) = (self.tools.pending.as_ref(), hover)
                && let (Some(a), Some(b)) = (at_pt(from.p), at_pt(to.p))
            {
                painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 7.0, 4.0));
            }
            if let Some(from) = self.tools.pending.as_ref()
                && let Some(a) = at_pt(from.p)
            {
                painter.circle_stroke(a, 5.0, stroke);
            }
            // a ghost of what a paste or a move puts down, at the hovered intersection
            let ghost: Vec<[[f64; 3]; 2]> = match (self.tools.tool, &self.tools.paste_clip, &self.tools.pending, &self.tools.move_refs) {
                ("paste", Some(c), _, _) => cc_detail::clipboard::clip_segments(c),
                ("move", _, Some(from), Some(refs)) => {
                    let rj: Vec<Value> = refs.iter().map(crate::detailtools::ref_json).collect();
                    cc_detail::transform::ghost_lines(&self.model.state, &rj, from.p).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            if let Some(h) = hover {
                for [a, b] in ghost.iter().take(4000) {
                    let (pa, pb) = ([h.p[0] + a[0], h.p[1] + a[1], h.p[2] + a[2]], [h.p[0] + b[0], h.p[1] + b[1], h.p[2] + b[2]]);
                    if let (Some(a), Some(b)) = (at_pt(pa), at_pt(pb)) {
                        painter.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.2, pick), 6.0, 4.0));
                    }
                }
                if let Some(p) = at_pt(h.p) {
                    painter.circle(p, 6.0, pick.gamma_multiply(0.35), stroke);
                }
                let name = cc_detail::model::node_name(&self.model.state, h.p);
                painter.text(
                    egui::pos2(rect.min.x + 10.0, rect.max.y - 10.0),
                    egui::Align2::LEFT_BOTTOM,
                    name,
                    egui::FontId::monospace(12.0),
                    t.text_strong,
                );
            }
        }
        if let Some(w) = &self.win
            && w.moved
        {
            let off = rect.min.to_vec2();
            let crossing = w.mode != Some("window");
            if w.lasso {
                let pts: Vec<egui::Pos2> = w.path.iter().map(|p| *p + off).collect();
                if w.mode == Some("fence") {
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, pick)));
                } else {
                    let mut closed = pts.clone();
                    if let Some(f) = pts.first() {
                        closed.push(*f);
                    }
                    if crossing {
                        painter.extend(egui::Shape::dashed_line(&closed, stroke, 6.0, 4.0));
                    } else {
                        painter.add(egui::Shape::line(closed, stroke));
                    }
                }
            } else {
                let r = Rect::from_two_pos(w.start + off, w.cur + off);
                painter.rect_filled(r, 0.0, pick.gamma_multiply(if crossing { 0.06 } else { 0.12 }));
                if crossing {
                    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    painter.extend(egui::Shape::dashed_line(&c, stroke, 6.0, 4.0));
                } else {
                    painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
                }
            }
        }
        if self.model.state.get("members").and_then(Value::as_array).is_some_and(Vec::is_empty)
            && self.model.state.get("footings").and_then(Value::as_array).is_some_and(Vec::is_empty)
            && self.tools.tool == "select"
        {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, t0("page.hint.start"), egui::FontId::proportional(18.0), t.text_dim);
        }
    }
}

/// A world point (metres) → physical pixels of the camera's drawing area (None behind the eye).
pub fn project(cam: &cc_render::mesh3d::Cam3, p: [f64; 3]) -> Option<(f64, f64)> {
    let v = [p[0] - cam.eye[0], p[1] - cam.eye[1], p[2] - cam.eye[2]];
    let r = cam.view_rot();
    let e = [r[0] * v[0] + r[4] * v[1] + r[8] * v[2], r[1] * v[0] + r[5] * v[1] + r[9] * v[2], r[2] * v[0] + r[6] * v[1] + r[10] * v[2]];
    let m = cam.proj();
    let cx = m[0] * e[0] + m[4] * e[1] + m[8] * e[2] + m[12];
    let cy = m[1] * e[0] + m[5] * e[1] + m[9] * e[2] + m[13];
    let cw = m[3] * e[0] + m[7] * e[1] + m[11] * e[2] + m[15];
    if cw <= 1e-12 {
        return None;
    }
    let (nx, ny) = (cx / cw, cy / cw);
    Some(((nx + 1.0) / 2.0 * cam.width, (1.0 - ny) / 2.0 * cam.height))
}

// ------------------------------------------------------------------------------------------------ app hooks

/// Show a DetailCraft model in the app (any other document closes).
pub fn install(app: &mut UiApp, dv: DetailView) {
    app.doc = None;
    app.editor = None;
    app.source = None;
    app.model3 = None;
    app.mc = None;
    app.scene3 = None;
    app.pdf = None;
    app.civil = None;
    app.part = None;
    app.error = None;
    app.message = dv.message.clone();
    app.detail = Some(dv);
}

/// Is this a DetailCraft model file? (`.detailcraft.json`, or JSON whose format is "detailcraft")
pub fn is_detail_file(lower: &str, bytes: &[u8]) -> bool {
    if lower.ends_with(".detailcraft.json") {
        return true;
    }
    if !lower.ends_with(".json") {
        return false;
    }
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or(&[])).replace([' ', '\n', '\r', '\t'], "");
    head.trim_start().starts_with('{') && head.contains("\"format\":\"detailcraft\"")
}

/// The open-file hook (lib.rs `open_file`): a DetailCraft model file, or an IFC while a DetailCraft model is open,
/// opens here (true); anything else closes the DetailCraft view (false) and opens as before.
pub fn open_file(app: &mut UiApp, f: &ImportedFile) -> bool {
    let lower = f.name.to_ascii_lowercase();
    let r = if is_detail_file(&lower, &f.bytes) {
        DetailView::open_model(&f.name, &f.bytes)
    } else if lower.ends_with(".ifc") && app.detail.is_some() {
        DetailView::import_ifc(&f.name, &f.bytes)
    } else {
        app.detail = None;
        return false;
    };
    match r {
        Ok(dv) => install(app, dv),
        Err(e) => match app.detail.as_mut() {
            Some(d) => d.message = e,
            None => app.error = Some(e),
        },
    }
    true
}

/// Before any command (lib.rs `run`): commands that open another document close the DetailCraft view.
pub fn before_run(app: &mut UiApp, id: &str) {
    if matches!(
        id,
        "file.sample"
            | "file.new"
            | "view.model.sample"
            | "view.demo3d"
            | "civil.sample"
            | "civil.demo"
            | "civil.synthetic"
            | "part.sample"
    ) {
        app.detail = None;
    }
}

/// The wall clock in ms (what date-stamped exports use unless a test fixes it).
fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// The `detail.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    // the zone of the exports' local-time fields (`app.tz`, else the machine's)
    let zone = app.zone;
    match id {
        "detail.sample" => {
            install(app, DetailView::sample()?);
            return Ok(());
        }
        "detail.new" => {
            install(app, DetailView::empty());
            return Ok(());
        }
        _ => {}
    }
    let dv = app.detail.as_mut().ok_or_else(|| format!("{id}: open a DetailCraft model first (detail.sample)"))?;
    match id {
        "detail.select" => {
            if name.is_empty() {
                dv.select(None);
            } else if let Some(one) = name.strip_prefix('+') {
                // Shift+click: add it or take it away
                let r = Ref::parse(one).ok_or_else(|| format!("detail.select: kind:id expected, not {one}"))?;
                if !dv.exists(&r) {
                    return Err(format!("detail.select: no {one}"));
                }
                if dv.tools.tool != "select" {
                    dv.set_tool("select")?;
                }
                dv.pick(Some(r), true);
            } else {
                let mut refs = Vec::new();
                for k in name.split(',') {
                    let r = Ref::parse(k).ok_or_else(|| format!("detail.select: kind:id expected, not {k}"))?;
                    if !dv.exists(&r) {
                        return Err(format!("detail.select: no {k}"));
                    }
                    refs.push(r);
                }
                if refs.len() == 1 {
                    dv.select(refs.pop());
                } else {
                    let n = refs.len();
                    dv.set_selection(refs);
                    dv.message = t("app.sel.many", &[("n", F::N(n as f64))]);
                }
            }
        }
        "detail.tool" => dv.set_tool(&name)?,
        "detail.node" => {
            let n = crate::detailtools::node_at(&dv.model.state, &name)?;
            dv.place_at(&n);
        }
        "detail.opt" => {
            // a number that does not read is said, as numIn does (not an error of the command)
            if let Err(e) = dv.tools.opts.set(&name) {
                dv.message = e;
            }
        }
        "detail.bygrid" => dv.by_grid(&name)?,
        "detail.act" => dv.act(&name),
        "detail.paste" => dv.paste_form(&name)?,
        "detail.xf" => dv.xf_form(&name)?,
        "detail.many" => dv.many(&name)?,
        "detail.grids" => dv.set_grids(&name)?,
        "detail.levels" => dv.set_levels(&name),
        "detail.phases" => dv.phase_op(&name)?,
        "detail.hole" => dv.hole_op(&name)?,
        "detail.cope" => dv.cope_op(&name)?,
        "detail.profile" => dv.add_profile(&name)?,
        "detail.view" => dv.set_view(&name)?,
        "detail.window" => {
            let ppp = dv.ppp;
            dv.window_cmd(&name, false, ppp)?;
        }
        "detail.lasso" => {
            let ppp = dv.ppp;
            dv.window_cmd(&name, true, ppp)?;
        }
        "detail.settings" => dv.settings_open = name != "close",

        "detail.set" => {
            if let Err(e) = dv.set(&name) {
                dv.message = e.clone();
                // a number that does not read is said, as numIn does; anything else is a bad command
                if e != t0("app.num.bad") {
                    return Err(e);
                }
            }
        }
        "detail.undo" => dv.undo(),
        "detail.redo" => dv.redo(),
        "detail.setting" => {
            let before = dv.settings.clone();
            if let Err(e) = dv.settings.set(&name) {
                dv.message = e.clone();
                return Err(e);
            }
            dv.setting_changed(&before);
        }
        "detail.clash" => {
            if let Some(f) = params.factor {
                dv.clash_tol = f.clamp(0.0, 50.0);
            }
            dv.run_clashes();
        }
        "detail.phase" => {
            let n = params.factor.or_else(|| name.parse().ok()).ok_or("detail.phase: factor = phase number")?;
            let hidden = dv.hidden_phases.contains(&n);
            dv.set_phase_shown(n, hidden);
        }
        "detail.tab" => dv.tab = Tab::parse(&name).ok_or_else(|| format!("detail.tab: unknown tab {name}"))?,
        "detail.camera" => dv.camera(&name)?,
        "detail.timings" => {
            dv.sync_scene();
            dv.message = dv.timings_text();
        }
        "detail.save" | "detail.export" => {
            let spec = if id == "detail.save" { "project".to_string() } else { name };
            let (spec, at) = match spec.rsplit_once('@') {
                Some((s, t)) => (s.to_string(), Some(t.trim().parse::<f64>().map_err(|_| "detail.export: @<time ms>")?)),
                None => (spec, None),
            };
            dv.recompute();
            let t1 = web_time::Instant::now();
            let out = detailexport::run(&dv.ctx(), &spec, at.unwrap_or_else(now_ms), at.is_some(), zone)?;
            let took = ms(t1);
            dv.set_timing("export", took);
            match out {
                Outcome::Nothing(m) => dv.message = m,
                Outcome::File(fname, bytes, msg) => {
                    let project = spec.trim() == "project";
                    let save = app.services.save.as_mut().ok_or("detail.export: saving is not available here")?;
                    if let Some(to) = save(&fname, bytes)?
                        && let Some(dv) = app.detail.as_mut()
                    {
                        if project {
                            dv.model.mark_saved();
                            dv.message = t("file.save.written", &[("file", F::S(to))]);
                        } else {
                            dv.message = msg;
                        }
                    }
                }
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// detailcraft.js's keys: Ctrl+Z / Y / S / C / X / V / D / A, Ctrl+Shift+V (paste in place), Esc, Delete and the tool
/// letters (V B C R P F S M); none while typing in a field.
fn keys(dv: &mut DetailView, ui: &egui::Ui, acts: &mut Vec<crate::detailpanel::Act>) {
    use crate::detailpanel::Act;
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let events = ui.input(|i| i.events.clone());
    let run = |acts: &mut Vec<Act>, id: &'static str, name: &str| acts.push(Act::Run(id, name.into()));
    for e in events {
        match e {
            egui::Event::Copy if !typing => run(acts, "detail.act", "copy"),
            egui::Event::Cut if !typing => run(acts, "detail.act", "cut"),
            egui::Event::Paste(text) if !typing => {
                // a copy from another DetailCraft window reads back as untrusted as a file (clipboard.js readClip)
                if let Ok(c) = cc_detail::clipboard::read_clip(&Value::String(text)) {
                    dv.tools.clip = Some(c);
                }
                run(acts, "detail.act", "paste");
            }
            egui::Event::Key { key, pressed: true, modifiers: m, .. } => {
                if m.command && !m.alt {
                    match key {
                        egui::Key::S => run(acts, "detail.save", ""),
                        _ if typing => {}
                        egui::Key::Z if m.shift => run(acts, "detail.redo", ""),
                        egui::Key::Z => run(acts, "detail.undo", ""),
                        egui::Key::Y => run(acts, "detail.redo", ""),
                        egui::Key::V if m.shift => run(acts, "detail.act", "paste-in-place"),
                        egui::Key::V => run(acts, "detail.act", "paste"),
                        egui::Key::C => run(acts, "detail.act", "copy"),
                        egui::Key::X => run(acts, "detail.act", "cut"),
                        egui::Key::D => run(acts, "detail.act", "duplicate"),
                        egui::Key::A => run(acts, "detail.act", "select-all"),
                        _ => {}
                    }
                    continue;
                }
                if typing || m.alt {
                    continue;
                }
                match key {
                    egui::Key::Escape => acts.push(Act::Escape),
                    egui::Key::Delete | egui::Key::Backspace if !dv.selected().is_empty() => run(acts, "detail.act", "delete"),
                    _ => {
                        let tool = match key {
                            egui::Key::V => "select",
                            egui::Key::B => "beam",
                            egui::Key::C => "column",
                            egui::Key::R => "brace",
                            egui::Key::P => "plate",
                            egui::Key::F => "footing",
                            egui::Key::S => "stair",
                            egui::Key::M => "move",
                            _ => continue,
                        };
                        run(acts, "detail.tool", tool);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The DetailCraft frame: bar, left / right / bottom panels and the 3D view (lib.rs calls this while a model is open).
pub fn ui(app: &mut UiApp, ui: &mut egui::Ui) {
    let tok = app.tokens;
    let dark = app.preset == Preset::Dark;
    let mut acts: Vec<crate::detailpanel::Act> = Vec::new();
    if let Some(dv) = app.detail.as_mut() {
        dv.recompute();
        keys(dv, ui, &mut acts);
        egui::Panel::top("detail_bar")
            .exact_size(32.0)
            .frame(egui::Frame::new().fill(tok.panel_darker).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| crate::detailpanel::bar(dv, ui, &tok, &mut acts));
        let frame = egui::Frame::new().fill(tok.panel).inner_margin(8);
        egui::Panel::left("detail_left").default_size(250.0).size_range(190.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                crate::detailpanel::left_head(dv, ui, &tok, &mut acts);
                crate::detailprops::left_setup(dv, ui, &tok, &mut acts);
                crate::detailpanel::left(dv, ui, &tok, &mut acts);
            });
        });
        egui::Panel::right("detail_right").default_size(300.0).size_range(230.0..=480.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::detailprops::right(dv, ui, &tok, &mut acts));
        });
        egui::Panel::bottom("detail_reports").default_size(220.0).size_range(90.0..=520.0).resizable(true).frame(frame).show(ui, |ui| {
            crate::detailpanel::reports(dv, ui, &tok, &mut acts);
        });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Panel::top("detail_viewbar")
                .exact_size(26.0)
                .frame(egui::Frame::new().fill(tok.panel).inner_margin(egui::Margin::symmetric(8, 2)))
                .show(ui, |ui| crate::detailprops::view_bar(dv, ui, &tok, &mut acts));
            let rect = ui.max_rect();
            dv.show(ui, rect, &tok, dark);
        });
        crate::detailprops::settings_window(dv, ui.ctx(), &tok, &mut acts);
        if let Some(text) = dv.tools.clip_out.take() {
            ui.ctx().copy_text(text);
        }
        if !dv.report.is_empty() {
            let mut open = true;
            egui::Window::new(RichText::new(t("app.ifc.rep.title", &[("file", F::S(dv.name.clone()))])).strong())
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for l in &dv.report {
                            ui.label(l);
                        }
                        ui.label(RichText::new(t0("app.ifc.rep.note")).color(tok.text_dim));
                    });
                    if ui.button(t0("app.ok")).clicked() {
                        acts.push(crate::detailpanel::Act::CloseReport);
                    }
                });
            if !open {
                acts.push(crate::detailpanel::Act::CloseReport);
            }
        }
        app.cursor = None;
    }
    for a in acts {
        let r = match a {
            crate::detailpanel::Act::Select(r) => {
                if let Some(dv) = app.detail.as_mut() {
                    dv.select(r);
                }
                Ok(())
            }
            crate::detailpanel::Act::Fit => {
                if let Some(dv) = app.detail.as_mut() {
                    dv.fit();
                }
                Ok(())
            }
            crate::detailpanel::Act::Focus(at) => {
                if let Some(dv) = app.detail.as_mut() {
                    dv.focus(at, 2500.0);
                }
                Ok(())
            }
            crate::detailpanel::Act::Escape => {
                if let Some(dv) = app.detail.as_mut() {
                    if dv.win.take().is_some() {
                        dv.message = t0("sel.window.cancel");
                    } else {
                        dv.escape();
                    }
                }
                Ok(())
            }
            crate::detailpanel::Act::CloseReport => {
                if let Some(dv) = app.detail.as_mut() {
                    dv.report.clear();
                }
                Ok(())
            }
            crate::detailpanel::Act::Run(id, name) => {
                let factor = name.parse::<f64>().ok();
                run(app, id, &Params { name: Some(name), factor, ..Default::default() })
            }
        };
        if let Err(e) = r {
            match app.detail.as_mut() {
                Some(dv) => dv.message = e,
                None => app.error = Some(e),
            }
        }
    }
    if let Some(dv) = app.detail.as_ref() {
        app.message = dv.status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_unique_and_apart_from_the_others() {
        let mut ids: Vec<&str> = COMMANDS
            .iter()
            .map(|c| c.0)
            .chain(crate::UI_COMMANDS.iter().map(|c| c.0))
            .chain(crate::civilview::COMMANDS.iter().map(|c| c.0))
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn files_are_told_apart() {
        assert!(is_detail_file("frame.detailcraft.json", b"{}"));
        assert!(is_detail_file("x.json", b"{\n \"format\": \"detailcraft\",\n \"version\": 2"));
        assert!(!is_detail_file("x.json", b"{\"format\":\"civilcraft-project\"}"));
        assert!(!is_detail_file("x.ifc", b"ISO-10303-21;"));
    }

    #[test]
    fn the_sample_opens_builds_and_marks() {
        let v = DetailView::sample().unwrap();
        assert_eq!(cc_detail::js::arr(v.model.state.get("members")).len(), 11);
        assert!(v.built.parts.len() > 11, "members and plates");
        assert!(!v.marks.parts.is_empty());
        assert!(!v.schedule.rows.is_empty(), "the footings have bars");
        assert!(!v.model.dirty());
    }

    #[test]
    fn an_edit_is_one_undo_step_and_rebuilds() {
        let mut v = DetailView::sample().unwrap();
        let beam = cc_detail::js::arr(v.model.state.get("members"))
            .iter()
            .find(|m| m.get("type").and_then(Value::as_str) == Some("beam"))
            .and_then(|m| m.get("id").and_then(Value::as_str))
            .unwrap()
            .to_string();
        let w0: f64 = v.built.parts.iter().map(|p| p.weight).sum();
        v.set(&format!("{beam}.profile=W21x44")).unwrap();
        assert_eq!(v.model.undo_stack.len(), 1);
        assert_eq!(v.model.undo_stack[0].label, "Change profile");
        let w1: f64 = v.built.parts.iter().map(|p| p.weight).sum();
        assert!(w1 > w0, "a heavier beam: {w0} → {w1}");
        assert!(v.model.dirty());
        v.undo();
        let w2: f64 = v.built.parts.iter().map(|p| p.weight).sum();
        assert_eq!(w2, w0);
        assert!(v.message.contains("Change profile"));
        v.redo();
        assert_eq!(v.built.parts.iter().map(|p| p.weight).sum::<f64>(), w1);
        // a refused edit changes nothing and adds no step
        assert!(v.set(&format!("{beam}.rotation=abc")).is_err());
        assert!(v.set("m999.name=x").is_err());
        assert_eq!(v.model.undo_stack.len(), 1);
        v.set(&format!("{beam}.start=0,0,4100")).unwrap();
        v.set("model.name=Frame B").unwrap();
        assert_eq!(v.model.state["name"], "Frame B");
        assert_eq!(v.model.undo_stack.len(), 3);
    }

    #[test]
    fn the_saved_file_opens_again_and_imports_its_own_ifc() {
        let v = DetailView::sample().unwrap();
        let text = detailexport::project_json(&v.model.state);
        let again = DetailView::open_model("Sample frame.detailcraft.json", text.as_bytes()).unwrap();
        // readModel puts a connection's keys in its own order (as the JS does): the second save is the fixed point
        let text2 = detailexport::project_json(&again.model.state);
        let third = DetailView::open_model("x.detailcraft.json", text2.as_bytes()).unwrap();
        assert_eq!(detailexport::project_json(&third.model.state), text2);
        assert_eq!(again.built.parts.len(), v.built.parts.len());
        let ifc =
            cc_io_detail::ifc_out::write_ifc(&v.model.state, &v.built, &v.marks, Some(&v.footing_marks), Some(&v.schedule), None, 0.0)
                .unwrap();
        let imp = DetailView::import_ifc("frame.ifc", ifc.as_bytes()).unwrap();
        // every part comes back as a member (plates as plate members)
        assert_eq!(cc_detail::js::arr(imp.model.state.get("members")).len(), v.built.parts.len());
        assert!(imp.model.dirty(), "an imported model is not saved yet");
        assert!(!imp.report.is_empty());
    }

    #[test]
    fn focus_brings_a_point_to_the_middle() {
        let mut v = DetailView::sample().unwrap();
        v.focus([6000.0, 7000.0, 4000.0], 2500.0);
        let c = v.view.cam.target;
        assert!((c[0] - 6.0).abs() < 1e-9 && (c[1] - 7.0).abs() < 1e-9 && (c[2] - 4.0).abs() < 1e-9);
        assert!(!v.view.fit_pending);
        v.focus([f64::NAN, 0.0, 0.0], 2500.0);
        assert!((v.view.cam.target[0] - 6.0).abs() < 1e-9, "a bad point changes nothing");
    }

    #[test]
    fn hostile_files_are_refused_not_a_crash() {
        assert!(DetailView::open_model("x.detailcraft.json", b"{not json").is_err());
        assert!(DetailView::open_model("x.detailcraft.json", b"{\"format\":\"detailcraft\",\"version\":99}").is_err());
        assert!(DetailView::open_model("x.detailcraft.json", &[0xff, 0xfe, 0]).is_err());
        let _ = DetailView::import_ifc("x.ifc", b"not an ifc");
        let _ = DetailView::import_ifc("x.ifc", b"ISO-10303-21;\nDATA;\n#1=IFCBEAM(;\nENDSEC;\n");
    }

    #[test]
    fn the_clash_check_and_exports_run_headless() {
        let mut v = DetailView::sample().unwrap();
        v.run_clashes();
        assert!(v.clash.is_some());
        for k in [
            "project",
            "ifc",
            "nc",
            "bom",
            "bending",
            "kiss",
            "cutlist",
            "report-csv",
            "report-pdf",
            "drawings",
            "drawings:dxf",
            "drawings:dwg-AC1018",
            "sheet:0",
            "sheet:1:dxf",
        ] {
            match detailexport::run(&v.ctx(), k, 1_760_700_000_000.0, true, cc_clock::Zone::Utc).unwrap() {
                Outcome::File(name, bytes, msg) => assert!(!bytes.is_empty() && !name.is_empty() && !msg.is_empty(), "{k}"),
                Outcome::Nothing(m) => panic!("{k}: {m}"),
            }
        }
        let beam = v.built.parts.iter().find(|p| p.kind == "beam").map(|p| p.id.clone()).unwrap();
        v.select(Some(Ref::Member(beam)));
        assert!(matches!(detailexport::run(&v.ctx(), "shop", 0.0, true, cc_clock::Zone::Utc).unwrap(), Outcome::File(..)));
        assert!(detailexport::run(&v.ctx(), "nope", 0.0, true, cc_clock::Zone::Utc).is_err());
        assert!(detailexport::run(&v.ctx(), "drawings:png", 0.0, true, cc_clock::Zone::Utc).is_err());
    }
}
