//! The drawing side of CadCraft's LISP (host-cad.js's CadHost, as cadfns.js, vla.js and dcl.js call it).
//!
//! 0.5 passes plain data across (a worker boundary in the browser); here the same shapes are [`Pv`] values: reals,
//! integers (`{ i }`), strings, points (lists of reals), entity names (`{ e }`), selection sets (`{ ids }`), symbols,
//! VLA handles and booleans. Methods that may wait for the person (a point, a running command, a dialog) return a
//! future; the rest answer at once. A method the drawing side does not have answers [`HostErr::Missing`] (0.5: "no
//! drawing: …", "no dialogs here", or nil where the function checks first).

use std::rc::Rc;

use crate::interp::LFut;
use crate::value::{EId, VlaH};

/// How a call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum HostErr {
    /// Esc, or a cancelled prompt
    Cancel,
    /// the call's message (it becomes the LISP error's)
    Msg(String),
    /// the drawing side has no such method
    Missing,
}

pub type HR<T> = Result<T, HostErr>;

/// Plain data (dxflist.js shapes).
#[derive(Clone, Debug, PartialEq)]
pub enum Pv {
    /// null / undefined / false
    Nil,
    /// true
    T,
    Num(f64),
    Str(String),
    /// an array (a point, a list)
    List(Vec<Pv>),
    /// `{ i }`
    Int(i32),
    /// `{ e }`
    E(EId),
    /// `{ ids }`
    Ids(Vec<EId>),
    /// `{ sym }`
    Sym(String),
    /// `{ vla }`
    Vla(VlaH),
    /// `{ bool }` (VLA booleans)
    Bool(bool),
    /// `{ arr, vt }` (VLA arrays; vt None = doubles)
    Arr(Vec<Pv>, Option<i32>),
    /// `{ min, max }` (GetBoundingBox)
    Bbox([f64; 3], [f64; 3]),
}

impl Pv {
    /// A number (`{ i }` too).
    pub fn num(&self) -> Option<f64> {
        match self {
            Pv::Num(n) => Some(*n),
            Pv::Int(i) => Some(f64::from(*i)),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        if let Pv::Str(s) = self { Some(s) } else { None }
    }
    /// A point (two or three numbers).
    pub fn pt(xs: &[f64]) -> Pv {
        Pv::List(xs.iter().map(|x| Pv::Num(*x)).collect())
    }
}

/// A DXF list: `[[code, value]…]`.
pub type Pairs = Vec<(i32, Pv)>;

/// What getpoint and friends ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetKind {
    Point,
    Corner,
    Dist,
    Angle,
    Orient,
    Real,
    Int,
    Kword,
    Str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GetOpts {
    pub msg: Option<String>,
    pub base: Option<Vec<f64>>,
    pub kw: Option<String>,
    pub flags: i32,
}

/// The answer to getpoint and friends.
#[derive(Clone, Debug, PartialEq)]
pub enum GetAns {
    /// Enter (nil)
    None,
    /// an initget keyword (its value)
    Kw(String),
    /// arbitrary text (initget 128)
    Text(String),
    Value(Pv),
}

/// entsel / nentsel.
#[derive(Clone, Debug, PartialEq)]
pub enum EntselAns {
    None,
    Kw(String),
    Pick { e: EId, p: Vec<f64>, matrix: Option<Vec<Vec<f64>>>, parents: Vec<EId> },
}

/// entmake's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum MakeAns {
    Error,
    /// ENDBLK: the block's name
    Block(String),
    /// an object made: its entity name
    E(EId),
    /// a BLOCK start, a member of the block being made, a LAYER
    Made,
}

/// A DCL dialog's event: a tile was used, or the dialog closed.
#[derive(Clone, Debug, PartialEq)]
pub enum DlgEvent {
    Done(Option<i32>),
    Event { key: String, value: String, reason: i32, x: i32, y: i32 },
    Nothing,
}

/// One operation on an open dialog (dcl.js `dialogOp`).
#[derive(Clone, Debug, PartialEq)]
pub enum DlgOp {
    Set(String, String),
    Get(String),
    Attr(String, String),
    Mode(String, i32),
    List { key: String, op: i32, index: i32, items: Vec<String> },
    Done(i32),
}

/// A DCL tile, references and derived types expanded (dcl.js `resolveTile`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub kind: String,
    /// attributes in the order written (later ones replace earlier ones of the same name)
    pub attrs: Vec<(String, DclVal)>,
    pub children: Vec<Tile>,
}

/// A DCL attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum DclVal {
    Str(String),
    Num(f64),
    Bool(bool),
    Id(String),
}

impl Tile {
    pub fn attr(&self, name: &str) -> Option<&DclVal> {
        self.attrs.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// A VLA operation (vla.js → vla-host.js `vlaCall(op, h, args)`).
#[derive(Clone, Debug, PartialEq)]
pub enum VlaOp {
    Get(VlaH, String),
    Put(VlaH, String, Pv),
    Call(VlaH, String, Vec<Pv>),
    Has(VlaH, String, bool),
    Can(VlaH, String),
    Items(VlaH),
    FromEname(EId),
    ToEname(VlaH),
    Erased(VlaH),
}

/// The drawing.
pub trait Host {
    // ---- input (may wait for the person) ----
    fn get(&self, kind: GetKind, o: GetOpts) -> LFut<HR<GetAns>>;
    fn entsel(&self, msg: Option<String>, kw: Option<String>, nested: bool) -> LFut<HR<EntselAns>>;
    /// ssget: None = nothing selected
    fn ssget(&self, mode: Option<String>, pts: Vec<Vec<f64>>, filter: Option<Pairs>) -> LFut<HR<Option<Vec<EId>>>>;
    fn sssetfirst(&self, ids: &[EId]) -> HR<()>;
    fn ssgetfirst(&self) -> HR<Option<Vec<EId>>>;
    /// (command …): answer a command's prompts with these values
    fn command(&self, args: Vec<Pv>, strict: bool) -> LFut<HR<()>>;
    /// a command's name by name or alias (vl-cmdf, getcname)
    fn getcname(&self, name: &str) -> HR<Option<String>>;

    // ---- variables ----
    fn getvar(&self, name: &str) -> HR<Pv>;
    /// false: rejected (read-only, unknown, a bad value)
    fn setvar(&self, name: &str, v: Pv) -> HR<bool>;
    fn trans(&self, p: &[f64], from: i32, to: i32, disp: bool) -> HR<Vec<f64>>;

    // ---- objects ----
    fn entget(&self, id: &EId) -> HR<Option<Pairs>>;
    fn entmod(&self, pairs: Pairs) -> HR<bool>;
    fn entmake(&self, pairs: Pairs) -> HR<MakeAns>;
    fn entdel(&self, id: &EId) -> HR<bool>;
    fn entlast(&self) -> HR<Option<EId>>;
    fn entnext(&self, id: Option<&EId>) -> HR<Option<EId>>;
    fn handent(&self, h: &str) -> HR<Option<EId>>;

    // ---- tables ----
    fn tblnext(&self, table: &str, rewind: bool) -> HR<Option<Pairs>>;
    fn tblsearch(&self, table: &str, name: &str, setnext: bool) -> HR<Option<Pairs>>;
    fn tblobjname(&self, table: &str, name: &str) -> HR<Option<EId>>;

    // ---- misc ----
    fn textbox(&self, pairs: Pairs) -> HR<Option<[[f64; 3]; 2]>>;
    fn osnap(&self, p: &[f64], modes: &str) -> HR<Option<Vec<f64>>>;
    fn alert(&self, msg: &str) -> LFut<HR<()>>;
    /// getfiled (Missing: the function answers nil)
    fn getfiled(&self, _title: &str, _name: &str, _ext: &str, _flags: i32) -> LFut<HR<Option<String>>> {
        Box::pin(async { Err(HostErr::Missing) })
    }
    fn redraw(&self) -> HR<()>;
    fn layout_names(&self) -> HR<Vec<String>>;
    fn vla(&self, op: VlaOp) -> HR<Pv>;

    // ---- dialogs (DCL) ----
    fn dialog_open(&self, _tree: &Tile) -> HR<()> {
        Err(HostErr::Missing)
    }
    fn dialog_op(&self, _op: DlgOp) -> HR<Option<String>> {
        Err(HostErr::Missing)
    }
    fn dialog_run(&self) -> LFut<HR<DlgEvent>> {
        Box::pin(async { Err(HostErr::Missing) })
    }

    /// Does the drawing side have this optional method ("getfiled", "colorDialog")? 0.5 checks before it reads
    /// the arguments.
    fn has(&self, _method: &str) -> bool {
        false
    }
    /// acad_colordlg's dialog → the colour picked (None: cancelled)
    fn color_dialog(&self, _color: i32) -> LFut<HR<Option<i32>>> {
        Box::pin(async { Err(HostErr::Missing) })
    }

    /// A pause for the window every few thousand steps (Esc gets through); None = run straight on.
    fn pause(&self) -> Option<LFut<()>> {
        None
    }
}

/// A host that is not there: every call is Missing (the pure interpreter, the fuzz target).
#[derive(Debug, Default)]
pub struct NoHost;

pub fn no_host() -> Option<Rc<dyn Host>> {
    None
}
