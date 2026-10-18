//! DetailCraft's modelling tools in the app (M14.3–M14.7), the way detailcraft.js runs them: placing members, plates,
//! footings and stairs on grid intersections (`placeAt` / `placeLine` / the "Add by grid" form), the connection tools
//! (`addEndPlates` … `applySimilar`), several things selected (`setSelection`, Shift+click, select all, selection
//! windows and lassos through winsel.js), the clipboard (copy, cut, paste on a grid intersection, at an offset or in
//! place, duplicate), "Move, rotate, mirror" (transform.js) and "Change on all of them", grids, levels, phases, holes,
//! copes, custom profiles and the Settings. Every model edit goes through the engine's `Model::change` (one undo step)
//! and says what it did in the status line in 0.5's words; this module holds the tool state and maps the `detail.*`
//! tool commands. The UI (detailview.rs, detailprops.rs) only calls these.

use cc_detail::i18n::{F, t, t0};
use cc_detail::js::{arr, num_to_string, round, str_to_number, string_of};
use serde_json::{Value, json};

use crate::detail3d::Ref;
use crate::detailview::DetailView;

/// The tools (detailcraft.js `setTool`).
pub const TOOLS: [&str; 9] = ["select", "beam", "column", "brace", "plate", "footing", "stair", "move", "paste"];

/// A grid intersection on a level (model.js `nodes(state)` entries).
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub p: [f64; 3],
    pub gx: String,
    pub gy: String,
    pub level: String,
}

/// model.js `nodes(state)`.
pub fn all_nodes(state: &Value) -> Vec<Node> {
    cc_detail::model::nodes(state)
        .iter()
        .map(|n| Node {
            p: cc_detail::js::p3_of(n.get("p")),
            gx: string_of(n.get("gx")),
            gy: string_of(n.get("gy")),
            level: string_of(n.get("level")),
        })
        .collect()
}

/// A node from "gy/gx/level" (labels and the level's name) or "x,y,z" (mm, one of the nodes).
pub fn node_at(state: &Value, spec: &str) -> Result<Node, String> {
    let nums: Vec<f64> = spec.split(',').map(|s| str_to_number(s.trim())).collect();
    if nums.len() == 3 && nums.iter().all(|v| v.is_finite()) {
        return all_nodes(state)
            .into_iter()
            .find(|n| (0..3).all(|k| n.p.get(k).zip(nums.get(k)).is_some_and(|(a, b)| (a - b).abs() < 0.5)))
            .ok_or_else(|| format!("no grid intersection at {spec}"));
    }
    let mut it = spec.splitn(3, '/');
    let (Some(gy), Some(gx), Some(level)) = (it.next(), it.next(), it.next()) else {
        return Err(format!("a grid intersection: \"gy/gx/level\" or \"x,y,z\", not {spec}"));
    };
    let p = cc_detail::model::point_at(state, &Value::from(gy), &Value::from(gx), &Value::from(level))
        .ok_or_else(|| format!("no grid intersection {spec}"))?;
    Ok(Node { p, gx: gx.into(), gy: gy.into(), level: level.into() })
}

/// The options of a member tool (detailcraft.js `opts.beam` …).
#[derive(Clone, Debug, PartialEq)]
pub struct MemOpt {
    pub profile: String,
    pub material: String,
    pub position: String,
    pub rotation: f64,
}

/// detailcraft.js `opts`: what the placing tools put in.
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub beam: MemOpt,
    pub column: MemOpt,
    /// a column's top level ("" = the next level up)
    pub column_to: String,
    pub brace: MemOpt,
    pub plate_t: f64,
    pub plate_w: f64,
    pub plate_material: String,
    pub plate_position: String,
    pub plate_rotation: f64,
    /// { L, B, H, concrete, mat: {…}, starters: {…} }
    pub footing: Value,
    /// STAIR_DEFAULTS with rise 0 (up to the next level)
    pub stair: Value,
}

fn mem_opt(kind: &str) -> MemOpt {
    let (profile, material, position) = cc_detail::model::defaults_for(kind);
    MemOpt { profile: profile.into(), material: material.into(), position: position.into(), rotation: 0.0 }
}

impl Opts {
    pub fn new(steel: &str) -> Opts {
        let d = cc_detail::rebar::footing_defaults();
        let mut stair = cc_detail::stair::stair_defaults();
        if let Some(o) = stair.as_object_mut() {
            o.insert("rise".into(), json!(0));
        }
        let mut o = Opts {
            beam: mem_opt("beam"),
            column: mem_opt("column"),
            column_to: String::new(),
            brace: mem_opt("brace"),
            plate_t: 20.0,
            plate_w: 200.0,
            plate_material: "A36".into(),
            plate_position: "middle".into(),
            plate_rotation: 0.0,
            footing: json!({ "L": d.get("L"), "B": d.get("B"), "H": d.get("H"), "concrete": d.get("concrete"), "mat": d.get("mat"), "starters": d.get("starters") }),
            stair,
        };
        o.apply_steel(steel);
        o
    }

    /// detailcraft.js `applySteel()`: the profiles and grades new members start with, and the footings' bars.
    pub fn apply_steel(&mut self, steel: &str) {
        let eu = steel == "eu";
        let pick = |us: (&'static str, &'static str), e: (&'static str, &'static str)| if eu { e } else { us };
        for (o, us, e) in [
            (&mut self.beam, ("W16x26", "A992"), ("IPE300", "S355")),
            (&mut self.column, ("W12x50", "A992"), ("HEB200", "S355")),
            (&mut self.brace, ("HSS6x6x1/4", "A500 Gr C"), ("HEA160", "S355")),
        ] {
            let (p, m) = pick(us, e);
            o.profile = p.into();
            o.material = m.into();
        }
        self.plate_material = if eu { "S355".into() } else { "A36".into() };
        let d = cc_detail::rebar::footing_defaults();
        let dm = |k: &str| d.get("mat").and_then(|m| m.get(k)).cloned().unwrap_or(Value::Null);
        if let Some(f) = self.footing.as_object_mut() {
            if let Some(m) = f.get_mut("mat").and_then(Value::as_object_mut) {
                m.insert("barX".into(), if eu { json!("H16") } else { dm("barX") });
                m.insert("barY".into(), if eu { json!("H16") } else { dm("barY") });
            }
            if let Some(s) = f.get_mut("starters").and_then(Value::as_object_mut) {
                s.insert(
                    "bar".into(),
                    if eu { json!("H20") } else { d.get("starters").and_then(|s| s.get("bar")).cloned().unwrap_or(Value::Null) },
                );
            }
            f.insert("concrete".into(), if eu { json!("C30/37") } else { d.get("concrete").cloned().unwrap_or(Value::Null) });
        }
    }

    /// `detail.opt`: "<tool>.<field>=<value>" (the tool options panel's fields; a number that does not read is refused
    /// with numIn's words).
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let (path, value) = spec.split_once('=').ok_or("detail.opt: name = \"<tool>.<field>=<value>\"")?;
        let (tool, field) = path.split_once('.').ok_or("detail.opt: <tool>.<field>=<value>")?;
        let number = || -> Result<f64, String> {
            let v = str_to_number(value);
            if value.trim().is_empty() || !v.is_finite() { Err(t0("app.num.bad")) } else { Ok(v) }
        };
        let flag = || -> Result<bool, String> {
            match value {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(format!("detail.opt: {path} = true | false")),
            }
        };
        match tool {
            "beam" | "column" | "brace" => {
                let o = match tool {
                    "beam" => &mut self.beam,
                    "column" => &mut self.column,
                    _ => &mut self.brace,
                };
                match field {
                    "profile" => o.profile = value.into(),
                    "material" => o.material = value.into(),
                    "position" => o.position = value.into(),
                    "rotation" => o.rotation = number()?,
                    "to" if tool == "column" => self.column_to = value.into(),
                    _ => return Err(format!("detail.opt: unknown field {path}")),
                }
            }
            "plate" => match field {
                "t" => self.plate_t = number()?,
                "w" => self.plate_w = number()?,
                "material" => self.plate_material = value.into(),
                "position" => self.plate_position = value.into(),
                "rotation" => self.plate_rotation = number()?,
                _ => return Err(format!("detail.opt: unknown field {path}")),
            },
            "footing" => {
                let v = match field {
                    "L"
                    | "B"
                    | "H"
                    | "mat.sX"
                    | "mat.sY"
                    | "mat.cover"
                    | "starters.nx"
                    | "starters.ny"
                    | "starters.sx"
                    | "starters.sy"
                    | "starters.projection" => json!(number()?),
                    "mat.hooks" | "starters.on" | "starters.hook" => json!(flag()?),
                    "concrete" | "mat.barX" | "mat.barY" | "starters.bar" => json!(value),
                    _ => return Err(format!("detail.opt: unknown field {path}")),
                };
                deep_set(&mut self.footing, field, v);
            }
            "stair" => {
                let v = match field {
                    "dir" | "rise" | "going" | "width" | "maxRiser" | "tread.t" | "tread.nosing" => json!(number()?),
                    "stringer" | "material" => json!(value),
                    _ => return Err(format!("detail.opt: unknown field {path}")),
                };
                deep_set(&mut self.stair, field, v);
            }
            _ => return Err(format!("detail.opt: unknown tool {tool}")),
        }
        Ok(())
    }

    /// The member options of a member tool.
    pub fn member(&self, tool: &str) -> Option<&MemOpt> {
        match tool {
            "beam" => Some(&self.beam),
            "column" => Some(&self.column),
            "brace" => Some(&self.brace),
            _ => None,
        }
    }
}

/// Sets "a.b" in an object (detailcraft.js `deepAssign(o, { a: { b: v } })`).
pub fn deep_set(o: &mut Value, path: &str, v: Value) {
    match path.split_once('.') {
        Some((a, b)) => {
            if let Some(sub) = o.get_mut(a) {
                deep_set(sub, b, v);
            }
        }
        None => {
            if let Some(m) = o.as_object_mut() {
                m.insert(path.into(), v);
            }
        }
    }
}

/// The tool state of the open model (detailcraft.js `tool`, `pending`, `opts`, `curPhase`, the clipboard, `moveRefs`).
#[derive(Clone, Debug)]
pub struct Tools {
    pub tool: &'static str,
    /// the first node of a two-point placement (or of a move)
    pub pending: Option<Node>,
    pub opts: Opts,
    /// new members, footings and stairs go in this phase (D13)
    pub cur_phase: f64,
    /// the last copy (readClip'd); kept in this window
    pub clip: Option<Value>,
    /// the clip being placed by the paste tool
    pub paste_clip: Option<Value>,
    /// what the Move tool moves
    pub move_refs: Option<Vec<Ref>>,
    /// the grid intersection under the pointer while placing
    pub hover: Option<Node>,
    /// a copy's text for the system clipboard (another DetailCraft window can paste it)
    pub clip_out: Option<String>,
}

impl Default for Tools {
    fn default() -> Self {
        Tools {
            tool: "select",
            pending: None,
            opts: Opts::new("us"),
            cur_phase: 1.0,
            clip: None,
            paste_clip: None,
            move_refs: None,
            hover: None,
            clip_out: None,
        }
    }
}

/// The connection parameters' labels and help (connections.js `PARAM_LABELS` / `PARAM_HELP` keys).
pub fn param_label_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "bolts" => "conn.param.bolts",
        "lap" => "conn.param.lap",
        "braceGap" => "conn.param.brace-gap",
        "braceEdge" => "conn.param.brace-edge",
        "bracePitch" => "conn.param.brace-pitch",
        "braceGauge" => "conn.param.brace-gauge",
        "jointGap" => "conn.param.joint-gap",
        "flangeT" => "conn.param.flange-t",
        "flangeRows" => "conn.param.flange-rows",
        "flangePitch" => "conn.param.flange-pitch",
        "flangeGauge" => "conn.param.flange-gauge",
        "flangeEdge" => "conn.param.flange-edge",
        "webT" => "conn.param.web-t",
        "webRows" => "conn.param.web-rows",
        "webCols" => "conn.param.web-cols",
        "webPitch" => "conn.param.web-pitch",
        "webPitchX" => "conn.param.web-pitch-x",
        "webEdge" => "conn.param.web-edge",
        "webH" => "conn.param.web-h",
        "tp" => "conn.param.tp",
        "width" => "conn.param.width",
        "length" => "conn.param.length",
        "extTop" => "conn.param.ext-top",
        "extBottom" => "conn.param.ext-bottom",
        "boltD" => "conn.param.bolt-d",
        "hole" => "conn.param.hole",
        "grade" => "conn.param.grade",
        "rows" => "conn.param.rows",
        "pitch" => "conn.param.pitch",
        "gauge" => "conn.param.gauge",
        "allowance" => "conn.param.allowance",
        "weld" => "conn.param.weld",
        "rodD" => "conn.param.rod-d",
        "sx" => "conn.param.sx",
        "sy" => "conn.param.sy",
        "projection" => "conn.param.projection",
        "embed" => "conn.param.embed",
        "plateGrade" => "conn.param.plate-grade",
        "grout" => "conn.param.grout",
        "stiffeners" => "conn.param.stiffeners",
        "stiffT" => "conn.param.stiff-t",
        "stiffH" => "conn.param.stiff-h",
        "lugD" => "conn.param.lug-d",
        "lugT" => "conn.param.lug-t",
        "lugW" => "conn.param.lug-w",
        "washers" => "conn.param.washers",
        "washerSize" => "conn.param.washer-size",
        "washerT" => "conn.param.washer-t",
        "edge" => "conn.param.edge",
        "ev" => "conn.param.ev",
        "gap" => "conn.param.gap",
        "eh" => "conn.param.eh",
        _ => return None,
    })
}
pub fn param_help_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "allowance" => "conn.help.allowance",
        "hole" => "conn.help.hole",
        "grout" => "conn.help.grout",
        "stiffeners" => "conn.help.stiffeners",
        "lugD" => "conn.help.lug-d",
        "washers" => "conn.help.washers",
        "washerSize" | "washerT" => "conn.help.washer-size",
        "ev" => "conn.help.ev",
        "gap" => "conn.help.gap",
        "eh" => "conn.help.eh",
        "lap" => "conn.help.lap",
        "bolts" => "conn.help.gu-bolts",
        "braceGap" => "conn.help.brace-gap",
        "braceGauge" => "conn.help.brace-gauge",
        "jointGap" => "conn.help.joint-gap",
        "flangeGauge" => "conn.help.flange-gauge",
        "webH" => "conn.help.web-h",
        "flangeEdge" | "webEdge" => "conn.help.sp-edge",
        _ => return None,
    })
}
/// `PARAM_LABELS[k] || k`.
pub fn param_label(k: &str) -> String {
    param_label_key(k).map_or_else(|| k.to_string(), t0)
}
/// connections.js `PARAM_CHOICES`.
pub fn param_choices(k: &str) -> Option<&'static [f64]> {
    match k {
        "stiffeners" => Some(&[0.0, 2.0, 4.0]),
        "washers" => Some(&[0.0, 1.0]),
        _ => None,
    }
}

fn mm(v: f64) -> String {
    num_to_string(round(v))
}
fn r1(v: f64) -> String {
    num_to_string(round(v * 10.0) / 10.0)
}

/// A ref as winsel.js / transform.js / clipboard.js take it: `{ kind, id }`.
pub fn ref_json(r: &Ref) -> Value {
    let kind = match r {
        Ref::Member(_) => "member",
        Ref::Conn(_) => "conn",
        Ref::Stair(_) => "stair",
        Ref::Footing(_) => "footing",
    };
    json!({ "kind": kind, "id": r.id() })
}
pub fn ref_of_json(v: &Value) -> Option<Ref> {
    Ref::parse(&format!("{}:{}", string_of(v.get("kind")), string_of(v.get("id"))))
}
fn refs_json(refs: &[Ref]) -> Vec<Value> {
    refs.iter().map(ref_json).collect()
}
fn ids_of(refs: &[Ref], want: fn(&Ref) -> bool) -> Value {
    Value::Array(refs.iter().filter(|r| want(r)).map(|r| Value::from(r.id())).collect())
}

/// detailcraft.js `countText(c)`.
pub fn count_text(c: &Value) -> String {
    let mut out = Vec::new();
    for (k, key) in [
        ("members", "app.count.members"),
        ("connections", "app.count.connections"),
        ("footings", "app.count.footings"),
        ("stairs", "app.count.stairs"),
    ] {
        let n = c.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        if n != 0.0 {
            out.push(t(key, &[("n", F::N(n))]));
        }
    }
    if out.is_empty() { t0("app.count.nothing") } else { out.join(&t0("app.count.sep")) }
}
/// detailcraft.js `whatText(members, footings)`.
pub fn what_text(members: f64, footings: f64) -> String {
    let mut out = Vec::new();
    if members != 0.0 {
        out.push(t("what.members", &[("n", F::N(members))]));
    }
    if footings != 0.0 {
        out.push(t("what.footings", &[("n", F::N(footings))]));
    }
    out.join(&t0("what.and"))
}

/// One step of a transform (detailcraft.js `runXf`'s `how`).
pub enum How {
    Move([f64; 3]),
    Rotate(f64, String),
    Mirror(String),
}
impl How {
    fn kind(&self) -> &'static str {
        match self {
            How::Move(_) => "move",
            How::Rotate(..) => "rotate",
            How::Mirror(_) => "mirror",
        }
    }
}

impl DetailView {
    fn say(&mut self, m: String) {
        self.message = m;
    }

    /// detailcraft.js `edit(label, fn)`: one undo step; a refused edit is said and nothing changes (None).
    pub fn edit<T>(&mut self, label: &str, f: impl FnOnce(&mut cc_detail::model::Tx) -> cc_detail::js::Res<T>) -> Option<T> {
        let t1 = web_time::Instant::now();
        let r = self.model.change(label, f);
        self.set_timing("edit", t1.elapsed().as_secs_f64() * 1000.0);
        self.recompute();
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.message = e;
                None
            }
        }
    }

    /// What is selected: several (multi) or the one (sel).
    pub fn selected(&self) -> Vec<Ref> {
        if !self.multi.is_empty() {
            return self.multi.clone();
        }
        self.sel.iter().cloned().collect()
    }

    /// detailcraft.js `setSelection(refs)`: missing ones and repeats dropped; one → sel, several → multi.
    pub fn set_selection(&mut self, refs: Vec<Ref>) {
        let mut list: Vec<Ref> = Vec::new();
        for r in refs {
            if self.exists(&r) && !list.contains(&r) {
                list.push(r);
            }
        }
        self.editing = None;
        if list.len() <= 1 {
            self.sel = list.pop();
            self.multi.clear();
        } else {
            self.sel = None;
            self.multi = list;
        }
    }

    /// The refs painted as selected (the Move tool shows what it moves).
    pub fn shown_selection(&self) -> Vec<Ref> {
        if self.tools.tool == "move" {
            return self.tools.move_refs.clone().unwrap_or_default();
        }
        self.selected()
    }

    /// detailcraft.js `pick(ref, e)`: a click in the view with the Select tool; Shift adds it or takes it away.
    pub fn pick(&mut self, r: Option<Ref>, add: bool) {
        if self.tools.tool != "select" {
            return;
        }
        if !add {
            self.set_selection(r.into_iter().collect());
            if let Some(s) = self.sel.clone() {
                let d = self.describe(&s);
                self.say(d);
            }
            return;
        }
        let Some(r) = r else { return };
        let cur = self.selected();
        let next: Vec<Ref> =
            if cur.contains(&r) { cur.into_iter().filter(|x| *x != r).collect() } else { cur.into_iter().chain([r]).collect() };
        let n = next.len();
        let one = next.first().cloned();
        self.set_selection(next);
        let m = match (n, one) {
            (1, Some(r)) => self.describe(&r),
            _ => t("app.sel.many", &[("n", F::N(n as f64))]),
        };
        self.say(m);
    }

    /// detailcraft.js `setTool(t)`.
    pub fn set_tool(&mut self, tool: &str) -> Result<(), String> {
        let tool = TOOLS.iter().copied().find(|x| *x == tool).ok_or_else(|| format!("detail.tool: unknown tool {tool}"))?;
        if tool == "move" {
            let refs: Vec<Ref> = self.selected().into_iter().filter(|r| !matches!(r, Ref::Conn(_))).collect();
            if refs.is_empty() {
                self.say(t0("xf.none"));
                return Ok(());
            }
            self.tools.move_refs = Some(refs);
        } else {
            self.tools.move_refs = None;
        }
        let mut tool = tool;
        self.tools.tool = tool;
        self.tools.pending = None;
        self.tools.paste_clip = if tool == "paste" { self.tools.clip.clone() } else { None };
        if tool == "paste" && self.tools.paste_clip.is_none() {
            tool = "select";
            self.tools.tool = tool;
        }
        if tool != "select" {
            self.sel = None;
            self.multi.clear();
        }
        self.editing = None;
        let m = match tool {
            "move" => t0("xf.move.from"),
            "select" => t0("tool.select.how"),
            other => t0(&format!("app.tool.how.{other}")),
        };
        self.say(m);
        Ok(())
    }

    /// detailcraft.js `placeAt(n)`: a click on a grid intersection with the current tool.
    pub fn place_at(&mut self, n: &Node) {
        let s = &self.model.state;
        let tool = self.tools.tool;
        if tool == "move" {
            let Some(from) = self.tools.pending.clone() else {
                let m = t("xf.move.to", &[("from", F::S(cc_detail::model::node_name(s, n.p)))]);
                self.tools.pending = Some(n.clone());
                self.say(m);
                return;
            };
            if from.p == n.p {
                self.say(t0("xf.move.same"));
                return;
            }
            let d = [n.p[0] - from.p[0], n.p[1] - from.p[1], n.p[2] - from.p[2]];
            let refs = self.tools.move_refs.clone().unwrap_or_default();
            let _ = self.set_tool("select");
            if let Ok(xf) = cc_detail::transform::move_xf([Some(&json!(d[0])), Some(&json!(d[1])), Some(&json!(d[2]))]) {
                self.run_xf(&refs, &xf, false, How::Move(d));
            }
            return;
        }
        if tool == "paste" {
            let Some(c) = self.tools.paste_clip.clone() else {
                let _ = self.set_tool("select");
                return;
            };
            let b = cc_detail::js::p3_of(c.get("base"));
            let node = cc_detail::model::node_name(s, n.p);
            self.paste_clip_at(&c, [n.p[0] - b[0], n.p[1] - b[1], n.p[2] - b[2]], "at", &[("node", F::S(node))]);
            return;
        }
        let cur_phase = self.tools.cur_phase;
        if tool == "column" {
            let levels = arr(s.get("levels"));
            let above: Vec<&Value> = levels.iter().filter(|l| cc_detail::js::num(l.get("z")) > n.p[2] + 0.5).collect();
            let to = &self.tools.opts.column_to;
            let top = above.iter().find(|l| l.get("name").and_then(Value::as_str) == Some(to.as_str())).or(above.first()).copied();
            let Some(top) = top else {
                self.say(t("app.place.column.no-level", &[("level", F::S(n.level.clone()))]));
                return;
            };
            let (top_z, top_name) = (cc_detail::js::num(top.get("z")), string_of(top.get("name")));
            let o = self.tools.opts.column.clone();
            let m = json!({ "type": "column", "profile": o.profile, "material": o.material, "position": o.position, "rotation": o.rotation, "start": n.p, "end": [n.p[0], n.p[1], top_z], "phase": cur_phase });
            if self.edit(&t0("app.undo.place.column"), |tx| tx.add_member(Some(&m))).is_some() {
                self.say(t(
                    "app.place.column.done",
                    &[
                        ("profile", F::S(o.profile)),
                        ("gy", F::S(n.gy.clone())),
                        ("gx", F::S(n.gx.clone())),
                        ("level", F::S(n.level.clone())),
                        ("top", F::S(top_name)),
                    ],
                ));
            }
            return;
        }
        if tool == "footing" {
            let o = self.tools.opts.footing.clone();
            let f = json!({ "x": n.p[0], "y": n.p[1], "top": n.p[2], "L": o.get("L"), "B": o.get("B"), "H": o.get("H"), "concrete": o.get("concrete"), "mat": o.get("mat"), "starters": o.get("starters"), "phase": cur_phase });
            if self.edit(&t0("app.undo.place.footing"), |tx| tx.add_footing(Some(&f))).is_some() {
                let g = |k: &str| mm(cc_detail::js::num(o.get(k)));
                self.say(t(
                    "app.place.footing.done",
                    &[
                        ("l", F::S(g("L"))),
                        ("b", F::S(g("B"))),
                        ("h", F::S(g("H"))),
                        ("gy", F::S(n.gy.clone())),
                        ("gx", F::S(n.gx.clone())),
                    ],
                ));
            }
            return;
        }
        if tool == "stair" {
            let o = self.tools.opts.stair.clone();
            let mut rise = cc_detail::js::num(o.get("rise"));
            if rise == 0.0 || rise.is_nan() {
                let mut up: Vec<f64> =
                    arr(s.get("levels")).iter().map(|l| cc_detail::js::num(l.get("z"))).filter(|z| *z > n.p[2] + 0.5).collect();
                up.sort_by(f64::total_cmp);
                let Some(z) = up.first() else {
                    self.say(t("app.place.stair.no-level", &[("level", F::S(n.level.clone()))]));
                    return;
                };
                rise = z - n.p[2];
            }
            let mut x = o.clone();
            if let Some(m) = x.as_object_mut() {
                m.insert("x".into(), json!(n.p[0]));
                m.insert("y".into(), json!(n.p[1]));
                m.insert("z".into(), json!(n.p[2]));
                m.insert("rise".into(), json!(rise));
                m.insert("phase".into(), json!(cur_phase));
            }
            if let Some(id) = self.edit(&t0("app.undo.place.stair"), |tx| tx.add_stair(Some(&x))) {
                let st = arr(self.model.state.get("stairs")).iter().find(|v| v.get("id") == Some(&id)).cloned().unwrap_or(Value::Null);
                let custom = arr(self.model.state.get("profiles")).to_vec();
                let g = cc_detail::stair::stair_geometry(&cc_detail::stair::Stair::from_value(&st), &|nm: &str| {
                    cc_detail::profiles::get_profile_str(nm, &custom)
                });
                self.say(t(
                    "app.place.stair.done",
                    &[("n", F::N(g.n)), ("r", F::S(r1(g.riser))), ("gy", F::S(n.gy.clone())), ("gx", F::S(n.gx.clone()))],
                ));
            }
            return;
        }
        if !matches!(tool, "beam" | "brace" | "plate") {
            return;
        }
        let Some(a) = self.tools.pending.clone() else {
            let m = t("app.place.from", &[("node", F::S(cc_detail::model::node_name(s, n.p)))]);
            self.tools.pending = Some(n.clone());
            self.say(m);
            return;
        };
        if a.p == n.p {
            self.say(t0("app.place.same"));
            return;
        }
        self.tools.pending = None;
        self.place_line(a.p, n.p);
    }

    /// detailcraft.js `placeLine(a, b)`.
    pub fn place_line(&mut self, a: [f64; 3], b: [f64; 3]) {
        let tool = self.tools.tool;
        let o = &self.tools.opts;
        let (profile, material, position, rotation) = match tool {
            "plate" => (
                cc_detail::profiles::plate_name(o.plate_t, o.plate_w),
                o.plate_material.clone(),
                o.plate_position.clone(),
                o.plate_rotation,
            ),
            _ => match o.member(tool) {
                Some(m) => (m.profile.clone(), m.material.clone(), m.position.clone(), m.rotation),
                None => return,
            },
        };
        let m = json!({ "type": tool, "profile": profile, "material": material, "position": position, "rotation": rotation, "start": a, "end": b, "phase": self.tools.cur_phase });
        if self.edit(&t0(&format!("app.undo.place.{tool}")), |tx| tx.add_member(Some(&m))).is_some() {
            let st = &self.model.state;
            let msg = t(
                &format!("app.place.done.{tool}"),
                &[
                    ("profile", F::S(profile)),
                    ("from", F::S(cc_detail::model::node_name(st, a))),
                    ("to", F::S(cc_detail::model::node_name(st, b))),
                ],
            );
            self.say(msg);
        }
    }

    /// detailcraft.js `addByGridForm` submit: "gy/gx/level" (one-point tools) or "gy1/gx1/l1;gy2/gx2/l2".
    pub fn by_grid(&mut self, spec: &str) -> Result<(), String> {
        let two = matches!(self.tools.tool, "beam" | "brace" | "plate");
        let (s1, s2) = match spec.split_once(';') {
            Some((a, b)) => (a, Some(b)),
            None => (spec, None),
        };
        let a = node_at(&self.model.state, s1)?;
        if !two {
            self.place_at(&a);
            return Ok(());
        }
        let b = node_at(&self.model.state, s2.ok_or("detail.bygrid: \"gy1/gx1/l1;gy2/gx2/l2\" for a two-point tool")?)?;
        if a.p == b.p {
            self.say(t0("app.grid.same"));
            return Ok(());
        }
        self.tools.pending = None;
        self.place_line(a.p, b.p);
        Ok(())
    }

    /// Esc (detailcraft.js's key handler, after the view's own window and drag).
    pub fn escape(&mut self) {
        if self.tools.tool == "move" {
            let refs = self.tools.move_refs.clone().unwrap_or_default();
            let _ = self.set_tool("select");
            self.set_selection(refs);
            self.say(t0("xf.move.cancel"));
        } else if self.tools.pending.is_some() {
            self.tools.pending = None;
            self.say(t0("app.place.cancelled"));
        } else if self.tools.tool == "paste" {
            let _ = self.set_tool("select");
            self.say(t0("app.paste.cancelled"));
        } else if self.tools.tool != "select" {
            let _ = self.set_tool("select");
        } else if !self.selected().is_empty() {
            self.act("select-none");
        }
    }

    // ------------------------------------------------------------------------------------------ connections

    fn mems(&self) -> Vec<cc_detail::state::Mem> {
        cc_detail::state::members(&self.model.state)
    }
    fn conns(&self) -> Vec<cc_detail::state::Conn> {
        cc_detail::state::conns(&self.model.state)
    }
    fn taken_ends(&self) -> Vec<String> {
        self.conns().iter().filter(|c| c.is_beam_end()).map(|c| format!("{}:{}", c.beam.as_deref().unwrap_or("undefined"), c.end)).collect()
    }
    fn after_conns(&mut self, ids: &[Value]) {
        if let Some(id) = ids.first().and_then(Value::as_str) {
            self.sel = Some(Ref::Conn(id.into()));
            self.recompute();
        }
    }

    /// detailcraft.js `addEndPlates()`.
    pub fn add_end_plates(&mut self) {
        let Some(Ref::Member(id)) = self.sel.clone() else { return };
        let joints: Vec<_> = cc_detail::connections::find_joints(&self.mems()).into_iter().filter(|j| j.beam == id).collect();
        let taken = self.taken_ends();
        let free: Vec<_> = joints.iter().filter(|j| !taken.contains(&format!("{}:{}", j.beam, j.end))).collect();
        let flange: Vec<_> = free.iter().filter(|j| j.face == "flange" || j.face == "web").collect();
        if joints.is_empty() {
            self.say(t0("app.endplate.no-joint"));
            return;
        }
        if flange.is_empty() {
            self.say(t0(if free.is_empty() { "app.endplate.both" } else { "app.endplate.skew" }));
            return;
        }
        let list: Vec<Value> =
            flange.iter().map(|j| json!({ "type": "endplate", "beam": j.beam, "end": j.end, "column": j.column })).collect();
        let n = list.len();
        let ids = self.edit(&t0("app.undo.endplates"), |tx| list.iter().map(|c| tx.add_connection(Some(c))).collect::<Result<Vec<_>, _>>());
        if let Some(ids) = ids {
            self.after_conns(&ids);
            self.say(t("app.endplate.added", &[("n", F::N(n as f64))]));
        }
    }

    /// detailcraft.js `addShearTabs()`.
    pub fn add_shear_tabs(&mut self) {
        let Some(Ref::Member(id)) = self.sel.clone() else { return };
        let joints: Vec<_> = cc_detail::connections::find_joints(&self.mems()).into_iter().filter(|j| j.beam == id).collect();
        let taken = self.taken_ends();
        let free: Vec<_> = joints.iter().filter(|j| !taken.contains(&format!("{}:{}", j.beam, j.end))).collect();
        if joints.is_empty() {
            self.say(t0("app.endplate.no-joint"));
            return;
        }
        if free.is_empty() {
            self.say(t0("app.endplate.both"));
            return;
        }
        let list: Vec<Value> =
            free.iter().map(|j| json!({ "type": "sheartab", "beam": j.beam, "end": j.end, "column": j.column })).collect();
        let n = list.len();
        let ids = self.edit(&t0("app.undo.sheartabs"), |tx| list.iter().map(|c| tx.add_connection(Some(c))).collect::<Result<Vec<_>, _>>());
        if let Some(ids) = ids {
            self.after_conns(&ids);
            self.say(t("app.sheartab.added", &[("n", F::N(n as f64))]));
        }
    }

    fn selected_member_ids(&self) -> Vec<String> {
        self.selected()
            .into_iter()
            .filter_map(|r| match r {
                Ref::Member(i) => Some(i),
                _ => None,
            })
            .collect()
    }

    /// detailcraft.js `addSplices()`.
    pub fn add_splices(&mut self) {
        let ids = self.selected_member_ids();
        if ids.is_empty() {
            return;
        }
        let free = cc_detail::connections::free_splices(&self.mems(), &self.conns(), Some(&ids));
        if free.is_empty() {
            self.say(t0("app.splice.none"));
            return;
        }
        let list: Vec<Value> = free.iter().map(|j| json!({ "type": "splice", "a": j.a, "aEnd": j.a_end, "b": j.b })).collect();
        let n = list.len();
        let cids = self.edit(&t0("app.undo.splice"), |tx| list.iter().map(|c| tx.add_connection(Some(c))).collect::<Result<Vec<_>, _>>());
        if let Some(cids) = cids {
            self.after_conns(&cids);
            self.say(t("app.splice.added2", &[("n", F::N(n as f64))]));
        }
    }

    /// detailcraft.js `addGussets()`.
    pub fn add_gussets(&mut self) {
        let ids = self.selected_member_ids();
        if ids.is_empty() {
            return;
        }
        let free = cc_detail::connections::free_brace_ends(&self.mems(), &self.conns(), Some(&ids));
        if free.is_empty() {
            self.say(t0("app.gusset.none"));
            return;
        }
        let list: Vec<Value> =
            free.iter().map(|j| json!({ "type": "gusset", "brace": j.brace, "end": j.end, "support": j.support })).collect();
        let n = list.len();
        let cids = self.edit(&t0("app.undo.gusset"), |tx| list.iter().map(|c| tx.add_connection(Some(c))).collect::<Result<Vec<_>, _>>());
        if let Some(cids) = cids {
            self.after_conns(&cids);
            self.say(t("app.gusset.added", &[("n", F::N(n as f64))]));
        }
    }

    /// detailcraft.js `addBasePlate()`.
    pub fn add_base_plate(&mut self) {
        let Some(Ref::Member(id)) = self.sel.clone() else { return };
        if self.conns().iter().any(|c| c.kind == "baseplate" && c.column.as_deref() == Some(id.as_str())) {
            self.say(t0("app.baseplate.has"));
            return;
        }
        let c = json!({ "type": "baseplate", "column": id });
        if let Some(cid) = self.edit(&t0("app.undo.baseplate"), |tx| tx.add_connection(Some(&c))) {
            self.after_conns(&[cid]);
            self.say(t0("app.baseplate.added"));
        }
    }

    fn connection(&self, id: &str) -> Option<Value> {
        arr(self.model.state.get("connections")).iter().find(|c| c.get("id").and_then(Value::as_str) == Some(id)).cloned()
    }

    /// How many places "Apply to similar" would add the selected connection (detailcraft.js `connProps`' count).
    pub fn similar_count(&self, cv: &Value) -> usize {
        let c = cc_detail::state::Conn::from_value(cv);
        let (ms, cs) = (self.mems(), self.conns());
        match c.kind.as_str() {
            "gusset" => cc_detail::connections::similar_brace_ends(&ms, &cs, c.brace.as_deref(), c.support.as_deref()).len(),
            "splice" => cc_detail::connections::similar_splices(&ms, &cs, c.a.as_deref()).len(),
            _ if c.is_beam_end() => cc_detail::connections::similar_joints(&ms, &cs, c.beam.as_deref(), &c.end).len(),
            _ => cc_detail::connections::similar_columns(&ms, &cs, c.column.as_deref()).len(),
        }
    }

    /// detailcraft.js `applySimilar()` (its success line is said even when the edit is refused, as there).
    pub fn apply_similar(&mut self) {
        let Some(Ref::Conn(id)) = self.sel.clone() else { return };
        let Some(cv) = self.connection(&id) else { return };
        let c = cc_detail::state::Conn::from_value(&cv);
        let params = cv.get("params").cloned().unwrap_or(Value::Null);
        let (ms, cs) = (self.mems(), self.conns());
        let add_all = |this: &mut DetailView, label: &str, list: Vec<Value>| {
            this.edit(label, |tx| {
                for c in &list {
                    tx.add_connection(Some(c))?;
                }
                Ok(())
            });
        };
        if c.kind == "gusset" {
            let js = cc_detail::connections::similar_brace_ends(&ms, &cs, c.brace.as_deref(), c.support.as_deref());
            if js.is_empty() {
                self.say(t0("app.gusset.none"));
                return;
            }
            let n = js.len();
            let list = js
                .iter()
                .map(|j| json!({ "type": "gusset", "brace": j.brace, "end": j.end, "support": j.support, "params": params }))
                .collect();
            add_all(self, &t0("app.undo.gusset"), list);
            self.say(t("app.gusset.added", &[("n", F::N(n as f64))]));
        } else if c.kind == "splice" {
            let js = cc_detail::connections::similar_splices(&ms, &cs, c.a.as_deref());
            if js.is_empty() {
                self.say(t0("app.splice.none"));
                return;
            }
            let n = js.len();
            let list = js.iter().map(|j| json!({ "type": "splice", "a": j.a, "aEnd": j.a_end, "b": j.b, "params": params })).collect();
            add_all(self, &t0("app.undo.splice"), list);
            self.say(t("app.splice.added2", &[("n", F::N(n as f64))]));
        } else if c.is_beam_end() {
            let js = cc_detail::connections::similar_joints(&ms, &cs, c.beam.as_deref(), &c.end);
            if js.is_empty() {
                self.say(t0("app.similar.endplate.none"));
                return;
            }
            let n = js.len();
            let list =
                js.iter().map(|j| json!({ "type": c.kind, "beam": j.beam, "end": j.end, "column": j.column, "params": params })).collect();
            add_all(self, &t0("app.undo.similar.endplate"), list);
            self.say(t("app.similar.joints", &[("n", F::N(n as f64))]));
        } else {
            let cols = cc_detail::connections::similar_columns(&ms, &cs, c.column.as_deref());
            if cols.is_empty() {
                self.say(t0("app.similar.baseplate.none"));
                return;
            }
            let n = cols.len();
            let list = cols.iter().map(|col| json!({ "type": "baseplate", "column": col, "params": params })).collect();
            add_all(self, &t0("app.undo.similar.baseplate"), list);
            self.say(t("app.similar.columns", &[("n", F::N(n as f64))]));
        }
    }

    // ------------------------------------------------------------------------------------------ selection, delete

    /// detailcraft.js `selectAll()`.
    pub fn select_all(&mut self) {
        if self.tools.tool != "select" {
            let _ = self.set_tool("select");
        }
        let s = &self.model.state;
        let hidden = &self.hidden_phases;
        let vis = |x: &Value| !hidden.contains(&cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0));
        let id = |x: &Value| string_of(x.get("id"));
        let mem: Vec<Ref> = arr(s.get("members")).iter().filter(|x| vis(x)).map(|x| Ref::Member(id(x))).collect();
        let foot: Vec<Ref> = arr(s.get("footings")).iter().filter(|x| vis(x)).map(|x| Ref::Footing(id(x))).collect();
        let conns: Vec<Ref> = arr(s.get("connections"))
            .iter()
            .filter(|c| !hidden.contains(&cc_detail::model::conn_phase(s, c)))
            .map(|x| Ref::Conn(id(x)))
            .collect();
        let sts: Vec<Ref> = arr(s.get("stairs")).iter().filter(|x| vis(x)).map(|x| Ref::Stair(id(x))).collect();
        let counts = json!({ "members": mem.len(), "connections": conns.len(), "footings": foot.len(), "stairs": sts.len() });
        let refs: Vec<Ref> = mem.into_iter().chain(conns).chain(foot).chain(sts).collect();
        if refs.is_empty() {
            self.say(t0(if self.hidden_phases.is_empty() { "app.sel.all.empty" } else { "phase.all.hidden" }));
            return;
        }
        self.set_selection(refs);
        let mut m = t("app.sel.all.done", &[("what", F::S(count_text(&counts)))]);
        if !self.hidden_phases.is_empty() {
            m.push(' ');
            m.push_str(&t0("phase.all.note"));
        }
        self.say(m);
    }

    fn delete_refs(&mut self, refs: &[Ref], label: &str) -> bool {
        let cs = ids_of(refs, |r| matches!(r, Ref::Conn(_)));
        let ms = ids_of(refs, |r| matches!(r, Ref::Member(_)));
        let fs = ids_of(refs, |r| matches!(r, Ref::Footing(_)));
        let ss = ids_of(refs, |r| matches!(r, Ref::Stair(_)));
        let ok = self
            .edit(label, |tx| {
                tx.remove_connections(Some(&cs));
                tx.remove_members(Some(&ms));
                tx.remove_footings(Some(&fs));
                tx.remove_stairs(Some(&ss));
                Ok(())
            })
            .is_some();
        self.set_selection(Vec::new());
        ok
    }

    /// detailcraft.js `deleteSelection()`.
    pub fn delete_selection(&mut self) {
        let refs = self.selected();
        let Some(r) = refs.first() else {
            self.say(t0("app.delete.none"));
            return;
        };
        let label = if refs.len() > 1 {
            t("app.undo.delete.many", &[("n", F::N(refs.len() as f64))])
        } else {
            match r {
                Ref::Member(i) => t0(&format!("app.undo.delete.{}", self.member(i).map(|m| string_of(m.get("type"))).unwrap_or_default())),
                Ref::Conn(_) => t0("app.undo.delete.conn"),
                _ => t0("app.undo.delete.footing"),
            }
        };
        if self.delete_refs(&refs, &label) {
            self.say(if refs.len() > 1 { t("app.delete.done.many", &[("n", F::N(refs.len() as f64))]) } else { t0("app.delete.done.one") });
        }
    }

    // ------------------------------------------------------------------------------------------ clipboard

    /// detailcraft.js `takeCopy()`.
    fn take_copy(&mut self) -> Option<Value> {
        let refs = self.selected();
        if refs.is_empty() {
            self.say(t0("app.copy.none"));
            return None;
        }
        match cc_detail::clipboard::copy_clip(&self.model.state, &refs_json(&refs), self.settings.conns, "cadcraft-rust") {
            Some(r) => Some(r),
            None => {
                self.say(t0("app.copy.conn-only"));
                None
            }
        }
    }
    fn left_out_text(&self, counts: &Value) -> String {
        let n = counts.get("leftOut").and_then(Value::as_f64).unwrap_or(0.0);
        if n == 0.0 {
            return String::new();
        }
        format!(" {}", if self.settings.conns { t("app.copy.left-out", &[("n", F::N(n))]) } else { t0("app.copy.left-out.off") })
    }

    /// detailcraft.js `copySelection()`.
    pub fn copy_selection(&mut self) -> Option<Value> {
        let r = self.take_copy()?;
        match cc_detail::clipboard::read_clip(r.get("clip").unwrap_or(&Value::Null)) {
            Ok(c) => {
                self.tools.clip = Some(c);
                self.tools.clip_out = r.get("clip").map(cc_detail::js::stringify);
            }
            Err(e) => {
                self.say(e);
                return None;
            }
        }
        let counts = r.get("counts").cloned().unwrap_or(Value::Null);
        let m = t("app.copy.done", &[("what", F::S(count_text(&counts)))]) + &self.left_out_text(&counts);
        self.say(m);
        Some(r)
    }

    /// detailcraft.js `cutSelection()`.
    pub fn cut_selection(&mut self) {
        let refs = self.selected();
        let Some(r) = self.copy_selection() else { return };
        if self.delete_refs(&refs, &t0("app.undo.cut")) {
            let counts = r.get("counts").cloned().unwrap_or(Value::Null);
            let m = t("app.cut.done", &[("what", F::S(count_text(&counts)))]) + &self.left_out_text(&counts);
            self.say(m);
        }
    }

    /// detailcraft.js `startPaste(inPlace)`.
    pub fn start_paste(&mut self, in_place: bool) {
        let Some(c) = self.tools.clip.clone() else {
            self.say(t0("app.paste.empty"));
            return;
        };
        if in_place {
            if self.tools.tool != "select" {
                let _ = self.set_tool("select");
            }
            self.paste_clip_at(&c, [0.0; 3], "in-place", &[]);
            return;
        }
        let _ = self.set_tool("paste");
    }

    /// detailcraft.js `duplicate()`.
    pub fn duplicate(&mut self) {
        let Some(r) = self.take_copy() else { return };
        let o = self.settings.offset;
        match cc_detail::clipboard::read_clip(r.get("clip").unwrap_or(&Value::Null)) {
            Ok(c) => self.paste_clip_at(
                &c,
                o,
                "duplicate",
                &[("x", F::S(num_to_string(o[0]))), ("y", F::S(num_to_string(o[1]))), ("z", F::S(num_to_string(o[2])))],
            ),
            Err(e) => self.say(e),
        }
    }

    /// detailcraft.js `pasteClipAt(c, offset, how, fills)`: one undo step, the pasted things selected afterwards.
    pub fn paste_clip_at(&mut self, c: &Value, offset: [f64; 3], how: &str, fills: &[(&str, F)]) {
        let off = json!(offset);
        let over = cc_detail::clipboard::overlaps(&self.model.state, c, Some(&off));
        let label = t0(match how {
            "duplicate" => "app.undo.duplicate",
            "in-place" => "app.undo.paste-in-place",
            _ => "app.undo.paste",
        });
        let Some(r) = self.edit(&label, |tx| cc_detail::clipboard::paste_into(tx, c, Some(&off))) else { return };
        if self.tools.tool != "select" {
            let _ = self.set_tool("select");
        }
        let refs: Vec<Ref> = arr(r.get("refs")).iter().filter_map(ref_of_json).collect();
        let conns = refs.iter().filter(|x| matches!(x, Ref::Conn(_))).count();
        self.set_selection(refs);
        let n = json!({ "members": arr(c.get("members")).len(), "connections": conns, "footings": arr(c.get("footings")).len(), "stairs": arr(c.get("stairs")).len() });
        let mut vars: Vec<(&str, F)> = fills.to_vec();
        vars.push(("what", F::S(count_text(&n))));
        let mut m = t(&format!("app.paste.done.{how}"), &vars);
        if over > 0 {
            m.push(' ');
            m.push_str(&t("app.paste.over", &[("n", F::N(over as f64))]));
        }
        if r.get("profilesKept").and_then(Value::as_f64).is_some_and(|k| k != 0.0) {
            m.push(' ');
            m.push_str(&t0("app.paste.profiles-kept"));
        }
        let warnings: Vec<String> = arr(c.get("warnings")).iter().map(|w| string_of(Some(w))).collect();
        if !warnings.is_empty() {
            m.push(' ');
            m.push_str(&warnings.join(" "));
        }
        m.push(' ');
        m.push_str(&t0("app.paste.undo-hint"));
        self.say(m);
    }

    /// The paste tool's typed forms: "offset:x,y,z", "at:gy/gx/level", "in-place".
    pub fn paste_form(&mut self, spec: &str) -> Result<(), String> {
        let c = self.tools.paste_clip.clone().or_else(|| self.tools.clip.clone());
        let Some(c) = c else {
            self.say(t0("app.paste.empty"));
            return Ok(());
        };
        let (k, v) = spec.split_once(':').unwrap_or((spec, ""));
        match k {
            "in-place" => self.paste_clip_at(&c, [0.0; 3], "in-place", &[]),
            "offset" => {
                let d = self.settings.offset;
                let o: Vec<f64> = v.split(',').enumerate().map(|(k, x)| num_or(x, d.get(k).copied().unwrap_or(0.0))).collect();
                let [x, y, z] = o.as_slice() else { return Err("detail.paste: offset:x,y,z".into()) };
                self.paste_clip_at(
                    &c,
                    [*x, *y, *z],
                    "offset",
                    &[("x", F::S(num_to_string(*x))), ("y", F::S(num_to_string(*y))), ("z", F::S(num_to_string(*z)))],
                );
            }
            "at" => {
                let n = node_at(&self.model.state, v)?;
                let b = cc_detail::js::p3_of(c.get("base"));
                let node = cc_detail::model::node_name(&self.model.state, n.p);
                self.paste_clip_at(&c, [n.p[0] - b[0], n.p[1] - b[1], n.p[2] - b[2]], "at", &[("node", F::S(node))]);
            }
            _ => return Err(format!("detail.paste: unknown form {spec}")),
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ transforms

    /// detailcraft.js `runXf(refs, xf, keep, how)`.
    pub fn run_xf(&mut self, refs: &[Ref], xf: &cc_detail::transform::Xf, keep: bool, how: How) {
        let kind = how.kind();
        let r = if keep {
            let Some(c) = cc_detail::clipboard::copy_clip(&self.model.state, &refs_json(refs), self.settings.conns, "cadcraft-rust") else {
                self.say(t0("xf.none"));
                return;
            };
            let clip = match cc_detail::clipboard::read_clip(c.get("clip").unwrap_or(&Value::Null)) {
                Ok(c) => c,
                Err(e) => {
                    self.say(e);
                    return;
                }
            };
            self.edit(&t0(&format!("xf.undo.{kind}.copy")), |tx| {
                let p = cc_detail::clipboard::paste_into(tx, &clip, Some(&json!([0, 0, 0])))?;
                let st = tx.s.clone();
                let pr = arr(p.get("refs")).to_vec();
                let mut x = cc_detail::transform::transform_refs(tx, &st, &pr, xf)?;
                if let Some(o) = x.as_object_mut() {
                    o.insert("refs".into(), Value::Array(pr));
                }
                Ok(x)
            })
        } else {
            let rj = refs_json(refs);
            self.edit(&t0(&format!("xf.undo.{kind}")), |tx| {
                let st = tx.s.clone();
                let mut x = cc_detail::transform::transform_refs(tx, &st, &rj, xf)?;
                if let Some(o) = x.as_object_mut() {
                    o.insert("refs".into(), Value::Array(rj.clone()));
                }
                Ok(x)
            })
        };
        let Some(r) = r else { return };
        self.set_selection(arr(r.get("refs")).iter().filter_map(ref_of_json).collect());
        let g = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let what = what_text(g("members"), g("footings"));
        let d = match &how {
            How::Move(d) => *d,
            _ => [0.0; 3],
        };
        let (deg, about, line) = match &how {
            How::Rotate(deg, about) => (num_to_string(*deg), about.clone(), String::new()),
            How::Mirror(l) => (String::new(), String::new(), l.clone()),
            How::Move(_) => (String::new(), String::new(), String::new()),
        };
        let fills = [
            ("what", F::S(what)),
            ("x", F::S(mm(d[0]))),
            ("y", F::S(mm(d[1]))),
            ("z", F::S(mm(d[2]))),
            ("deg", F::S(deg)),
            ("about", F::S(about)),
            ("line", F::S(line)),
        ];
        let mut m = t(&format!("xf.done.{kind}{}", if keep { ".copy" } else { "" }), &fills);
        for n in arr(r.get("notes")) {
            m.push(' ');
            m.push_str(&t(&string_of(n.get("key")), &[("n", F::N(cc_detail::js::num(n.get("n"))))]));
        }
        m.push(' ');
        m.push_str(&t0("xf.undo.hint"));
        self.say(m);
    }

    /// `detail.xf`: the Properties' "Move, rotate, mirror" forms on the selection's members, footings and stairs:
    /// "move:dx,dy,dz", "rotate:deg" or "rotate:deg@gy/gx" (about a grid intersection), "mirror:x:<label>" /
    /// "mirror:y:<label>"; ":copy" at the end keeps the original.
    pub fn xf_form(&mut self, spec: &str) -> Result<(), String> {
        let (spec, keep) = match spec.strip_suffix(":copy") {
            Some(s) => (s, true),
            None => (spec, false),
        };
        let own: Vec<Ref> = self.selected().into_iter().filter(|r| !matches!(r, Ref::Conn(_))).collect();
        if own.is_empty() {
            return Err("detail.xf: select members, footings or stairs first".into());
        }
        let (k, v) = spec.split_once(':').ok_or("detail.xf: move:dx,dy,dz | rotate:deg[@gy/gx] | mirror:x|y:<label>")?;
        let s = self.model.state.clone();
        match k {
            "move" => {
                let off: Vec<Value> = v.split(',').map(|x| json!(num_or(x, 0.0))).collect();
                let num = |i: usize| off.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let d = [num(0), num(1), num(2)];
                let xf = if off.len() == 3 && d.iter().all(|x| x.is_finite()) {
                    cc_detail::transform::move_xf([off.first(), off.get(1), off.get(2)]).ok()
                } else {
                    None
                };
                match xf {
                    Some(xf) => self.run_xf(&own, &xf, keep, How::Move(d)),
                    None => self.say(t0("xf.bad.offset")),
                }
            }
            "rotate" => {
                let (deg, about) = v.split_once('@').unwrap_or((v, ""));
                let deg = num_or(deg, 90.0);
                let c = if about.is_empty() {
                    cc_detail::transform::selection_center(&s, &refs_json(&own)).ok().flatten().map(|m| [m[0], m[1]])
                } else {
                    let (gy, gx) = about.split_once('/').ok_or("detail.xf: rotate:deg@gy/gx")?;
                    let l0 = arr(s.get("levels")).first().and_then(|l| l.get("name")).cloned().unwrap_or(Value::Null);
                    cc_detail::model::point_at(&s, &Value::from(gy), &Value::from(gx), &l0).map(|p| [p[0], p[1]])
                };
                let Some(c) = c else { return Ok(()) };
                match cc_detail::transform::rotate_xf(&[json!(c[0]), json!(c[1])], Some(&json!(deg))) {
                    Ok(xf) => {
                        let about = if about.is_empty() { t0("xf.rotate.middle.short") } else { about.to_string() };
                        self.run_xf(&own, &xf, keep, How::Rotate(deg, about));
                    }
                    Err(_) => self.say(t0("xf.bad.angle")),
                }
            }
            "mirror" => {
                let (ax, label) = v.split_once(':').ok_or("detail.xf: mirror:x:<label> | mirror:y:<label>")?;
                let g = arr(s.get("grids").and_then(|g| g.get(ax)))
                    .iter()
                    .find(|q| q.get("label").and_then(Value::as_str) == Some(label))
                    .cloned();
                let Some(g) = g else { return Ok(()) };
                let at = cc_detail::js::num(g.get("at"));
                let xf = if ax == "x" {
                    cc_detail::transform::mirror_xf(&[json!(at), json!(0)], &[json!(at), json!(1)])
                } else {
                    cc_detail::transform::mirror_xf(&[json!(0), json!(at)], &[json!(1), json!(at)])
                };
                if let Ok(xf) = xf {
                    self.run_xf(&own, &xf, keep, How::Mirror(label.into()));
                }
            }
            _ => return Err(format!("detail.xf: unknown transform {k}")),
        }
        Ok(())
    }

    /// `detail.many`: "Change on all of them" (transform.js `applyToMany`): "<field>=<value>" on the selected members.
    pub fn many(&mut self, spec: &str) -> Result<(), String> {
        let (key, value) = spec.split_once('=').ok_or("detail.many: \"<field>=<value>\"")?;
        if !cc_detail::transform::MANY_FIELDS.contains(&key) {
            return Err(format!("detail.many: unknown field {key}"));
        }
        let ids: Vec<Value> = self.selected_member_ids().into_iter().map(Value::from).collect();
        if ids.len() < 2 {
            return Err("detail.many: select two or more members".into());
        }
        let v = if matches!(key, "plateT" | "plateW" | "rotation") {
            let n = str_to_number(value);
            if value.trim().is_empty() || !n.is_finite() {
                self.say(t0("app.num.bad"));
                return Ok(());
            }
            json!(n)
        } else {
            json!(value)
        };
        let mk = many_key(key);
        let field = t0(&mk);
        if let Some(r) = self.edit(&t("many.undo", &[("field", F::S(field.clone()))]), |tx| {
            let st = tx.s.clone();
            cc_detail::transform::apply_to_many(tx, &st, &ids, key, Some(&v))
        }) {
            let g = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let mut m = t("many.done", &[("field", F::S(field)), ("n", F::N(g("changed")))]);
            if g("skipped") != 0.0 {
                m.push(' ');
                m.push_str(&t("many.skipped", &[("n", F::N(g("skipped")))]));
            }
            self.say(m);
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ model setup

    /// The left panel's grid form (model.js `gridsFromSpacings`): "<numbered>|<spacings>|<lettered>|<spacings>".
    pub fn set_grids(&mut self, spec: &str) -> Result<(), String> {
        let mut it = spec.split('|');
        let (Some(a), Some(b), Some(c), Some(d)) = (it.next(), it.next(), it.next(), it.next()) else {
            return Err("detail.grids: \"1 2 3|6000 6000|A B|6000\"".into());
        };
        let g = match cc_detail::model::grids_from_spacings(a, b, c, d) {
            Ok(g) => g,
            Err(e) => {
                self.say(e);
                return Ok(());
            }
        };
        if self.edit(&t0("app.undo.grids"), |tx| tx.set_grids(Some(&g))).is_some() {
            self.say(t0("app.grids.done"));
            self.fit();
        }
        Ok(())
    }

    /// The levels form: "name=z;name=z" (the z as typed; model.js `setLevels` reads it).
    pub fn set_levels(&mut self, spec: &str) {
        let rows: Vec<Value> = spec
            .split(';')
            .filter(|r| !r.is_empty())
            .map(|r| {
                let (n, z) = r.rsplit_once('=').unwrap_or((r, ""));
                json!({ "name": n, "z": z })
            })
            .collect();
        if self.edit(&t0("app.undo.levels"), |tx| tx.set_levels(Some(&Value::Array(rows)))).is_some() {
            self.say(t0("app.levels.done"));
        }
    }

    /// detailcraft.js `phaseName(n)`.
    pub fn phase_name(&self, n: f64) -> String {
        arr(self.model.state.get("phases"))
            .iter()
            .find(|p| p.get("n").and_then(Value::as_f64) == Some(n))
            .and_then(|p| p.get("name").and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map_or_else(|| t("phase.default", &[("n", F::N(n))]), str::to_string)
    }

    /// `detail.phases`: "add", "rename:<n>=<name>", "remove:<n>", "current:<n>", "per:true|false", "of:<n>" (the
    /// selection's members, footings and stairs into phase n).
    pub fn phase_op(&mut self, spec: &str) -> Result<(), String> {
        let (k, v) = spec.split_once(':').unwrap_or((spec, ""));
        let num = |s: &str| -> Result<f64, String> {
            let n = str_to_number(s.trim());
            if n.is_finite() { Ok(n) } else { Err(format!("detail.phases: a phase number, not {s}")) }
        };
        match k {
            "add" => {
                if let Some(n) = self.edit(&t0("phase.undo.add"), |tx| tx.add_phase(None)) {
                    self.tools.cur_phase = n;
                    self.say(t("phase.added", &[("phase", F::S(self.phase_name(n)))]));
                }
            }
            "rename" => {
                let (n, name) = v.split_once('=').ok_or("detail.phases: rename:<n>=<name>")?;
                let n = num(n)?;
                self.edit(&t0("phase.undo.rename"), |tx| tx.rename_phase(Some(&json!(n)), Some(&json!(name))));
            }
            "remove" => {
                let n = num(v)?;
                if self.edit(&t0("phase.undo.remove"), |tx| tx.remove_phase(Some(&json!(n)))).is_some() {
                    self.say(t("phase.removed", &[("phase", F::S(self.phase_name(n)))]));
                }
            }
            "current" => {
                let n = num(v)?;
                self.tools.cur_phase = n;
                self.say(t("phase.current.set", &[("phase", F::S(self.phase_name(n)))]));
            }
            "per" => {
                let on = v == "true";
                if self
                    .edit(&t0("phase.undo.numbering"), |tx| {
                        tx.set_numbering(Some(&json!({ "perPhase": on })));
                        Ok(())
                    })
                    .is_some()
                {
                    self.say(t0(if on { "phase.per.on" } else { "phase.per.off" }));
                }
            }
            "of" => {
                let n = num(v)?;
                let own: Vec<Ref> = self.selected().into_iter().filter(|r| !matches!(r, Ref::Conn(_))).collect();
                let refs = Value::Array(refs_json(&own));
                if self.edit(&t0("phase.undo.set"), |tx| tx.set_phase_of(Some(&refs), Some(&json!(n)))).is_some() {
                    let nm = own.iter().filter(|r| matches!(r, Ref::Member(_))).count() as f64;
                    let nf = own.iter().filter(|r| matches!(r, Ref::Footing(_))).count() as f64;
                    self.say(t("phase.set", &[("what", F::S(what_text(nm, nf))), ("phase", F::S(self.phase_name(n)))]));
                }
            }
            _ => return Err(format!("detail.phases: unknown action {k}")),
        }
        Ok(())
    }

    /// detailcraft.js `setPhaseShown(n, on)`.
    pub fn set_phase_shown(&mut self, n: f64, on: bool) {
        if on {
            self.hidden_phases.retain(|x| *x != n);
        } else if !self.hidden_phases.contains(&n) {
            self.hidden_phases.push(n);
        }
        let s = &self.model.state;
        let phase_of = |r: &Ref| -> f64 {
            let find = |k: &str| arr(s.get(k)).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(r.id())).cloned();
            match r {
                Ref::Conn(_) => find("connections").map_or(1.0, |c| cc_detail::model::conn_phase(s, &c)),
                Ref::Member(_) => find("members").map_or(1.0, |x| cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0)),
                Ref::Stair(_) => find("stairs").map_or(1.0, |x| cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0)),
                Ref::Footing(_) => find("footings").map_or(1.0, |x| cc_detail::js::or(cc_detail::js::f(x.get("phase")), 1.0)),
            }
        };
        let keep: Vec<Ref> = self.selected().into_iter().filter(|r| !self.hidden_phases.contains(&phase_of(r))).collect();
        self.set_selection(keep);
        self.say(t(if on { "phase.shown" } else { "phase.hidden" }, &[("phase", F::S(self.phase_name(n)))]));
    }

    /// `detail.hole`: "<member>:add:<face>,<x>,<y>,<d>" or "<member>:remove:<hole id>".
    pub fn hole_op(&mut self, spec: &str) -> Result<(), String> {
        let mut it = spec.splitn(3, ':');
        let (Some(m), Some(k), Some(v)) = (it.next(), it.next(), it.next()) else {
            return Err("detail.hole: <member>:add:face,x,y,d | <member>:remove:<hole id>".into());
        };
        let mv = json!(m);
        match k {
            "add" => {
                let f: Vec<&str> = v.split(',').collect();
                let [face, x, y, d] = f.as_slice() else { return Err("detail.hole: add:face,x,y,d".into()) };
                // a field that does not read keeps the form's starting value (numIn leaves it unchanged)
                let h = json!({ "face": face, "x": num_or(x, 100.0), "y": num_or(y, 0.0), "d": num_or(d, 22.0) });
                if self.edit(&t0("app.undo.add-hole"), |tx| tx.add_hole(Some(&mv), Some(&h))).is_some() {
                    self.say(t0("app.holes.added"));
                }
            }
            "remove" => {
                let hv = json!(v);
                self.edit(&t0("app.undo.remove-hole"), |tx| {
                    tx.remove_hole(Some(&mv), Some(&hv));
                    Ok(())
                });
            }
            _ => return Err(format!("detail.hole: unknown action {k}")),
        }
        Ok(())
    }

    /// `detail.cope`: "<member>:add:<end>,<side>,<length>,<depth>" or "<member>:remove:<cope id>".
    pub fn cope_op(&mut self, spec: &str) -> Result<(), String> {
        let mut it = spec.splitn(3, ':');
        let (Some(m), Some(k), Some(v)) = (it.next(), it.next(), it.next()) else {
            return Err("detail.cope: <member>:add:end,side,length,depth | <member>:remove:<cope id>".into());
        };
        let mv = json!(m);
        match k {
            "add" => {
                let f: Vec<&str> = v.split(',').collect();
                let [end, side, length, depth] = f.as_slice() else { return Err("detail.cope: add:end,side,length,depth".into()) };
                let c = json!({ "kind": "cope", "end": end, "side": side, "length": num_or(length, 100.0), "depth": num_or(depth, 40.0) });
                self.edit(&t0("app.undo.add-cope"), |tx| tx.add_cut(Some(&mv), Some(&c)));
            }
            "remove" => {
                let cv = json!(v);
                self.edit(&t0("app.undo.remove-cope"), |tx| {
                    tx.remove_cut(Some(&mv), Some(&cv));
                    Ok(())
                });
            }
            _ => return Err(format!("detail.cope: unknown action {k}")),
        }
        Ok(())
    }

    /// `detail.profile`: the custom profile form, "<name>;<kind>;<dim>=<value>;…" (an empty value is NaN, as there).
    pub fn add_profile(&mut self, spec: &str) -> Result<(), String> {
        let mut it = spec.split(';');
        let (Some(name), Some(kind)) = (it.next(), it.next()) else { return Err("detail.profile: <name>;<kind>;<dim>=<value>…".into()) };
        let mut p = serde_json::Map::new();
        p.insert("name".into(), json!(name));
        p.insert("kind".into(), json!(kind));
        let keys = cc_detail::profiles::custom_kind_keys(kind).unwrap_or(&[]);
        let given: Vec<(&str, &str)> = it.filter_map(|kv| kv.split_once('=')).collect();
        for k in keys {
            let v = given.iter().find(|(a, _)| a == k).map_or("", |(_, b)| *b);
            let n = str_to_number(v);
            // `value === '' ? NaN : Number(value)`: NaN has no JSON form; null reads as not a number in cleanCustom too
            p.insert((*k).into(), if v.is_empty() || !n.is_finite() { Value::Null } else { json!(n) });
        }
        let pv = Value::Object(p);
        if self.edit(&t("app.undo.add-profile", &[("name", F::S(name.into()))]), |tx| tx.add_profile(Some(&pv))).is_some() {
            self.say(t("app.profile.added", &[("name", F::S(name.trim().into()))]));
        }
        Ok(())
    }

    /// The model summary's "Remove them" for loose bolts from an IFC import.
    pub fn remove_loose_bolts(&mut self) {
        let n = arr(self.model.state.get("bolts")).len();
        if self
            .edit(&t0("app.undo.bolts-remove"), |tx| {
                tx.remove_bolts(None);
                Ok(())
            })
            .is_some()
        {
            self.say(t("app.bolts.removed", &[("n", F::N(n as f64))]));
        }
    }

    /// `detail.set` on a footing ("f1.L=2400", "f1.mat.barX=#6"), a stair ("s1.tread.t=8") or a connection's parameter
    /// ("j2.tp=25"): one undo step each, as the Properties' fields make them. None: the id is none of those.
    pub fn set_other(&mut self, id: &str, field: &str, value: &str) -> Option<Result<(), String>> {
        let has = |k: &str| arr(self.model.state.get(k)).iter().any(|x| x.get("id").and_then(Value::as_str) == Some(id));
        let kind = ["footings", "stairs", "connections"].into_iter().find(|k| has(k))?;
        let number = || -> Result<f64, String> {
            let v = str_to_number(value);
            if value.trim().is_empty() || !v.is_finite() { Err(t0("app.num.bad")) } else { Ok(v) }
        };
        let r = (|| -> Result<(), String> {
            let idv = json!(id);
            match kind {
                "footings" => {
                    let v = match field {
                        "x"
                        | "y"
                        | "top"
                        | "rotation"
                        | "L"
                        | "B"
                        | "H"
                        | "mat.sX"
                        | "mat.sY"
                        | "mat.cover"
                        | "starters.nx"
                        | "starters.ny"
                        | "starters.sx"
                        | "starters.sy"
                        | "starters.projection" => json!(number()?),
                        "mat.hooks" | "starters.on" | "starters.hook" => json!(value == "true"),
                        "concrete" | "mat.barX" | "mat.barY" | "starters.bar" => json!(value),
                        "phase" => return self.phase_of_one(Ref::Footing(id.into()), number()?),
                        _ => return Err(format!("detail.set: unknown footing field {field}")),
                    };
                    let patch = nested(field, v);
                    self.edit(&t0("app.undo.footing"), |tx| tx.update_footing(Some(&idv), Some(&patch)));
                }
                "stairs" => {
                    let v = match field {
                        "x" | "y" | "z" | "dir" | "rise" | "going" | "width" | "maxRiser" | "tread.t" | "tread.nosing" => json!(number()?),
                        "stringer" | "material" => json!(value),
                        "phase" => return self.phase_of_one(Ref::Stair(id.into()), number()?),
                        _ => return Err(format!("detail.set: unknown stair field {field}")),
                    };
                    let patch = nested(field, v);
                    self.edit(&t0("app.undo.stair"), |tx| tx.update_stair(Some(&idv), Some(&patch)));
                }
                _ => {
                    let c = self.connection(id).unwrap_or(Value::Null);
                    let cur =
                        c.get("params").and_then(|p| p.get(field)).ok_or_else(|| format!("detail.set: {id} has no parameter {field}"))?;
                    let v = if param_choices(field).is_some() || cur.is_number() { json!(number()?) } else { json!(value) };
                    let mut patch = serde_json::Map::new();
                    patch.insert(field.into(), v);
                    let patch = Value::Object(patch);
                    let what = param_label(field).to_lowercase();
                    self.edit(&t("app.undo.change", &[("what", F::S(what))]), |tx| tx.update_connection(Some(&idv), Some(&patch)));
                }
            }
            Ok(())
        })();
        if let Err(e) = &r {
            self.say(e.clone());
        }
        Some(r)
    }

    /// The phase field of one footing or stair (detailcraft.js `phaseField`).
    pub fn phase_of_one(&mut self, r: Ref, n: f64) -> Result<(), String> {
        let refs = Value::Array(vec![ref_json(&r)]);
        if self.edit(&t0("phase.undo.set"), |tx| tx.set_phase_of(Some(&refs), Some(&json!(n)))).is_some() {
            let nm = if matches!(r, Ref::Member(_)) { 1.0 } else { 0.0 };
            let nf = if matches!(r, Ref::Footing(_)) { 1.0 } else { 0.0 };
            self.say(t("phase.set", &[("what", F::S(what_text(nm, nf))), ("phase", F::S(self.phase_name(n)))]));
        }
        Ok(())
    }

    /// The toolbar's actions (detailcraft.js `act(a)` for what is not a tool or a file).
    pub fn act(&mut self, a: &str) {
        match a {
            "endplates" => self.add_end_plates(),
            "baseplate" => self.add_base_plate(),
            "sheartabs" => self.add_shear_tabs(),
            "splice" => self.add_splices(),
            "gusset" => self.add_gussets(),
            "similar" => self.apply_similar(),
            "delete" => self.delete_selection(),
            "copy" => {
                self.copy_selection();
            }
            "cut" => self.cut_selection(),
            "paste" => self.start_paste(false),
            "paste-in-place" => self.start_paste(true),
            "duplicate" => self.duplicate(),
            "move" => {
                let _ = self.set_tool("move");
            }
            "select-all" => self.select_all(),
            "select-none" => {
                let had = !self.selected().is_empty();
                self.set_selection(Vec::new());
                self.say(t0(if had { "app.sel.none.done" } else { "app.sel.none.already" }));
            }
            "remove-bolts" => self.remove_loose_bolts(),
            _ => self.say(format!("detail.act: unknown action {a}")),
        }
    }

    /// detailcraft.js `windowPicked(w)`: what a selection window or lasso takes (winsel.js), added with Shift.
    pub fn window_picked(
        &mut self,
        project: &dyn Fn([f64; 3]) -> cc_detail::transform::Screen,
        path: &[cc_detail::lasso_core::Pt],
        mode: &str,
        add: bool,
        planes: &[cc_detail::winsel::Plane],
    ) {
        if self.tools.tool != "select" {
            return;
        }
        let t1 = web_time::Instant::now();
        let items = cc_detail::winsel::selection_items(&self.model.state, &self.built, Some(&self.hidden_phases));
        let got: Vec<Ref> = cc_detail::winsel::window_select(&items, project, path, mode, planes).iter().filter_map(ref_of_json).collect();
        self.set_timing("select", t1.elapsed().as_secs_f64() * 1000.0);
        let kind = match mode {
            "fence" => "fence",
            "crossing" => "crossing",
            _ => "window",
        };
        if got.is_empty() {
            if !add {
                self.set_selection(Vec::new());
            }
            self.say(t0(&format!("sel.{kind}.none")));
            return;
        }
        let n = got.len();
        let next = if add { self.selected().into_iter().chain(got).collect() } else { got };
        self.set_selection(next);
        let mut m = t(&format!("sel.{kind}.got"), &[("n", F::N(n as f64))]);
        if add {
            m.push(' ');
            m.push_str(&t("sel.total", &[("n", F::N(self.selected().len() as f64))]));
        }
        self.say(m);
    }
}

/// A typed number, or the field's value before (numIn keeps it when the text does not read).
fn num_or(s: &str, d: f64) -> f64 {
    let v = str_to_number(s.trim());
    if s.trim().is_empty() || !v.is_finite() { d } else { v }
}

/// `{ a: { b: v } }` from "a.b" and v.
fn nested(path: &str, v: Value) -> Value {
    match path.split_once('.') {
        Some((a, b)) => {
            let mut m = serde_json::Map::new();
            m.insert(a.into(), nested(b, v));
            Value::Object(m)
        }
        None => {
            let mut m = serde_json::Map::new();
            m.insert(path.into(), v);
            Value::Object(m)
        }
    }
}

/// detailcraft.js `manyKey(f)`: plateT → many.plate-t.
pub fn many_key(f: &str) -> String {
    let mut s = String::from("many.");
    for c in f.chars() {
        if c.is_ascii_uppercase() {
            s.push('-');
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter() -> DetailView {
        DetailView::empty()
    }

    #[test]
    fn nodes_read_both_ways() {
        let v = starter();
        let a = node_at(&v.model.state, "A/1/Level 1").unwrap();
        assert_eq!(a.p, [0.0, 0.0, 0.0]);
        let b = node_at(&v.model.state, "6000,6000,4000").unwrap();
        assert_eq!((b.gy.as_str(), b.gx.as_str(), b.level.as_str()), ("B", "2", "Level 2"));
        assert!(node_at(&v.model.state, "Z/9/Level 1").is_err());
        assert!(node_at(&v.model.state, "1,2,3").is_err());
        assert!(node_at(&v.model.state, "nonsense").is_err());
    }

    #[test]
    fn placing_a_beam_takes_two_clicks_and_one_undo_step() {
        let mut v = starter();
        v.set_tool("beam").unwrap();
        assert_eq!(v.message, t0("app.tool.how.beam"));
        let a = node_at(&v.model.state, "A/1/Level 2").unwrap();
        v.place_at(&a);
        assert!(v.tools.pending.is_some());
        v.place_at(&a);
        assert_eq!(v.message, t0("app.place.same"));
        let b = node_at(&v.model.state, "A/2/Level 2").unwrap();
        v.place_at(&b);
        assert_eq!(arr(v.model.state.get("members")).len(), 1);
        assert_eq!(v.model.undo_stack.len(), 1);
        assert!(v.message.starts_with("Beam W16x26"), "{}", v.message);
        // a column from a level with nothing above is refused, nothing changes
        v.set_tool("column").unwrap();
        v.place_at(&b);
        assert_eq!(arr(v.model.state.get("members")).len(), 1);
        v.place_at(&node_at(&v.model.state, "A/1/Level 1").unwrap());
        assert_eq!(arr(v.model.state.get("members")).len(), 2);
        v.set_tool("footing").unwrap();
        v.place_at(&node_at(&v.model.state, "A/1/Level 1").unwrap());
        v.set_tool("stair").unwrap();
        v.place_at(&node_at(&v.model.state, "B/2/Level 1").unwrap());
        assert_eq!(arr(v.model.state.get("footings")).len(), 1);
        assert_eq!(arr(v.model.state.get("stairs")).len(), 1);
        assert_eq!(v.model.undo_stack.len(), 4);
    }

    #[test]
    fn options_are_checked() {
        let mut o = Opts::new("us");
        o.set("beam.profile=W21x44").unwrap();
        o.set("footing.mat.barX=#6").unwrap();
        o.set("stair.tread.t=8").unwrap();
        assert_eq!(o.beam.profile, "W21x44");
        assert_eq!(o.footing["mat"]["barX"], "#6");
        assert_eq!(o.stair["tread"]["t"], 8.0);
        assert_eq!(o.set("beam.rotation=abc"), Err(t0("app.num.bad")));
        assert!(o.set("beam.nope=1").is_err());
        assert!(o.set("footing.mat.hooks=maybe").is_err());
        let e = Opts::new("eu");
        assert_eq!((e.beam.profile.as_str(), e.footing["concrete"].as_str()), ("IPE300", Some("C30/37")));
    }

    #[test]
    fn copy_paste_duplicate_and_transforms_are_single_steps() {
        let mut v = DetailView::sample().unwrap();
        let n0 = arr(v.model.state.get("members")).len();
        v.act("paste");
        assert_eq!(v.message, t0("app.paste.empty"));
        v.select_all();
        assert!(v.multi.len() > 10);
        v.act("copy");
        assert!(v.tools.clip.is_some());
        v.act("duplicate");
        assert_eq!(arr(v.model.state.get("members")).len(), 2 * n0);
        assert_eq!(v.model.undo_stack.len(), 1);
        v.undo();
        v.start_paste(false);
        assert_eq!(v.tools.tool, "paste");
        v.place_at(&node_at(&v.model.state, "A/1/Level 1").unwrap_or_else(|_| all_nodes(&v.model.state).remove(0)));
        assert_eq!(v.tools.tool, "select");
        assert_eq!(arr(v.model.state.get("members")).len(), 2 * n0);
        v.undo();
        let beam = arr(v.model.state.get("members")).iter().find(|m| m["type"] == "beam").unwrap()["id"].as_str().unwrap().to_string();
        v.set_selection(vec![Ref::Member(beam.clone())]);
        v.xf_form("move:0,0,100").unwrap();
        assert!(v.message.starts_with(
            &t(
                "xf.done.move",
                &[("what", F::S(what_text(1.0, 0.0))), ("x", F::S("0".into())), ("y", F::S("0".into())), ("z", F::S("100".into()))]
            )[..20]
        ));
        v.xf_form("rotate:90:copy").unwrap();
        v.xf_form("move:2000000,0,0").unwrap();
        assert_eq!(v.message, t0("xf.bad.offset"));
        v.act("delete");
        assert_eq!(v.message, t0("app.delete.done.one"));
        assert!(v.selected().is_empty());
    }

    #[test]
    fn connections_go_through_the_engine() {
        let mut v = starter();
        let s = &v.model.state;
        let c1 = json!({ "type": "column", "profile": "W12x50", "start": node_at(s, "A/1/Level 1").unwrap().p, "end": node_at(s, "A/1/Level 2").unwrap().p });
        let c2 = json!({ "type": "column", "profile": "W12x50", "start": node_at(s, "A/2/Level 1").unwrap().p, "end": node_at(s, "A/2/Level 2").unwrap().p });
        let b = json!({ "type": "beam", "profile": "W16x26", "start": node_at(s, "A/1/Level 2").unwrap().p, "end": node_at(s, "A/2/Level 2").unwrap().p });
        let ids = v.edit("x", |tx| Ok(vec![tx.add_member(Some(&c1))?, tx.add_member(Some(&c2))?, tx.add_member(Some(&b))?])).unwrap();
        v.set_selection(vec![Ref::Member(ids[2].as_str().unwrap().into())]);
        v.add_end_plates();
        assert_eq!(arr(v.model.state.get("connections")).len(), 2, "{}", v.message);
        assert!(matches!(v.sel, Some(Ref::Conn(_))));
        v.set_selection(vec![Ref::Member(ids[0].as_str().unwrap().into())]);
        v.add_base_plate();
        v.apply_similar();
        assert_eq!(arr(v.model.state.get("connections")).len(), 4, "{}", v.message);
        v.set_selection(vec![Ref::Member(ids[0].as_str().unwrap().into())]);
        v.add_base_plate();
        assert_eq!(v.message, t0("app.baseplate.has"));
    }

    #[test]
    fn grids_levels_phases() {
        let mut v = starter();
        v.set_grids("1 2 3 4|6000 6000 6000|A B C|7000 7000").unwrap();
        assert_eq!(v.message, t0("app.grids.done"));
        v.set_grids("1 2|6000 6000|A|").unwrap();
        assert_eq!(arr(v.model.state["grids"].get("x")).len(), 4, "a bad grid changes nothing");
        v.set_levels("Ground=0;First=4500;Roof=9000");
        assert_eq!(arr(v.model.state.get("levels")).len(), 3);
        v.phase_op("add").unwrap();
        assert_eq!(v.tools.cur_phase, 2.0);
        v.phase_op("rename:2=Erection 2").unwrap();
        assert_eq!(v.phase_name(2.0), "Erection 2");
        v.phase_op("remove:2").unwrap();
        assert_eq!(arr(v.model.state.get("phases")).len(), 1);
        v.phase_op("remove:1").unwrap();
        assert_eq!(arr(v.model.state.get("phases")).len(), 1, "the last phase stays");
        assert!(v.phase_op("bogus").is_err());
        assert_eq!(many_key("plateT"), "many.plate-t");
    }
}
