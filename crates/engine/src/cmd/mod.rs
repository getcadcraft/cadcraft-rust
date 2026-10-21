//! The drafting commands (commands.js `def(...)`): the registry, lookup by name or alias.

pub mod arrange;
pub mod block;
pub mod blockcopy;
pub mod clipcmd;
pub mod corner;
pub mod dim;
pub mod draw;
pub mod draw2;
pub mod hatch;
pub mod inquiry;
pub mod layers;
pub mod layout;
pub mod libplace;
pub mod modify;
pub mod pictures;
pub mod publish;
pub mod qselect;
pub mod trim;
pub mod ucscmd;
pub mod units;
pub mod view;
pub mod visedit;
pub mod xref;

use crate::engine::Ctx;
use crate::prompt::CmdFuture;

/// HATCH's settings (`E.hatch`).
#[derive(Clone, Debug, PartialEq)]
pub struct HatchMemo {
    pub pattern: String,
    pub scale: f64,
    pub angle: f64,
}

/// What commands remember for the session (commands.js keeps these on the engine: `E.hatch`, …).
#[derive(Clone, Debug, Default)]
pub struct Memo {
    pub hatch: Option<HatchMemo>,
    /// the last dimension made (DIMCONTINUE / DIMBASELINE start from it)
    pub last_dim: Option<u64>,
    /// FILLET's radius (`E.filletR`)
    pub fillet_r: Option<f64>,
    /// FILLET / CHAMFER trim mode (`E.cornerTrim`; None = trim)
    pub corner_trim: Option<bool>,
    pub chamfer: Option<corner::ChamferMemo>,
    pub lengthen: Option<arrange::LengthenMemo>,
    pub match_on: Option<arrange::MatchOn>,
    /// XLINE Offset's distance (`E.xlineOffset`)
    pub xline_offset: Option<crate::engine::OffsetMemo>,
    pub mtext_ls: Option<f64>,
    pub mtext_just: Option<String>,
    pub revcloud: Option<draw2::CloudMemo>,
    pub mleader_asz: Option<f64>,
    pub mleader_dogleg: Option<f64>,
    /// the block BLOCK made or INSERT placed last (`E.lastBlock`)
    pub last_block: Option<String>,
    pub last_insert_scale: Option<f64>,
    pub last_insert_rot: Option<f64>,
    pub att_modes: block::AttModes,
    /// the block editor while it is open (doc.js `blockEdit`)
    pub block_edit: Option<block::BlockEdit>,
    /// what the last LAYISO did (`doc.layIso`)
    pub lay_iso: Option<layers::LayIso>,
    /// SELECTSIMILAR's switches (`E.similarOn`: layer, color, name, pattern, height)
    pub similar_on: Option<[bool; 5]>,
    /// UCS Previous (doc.js `ucsPrev`, this session; None = the world)
    pub ucs_prev: Vec<Option<crate::ucs::Ucs>>,
}

/// One command.
pub struct CmdDef {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    /// Enter at an empty command line does not repeat it
    pub no_repeat: bool,
    /// it leaves the selection as it was
    pub keep_selection: bool,
    pub run: fn(Ctx) -> CmdFuture,
}

const fn def(name: &'static str, aliases: &'static [&'static str], run: fn(Ctx) -> CmdFuture) -> CmdDef {
    CmdDef { name, aliases, no_repeat: false, keep_selection: false, run }
}

/// `{ noRepeat: true, keepSelection: true }`
const fn quiet(d: CmdDef) -> CmdDef {
    CmdDef { no_repeat: true, keep_selection: true, ..d }
}

pub static COMMANDS: &[CmdDef] = &[
    def("LINE", &["L"], |c| Box::pin(draw::line(c))),
    def("PLINE", &["PL"], |c| Box::pin(draw::pline(c))),
    def("TTR", &[], |c| Box::pin(draw::circle_ttr_cmd(c))),
    def("CIRCLE", &["C"], |c| Box::pin(draw::circle(c))),
    def("ARC", &["A"], |c| Box::pin(draw::arc(c))),
    def("RECTANG", &["REC", "RECTANGLE"], |c| Box::pin(draw::rectang(c))),
    def("POLYGON", &["POL"], |c| Box::pin(draw::polygon(c))),
    def("ELLIPSE", &["EL"], |c| Box::pin(draw::ellipse(c))),
    def("POINT", &["PO"], |c| Box::pin(draw::point(c))),
    def("TEXT", &["DT", "DTEXT"], |c| Box::pin(draw::text(c))),
    def("XLINE", &["XL"], |c| Box::pin(draw2::xline(c))),
    def("RAY", &[], |c| Box::pin(draw2::ray(c))),
    def("HATCH", &["H", "BH", "BHATCH", "-HATCH"], |c| Box::pin(hatch::hatch(c))),
    def("MTEXT", &["MT", "T"], |c| Box::pin(draw2::mtext(c))),
    def("REVCLOUD", &["RC"], |c| Box::pin(draw2::revcloud_cmd(c))),
    def("MLEADER", &["MLD", "MLEAD"], |c| Box::pin(draw2::mleader(c))),
    def("TEXTEDIT", &["ED", "DDEDIT"], |c| Box::pin(draw2::textedit(c))),
    def("ERASE", &["E", "DELETE"], |c| Box::pin(modify::erase(c))),
    def("MOVE", &["M"], |c| Box::pin(modify::move_like(c, false))),
    def("COPY", &["CO", "CP"], |c| Box::pin(modify::move_like(c, true))),
    def("ROTATE", &["RO"], |c| Box::pin(modify::rotate(c))),
    def("SCALE", &["SC"], |c| Box::pin(modify::scale(c))),
    def("MIRROR", &["MI"], |c| Box::pin(modify::mirror(c))),
    def("OFFSET", &["O"], |c| Box::pin(modify::offset(c))),
    def("TRIM", &["TR"], |c| Box::pin(trim::trim_or_extend(c, true))),
    def("EXTEND", &["EX"], |c| Box::pin(trim::trim_or_extend(c, false))),
    def("STRETCH", &["S"], |c| Box::pin(arrange::stretch(c))),
    def("ARRAY", &["AR"], |c| Box::pin(arrange::array_cmd(c, None))),
    def("ARRAYRECT", &[], |c| Box::pin(arrange::array_cmd(c, Some("rect")))),
    def("ARRAYPOLAR", &[], |c| Box::pin(arrange::array_cmd(c, Some("polar")))),
    def("BREAK", &["BR"], |c| Box::pin(arrange::break_cmd(c, false))),
    def("BREAKATPOINT", &[], |c| Box::pin(arrange::break_cmd(c, true))),
    def("LENGTHEN", &["LEN"], |c| Box::pin(arrange::lengthen(c))),
    def("JOIN", &["J"], |c| Box::pin(arrange::join(c))),
    def("MATCHPROP", &["MA", "PAINTER"], |c| Box::pin(arrange::matchprop(c))),
    def("FILLET", &["F"], |c| Box::pin(corner::fillet(c))),
    def("CHAMFER", &["CHA"], |c| Box::pin(corner::chamfer(c))),
    def("EXPLODE", &["X"], |c| Box::pin(corner::explode(c))),
    def("DIMLINEAR", &["DLI", "DIMLIN"], |c| Box::pin(dim::dimlinear(c))),
    def("DIMALIGNED", &["DAL", "DIMALI"], |c| Box::pin(dim::dimaligned(c))),
    def("DIMRADIUS", &["DRA", "DIMRAD"], |c| Box::pin(dim::radial(c, "radius"))),
    def("DIMDIAMETER", &["DDI", "DIMDIA"], |c| Box::pin(dim::radial(c, "diameter"))),
    def("DIMANGULAR", &["DAN", "DIMANG"], |c| Box::pin(dim::dimangular(c))),
    def("DIMCONTINUE", &["DCO", "DIMCONT"], |c| Box::pin(dim::chain(c, false))),
    def("DIMBASELINE", &["DBA", "DIMBASE"], |c| Box::pin(dim::chain(c, true))),
    quiet(def("DIMSCALE", &[], |c| Box::pin(dim::dimvar(c, "DIMSCALE")))),
    quiet(def("DIMTXT", &[], |c| Box::pin(dim::dimvar(c, "DIMTXT")))),
    quiet(def("DIMASZ", &[], |c| Box::pin(dim::dimvar(c, "DIMASZ")))),
    quiet(def("DIMDEC", &[], |c| Box::pin(dim::dimvar(c, "DIMDEC")))),
    quiet(def("DIMGAP", &[], |c| Box::pin(dim::dimvar(c, "DIMGAP")))),
    quiet(def("DIMEXE", &[], |c| Box::pin(dim::dimvar(c, "DIMEXE")))),
    quiet(def("DIMEXO", &[], |c| Box::pin(dim::dimvar(c, "DIMEXO")))),
    quiet(def("DIMLUNIT", &[], |c| Box::pin(dim::dimlunit(c)))),
    CmdDef { keep_selection: true, ..def("COPYCLIP", &[], |c| Box::pin(clipcmd::copyclip(c))) },
    def("CUTCLIP", &[], |c| Box::pin(clipcmd::cutclip(c))),
    CmdDef { keep_selection: true, ..def("COPYBASE", &[], |c| Box::pin(clipcmd::copybase(c))) },
    CmdDef { keep_selection: true, ..def("PASTECLIP", &[], |c| Box::pin(clipcmd::pasteclip(c))) },
    CmdDef { keep_selection: true, ..def("PASTEORIG", &[], |c| Box::pin(clipcmd::pasteorig(c))) },
    CmdDef { keep_selection: true, ..def("DUPLICATE", &["DUP"], |c| Box::pin(inquiry::duplicate(c))) },
    quiet(def("SELECTALL", &["AI_SELALL"], |c| Box::pin(inquiry::selectall(c)))),
    CmdDef { no_repeat: true, ..def("DESELECT", &["AI_DESELECT", "SELECTNONE"], |c| Box::pin(inquiry::deselect(c))) },
    quiet(def("SELECT", &[], |c| Box::pin(inquiry::select_cmd(c)))),
    quiet(def("QSELECT", &["QS"], |c| Box::pin(qselect::qselect(c)))),
    CmdDef { keep_selection: true, ..def("SELECTSIMILAR", &["SIM"], |c| Box::pin(qselect::selectsimilar(c))) },
    quiet(def("FIND", &[], |c| Box::pin(qselect::find(c)))),
    quiet(def("UNITS", &["UN", "DDUNITS"], |c| Box::pin(units::units(c)))),
    def("DIST", &["DI"], |c| Box::pin(inquiry::dist_cmd(c))),
    def("AREA", &["AA"], |c| Box::pin(inquiry::area(c))),
    def("MEASUREGEOM", &["MEA"], |c| Box::pin(inquiry::measuregeom(c))),
    def("ID", &[], |c| Box::pin(inquiry::id_cmd(c))),
    def("LIST", &["LI", "LS"], |c| Box::pin(inquiry::list(c))),
    def("BLOCK", &["B", "BMAKE"], |c| Box::pin(block::block(c))),
    def("ATTDEF", &["ATT", "DDATTDEF", "-ATTDEF"], |c| Box::pin(block::attdef(c))),
    CmdDef { no_repeat: true, ..def("BEDIT", &["BE"], |c| Box::pin(block::bedit(c))) },
    CmdDef { no_repeat: true, ..def("BCLOSE", &[], |c| Box::pin(block::bclose(c))) },
    CmdDef { no_repeat: true, ..def("BVSTATE", &["BVS"], |c| Box::pin(visedit::bvstate(c))) },
    def("BVHIDE", &["BVH"], |c| Box::pin(visedit::bvhide(c))),
    def("BVSHOW", &["BVSH"], |c| Box::pin(visedit::bvshow(c))),
    def("INSERT", &["I", "DDINSERT"], |c| Box::pin(block::insert(c))),
    quiet(def("LIBRARY", &["LIB"], |c| Box::pin(block::library(c)))),
    def("LAYISO", &[], |c| Box::pin(layers::layiso(c))),
    quiet(def("LAYUNISO", &[], |c| Box::pin(layers::layuniso(c)))),
    def("LAYOFF", &[], |c| Box::pin(layers::by_pick(c, "LAYOFF", false, "layoff"))),
    def("LAYFRZ", &[], |c| Box::pin(layers::by_pick(c, "LAYFRZ", true, "layfrz"))),
    def("LAYMCUR", &[], |c| Box::pin(layers::laymcur(c))),
    quiet(def("LAYON", &[], |c| Box::pin(layers::all_layers(c, "LAYON", true, "layon")))),
    quiet(def("LAYTHW", &[], |c| Box::pin(layers::all_layers(c, "LAYTHW", false, "laythw")))),
    def("LAYMRG", &[], |c| Box::pin(layers::laymrg(c))),
    def("LAYDEL", &[], |c| Box::pin(layers::laydel(c))),
    def("RENAME", &["-RENAME", "REN"], |c| Box::pin(layers::rename(c))),
    quiet(def("LINETYPE", &["LT", "CELTYPE"], |c| Box::pin(layers::linetype(c)))),
    quiet(def("LTSCALE", &["LTS"], |c| Box::pin(layers::ltscale(c)))),
    quiet(def("LWEIGHT", &["LW", "LINEWEIGHT"], |c| Box::pin(layers::lweight(c)))),
    quiet(def("PURGE", &["PU"], |c| Box::pin(inquiry::purge(c)))),
    CmdDef { no_repeat: true, ..def("LAYOUT", &["LO", "NEWLAYOUT"], |c| Box::pin(layout::layout(c))) },
    def("MVIEW", &["MV"], |c| Box::pin(layout::mview(c))),
    quiet(def("VPORTS", &["VIEWPORTS"], |c| Box::pin(layout::vports(c)))),
    CmdDef { no_repeat: true, ..def("MSPACE", &["MS"], |c| Box::pin(layout::mspace(c))) },
    CmdDef { no_repeat: true, ..def("PSPACE", &["PS"], |c| Box::pin(layout::pspace(c))) },
    def("VPLAYER", &[], |c| Box::pin(layout::vplayer(c))),
    def("VIEWLABEL", &["VL"], |c| Box::pin(layout::viewlabel(c))),
    quiet(def("NAMEDVIEW", &["NV", "VIEWS"], |c| Box::pin(layout::namedview(c)))),
    quiet(def("UCS", &[], |c| Box::pin(ucscmd::ucs(c)))),
    quiet(def("TURNLEFT", &[], |c| Box::pin(ucscmd::quarter_turn(c, -1.0)))),
    quiet(def("TURNRIGHT", &[], |c| Box::pin(ucscmd::quarter_turn(c, 1.0)))),
    quiet(def("TURNWORLD", &[], |c| Box::pin(ucscmd::quarter_turn(c, 0.0)))),
    quiet(def("PLAN", &[], |c| Box::pin(ucscmd::plan(c)))),
    quiet(def("LWDISPLAY", &["LWT"], |c| Box::pin(ucscmd::lwdisplay(c)))),
    quiet(def("POLARANG", &[], |c| Box::pin(ucscmd::polarang(c)))),
    CmdDef { keep_selection: true, ..def("COUNT", &["BCOUNT"], |c| Box::pin(ucscmd::count(c))) },
    def("XATTACH", &["XA"], |c| Box::pin(xref::xattach(c))),
    def("XREF", &["XR", "-XREF"], |c| Box::pin(xref::xref(c))),
    def("XCLIP", &["XC"], |c| Box::pin(xref::xclip(c))),
    def("IMAGEATTACH", &["IAT", "IMAGEATT"], |c| Box::pin(pictures::imageattach(c))),
    CmdDef { no_repeat: true, ..def("IMAGE", &["IM"], |c| Box::pin(pictures::image(c))) },
    def("IMAGECLIP", &["ICL"], |c| Box::pin(pictures::imageclip(c))),
    // PUBLISH is its own command in 0.5 (layertools.js, several sheets in one PDF): not an alias of PLOT here
    def("PUBLISH", &["-PUBLISH"], |c| Box::pin(publish::publish(c))),
    quiet(def("PLOT", &["PRINT", "EXPORTPDF"], |c| Box::pin(view::plot(c)))),
    quiet(def("_PLOTWIN", &[], |c| Box::pin(view::plot_window(c)))),
    quiet(def("PROPERTIES", &["PR", "PROPS", "CH", "MO", "DDMODIFY"], |c| Box::pin(view::properties(c)))),
    quiet(def("PROPERTIESCLOSE", &["PRCLOSE"], |c| Box::pin(view::properties_close(c)))),
    quiet(def("QPMODE", &["QP"], |c| Box::pin(view::qpmode(c)))),
    CmdDef { no_repeat: true, keep_selection: true, ..def("ZOOM", &["Z"], |c| Box::pin(view::zoom(c))) },
    CmdDef { no_repeat: true, ..def("UNDO", &["U"], |c| Box::pin(view::undo(c))) },
    CmdDef { no_repeat: true, ..def("REDO", &[], |c| Box::pin(view::redo(c))) },
];

/// A command by name or alias (case-insensitive; a leading `_ . ' -` is ignored).
pub fn lookup(name: &str) -> Option<&'static CmdDef> {
    let raw = cc_cad::js::trim(name).to_uppercase();
    if raw.starts_with(['-', '_'])
        && let Some(c) = COMMANDS.iter().find(|c| c.name == raw)
    {
        return Some(c);
    }
    let u = raw.trim_start_matches(['_', '.', '\'', '-']);
    COMMANDS.iter().find(|c| c.name == u).or_else(|| COMMANDS.iter().find(|c| c.aliases.contains(&u)))
}

/// The names (for the CLI and tests).
pub fn names() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}
