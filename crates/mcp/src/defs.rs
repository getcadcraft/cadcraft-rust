//! The headless tools' definitions (0.5 tools/cli/headless/tools.mjs): names, groups, titles, descriptions and argument
//! schemas exactly as 0.5 writes them, key order included (the parity harness compares `tools/list` byte for byte).

use serde_json::{Map, Value, json};

use crate::rpc::ToolDef;

/// `obj(properties, required)`: an object schema with `additionalProperties: false`.
pub fn obj(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut p = Map::new();
    for (k, v) in properties {
        p.insert(k.to_string(), v);
    }
    json!({"type": "object", "properties": Value::Object(p), "required": required, "additionalProperties": false})
}

/// `{...a, ...b}` for two JSON objects.
fn with(a: &Value, b: Value) -> Value {
    let mut m = a.as_object().cloned().unwrap_or_default();
    if let Value::Object(o) = b {
        for (k, v) in o {
            m.insert(k, v);
        }
    }
    Value::Object(m)
}

fn s_doc() -> Value {
    json!({"type": "string", "pattern": "^d[0-9]{1,6}$", "description": "A handle from drawing_open, ifc_open or takeoff_open."})
}
fn s_path() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 4096, "description": "A file inside the folders the assistant may use (relative = inside the first one)."})
}
fn s_offset() -> Value {
    json!({"type": "integer", "minimum": 0, "default": 0})
}
fn s_overwrite() -> Value {
    json!({"type": "boolean", "default": false, "description": "Replace a file that already exists (and was not opened or written in this session)."})
}
fn bbox() -> Value {
    json!({"type": "object", "properties": {"minX": {"type": "number"}, "minY": {"type": "number"}, "maxX": {"type": "number"}, "maxY": {"type": "number"}}, "required": ["minX", "minY", "maxX", "maxY"], "additionalProperties": false})
}
fn pt2() -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2})
}
fn input_item() -> Value {
    json!({"anyOf": [
        {"type": "string", "maxLength": 10000},
        obj(vec![("pick", pt2()), ("shift", json!({"type": "boolean"}))], &["pick"]),
        obj(vec![("window", json!({"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4})), ("crossing", json!({"type": "boolean"}))], &["window"]),
        obj(vec![("entity", json!({"type": "integer", "minimum": 1})), ("at", pt2()), ("shift", json!({"type": "boolean"}))], &["entity"]),
        obj(vec![("escape", json!({"type": "boolean", "const": true}))], &["escape"]),
    ]})
}

/// Object types CadCraft lists (agents.js OBJECT_TYPES).
pub const OBJECT_TYPES: [&str; 16] = [
    "line", "circle", "arc", "poly", "ellipse", "text", "point", "insert", "hatch", "dim", "viewport", "mleader", "spline", "face3d",
    "image", "wipeout",
];
/// Papers drawing_plot takes.
pub const PAPERS: [&str; 13] =
    ["iso-a4", "iso-a3", "iso-a2", "iso-a1", "iso-a0", "letter", "legal", "ansi-b", "ansi-c", "ansi-d", "arch-c", "arch-d", "arch-e"];
const VERSIONS: [&str; 6] = ["2000", "2004", "2007", "2010", "2013", "2018"];

/// The rules schema of drawing_check (agents.js RULES_SCHEMA).
pub fn rules_schema() -> Value {
    let arr = |items: Value, max: u32| json!({"type": "array", "items": items, "maxItems": max});
    let mut props = Map::new();
    props.insert(
        "layers".into(),
        obj(
            vec![
                ("allowed", arr(json!({"type": "string"}), 500)),
                ("required", arr(json!({"type": "string"}), 500)),
                ("noObjectsOn", arr(json!({"type": "string"}), 50)),
            ],
            &[],
        ),
    );
    props.insert(
        "objects".into(),
        obj(
            vec![
                ("colorByLayer", json!({"type": "boolean"})),
                ("linetypeByLayer", json!({"type": "boolean"})),
                ("lineweightByLayer", json!({"type": "boolean"})),
                ("textHeights", arr(json!({"type": "number"}), 50)),
                ("textStyles", arr(json!({"type": "string"}), 50)),
            ],
            &[],
        ),
    );
    props.insert("blocks".into(), obj(vec![("noUnused", json!({"type": "boolean"}))], &[]));
    props.insert(
        "units".into(),
        obj(
            vec![
                ("lunits", json!({"type": "integer", "minimum": 1, "maximum": 5})),
                ("insunits", json!({"type": "string", "maxLength": 40})),
            ],
            &[],
        ),
    );
    json!({"type": "object", "properties": Value::Object(props), "additionalProperties": false})
}

/// Every headless tool (AGENTS-SPEC § 5.1), in 0.5's order.
pub fn headless_defs() -> Vec<ToolDef> {
    let d = |name: &str, group: &'static str, title: &str, desc: &str, schema: Value| ToolDef::new(name, group, title, desc, Some(schema));
    let doc_only = || obj(vec![("doc", s_doc())], &["doc"]);
    let fmt = || json!({"type": "string", "enum": ["dwg", "dxf2000", "dxf"]});
    let ver = || json!({"type": "string", "enum": VERSIONS});
    vec![
        d(
            "files_list",
            "read",
            "List files",
            "Files in a folder the assistant may use, newest first (up to 2000). Hidden files and links are skipped.",
            obj(
                vec![
                    (
                        "folder",
                        with(&s_path(), json!({"description": "A folder inside the allowed ones; default the first allowed folder."})),
                    ),
                    (
                        "extensions",
                        json!({"type": "array", "items": {"type": "string", "maxLength": 12}, "maxItems": 20, "description": "e.g. [\"dwg\", \"dxf\"]"}),
                    ),
                    ("recursive", json!({"type": "boolean", "default": false})),
                ],
                &[],
            ),
        ),
        d(
            "drawing_open",
            "read",
            "Open a drawing",
            "Open a DWG or DXF drawing with CadCraft. Returns a handle (d1, …) for the other drawing tools and a short summary.",
            obj(vec![("path", s_path())], &["path"]),
        ),
        d(
            "drawing_new",
            "edit",
            "Start a new drawing",
            "A new, empty drawing in memory (save it with drawing_save and a path).",
            obj(vec![("name", json!({"type": "string", "maxLength": 120, "default": "Drawing1"}))], &[]),
        ),
        d(
            "drawing_describe",
            "read",
            "Describe a drawing",
            "Units and length format, extents, the layer table (state and object counts), blocks and their uses, objects by type, sheets with their viewports, views, notes from reading the file.",
            doc_only(),
        ),
        d(
            "objects_list",
            "read",
            "List objects",
            "Objects in an open drawing, filtered and paged: each with its id, type, layer, colour, geometry, length/area where it has one, and a one-line description. Coordinates are drawing units.",
            obj(
                vec![
                    ("doc", s_doc()),
                    ("type", json!({"type": "string", "enum": OBJECT_TYPES})),
                    ("layer", json!({"type": "string", "maxLength": 255})),
                    ("block", json!({"type": "string", "maxLength": 255, "description": "References of this block only."})),
                    ("text", json!({"type": "string", "maxLength": 200, "description": "Texts (and attribute values) containing this."})),
                    ("within", with(&bbox(), json!({"description": "Only objects entirely inside this box."}))),
                    ("space", json!({"type": "string", "maxLength": 255, "description": "\"model\" (default) or a sheet name."})),
                    ("offset", s_offset()),
                    ("limit", json!({"type": "integer", "minimum": 1, "maximum": 1000, "default": 100})),
                ],
                &["doc"],
            ),
        ),
        d(
            "object_get",
            "read",
            "Get one object",
            "One object in full (every field, block attributes and the block definition of a reference).",
            obj(vec![("doc", s_doc()), ("id", json!({"type": "integer", "minimum": 1}))], &["doc", "id"]),
        ),
        d(
            "drawing_check",
            "read",
            "Check against standards",
            "Check a drawing against layer and drafting rules: layers.allowed (names or /regex/), layers.required, layers.noObjectsOn, objects.colorByLayer / linetypeByLayer / lineweightByLayer, objects.textHeights, objects.textStyles, blocks.noUnused, units.lunits / units.insunits. Returns each finding with the ids it concerns.",
            obj(vec![("doc", s_doc()), ("rules", rules_schema())], &["doc", "rules"]),
        ),
        d(
            "command_run",
            "edit",
            "Run a CadCraft command",
            "Type at CadCraft's command line: one string, or a list where each item answers the next prompt (\"\" = Enter). Items may also be {pick: [x, y]}, {window: [x1, y1, x2, y2], crossing}, {entity: id} or {escape: true}. Example: [\"LINE\", \"0,0\", \"@100,0\", \"@0,50\", \"\"]. A command still waiting at the end is ended with Esc. The whole call is one undo step labelled \"Assistant: …\". Commands that need a dialog on screen are refused.",
            obj(
                vec![
                    ("doc", s_doc()),
                    (
                        "input",
                        json!({"anyOf": [{"type": "string", "maxLength": 100000}, {"type": "array", "items": input_item(), "minItems": 1, "maxItems": 5000}]}),
                    ),
                    (
                        "select",
                        json!({"type": "array", "items": {"type": "integer", "minimum": 1}, "maxItems": 100000, "description": "Object ids selected before the command (noun-verb)."}),
                    ),
                ],
                &["doc", "input"],
            ),
        ),
        d(
            "script_run",
            "edit",
            "Run a script",
            "Run a script on an open drawing as one undo step: language \"scr\" (command-line lines, as a .scr file) or \"lisp\" (AutoLISP-style, run by CadCraft's own interpreter; (command …) drives CadCraft's commands).",
            obj(
                vec![
                    ("doc", s_doc()),
                    ("script", json!({"type": "string", "minLength": 1, "maxLength": 1000000})),
                    ("language", json!({"type": "string", "enum": ["scr", "lisp"], "default": "scr"})),
                ],
                &["doc", "script"],
            ),
        ),
        d(
            "undo",
            "edit",
            "Undo",
            "Undo the assistant's last steps on a drawing (only steps labelled \"Assistant:\" unless any is true).",
            obj(
                vec![
                    ("doc", s_doc()),
                    ("steps", json!({"type": "integer", "minimum": 1, "maximum": 100, "default": 1})),
                    ("any", json!({"type": "boolean", "default": false})),
                ],
                &["doc"],
            ),
        ),
        d(
            "drawing_save",
            "save",
            "Save a drawing",
            "Write the drawing: to its own file (default) or to a new .dwg/.dxf path. format dwg | dxf2000 | dxf (R12); DWG version 2000, 2004, 2007, 2010, 2013 or 2018 (default 2018).",
            obj(vec![("doc", s_doc()), ("path", s_path()), ("format", fmt()), ("version", ver()), ("overwrite", s_overwrite())], &["doc"]),
        ),
        d(
            "drawing_convert",
            "export",
            "Convert a drawing",
            "Read a DWG/DXF and write it as another .dwg (version) or .dxf (DXF 2000, or R12 with format dxf). The original is not changed.",
            obj(
                vec![("path", s_path()), ("out", s_path()), ("format", fmt()), ("version", ver()), ("overwrite", s_overwrite())],
                &["path", "out"],
            ),
        ),
        d(
            "drawing_plot",
            "export",
            "Plot to PDF",
            "A vector PDF of a sheet (layout, as set up) or of the model (extents or a window, on a paper at a scale or fitted). Papers: iso-a4 … iso-a0, letter, legal, ansi-b … ansi-d, arch-c … arch-e. Scales \"1:50\", \"1/4\\\" = 1'-0\\\"\" or \"fit\".",
            obj(
                vec![
                    ("doc", s_doc()),
                    ("out", s_path()),
                    ("layout", json!({"type": "string", "maxLength": 255})),
                    ("window", bbox()),
                    ("paper", json!({"type": "string", "enum": PAPERS, "default": "iso-a3"})),
                    ("portrait", json!({"type": "boolean", "default": false})),
                    ("scale", json!({"type": "string", "maxLength": 40, "default": "fit"})),
                    ("mono", json!({"type": "boolean", "default": true})),
                    ("pen", json!({"type": "number", "minimum": 0.05, "maximum": 2, "default": 0.25})),
                    ("overwrite", s_overwrite()),
                ],
                &["doc", "out"],
            ),
        ),
        d("doc_close", "read", "Close a file", "Forget an open drawing, model or takeoff (unsaved changes are dropped).", doc_only()),
        d(
            "ifc_open",
            "read",
            "Open an IFC model",
            "Open an IFC model (IFC2X3, IFC4, IFC4X3) for questions: schema, project, storeys, element classes. Geometry is not built.",
            obj(vec![("path", s_path())], &["path"]),
        ),
        d(
            "ifc_query",
            "read",
            "Find IFC elements",
            "Elements of an open IFC model by class (subclasses included, e.g. IfcWall finds IfcWallStandardCase), storey name, name (contains) and a property (its set optional; equals or contains; type properties count). Paged.",
            obj(
                vec![
                    ("doc", s_doc()),
                    ("class", json!({"type": "string", "pattern": "^[A-Za-z0-9]{3,80}$"})),
                    ("storey", json!({"type": "string", "maxLength": 255})),
                    ("name", json!({"type": "string", "maxLength": 255})),
                    (
                        "property",
                        obj(
                            vec![
                                ("set", json!({"type": "string", "maxLength": 255})),
                                ("name", json!({"type": "string", "minLength": 1, "maxLength": 255})),
                                ("equals", json!({"type": ["string", "number", "boolean"]})),
                                ("contains", json!({"type": "string", "maxLength": 255})),
                            ],
                            &["name"],
                        ),
                    ),
                    ("offset", s_offset()),
                    ("limit", json!({"type": "integer", "minimum": 1, "maximum": 500, "default": 100})),
                ],
                &["doc"],
            ),
        ),
        d(
            "ifc_properties",
            "read",
            "IFC element properties",
            "Attributes, property sets and quantities of one element (by its line id from ifc_query), and its type's property sets.",
            obj(vec![("doc", s_doc()), ("id", json!({"type": "integer", "minimum": 1}))], &["doc", "id"]),
        ),
        d(
            "takeoff_open",
            "read",
            "Open a takeoff",
            "Open a TakeoffCraft project (.takeoff.json): items, marks, the PDFs it was measured on and their scales.",
            obj(vec![("path", s_path())], &["path"]),
        ),
        d(
            "takeoff_quantities",
            "read",
            "Takeoff quantities",
            "Quantities per item (in its unit) or per sheet. Marks on sheets with no scale are listed as unscaled, never counted as zero.",
            obj(vec![("doc", s_doc()), ("by", json!({"type": "string", "enum": ["item", "sheet"], "default": "item"}))], &["doc"]),
        ),
        d(
            "takeoff_estimate",
            "read",
            "Takeoff estimate",
            "The priced estimate as TakeoffCraft computes it: a row per item and per assembly component (quantity, waste, material and labor), totals in cents and as text; incomplete when a formula fails or marks are unscaled.",
            doc_only(),
        ),
        d(
            "takeoff_export",
            "export",
            "Export a takeoff",
            "Write the estimate as .csv or .xlsx (the workbook TakeoffCraft makes: estimate with live formulas, by sheet, marks, scales).",
            obj(vec![("doc", s_doc()), ("out", s_path()), ("overwrite", s_overwrite())], &["doc", "out"]),
        ),
        d(
            "markups_list",
            "read",
            "List PDF markups",
            "The sheets of a PDF and its markups as MarkupCraft reads them (type, sheet, text, comment, status, author, dates, punch item, replies), filtered and paged. Annotations from other programs that MarkupCraft cannot edit are marked readOnly.",
            obj(
                vec![
                    ("path", s_path()),
                    ("sheet", json!({"type": "string", "maxLength": 255})),
                    ("status", json!({"type": "string", "enum": ["Open", "Accepted", "Rejected", "Completed"]})),
                    ("type", json!({"type": "string", "maxLength": 40})),
                    ("author", json!({"type": "string", "maxLength": 200})),
                    ("offset", s_offset()),
                    ("limit", json!({"type": "integer", "minimum": 1, "maximum": 1000, "default": 200})),
                ],
                &["path"],
            ),
        ),
    ]
}
