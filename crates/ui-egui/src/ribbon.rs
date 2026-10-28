//! CadCraft's ribbon and menus (0.5 `input-cadcraft.js`, `ui.inc`'s toolbar, the shell's `app-input-ui.js`): the three
//! input styles as data, and the bar that shows the chosen one over the drafting editor.
//!
//! * [`COMMANDS`]: the command registry of the toolbar, the Layers panel's tools, the status bar's aids, the Model / 3D
//!   tabs and the 3D group (input-cadcraft.js `COMMANDS`, ids never renamed). `cmd` is what the app's own button runs:
//!   its `data-cmd` (typed at the command line, first word run and the rest fed to its prompts, as cadcraft.js's toolbar
//!   does), or `toggle:<name>` / `tab:<n>` / `app:<name>` for the status bar's toggles, the space tabs and the keys-only
//!   commands.
//! * [`STYLES`]: CadCraft (the toolbar of `ui.inc`, [`TOOLBAR`]), Drafting classic and Ribbon (a File menu, quick
//!   buttons, tabs of panels), each with its label overrides. Labels are 0.5's catalog keys (`input.cmd.<id>`,
//!   `input.m.<label>`, `input.style.<id>`, `toolbar.*`, `ui.*`).
//!
//! Everything a button does goes through `UiApp::run` (`input.run {name: id}`), so the CLI and tests reach it.
//! Spec gap: the shell's own words (`shell.input.*`: "it is not in this version", the style legend) are not in the
//! app's catalog; they are written here in plain English, as the shell's en.json has them.

use egui::{Color32, Pos2, Rect, RichText, Stroke, Vec2};
use serde_json::{Value, json};

use crate::{Params, UiApp, edit::lang_text};

/// One command of the registry.
#[derive(Clone, Copy, Debug)]
pub struct Cmd {
    pub id: &'static str,
    pub group: &'static str,
    /// the icon's name (0.5's Bootstrap icon name without `bi-`; drawn in code here, [`paint_icon`])
    pub icon: &'static str,
    /// what the app's button runs (see the module docs)
    pub cmd: &'static str,
    /// works on the Model tab and the sheets
    pub tab: bool,
    /// the 3D group's button (runs on the 3D tab)
    pub in3d: bool,
    /// the button exists only sometimes (the 3D group, Open in 3D)
    pub optional: bool,
    /// CadCraft's own keys
    pub keys: &'static [&'static str],
    /// a key that never moves (Delete, Esc)
    pub fixed: bool,
    /// only a key (no button)
    pub key_only: bool,
}

const fn c(id: &'static str, group: &'static str, icon: &'static str, cmd: &'static str) -> Cmd {
    Cmd { id, group, icon, cmd, tab: false, in3d: false, optional: false, keys: &[], fixed: false, key_only: false }
}
const fn t(id: &'static str, group: &'static str, icon: &'static str, cmd: &'static str) -> Cmd {
    Cmd { tab: true, ..c(id, group, icon, cmd) }
}
const fn k(id: &'static str, group: &'static str, icon: &'static str, cmd: &'static str, keys: &'static [&'static str]) -> Cmd {
    Cmd { keys, ..c(id, group, icon, cmd) }
}
const fn d3(id: &'static str, icon: &'static str, cmd: &'static str) -> Cmd {
    Cmd { in3d: true, optional: true, ..c(id, "3d", icon, cmd) }
}

/// input-cadcraft.js `COMMANDS`, in its order.
pub const COMMANDS: &[Cmd] = &[
    c("file.new", "file", "file-earmark-plus", "NEW"),
    k("file.open", "file", "folder2-open", "OPEN", &["Ctrl+O"]),
    k("file.save", "file", "download", "SAVE", &["Ctrl+S"]),
    k("file.saveas", "file", "file-earmark-arrow-down", "SAVEAS", &["Ctrl+Shift+S"]),
    k("file.plot", "file", "printer", "PLOT", &["Ctrl+P"]),
    t("draw.line", "draw", "slash-lg", "LINE"),
    t("draw.pline", "draw", "vector-pen", "PLINE"),
    t("draw.circle", "draw", "circle", "CIRCLE"),
    t("draw.arc", "draw", "bezier2", "ARC"),
    t("draw.rectang", "draw", "square", "RECTANG"),
    t("draw.polygon", "draw", "hexagon", "POLYGON"),
    t("draw.ellipse", "draw", "egg", "ELLIPSE"),
    t("draw.text", "draw", "type", "TEXT"),
    t("draw.mtext", "draw", "text-paragraph", "MTEXT"),
    t("draw.hatch", "draw", "grid-3x3", "HATCH"),
    t("draw.point", "draw", "dot", "POINT"),
    t("draw.circle-ttr", "draw", "circle", "CIRCLE T"),
    t("draw.circle-ttt", "draw", "circle", "CIRCLE TTT"),
    t("draw.xline", "draw", "arrows", "XLINE"),
    t("draw.ray", "draw", "arrow-up-right", "RAY"),
    t("modify.move", "modify", "arrows-move", "MOVE"),
    t("modify.copy", "modify", "copy", "COPY"),
    t("modify.rotate", "modify", "arrow-repeat", "ROTATE"),
    t("modify.scale", "modify", "arrows-angle-expand", "SCALE"),
    t("modify.mirror", "modify", "symmetry-vertical", "MIRROR"),
    t("modify.stretch", "modify", "arrows-expand-vertical", "STRETCH"),
    t("modify.offset", "modify", "distribute-vertical", "OFFSET"),
    t("modify.trim", "modify", "scissors", "TRIM"),
    t("modify.extend", "modify", "arrow-up-right", "EXTEND"),
    t("modify.fillet", "modify", "bezier", "FILLET"),
    t("modify.chamfer", "modify", "triangle-half", "CHAMFER"),
    t("modify.explode", "modify", "asterisk", "EXPLODE"),
    t("modify.erase", "modify", "eraser", "ERASE"),
    t("modify.array", "modify", "grid-3x2", "ARRAY"),
    t("modify.join", "modify", "link", "JOIN"),
    t("modify.matchprop", "modify", "brush", "MATCHPROP"),
    t("modify.break", "modify", "scissors", "BREAK"),
    t("modify.lengthen", "modify", "arrows", "LENGTHEN"),
    t("annotate.dimlinear", "annotate", "arrow-left-right", "DIMLINEAR"),
    t("annotate.dimaligned", "annotate", "arrows-angle-contract", "DIMALIGNED"),
    t("annotate.dimangular", "annotate", "triangle", "DIMANGULAR"),
    t("annotate.dimradius", "annotate", "record-circle", "DIMRADIUS"),
    t("annotate.dimdiameter", "annotate", "slash-circle", "DIMDIAMETER"),
    t("annotate.mleader", "annotate", "arrow-up-left", "MLEADER"),
    t("annotate.dimcontinue", "annotate", "arrow-left-right", "DIMCONTINUE"),
    t("annotate.dimbaseline", "annotate", "arrow-left-right", "DIMBASELINE"),
    t("annotate.revcloud", "annotate", "cloud", "REVCLOUD"),
    t("param.auto", "parametric", "magic", "AUTOCONSTRAIN"),
    t("param.geom", "parametric", "bounding-box", "GEOMCONSTRAINT"),
    t("param.dim", "parametric", "bounding-box-circles", "DIMCONSTRAINT"),
    t("param.parameters", "parametric", "sliders", "PARAMETERS"),
    t("param.horizontal", "parametric", "dash-lg", "GCHORIZONTAL"),
    t("param.vertical", "parametric", "grip-vertical", "GCVERTICAL"),
    t("param.parallel", "parametric", "pause", "GCPARALLEL"),
    t("param.perpendicular", "parametric", "plus-lg", "GCPERPENDICULAR"),
    t("param.coincident", "parametric", "record", "GCCOINCIDENT"),
    t("param.tangent", "parametric", "circle", "GCTANGENT"),
    t("param.equal", "parametric", "distribute-horizontal", "GCEQUAL"),
    t("param.fix", "parametric", "lock", "GCFIX"),
    t("param.collinear", "parametric", "three-dots", "GCCOLLINEAR"),
    t("param.concentric", "parametric", "bullseye", "GCCONCENTRIC"),
    t("param.symmetric", "parametric", "symmetry-vertical", "GCSYMMETRIC"),
    t("param.dclinear", "parametric", "arrow-left-right", "DCLINEAR"),
    t("param.dcaligned", "parametric", "arrows-angle-contract", "DCALIGNED"),
    t("param.dcangular", "parametric", "triangle", "DCANGULAR"),
    t("param.dcradius", "parametric", "record-circle", "DCRADIUS"),
    t("param.dcdiameter", "parametric", "slash-circle", "DCDIAMETER"),
    t("param.dcconvert", "parametric", "arrow-left-right", "DCCONVERT"),
    t("param.delete", "parametric", "x-octagon", "DELCONSTRAINT"),
    t("param.bars-show", "parametric", "eye", "CONSTRAINTBAR S"),
    t("param.bars-hide", "parametric", "eye-slash", "CONSTRAINTBAR H"),
    t("param.dcdisplay", "parametric", "eye", "DCDISPLAY"),
    t("param.swbout", "parametric", "file-earmark-code", "SWBOUT"),
    t("blocks.make", "blocks", "box", "BLOCK"),
    t("blocks.insert", "blocks", "box-arrow-in-down", "INSERT"),
    t("blocks.library", "blocks", "collection", "LIBRARY"),
    t("blocks.bedit", "blocks", "pencil-square", "BEDIT"),
    t("blocks.attdef", "blocks", "tag", "ATTDEF"),
    t("blocks.count", "blocks", "123", "COUNT"),
    t("blocks.xattach", "blocks", "paperclip", "XATTACH"),
    t("blocks.xref", "blocks", "link-45deg", "XREF"),
    t("blocks.delblocks", "blocks", "trash", "DELBLOCKS"),
    t("layers.iso", "layers", "layers", "LAYISO"),
    t("layers.uniso", "layers", "layers-fill", "LAYUNISO"),
    t("layers.off", "layers", "eye-slash", "LAYOFF"),
    t("layers.frz", "layers", "snow", "LAYFRZ"),
    t("layers.mcur", "layers", "check2-circle", "LAYMCUR"),
    t("layers.on", "layers", "eye", "LAYON"),
    t("layers.thw", "layers", "sun", "LAYTHW"),
    t("layers.mrg", "layers", "intersect", "LAYMRG"),
    t("layers.del", "layers", "trash", "LAYDEL"),
    t("layers.rename", "layers", "input-cursor-text", "RENAME"),
    t("sheets.layout", "sheets", "layout-text-window", "LAYOUT"),
    t("sheets.mview", "sheets", "window", "MVIEW"),
    k("edit.undo", "edit", "arrow-counterclockwise", "UNDO", &["Ctrl+Z"]),
    k("edit.redo", "edit", "arrow-clockwise", "REDO", &["Ctrl+Y", "Ctrl+Shift+Z"]),
    k("edit.copy", "edit", "clipboard", "COPYCLIP", &["Ctrl+C"]),
    k("edit.paste", "edit", "clipboard-plus", "PASTECLIP", &["Ctrl+V"]),
    k("edit.cut", "edit", "scissors", "CUTCLIP", &["Ctrl+X"]),
    k("edit.paste-in-place", "edit", "clipboard-check", "PASTEORIG", &["Ctrl+Shift+V"]),
    k("edit.duplicate", "edit", "files", "DUPLICATE", &["Ctrl+D"]),
    Cmd { fixed: true, ..k("edit.delete", "edit", "trash", "ERASE", &["Delete"]) },
    k("edit.select-all", "edit", "check2-all", "SELECTALL", &["Ctrl+A"]),
    Cmd { fixed: true, ..k("edit.select-none", "edit", "x-square", "DESELECT", &["Escape"]) },
    t("edit.qselect", "edit", "funnel", "QSELECT"),
    t("edit.similar", "edit", "ui-checks", "SELECTSIMILAR"),
    t("edit.find", "edit", "search", "FIND"),
    t("view.extents", "view", "arrows-fullscreen", "ZOOM E"),
    t("view.window", "view", "zoom-in", "ZOOM W"),
    t("view.previous", "view", "skip-backward", "ZOOM P"),
    t("view.split-2v", "view", "layout-split", "SPLIT 2V"),
    t("view.split-2h", "view", "distribute-vertical", "SPLIT 2H"),
    t("view.split-4", "view", "grid", "SPLIT 4"),
    t("view.split-1", "view", "square", "SPLIT SI"),
    t("view.linkviews", "view", "link", "LINKVIEWS"),
    t("view.turn-left", "view", "arrow-counterclockwise", "TURNLEFT"),
    t("view.turn-right", "view", "arrow-clockwise", "TURNRIGHT"),
    t("view.turn-world", "view", "compass", "TURNWORLD"),
    t("view.maplayers", "view", "map", "MAPLAYERS"),
    t("view.pcattach", "view", "cloud-plus", "POINTCLOUDATTACH"),
    t("view.pointcloud", "view", "cloud-haze2", "POINTCLOUD"),
    Cmd { optional: true, ..c("view.open3d", "view", "box-arrow-up-right", "OPENIN3D") },
    c("view.model", "view", "square", "tab:1"),
    c("view.3d", "view", "box", "tab:2"),
    t("tools.dist", "tools", "rulers", "DIST"),
    t("tools.area", "tools", "bounding-box", "AREA"),
    t("tools.measuregeom", "tools", "rulers", "MEASUREGEOM"),
    t("tools.compare", "tools", "layout-split", "COMPARESETTINGS"),
    t("tools.appload", "tools", "filetype-js", "APPLOAD"),
    t("tools.aliasedit", "tools", "card-text", "ALIASEDIT"),
    t("tools.shortcuts", "tools", "keyboard", "SHORTCUTS"),
    k("aids.osnap", "aids", "magnet", "toggle:osnap", &["F3"]),
    k("aids.grid", "aids", "grid-3x3-gap", "toggle:grid", &["F7"]),
    k("aids.ortho", "aids", "plus-lg", "toggle:ortho", &["F8"]),
    k("aids.polar", "aids", "compass", "toggle:polar", &["F10"]),
    k("aids.otrack", "aids", "bullseye", "toggle:otrack", &["F11"]),
    k("aids.dyn", "aids", "input-cursor", "toggle:dyn", &["F12"]),
    c("aids.lwt", "aids", "border-width", "toggle:lwt"),
    d3("3d.extrude", "box-arrow-up", "EXTRUDE"),
    d3("3d.presspull", "arrows-vertical", "PRESSPULL"),
    d3("3d.box", "box", "BOX"),
    d3("3d.cylinder", "database", "CYLINDER"),
    d3("3d.wedge", "triangle-half", "WEDGE"),
    d3("3d.move", "arrows-move", "MOVE"),
    d3("3d.copy", "copy", "COPY"),
    d3("3d.rotate", "arrow-repeat", "ROTATE"),
    d3("3d.erase", "eraser", "ERASE"),
    d3("3d.orbit", "globe", "3DORBIT"),
    d3("3d.revolve", "arrow-clockwise", "REVOLVE"),
    d3("3d.sweep", "bezier2", "SWEEP"),
    d3("3d.union", "union", "UNION"),
    d3("3d.subtract", "subtract", "SUBTRACT"),
    d3("3d.intersect", "intersect", "INTERSECT"),
    d3("3d.view-iso", "badge-3d", "VIEW I"),
    d3("3d.view-top", "square", "VIEW T"),
    d3("3d.view-bottom", "square", "VIEW BO"),
    d3("3d.view-front", "layout-sidebar-inset", "VIEW F"),
    d3("3d.view-back", "layout-sidebar", "VIEW BA"),
    d3("3d.view-left", "layout-sidebar", "VIEW L"),
    d3("3d.view-right", "layout-sidebar-inset-reverse", "VIEW R"),
    d3("3d.persp", "camera", "VIEW P"),
    d3("3d.parallel", "camera-video", "VIEW PA"),
    d3("3d.view-all", "arrows-fullscreen", "VIEW E"),
    d3("3d.shaded", "circle-fill", "VSCURRENT S"),
    d3("3d.edges", "circle-half", "VSCURRENT E"),
    d3("3d.hidden", "circle", "VSCURRENT H"),
    d3("3d.wire", "bounding-box", "VSCURRENT W"),
    d3("3d.xray", "transparency", "VSCURRENT X"),
    d3("3d.glbout", "box-arrow-up-right", "GLBOUT"),
    d3("3d.objout", "box-arrow-up-right", "OBJOUT"),
    d3("3d.stlout", "printer", "STLOUT"),
    Cmd { key_only: true, ..k("app.settings", "app", "gear", "app:settings", &["Ctrl+,"]) },
    Cmd { key_only: true, ..k("app.palette", "app", "search", "app:palette", &["Ctrl+K"]) },
    Cmd { key_only: true, ..k("app.help", "app", "question-circle", "app:help", &["F1"]) },
];

pub const GROUPS: &[&str] =
    &["file", "draw", "modify", "annotate", "parametric", "blocks", "layers", "sheets", "edit", "view", "tools", "aids", "3d", "app"];

/// The command of an id.
pub fn cmd(id: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| c.id == id)
}

// ---------------------------------------------------------------------------------------------- the toolbar (ui.inc)

/// A toolbar button: (command, icon, label key, tooltip key).
pub type Tool = (&'static str, &'static str, &'static str, &'static str);
/// A menu item: (command, label key, the key shown) or a separator (`"-"`, "", "").
pub type Item = (&'static str, &'static str, &'static str);

/// One group of the toolbar.
pub struct Group {
    /// `data-tb-key`; its label is `toolbar.group.<key in lower case>`
    pub key: &'static str,
    pub tools: &'static [Tool],
    /// the group's "More" menu (`$__more`): label `toolbar.more`, tooltip `toolbar.more.title` with {group}
    pub more: &'static [Item],
    /// the Edit / Parametric menu: (summary label key, tooltip key, list label key, items)
    pub menu: Option<(&'static str, &'static str, &'static str, &'static [Item])>,
}

pub const SEP: Item = ("-", "", "");

/// ui.inc `$__tools`, `$__more` and the Edit and Parametric menus. (OPENIN3D in View › More shows only where ModelCraft
/// takes a handed-over drawing: not in this build.)
pub const TOOLBAR: &[Group] = &[
    Group {
        key: "File",
        tools: &[
            ("NEW", "file-earmark-plus", "ui.new", "ui.new_drawing"),
            ("OPEN", "folder2-open", "ui.open", "ui.open_dwg_dxf_ctrl"),
            ("SAVE", "download", "ui.save", "ui.save_copy_drawing_s"),
            ("SAVEAS", "file-earmark-arrow-down", "ui.save_as", "ui.save_copy_dwg_dxf"),
            ("PLOT", "printer", "ui.plot", "ui.plot_pdf_ctrl_p"),
        ],
        more: &[],
        menu: None,
    },
    Group {
        key: "Draw",
        tools: &[
            ("LINE", "slash-lg", "ui.line", "ui.line.title"),
            ("PLINE", "vector-pen", "ui.polyline", "ui.polyline_pl"),
            ("CIRCLE", "circle", "ui.circle", "ui.circle.title"),
            ("ARC", "bezier2", "ui.arc", "ui.arc.title"),
            ("RECTANG", "square", "ui.rectangle", "ui.rectangle_rec"),
            ("POLYGON", "hexagon", "ui.polygon", "ui.polygon_pol"),
            ("ELLIPSE", "egg", "ui.ellipse", "ui.ellipse_el"),
            ("TEXT", "type", "ui.text", "toolbar.text.title"),
            ("MTEXT", "text-paragraph", "toolbar.mtext", "toolbar.mtext.title"),
            ("HATCH", "grid-3x3", "toolbar.hatch", "toolbar.hatch.title"),
        ],
        more: &[
            ("POINT", "toolbar.point", "PO"),
            ("CIRCLE T", "toolbar.circle.ttr", "C T"),
            ("CIRCLE TTT", "toolbar.circle.ttt", "C TTT"),
            ("XLINE", "toolbar.xline", "XL"),
            ("RAY", "toolbar.ray", "RAY"),
        ],
        menu: None,
    },
    Group {
        key: "Modify",
        tools: &[
            ("MOVE", "arrows-move", "ui.move", "ui.move.title"),
            ("COPY", "copy", "ui.copy", "ui.copy_objects_base_point"),
            ("ROTATE", "arrow-repeat", "ui.rotate", "ui.rotate_ro"),
            ("SCALE", "arrows-angle-expand", "ui.scale", "ui.scale_sc"),
            ("MIRROR", "symmetry-vertical", "ui.mirror", "ui.mirror_mi"),
            ("STRETCH", "arrows-expand-vertical", "toolbar.stretch", "toolbar.stretch.title"),
            ("OFFSET", "distribute-vertical", "ui.offset", "ui.offset.title"),
            ("TRIM", "scissors", "ui.trim", "ui.trim_tr"),
            ("EXTEND", "arrow-up-right", "ui.extend", "ui.extend_ex"),
            ("FILLET", "bezier", "ui.fillet", "ui.fillet.title"),
            ("CHAMFER", "triangle-half", "toolbar.chamfer", "toolbar.chamfer.title"),
            ("EXPLODE", "asterisk", "ui.explode", "ui.explode.title"),
            ("ERASE", "eraser", "ui.erase", "ui.erase_e_delete"),
        ],
        more: &[
            ("ARRAY", "toolbar.array", "AR"),
            ("JOIN", "toolbar.join", "J"),
            ("MATCHPROP", "toolbar.matchprop.long", "MA"),
            ("BREAK", "toolbar.break", "BR"),
            ("LENGTHEN", "toolbar.lengthen", "LEN"),
        ],
        menu: None,
    },
    Group {
        key: "Annotate",
        tools: &[
            ("DIMLINEAR", "arrow-left-right", "ui.linear", "ui.linear_dimension_horizontal_vertical"),
            ("DIMALIGNED", "arrows-angle-contract", "ui.aligned", "ui.aligned_dimension_dal"),
            ("DIMANGULAR", "triangle", "ui.angle", "ui.angular_dimension_dan"),
            ("DIMRADIUS", "record-circle", "ui.radius", "ui.radius_dimension_dra"),
            ("DIMDIAMETER", "slash-circle", "ui.diameter", "ui.diameter_dimension_ddi"),
            ("MLEADER", "arrow-up-left", "toolbar.mleader", "toolbar.mleader.title"),
        ],
        more: &[
            ("DIMCONTINUE", "toolbar.dimcontinue", "DCO"),
            ("DIMBASELINE", "toolbar.dimbaseline", "DBA"),
            ("REVCLOUD", "toolbar.revcloud", "RC"),
        ],
        menu: None,
    },
    Group {
        key: "Parametric",
        tools: &[
            ("AUTOCONSTRAIN", "magic", "ui.auto", "ui.add_coincident_horizontal_vertical"),
            ("GEOMCONSTRAINT", "bounding-box", "ui.constrain", "ui.geomconstraint.title"),
            ("DIMCONSTRAINT", "bounding-box-circles", "ui.dimension", "ui.dimensional_constraint_linear_aligned"),
            ("PARAMETERS", "sliders", "ui.parameters", "ui.parameters_panel_user_parameters"),
        ],
        more: &[],
        menu: Some(("ui.more", "ui.more_constraints_dimensional_constraints", "ui.more_constraints", PARAM_MENU)),
    },
    Group {
        key: "Blocks",
        tools: &[
            ("BLOCK", "box", "ui.make", "ui.make_block_objects_b"),
            ("INSERT", "box-arrow-in-down", "ui.insert", "ui.insert.title"),
            ("LIBRARY", "collection", "ui.library", "ui.library_place_items_check"),
        ],
        more: &[
            ("BEDIT", "toolbar.bedit", "BE"),
            ("ATTDEF", "toolbar.attdef", "ATT"),
            ("COUNT", "toolbar.count", "COUNT"),
            ("XATTACH", "toolbar.xattach", "XA"),
            ("XREF", "toolbar.xref", "XR"),
            ("DELBLOCKS", "toolbar.delblocks", "DELBLOCKS"),
        ],
        menu: None,
    },
    Group {
        key: "Sheets",
        tools: &[
            ("LAYOUT", "layout-text-window", "ui.new_sheet", "ui.new_layout_title_block"),
            ("MVIEW", "window", "ui.viewport", "ui.add_viewport_onto_model"),
        ],
        more: &[],
        menu: None,
    },
    Group {
        key: "Edit",
        tools: &[
            ("UNDO", "arrow-counterclockwise", "ui.undo", "ui.undo_ctrl_z"),
            ("REDO", "arrow-clockwise", "ui.redo", "ui.redo_ctrl_y"),
            ("COPYCLIP", "clipboard", "ui.copy", "ui.copy_selection_clipboard_ctrl"),
            ("PASTECLIP", "clipboard-plus", "ui.paste", "ui.paste_copy_follows_cursor"),
        ],
        more: &[],
        menu: Some(("ui.more", "ui.more_editing_cut_paste", "ui.more_editing", EDIT_MENU)),
    },
    Group {
        key: "View",
        tools: &[
            ("ZOOM E", "arrows-fullscreen", "ui.extents", "ui.zoom_whole_drawing_double"),
            ("ZOOM W", "zoom-in", "ui.window", "ui.zoom_window_z_w"),
            ("ZOOM P", "skip-backward", "toolbar.zoomprev", "toolbar.zoomprev.title"),
        ],
        more: &[
            ("DIST", "toolbar.measure", "DI"),
            ("AREA", "toolbar.area", "AA"),
            ("MEASUREGEOM", "toolbar.measuregeom", "MEA"),
            ("SPLIT 2V", "toolbar.split.2v", "SPLIT"),
            ("SPLIT 2H", "toolbar.split.2h", "SPLIT"),
            ("SPLIT 4", "toolbar.split.4", "SPLIT"),
            ("SPLIT SI", "toolbar.split.1", "SPLIT"),
            ("TURNLEFT", "toolbar.turn.left", "TURNLEFT"),
            ("TURNRIGHT", "toolbar.turn.right", "TURNRIGHT"),
            ("TURNWORLD", "toolbar.turn.world", "TURNWORLD"),
            ("COMPARESETTINGS", "toolbar.compare", "CMPSET"),
            ("MAPLAYERS", "toolbar.maplayers", "ML"),
            ("POINTCLOUDATTACH", "toolbar.pcattach", "PCATTACH"),
            ("POINTCLOUD", "toolbar.pointcloud", "PC"),
            ("LINKVIEWS", "toolbar.linkviews", "LINKVIEWS"),
            ("APPLOAD", "toolbar.appload", "AP"),
            ("ALIASEDIT", "toolbar.aliasedit", "ALIASEDIT"),
            ("SHORTCUTS", "toolbar.shortcuts", "KEYS"),
        ],
        menu: None,
    },
];

const EDIT_MENU: &[Item] = &[
    ("CUTCLIP", "ui.cut", "Ctrl+X"),
    ("COPYCLIP", "ui.copy", "Ctrl+C"),
    ("PASTECLIP", "ui.paste", "Ctrl+V"),
    ("PASTEORIG", "ui.paste_place", "Ctrl+Shift+V"),
    ("DUPLICATE", "ui.duplicate_move", "Ctrl+D"),
    ("ERASE", "ui.delete", "Delete"),
    SEP,
    ("SELECTALL", "ui.select_all", "Ctrl+A"),
    ("DESELECT", "ui.select_none", "Esc"),
    ("QSELECT", "toolbar.qselect", "QS"),
    ("SELECTSIMILAR", "toolbar.similar", "SIM"),
    ("FIND", "toolbar.find", "FIND"),
];

const PARAM_MENU: &[Item] = &[
    ("GCHORIZONTAL", "ui.horizontal", ""),
    ("GCVERTICAL", "ui.vertical", ""),
    ("GCPARALLEL", "ui.parallel", ""),
    ("GCPERPENDICULAR", "ui.perpendicular", ""),
    ("GCCOINCIDENT", "ui.coincident", ""),
    ("GCTANGENT", "ui.tangent", ""),
    ("GCEQUAL", "ui.equal", ""),
    ("GCFIX", "ui.fix", ""),
    ("GCCOLLINEAR", "ui.collinear", ""),
    ("GCCONCENTRIC", "ui.concentric", ""),
    ("GCSYMMETRIC", "ui.symmetric", ""),
    SEP,
    ("DCLINEAR", "ui.linear_dimensional", ""),
    ("DCALIGNED", "ui.aligned_dimensional", ""),
    ("DCANGULAR", "ui.angular_dimensional", ""),
    ("DCRADIUS", "ui.radius_dimensional", ""),
    ("DCDIAMETER", "ui.diameter_dimensional", ""),
    ("DCCONVERT", "ui.convert_dimensions", ""),
    SEP,
    ("DELCONSTRAINT", "ui.delete_constraints", ""),
    ("CONSTRAINTBAR S", "ui.show_constraint_bars", ""),
    ("CONSTRAINTBAR H", "ui.hide_constraint_bars", ""),
    ("DCDISPLAY", "ui.show_hide_dimensional_constraints", ""),
    SEP,
    ("SWBOUT", "ui.macro_solidworks_swb", ""),
];

// ---------------------------------------------------------------------------------------------- the styles

/// A ribbon panel: (label key `input.m.<label>`, command ids).
pub type Panel = (&'static str, &'static [&'static str]);
/// A ribbon tab: (label, panels).
pub type Tab = (&'static str, &'static [Panel]);

/// One input style (input-cadcraft.js `STYLES`).
pub struct Style {
    pub id: &'static str,
    /// "toolbar" | "ribbon"
    pub layout: &'static str,
    pub command_line: bool,
    /// menus: (label, items; "-" = a separator)
    pub menus: &'static [(&'static str, &'static [&'static str])],
    pub quick: &'static [&'static str],
    pub tabs: &'static [Tab],
    /// label overrides: (command id, label key)
    pub labels: &'static [(&'static str, &'static str)],
    /// the mouse: (wheel, zoom speed, buttons that pan in 2D)
    pub pan2d: &'static [&'static str],
}

const FILE_MENU: &[&str] =
    &["file.new", "file.open", "-", "file.save", "file.saveas", "-", "file.plot", "-", "app.settings", "app.palette", "app.help"];
const LAYERS: &[&str] = &[
    "layers.iso",
    "layers.uniso",
    "layers.off",
    "layers.frz",
    "layers.mcur",
    "layers.on",
    "layers.thw",
    "layers.mrg",
    "layers.del",
    "layers.rename",
];
const AIDS: &[&str] = &["aids.osnap", "aids.grid", "aids.ortho", "aids.polar", "aids.otrack", "aids.dyn", "aids.lwt"];
const GEOM: &[&str] = &[
    "param.horizontal",
    "param.vertical",
    "param.parallel",
    "param.perpendicular",
    "param.coincident",
    "param.tangent",
    "param.equal",
    "param.fix",
    "param.collinear",
    "param.concentric",
    "param.symmetric",
];
const TAB_3D: Tab = (
    "3d",
    &[
        ("modeling", &["3d.extrude", "3d.presspull", "3d.box", "3d.cylinder", "3d.wedge", "3d.revolve", "3d.sweep"]),
        ("solidedit", &["3d.union", "3d.subtract", "3d.intersect"]),
        ("modify", &["3d.move", "3d.copy", "3d.rotate", "3d.erase"]),
        (
            "views",
            &[
                "3d.orbit",
                "3d.view-iso",
                "3d.view-top",
                "3d.view-bottom",
                "3d.view-front",
                "3d.view-back",
                "3d.view-left",
                "3d.view-right",
                "3d.persp",
                "3d.parallel",
                "3d.view-all",
            ],
        ),
        ("visual", &["3d.shaded", "3d.edges", "3d.hidden", "3d.wire", "3d.xray"]),
        ("export", &["3d.glbout", "3d.objout", "3d.stlout"]),
    ],
);

/// CadCraft's own style: the toolbar.
const CADCRAFT_STYLE: Style = Style {
    id: "cadcraft",
    layout: "toolbar",
    command_line: false,
    menus: &[],
    quick: &[],
    tabs: &[],
    labels: &[],
    pan2d: &["middle", "right"],
};

pub const STYLES: &[Style] = &[
    CADCRAFT_STYLE,
    Style {
        id: "drafting",
        layout: "ribbon",
        command_line: true,
        menus: &[("file", FILE_MENU)],
        quick: &["file.new", "file.open", "file.save", "file.saveas", "file.plot", "edit.undo", "edit.redo"],
        tabs: &[
            (
                "home",
                &[
                    (
                        "draw",
                        &[
                            "draw.line",
                            "draw.pline",
                            "draw.circle",
                            "draw.arc",
                            "draw.rectang",
                            "draw.polygon",
                            "draw.ellipse",
                            "draw.hatch",
                            "draw.point",
                            "draw.circle-ttr",
                            "draw.circle-ttt",
                            "draw.xline",
                            "draw.ray",
                        ],
                    ),
                    (
                        "modify",
                        &[
                            "modify.move",
                            "modify.rotate",
                            "modify.trim",
                            "modify.extend",
                            "modify.erase",
                            "modify.copy",
                            "modify.mirror",
                            "modify.fillet",
                            "modify.chamfer",
                            "modify.explode",
                            "modify.stretch",
                            "modify.scale",
                            "modify.array",
                            "modify.offset",
                            "modify.join",
                            "modify.break",
                            "modify.lengthen",
                        ],
                    ),
                    ("annotation", &["draw.mtext", "annotate.dimlinear", "annotate.mleader"]),
                    ("layers", LAYERS),
                    ("block", &["blocks.insert", "blocks.make"]),
                    ("properties", &["modify.matchprop"]),
                    (
                        "utilities",
                        &[
                            "tools.dist",
                            "tools.area",
                            "tools.measuregeom",
                            "edit.qselect",
                            "edit.select-all",
                            "edit.similar",
                            "edit.select-none",
                        ],
                    ),
                    ("clipboard", &["edit.paste", "edit.copy", "edit.cut", "edit.paste-in-place", "edit.duplicate", "edit.delete"]),
                ],
            ),
            (
                "insert",
                &[
                    ("block", &["blocks.bedit", "blocks.attdef"]),
                    ("reference", &["blocks.xattach", "blocks.xref"]),
                    ("library", &["blocks.library"]),
                    ("pointcloud", &["view.pcattach", "view.pointcloud"]),
                    ("data", &["blocks.count", "blocks.delblocks"]),
                    ("map", &["view.maplayers"]),
                ],
            ),
            (
                "annotate",
                &[
                    ("text", &["draw.text", "edit.find"]),
                    (
                        "dimensions",
                        &[
                            "annotate.dimaligned",
                            "annotate.dimangular",
                            "annotate.dimradius",
                            "annotate.dimdiameter",
                            "annotate.dimcontinue",
                            "annotate.dimbaseline",
                        ],
                    ),
                    ("markup", &["annotate.revcloud"]),
                ],
            ),
            (
                "parametric",
                &[
                    (
                        "geometric",
                        &[
                            "param.auto",
                            "param.geom",
                            "param.horizontal",
                            "param.vertical",
                            "param.parallel",
                            "param.perpendicular",
                            "param.coincident",
                            "param.tangent",
                            "param.equal",
                            "param.fix",
                            "param.collinear",
                            "param.concentric",
                            "param.symmetric",
                            "param.bars-show",
                            "param.bars-hide",
                        ],
                    ),
                    (
                        "dimensional",
                        &[
                            "param.dim",
                            "param.dclinear",
                            "param.dcaligned",
                            "param.dcangular",
                            "param.dcradius",
                            "param.dcdiameter",
                            "param.dcconvert",
                            "param.dcdisplay",
                        ],
                    ),
                    ("manage", &["param.delete", "param.parameters", "param.swbout"]),
                ],
            ),
            (
                "view",
                &[
                    ("navigate", &["view.extents", "view.window", "view.previous"]),
                    ("viewports", &["view.split-2v", "view.split-2h", "view.split-4", "view.split-1", "view.linkviews"]),
                    ("coordinates", &["view.turn-left", "view.turn-right", "view.turn-world"]),
                    ("model", &["view.model", "view.3d", "view.open3d"]),
                    ("aids", AIDS),
                ],
            ),
            ("manage", &[("customization", &["tools.appload", "tools.aliasedit", "tools.shortcuts"]), ("compare", &["tools.compare"])]),
            ("output", &[("plot", &["file.plot"]), ("sheets", &["sheets.layout", "sheets.mview"])]),
            TAB_3D,
        ],
        labels: &[
            ("file.plot", "input.lbl.drafting.plot"),
            ("draw.mtext", "input.lbl.drafting.mtext"),
            ("annotate.dimlinear", "input.lbl.drafting.dim"),
            ("modify.matchprop", "input.lbl.drafting.matchprop"),
        ],
        pan2d: &["middle"],
    },
    Style {
        id: "ribbon",
        layout: "ribbon",
        command_line: false,
        menus: &[("file", FILE_MENU)],
        quick: &["file.open", "file.save", "edit.undo", "edit.redo"],
        tabs: &[
            (
                "draw",
                &[
                    ("lines", &["draw.line", "draw.pline", "draw.xline", "draw.ray", "draw.point"]),
                    (
                        "shapes",
                        &["draw.circle", "draw.circle-ttr", "draw.circle-ttt", "draw.arc", "draw.rectang", "draw.polygon", "draw.ellipse"],
                    ),
                    ("text", &["draw.text", "draw.mtext"]),
                    ("fill", &["draw.hatch"]),
                ],
            ),
            (
                "modify",
                &[
                    ("clipboard", &["edit.cut", "edit.copy", "edit.paste", "edit.paste-in-place", "edit.duplicate", "edit.delete"]),
                    (
                        "arrange",
                        &["modify.move", "modify.copy", "modify.rotate", "modify.scale", "modify.mirror", "modify.stretch", "modify.array"],
                    ),
                    (
                        "shape",
                        &[
                            "modify.offset",
                            "modify.trim",
                            "modify.extend",
                            "modify.fillet",
                            "modify.chamfer",
                            "modify.break",
                            "modify.lengthen",
                            "modify.join",
                            "modify.explode",
                            "modify.erase",
                            "modify.matchprop",
                        ],
                    ),
                    ("select", &["edit.select-all", "edit.select-none", "edit.qselect", "edit.similar", "edit.find"]),
                ],
            ),
            (
                "annotate",
                &[
                    (
                        "dimensions",
                        &[
                            "annotate.dimlinear",
                            "annotate.dimaligned",
                            "annotate.dimangular",
                            "annotate.dimradius",
                            "annotate.dimdiameter",
                            "annotate.dimcontinue",
                            "annotate.dimbaseline",
                        ],
                    ),
                    ("notes", &["annotate.mleader", "annotate.revcloud"]),
                ],
            ),
            (
                "parametric",
                &[
                    ("constraints", &["param.auto", "param.geom", "param.dim", "param.parameters"]),
                    ("geometric", GEOM),
                    (
                        "dimensional",
                        &[
                            "param.dclinear",
                            "param.dcaligned",
                            "param.dcangular",
                            "param.dcradius",
                            "param.dcdiameter",
                            "param.dcconvert",
                            "param.dcdisplay",
                        ],
                    ),
                    ("manage", &["param.delete", "param.bars-show", "param.bars-hide", "param.swbout"]),
                ],
            ),
            (
                "blocks",
                &[
                    ("blocks", &["blocks.make", "blocks.insert", "blocks.bedit", "blocks.attdef", "blocks.count", "blocks.delblocks"]),
                    ("links", &["blocks.xattach", "blocks.xref", "blocks.library"]),
                    ("layers", LAYERS),
                ],
            ),
            ("sheets", &[("sheets", &["sheets.layout", "sheets.mview"]), ("plot", &["file.plot"])]),
            (
                "view",
                &[
                    ("zoom", &["view.extents", "view.window", "view.previous"]),
                    ("panes", &["view.split-2v", "view.split-2h", "view.split-4", "view.split-1", "view.linkviews"]),
                    ("turn", &["view.turn-left", "view.turn-right", "view.turn-world"]),
                    ("show", &["view.model", "view.3d", "view.open3d", "view.maplayers", "view.pcattach", "view.pointcloud"]),
                    ("aids", AIDS),
                ],
            ),
            TAB_3D,
            (
                "tools",
                &[
                    ("measure", &["tools.dist", "tools.area", "tools.measuregeom"]),
                    ("customize", &["tools.appload", "tools.aliasedit", "tools.shortcuts", "tools.compare"]),
                ],
            ),
        ],
        labels: &[],
        pan2d: &["middle", "right"],
    },
];

pub const STYLE_ORDER: &[&str] = &["cadcraft", "drafting", "ribbon"];

pub fn style(id: &str) -> Option<&'static Style> {
    STYLES.iter().find(|s| s.id == id)
}

/// The label of a command in a style (its override, else `input.cmd.<id>`).
pub fn label(st: &Style, id: &str) -> String {
    let key = st.labels.iter().find(|(c, _)| *c == id).map_or_else(|| format!("input.cmd.{id}"), |(_, k)| (*k).to_string());
    lang_text(&key, &[])
}

/// The toolbar group's label.
pub fn group_label(key: &str) -> String {
    lang_text(&format!("toolbar.group.{}", key.to_ascii_lowercase()), &[])
}

// ---------------------------------------------------------------------------------------------- the bar's state

/// What the window keeps of the bar (0.5 keeps `{style, tab}` in localStorage `cadcraft.input`).
#[derive(Clone, Debug)]
pub struct Bar {
    pub style: String,
    /// the ribbon's open tab
    pub tab: usize,
    /// the open menu: "file", "saveas", "more:<group>", "menu:<group>"
    pub open: Option<String>,
    /// the frame the menu opened in (a click elsewhere closes it, but not the click that opened it)
    opened_at: u64,
    /// the style or tab changed since the host last saved them
    pub dirty: bool,
}

impl Default for Bar {
    fn default() -> Self {
        Bar { style: "cadcraft".into(), tab: 0, open: None, opened_at: 0, dirty: false }
    }
}

impl Bar {
    /// `{style, tab}` as the host keeps it.
    pub fn prefs(&self) -> Value {
        json!({"style": self.style, "tab": self.tab})
    }
    /// Read what the host kept (anything unknown keeps the defaults).
    pub fn load_prefs(&mut self, v: &Value) {
        if let Some(s) = v.get("style").and_then(Value::as_str)
            && style(s).is_some()
        {
            self.style = s.to_string();
        }
        if let Some(t) = v.get("tab").and_then(Value::as_u64) {
            self.tab = usize::try_from(t).unwrap_or(0).min(64);
        }
    }
}

// ---------------------------------------------------------------------------------------------- running

/// What a command's button does in this build.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// typed at the command line: the first word runs, the rest answer its prompts
    Engine(&'static str),
    /// one of the window's own commands
    Ui(&'static str, Option<usize>),
    /// the Save as choice of formats
    SaveAs,
    /// the app's button is not in this build
    Missing,
}

/// The action behind a button's `data-cmd` (NEW / OPEN / SAVE are the window's own buttons; the status bar's toggles
/// the window has are its F3 / F8 / F10).
pub fn action(cmd: &'static str) -> Act {
    match cmd {
        "NEW" => Act::Ui("file.new", None),
        "OPEN" => Act::Ui("file.open", None),
        "SAVE" => Act::Ui("file.save", None),
        "SAVEAS" => Act::SaveAs,
        "toggle:osnap" => Act::Ui("edit.osnap", None),
        "toggle:ortho" => Act::Ui("edit.ortho", None),
        "toggle:polar" => Act::Ui("edit.polar", None),
        "tab:1" => Act::Ui("space.select", Some(0)),
        c if c.starts_with("toggle:") || c.starts_with("tab:") || c.starts_with("app:") => Act::Missing,
        c => Act::Engine(c),
    }
}

/// Click a button whose `data-cmd` is `cmd` (`label` for the status line).
fn press(app: &mut UiApp, cmd: &'static str, label: &str) -> Result<(), String> {
    app.ribbon.open = None;
    match action(cmd) {
        Act::Engine(text) => {
            let ed = app.editor.as_mut().ok_or("input.run: no drawing is open for editing (file.new)")?;
            ed.run_button(text);
            app.sync_editor();
            Ok(())
        }
        Act::Ui(id, index) => app.run(id, &Params { index, ..Default::default() }),
        Act::SaveAs => {
            app.ribbon.open = Some("saveas".into());
            Ok(())
        }
        Act::Missing => {
            // app-input-ui.js: shell.input.cant / shell.input.why.nothere
            app.message = format!("{label}: it is not in this version.");
            Ok(())
        }
    }
}

/// The `input.*` commands: `input.style {name}`, `input.tab {index}`, `input.run {name: command id}`,
/// `input.menu {name?}`, `input.saveas {name: format}`.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "input.style" => {
            let n = params.name.clone().unwrap_or_default();
            if style(&n).is_none() {
                return Err(format!("input.style: name is one of {}", STYLE_ORDER.join(", ")));
            }
            app.ribbon.style = n;
            app.ribbon.open = None;
            app.ribbon.dirty = true;
        }
        "input.tab" => {
            let i = params.index.or_else(|| params.factor.map(|f| f as usize)).ok_or("input.tab: index is required")?;
            let n = style(&app.ribbon.style).map_or(0, |s| s.tabs.len());
            if i >= n {
                return Err(format!("input.tab: index {i} is out of range (0..{n})"));
            }
            app.ribbon.tab = i;
            app.ribbon.dirty = true;
        }
        "input.run" => {
            let n = params.name.clone().ok_or("input.run: name (a command id) is required")?;
            if let Some(c) = cmd(&n) {
                let st = style(&app.ribbon.style).unwrap_or(&CADCRAFT_STYLE);
                return press(app, c.cmd, &label(st, c.id));
            }
            // a toolbar button by its data-cmd ("ZOOM E", "CIRCLE T" …)
            let hit = TOOLBAR.iter().find_map(|g| {
                g.tools
                    .iter()
                    .map(|t| t.0)
                    .chain(g.more.iter().map(|i| i.0))
                    .chain(g.menu.iter().flat_map(|m| m.3.iter().map(|i| i.0)))
                    .find(|c| *c == n && *c != "-")
            });
            let c = hit.ok_or_else(|| format!("input.run: no command {n}"))?;
            return press(app, c, c);
        }
        "input.menu" => {
            let n = params.name.clone().filter(|s| !s.is_empty());
            app.ribbon.open = n;
            app.ribbon.opened_at = u64::MAX;
        }
        "input.saveas" => {
            app.ribbon.open = None;
            let f = params.name.clone().ok_or("input.saveas: name (a format) is required")?;
            return app.run("file.save", &Params { name: Some(f), ..Default::default() });
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------- the dump

fn item_json(st: &Style, id: &str) -> Value {
    if id == "-" {
        return Value::String("-".into());
    }
    json!({"id": id, "label": label(st, id), "cmd": cmd(id).map_or("", |c| c.cmd)})
}

/// The registry and every style, labels in English (`cadcraft-shot ribbon`; forks/tools/ribbondiff.mjs compares it
/// with the frozen 0.5 input-cadcraft.js and ui.inc).
pub fn dump() -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .map(|c| {
            json!({"id": c.id, "group": c.group, "icon": format!("bi-{}", c.icon), "cmd": c.cmd, "label": lang_text(&format!("input.cmd.{}", c.id), &[]),
                "tab": c.tab, "in3d": c.in3d, "optional": c.optional, "defKeys": c.keys, "fixed": c.fixed, "keyOnly": c.key_only})
        })
        .collect();
    let items = |list: &[Item]| -> Vec<Value> {
        list.iter()
            .map(|(c, l, k)| if *c == "-" { Value::String("-".into()) } else { json!({"cmd": c, "label": lang_text(l, &[]), "key": k}) })
            .collect()
    };
    let toolbar: Vec<Value> = TOOLBAR
        .iter()
        .map(|g| {
            let gl = group_label(g.key);
            json!({
                "group": g.key,
                "label": gl,
                "tools": g.tools.iter().map(|(c, i, l, ti)| json!({"cmd": c, "icon": format!("bi-{i}"), "label": lang_text(l, &[]), "title": lang_text(ti, &[])})).collect::<Vec<_>>(),
                "more": if g.more.is_empty() { Value::Null } else {
                    json!({"label": lang_text("toolbar.more", &[]), "title": lang_text("toolbar.more.title", &[("group", gl.clone())]), "items": items(g.more)})
                },
                "menu": g.menu.map_or(Value::Null, |(l, ti, ll, it)| json!({"label": lang_text(l, &[]), "title": lang_text(ti, &[]), "list": lang_text(ll, &[]), "items": items(it)})),
            })
        })
        .collect();
    let mut styles = serde_json::Map::new();
    for st in STYLES {
        let mut o = serde_json::Map::new();
        o.insert("id".into(), json!(st.id));
        o.insert("name".into(), json!(lang_text(&format!("input.style.{}", st.id), &[])));
        o.insert("layout".into(), json!(st.layout));
        o.insert("commandLine".into(), json!(st.command_line));
        o.insert("pan2d".into(), json!(st.pan2d));
        if st.layout == "toolbar" {
            o.insert("toolbar".into(), Value::Array(toolbar.clone()));
        } else {
            let menus: Vec<Value> = st
                .menus
                .iter()
                .map(|(l, it)| json!({"label": lang_text(&format!("input.m.{l}"), &[]), "items": it.iter().map(|x| item_json(st, x)).collect::<Vec<_>>()}))
                .collect();
            o.insert("menus".into(), Value::Array(menus));
            o.insert("quick".into(), st.quick.iter().map(|x| item_json(st, x)).collect());
            let tabs: Vec<Value> = st
                .tabs
                .iter()
                .map(|(l, ps)| {
                    json!({"label": lang_text(&format!("input.m.{l}"), &[]), "panels": ps.iter().map(|(pl, it)| json!({"label": lang_text(&format!("input.m.{pl}"), &[]), "items": it.iter().map(|x| item_json(st, x)).collect::<Vec<_>>()})).collect::<Vec<_>>()})
                })
                .collect();
            o.insert("tabs".into(), Value::Array(tabs));
        }
        styles.insert(st.id.into(), Value::Object(o));
    }
    json!({"commands": commands, "groups": GROUPS, "styleOrder": STYLE_ORDER, "styles": styles})
}

// ---------------------------------------------------------------------------------------------- icons

/// A pen on a 20 × 20 design grid scaled into `r`.
struct Pen<'a> {
    p: &'a egui::Painter,
    r: Rect,
    c: Color32,
    w: f32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.r.left() + x / 20.0 * self.r.width(), self.r.top() + y / 20.0 * self.r.height())
    }
    fn s(&self, v: f32) -> f32 {
        v / 20.0 * self.r.width()
    }
    fn line(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(egui::Shape::line(v, Stroke::new(self.w, self.c)));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(egui::Shape::closed_line(v, Stroke::new(self.w, self.c)));
    }
    fn fill(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(egui::Shape::convex_polygon(v, self.c, Stroke::NONE));
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    }
    fn circle(&self, x: f32, y: f32, r: f32) {
        self.p.circle_stroke(self.at(x, y), self.s(r), Stroke::new(self.w, self.c));
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.p.circle_filled(self.at(x, y), self.s(r), self.c);
    }
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32) {
        let n = 16;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (x + r * a.cos(), y - r * a.sin())
            })
            .collect();
        self.line(&pts);
    }
    fn arrow(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.line(&[(x0, y0), (x1, y1)]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l * 4.0, dy / l * 4.0);
        self.line(&[(x1 - ux - uy * 0.7, y1 - uy + ux * 0.7), (x1, y1), (x1 - ux + uy * 0.7, y1 - uy - ux * 0.7)]);
    }
}

/// Draw one of the bar's icons (thin monochrome line icons on the 20 px grid; a shape for each of 0.5's names, by
/// family where several share one look).
pub fn paint_icon(painter: &egui::Painter, r: Rect, name: &str, color: Color32) {
    let p = Pen { p: painter, r, c: color, w: (r.width() / 14.0).clamp(1.0, 2.0) };
    match name {
        "slash-lg" => p.line(&[(4.0, 16.0), (16.0, 4.0)]),
        "vector-pen" => {
            p.line(&[(3.0, 16.0), (8.0, 6.0), (13.0, 13.0), (17.0, 4.0)]);
            p.dot(8.0, 6.0, 1.2);
            p.dot(13.0, 13.0, 1.2);
        }
        "circle" => p.circle(10.0, 10.0, 6.5),
        "circle-fill" => p.dot(10.0, 10.0, 6.5),
        "circle-half" => {
            p.circle(10.0, 10.0, 6.5);
            p.fill(&[(10.0, 3.5), (13.3, 4.4), (15.6, 6.7), (16.5, 10.0), (15.6, 13.3), (13.3, 15.6), (10.0, 16.5)]);
        }
        "record-circle" => {
            p.circle(10.0, 10.0, 7.0);
            p.dot(10.0, 10.0, 2.5);
        }
        "record" => p.dot(10.0, 10.0, 3.5),
        "bullseye" => {
            p.circle(10.0, 10.0, 7.0);
            p.circle(10.0, 10.0, 4.0);
            p.dot(10.0, 10.0, 1.2);
        }
        "slash-circle" => {
            p.circle(10.0, 10.0, 7.0);
            p.line(&[(5.0, 15.0), (15.0, 5.0)]);
        }
        "bezier2" | "bezier" => {
            p.arc(10.0, 15.0, 7.0, 0.0, 180.0);
            p.dot(3.0, 15.0, 1.2);
            p.dot(17.0, 15.0, 1.2);
        }
        "square" => p.rect(4.0, 4.0, 16.0, 16.0),
        "hexagon" => p.closed(&[(10.0, 3.0), (16.0, 6.5), (16.0, 13.5), (10.0, 17.0), (4.0, 13.5), (4.0, 6.5)]),
        "egg" => {
            p.arc(10.0, 10.0, 7.0, 0.0, 360.0);
            p.line(&[(3.0, 10.0), (17.0, 10.0)]);
        }
        "type" => {
            p.line(&[(4.0, 5.0), (16.0, 5.0)]);
            p.line(&[(10.0, 5.0), (10.0, 16.0)]);
        }
        "text-paragraph" => {
            for y in [5.0, 9.0, 13.0] {
                p.line(&[(3.0, y), (17.0, y)]);
            }
            p.line(&[(3.0, 17.0), (11.0, 17.0)]);
        }
        "grid-3x3" | "grid-3x3-gap" | "grid" | "grid-3x2" => {
            p.rect(3.0, 3.0, 17.0, 17.0);
            p.line(&[(7.7, 3.0), (7.7, 17.0)]);
            p.line(&[(12.3, 3.0), (12.3, 17.0)]);
            if name != "grid-3x2" {
                p.line(&[(3.0, 7.7), (17.0, 7.7)]);
            }
            p.line(&[(3.0, 12.3), (17.0, 12.3)]);
        }
        "dot" => p.dot(10.0, 10.0, 2.5),
        "arrows" | "arrow-left-right" => {
            p.arrow(10.0, 10.0, 17.0, 10.0);
            p.arrow(10.0, 10.0, 3.0, 10.0);
        }
        "arrows-vertical" | "arrows-expand-vertical" => {
            p.arrow(10.0, 10.0, 10.0, 3.0);
            p.arrow(10.0, 10.0, 10.0, 17.0);
            if name == "arrows-expand-vertical" {
                p.line(&[(3.0, 10.0), (17.0, 10.0)]);
            }
        }
        "arrow-up-right" | "box-arrow-up-right" => {
            p.arrow(4.0, 16.0, 16.0, 4.0);
            if name == "box-arrow-up-right" {
                p.line(&[(10.0, 4.0), (4.0, 4.0), (4.0, 16.0), (16.0, 16.0), (16.0, 10.0)]);
            }
        }
        "arrow-up-left" => {
            p.arrow(16.0, 16.0, 4.0, 4.0);
            p.line(&[(16.0, 16.0), (18.0, 16.0)]);
        }
        "arrows-move" => {
            p.arrow(10.0, 10.0, 10.0, 2.5);
            p.arrow(10.0, 10.0, 10.0, 17.5);
            p.arrow(10.0, 10.0, 2.5, 10.0);
            p.arrow(10.0, 10.0, 17.5, 10.0);
        }
        "arrows-angle-expand" | "arrows-fullscreen" => {
            p.arrow(10.0, 10.0, 3.5, 16.5);
            p.arrow(10.0, 10.0, 16.5, 3.5);
            if name == "arrows-fullscreen" {
                p.arrow(10.0, 10.0, 3.5, 3.5);
                p.arrow(10.0, 10.0, 16.5, 16.5);
            }
        }
        "arrows-angle-contract" => {
            p.arrow(3.5, 16.5, 9.0, 11.0);
            p.arrow(16.5, 3.5, 11.0, 9.0);
        }
        "copy" | "files" => {
            p.rect(7.0, 3.0, 17.0, 13.0);
            p.line(&[(13.0, 17.0), (3.0, 17.0), (3.0, 7.0)]);
        }
        "arrow-repeat" => {
            p.arc(10.0, 10.0, 6.0, 30.0, 300.0);
            p.arrow(13.0, 13.5, 15.2, 13.0);
        }
        "arrow-counterclockwise" => {
            p.arc(10.0, 10.0, 6.0, -60.0, 200.0);
            p.arrow(4.4, 12.0, 4.0, 13.5);
        }
        "arrow-clockwise" => {
            p.arc(10.0, 10.0, 6.0, -20.0, 240.0);
            p.arrow(15.6, 12.0, 16.0, 13.5);
        }
        "symmetry-vertical" => {
            p.line(&[(10.0, 2.0), (10.0, 18.0)]);
            p.closed(&[(8.0, 5.0), (8.0, 15.0), (3.0, 15.0)]);
            p.closed(&[(12.0, 5.0), (12.0, 15.0), (17.0, 15.0)]);
        }
        "distribute-vertical" | "distribute-horizontal" => {
            if name == "distribute-vertical" {
                p.line(&[(3.0, 4.0), (17.0, 4.0)]);
                p.rect(5.0, 8.0, 15.0, 12.0);
                p.line(&[(3.0, 16.0), (17.0, 16.0)]);
            } else {
                p.line(&[(4.0, 3.0), (4.0, 17.0)]);
                p.rect(8.0, 5.0, 12.0, 15.0);
                p.line(&[(16.0, 3.0), (16.0, 17.0)]);
            }
        }
        "scissors" => {
            p.circle(6.0, 15.0, 2.5);
            p.circle(14.0, 15.0, 2.5);
            p.line(&[(7.5, 13.0), (15.0, 3.0)]);
            p.line(&[(12.5, 13.0), (5.0, 3.0)]);
        }
        "triangle-half" | "triangle" => {
            p.closed(&[(10.0, 3.0), (17.0, 16.0), (3.0, 16.0)]);
            if name == "triangle-half" {
                p.fill(&[(10.0, 3.0), (10.0, 16.0), (3.0, 16.0)]);
            }
        }
        "asterisk" => {
            p.line(&[(10.0, 3.0), (10.0, 17.0)]);
            p.line(&[(4.0, 6.5), (16.0, 13.5)]);
            p.line(&[(4.0, 13.5), (16.0, 6.5)]);
        }
        "eraser" => {
            p.closed(&[(3.0, 13.0), (11.0, 5.0), (17.0, 11.0), (11.0, 17.0), (7.0, 17.0)]);
            p.line(&[(7.0, 9.0), (13.0, 15.0)]);
        }
        "link" | "link-45deg" | "paperclip" => {
            p.arc(7.0, 13.0, 3.5, 45.0, 315.0);
            p.arc(13.0, 7.0, 3.5, 225.0, 495.0);
            p.line(&[(8.0, 12.0), (12.0, 8.0)]);
        }
        "brush" => {
            p.line(&[(16.0, 3.0), (9.0, 11.0)]);
            p.closed(&[(9.0, 11.0), (6.0, 11.0), (3.0, 17.0), (9.0, 14.0)]);
        }
        "cloud" | "cloud-plus" | "cloud-haze2" => {
            p.arc(7.0, 12.0, 3.5, 90.0, 270.0);
            p.arc(10.0, 9.0, 4.0, 10.0, 170.0);
            p.arc(14.0, 12.0, 3.0, -90.0, 90.0);
            p.line(&[(7.0, 15.5), (14.0, 15.0)]);
            if name == "cloud-plus" {
                p.line(&[(10.0, 10.0), (10.0, 14.0)]);
                p.line(&[(8.0, 12.0), (12.0, 12.0)]);
            }
        }
        "magic" => {
            p.line(&[(3.0, 17.0), (13.0, 7.0)]);
            p.line(&[(15.0, 2.0), (15.0, 6.0)]);
            p.line(&[(13.0, 4.0), (17.0, 4.0)]);
        }
        "bounding-box" | "bounding-box-circles" => {
            p.rect(5.0, 5.0, 15.0, 15.0);
            for (x, y) in [(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0)] {
                if name == "bounding-box" {
                    p.rect(x - 1.5, y - 1.5, x + 1.5, y + 1.5);
                } else {
                    p.circle(x, y, 1.8);
                }
            }
        }
        "sliders" => {
            for (y, x) in [(5.0, 13.0), (10.0, 7.0), (15.0, 11.0)] {
                p.line(&[(3.0, y), (17.0, y)]);
                p.dot(x, y, 1.8);
            }
        }
        "dash-lg" => p.line(&[(3.0, 10.0), (17.0, 10.0)]),
        "grip-vertical" => p.line(&[(10.0, 3.0), (10.0, 17.0)]),
        "pause" => {
            p.line(&[(7.0, 4.0), (7.0, 16.0)]);
            p.line(&[(13.0, 4.0), (13.0, 16.0)]);
        }
        "plus-lg" => {
            p.line(&[(10.0, 3.0), (10.0, 17.0)]);
            p.line(&[(3.0, 10.0), (17.0, 10.0)]);
        }
        "lock" => {
            p.rect(5.0, 9.0, 15.0, 17.0);
            p.arc(10.0, 9.0, 3.5, 0.0, 180.0);
        }
        "three-dots" => {
            for x in [5.0, 10.0, 15.0] {
                p.dot(x, 10.0, 1.4);
            }
        }
        "x-octagon" | "x-square" => {
            if name == "x-square" {
                p.rect(3.0, 3.0, 17.0, 17.0);
            } else {
                p.closed(&[(7.0, 3.0), (13.0, 3.0), (17.0, 7.0), (17.0, 13.0), (13.0, 17.0), (7.0, 17.0), (3.0, 13.0), (3.0, 7.0)]);
            }
            p.line(&[(7.0, 7.0), (13.0, 13.0)]);
            p.line(&[(13.0, 7.0), (7.0, 13.0)]);
        }
        "eye" | "eye-slash" => {
            p.arc(10.0, 14.0, 8.0, 30.0, 150.0);
            p.arc(10.0, 6.0, 8.0, 210.0, 330.0);
            p.circle(10.0, 10.0, 2.2);
            if name == "eye-slash" {
                p.line(&[(3.0, 17.0), (17.0, 3.0)]);
            }
        }
        "box" | "badge-3d" | "box-arrow-up" | "box-arrow-in-down" => {
            p.closed(&[(10.0, 3.0), (17.0, 6.5), (17.0, 14.0), (10.0, 17.5), (3.0, 14.0), (3.0, 6.5)]);
            p.line(&[(3.0, 6.5), (10.0, 10.0), (17.0, 6.5)]);
            p.line(&[(10.0, 10.0), (10.0, 17.5)]);
        }
        "database" => {
            p.arc(10.0, 5.0, 6.0, 0.0, 360.0);
            p.line(&[(4.0, 5.0), (4.0, 15.0)]);
            p.line(&[(16.0, 5.0), (16.0, 15.0)]);
            p.arc(10.0, 15.0, 6.0, 180.0, 360.0);
        }
        "collection" => {
            p.rect(3.0, 8.0, 17.0, 17.0);
            p.line(&[(5.0, 5.5), (15.0, 5.5)]);
            p.line(&[(7.0, 3.0), (13.0, 3.0)]);
        }
        "pencil-square" | "input-cursor-text" | "input-cursor" => {
            p.rect(3.0, 5.0, 15.0, 17.0);
            p.line(&[(9.0, 11.0), (17.0, 3.0)]);
        }
        "tag" => {
            p.closed(&[(3.0, 3.0), (10.0, 3.0), (17.0, 10.0), (10.0, 17.0), (3.0, 10.0)]);
            p.dot(7.0, 7.0, 1.2);
        }
        "123" => {
            p.line(&[(3.0, 7.0), (5.0, 5.0), (5.0, 15.0)]);
            p.line(&[(8.0, 7.0), (10.0, 5.0), (12.0, 7.0), (8.0, 15.0), (12.0, 15.0)]);
            p.line(&[(14.0, 5.0), (17.0, 5.0), (15.0, 9.0), (17.0, 12.0), (14.0, 15.0)]);
        }
        "trash" => {
            p.line(&[(3.0, 5.0), (17.0, 5.0)]);
            p.line(&[(8.0, 5.0), (8.0, 3.0), (12.0, 3.0), (12.0, 5.0)]);
            p.closed(&[(5.0, 5.0), (15.0, 5.0), (14.0, 17.0), (6.0, 17.0)]);
        }
        "layers" | "layers-fill" => {
            p.closed(&[(10.0, 3.0), (17.0, 7.0), (10.0, 11.0), (3.0, 7.0)]);
            p.line(&[(3.0, 11.0), (10.0, 15.0), (17.0, 11.0)]);
            if name == "layers-fill" {
                p.fill(&[(10.0, 3.0), (17.0, 7.0), (10.0, 11.0), (3.0, 7.0)]);
            }
        }
        "snow" => {
            p.line(&[(10.0, 2.5), (10.0, 17.5)]);
            p.line(&[(3.5, 6.0), (16.5, 14.0)]);
            p.line(&[(3.5, 14.0), (16.5, 6.0)]);
        }
        "sun" => {
            p.circle(10.0, 10.0, 3.5);
            for a in (0..8).map(|i| i as f32 * 45.0_f32.to_radians()) {
                p.line(&[(10.0 + 5.5 * a.cos(), 10.0 + 5.5 * a.sin()), (10.0 + 7.5 * a.cos(), 10.0 + 7.5 * a.sin())]);
            }
        }
        "check2-circle" | "check2-all" => {
            if name == "check2-circle" {
                p.circle(10.0, 10.0, 7.0);
            }
            p.line(&[(6.0, 10.0), (9.0, 13.0), (14.5, 7.0)]);
        }
        "intersect" | "union" | "subtract" => {
            p.rect(3.0, 3.0, 13.0, 13.0);
            p.rect(7.0, 7.0, 17.0, 17.0);
            match name {
                "intersect" => p.fill(&[(7.0, 7.0), (13.0, 7.0), (13.0, 13.0), (7.0, 13.0)]),
                "subtract" => p.fill(&[(3.0, 3.0), (13.0, 3.0), (13.0, 7.0), (7.0, 7.0), (7.0, 13.0), (3.0, 13.0)]),
                _ => {}
            }
        }
        "layout-text-window" | "window" | "layout-split" | "layout-sidebar" | "layout-sidebar-inset" | "layout-sidebar-inset-reverse" => {
            p.rect(3.0, 4.0, 17.0, 16.0);
            match name {
                "window" | "layout-text-window" => p.line(&[(3.0, 7.0), (17.0, 7.0)]),
                "layout-split" => p.line(&[(10.0, 4.0), (10.0, 16.0)]),
                "layout-sidebar" | "layout-sidebar-inset" => p.line(&[(7.0, 4.0), (7.0, 16.0)]),
                _ => p.line(&[(13.0, 4.0), (13.0, 16.0)]),
            }
        }
        "clipboard" | "clipboard-plus" | "clipboard-check" => {
            p.rect(4.0, 4.0, 16.0, 18.0);
            p.rect(7.0, 2.5, 13.0, 5.5);
            if name == "clipboard-plus" {
                p.line(&[(10.0, 9.0), (10.0, 15.0)]);
                p.line(&[(7.0, 12.0), (13.0, 12.0)]);
            } else if name == "clipboard-check" {
                p.line(&[(7.0, 12.0), (9.0, 14.0), (13.0, 10.0)]);
            }
        }
        "funnel" => p.closed(&[(3.0, 4.0), (17.0, 4.0), (11.5, 10.5), (11.5, 16.0), (8.5, 17.0), (8.5, 10.5)]),
        "ui-checks" => {
            p.rect(3.0, 4.0, 7.0, 8.0);
            p.rect(3.0, 12.0, 7.0, 16.0);
            p.line(&[(10.0, 6.0), (17.0, 6.0)]);
            p.line(&[(10.0, 14.0), (17.0, 14.0)]);
        }
        "search" | "zoom-in" => {
            p.circle(8.5, 8.5, 5.0);
            p.line(&[(12.0, 12.0), (17.0, 17.0)]);
            if name == "zoom-in" {
                p.line(&[(8.5, 6.0), (8.5, 11.0)]);
                p.line(&[(6.0, 8.5), (11.0, 8.5)]);
            }
        }
        "skip-backward" => {
            p.closed(&[(10.0, 5.0), (10.0, 15.0), (4.0, 10.0)]);
            p.closed(&[(16.0, 5.0), (16.0, 15.0), (10.0, 10.0)]);
        }
        "compass" => {
            p.circle(10.0, 10.0, 7.0);
            p.closed(&[(13.0, 7.0), (11.0, 11.0), (7.0, 13.0), (9.0, 9.0)]);
        }
        "globe" => {
            p.circle(10.0, 10.0, 7.0);
            p.line(&[(3.0, 10.0), (17.0, 10.0)]);
            p.arc(10.0, 10.0, 7.0, 90.0, 90.0);
            p.closed(&[(10.0, 3.0), (12.5, 10.0), (10.0, 17.0), (7.5, 10.0)]);
        }
        "map" => {
            p.line(&[(3.0, 5.0), (8.0, 3.0), (12.0, 5.0), (17.0, 3.0), (17.0, 15.0), (12.0, 17.0), (8.0, 15.0), (3.0, 17.0), (3.0, 5.0)])
        }
        "rulers" => {
            p.closed(&[(3.0, 3.0), (6.0, 3.0), (6.0, 14.0), (17.0, 14.0), (17.0, 17.0), (3.0, 17.0)]);
            for y in [6.0, 9.0, 12.0] {
                p.line(&[(3.0, y), (4.5, y)]);
            }
        }
        "magnet" => {
            p.arc(10.0, 9.0, 6.0, 180.0, 360.0);
            p.line(&[(4.0, 9.0), (4.0, 3.0)]);
            p.line(&[(16.0, 9.0), (16.0, 3.0)]);
            p.line(&[(4.0, 6.0), (7.0, 6.0)]);
            p.line(&[(13.0, 6.0), (16.0, 6.0)]);
        }
        "border-width" => {
            p.line(&[(3.0, 5.0), (17.0, 5.0)]);
            p.dot(10.0, 10.0, 0.0);
            p.p.rect_filled(Rect::from_min_max(p.at(3.0, 9.0), p.at(17.0, 11.5)), 0.0, color);
            p.p.rect_filled(Rect::from_min_max(p.at(3.0, 14.0), p.at(17.0, 17.5)), 0.0, color);
        }
        "camera" | "camera-video" => {
            p.rect(3.0, 6.0, 14.0, 15.0);
            p.closed(&[(14.0, 9.0), (17.0, 7.0), (17.0, 14.0), (14.0, 12.0)]);
        }
        "transparency" => {
            p.circle(10.0, 10.0, 7.0);
            p.line(&[(3.0, 10.0), (17.0, 10.0)]);
            p.line(&[(5.0, 6.0), (15.0, 6.0)]);
            p.line(&[(5.0, 14.0), (15.0, 14.0)]);
        }
        "printer" => {
            p.rect(3.0, 8.0, 17.0, 14.0);
            p.rect(6.0, 3.0, 14.0, 8.0);
            p.rect(6.0, 12.0, 14.0, 17.0);
        }
        "download" | "file-earmark-arrow-down" => {
            p.arrow(10.0, 3.0, 10.0, 13.0);
            p.line(&[(3.0, 12.0), (3.0, 17.0), (17.0, 17.0), (17.0, 12.0)]);
        }
        "folder2-open" => {
            p.line(&[(3.0, 16.0), (3.0, 4.0), (8.0, 4.0), (10.0, 6.0), (16.0, 6.0), (16.0, 9.0)]);
            p.closed(&[(3.0, 16.0), (6.0, 9.0), (18.0, 9.0), (15.0, 16.0)]);
        }
        "gear" => {
            p.circle(10.0, 10.0, 5.0);
            p.circle(10.0, 10.0, 2.0);
        }
        "question-circle" => {
            p.circle(10.0, 10.0, 7.0);
            p.arc(10.0, 8.0, 2.5, -90.0, 180.0);
            p.dot(10.0, 14.0, 0.8);
        }
        "keyboard" | "card-text" | "filetype-js" | "file-earmark-code" | "file-earmark-plus" => {
            if name == "keyboard" {
                p.rect(2.0, 6.0, 18.0, 15.0);
                p.line(&[(6.0, 12.0), (14.0, 12.0)]);
                for x in [5.0, 8.0, 11.0, 14.0] {
                    p.dot(x, 9.0, 0.8);
                }
            } else {
                p.closed(&[(4.0, 2.5), (12.0, 2.5), (16.0, 6.5), (16.0, 17.5), (4.0, 17.5)]);
                match name {
                    "file-earmark-plus" => {
                        p.line(&[(10.0, 8.0), (10.0, 14.0)]);
                        p.line(&[(7.0, 11.0), (13.0, 11.0)]);
                    }
                    "file-earmark-code" | "filetype-js" => {
                        p.line(&[(8.5, 9.0), (6.5, 11.5), (8.5, 14.0)]);
                        p.line(&[(11.5, 9.0), (13.5, 11.5), (11.5, 14.0)]);
                    }
                    _ => {
                        for y in [8.0, 11.0, 14.0] {
                            p.line(&[(7.0, y), (13.0, y)]);
                        }
                    }
                }
            }
        }
        _ => p.rect(5.0, 5.0, 15.0, 15.0),
    }
}

// ---------------------------------------------------------------------------------------------- the bar

/// One button: icon then label (toolbar) or icon over label (ribbon, `big`), or the icon alone (`icon_only`).
fn tool_button(
    ui: &mut egui::Ui,
    tk: &crate::theme::Tokens,
    icon: &str,
    label: &str,
    big: bool,
    icon_only: bool,
    pressed: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    // a ribbon button's label wraps onto two lines (a long "Circle: tangent, tangent, radius")
    let galley = if big {
        ui.painter().layout(label.to_string(), font.clone(), tk.text, 84.0)
    } else {
        ui.painter().layout_no_wrap(label.to_string(), font.clone(), tk.text)
    };
    let size = if icon_only {
        Vec2::new(22.0, 22.0)
    } else if big {
        Vec2::new((galley.size().x + 8.0).max(40.0), (30.0 + galley.size().y).max(54.0))
    } else {
        Vec2::new(galley.size().x + 26.0, 22.0)
    };
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if pressed {
        tk.row_selected
    } else if resp.hovered() {
        tk.panel_darker
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, f32::from(tk.radius), bg);
    let ink = if resp.hovered() || pressed { tk.text_strong } else { tk.text };
    if icon_only {
        paint_icon(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.0)), icon, ink);
    } else if big {
        paint_icon(ui.painter(), Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 14.0), Vec2::splat(20.0)), icon, ink);
        ui.painter().galley(Pos2::new(rect.center().x - galley.size().x / 2.0, rect.top() + 27.0), galley, ink);
    } else {
        paint_icon(ui.painter(), Rect::from_min_size(Pos2::new(rect.left() + 4.0, rect.center().y - 8.0), Vec2::splat(16.0)), icon, ink);
        ui.painter().galley(Pos2::new(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, ink);
    }
    resp
}

/// A menu entry (label, key on the right).
fn menu_entry(ui: &mut egui::Ui, tk: &crate::theme::Tokens, icon: Option<&str>, label: &str, key: &str) -> egui::Response {
    let w = 240.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, f32::from(tk.radius), tk.row_selected);
    }
    let ink = if resp.hovered() { tk.text_strong } else { tk.text };
    let mut x = rect.left() + 6.0;
    if let Some(i) = icon {
        paint_icon(ui.painter(), Rect::from_min_size(Pos2::new(x, rect.center().y - 7.0), Vec2::splat(14.0)), i, ink);
        x += 20.0;
    }
    let font = egui::FontId::proportional(12.0);
    ui.painter().text(Pos2::new(x, rect.center().y), egui::Align2::LEFT_CENTER, label, font, ink);
    if !key.is_empty() {
        ui.painter().text(
            Pos2::new(rect.right() - 6.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            key,
            egui::FontId::monospace(10.5),
            tk.text_dim,
        );
    }
    resp
}

/// What the person clicked in the bar this frame.
enum Click {
    Cmd(&'static str, String),
    Toggle(String),
    Tab(usize),
    Style(&'static str),
    SaveAs(&'static str),
}

/// Show a popup menu under `anchor` when `key` is open.
fn popup(ui: &egui::Ui, bar: &Bar, key: &str, anchor: Rect, tk: &crate::theme::Tokens, add: impl FnOnce(&mut egui::Ui)) -> Option<Rect> {
    if bar.open.as_deref() != Some(key) {
        return None;
    }
    let r = egui::Area::new(ui.make_persistent_id(("cc-in-menu", key)))
        .fixed_pos(anchor.left_bottom() + Vec2::new(0.0, 2.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::new().fill(tk.panel).stroke(Stroke::new(1.0, tk.divider)).corner_radius(tk.radius).inner_margin(4).show(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    add(ui);
                },
            );
        });
    Some(r.response.rect)
}

fn sep(ui: &mut egui::Ui, tk: &crate::theme::Tokens) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(240.0, 7.0), egui::Sense::hover());
    ui.painter()
        .line_segment([Pos2::new(r.left() + 4.0, r.center().y), Pos2::new(r.right() - 4.0, r.center().y)], Stroke::new(1.0, tk.divider));
}

/// The bar over the drafting editor: the chosen style's toolbar or ribbon.
pub fn bar(app: &mut UiApp, ui: &mut egui::Ui) {
    let tk = app.tokens;
    let st = style(&app.ribbon.style).unwrap_or(&CADCRAFT_STYLE);
    let running = app.editor.as_ref().and_then(|e| e.eng.running());
    let pressed = |c: &str| {
        let first = c.split(' ').next().unwrap_or("");
        running.is_some_and(|r| cc_engine::cmd::lookup(first).is_some_and(|d| d.name == r))
    };
    let frame_no = ui.ctx().cumulative_frame_nr();
    let mut click: Option<Click> = None;
    let mut menu_rects: Vec<Rect> = Vec::new();
    let bar_state = app.ribbon.clone();
    let toggle = |key: String, click: &mut Option<Click>| {
        *click = Some(Click::Toggle(key));
    };
    let height = if st.layout == "toolbar" { 46.0 } else { 116.0 };
    egui::Panel::top("cc_inbar")
        .exact_size(height)
        .frame(egui::Frame::new().fill(tk.app_bar).inner_margin(egui::Margin::symmetric(6, 2)))
        .show(ui, |ui| {
            if st.layout == "toolbar" {
                egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for g in TOOLBAR {
                            let gl = group_label(g.key);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(RichText::new(&gl).size(10.0).color(tk.text_dim));
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 1.0;
                                    for (cmd, icon, lk, tk2) in g.tools {
                                        let r = tool_button(ui, &tk, icon, &lang_text(lk, &[]), false, false, pressed(cmd))
                                            .on_hover_text(lang_text(tk2, &[]));
                                        if r.clicked() {
                                            click = Some(Click::Cmd(cmd, lang_text(lk, &[])));
                                        }
                                    }
                                    if !g.more.is_empty() {
                                        let key = format!("more:{}", g.key);
                                        let r = tool_button(
                                            ui,
                                            &tk,
                                            "three-dots",
                                            &lang_text("toolbar.more", &[]),
                                            false,
                                            false,
                                            bar_state.open.as_deref() == Some(&key),
                                        )
                                        .on_hover_text(lang_text("toolbar.more.title", &[("group", gl.clone())]));
                                        if r.clicked() {
                                            toggle(key.clone(), &mut click);
                                        }
                                        if let Some(mr) = popup(ui, &bar_state, &key, r.rect, &tk, |ui| {
                                            for (cmd, lk, kk) in g.more {
                                                if menu_entry(ui, &tk, None, &lang_text(lk, &[]), kk).clicked() {
                                                    click = Some(Click::Cmd(cmd, lang_text(lk, &[])));
                                                }
                                            }
                                        }) {
                                            menu_rects.push(mr.union(r.rect));
                                        }
                                    }
                                    if let Some((ml, mt, _list, items)) = g.menu {
                                        let key = format!("menu:{}", g.key);
                                        let r = tool_button(
                                            ui,
                                            &tk,
                                            "three-dots",
                                            &lang_text(ml, &[]),
                                            false,
                                            false,
                                            bar_state.open.as_deref() == Some(&key),
                                        )
                                        .on_hover_text(lang_text(mt, &[]));
                                        if r.clicked() {
                                            toggle(key.clone(), &mut click);
                                        }
                                        if let Some(mr) = popup(ui, &bar_state, &key, r.rect, &tk, |ui| {
                                            for (cmd, lk, kk) in items {
                                                if *cmd == "-" {
                                                    sep(ui, &tk);
                                                } else if menu_entry(ui, &tk, None, &lang_text(lk, &[]), kk).clicked() {
                                                    click = Some(Click::Cmd(cmd, lang_text(lk, &[])));
                                                }
                                            }
                                        }) {
                                            menu_rects.push(mr.union(r.rect));
                                        }
                                    }
                                });
                            });
                            let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, 38.0), egui::Sense::hover());
                            ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, tk.divider));
                        }
                        style_picker(ui, &tk, st, &mut click);
                    });
                });
            } else {
                // head: File menu · tabs · quick buttons (on the right)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if let Some((ml, items)) = st.menus.first() {
                        let open = bar_state.open.as_deref() == Some("file");
                        let txt = RichText::new(lang_text(&format!("input.m.{ml}"), &[])).strong().color(if open {
                            tk.text_strong
                        } else {
                            tk.text
                        });
                        let r = ui.add(
                            egui::Button::new(txt).fill(if open { tk.accent } else { tk.panel_darker }).min_size(Vec2::new(48.0, 22.0)),
                        );
                        if r.clicked() {
                            toggle("file".into(), &mut click);
                        }
                        if let Some(mr) = popup(ui, &bar_state, "file", r.rect, &tk, |ui| {
                            let mut last_sep = true;
                            for x in *items {
                                if *x == "-" {
                                    if !last_sep {
                                        sep(ui, &tk);
                                        last_sep = true;
                                    }
                                    continue;
                                }
                                let Some(c) = cmd(x) else { continue };
                                let keys = c.keys.first().copied().unwrap_or("");
                                if menu_entry(ui, &tk, Some(c.icon), &label(st, x), keys).clicked() {
                                    click = Some(Click::Cmd(c.cmd, label(st, x)));
                                }
                                last_sep = false;
                            }
                        }) {
                            menu_rects.push(mr.union(r.rect));
                        }
                    }
                    ui.add_space(6.0);
                    for (i, (tl, _)) in st.tabs.iter().enumerate() {
                        let on = i == bar_state.tab;
                        let txt = RichText::new(lang_text(&format!("input.m.{tl}"), &[])).size(12.0).color(if on {
                            tk.text_strong
                        } else {
                            tk.text_dim
                        });
                        ui.add_space(4.0);
                        let r = ui.add(egui::Button::new(txt).frame(false).min_size(Vec2::new(0.0, 22.0)));
                        ui.add_space(4.0);
                        if on {
                            ui.painter().line_segment([r.rect.left_bottom(), r.rect.right_bottom()], Stroke::new(2.0, tk.accent));
                        }
                        if r.clicked() {
                            click = Some(Click::Tab(i));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        style_picker(ui, &tk, st, &mut click);
                        for x in st.quick.iter().rev() {
                            let Some(c) = cmd(x) else { continue };
                            let tip = match c.keys.first() {
                                Some(k) => format!("{} ({k})", label(st, x)),
                                None => label(st, x),
                            };
                            let r = tool_button(ui, &tk, c.icon, "", false, true, false).on_hover_text(tip);
                            if r.clicked() {
                                click = Some(Click::Cmd(c.cmd, label(st, x)));
                            }
                        }
                    });
                });
                // body: the open tab's panels
                let tab = st.tabs.get(bar_state.tab).or_else(|| st.tabs.first());
                egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (pl, items) in tab.map_or(&[][..], |t| t.1) {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 1.0;
                                    for x in *items {
                                        // app-input-ui.js: an optional command whose button is not there (the 3D group,
                                        // Open in ModelCraft) is left out
                                        let Some(c) = cmd(x).filter(|c| !c.optional) else { continue };
                                        let tip = match c.keys.first() {
                                            Some(k) => format!("{} ({k})", label(st, x)),
                                            None => label(st, x),
                                        };
                                        let r = tool_button(ui, &tk, c.icon, &label(st, x), true, false, pressed(c.cmd)).on_hover_text(tip);
                                        if r.clicked() {
                                            click = Some(Click::Cmd(c.cmd, label(st, x)));
                                        }
                                    }
                                });
                                let w = ui.min_rect().width();
                                ui.allocate_ui_with_layout(Vec2::new(w, 14.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(lang_text(&format!("input.m.{pl}"), &[])).size(10.0).color(tk.text_dim));
                                });
                            });
                            let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, 68.0), egui::Sense::hover());
                            ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, tk.divider));
                        }
                    });
                });
            }
        });
    // Save as: the formats (the JS asks in its own dialog)
    if bar_state.open.as_deref() == Some("saveas") {
        let anchor = Rect::from_min_size(ui.max_rect().left_top() + Vec2::new(8.0, 40.0), Vec2::ZERO);
        if let Some(mr) = popup(ui, &bar_state, "saveas", anchor, &tk, |ui| {
            for f in cc_io_cad::SAVE_FORMATS {
                let l = match *f {
                    "dxf2000" => "DXF 2000".to_string(),
                    "dxf12" => "DXF R12".to_string(),
                    v => format!("DWG {v}"),
                };
                if menu_entry(ui, &tk, None, &l, "").clicked() {
                    click = Some(Click::SaveAs(f));
                }
            }
        }) {
            menu_rects.push(mr);
        }
    }
    // a click outside an open menu closes it (not the click that opened it)
    if app.ribbon.open.is_some() && click.is_none() && app.ribbon.opened_at != frame_no && app.ribbon.opened_at != u64::MAX {
        let outside = ui
            .ctx()
            .input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !menu_rects.iter().any(|r| r.contains(p))));
        if outside || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            app.ribbon.open = None;
        }
    }
    if app.ribbon.opened_at == u64::MAX {
        app.ribbon.opened_at = frame_no;
    }
    let r = match click {
        None => Ok(()),
        Some(Click::Cmd(c, l)) => press(app, c, &l),
        Some(Click::Toggle(key)) => {
            if app.ribbon.open.as_deref() == Some(key.as_str()) {
                app.ribbon.open = None;
            } else {
                app.ribbon.open = Some(key);
                app.ribbon.opened_at = frame_no;
            }
            Ok(())
        }
        Some(Click::Tab(i)) => run(app, "input.tab", &Params { index: Some(i), ..Default::default() }),
        Some(Click::Style(s)) => run(app, "input.style", &Params { name: Some(s.into()), ..Default::default() }),
        Some(Click::SaveAs(f)) => run(app, "input.saveas", &Params { name: Some(f.into()), ..Default::default() }),
    };
    if let Err(e) = r {
        app.error = Some(e);
    }
}

/// The style chooser at the end of the bar (0.5 has it in Settings › Menus, keys and mouse).
fn style_picker(ui: &mut egui::Ui, tk: &crate::theme::Tokens, st: &Style, click: &mut Option<Click>) {
    let name = lang_text(&format!("input.style.{}", st.id), &[]);
    egui::ComboBox::new("cc-in-style", "")
        .selected_text(RichText::new(name).size(11.0).color(tk.text))
        .width(120.0)
        .show_ui(ui, |ui| {
            for id in STYLE_ORDER {
                let r = ui
                    .selectable_label(*id == st.id, lang_text(&format!("input.style.{id}"), &[]))
                    .on_hover_text(lang_text(&format!("input.style.{id}.hint"), &[]));
                if r.clicked() {
                    *click = Some(Click::Style(id));
                }
            }
        })
        .response
        .on_hover_text("Menus, keys and mouse");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_and_key_resolves() {
        let en = crate::edit::lang_en_pub();
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len(), "command ids are unique");
        for c in COMMANDS {
            assert!(en.has(&format!("input.cmd.{}", c.id)), "input.cmd.{}", c.id);
            assert!(GROUPS.contains(&c.group), "{}", c.group);
        }
        for st in STYLES {
            assert!(en.has(&format!("input.style.{}", st.id)));
            for (l, items) in st.menus {
                assert!(en.has(&format!("input.m.{l}")));
                for x in *items {
                    assert!(*x == "-" || cmd(x).is_some(), "{x}");
                }
            }
            for x in st.quick {
                assert!(cmd(x).is_some(), "{x}");
            }
            for (tl, panels) in st.tabs {
                assert!(en.has(&format!("input.m.{tl}")), "{tl}");
                for (pl, items) in *panels {
                    assert!(en.has(&format!("input.m.{pl}")), "{pl}");
                    for x in *items {
                        assert!(cmd(x).is_some(), "{x}");
                    }
                }
            }
            for (id, key) in st.labels {
                assert!(cmd(id).is_some() && en.has(key), "{id} {key}");
            }
        }
        for g in TOOLBAR {
            assert!(en.has(&format!("toolbar.group.{}", g.key.to_ascii_lowercase())));
            for (_, _, l, t) in g.tools {
                assert!(en.has(l) && en.has(t), "{l} {t}");
            }
            for (c, l, _) in g.more.iter().chain(g.menu.iter().flat_map(|m| m.3.iter())) {
                assert!(*c == "-" || en.has(l), "{l}");
            }
            if let Some((a, b, d, _)) = g.menu {
                assert!(en.has(a) && en.has(b) && en.has(d));
            }
        }
        assert_eq!(STYLE_ORDER.len(), STYLES.len());
    }

    #[test]
    fn buttons_run_through_the_command_line() {
        let mut app = UiApp::default();
        assert!(run(&mut app, "input.run", &Params { name: Some("draw.line".into()), ..Default::default() }).is_err());
        app.run("file.new", &Params::default()).unwrap_or_default();
        run(&mut app, "input.run", &Params { name: Some("draw.line".into()), ..Default::default() }).unwrap_or_default();
        assert_eq!(app.editor.as_ref().and_then(|e| e.eng.running()), Some("LINE"));
        // ZOOM E: ZOOM runs, E answers its prompt
        run(&mut app, "input.run", &Params { name: Some("view.extents".into()), ..Default::default() }).unwrap_or_default();
        assert_eq!(app.editor.as_ref().and_then(|e| e.eng.running()), None);
        run(&mut app, "input.run", &Params { name: Some("aids.grid".into()), ..Default::default() }).unwrap_or_default();
        assert_eq!(app.message, "Grid: it is not in this version.");
        run(&mut app, "input.style", &Params { name: Some("drafting".into()), ..Default::default() }).unwrap_or_default();
        assert!(run(&mut app, "input.tab", &Params { index: Some(7), ..Default::default() }).is_ok());
        assert!(run(&mut app, "input.tab", &Params { index: Some(8), ..Default::default() }).is_err());
        assert!(run(&mut app, "input.style", &Params { name: Some("nope".into()), ..Default::default() }).is_err());
        assert_eq!(app.ribbon.prefs(), json!({"style": "drafting", "tab": 7}));
        let mut b = Bar::default();
        b.load_prefs(&json!({"style": "ribbon", "tab": 3}));
        assert_eq!((b.style.as_str(), b.tab), ("ribbon", 3));
        b.load_prefs(&json!({"style": "bogus"}));
        assert_eq!(b.style, "ribbon");
    }
}
