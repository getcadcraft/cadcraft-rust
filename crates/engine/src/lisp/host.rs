//! The drawing side of CadCraft's LISP (host-cad.js): what cc-lisp's built-ins ask of the drawing, answered from the
//! Doc and the command line. Every LISP run happens inside an engine command, so get* prompts are the command line's
//! own prompts and everything a routine changes is one undo step.
//!
//! (command …) runs a command inside the routine's own: its future is polled from here, and the routine's values
//! answer its prompts through the same parsing typed text and clicks get ([`crate::engine::Shared::text_act`],
//! [`crate::engine::Shared::click_act`]). PAUSE hands the prompt to the person: the routine waits for the next answer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use cc_cad::entity::{Entity, Geo, Obj};
use cc_cad::{Doc, MODEL, SpaceId, js};
use cc_cad_geom::geom::{Bounds, intersect, prim_point, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{SNAP_KINDS, empty_bounds, entity_bounds, fence_hits, grow_pt, point_in_poly, snap_points, valid_bounds};
use cc_geom::P2;
use cc_lisp::LErr;
use cc_lisp::host::{EntselAns, GetAns, GetKind, GetOpts, HR, HostErr, MakeAns, Pairs, Pv, VlaOp};
use cc_lisp::interp::LFut;
use cc_lisp::value::EId;
use serde_json::{Value, json};

use super::dxflist::{self, FromCtx, PairCtx};
use super::{LispSession, is_lisp};
use crate::cmd::CmdDef;
use crate::engine::{Act, Ctx, Opts, Reply, Shared};
use crate::input::Opt;
use crate::prompt::{Answer, CmdError, CmdFuture, Kind, Prompter};
use crate::ucs::{active_ucs, to_ucs, to_world, vec_to_ucs, vec_to_world};

pub const PAUSE: &str = "\\";

/// A command (command …) started and that still runs.
struct Nested {
    name: &'static str,
    fut: Option<CmdFuture>,
}

/// An entmake BLOCK … ENDBLK being made.
struct Making {
    name: String,
    base: P2,
    entities: Vec<Entity>,
}

#[derive(Default)]
struct CadState {
    sys: HashMap<String, Pv>,
    errno: i32,
    deleted: HashMap<u64, (Entity, SpaceId)>,
    tbl_pos: HashMap<String, usize>,
    making: Option<Making>,
}

/// The drawing side.
pub struct CadHost {
    me: Weak<CadHost>,
    sh: Weak<Shared>,
    pr: Prompter,
    session: Weak<LispSession>,
    st: RefCell<CadState>,
    nested: RefCell<Option<Nested>>,
    /// when the session started (MILLISECS)
    t0: f64,
    /// the vla- objects' block-member serial numbers
    pub(crate) vla: RefCell<super::vlahost::VlaState>,
    /// a value typed into a prompt is being evaluated: nothing may ask the person
    pub prompt_eval: std::cell::Cell<bool>,
}

impl std::fmt::Debug for CadHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CadHost").finish()
    }
}

fn i(n: f64) -> Pv {
    Pv::Int(cc_lisp::value::to_i32(n))
}
fn pt3(x: f64, y: f64) -> Pv {
    Pv::List(vec![Pv::Num(x), Pv::Num(y), Pv::Num(0.0)])
}
fn cmd_err(e: CmdError) -> HostErr {
    match e {
        CmdError::Cancelled => HostErr::Cancel,
        CmdError::Msg(m) => HostErr::Msg(m),
    }
}

/// The defaults of the variables 0.5 keeps itself.
fn sysvar_defaults() -> HashMap<String, Pv> {
    let mut m: HashMap<String, Pv> = HashMap::new();
    let ints = [
        ("CMDECHO", 1),
        ("OSMODE", 4133),
        ("ORTHOMODE", 0),
        ("SNAPMODE", 0),
        ("GRIDMODE", 0),
        ("FILEDIA", 1),
        ("CMDDIA", 1),
        ("EXPERT", 0),
        ("PICKFIRST", 1),
        ("PICKADD", 2),
        ("HIGHLIGHT", 1),
        ("PDMODE", 0),
        ("AUNITS", 0),
        ("AUPREC", 0),
        ("ANGDIR", 0),
        ("DIMZIN", 0),
        ("NOMUTT", 0),
        ("QAFLAGS", 0),
        ("DELOBJ", 1),
        ("PEDITACCEPT", 0),
        ("ATTREQ", 1),
        ("ATTDIA", 0),
        ("ATTMODE", 1),
        ("MIRRTEXT", 0),
        ("WORLDUCS", 1),
        ("MEASUREMENT", 1),
        ("SAVETIME", 10),
        ("OSNAPCOORD", 2),
        ("UCSICON", 3),
        ("REGENMODE", 1),
        ("BLIPMODE", 0),
        ("TRIMMODE", 1),
        ("DWGTITLED", 0),
        ("PSLTSCALE", 1),
        ("MAXSORT", 1000),
    ];
    for (k, v) in ints {
        m.insert(k.into(), Pv::Int(v));
    }
    let reals = [
        ("PDSIZE", 0.0),
        ("ANGBASE", 0.0),
        ("ELEVATION", 0.0),
        ("THICKNESS", 0.0),
        ("CELTSCALE", 1.0),
        ("SNAPANG", 0.0),
        ("VIEWTWIST", 0.0),
        ("OFFSETDIST", -1.0),
        ("FILLETRAD", 0.0),
        ("CHAMFERA", 0.0),
        ("CHAMFERB", 0.0),
        ("POLARANG", std::f64::consts::PI / 2.0),
    ];
    for (k, v) in reals {
        m.insert(k.into(), Pv::Num(v));
    }
    for (k, v) in [("TEXTSTYLE", "Standard"), ("DIMSTYLE", "Standard"), ("LOCALE", "en")] {
        m.insert(k.into(), Pv::Str(v.into()));
    }
    m.insert("UCSORG".into(), Pv::pt(&[0.0, 0.0, 0.0]));
    m.insert("UCSXDIR".into(), Pv::pt(&[1.0, 0.0, 0.0]));
    m.insert("UCSYDIR".into(), Pv::pt(&[0.0, 1.0, 0.0]));
    m.insert("LIMMIN".into(), Pv::pt(&[0.0, 0.0]));
    m.insert("LIMMAX".into(), Pv::pt(&[420.0, 297.0]));
    for k in 1..=5 {
        m.insert(format!("USERI{k}"), Pv::Int(0));
        m.insert(format!("USERR{k}"), Pv::Num(0.0));
        m.insert(format!("USERS{k}"), Pv::Str(String::new()));
    }
    m
}
const SYSVARS_RO: [&str; 18] = [
    "CMDACTIVE",
    "CTAB",
    "DWGNAME",
    "DWGPREFIX",
    "EXTMIN",
    "EXTMAX",
    "ACADVER",
    "PRODUCT",
    "PLATFORM",
    "DATE",
    "CDATE",
    "MILLISECS",
    "LASTPOINT",
    "VIEWCTR",
    "VIEWSIZE",
    "SCREENSIZE",
    "UNDOCTL",
    "TILEMODE",
];

/// One initget keyword: what to type, the label, the value returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Kw {
    pub key: String,
    pub label: String,
    pub value: String,
}
/// initget keywords "Name eXit LType,LT _Name Exit LType" → keywords.
pub fn parse_keywords(spec: Option<&str>) -> Vec<Kw> {
    let Some(spec) = spec.filter(|s| !s.is_empty()) else { return Vec::new() };
    let mut parts = spec.split('_');
    let local = parts.next().unwrap_or("");
    let global = parts.next().unwrap_or("");
    let ws = |s: &str| s.split(|c: char| js::is_ws(c)).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let loc = ws(js::trim(local));
    let glob = ws(js::trim(global));
    loc.iter()
        .enumerate()
        .map(|(k, tok)| {
            let (full, key) = if tok.contains(',') {
                let mut it = tok.split(',');
                (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
            } else {
                let caps: String = tok.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect();
                let key =
                    if *tok == tok.to_uppercase() || *tok == tok.to_lowercase() || caps.is_empty() { tok.to_uppercase() } else { caps };
                (tok.clone(), key)
            };
            Kw { key: key.to_uppercase(), label: full.clone(), value: glob.get(k).cloned().unwrap_or(full) }
        })
        .collect()
}
/// A routine's prompt ("\nSelect block [Name/Exit] <Exit>: ") as the command line shows it (the engine adds ":").
pub fn clean_prompt(msg: Option<&str>, dflt: String) -> String {
    match msg {
        Some(m) if !js::trim(m).is_empty() => {
            let s = m.trim_start_matches(|c: char| js::is_ws(c));
            let s = s.trim_end_matches(|c: char| js::is_ws(c));
            let s = s.strip_suffix(':').unwrap_or(s);
            s.trim_end_matches(|c: char| js::is_ws(c)).to_string()
        }
        _ => dflt,
    }
}
fn osnap_override(s: &str) -> bool {
    let t = s.strip_prefix('_').unwrap_or(s).to_lowercase();
    matches!(
        t.as_str(),
        "non"
            | "none"
            | "end"
            | "endp"
            | "mid"
            | "int"
            | "cen"
            | "nea"
            | "per"
            | "qua"
            | "tan"
            | "ins"
            | "nod"
            | "app"
            | "ext"
            | "par"
            | "gcen"
            | "mtp"
    )
}
/// `numText(v)`: an integer as it is, else 15 significant digits.
fn num_text(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        return js::num_to_string(v);
    }
    js::num_to_string(js::str_to_number(&js::to_exponential(v, 14)))
}
fn describe(a: &Pv) -> String {
    match a {
        Pv::List(xs) => format!(
            "({})",
            xs.iter()
                .map(|x| match x {
                    Pv::Num(n) => num_text(*n),
                    Pv::Int(n) => n.to_string(),
                    other => dxflist::js_str(other),
                })
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Pv::Int(n) => n.to_string(),
        Pv::E(_) => "<Entity name>".into(),
        other => dxflist::js_str(other),
    }
}
/// A point on an object (a reference handed to an object prompt).
fn any_point(e: &Entity) -> P2 {
    let o = e.to_obj();
    let f = |k: &str| js::finite(o.get(k));
    if let (Some(x), Some(y)) = (f("x1"), o.get("y1").and_then(Value::as_f64)) {
        return P2::new(x, y);
    }
    if let Some(cx) = f("cx") {
        return P2::new(cx + js::or(o.get("r").and_then(Value::as_f64), 0.0), o.get("cy").and_then(Value::as_f64).unwrap_or(f64::NAN));
    }
    if let Some(p) = o.get("pts").and_then(Value::as_array).and_then(|a| a.first()) {
        return P2::new(js::to_number_opt(p.get("x")), js::to_number_opt(p.get("y")));
    }
    if let Some(x) = f("x") {
        return P2::new(x, o.get("y").and_then(Value::as_f64).unwrap_or(f64::NAN));
    }
    P2::new(0.0, 0.0)
}

/// Waits for the person once: the routine's command is polled again when an answer comes.
struct Turn(bool);
impl Future for Turn {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        Poll::Pending
    }
}

/// An object found by its entity name.
pub struct Found {
    pub e: Entity,
    /// a model / sheet object: its space and the space's name
    pub space: Option<(SpaceId, String)>,
    /// a member of a block definition: the block and the index
    pub block: Option<(String, usize)>,
}

impl CadHost {
    pub fn new(sh: Weak<Shared>, pr: Prompter, session: Weak<LispSession>) -> CadHost {
        let t0 = cc_clock::now_ms();
        let st = CadState { sys: sysvar_defaults(), ..CadState::default() };
        // `me` is set by `make`
        CadHost {
            me: Weak::new(),
            sh,
            pr,
            session,
            st: RefCell::new(st),
            nested: RefCell::new(None),
            t0,
            vla: RefCell::default(),
            prompt_eval: std::cell::Cell::new(false),
        }
    }
    /// The host in an Rc that knows itself.
    pub fn make(sh: Weak<Shared>, pr: Prompter, session: Weak<LispSession>) -> Rc<CadHost> {
        Rc::new_cyclic(|me| {
            let mut h = CadHost::new(sh, pr, session);
            h.me = me.clone();
            h
        })
    }
    pub(crate) fn shared(&self) -> Option<Rc<Shared>> {
        self.sh.upgrade()
    }
    fn ctx(&self) -> Option<Ctx> {
        Some(Ctx { sh: self.sh.upgrade()?, pr: self.pr.clone() })
    }
    fn print(&self, s: &str) {
        if let Some(sh) = self.shared() {
            sh.print(s);
        }
    }
    fn t(&self, key: &str, vars: &[(&str, String)]) -> String {
        self.shared().map_or_else(|| key.to_string(), |sh| sh.tv(key, vars))
    }
    /// The routine's buffered text prints before the command line asks anything.
    fn before_input(&self) {
        if let Some(s) = self.session.upgrade() {
            s.flush();
        }
    }

    // ---- objects by entity name ----

    pub fn space_name(doc: &Doc, s: SpaceId) -> String {
        if s == MODEL {
            return "Model".into();
        }
        doc.layouts.iter().find(|l| l.space == s).map_or_else(|| "Model".into(), |l| l.name.clone())
    }
    /// An object by entity name: model, sheets, the block being edited, or a block's member (`b:<block>:<i>`).
    pub fn find(doc: &Doc, id: &EId) -> Option<Found> {
        match id {
            EId::S(s) => {
                let rest = s.strip_prefix("b:")?;
                let k = rest.rfind(':')?;
                let name = rest.get(..k)?;
                let idx = js::str_to_number(rest.get(k.saturating_add(1)..)?);
                if !(idx >= 0.0 && idx.fract() == 0.0) {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let idx = idx as usize;
                let b = doc.blocks.get(&name.to_string())?;
                let e = b.entities.get(idx)?.clone();
                Some(Found { e, space: None, block: Some((name.to_string(), idx)) })
            }
            EId::N(n) => {
                if let Some(e) = doc.space_map(MODEL).and_then(|m| m.get(n)) {
                    return Some(Found { e: e.clone(), space: Some((MODEL, "Model".into())), block: None });
                }
                for l in &doc.layouts {
                    if let Some(e) = doc.space_map(l.space).and_then(|m| m.get(n)) {
                        return Some(Found { e: e.clone(), space: Some((l.space, l.name.clone())), block: None });
                    }
                }
                let editing = doc.space != MODEL && !doc.layouts.iter().any(|l| l.space == doc.space);
                if editing && let Some(e) = doc.get(*n) {
                    return Some(Found { e: e.clone(), space: Some((doc.space, "Model".into())), block: None });
                }
                None
            }
        }
    }
    fn handle(id: &EId) -> Option<String> {
        id.num().map(|n| format!("{n:X}"))
    }

    /// The tags a reference shows as ATTRIBs: its block's non-constant definitions with a value, then values with no
    /// definition.
    fn att_tags(doc: &Doc, e: &Entity) -> Vec<String> {
        let Geo::Insert(ins) = &e.geo else { return Vec::new() };
        let defs: Vec<Value> = doc.blocks.get(&ins.block).and_then(|b| b.attdefs.clone()).unwrap_or_default();
        let vals = ins.attrs.clone().unwrap_or_default();
        let mut tags: Vec<String> = defs
            .iter()
            .filter(|d| !js::truthy(d.get("constant")))
            .filter_map(|d| d.get("tag").map(js::to_js_string))
            .filter(|t| vals.contains_key(t))
            .collect();
        for k in cc_cad_geom::geom::js_keys(&vals) {
            if !tags.contains(k) {
                tags.push(k.clone());
            }
        }
        tags
    }
    /// `a:<insert>:<TAG>` (an ATTRIB) or `s:<insert>` (its SEQEND) → what it is and its DXF list.
    fn sub_of(doc: &Doc, id: &EId) -> Option<(bool, u64, Entity, SpaceId, Option<String>, Pairs)> {
        let s = id.text()?;
        let (kind, rest) = match s.strip_prefix("a:") {
            Some(r) => (true, r),
            None => (false, s.strip_prefix("s:")?),
        };
        let (num, tag) = match rest.find(':') {
            Some(k) => (rest.get(..k)?, Some(rest.get(k.saturating_add(1)..)?.to_string())),
            None => (rest, None),
        };
        if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if !kind && tag.is_some() {
            // "s:<id>:<x>" reads as an ATTRIB-less id with a tag: 0.5's pattern takes it (the tag is ignored)
        }
        let n: u64 = num.parse().ok()?;
        let f = CadHost::find(doc, &EId::N(n))?;
        let Geo::Insert(ins) = &f.e.geo else { return None };
        let (space, _) = f.space.clone()?;
        let owner = Pv::E(EId::N(n));
        let layer = f.e.layer.clone().unwrap_or_else(|| "0".into());
        if !kind {
            let pairs =
                vec![(-1, Pv::E(id.clone())), (0, Pv::Str("SEQEND".into())), (330, owner.clone()), (8, Pv::Str(layer)), (-2, owner)];
            return Some((false, n, f.e.clone(), space, None, pairs));
        }
        let tag = tag?;
        if !CadHost::att_tags(doc, &f.e).contains(&tag) {
            return None;
        }
        let b = doc.blocks.get(&ins.block);
        let d: Value = ins
            .attloc
            .as_ref()
            .and_then(|a| a.get(&tag).cloned())
            .filter(|v| js::truthy(Some(v)))
            .or_else(|| {
                b.and_then(|b| b.attdefs.as_ref())
                    .and_then(|a| a.iter().find(|x| x.get("tag").map(js::to_js_string).as_deref() == Some(tag.as_str())).cloned())
            })
            .unwrap_or_else(|| json!({ "x": 0, "y": 0, "h": 2.5, "rot": 0 }));
        let rot = js::or(ins.rot, 0.0);
        let (cs, sn) = (rot.jcos(), rot.jsin());
        let sx = ins.sx.unwrap_or(1.0);
        let sy = ins.sy.unwrap_or(1.0);
        let (bx, by) = b.map_or((0.0, 0.0), |b| (js::or(Some(b.base.x), 0.0), js::or(Some(b.base.y), 0.0)));
        let num0 = |k: &str| js::or(Some(js::to_number_opt(d.get(k))), 0.0);
        let lx = (num0("x") - bx) * sx;
        let ly = (num0("y") - by) * sy;
        let px = ins.x + lx * cs - ly * sn;
        let py = ins.y + lx * sn + ly * cs;
        let h = d.get("h").and_then(Value::as_f64).filter(|h| *h > 0.0).unwrap_or(2.5) * sy.abs();
        let val = ins.attrs.as_ref().and_then(|a| a.get(&tag)).filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default();
        let a50 = (js::or(d.get("rot").and_then(Value::as_f64), 0.0) + rot) % (2.0 * std::f64::consts::PI);
        let ha = match d.get("ha").and_then(Value::as_str) {
            Some("center") => 1,
            Some("right") => 2,
            _ => 0,
        };
        let va = match d.get("va").and_then(Value::as_str) {
            Some("bottom") => 1,
            Some("middle") => 2,
            Some("top") => 3,
            _ => 0,
        };
        let dl = d.get("layer").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_else(|| layer.clone());
        let mut pairs = vec![
            (-1, Pv::E(id.clone())),
            (0, Pv::Str("ATTRIB".into())),
            (330, owner),
            (100, Pv::Str("AcDbEntity".into())),
            (8, Pv::Str(dl)),
            (100, Pv::Str("AcDbText".into())),
            (10, pt3(px, py)),
            (40, Pv::Num(h)),
            (1, Pv::Str(val)),
            (50, Pv::Num(a50)),
            (41, Pv::Num(js::or(d.get("wf").and_then(Value::as_f64), 1.0))),
            (7, Pv::Str("Standard".into())),
            (72, Pv::Int(ha)),
            (11, pt3(px, py)),
            (100, Pv::Str("AcDbAttribute".into())),
            (2, Pv::Str(tag.clone())),
            (70, Pv::Int(i32::from(js::truthy(d.get("invisible"))))),
            (74, Pv::Int(va)),
        ];
        if let Some(c) = d.get("color").and_then(Value::as_f64) {
            pairs.insert(5, (62, i(c)));
        }
        Some((true, n, f.e.clone(), space, Some(tag), pairs))
    }

    /// Every object id, in drawing order: the model, then each sheet.
    fn order(doc: &Doc) -> Vec<u64> {
        let mut out: Vec<u64> = doc.space_entities(MODEL).filter_map(|e| e.id).collect();
        for l in &doc.layouts {
            out.extend(doc.space_entities(l.space).filter_map(|e| e.id));
        }
        out
    }
    fn last_id(doc: &Doc) -> Option<u64> {
        CadHost::order(doc).into_iter().max()
    }

    // ---- (command …) ----

    fn has_nested(&self) -> bool {
        self.nested.borrow().is_some()
    }
    /// Poll the running command once; when it ends, what 0.5's handlers do (print how it ended, clear the rubber band).
    fn poll_nested(&self) {
        let Some(mut fut) = self.nested.borrow_mut().as_mut().and_then(|n| n.fut.take()) else { return };
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Pending => {
                if let Some(n) = self.nested.borrow_mut().as_mut() {
                    n.fut = Some(fut);
                }
            }
            Poll::Ready(r) => {
                drop(fut);
                self.nested.borrow_mut().take();
                self.ended(r);
            }
        }
    }
    fn ended(&self, r: Result<(), CmdError>) {
        match r {
            Err(CmdError::Cancelled) => self.print("*Cancel*"),
            Err(CmdError::Msg(m)) => self.print(&format!("Error: {m}")),
            Ok(()) => {}
        }
        if let Some(sh) = self.shared() {
            {
                let mut st = sh.st.borrow_mut();
                st.preview_fn = None;
                st.base = None;
            }
            sh.host.borrow_mut().preview(Vec::new());
        }
    }
    fn start(&self, c: &Ctx, cmd: CmdDef) {
        c.sh.host.borrow_mut().set_selection(&[]);
        let echo = self.st.borrow().sys.get("CMDECHO").and_then(Pv::num).unwrap_or(1.0);
        if echo != 0.0 {
            self.print(&self.t("cmd.echo", &[("name", cmd.name.to_string())]));
        }
        let fut = (cmd.run)(c.clone());
        *self.nested.borrow_mut() = Some(Nested { name: cmd.name, fut: Some(fut) });
        self.poll_nested();
    }
    /// Esc on the running command.
    fn escape_nested(&self, c: &Ctx) {
        if c.sh.st.borrow_mut().waiting.take().is_none() && !self.has_nested() {
            return;
        }
        self.pr.deliver(Err(CmdError::Cancelled));
        self.poll_nested();
        // a command that swallows Esc and asks again is stopped
        if let Some(n) = self.nested.borrow_mut().take() {
            drop(n);
            self.pr.clear();
            c.sh.st.borrow_mut().waiting = None;
            self.ended(Err(CmdError::Cancelled));
        }
    }
    /// An answer for the running command's prompt (as typed, clicked) → it runs on.
    fn apply(&self, c: &Ctx, act: Act) {
        match act {
            Act::Resolve(v) => {
                if c.sh.take_reply(v) {
                    self.pr.deliver(Ok(Answer::None));
                    self.poll_nested();
                }
            }
            Act::Escape => self.escape_nested(c),
            Act::Stay => {}
        }
    }
    fn text(&self, c: &Ctx, s: &str) {
        let act = c.sh.text_act(s);
        self.apply(c, act);
    }
    fn click(&self, c: &Ctx, p: P2) {
        let (act, _) = c.sh.click_act(p, None, p);
        self.apply(c, act);
    }
    fn pick_ids(&self, c: &Ctx, ids: &[EId]) {
        let Some((kind, _, _)) = c.sh.waiting_info() else { return };
        match kind {
            Kind::Select => {
                let have: Vec<u64> = {
                    let d = c.sh.doc.borrow();
                    ids.iter().filter_map(EId::num).filter(|n| d.get(*n).is_some()).collect()
                };
                let set = c.sh.with_set(|s| {
                    for id in have {
                        s.add(id);
                    }
                });
                c.sh.host.borrow_mut().set_selection(&set.0);
                self.print(&format!("{} found", ids.len()));
            }
            Kind::Entity => {
                let e = ids.first().and_then(EId::num).and_then(|n| c.sh.doc.borrow().get(n).cloned());
                if let Some(e) = e
                    && let Some(id) = e.id
                {
                    self.apply(c, Act::Resolve(Reply::Pick { id, at: any_point(&e), shift: false }));
                }
            }
            _ => {}
        }
    }
    /// One of the routine's values for the running command's prompt.
    fn feed(&self, c: &Ctx, a: &Pv) {
        let Some((kind, _, _)) = c.sh.waiting_info() else { return };
        let uc = active_ucs(&c.sh.doc.borrow());
        match a {
            Pv::Str(s) => self.text(c, s),
            Pv::Num(_) | Pv::Int(_) => self.text(c, &num_text(a.num().unwrap_or(0.0))),
            Pv::List(xs) if xs.len() >= 2 && xs.iter().all(|x| matches!(x, Pv::Num(_) | Pv::Int(_))) => {
                let p = to_world(uc, P2::new(xs.first().and_then(Pv::num).unwrap_or(0.0), xs.get(1).and_then(Pv::num).unwrap_or(0.0)));
                if matches!(kind, Kind::Point | Kind::Distance | Kind::Angle | Kind::Select | Kind::Entity) {
                    self.click(c, p);
                } else {
                    self.text(c, &format!("{},{}", num_text(p.x), num_text(p.y)));
                }
            }
            Pv::E(id) => self.pick_ids(c, std::slice::from_ref(id)),
            Pv::Ids(ids) => self.pick_ids(c, ids),
            Pv::List(xs) if matches!(xs.first(), Some(Pv::E(_))) => {
                let id = if let Some(Pv::E(id)) = xs.first() { id.clone() } else { return };
                let p = match xs.get(1) {
                    Some(Pv::List(q)) => {
                        Some(to_world(uc, P2::new(q.first().and_then(Pv::num).unwrap_or(0.0), q.get(1).and_then(Pv::num).unwrap_or(0.0))))
                    }
                    _ => None,
                };
                if kind == Kind::Entity
                    && let (Some(p), Some(n)) = (p, id.num())
                {
                    self.apply(c, Act::Resolve(Reply::Pick { id: n, at: p, shift: false }));
                    return;
                }
                self.pick_ids(c, &[id]);
            }
            other => self.text(c, &dxflist::js_str(other)),
        }
    }

    /// The routine's (command …).
    async fn command_impl(&self, args: Vec<Pv>, strict: bool) -> HR<()> {
        self.before_input();
        let Some(c) = self.ctx() else { return Err(HostErr::Missing) };
        if args.is_empty() {
            if self.has_nested() {
                self.escape_nested(&c);
            }
            return Ok(());
        }
        for a in args {
            if matches!(a, Pv::Nil) {
                continue;
            }
            if !self.has_nested() {
                let Pv::Str(s) = &a else { return Err(HostErr::Msg(self.t("lisp.cmd.notname", &[]))) };
                if s.is_empty() {
                    continue;
                }
                let name = s.trim_start_matches(['_', '.', '\'']);
                let cmd = c.sh.lookup(name);
                match cmd {
                    Some(cmd) if !is_lisp(&cmd) => {
                        self.start(&c, cmd);
                        continue;
                    }
                    found => {
                        let up = name.to_uppercase();
                        self.print(&self.t("lisp.cmd.unknown", &[("name", up.clone())]));
                        return Err(HostErr::Msg(match found {
                            Some(cmd) => self.t("lisp.cmd.nolisp", &[("name", cmd.name.to_string())]),
                            None => self.t("lisp.cmd.unknown", &[("name", up)]),
                        }));
                    }
                }
            }
            let wait = c.sh.wait_seq();
            if wait.is_none() && !self.has_nested() {
                continue;
            }
            if a == Pv::Str(PAUSE.into()) {
                if strict {
                    return Err(HostErr::Msg(self.t("lisp.cmd.nopause", &[])));
                }
                loop {
                    Turn(false).await;
                    self.poll_nested();
                    if !self.has_nested() || c.sh.wait_seq() != wait {
                        break;
                    }
                }
                // host-cad.js tests `n.error.name === 'Cancel' || n.error.cancel` here, which a command ended by Esc never
                // has (its Cancel is an Error named "Error"): the routine goes on with its next answer, as 0.5 does
                continue;
            }
            if let Pv::Str(s) = &a
                && c.sh.waiting_info().is_some_and(|w| w.0 == Kind::Point)
                && osnap_override(s)
            {
                continue;
            }
            let kind = c.sh.waiting_info().map(|w| w.0);
            self.feed(&c, &a);
            if self.has_nested() && c.sh.wait_seq() == wait && kind != Some(Kind::Select) {
                self.escape_nested(&c);
                return Err(HostErr::Msg(self.t("lisp.cmd.badinput", &[("value", describe(&a))])));
            }
        }
        if strict && self.has_nested() {
            self.escape_nested(&c);
            return Err(HostErr::Msg(self.t("lisp.cmd.unfinished", &[])));
        }
        Ok(())
    }

    /// The routine ended with a command still asking: the person finishes it.
    pub async fn finish_nested(&self, c: &Ctx) {
        let _ = c;
        while self.has_nested() {
            Turn(false).await;
            self.poll_nested();
        }
    }
    /// A hidden command ended (a file loaded, a typed "(…"): a command still asking is left (0.5 leaves it behind).
    pub fn drop_nested(&self, c: &Ctx) {
        if self.nested.borrow_mut().take().is_some() {
            c.sh.st.borrow_mut().waiting = None;
        }
    }
    pub fn cmd_active(&self) -> bool {
        self.has_nested()
    }

    // ---- input ----

    async fn get_impl(&self, kind: GetKind, o: GetOpts) -> HR<GetAns> {
        self.before_input();
        self.st.borrow_mut().errno = 0;
        let Some(c) = self.ctx() else { return Err(HostErr::Missing) };
        let kws = parse_keywords(o.kw.as_deref());
        let flags = o.flags;
        let options: Vec<Opt> = kws.iter().map(|k| Opt { key: k.key.clone(), label: k.label.clone(), en: None, hidden: true }).collect();
        let uc = active_ucs(&c.doc());
        let base = o.base.as_ref().map(|b| to_world(uc, P2::new(b.first().copied().unwrap_or(0.0), b.get(1).copied().unwrap_or(0.0))));
        let dflt = match kind {
            GetKind::Point => "lisp.get.point",
            GetKind::Corner => "lisp.get.corner",
            GetKind::Dist => "lisp.get.dist",
            GetKind::Angle | GetKind::Orient => "lisp.get.angle",
            GetKind::Real => "lisp.get.real",
            GetKind::Int => "lisp.get.int",
            GetKind::Str => "lisp.get.string",
            GetKind::Kword => "lisp.get.kword",
        };
        let prompt = clean_prompt(o.msg.as_deref(), c.t(dflt));
        let any_text = flags & 128 != 0;
        loop {
            let opts = Opts { base, none: true, options: options.clone(), any_text, ..Opts::default() };
            let r = match kind {
                GetKind::Point => c.point(prompt.clone(), opts).await,
                GetKind::Corner => {
                    if let Some(b) = base {
                        c.preview(Some(Rc::new(move |_, cur: P2| {
                            let pts: Vec<Value> = [(b.x, b.y), (cur.x, b.y), (cur.x, cur.y), (b.x, cur.y)]
                                .iter()
                                .map(|(x, y)| json!({ "x": cc_cad::entity::num(*x), "y": cc_cad::entity::num(*y), "b": 0 }))
                                .collect();
                            vec![Entity::from_value(&json!({ "type": "poly", "closed": true, "pts": pts }))]
                        })));
                    }
                    let r = c.point(prompt.clone(), opts).await;
                    c.preview(None);
                    r
                }
                GetKind::Dist => c.distance(prompt.clone(), opts).await,
                GetKind::Angle | GetKind::Orient => {
                    let r = c.angle(prompt.clone(), Opts { ucs: true, ..opts }).await;
                    match (r, uc) {
                        (Ok(Reply::Num(n)), Some(u)) => Ok(Reply::Num(n - u.ang)),
                        (r, _) => r,
                    }
                }
                GetKind::Real | GetKind::Int => c.number(prompt.clone(), Opts { base: None, ..opts }).await,
                GetKind::Str => {
                    let r = c.string(prompt.clone(), Opts::default()).await.map_err(cmd_err)?;
                    let s = if let Reply::Str(s) = r { s } else { String::new() };
                    return Ok(GetAns::Value(Pv::Str(s)));
                }
                GetKind::Kword => c.keyword(prompt.clone(), Opts { base: None, ..opts }).await,
            }
            .map_err(cmd_err)?;
            match r {
                Reply::Arbitrary(s) => return Ok(GetAns::Text(s)),
                Reply::Opt(k) => {
                    let v = kws.iter().find(|x| x.key == k).map_or(k.clone(), |x| x.value.clone());
                    return Ok(GetAns::Kw(v));
                }
                Reply::Null => {
                    if flags & 1 != 0 {
                        self.print(&c.t("lisp.get.required"));
                        continue;
                    }
                    return Ok(GetAns::None);
                }
                Reply::Point(p, _) if matches!(kind, GetKind::Point | GetKind::Corner) => {
                    let q = to_ucs(uc, p);
                    return Ok(GetAns::Value(pt3(q.x, q.y)));
                }
                Reply::Num(n) => {
                    if kind == GetKind::Int && n.fract() != 0.0 {
                        self.print(&c.t("lisp.get.needint"));
                        continue;
                    }
                    if kind == GetKind::Int && n.abs() > 32767.0 {
                        self.print(&c.t("lisp.get.intrange"));
                        continue;
                    }
                    if flags & 2 != 0 && n == 0.0 {
                        self.print(&c.t("lisp.get.nonzero"));
                        continue;
                    }
                    if flags & 4 != 0 && n < 0.0 {
                        self.print(&c.t("lisp.get.nonneg"));
                        continue;
                    }
                    return Ok(GetAns::Value(if kind == GetKind::Int { i(n) } else { Pv::Num(n) }));
                }
                Reply::Str(s) => return Ok(GetAns::Value(Pv::Str(s))),
                _ => return Ok(GetAns::Value(Pv::Nil)),
            }
        }
    }

    async fn entsel_impl(&self, msg: Option<String>, kw: Option<String>, nested: bool) -> HR<EntselAns> {
        self.before_input();
        self.st.borrow_mut().errno = 0;
        let Some(c) = self.ctx() else { return Err(HostErr::Missing) };
        let kws = parse_keywords(kw.as_deref());
        let options: Vec<Opt> = kws.iter().map(|k| Opt { key: k.key.clone(), label: k.label.clone(), en: None, hidden: true }).collect();
        let r = c
            .entity(clean_prompt(msg.as_deref(), c.t("lisp.get.object")), Opts { none: true, empty: true, options, ..Opts::default() })
            .await
            .map_err(cmd_err)?;
        match r {
            Reply::Opt(k) => Ok(EntselAns::Kw(kws.iter().find(|x| x.key == k).map_or(k.clone(), |x| x.value.clone()))),
            Reply::Null => Ok(EntselAns::None),
            Reply::Empty { .. } => {
                self.st.borrow_mut().errno = 7;
                Ok(EntselAns::None)
            }
            Reply::Pick { id, at, .. } => {
                if !nested {
                    return Ok(EntselAns::Pick { e: EId::N(id), p: vec![at.x, at.y, 0.0], matrix: None, parents: Vec::new() });
                }
                Ok(self.nested_at(&c.doc(), id, at))
            }
            _ => Ok(EntselAns::None),
        }
    }

    /// nentsel: the object inside a block reference nearest the pick, its block-to-world matrix and the references.
    fn nested_at(&self, doc: &Doc, id: u64, pick: P2) -> EntselAns {
        let p = vec![pick.x, pick.y, 0.0];
        let plain = || EntselAns::Pick { e: EId::N(id), p: p.clone(), matrix: None, parents: Vec::new() };
        let Some(mut e) = doc.get(id).cloned() else { return plain() };
        if !matches!(e.geo, Geo::Insert(_)) {
            return plain();
        }
        let mut parents: Vec<EId> = Vec::new();
        let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let mut r: Option<EId> = None;
        for _depth in 0..16 {
            let Geo::Insert(ins) = &e.geo else { break };
            let Some(b) = doc.blocks.get(&ins.block) else { break };
            parents.insert(0, r.clone().unwrap_or(EId::N(id)));
            let rot = js::or(ins.rot, 0.0);
            let (cs, sn) = (rot.jcos(), rot.jsin());
            let sx = ins.sx.unwrap_or(1.0);
            let sy = ins.sy.unwrap_or(1.0);
            let (bx, by) = (js::or(Some(b.base.x), 0.0), js::or(Some(b.base.y), 0.0));
            let l = [cs * sx, sn * sx, -sn * sy, cs * sy, ins.x - bx * cs * sx + by * sn * sy, ins.y - bx * sn * sx - by * cs * sy];
            m = [
                m[0] * l[0] + m[2] * l[1],
                m[1] * l[0] + m[3] * l[1],
                m[0] * l[2] + m[2] * l[3],
                m[1] * l[2] + m[3] * l[3],
                m[0] * l[4] + m[2] * l[5] + m[4],
                m[1] * l[4] + m[3] * l[5] + m[5],
            ];
            let det = m[0] * m[3] - m[1] * m[2];
            if det.abs() < 1e-18 {
                break;
            }
            let (dx, dy) = (pick.x - m[4], pick.y - m[5]);
            let q = P2::new((m[3] * dx - m[2] * dy) / det, (-m[1] * dx + m[0] * dy) / det);
            let mut best: Option<usize> = None;
            let mut bd = f64::INFINITY;
            for (k, be) in b.entities.iter().enumerate() {
                let bb = entity_bounds(be, Some(&doc.blocks));
                if !valid_bounds(&bb) {
                    continue;
                }
                let d = js::hypot(js::max_of([bb.min_x - q.x, 0.0, q.x - bb.max_x]), js::max_of([bb.min_y - q.y, 0.0, q.y - bb.max_y]));
                if d < bd {
                    bd = d;
                    best = Some(k);
                }
            }
            let Some(k) = best else { break };
            r = Some(EId::S(format!("b:{}:{k}", b.name).into()));
            let Some(next) = b.entities.get(k).cloned() else { break };
            e = next;
        }
        let Some(r) = r else { return plain() };
        // parents: the references from the innermost out, then the picked one (0.5: slice(1).reverse(), + {e: id}, reverse)
        let mut ps: Vec<EId> = parents.iter().skip(1).cloned().collect();
        ps.reverse();
        ps.push(EId::N(id));
        ps.reverse();
        EntselAns::Pick {
            e: r,
            p,
            matrix: Some(vec![vec![m[0], m[2], 0.0], vec![m[1], m[3], 0.0], vec![0.0, 0.0, 1.0], vec![m[4], m[5], 0.0]]),
            parents: ps,
        }
    }

    // ---- selection sets ----

    fn candidates(doc: &Doc, all: bool) -> Vec<(Entity, String)> {
        let mut out = Vec::new();
        if all {
            for e in doc.space_entities(MODEL) {
                out.push((e.clone(), "Model".to_string()));
            }
            for l in &doc.layouts {
                for e in doc.space_entities(l.space) {
                    out.push((e.clone(), l.name.clone()));
                }
            }
        } else {
            let name = CadHost::space_name(doc, doc.space);
            for e in doc.entities() {
                out.push((e.clone(), name.clone()));
            }
        }
        out
    }

    async fn ssget_impl(&self, mode: Option<String>, pts: Vec<Vec<f64>>, filter: Option<Pairs>) -> HR<Option<Vec<EId>>> {
        self.before_input();
        let Some(c) = self.ctx() else { return Err(HostErr::Missing) };
        let mode0 = mode.unwrap_or_default().to_uppercase();
        let mode0 = mode0.strip_prefix('_').unwrap_or(&mode0).to_string();
        let mode = mode0.strip_prefix("+.").unwrap_or(&mode0).to_string();
        let pts: Vec<P2> =
            pts.iter().map(|p| P2::new(p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN))).collect();
        let head = mode.split(':').next().unwrap_or("").to_string();
        let key = if head.is_empty() && !pts.is_empty() { "PT".to_string() } else { head };
        let here = |d: &Doc| -> Vec<Entity> { d.entities().filter(|e| d.editable(e)).cloned().collect() };
        let bx = |a: P2, b: P2| vec![P2::new(a.x, a.y), P2::new(b.x, a.y), P2::new(b.x, b.y), P2::new(a.x, b.y)];
        let ids: Vec<u64> = match key.as_str() {
            "X" => CadHost::candidates(&c.doc(), true).iter().filter_map(|(e, _)| e.id).collect(),
            "W" | "C" => {
                let (Some(a), Some(b)) = (pts.first(), pts.get(1)) else { return Ok(None) };
                let d = c.doc();
                lasso_hits(&here(&d), &bx(*a, *b), if mode.starts_with('W') { "window" } else { "crossing" }, &d)
            }
            "WP" | "CP" => {
                if pts.len() < 3 {
                    return Ok(None);
                }
                let d = c.doc();
                lasso_hits(&here(&d), &pts, if mode == "WP" { "window" } else { "crossing" }, &d)
            }
            "F" => {
                if pts.len() < 2 {
                    return Ok(None);
                }
                let d = c.doc();
                lasso_hits(&here(&d), &pts, "fence", &d)
            }
            "P" => {
                let prev = c.st().prev_selection.clone().unwrap_or_default();
                let d = c.doc();
                prev.0.into_iter().filter(|id| d.get(*id).is_some()).collect()
            }
            "L" => CadHost::last_id(&c.doc()).into_iter().collect(),
            "I" => {
                let sel = c.sh.host.borrow().selection();
                let d = c.doc();
                sel.into_iter().filter(|id| d.get(*id).is_some()).collect()
            }
            "PT" => {
                let p = pts.first().copied().unwrap_or(P2::new(0.0, 0.0));
                let r = 1e-6 + js::or(c.sh.host.borrow().pick_tolerance(), 0.5);
                let id = c.sh.host.borrow().pick_at(&c.doc(), p);
                match id {
                    Some(id) => vec![id],
                    None => {
                        let d = c.doc();
                        lasso_hits(&here(&d), &bx(P2::new(p.x - r, p.y - r), P2::new(p.x + r, p.y + r)), "crossing", &d)
                            .into_iter()
                            .take(1)
                            .collect()
                    }
                }
            }
            _ => {
                let single = mode.contains(":S") || mode.contains(":E");
                let set =
                    c.select_opts(Some(clean_prompt(None, c.t("lisp.get.select"))), true, Vec::new(), single).await.map_err(cmd_err)?;
                match set {
                    crate::engine::Sel::Set(s) => s.0,
                    crate::engine::Sel::Opt(_) => Vec::new(),
                }
            }
        };
        let ids = match filter.filter(|f| !f.is_empty()) {
            Some(f) => {
                let d = c.doc();
                let all = CadHost::candidates(&d, true);
                let mut keep = Vec::new();
                for id in ids {
                    let Some((e, space)) = all.iter().find(|(e, _)| e.id == Some(id)) else { continue };
                    let pairs = dxflist::to_pairs(
                        &e.to_obj(),
                        &PairCtx { space: Some(space.clone()), handle: CadHost::handle(&EId::N(id)), ..PairCtx::default() },
                    );
                    if dxflist::match_filter(&pairs, &f).map_err(|e| HostErr::Msg(e.msg))? {
                        keep.push(id);
                    }
                }
                keep
            }
            None => ids,
        };
        if !ids.is_empty() {
            c.st().prev_selection = Some(crate::engine::IdSet::from_vec(ids.clone()));
        }
        Ok(if ids.is_empty() { None } else { Some(ids.into_iter().map(EId::N).collect()) })
    }
}

/// lasso.js `lassoHits`: objects a closed lasso holds (window), touches (crossing), or a path crosses (fence).
pub fn lasso_hits(entities: &[Entity], pts: &[P2], mode: &str, doc: &Doc) -> Vec<u64> {
    let blocks = Some(&doc.blocks);
    if mode == "fence" {
        return fence_hits(entities, pts, blocks).into_iter().flatten().collect();
    }
    if pts.len() < 3 {
        return Vec::new();
    }
    let mut lb = empty_bounds();
    for p in pts {
        grow_pt(&mut lb, p.x, p.y);
    }
    let edges: Vec<cc_cad_geom::geom::Prim> = (0..pts.len())
        .filter_map(|k| {
            let a = pts.get(k)?;
            let b = pts.get(k.saturating_add(1).checked_rem(pts.len()).unwrap_or(0))?;
            (cc_cad_geom::geom::dist(*a, *b) > cc_cad_geom::geom::EPS).then(|| cc_cad_geom::geom::Prim::l(a.x, a.y, b.x, b.y))
        })
        .collect();
    let cuts_any = |ps: &[cc_cad_geom::geom::Prim]| ps.iter().any(|p| edges.iter().any(|ed| !intersect(p, ed, false, false).is_empty()));
    let mut out = Vec::new();
    for e in entities {
        let Some(id) = e.id else { continue };
        if matches!(e.geo, Geo::Xline(_) | Geo::Ray(_)) {
            if mode == "crossing" {
                let ps = prims(e, None);
                let start_in = matches!(&e.geo, Geo::Ray(r) if point_in_poly(P2::new(r.x, r.y), pts));
                if cuts_any(&ps) || start_in {
                    out.push(id);
                }
            }
            continue;
        }
        let b = entity_bounds(e, blocks);
        if !valid_bounds(&b) {
            continue;
        }
        if b.max_x < lb.min_x || b.min_x > lb.max_x || b.max_y < lb.min_y || b.min_y > lb.max_y {
            continue;
        }
        let box_inside = b.min_x >= lb.min_x && b.max_x <= lb.max_x && b.min_y >= lb.min_y && b.max_y <= lb.max_y;
        if mode == "window" && !box_inside {
            continue;
        }
        let is_point = matches!(e.geo, Geo::Point(_));
        let ps = if is_point { Vec::new() } else { prims(e, blocks) };
        let cuts = cuts_any(&ps);
        if mode == "window" {
            if cuts {
                continue;
            }
            let probe: Vec<P2> = if let Geo::Point(p) = &e.geo {
                vec![P2::new(p.x, p.y)]
            } else if !ps.is_empty() {
                ps.iter().map(|p| prim_point(p, 0.0)).collect()
            } else {
                vec![P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0)]
            };
            if probe.iter().all(|q| point_in_poly(*q, pts)) {
                out.push(id);
            }
            continue;
        }
        if cuts {
            out.push(id);
            continue;
        }
        if let Geo::Point(p) = &e.geo {
            if point_in_poly(P2::new(p.x, p.y), pts) {
                out.push(id);
            }
            continue;
        }
        if ps.iter().any(|p| point_in_poly(prim_point(p, 0.0), pts)) {
            out.push(id);
            continue;
        }
        if matches!(e.geo, Geo::Text(_) | Geo::Hatch(_))
            && pts.iter().any(|q| q.x >= b.min_x && q.x <= b.max_x && q.y >= b.min_y && q.y <= b.max_y)
        {
            out.push(id);
        }
    }
    out
}

// ---- the drawing side cc-lisp calls ----

impl cc_lisp::Host for CadHost {
    fn get(&self, kind: GetKind, o: GetOpts) -> LFut<HR<GetAns>> {
        let me = self.me.clone();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            if h.prompt_eval.get() {
                return Err(HostErr::Cancel);
            }
            h.get_impl(kind, o).await
        })
    }
    fn entsel(&self, msg: Option<String>, kw: Option<String>, nested: bool) -> LFut<HR<EntselAns>> {
        let me = self.me.clone();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            if h.prompt_eval.get() {
                return Err(HostErr::Cancel);
            }
            h.entsel_impl(msg, kw, nested).await
        })
    }
    fn ssget(&self, mode: Option<String>, pts: Vec<Vec<f64>>, filter: Option<Pairs>) -> LFut<HR<Option<Vec<EId>>>> {
        let me = self.me.clone();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            if h.prompt_eval.get() {
                return Err(HostErr::Cancel);
            }
            h.ssget_impl(mode, pts, filter).await
        })
    }
    fn sssetfirst(&self, ids: &[EId]) -> HR<()> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let keep: Vec<u64> = {
            let d = sh.doc.borrow();
            ids.iter().filter(|id| CadHost::find(&d, id).is_some()).filter_map(EId::num).collect()
        };
        sh.host.borrow_mut().set_selection(&keep);
        Ok(())
    }
    fn ssgetfirst(&self) -> HR<Option<Vec<EId>>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let sel = sh.host.borrow().selection();
        let d = sh.doc.borrow();
        let ids: Vec<EId> = sel.into_iter().filter(|id| d.get(*id).is_some()).map(EId::N).collect();
        Ok(if ids.is_empty() { None } else { Some(ids) })
    }
    fn command(&self, args: Vec<Pv>, strict: bool) -> LFut<HR<()>> {
        let me = self.me.clone();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            if h.prompt_eval.get() {
                return Err(HostErr::Cancel);
            }
            h.command_impl(args, strict).await
        })
    }
    fn getcname(&self, name: &str) -> HR<Option<String>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        Ok(sh.lookup(name.trim_start_matches(['_', '.'])).map(|c| c.name.to_string()))
    }

    fn getvar(&self, name: &str) -> HR<Pv> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        Ok(self.getvar_impl(&sh, name))
    }
    fn setvar(&self, name: &str, v: Pv) -> HR<bool> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        Ok(self.setvar_impl(&sh, name, v))
    }
    fn trans(&self, p: &[f64], from: i32, to: i32, disp: bool) -> HR<Vec<f64>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let u = active_ucs(&sh.doc.borrow());
        let tw = js::or(sh.host.borrow().view_twist(), 0.0);
        let q = P2::new(js::or(p.first().copied(), 0.0), js::or(p.get(1).copied(), 0.0));
        let rot = |v: P2, a: f64| P2::new(v.x * a.jcos() - v.y * a.jsin(), v.x * a.jsin() + v.y * a.jcos());
        let w = match from {
            1 => {
                if disp {
                    vec_to_world(u, q)
                } else {
                    to_world(u, q)
                }
            }
            2 => rot(q, tw),
            _ => q,
        };
        let r = match to {
            1 => {
                if disp {
                    vec_to_ucs(u, w)
                } else {
                    to_ucs(u, w)
                }
            }
            2 => rot(w, -tw),
            _ => w,
        };
        Ok(if p.len() > 2 { vec![r.x, r.y, js::or(p.get(2).copied(), 0.0)] } else { vec![r.x, r.y] })
    }

    fn entget(&self, id: &EId) -> HR<Option<Pairs>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let d = sh.doc.borrow();
        if let Some((_, _, _, _, _, pairs)) = CadHost::sub_of(&d, id) {
            return Ok(Some(pairs));
        }
        let Some(f) = CadHost::find(&d, id) else { return Ok(None) };
        if let Some((b, _)) = &f.block {
            return Ok(Some(dxflist::to_pairs(
                &f.e.to_obj(),
                &PairCtx { ename: Some(id.clone()), owner: Some(EId::S(format!("B:{b}").into())), ..PairCtx::default() },
            )));
        }
        let space = f.space.map(|s| s.1);
        Ok(Some(dxflist::to_pairs(
            &f.e.to_obj(),
            &PairCtx { ename: Some(id.clone()), handle: CadHost::handle(id), space, ..PairCtx::default() },
        )))
    }
    fn entmod(&self, pairs: Pairs) -> HR<bool> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let Some(Pv::E(eref)) = pairs.iter().find(|(c, _)| *c == -1).map(|(_, v)| v.clone()) else { return Ok(false) };
        let sub = CadHost::sub_of(&sh.doc.borrow(), &eref);
        if let Some((is_att, _n, ins, space, tag, _)) = sub {
            if !is_att {
                return Ok(true);
            }
            let tag = tag.unwrap_or_default();
            let Some(Pv::Str(v)) = pairs.iter().find(|(c, _)| *c == 1).map(|(_, v)| v.clone()) else { return Ok(true) };
            let Geo::Insert(gi) = &ins.geo else { return Ok(true) };
            let cur = gi.attrs.as_ref().and_then(|a| a.get(&tag)).filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default();
            if v == cur {
                return Ok(true);
            }
            let mut ne = ins.clone();
            if let Geo::Insert(g) = &mut ne.geo {
                let mut a = g.attrs.clone().unwrap_or_default();
                a.insert(tag, Value::String(v));
                g.attrs = Some(a);
            }
            let id = ins.id.unwrap_or(0);
            let r = sh.doc.borrow_mut().change("ENTMOD", |d| d.replace_in(id, ne, space));
            sh.flush_notes();
            return r.map(|_| true).map_err(|e| HostErr::Msg(e.to_string()));
        }
        let f = CadHost::find(&sh.doc.borrow(), &eref);
        let Some(f) = f else { return Ok(false) };
        let old = f.e.to_obj();
        let exists = |n: &str| sh.doc.borrow().blocks.contains_key(n);
        let r = dxflist::from_pairs(&pairs, Some(&old), &FromCtx { block_exists: Some(&exists), ..FromCtx::default() });
        let Ok(obj) = r else { return Ok(false) };
        let ne = Entity::from_value(&Value::Object(obj));
        if let Some((bname, idx)) = f.block {
            let b = sh.doc.borrow().blocks.get(&bname).cloned();
            let Some(mut b) = b else { return Ok(false) };
            if let Some(slot) = b.entities.get_mut(idx) {
                *slot = ne;
            }
            let r = sh.doc.borrow_mut().change("ENTMOD", |d| d.define_block(&b).map(|_| ()));
            sh.flush_notes();
            return r.map(|_| true).map_err(|e| HostErr::Msg(e.to_string()));
        }
        if let Some(l) = ne.layer.clone().filter(|l| !l.is_empty()) {
            sh.doc.borrow_mut().ensure_layer(&l);
        }
        let (space, _) = f.space.unwrap_or((MODEL, String::new()));
        let id = f.e.id.unwrap_or(0);
        let r = sh.doc.borrow_mut().change("ENTMOD", |d| d.replace_in(id, ne, space));
        sh.flush_notes();
        r.map(|_| true).map_err(|e| HostErr::Msg(e.to_string()))
    }
    fn entmake(&self, pairs: Pairs) -> HR<MakeAns> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let t0 = pairs.iter().find(|(c, _)| *c == 0).map(|(_, v)| v.clone());
        let ty = match &t0 {
            Some(Pv::Str(s)) if !s.is_empty() => s.to_uppercase(),
            Some(Pv::Nil) | None => String::new(),
            Some(Pv::Num(n)) if *n == 0.0 || n.is_nan() => String::new(),
            Some(v) => dxflist::js_str(v).to_uppercase(),
        };
        let find = |code: i32| pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
        if ty == "BLOCK" {
            let Some(Pv::Str(name)) = find(2).filter(|v| matches!(v, Pv::Str(s) if !s.is_empty())) else { return Ok(MakeAns::Error) };
            let b = find(10);
            let (x, y) = match &b {
                Some(Pv::List(xs)) => (js::or(xs.first().and_then(Pv::num), 0.0), js::or(xs.get(1).and_then(Pv::num), 0.0)),
                _ => (0.0, 0.0),
            };
            self.st.borrow_mut().making = Some(Making { name, base: P2::new(x, y), entities: Vec::new() });
            return Ok(MakeAns::Made);
        }
        if ty == "ENDBLK" {
            let Some(m) = self.st.borrow_mut().making.take() else { return Ok(MakeAns::Error) };
            if m.entities.is_empty() {
                return Ok(MakeAns::Error);
            }
            let b = cc_cad::Block::new(&m.name, m.base, m.entities);
            let r = sh.doc.borrow_mut().change("ENTMAKE", |d| d.define_block(&b).map(|_| ()));
            sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(MakeAns::Block(m.name));
        }
        if ty == "LAYER" {
            let Some(Pv::Str(name)) = find(2).filter(|v| matches!(v, Pv::Str(s) if !s.is_empty())) else { return Ok(MakeAns::Error) };
            let col = find(62).and_then(|v| v.num()).unwrap_or(7.0);
            let fl = cc_lisp::value::to_i32(find(70).and_then(|v| v.num()).unwrap_or(0.0));
            let mut patch = Obj::new();
            patch.insert("color".into(), cc_cad::entity::num(js::or(Some(col.abs()), 7.0)));
            patch.insert("on".into(), Value::Bool(col >= 0.0));
            patch.insert("frozen".into(), Value::Bool(fl & 1 != 0));
            patch.insert("locked".into(), Value::Bool(fl & 4 != 0));
            let r = sh.doc.borrow_mut().set_layer(&name, &patch, false);
            sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(MakeAns::Made);
        }
        let (layer, color, th) = {
            let d = sh.doc.borrow();
            (d.current.layer.clone(), d.current.color.to_value(), d.current.text_height)
        };
        let exists = |n: &str| sh.doc.borrow().blocks.contains_key(n);
        let r = dxflist::from_pairs(
            &pairs,
            None,
            &FromCtx { layer: Some(layer), color: Some(color), text_height: Some(th), block_exists: Some(&exists), has_color: true },
        );
        let Ok(obj) = r else { return Ok(MakeAns::Error) };
        let ne = Entity::from_value(&Value::Object(obj));
        if let Some(m) = self.st.borrow_mut().making.as_mut() {
            m.entities.push(ne);
            return Ok(MakeAns::Made);
        }
        let space = {
            let d = sh.doc.borrow();
            match find(410) {
                Some(Pv::Str(s)) if s == "Model" => MODEL,
                Some(Pv::Str(s)) => d.layouts.iter().find(|l| l.name == s).map_or(d.space, |l| l.space),
                _ => d.space,
            }
        };
        let r = sh.doc.borrow_mut().change("ENTMAKE", |d| d.add_in_id(ne, space));
        sh.flush_notes();
        let id = r.map_err(|e| HostErr::Msg(e.to_string()))?;
        Ok(MakeAns::E(EId::N(id)))
    }
    fn entdel(&self, id: &EId) -> HR<bool> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        if let Some(n) = id.num() {
            let gone = self.st.borrow_mut().deleted.remove(&n);
            if let Some((e, space)) = gone {
                let r = sh.doc.borrow_mut().change("ENTDEL", |d| d.reinsert_in(n, e, space));
                sh.flush_notes();
                r.map_err(|e| HostErr::Msg(e.to_string()))?;
                return Ok(true);
            }
        }
        let f = CadHost::find(&sh.doc.borrow(), id);
        let Some(f) = f else { return Ok(false) };
        if f.block.is_some() {
            return Ok(false);
        }
        let (space, _) = f.space.unwrap_or((MODEL, String::new()));
        let n = f.e.id.unwrap_or(0);
        self.st.borrow_mut().deleted.insert(n, (f.e.clone(), space));
        let r = sh.doc.borrow_mut().change("ENTDEL", |d| d.remove_in(n, space));
        sh.flush_notes();
        r.map_err(|e| HostErr::Msg(e.to_string()))?;
        Ok(true)
    }
    fn entlast(&self) -> HR<Option<EId>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        Ok(CadHost::last_id(&sh.doc.borrow()).map(EId::N))
    }
    fn entnext(&self, id: Option<&EId>) -> HR<Option<EId>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let d = sh.doc.borrow();
        let o = CadHost::order(&d);
        let Some(id) = id else { return Ok(o.first().copied().map(EId::N)) };
        let after = |n: u64| -> Option<EId> {
            let k = o.iter().position(|x| *x == n)?;
            o.get(k.saturating_add(1)).copied().map(EId::N)
        };
        if let Some((is_att, n, ins, _, tag, _)) = CadHost::sub_of(&d, id) {
            if is_att {
                let tags = CadHost::att_tags(&d, &ins);
                let k = tags.iter().position(|t| Some(t) == tag.as_ref()).map_or(0, |k| k.saturating_add(1));
                return Ok(Some(match tags.get(k) {
                    Some(t) => EId::S(format!("a:{n}:{t}").into()),
                    None => EId::S(format!("s:{n}").into()),
                }));
            }
            return Ok(after(n));
        }
        if let EId::N(n) = id {
            if let Some(f) = CadHost::find(&d, id) {
                let tags = CadHost::att_tags(&d, &f.e);
                if let Some(t) = tags.first() {
                    return Ok(Some(EId::S(format!("a:{n}:{t}").into())));
                }
            }
            return Ok(after(*n));
        }
        if let Some(s) = id.text()
            && let Some(rest) = s.strip_prefix("b:")
            && let Some(k) = rest.rfind(':')
        {
            let name = rest.get(..k).unwrap_or("");
            let idx = js::str_to_number(rest.get(k.saturating_add(1)..).unwrap_or(""));
            let Some(b) = d.blocks.get(&name.to_string()) else { return Ok(None) };
            #[allow(clippy::cast_precision_loss)]
            let len = b.entities.len() as f64;
            return Ok(if idx + 1.0 < len { Some(EId::S(format!("b:{name}:{}", js::num_to_string(idx + 1.0)).into())) } else { None });
        }
        Ok(None)
    }
    fn handent(&self, h: &str) -> HR<Option<EId>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let n = parse_int16(h);
        let Some(n) = n else { return Ok(None) };
        if n < 0 {
            return Ok(None);
        }
        let id = EId::N(n.unsigned_abs());
        Ok(CadHost::find(&sh.doc.borrow(), &id).map(|_| id))
    }

    fn tblnext(&self, table: &str, rewind: bool) -> HR<Option<Pairs>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let key = table.to_uppercase();
        let Some(list) = table_list(&sh.doc.borrow(), &key) else { return Ok(None) };
        let k = if rewind { 0 } else { self.st.borrow().tbl_pos.get(&key).copied().unwrap_or(0) };
        if k >= list.len() {
            return Ok(None);
        }
        self.st.borrow_mut().tbl_pos.insert(key, k.saturating_add(1));
        Ok(list.into_iter().nth(k).map(|x| x.1))
    }
    fn tblsearch(&self, table: &str, name: &str, setnext: bool) -> HR<Option<Pairs>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let key = table.to_uppercase();
        let Some(list) = table_list(&sh.doc.borrow(), &key) else { return Ok(None) };
        let Some(k) = list.iter().position(|(n, _)| n.to_uppercase() == name.to_uppercase()) else { return Ok(None) };
        if setnext {
            self.st.borrow_mut().tbl_pos.insert(key, k.saturating_add(1));
        }
        Ok(list.into_iter().nth(k).map(|x| x.1))
    }
    fn tblobjname(&self, table: &str, name: &str) -> HR<Option<EId>> {
        let key = table.to_uppercase();
        let hit = self.tblsearch(&key, name, false)?;
        let Some(hit) = hit else { return Ok(None) };
        let n = hit.iter().find(|(c, _)| *c == 2).map(|(_, v)| dxflist::js_str(v)).unwrap_or_default();
        Ok(Some(EId::S(format!("T:{key}:{n}").into())))
    }

    fn textbox(&self, pairs: Pairs) -> HR<Option<[[f64; 3]; 2]>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let mut ps: Pairs = vec![(0, Pv::Str("TEXT".into())), (10, Pv::pt(&[0.0, 0.0, 0.0]))];
        ps.extend(pairs.into_iter().filter(|(c, _)| ![0, 10, 11, 72, 73, 50].contains(c)));
        let th = sh.doc.borrow().current.text_height;
        let Ok(obj) = dxflist::from_pairs(&ps, None, &FromCtx { text_height: Some(th), ..FromCtx::default() }) else { return Ok(None) };
        let mut o = obj;
        for (k, v) in [("x", json!(0)), ("y", json!(0)), ("rot", json!(0)), ("ha", json!("left")), ("va", json!("baseline"))] {
            o.insert(k.into(), v);
        }
        let e = Entity::from_value(&Value::Object(o));
        let Geo::Text(t) = &e.geo else { return Ok(None) };
        let lay = cc_cad_geom::geom::text_layout(t);
        let [bx, by, bw, bh] = lay.bx;
        Ok(Some([[bx, js::min(by, 0.0), 0.0], [bx + bw, js::max(by + bh, t.h), 0.0]]))
    }
    fn osnap(&self, p: &[f64], modes: &str) -> HR<Option<Vec<f64>>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        let want: Vec<&str> = modes
            .to_lowercase()
            .split(',')
            .map(|s| {
                let t = js::trim(s);
                let t = t.strip_prefix('_').unwrap_or(t);
                let short: String = t.chars().take(3).collect();
                match short.as_str() {
                    "end" => "end",
                    "mid" => "mid",
                    "cen" => "cen",
                    "nod" => "node",
                    "qua" => "quad",
                    "int" => "int",
                    "ins" => "ins",
                    "per" => "perp",
                    "nea" => "near",
                    _ => "",
                }
            })
            .filter(|k| SNAP_KINDS.contains(k))
            .collect();
        let q = P2::new(p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN));
        let r = js::or(sh.host.borrow().pick_tolerance(), 1.0);
        let d = sh.doc.borrow();
        let mut best: Option<P2> = None;
        let mut bd = f64::INFINITY;
        for e in d.entities() {
            if !d.visible(e) {
                continue;
            }
            for s in snap_points(e, q, r, &want, Some(&d.blocks), None) {
                let dd = js::hypot(s.x - q.x, s.y - q.y);
                if dd < bd {
                    bd = dd;
                    best = Some(P2::new(s.x, s.y));
                }
            }
        }
        Ok(best.map(|b| vec![b.x, b.y, 0.0]))
    }
    fn alert(&self, msg: &str) -> LFut<HR<()>> {
        let me = self.me.clone();
        let msg = msg.to_string();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            let Some(c) = h.ctx() else { return Err(HostErr::Missing) };
            let buttons = vec![("cancel".to_string(), c.t("lisp.alert.ok"))];
            match c.choose(&c.t("lisp.alert.title"), &msg, &buttons).await.map_err(cmd_err)? {
                Some(_) => Ok(()),
                None => {
                    c.print(&msg);
                    Ok(())
                }
            }
        })
    }
    fn getfiled(&self, title: &str, name: &str, ext: &str, flags: i32) -> LFut<HR<Option<String>>> {
        let me = self.me.clone();
        let (title, name, ext) = (title.to_string(), name.to_string(), ext.to_string());
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            let Some(c) = h.ctx() else { return Err(HostErr::Missing) };
            let _ = (name, flags);
            let accept =
                if ext.is_empty() { String::new() } else { format!(".{}", ext.trim_start_matches('.').split(';').next().unwrap_or("")) };
            let r = c.sh.host.borrow_mut().pick_text_file(&title, &accept);
            let file = match r {
                crate::engine::Pick::Now(f) => f,
                crate::engine::Pick::Later => match c.ask(Kind::File, title.clone(), Opts::default()).await.map_err(cmd_err)? {
                    Reply::Files(f) => f.into_iter().next(),
                    _ => None,
                },
                crate::engine::Pick::Unsupported => return Err(HostErr::Missing),
            };
            let Some(f) = file else { return Ok(None) };
            let text = cc_lisp::reader::decode_source(&f.bytes);
            if let Some(s) = h.session.upgrade()
                && let Ok(mut slot) = s.lisp.try_borrow_mut()
                && let Some(l) = slot.as_mut()
            {
                l.put_file(&cc_lisp::value::lstr(&f.name), &cc_lisp::value::lstr(&text));
            }
            Ok(Some(f.name))
        })
    }
    fn has(&self, method: &str) -> bool {
        match method {
            "getfiled" => self.shared().is_some_and(|sh| sh.host.borrow().has_getfiled()),
            _ => false,
        }
    }
    fn redraw(&self) -> HR<()> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        sh.host.borrow_mut().preview(Vec::new());
        Ok(())
    }
    fn layout_names(&self) -> HR<Vec<String>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        Ok(sh.doc.borrow().layouts.iter().map(|l| l.name.clone()).collect())
    }
    fn vla(&self, op: VlaOp) -> HR<Pv> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        super::vlahost::vla_call(self, &sh, op)
    }

    fn dialog_open(&self, tree: &cc_lisp::host::Tile) -> HR<()> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        if sh.host.borrow_mut().dialog_open(tree) { Ok(()) } else { Err(HostErr::Missing) }
    }
    fn dialog_op(&self, op: cc_lisp::host::DlgOp) -> HR<Option<String>> {
        let Some(sh) = self.shared() else { return Err(HostErr::Missing) };
        sh.host.borrow_mut().dialog_op(op).ok_or(HostErr::Missing)
    }
    fn dialog_run(&self) -> LFut<HR<cc_lisp::host::DlgEvent>> {
        let me = self.me.clone();
        Box::pin(async move {
            let Some(h) = me.upgrade() else { return Err(HostErr::Missing) };
            let Some(c) = h.ctx() else { return Err(HostErr::Missing) };
            let r = c.sh.host.borrow_mut().dialog_run();
            match r {
                crate::engine::Pick::Now(ev) => Ok(ev),
                crate::engine::Pick::Unsupported => Err(HostErr::Missing),
                crate::engine::Pick::Later => match c.ask(Kind::File, String::new(), Opts::default()).await.map_err(cmd_err)? {
                    Reply::Dialog(ev) => Ok(ev),
                    _ => Ok(cc_lisp::host::DlgEvent::Done(Some(0))),
                },
            }
        })
    }
    fn pause(&self) -> Option<LFut<()>> {
        let sh = self.shared()?;
        if !sh.host.borrow().pumps() {
            return None;
        }
        sh.st.borrow_mut().wants_poll = true;
        Some(Box::pin(Turn(false)))
    }
}

/// `parseInt(String(h), 16)`: leading white space, a sign, an optional 0x, hex digits up to the first other.
fn parse_int16(h: &str) -> Option<i64> {
    let t = js::trim_start(h);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let digits: String = t.chars().take_while(char::is_ascii_hexdigit).collect();
    if digits.is_empty() {
        return None;
    }
    let v = i64::from_str_radix(digits.get(..15.min(digits.len())).unwrap_or(""), 16).ok()?;
    Some(if neg { v.saturating_neg() } else { v })
}

/// A table's records (tblnext / tblsearch): (name, its DXF list).
fn table_list(doc: &Doc, table: &str) -> Option<Vec<(String, Pairs)>> {
    Some(match table {
        "LAYER" => doc.layers.values().map(|l| (l.name.clone(), dxflist::layer_pairs(l))).collect(),
        "BLOCK" => doc
            .blocks
            .values()
            .filter(|b| {
                let l = b.name.to_lowercase();
                !(l.starts_with("*model_space") || l.starts_with("*paper_space"))
            })
            .map(|b| {
                let first = (!b.entities.is_empty()).then(|| EId::S(format!("b:{}:0", b.name).into()));
                (b.name.clone(), dxflist::block_pairs(b, first))
            })
            .collect(),
        "LTYPE" => {
            let mut v = vec![
                ("ByBlock".to_string(), dxflist::ltype_pairs("ByBlock", "", &[])),
                ("ByLayer".to_string(), dxflist::ltype_pairs("ByLayer", "", &[])),
                ("Continuous".to_string(), dxflist::ltype_pairs("Continuous", "Solid line", &[])),
            ];
            for lt in doc.linetypes.values() {
                v.push((lt.name.clone(), dxflist::ltype_pairs(&lt.name, &lt.description, &lt.pattern)));
            }
            v
        }
        "VIEW" => doc.views.iter().map(|v| (v.name.clone(), dxflist::view_pairs(v))).collect(),
        "STYLE" => {
            let mut p = dxflist::name_pairs("STYLE", "Standard");
            p.extend([(40, Pv::Num(0.0)), (41, Pv::Num(1.0)), (3, Pv::Str("txt".into()))]);
            vec![("Standard".to_string(), p)]
        }
        "DIMSTYLE" => vec![("Standard".to_string(), dxflist::name_pairs("DIMSTYLE", "Standard"))],
        "APPID" => ["ACAD", "CADCRAFT"].iter().map(|n| ((*n).to_string(), dxflist::name_pairs("APPID", n))).collect(),
        "VPORT" => vec![("*Active".to_string(), dxflist::name_pairs("VPORT", "*Active"))],
        "UCS" => crate::cmd::ucscmd::named_ucs(doc)
            .into_iter()
            .map(|(name, u)| {
                let mut p = dxflist::name_pairs("UCS", &name);
                p.push((10, pt3(u.x, u.y)));
                p.push((11, pt3(u.ang.jcos(), u.ang.jsin())));
                p.push((12, pt3(-u.ang.jsin(), u.ang.jcos())));
                (name, p)
            })
            .collect(),
        _ => return None,
    })
}

impl CadHost {
    fn getvar_impl(&self, sh: &Shared, name: &str) -> Pv {
        let n = name.to_uppercase();
        let d = sh.doc.borrow();
        let st = sh.st.borrow();
        let ucs = active_ucs(&d);
        let meta = |k: &str| d.meta.get(k).filter(|v| !v.is_null());
        match n.as_str() {
            "CLAYER" => Pv::Str(d.current.layer.clone()),
            "CECOLOR" => match d.current.color.to_value() {
                Value::Null => Pv::Str("BYLAYER".into()),
                Value::Number(x) if x.as_f64() == Some(0.0) => Pv::Str("BYBLOCK".into()),
                Value::Number(x) => Pv::Str(js::num_to_string(x.as_f64().unwrap_or(0.0))),
                Value::String(s) => {
                    let h = |a: usize| i64::from_str_radix(s.get(a..a.saturating_add(2)).unwrap_or("0"), 16).unwrap_or(0);
                    Pv::Str(format!("RGB:{},{},{}", h(1), h(3), h(5)))
                }
                _ => Pv::Str("BYLAYER".into()),
            },
            "CELTYPE" => Pv::Str(d.current.linetype.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "ByLayer".into())),
            "CELWEIGHT" => match d.current.lineweight.filter(|w| w.is_finite()) {
                Some(w) => i(if w < 0.0 { w } else { js::round(w * 100.0) }),
                None => Pv::Int(-1),
            },
            "TEXTSIZE" => Pv::Num(d.current.text_height),
            "LUNITS" => i(meta("lunits").map_or(2.0, js::to_number)),
            "LUPREC" => i(meta("luprec").map_or(4.0, js::to_number)),
            "INSUNITS" => Pv::Int(match meta("units").and_then(Value::as_str) {
                Some("inches") => 1,
                Some("feet") => 2,
                Some("millimeters") => 4,
                Some("centimeters") => 5,
                Some("meters") => 6,
                _ => 0,
            }),
            "LTSCALE" => meta("ltscale").map_or(Pv::Num(1.0), |v| Pv::Num(js::to_number(v))),
            "DIMSCALE" | "DIMTXT" | "DIMASZ" => {
                let k = match n.as_str() {
                    "DIMSCALE" => "scale",
                    "DIMTXT" => "txt",
                    _ => "asz",
                };
                let dflt = if k == "scale" { 1.0 } else { 2.5 };
                Pv::Num(d.current.dim.get(k).filter(|v| !v.is_null()).map_or(dflt, js::to_number))
            }
            "DIMDEC" => i(d.current.dim.get("dec").filter(|v| !v.is_null()).map_or(2.0, js::to_number)),
            "CTAB" => Pv::Str(CadHost::space_name(&d, d.space)),
            "UCSORG" => ucs.map_or(Pv::pt(&[0.0, 0.0, 0.0]), |u| pt3(u.x, u.y)),
            "UCSXDIR" => ucs.map_or(Pv::pt(&[1.0, 0.0, 0.0]), |u| pt3(u.ang.jcos(), u.ang.jsin())),
            "UCSYDIR" => ucs.map_or(Pv::pt(&[0.0, 1.0, 0.0]), |u| pt3(-u.ang.jsin(), u.ang.jcos())),
            "WORLDUCS" => Pv::Int(i32::from(ucs.is_none())),
            "UCSNAME" => match ucs {
                None => Pv::Str("*WORLD*".into()),
                Some(u) => Pv::Str(
                    crate::cmd::ucscmd::named_ucs(&d)
                        .into_iter()
                        .find(|(_, x)| (x.x - u.x).abs() < 1e-9 && (x.y - u.y).abs() < 1e-9 && (x.ang - u.ang).abs() < 1e-12)
                        .map(|(n, _)| n)
                        .unwrap_or_default(),
                ),
            },
            "VIEWTWIST" => Pv::Num(js::or(sh.host.borrow().view_twist(), 0.0)),
            "TILEMODE" => Pv::Int(i32::from(d.in_model() || !d.layouts.iter().any(|l| l.space == d.space))),
            "CVPORT" => Pv::Int(if d.layout().is_some() { 1 } else { 2 }),
            "DWGNAME" => {
                let name = d.meta.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("Drawing1").to_string();
                let low = name.to_lowercase();
                Pv::Str(if low.ends_with(".dwg") || low.ends_with(".dxf") { name } else { format!("{name}.dwg") })
            }
            "DWGPREFIX" => Pv::Str(String::new()),
            "CMDACTIVE" => Pv::Int(i32::from(self.cmd_active())),
            "CMDNAMES" => {
                Pv::Str(self.nested.borrow().as_ref().map_or_else(|| st.running.unwrap_or("").to_string(), |n| n.name.to_string()))
            }
            "ERRNO" => Pv::Int(self.st.borrow().errno),
            "UNDOCTL" => Pv::Int(5),
            "ACADVER" => Pv::Str("24.0".into()),
            "PRODUCT" => Pv::Str("CadCraft".into()),
            "PLATFORM" => Pv::Str("CadCraft (browser)".into()),
            "LASTPOINT" => st.last_point.map_or(Pv::pt(&[0.0, 0.0, 0.0]), |p| pt3(p.x, p.y)),
            "EXTMIN" | "EXTMAX" => {
                let mut b: Option<Bounds> = None;
                for e in d.entities() {
                    let x = entity_bounds(e, Some(&d.blocks));
                    if !valid_bounds(&x) {
                        continue;
                    }
                    b = Some(match b {
                        None => x,
                        Some(o) => Bounds {
                            min_x: js::min(o.min_x, x.min_x),
                            min_y: js::min(o.min_y, x.min_y),
                            max_x: js::max(o.max_x, x.max_x),
                            max_y: js::max(o.max_y, x.max_y),
                        },
                    });
                }
                match (b, n.as_str()) {
                    (None, "EXTMIN") => Pv::pt(&[1e20, 1e20, 1e20]),
                    (None, _) => Pv::pt(&[-1e20, -1e20, -1e20]),
                    (Some(b), "EXTMIN") => pt3(b.min_x, b.min_y),
                    (Some(b), _) => pt3(b.max_x, b.max_y),
                }
            }
            "VIEWCTR" => {
                let h = sh.host.borrow();
                match (h.view_rect(), h.view_center()) {
                    (Some(_), Some(cpt)) => pt3(cpt.x, cpt.y),
                    _ => Pv::pt(&[0.0, 0.0, 0.0]),
                }
            }
            "VIEWSIZE" => sh.host.borrow().view_rect().map_or(Pv::Num(100.0), |(_, h)| Pv::Num(h)),
            "SCREENSIZE" => {
                let px = sh.host.borrow().screen_px();
                let (w, h) = px.unwrap_or((1000.0, 600.0));
                Pv::List(vec![Pv::Num(js::or(Some(w), 1000.0)), Pv::Num(js::or(Some(h), 600.0))])
            }
            "DATE" => {
                let (now, _) = st.clock.unwrap_or((cc_clock::now_ms(), cc_clock::Zone::System));
                Pv::Num(now / 86_400_000.0 + 2_440_587.5)
            }
            "CDATE" => {
                let (now, zone) = st.clock.unwrap_or((cc_clock::now_ms(), cc_clock::Zone::System));
                Pv::Num(cdate(now, zone))
            }
            "MILLISECS" => {
                let ms = (cc_clock::now_ms() - self.t0).max(0.0) % 2_147_483_647.0;
                i(ms.floor())
            }
            _ => self.st.borrow().sys.get(&n).cloned().unwrap_or(Pv::Nil),
        }
    }

    fn setvar_impl(&self, sh: &Shared, name: &str, value: Pv) -> bool {
        let n = name.to_uppercase();
        if SYSVARS_RO.contains(&n.as_str()) {
            return false;
        }
        let v = match &value {
            Pv::Int(k) => Pv::Num(f64::from(*k)),
            other => other.clone(),
        };
        let num = v.num();
        match n.as_str() {
            "CLAYER" => {
                let Pv::Str(s) = &v else { return false };
                if !sh.doc.borrow().layers.contains_key(s.as_str()) {
                    return false;
                }
                let r = sh.doc.borrow_mut().set_layer(s, &Obj::new(), true);
                sh.flush_notes();
                r.is_ok()
            }
            "CECOLOR" => {
                let s = dxflist::js_str(&v).to_uppercase();
                let col: Option<Value> = if s == "BYLAYER" {
                    Some(Value::Null)
                } else if s == "BYBLOCK" {
                    Some(json!(0))
                } else if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) && (1.0..=255.0).contains(&js::str_to_number(&s)) {
                    Some(cc_cad::entity::num(js::str_to_number(&s)))
                } else if let Some(rest) = s.strip_prefix("RGB:") {
                    let parts: Vec<&str> = rest.split(',').collect();
                    if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
                        Some(Value::String(format!(
                            "#{}",
                            parts.iter().map(|p| format!("{:02x}", byte_of(js::str_to_number(p)))).collect::<String>()
                        )))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let mut d = sh.doc.borrow_mut();
                match col {
                    Some(c) => {
                        d.current.color = cc_cad::Color::from_value(&c).unwrap_or(cc_cad::Color::Null);
                        true
                    }
                    None => {
                        d.current.color = cc_cad::Color::Null;
                        false
                    }
                }
            }
            "CELTYPE" => {
                let s = dxflist::js_str(&v);
                sh.doc.borrow_mut().current.linetype = if s.eq_ignore_ascii_case("bylayer") { None } else { Some(s) };
                true
            }
            "CELWEIGHT" => {
                let w = num.unwrap_or(f64::NAN);
                sh.doc.borrow_mut().current.lineweight = if w == -1.0 {
                    None
                } else if w < 0.0 {
                    Some(w)
                } else {
                    Some(w / 100.0)
                };
                true
            }
            "TEXTSIZE" => {
                let Some(w) = num.filter(|w| *w > 0.0) else { return false };
                sh.doc.borrow_mut().current.text_height = w;
                true
            }
            "LUNITS" | "LUPREC" => {
                let Some(w) = num.filter(|w| w.fract() == 0.0 && w.is_finite()) else { return false };
                let mut patch = Obj::new();
                patch.insert(if n == "LUNITS" { "lunits" } else { "luprec" }.into(), cc_cad::entity::num(w));
                let r = sh.doc.borrow_mut().change("SETVAR", |d| d.set_meta(&patch));
                sh.flush_notes();
                r.is_ok()
            }
            "ERRNO" => {
                self.st.borrow_mut().errno = cc_lisp::value::to_i32(num.unwrap_or(0.0));
                true
            }
            "DIMSCALE" | "DIMTXT" | "DIMASZ" => {
                let Some(w) = num.filter(|w| *w > 0.0) else { return false };
                let k = match n.as_str() {
                    "DIMSCALE" => "scale",
                    "DIMTXT" => "txt",
                    _ => "asz",
                };
                sh.doc.borrow_mut().current.dim.insert(k.into(), cc_cad::entity::num(w));
                true
            }
            "DIMDEC" => {
                let k = cc_lisp::value::to_i32(num.unwrap_or(0.0));
                sh.doc.borrow_mut().current.dim.insert("dec".into(), json!(k));
                true
            }
            "ORTHOMODE" | "SNAPMODE" | "GRIDMODE" => {
                let w = num.unwrap_or(f64::NAN);
                self.st.borrow_mut().sys.insert(n.clone(), i(w));
                let key = match n.as_str() {
                    "ORTHOMODE" => "ortho",
                    "SNAPMODE" => "snapGrid",
                    _ => "grid",
                };
                sh.host.borrow_mut().set_toggle(key, w != 0.0 && !w.is_nan());
                true
            }
            _ => {
                let old = self.st.borrow().sys.get(&n).cloned();
                let Some(old) = old else { return false };
                let want_int = matches!(old, Pv::Int(_));
                if want_int {
                    let Some(w) = num.filter(|w| w.fract() == 0.0 && w.is_finite()) else { return false };
                    self.st.borrow_mut().sys.insert(n, i(w));
                } else {
                    self.st.borrow_mut().sys.insert(n, value);
                }
                true
            }
        }
    }
}

/// CDATE: the local time as YYYYMMDD.HHMMSSmm (a number).
pub fn cdate(now: f64, zone: cc_clock::Zone) -> f64 {
    let local = zone.local_ms(now);
    let days = (local / 86_400_000.0).floor();
    let ms_day = local - days * 86_400_000.0;
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if m <= 2.0 { y + 1.0 } else { y };
    let hh = (ms_day / 3_600_000.0).floor();
    let mi = ((ms_day - hh * 3_600_000.0) / 60_000.0).floor();
    let ss = ((ms_day - hh * 3_600_000.0 - mi * 60_000.0) / 1000.0).floor();
    let cs = ((ms_day - hh * 3_600_000.0 - mi * 60_000.0 - ss * 1000.0) / 10.0).floor();
    let pad = |x: f64, n: usize| format!("{:0>n$}", js::num_to_string(x));
    let text = format!("{}{}{}.{}{}{}{}", js::num_to_string(y), pad(m, 2), pad(d, 2), pad(hh, 2), pad(mi, 2), pad(ss, 2), pad(cs, 2));
    js::str_to_number(&text)
}

/// For tests: a lasso error type the module needs.
pub fn lerr(m: &str) -> LErr {
    LErr::new(m)
}

/// A colour part as a byte (0..=255, NaN = 0), as `Number(p).toString(16)` reads for valid parts.
fn byte_of(x: f64) -> u8 {
    let r = x.clamp(0.0, 255.0).trunc();
    (0u8..=255).find(|b| f64::from(*b) == r).unwrap_or(0)
}
