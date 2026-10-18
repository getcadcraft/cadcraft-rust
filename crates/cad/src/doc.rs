//! The drawing a CadCraft session edits: doc.js's `Doc`.
//!
//! Spaces: the model and each layout's paper space are entity maps (`OMap`, JS `Map` order) kept in an arena under a
//! [`SpaceId`] (0 = the model); a layout names its space. Undo ops name the space they touched, so a step recorded on a
//! sheet undoes on that sheet even after the sheet list changed — the JS keeps a reference to the `Map` for the same
//! effect. `space` is whichever is open ([`Doc::set_space`]).
//!
//! Every change goes through [`Doc::change`]: the closure calls add / update / remove (entities), define_block /
//! remove_block, set_layouts, set_meta, set_style, set_views, set_layer_state; the whole change is one undo step.
//! Instead of listeners, the events doc.js emits ('change', 'layers', 'load', 'space') queue up in order:
//! [`Doc::drain_events`].

use std::collections::HashMap;
use std::fmt;

use cc_geom::P2;
use serde_json::Value;

use crate::entity::{Color, Entity, Geo, Obj, num, pt};
use crate::js;
use crate::omap::OMap;

/// Which entity map: 0 = the model, others = layouts' paper spaces.
pub type SpaceId = u64;
pub const MODEL: SpaceId = 0;

/// ISO-25-like dimension defaults (dims.js `DIM_DEFAULTS`; the measurements in drawing units, before × scale).
pub const DIM_DEFAULTS: [(&str, f64); 11] = [
    ("txt", 2.5),
    ("asz", 2.5),
    ("exe", 1.25),
    ("exo", 0.625),
    ("gap", 0.625),
    ("dli", 3.75),
    ("dec", 2.0),
    ("adec", 0.0),
    ("scale", 1.0),
    ("tad", 1.0),
    ("zin", 8.0),
];
pub fn dim_defaults() -> Obj {
    DIM_DEFAULTS.iter().map(|(k, v)| ((*k).to_string(), num(*v))).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    /// An edit outside `change` (a programming error in the JS: 'Edits go through doc.change()').
    NotInChange,
    /// `setStyle` with a table other than dimStyles / textStyles.
    BadStyleTable,
    /// Raised by the closure of a change (it is rolled back).
    Failed(String),
}
impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::NotInChange => f.write_str("Edits go through doc.change()"),
            DocError::BadStyleTable => f.write_str("setStyle: dimStyles or textStyles"),
            DocError::Failed(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for DocError {}

/// A layer: { name, color, on, frozen, locked, plot, linetype, lineweight, newVpFrozen? } plus any other keys a
/// layer tool set (`tr`, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub color: Color,
    pub on: bool,
    pub frozen: bool,
    pub locked: bool,
    pub plot: bool,
    /// absent on a layer `setLayer` made from nothing
    pub linetype: Option<String>,
    pub lineweight: Option<f64>,
    pub new_vp_frozen: Option<bool>,
    pub extra: Obj,
}
impl Layer {
    /// doc.js `addLayer`'s normalisation of a layer object.
    pub fn from_value(l: &Value) -> Layer {
        let g = |k: &str| l.get(k);
        let name = g("name").map(js::to_js_string).unwrap_or_else(|| "undefined".into());
        let name = match g("name") {
            Some(Value::String(s)) => s.clone(),
            _ => name,
        };
        let color = match g("color") {
            None | Some(Value::Null) => Color::Num(7.0),
            Some(v) => Color::from_value(v).unwrap_or(Color::Num(7.0)),
        };
        let on = match g("on") {
            None | Some(Value::Null) => true,
            v => js::truthy(v),
        };
        let plot = match g("plot") {
            None | Some(Value::Null) => true,
            v => js::truthy(v),
        };
        let linetype = match g("linetype") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => "Continuous".into(),
        };
        let lineweight = js::finite(g("lineweight")).unwrap_or(-3.0);
        Layer {
            name,
            color,
            on,
            frozen: js::truthy(g("frozen")),
            locked: js::truthy(g("locked")),
            plot,
            linetype: Some(linetype),
            lineweight: Some(lineweight),
            new_vp_frozen: if js::truthy(g("newVpFrozen")) { Some(true) } else { None },
            extra: Obj::new(),
        }
    }
    /// A layer object as a layer table entry (setLayerState: kept as given).
    pub fn from_state_value(l: &Value) -> Layer {
        let mut m = l.as_object().cloned().unwrap_or_default();
        let mut take = |k: &str| m.remove(k);
        let name = take("name").map(|v| if let Value::String(s) = v { s } else { js::to_js_string(&v) }).unwrap_or_default();
        let color = take("color").and_then(|v| Color::from_value(&v)).unwrap_or(Color::Num(7.0));
        let b = |v: Option<Value>, d: bool| v.map_or(d, |v| js::truthy(Some(&v)));
        let on = b(take("on"), false);
        let frozen = b(take("frozen"), false);
        let locked = b(take("locked"), false);
        let plot = b(take("plot"), false);
        let linetype = take("linetype").and_then(|v| v.as_str().map(str::to_string));
        let lineweight = take("lineweight").and_then(|v| v.as_f64());
        let new_vp_frozen = take("newVpFrozen").and_then(|v| v.as_bool());
        Layer { name, color, on, frozen, locked, plot, linetype, lineweight, new_vp_frozen, extra: m }
    }
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("color".into(), self.color.to_value());
        m.insert("on".into(), Value::Bool(self.on));
        m.insert("frozen".into(), Value::Bool(self.frozen));
        m.insert("locked".into(), Value::Bool(self.locked));
        m.insert("plot".into(), Value::Bool(self.plot));
        if let Some(l) = &self.linetype {
            m.insert("linetype".into(), Value::String(l.clone()));
        }
        if let Some(w) = self.lineweight {
            m.insert("lineweight".into(), num(w));
        }
        if let Some(v) = self.new_vp_frozen {
            m.insert("newVpFrozen".into(), Value::Bool(v));
        }
        for (k, v) in &self.extra {
            if !m.contains_key(k) {
                m.insert(k.clone(), v.clone());
            }
        }
        Value::Object(m)
    }
    /// `Object.assign(layer, patch)`: known keys typed (flags by truthiness), the rest kept as given.
    pub fn assign(&mut self, patch: &Obj) {
        for (k, v) in patch {
            match k.as_str() {
                "name" => self.name = v.as_str().map_or_else(|| js::to_js_string(v), str::to_string),
                "color" => self.color = Color::from_value(v).unwrap_or(Color::Null),
                "on" => self.on = js::truthy(Some(v)),
                "frozen" => self.frozen = js::truthy(Some(v)),
                "locked" => self.locked = js::truthy(Some(v)),
                "plot" => self.plot = js::truthy(Some(v)),
                "linetype" => self.linetype = v.as_str().map(str::to_string),
                "lineweight" => self.lineweight = v.as_f64(),
                "newVpFrozen" => self.new_vp_frozen = v.as_bool(),
                _ => {
                    self.extra.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// A block definition { name, base, entities, attdefs?, xref?, dyn? }.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub name: String,
    pub base: P2,
    pub entities: Vec<Entity>,
    pub attdefs: Option<Vec<Value>>,
    pub xref: Option<Value>,
    pub dyn_: Option<Value>,
}
impl Block {
    pub fn new(name: &str, base: P2, entities: Vec<Entity>) -> Self {
        Block { name: name.into(), base, entities, attdefs: None, xref: None, dyn_: None }
    }
    /// From the JSON shape `{ name, base, entities, attdefs?, xref?, dyn? }` as given (no normalising).
    pub fn from_value(v: &Value) -> Block {
        let name = v.get("name").map(|n| n.as_str().map_or_else(|| js::to_js_string(n), str::to_string)).unwrap_or_default();
        let base = match v.get("base") {
            Some(b) if js::truthy(Some(b)) => P2::new(js::to_number_opt(b.get("x")), js::to_number_opt(b.get("y"))),
            _ => P2::new(0.0, 0.0),
        };
        let entities = v.get("entities").and_then(Value::as_array).map(|a| a.iter().map(Entity::from_value).collect()).unwrap_or_default();
        let attdefs = v.get("attdefs").and_then(Value::as_array).filter(|a| !a.is_empty()).cloned();
        let xref = v.get("xref").filter(|x| x.is_object()).cloned();
        let dyn_ = v.get("dyn").filter(|x| x.is_object()).cloned();
        Block { name, base, entities, attdefs, xref, dyn_ }
    }
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("base".into(), pt(self.base));
        m.insert("entities".into(), Value::Array(self.entities.iter().map(Entity::to_value).collect()));
        if let Some(a) = &self.attdefs {
            m.insert("attdefs".into(), Value::Array(a.clone()));
        }
        if let Some(x) = &self.xref {
            m.insert("xref".into(), x.clone());
        }
        if let Some(d) = &self.dyn_ {
            m.insert("dyn".into(), d.clone());
        }
        Value::Object(m)
    }
}

/// A layout (sheet): its name, paper `{ w, h, unit, x0, y0, key? }` and the space holding its entities.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub name: String,
    pub paper: Obj,
    pub space: SpaceId,
}
impl Layout {
    pub fn paper_num(&self, k: &str) -> Option<f64> {
        self.paper.get(k).and_then(Value::as_f64)
    }
    pub fn w(&self) -> Option<f64> {
        self.paper_num("w")
    }
    pub fn h(&self) -> Option<f64> {
        self.paper_num("h")
    }
    pub fn x0(&self) -> Option<f64> {
        self.paper_num("x0")
    }
    pub fn y0(&self) -> Option<f64> {
        self.paper_num("y0")
    }
    pub fn unit(&self) -> Option<&str> {
        self.paper.get("unit").and_then(Value::as_str)
    }
    pub fn key(&self) -> Option<&str> {
        self.paper.get("key").and_then(Value::as_str)
    }
}

/// A named view { name, cx, cy, w, h, layout? }.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub name: String,
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    pub layout: Option<String>,
}
impl View {
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        for (k, v) in [("cx", self.cx), ("cy", self.cy), ("w", self.w), ("h", self.h)] {
            m.insert(k.into(), num(v));
        }
        if let Some(l) = &self.layout {
            m.insert("layout".into(), Value::String(l.clone()));
        }
        Value::Object(m)
    }
    /// A view as given to `set_views` (no checks; load() checks file views).
    pub fn from_value(v: &Value) -> View {
        let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        View {
            name: v.get("name").and_then(Value::as_str).unwrap_or("").into(),
            cx: f("cx"),
            cy: f("cy"),
            w: f("w"),
            h: f("h"),
            layout: v.get("layout").and_then(Value::as_str).map(str::to_string),
        }
    }
}

/// A linetype a file brought { name, description, pattern }.
#[derive(Clone, Debug, PartialEq)]
pub struct Linetype {
    pub name: String,
    pub description: String,
    pub pattern: Vec<f64>,
}

/// A file's text style { font, h, wf, obl } (typed view of a `text_styles` entry).
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font: String,
    pub h: f64,
    pub wf: f64,
    pub obl: f64,
}
impl TextStyle {
    pub fn of(o: &Obj) -> TextStyle {
        let f = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        TextStyle { font: o.get("font").map(js::to_js_string).unwrap_or_default(), h: f("h"), wf: f("wf"), obl: f("obl") }
    }
}

/// What new objects take: layer, colour, text height, dimension style, linetype / lineweight (None = by layer).
#[derive(Clone, Debug, PartialEq)]
pub struct Current {
    pub layer: String,
    pub color: Color,
    pub text_height: f64,
    pub dim: Obj,
    pub linetype: Option<String>,
    pub lineweight: Option<f64>,
}
impl Current {
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        m.insert("layer".into(), Value::String(self.layer.clone()));
        m.insert("color".into(), self.color.to_value());
        m.insert("textHeight".into(), num(self.text_height));
        m.insert("dim".into(), Value::Object(self.dim.clone()));
        m.insert("linetype".into(), self.linetype.clone().map_or(Value::Null, Value::String));
        m.insert("lineweight".into(), self.lineweight.map_or(Value::Null, num));
        Value::Object(m)
    }
}

/// The layer table and current layer as an undo step keeps them.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerState {
    pub list: Vec<Layer>,
    pub current: String,
}

/// The named-style tables `set_style` edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleTable {
    DimStyles,
    TextStyles,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Op {
    Ent {
        space: SpaceId,
        id: u64,
        before: Option<Entity>,
        after: Option<Entity>,
    },
    Block {
        name: String,
        before: Option<Block>,
        after: Option<Block>,
    },
    Layouts {
        before: Vec<Layout>,
        after: Vec<Layout>,
    },
    /// before: the old value of each key (None = the key was not there)
    Meta {
        before: Vec<(String, Option<Value>)>,
        after: Vec<(String, Option<Value>)>,
    },
    LayerState {
        before: LayerState,
        after: LayerState,
    },
    Style {
        table: StyleTable,
        name: String,
        before: Option<Value>,
        after: Option<Value>,
    },
    Views {
        before: Vec<View>,
        after: Vec<View>,
    },
}

/// One undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub label: String,
    pub ops: Vec<Op>,
}

/// What doc.js emits to its listeners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocEvent {
    Change,
    Layers,
    Load,
    Space,
}
impl DocEvent {
    pub fn name(self) -> &'static str {
        match self {
            DocEvent::Change => "change",
            DocEvent::Layers => "layers",
            DocEvent::Load => "load",
            DocEvent::Space => "space",
        }
    }
}

/// The drawing. See the module doc.
#[derive(Clone, Debug)]
pub struct Doc {
    spaces: HashMap<SpaceId, OMap<u64, Entity>>,
    next_space: SpaceId,
    /// the open space
    pub space: SpaceId,
    pub layouts: Vec<Layout>,
    pub views: Vec<View>,
    pub layers: OMap<String, Layer>,
    pub linetypes: OMap<String, Linetype>,
    /// a file's text styles: { font, h, wf, obl } from load; whatever set_style was given
    pub text_styles: OMap<String, Obj>,
    pub dim_styles: OMap<String, Obj>,
    pub blocks: OMap<String, Block>,
    pub next_id: u64,
    pub current: Current,
    pub meta: Obj,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    /// bumps on every change (renderers cache on it)
    pub version: u64,
    pub saved_version: u64,
    pending: Option<Step>,
    layers_moved: bool,
    space_lost: bool,
    events: Vec<DocEvent>,
    // ---- kept, not ported (see the crate doc): round-tripped as the file gave them
    /// split screen (panes.js shape), as given to load
    pub vports: Option<Value>,
    /// the file's UCS { current, named }, as given to load
    pub ucs: Option<Value>,
    /// one pane's view turn (radians)
    pub view_twist: f64,
    /// constraints and parameters (gcons.js payload), as given to load
    pub parametric: Option<Value>,
    /// 'face3d' entities and `faces3d` that load took out of the 2D drawing (model3d.js split3d)
    pub faces3d: Vec<Value>,
    /// the file's dynamic-block objects (dwg-save.js)
    pub dyn_src: Option<Value>,
}

impl Default for Doc {
    fn default() -> Self {
        Self::new()
    }
}

/// The entity's block name (`e.block`): an insert's, a dimension's imported geometry, or any other object's
/// `block` key.
pub fn block_ref(e: &Entity) -> Option<&str> {
    match &e.geo {
        Geo::Insert(i) => Some(i.block.as_str()),
        Geo::Dim(d) => d.block.as_deref(),
        _ => e.extra.get("block").and_then(Value::as_str),
    }
}

impl Doc {
    pub fn new() -> Self {
        let mut spaces = HashMap::new();
        spaces.insert(MODEL, OMap::new());
        let mut d = Doc {
            spaces,
            next_space: 1,
            space: MODEL,
            layouts: Vec::new(),
            views: Vec::new(),
            layers: OMap::new(),
            linetypes: OMap::new(),
            text_styles: OMap::new(),
            dim_styles: OMap::new(),
            blocks: OMap::new(),
            next_id: 1,
            current: Current {
                layer: "0".into(),
                color: Color::Null,
                text_height: 2.5,
                dim: dim_defaults(),
                linetype: None,
                lineweight: None,
            },
            meta: default_meta(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            version: 0,
            saved_version: 0,
            pending: None,
            layers_moved: false,
            space_lost: false,
            events: Vec::new(),
            vports: None,
            ucs: None,
            view_twist: 0.0,
            parametric: None,
            faces3d: Vec::new(),
            dyn_src: None,
        };
        d.add_layer(&serde_json::json!({ "name": "0", "color": 7 }));
        d
    }

    // ---- events
    fn emit(&mut self, e: DocEvent) {
        self.events.push(e);
    }
    /// The events since the last call, in order.
    pub fn drain_events(&mut self) -> Vec<DocEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
        self.emit(DocEvent::Change);
    }

    // ---- spaces
    /// A new, empty space for a layout (put it in a layout and pass the list to `set_layouts`).
    pub fn new_space(&mut self) -> SpaceId {
        let id = self.next_space;
        self.next_space = self.next_space.saturating_add(1);
        self.spaces.insert(id, OMap::new());
        id
    }
    /// A space of its own (the block editor's, commands.js BEDIT), holding these objects as they are (each keeps
    /// its id): not an edit. Open it with `set_space_id`; `drop_space` when done.
    pub fn loose_space(&mut self, ents: Vec<Entity>) -> SpaceId {
        let id = self.new_space();
        if let Some(m) = self.spaces.get_mut(&id) {
            for e in ents {
                if let Some(i) = e.id {
                    m.insert(i, e);
                }
            }
        }
        id
    }
    /// Forget a loose space (never the model or a layout's).
    pub fn drop_space(&mut self, s: SpaceId) {
        if s != MODEL && !self.layouts.iter().any(|l| l.space == s) {
            self.spaces.remove(&s);
        }
    }
    pub fn space_map(&self, s: SpaceId) -> Option<&OMap<u64, Entity>> {
        self.spaces.get(&s)
    }
    /// The open space's entities in draw order.
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.spaces.get(&self.space).into_iter().flat_map(OMap::values)
    }
    /// The model's entities in draw order.
    pub fn model(&self) -> impl Iterator<Item = &Entity> {
        self.spaces.get(&MODEL).into_iter().flat_map(OMap::values)
    }
    pub fn space_entities(&self, s: SpaceId) -> impl Iterator<Item = &Entity> {
        self.spaces.get(&s).into_iter().flat_map(OMap::values)
    }
    pub fn in_model(&self) -> bool {
        self.space == MODEL
    }
    /// The open layout, or None in model space.
    pub fn layout(&self) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.space == self.space)
    }
    /// Open the model (None) or a layout by name (an unknown name opens the model). Not an edit.
    pub fn set_space(&mut self, which: Option<&str>) {
        let next = which.and_then(|n| self.layouts.iter().find(|l| l.name == n)).map_or(MODEL, |l| l.space);
        self.set_space_id(next);
    }
    pub fn set_space_id(&mut self, next: SpaceId) {
        if next == self.space {
            return;
        }
        self.space = next;
        self.emit(DocEvent::Space);
    }

    // ---- layers
    /// doc.js `addLayer(l)` (l: a layer object as JSON).
    pub fn add_layer(&mut self, l: &Value) -> Layer {
        let layer = Layer::from_value(l);
        self.layers.insert(layer.name.clone(), layer.clone());
        layer
    }
    /// The layer, made (with the defaults) when it does not exist.
    pub fn layer(&mut self, name: &str) -> Layer {
        if let Some(l) = self.layers.get(&name.to_string()) {
            return l.clone();
        }
        self.add_layer(&serde_json::json!({ "name": name }))
    }
    /// What a new object takes besides layer and colour: `{lt, lw}` when they are not by layer.
    pub fn current_style(&self) -> (Option<String>, Option<f64>) {
        let lt = self.current.linetype.clone().filter(|s| !s.is_empty());
        let lw = self.current.lineweight.filter(|w| w.is_finite());
        (lt, lw)
    }
    pub fn layer_state(&self) -> LayerState {
        LayerState { list: self.layers.values().cloned().collect(), current: self.current.layer.clone() }
    }
    pub fn set_layer_state(&mut self, state: &LayerState) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let before = self.layer_state();
        self.layers.clear();
        for l in &state.list {
            self.layers.insert(l.name.clone(), l.clone());
        }
        if !self.layers.contains_key(&"0".to_string()) {
            self.add_layer(&serde_json::json!({ "name": "0", "color": 7 }));
        }
        self.current.layer = if self.layers.contains_key(&state.current) { state.current.clone() } else { "0".into() };
        let after = self.layer_state();
        self.push(Op::LayerState { before, after })?;
        self.layers_moved = true;
        Ok(())
    }
    /// Change a layer and/or make it current (`current` also turns it on and thaws it): one undo step, or part of
    /// the change it runs in. A new name moves the objects on the layer (model, sheets, blocks) in the same step.
    pub fn set_layer(&mut self, name: &str, patch: &Obj, current: bool) -> Result<(), DocError> {
        let rename = patch.get("name").and_then(Value::as_str).filter(|n| !n.is_empty() && *n != name).map(str::to_string);
        let label = if rename.is_some() { "RENAME LAYER" } else { "LAYER" };
        let name = name.to_string();
        self.change(label, |d| {
            let mut state = d.layer_state();
            let pos = match state.list.iter().position(|x| x.name == name) {
                Some(p) => p,
                None => {
                    state.list.push(Layer {
                        name: name.clone(),
                        color: Color::Num(7.0),
                        on: true,
                        frozen: false,
                        locked: false,
                        plot: true,
                        linetype: None,
                        lineweight: None,
                        new_vp_frozen: None,
                        extra: Obj::new(),
                    });
                    state.list.len().saturating_sub(1)
                }
            };
            if let Some(l) = state.list.get_mut(pos) {
                l.assign(patch);
                if current {
                    l.on = true;
                    l.frozen = false;
                    state.current = l.name.clone();
                }
            }
            if let Some(rename) = &rename {
                if state.current == name {
                    state.current = rename.clone();
                }
                let mut maps = vec![MODEL];
                maps.extend(d.layouts.iter().map(|l| l.space));
                for s in maps {
                    let list: Vec<Entity> = d.space_entities(s).cloned().collect();
                    for e in list {
                        let frz = match e.extra.get("frz") {
                            Some(Value::Array(a)) if a.iter().any(|n| n.as_str() == Some(name.as_str())) => Some(Value::Array(
                                a.iter()
                                    .map(|n| if n.as_str() == Some(name.as_str()) { Value::String(rename.clone()) } else { n.clone() })
                                    .collect(),
                            )),
                            _ => None,
                        };
                        let on = e.layer.as_deref() == Some(name.as_str());
                        if on || frz.is_some() {
                            let mut n = e.clone();
                            if on {
                                n.layer = Some(rename.clone());
                            }
                            if let Some(f) = frz {
                                n.extra.insert("frz".into(), f);
                            }
                            if let Some(id) = e.id {
                                d.update_in(id, n, s)?;
                            }
                        }
                    }
                }
                let blocks: Vec<Block> = d.blocks.values().cloned().collect();
                for b in blocks {
                    if b.entities.iter().any(|e| e.layer.as_deref() == Some(name.as_str())) {
                        let mut nb = b.clone();
                        for e in &mut nb.entities {
                            if e.layer.as_deref() == Some(name.as_str()) {
                                e.layer = Some(rename.clone());
                            }
                        }
                        d.define_block(&nb)?;
                    }
                }
            }
            d.set_layer_state(&state)
        })?;
        self.emit_layers();
        Ok(())
    }
    fn emit_layers(&mut self) {
        if self.layers_moved && self.pending.is_none() {
            self.layers_moved = false;
            self.emit(DocEvent::Layers);
        }
    }
    pub fn visible(&self, e: &Entity) -> bool {
        match self.layers.get(&e.layer_str().to_string()) {
            None => true,
            Some(l) => l.on && !l.frozen,
        }
    }
    pub fn editable(&self, e: &Entity) -> bool {
        self.visible(e) && !self.layers.get(&e.layer_str().to_string()).is_some_and(|l| l.locked)
    }
    /// `resolveColor(doc, e, byBlock, parentLayer)`.
    pub fn resolve_color(&self, e: &Entity, by_block: Option<&Color>, parent_layer: Option<&str>) -> String {
        crate::color::resolve_color(e, by_block, parent_layer, &|n| self.layers.get(&n.to_string()).map(|l| l.color.clone()))
    }

    // ---- reading
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.spaces.get(&self.space)?.get(&id)
    }
    pub fn all(&self) -> Vec<Entity> {
        self.entities().cloned().collect()
    }
    pub fn count(&self) -> usize {
        self.spaces.get(&self.space).map_or(0, OMap::len)
    }

    // ---- changes
    /// One undo step: `f` makes the edits; an error rolls them all back and comes out unchanged. Nested calls are
    /// part of the outer change.
    pub fn change<R>(&mut self, label: &str, f: impl FnOnce(&mut Doc) -> Result<R, DocError>) -> Result<R, DocError> {
        if self.pending.is_some() {
            return f(self);
        }
        self.pending = Some(Step { label: label.into(), ops: Vec::new() });
        let result = f(self);
        let step = self.pending.take().unwrap_or(Step { label: label.into(), ops: Vec::new() });
        match result {
            Err(e) => {
                self.rollback(&step.ops);
                Err(e)
            }
            Ok(r) => {
                if !step.ops.is_empty() {
                    self.undo_stack.push(step);
                    if self.undo_stack.len() > 500 {
                        self.undo_stack.remove(0);
                    }
                    self.redo_stack.clear();
                    self.version = self.version.saturating_add(1);
                    self.emit(DocEvent::Change);
                    self.emit_layers();
                }
                Ok(r)
            }
        }
    }
    fn must_be_in_change(&self) -> Result<(), DocError> {
        if self.pending.is_some() { Ok(()) } else { Err(DocError::NotInChange) }
    }
    fn push(&mut self, op: Op) -> Result<(), DocError> {
        self.pending.as_mut().ok_or(DocError::NotInChange)?.ops.push(op);
        Ok(())
    }

    /// Add to the open space: `{ color: null, ...e, layer: e.layer ?? current.layer }` with a new id.
    pub fn add(&mut self, e: Entity) -> Result<Entity, DocError> {
        let s = self.space;
        self.add_in(e, s)
    }
    pub fn add_in(&mut self, e: Entity, space: SpaceId) -> Result<Entity, DocError> {
        self.must_be_in_change()?;
        let mut ent = e;
        if ent.color.is_none() {
            ent.color = Some(Color::Null);
        }
        if ent.layer.is_none() {
            ent.layer = Some(self.current.layer.clone());
            ent.extra.remove("layer");
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        ent.id = Some(id);
        ent.extra.remove("id");
        let lname = ent.layer_str().to_string();
        self.layer(&lname);
        self.spaces.entry(space).or_default().insert(id, ent.clone());
        self.push(Op::Ent { space, id, before: None, after: Some(ent.clone()) })?;
        Ok(ent)
    }
    /// Replace entity `id` of the open space with `{ ...e, id }` (None when there is no such entity).
    pub fn update(&mut self, id: u64, e: Entity) -> Result<Option<Entity>, DocError> {
        let s = self.space;
        self.update_in(id, e, s)
    }
    pub fn update_in(&mut self, id: u64, e: Entity, space: SpaceId) -> Result<Option<Entity>, DocError> {
        self.must_be_in_change()?;
        let Some(before) = self.spaces.get(&space).and_then(|m| m.get(&id)).cloned() else { return Ok(None) };
        let mut after = e;
        after.id = Some(id);
        after.extra.remove("id");
        self.spaces.entry(space).or_default().insert(id, after.clone());
        self.push(Op::Ent { space, id, before: Some(before), after: Some(after.clone()) })?;
        Ok(Some(after))
    }
    pub fn remove(&mut self, id: u64) -> Result<(), DocError> {
        let s = self.space;
        self.remove_in(id, s)
    }
    pub fn remove_in(&mut self, id: u64, space: SpaceId) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let Some(before) = self.spaces.get_mut(&space).and_then(|m| m.remove(&id)) else { return Ok(()) };
        self.push(Op::Ent { space, id, before: Some(before), after: None })
    }
    /// Add or replace a block definition (entities lose their ids; base `x || 0`, `y || 0`).
    pub fn define_block(&mut self, b: &Block) -> Result<Block, DocError> {
        self.must_be_in_change()?;
        let before = self.blocks.get(&b.name).cloned();
        let after = Block {
            name: b.name.clone(),
            base: P2::new(js::or(Some(b.base.x), 0.0), js::or(Some(b.base.y), 0.0)),
            entities: b
                .entities
                .iter()
                .map(|e| {
                    let mut c = e.clone();
                    c.id = None;
                    c.extra.remove("id");
                    c
                })
                .collect(),
            attdefs: b.attdefs.clone().filter(|a| !a.is_empty()),
            xref: b.xref.clone().filter(Value::is_object),
            dyn_: b.dyn_.clone().filter(Value::is_object),
        };
        self.blocks.insert(b.name.clone(), after.clone());
        self.push(Op::Block { name: b.name.clone(), before, after: Some(after.clone()) })?;
        Ok(after)
    }
    pub fn remove_block(&mut self, name: &str) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let Some(before) = self.blocks.remove(&name.to_string()) else { return Ok(()) };
        self.push(Op::Block { name: name.into(), before: Some(before), after: None })
    }
    /// Change drawing settings kept in meta (lunits, luprec, …): undoable.
    pub fn set_meta(&mut self, patch: &Obj) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let before = patch.keys().map(|k| (k.clone(), self.meta.get(k).cloned())).collect();
        let after = patch.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect();
        self.push(Op::Meta { before, after })?;
        for (k, v) in patch {
            self.meta.insert(k.clone(), v.clone());
        }
        Ok(())
    }
    /// Set (or with None remove) one named style.
    pub fn set_style(&mut self, table: StyleTable, name: &str, value: Option<&Value>) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let before = self.style_value(table, name);
        self.push(Op::Style { table, name: name.into(), before, after: value.cloned() })?;
        self.put_style(table, name, value);
        Ok(())
    }
    /// A style table by its JS name ('dimStyles' | 'textStyles').
    pub fn style_table(name: &str) -> Result<StyleTable, DocError> {
        match name {
            "dimStyles" => Ok(StyleTable::DimStyles),
            "textStyles" => Ok(StyleTable::TextStyles),
            _ => Err(DocError::BadStyleTable),
        }
    }
    fn style_value(&self, table: StyleTable, name: &str) -> Option<Value> {
        let k = name.to_string();
        match table {
            StyleTable::DimStyles => self.dim_styles.get(&k).map(|o| Value::Object(o.clone())),
            StyleTable::TextStyles => self.text_styles.get(&k).map(|o| Value::Object(o.clone())),
        }
    }
    fn put_style(&mut self, table: StyleTable, name: &str, value: Option<&Value>) {
        let k = name.to_string();
        match (table, value) {
            (StyleTable::DimStyles, Some(v)) => self.dim_styles.insert(k, v.as_object().cloned().unwrap_or_default()),
            (StyleTable::TextStyles, Some(v)) => self.text_styles.insert(k, v.as_object().cloned().unwrap_or_default()),
            (StyleTable::DimStyles, None) => {
                self.dim_styles.remove(&k);
            }
            (StyleTable::TextStyles, None) => {
                self.text_styles.remove(&k);
            }
        }
    }
    /// Replace the named views (undoable).
    pub fn set_views(&mut self, list: Vec<View>) -> Result<(), DocError> {
        self.must_be_in_change()?;
        let before = std::mem::replace(&mut self.views, list.clone());
        self.push(Op::Views { before, after: list })
    }
    /// Replace the list of layouts (add, remove, rename, re-order, change paper).
    pub fn set_layouts(&mut self, list: Vec<Layout>) -> Result<(), DocError> {
        self.must_be_in_change()?;
        for l in &list {
            self.spaces.entry(l.space).or_default();
        }
        let before = std::mem::replace(&mut self.layouts, list.clone());
        self.push(Op::Layouts { before, after: list.clone() })?;
        if !self.in_model() && !list.iter().any(|l| l.space == self.space) {
            self.space = MODEL;
        }
        Ok(())
    }

    fn apply(&mut self, op: &Op, forward: bool) {
        match op {
            Op::Layouts { before, after } => {
                let v = if forward { after } else { before };
                self.layouts = v.clone();
                if !self.in_model() && !v.iter().any(|l| l.space == self.space) {
                    self.space = MODEL;
                    self.space_lost = true;
                }
            }
            Op::Meta { before, after } => {
                for (k, v) in if forward { after } else { before } {
                    match v {
                        Some(v) => {
                            self.meta.insert(k.clone(), v.clone());
                        }
                        None => {
                            self.meta.remove(k);
                        }
                    }
                }
            }
            Op::Views { before, after } => self.views = if forward { after.clone() } else { before.clone() },
            Op::LayerState { before, after } => {
                let v = if forward { after } else { before };
                self.layers.clear();
                for l in &v.list {
                    self.layers.insert(l.name.clone(), l.clone());
                }
                self.current.layer = v.current.clone();
                self.layers_moved = true;
            }
            Op::Style { table, name, before, after } => {
                let v = if forward { after } else { before };
                self.put_style(*table, name, v.as_ref());
            }
            Op::Block { name, before, after } => match if forward { after } else { before } {
                Some(b) => self.blocks.insert(name.clone(), b.clone()),
                None => {
                    self.blocks.remove(name);
                }
            },
            Op::Ent { space, id, before, after } => {
                let m = self.spaces.entry(*space).or_default();
                match if forward { after } else { before } {
                    Some(e) => m.insert(*id, e.clone()),
                    None => {
                        m.remove(id);
                    }
                }
            }
        }
    }
    fn rollback(&mut self, ops: &[Op]) {
        for op in ops.iter().rev() {
            self.apply(op, false);
        }
    }

    /// Undo steps recorded so far: pass to `squash` to fold everything after it into one step.
    pub fn mark(&self) -> usize {
        self.undo_stack.len()
    }
    pub fn squash(&mut self, mark: usize, label: &str) {
        let n = self.undo_stack.len();
        if n.saturating_sub(mark) < 2 {
            if n > mark
                && let Some(s) = self.undo_stack.get_mut(mark)
            {
                s.label = label.into();
            }
            return;
        }
        let steps = self.undo_stack.split_off(mark.min(n));
        self.undo_stack.push(Step { label: label.into(), ops: steps.into_iter().flat_map(|s| s.ops).collect() });
    }
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    /// Undo the last step → its label (None = nothing to undo).
    pub fn undo(&mut self) -> Option<String> {
        let step = self.undo_stack.pop()?;
        self.rollback(&step.ops);
        let label = step.label.clone();
        self.redo_stack.push(step);
        self.after_history();
        Some(label)
    }
    pub fn redo(&mut self) -> Option<String> {
        let step = self.redo_stack.pop()?;
        for op in &step.ops {
            self.apply(op, true);
        }
        let label = step.label.clone();
        self.undo_stack.push(step);
        self.after_history();
        Some(label)
    }
    fn after_history(&mut self) {
        self.version = self.version.saturating_add(1);
        if self.space_lost {
            self.space_lost = false;
            self.emit(DocEvent::Space);
        }
        self.emit(DocEvent::Change);
        self.emit_layers();
    }

    /// Every entity, in every space (model, then the layouts in order).
    pub fn every_entity(&self) -> impl Iterator<Item = &Entity> {
        self.model().chain(self.layouts.iter().flat_map(|l| self.space_entities(l.space)))
    }
    /// Inserts (and dimensions) that use a block, in any space or inside other blocks.
    pub fn block_uses(&self, name: &str) -> usize {
        let a = self.every_entity().filter(|e| block_ref(e) == Some(name)).count();
        let b = self.blocks.values().flat_map(|b| b.entities.iter()).filter(|e| block_ref(e) == Some(name)).count();
        a.saturating_add(b)
    }

    /// Replace everything (open / new) with what a reader made (doc.load's input shape). Not undoable.
    pub fn load(&mut self, input: &Value) {
        let get = |k: &str| input.get(k).filter(|v| !v.is_null());
        let list = |k: &str| get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        self.view_twist = js::finite(input.get("viewTwist")).filter(|v| v.abs() <= 100.0).unwrap_or(0.0);
        self.ucs = get("ucs").cloned();
        self.vports = get("vports").cloned();
        // model3d.js split3d: 'face3d' objects are not 2D; CADCRAFT_3D blocks hold the 3D model
        let mut entities = list("entities");
        let mut faces: Vec<Value> = get("faces3d").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut kept = Vec::new();
        entities.retain(|e| {
            if e.get("type").and_then(Value::as_str) == Some("face3d") {
                kept.push(e.clone());
                false
            } else {
                true
            }
        });
        faces.extend(kept);
        self.faces3d = faces;
        let mut blocks = list("blocks");
        blocks.retain(|b| !b.get("name").is_some_and(|n| js::to_js_string(n).to_uppercase() == "CADCRAFT_3D"));
        let mut model = OMap::new();
        self.spaces.clear();
        self.layers.clear();
        self.blocks.clear();
        self.layouts.clear();
        self.space = MODEL;
        self.next_space = 1;
        self.next_id = 1;
        self.add_layer(&serde_json::json!({ "name": "0", "color": 7 }));
        for l in list("layers") {
            self.add_layer(&l);
        }
        for b in &blocks {
            let mut blk = Block::from_value(b);
            blk.attdefs = b.get("attdefs").and_then(Value::as_array).filter(|a| !a.is_empty()).cloned();
            blk.xref = b
                .get("xref")
                .filter(|x| x.is_object())
                .map(|x| serde_json::json!({ "path": js::slice16(&x.get("path").filter(|p| js::truthy(Some(p))).map(js::to_js_string).unwrap_or_default(), 1024) }));
            blk.dyn_ = b.get("dyn").filter(|x| x.is_object()).cloned();
            self.blocks.insert(blk.name.clone(), blk);
        }
        self.dyn_src = input.get("dynSrc").filter(|d| d.get("objects").is_some_and(Value::is_array)).cloned();
        self.fill(&mut model, &entities);
        let parametric = get("parametric").is_some();
        let keys: Vec<u64> = model.keys().copied().collect();
        for k in keys {
            if let Some(e) = model.get_mut(&k) {
                e.extra.remove("ccref");
            }
        }
        self.parametric = if parametric { get("parametric").cloned() } else { None };
        self.spaces.insert(MODEL, model);
        for l in list("layouts") {
            let mut map = OMap::new();
            let ents = l.get("entities").and_then(Value::as_array).cloned().unwrap_or_default();
            self.fill(&mut map, &ents);
            let s = self.next_space;
            self.next_space = self.next_space.saturating_add(1);
            self.spaces.insert(s, map);
            let mut paper = Obj::new();
            paper.insert("x0".into(), Value::from(0));
            paper.insert("y0".into(), Value::from(0));
            if let Some(p) = l.get("paper").and_then(Value::as_object) {
                for (k, v) in p {
                    paper.insert(k.clone(), v.clone());
                }
            }
            let name = l.get("name").map(|n| n.as_str().map_or_else(|| js::to_js_string(n), str::to_string)).unwrap_or_default();
            self.layouts.push(Layout { name, paper, space: s });
        }
        self.views = get("views")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| {
                        let name = v.get("name")?.as_str().filter(|n| !n.is_empty())?;
                        let f = |k: &str| js::finite(v.get(k));
                        let (cx, cy, w, h) = (f("cx")?, f("cy")?, f("w")?, f("h")?);
                        if !(w > 0.0 && h > 0.0) {
                            return None;
                        }
                        Some(View {
                            name: js::slice16(name, 255),
                            cx,
                            cy,
                            w,
                            h,
                            layout: v.get("layout").and_then(Value::as_str).map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut meta = default_meta();
        if let Some(m) = get("meta").and_then(Value::as_object) {
            for (k, v) in m {
                meta.insert(k.clone(), v.clone());
            }
        }
        self.meta = meta;
        self.current.layer = "0".into();
        self.current.linetype = None;
        self.current.lineweight = None;
        self.linetypes = list("linetypes")
            .iter()
            .filter_map(|l| {
                let name = l.get("name")?.as_str().filter(|n| !n.is_empty())?;
                let pat = l.get("pattern")?.as_array()?;
                if pat.len() > 64 {
                    return None;
                }
                let pattern = pat.iter().map(|v| v.as_f64().filter(|x| x.is_finite())).collect::<Option<Vec<f64>>>()?;
                let description =
                    js::slice16(&l.get("description").filter(|d| js::truthy(Some(d))).map(js::to_js_string).unwrap_or_default(), 255);
                Some((name.to_string(), Linetype { name: name.to_string(), description, pattern }))
            })
            .collect();
        let mut dim = dim_defaults();
        if let Some(ds) = get("dimStyle").and_then(Value::as_object) {
            for (k, v) in ds {
                dim.insert(k.clone(), v.clone());
            }
        }
        self.current.dim = dim;
        self.text_styles = named(get("textStyles"))
            .into_iter()
            .map(|(n, v)| {
                let f = |k: &str| js::finite(v.get(k));
                let font = js::slice16(&v.get("font").filter(|x| js::truthy(Some(x))).map(js::to_js_string).unwrap_or_default(), 255);
                (
                    n,
                    text_style_obj(&TextStyle {
                        font,
                        h: f("h").filter(|h| *h >= 0.0).unwrap_or(0.0),
                        wf: f("wf").filter(|w| *w > 0.0).unwrap_or(1.0),
                        obl: f("obl").unwrap_or(0.0),
                    }),
                )
            })
            .collect();
        self.dim_styles = named(get("dimStyles"))
            .into_iter()
            .map(|(n, v)| {
                let mut o = Obj::new();
                let entries: Vec<(String, Value)> = match &v {
                    Value::Object(m) => m.iter().map(|(k, x)| (k.clone(), x.clone())).collect(),
                    Value::Array(a) => a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect(),
                    _ => Vec::new(),
                };
                for (k, x) in entries {
                    let keep = match &x {
                        Value::Null | Value::String(_) | Value::Bool(_) => true,
                        Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
                        _ => false,
                    };
                    if keep {
                        o.insert(k, x);
                    }
                }
                (n, o)
            })
            .collect();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.version = self.version.saturating_add(1);
        self.saved_version = self.version;
        self.emit(DocEvent::Load);
    }
    fn fill(&mut self, map: &mut OMap<u64, Entity>, list: &[Value]) {
        for v in list {
            let mut ent = Entity::from_value(v);
            if ent.color.is_none() && !ent.extra.contains_key("color") {
                ent.color = Some(Color::Null);
            }
            if ent.layer.as_deref().is_none_or(str::is_empty) {
                ent.layer = Some("0".into());
                ent.extra.remove("layer");
            }
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            ent.id = Some(id);
            ent.extra.remove("id");
            let lname = ent.layer_str().to_string();
            self.layer(&lname);
            map.insert(id, ent);
        }
    }

    /// The comparison form (the parity harness writes the same from the JS Doc):
    /// `{ meta, current, layers[], blocks[], model[], layouts[{name, paper, entities[]}], views[], space,
    /// textStyles[[n, v]], dimStyles[[n, v]], linetypes[], version, nextId, undo[labels], redo[labels], dirty,
    /// viewTwist }`. `space` = the open layout's index, -1 in the model.
    pub fn snapshot(&self) -> Value {
        let ents = |s: SpaceId| Value::Array(self.space_entities(s).map(Entity::to_value).collect());
        let space = if self.in_model() {
            -1i64
        } else {
            self.layouts.iter().position(|l| l.space == self.space).map_or(-1, |p| i64::try_from(p).unwrap_or(-1))
        };
        serde_json::json!({
            "meta": Value::Object(self.meta.clone()),
            "current": self.current.to_value(),
            "layers": self.layers.values().map(Layer::to_value).collect::<Vec<_>>(),
            "blocks": self.blocks.values().map(Block::to_value).collect::<Vec<_>>(),
            "model": ents(MODEL),
            "layouts": self.layouts.iter().map(|l| serde_json::json!({ "name": l.name, "paper": Value::Object(l.paper.clone()), "entities": ents(l.space) })).collect::<Vec<_>>(),
            "views": self.views.iter().map(View::to_value).collect::<Vec<_>>(),
            "space": space,
            "textStyles": self.text_styles.iter().map(|(n, v)| serde_json::json!([n, Value::Object(v.clone())])).collect::<Vec<_>>(),
            "dimStyles": self.dim_styles.iter().map(|(n, v)| serde_json::json!([n, Value::Object(v.clone())])).collect::<Vec<_>>(),
            "linetypes": self.linetypes.values().map(|l| serde_json::json!({ "name": l.name, "description": l.description, "pattern": l.pattern.iter().map(|v| num(*v)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "version": self.version,
            "nextId": self.next_id,
            "undo": self.undo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
            "redo": self.redo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
            "dirty": self.dirty(),
            "viewTwist": num(self.view_twist),
        })
    }
}

fn text_style_obj(t: &TextStyle) -> Obj {
    let mut o = Obj::new();
    o.insert("font".into(), Value::String(t.font.clone()));
    o.insert("h".into(), num(t.h));
    o.insert("wf".into(), num(t.wf));
    o.insert("obl".into(), num(t.obl));
    o
}

fn default_meta() -> Obj {
    let mut m = Obj::new();
    m.insert("name".into(), Value::String("Drawing1".into()));
    for k in ["units", "source", "version"] {
        m.insert(k.into(), Value::String(String::new()));
    }
    m
}

/// load()'s `named(m)`: [[name, value]] pairs with a string name (1–255 UTF-16 units) and an object value; at most 1000.
fn named(v: Option<&Value>) -> Vec<(String, Value)> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    a.iter()
        .filter_map(|p| {
            let n = p.get(0)?.as_str()?;
            let x = p.get(1)?;
            let len16: usize = n.chars().map(char::len_utf16).sum();
            (!n.is_empty() && len16 <= 255 && (x.is_object() || x.is_array())).then(|| (n.to_string(), x.clone()))
        })
        .take(1000)
        .collect()
}
