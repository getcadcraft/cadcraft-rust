//! cc-skp reader for classic SketchUp files (SketchUp 3 … 2020), port of `lib/skp/legacy.js`.
//! Format knowledge comes from OpenSKP (MIT, see NOTICE): its legacy walk was the JS authors' map. The code is ours.
//!
//! The format in brief: after the header records the body is ONE uncompressed MFC CArchive object stream with a
//! single store map: every new class record (0xFFFF, schema u16, name length u16, ASCII name) and every new object
//! takes the next 1-based index; later references are back-refs to that index (u16, or 0x7FFF + u32 once indexes get
//! big); 0x8000|n = "a new object of the class at index n". The stream opens with CVersionMap (class name → schema
//! number, which we use to tell layouts apart), then document settings we do not walk, then the material manager,
//! the layer list, the definition list, the model's own entity list and finally options dictionaries
//! (UnitsOptions → LengthUnit). Because we skip the settings region we don't know how many store-map indexes it
//! used: the "slot base" is bootstrapped from the first back-references (OpenSKP's trick). Geometry: CFace →
//! CLoop → CEdgeUse → CEdge → CVertex; components/groups are CComponentDefinition + CComponentInstance/CGroup
//! with a 13-double transform (3×3 row by row, origin, w).
//!
//! Untrusted input: every read goes through the bounds-checked `Reader`, counts are capped, object nesting is
//! depth-capped (`MAX_OBJECT_DEPTH`: the walk recurses through `obj`, so this bounds the stack), and any internal
//! failure is turned into `SkpError('damaged')`.
//!
//! JS objects are shared by reference: a loop keeps the edge-use objects it read even after the store map forgets
//! them. Here every object value lives in an arena (`vals`) and the store map holds arena ids, so "release"
//! (forgetting a finished definition's geometry in the store map) and back-references behave exactly as in the JS.

use std::collections::HashMap;

use crate::messages::msg;
use crate::model::{Def, Edge, Face, Image, Inst, Layer, Material, P3, Texture};
use crate::read::{Body, Header};
use crate::util::{FxMap, FxSet, Limits, Reader, SkpError, err, hypot, js_num_text, ok_num};

// ── errors ─────────────────────────────────────────────────────────────────────────────────────────────

/// `Misparse` (a record we could not make sense of: caught where the layout tolerates it), an `SkpError` (passes
/// through), or an internal failure (the JS's TypeErrors; becomes `failed_internal`).
#[derive(Debug)]
enum LErr {
    Mis(String),
    Skp(SkpError),
    Internal(String),
}

impl From<SkpError> for LErr {
    fn from(e: SkpError) -> Self {
        LErr::Skp(e)
    }
}

type LRes<T> = Result<T, LErr>;

fn mis(key: &str, fills: &[(&str, &str)]) -> LErr {
    LErr::Mis(msg(key, fills))
}

fn mis_at(key: &str, at: usize) -> LErr {
    mis(key, &[("at", &at.to_string())])
}

// ── small helpers ──────────────────────────────────────────────────────────────────────────────────────

const STR: [Option<u8>; 3] = [Some(0xff), Some(0xfe), Some(0xff)];

/// u16 at p, or -1 past the end (the JS helpers return -1).
#[inline]
fn u16at(b: &[u8], p: usize) -> i64 {
    match b.get(p..p.saturating_add(2)) {
        Some(&[x, y]) => i64::from(u16::from_le_bytes([x, y])),
        _ => -1,
    }
}

#[inline]
fn u32at(b: &[u8], p: usize) -> i64 {
    match b.get(p..p.saturating_add(4)) {
        Some(&[a, c, d, e]) => i64::from(u32::from_le_bytes([a, c, d, e])),
        _ => -1,
    }
}

#[inline]
fn str_at(b: &[u8], p: usize) -> bool {
    b.get(p..p.saturating_add(3)) == Some(&[0xff, 0xfe, 0xff])
}

fn ascii_at(b: &[u8], p: usize, s: &str) -> bool {
    b.get(p..p.saturating_add(s.len())) == Some(s.as_bytes())
}

/// First index of `needle` (None = any byte) in b[start, end), or None. Candidates come from a substring search
/// (memchr's memmem) for the needle's longest run of fixed bytes; each is then checked whole.
fn find(b: &[u8], needle: &[Option<u8>], start: usize, end: usize) -> Option<usize> {
    let last = end.min(b.len()).checked_sub(needle.len())?;
    if start > last {
        return None;
    }
    // the longest run of fixed bytes: (offset in the needle, bytes)
    let (mut best, mut cur): ((usize, usize), (usize, usize)) = ((0, 0), (0, 0));
    for (j, n) in needle.iter().enumerate() {
        if n.is_some() {
            if cur.1 == 0 {
                cur.0 = j;
            }
            cur.1 = cur.1.saturating_add(1);
            if cur.1 > best.1 {
                best = cur;
            }
        } else {
            cur.1 = 0;
        }
    }
    if best.1 == 0 {
        return Some(start); // a needle of wildcards matches anywhere
    }
    let run: Vec<u8> = needle.get(best.0..best.0.saturating_add(best.1))?.iter().flatten().copied().collect();
    let matches_at = |i: usize| needle.iter().enumerate().all(|(j, n)| n.is_none_or(|n| b.get(i.saturating_add(j)) == Some(&n)));
    let hay = b.get(start.saturating_add(best.0)..last.saturating_add(best.0).saturating_add(best.1))?;
    let finder = memchr::memmem::Finder::new(&run);
    let mut from = 0usize;
    while let Some(k) = hay.get(from..).and_then(|h| finder.find(h)) {
        let i = start.saturating_add(from).saturating_add(k);
        if matches_at(i) {
            return Some(i);
        }
        from = from.saturating_add(k).saturating_add(1);
    }
    None
}

/// A new-class record for `name`: FF FF <schema u16> <len u16> name.
fn class_record(name: &str) -> Vec<Option<u8>> {
    let mut v = vec![Some(0xff), Some(0xff), None, None, Some(u8::try_from(name.len()).unwrap_or(0)), Some(0)];
    v.extend(name.bytes().map(Some));
    v
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ── class names ────────────────────────────────────────────────────────────────────────────────────────

type Name = u32;

const KNOWN: [&str; 26] = [
    "CVertex",
    "CEdge",
    "CCurve",
    "CArcCurve",
    "CEdgeUse",
    "CLoop",
    "CFace",
    "CLayer",
    "CMaterial",
    "CDib",
    "CAttributeContainer",
    "CAttributeNamed",
    "CCamera",
    "CThumbnail",
    "CRelationship",
    "CComponentDefinition",
    "CImage",
    "CComponentInstance",
    "CGroup",
    "CFaceTextureCoords",
    "CConstructionLine",
    "CConstructionPoint",
    "CSectionPlane",
    "CSkFont",
    "CDimensionLinear",
    "CText",
];
const C_VERTEX: Name = 0;
const C_EDGE: Name = 1;
const C_CURVE: Name = 2;
const C_ARC_CURVE: Name = 3;
const C_EDGE_USE: Name = 4;
const C_LOOP: Name = 5;
const C_FACE: Name = 6;
const C_LAYER: Name = 7;
const C_MATERIAL: Name = 8;
const C_DIB: Name = 9;
const C_ATTR_CONTAINER: Name = 10;
const C_ATTR_NAMED: Name = 11;
const C_CAMERA: Name = 12;
const C_THUMBNAIL: Name = 13;
const C_RELATIONSHIP: Name = 14;
const C_DEFINITION: Name = 15;
const C_IMAGE: Name = 16;
const C_INSTANCE: Name = 17;
const C_GROUP: Name = 18;
const C_FTC: Name = 19;
const C_CLINE: Name = 20;
const C_CPOINT: Name = 21;
const C_SECTION: Name = 22;
const C_SKFONT: Name = 23;
const C_DIM_LINEAR: Name = 24;
const C_TEXT: Name = 25;
/// Pseudo names the walk hands back for references it cannot resolve (never a class from the file).
const PREMODEL: Name = 26;
const RESERVED: Name = 27;

// ── values ─────────────────────────────────────────────────────────────────────────────────────────────

type ValId = u32;

#[derive(Debug, Clone, Copy, Default)]
struct Db {
    mat: u32,
    hidden: bool,
    soft: bool,
    smooth: bool,
    layer: Option<u32>,
}

#[derive(Debug)]
struct FaceV {
    db: Db,
    plane: [f64; 4],
    loops: Vec<Option<ValId>>,
    back: u16,
    ftc: Option<ValId>,
}

#[derive(Debug)]
struct TexV {
    rgba: [u8; 4],
    opacity: f64,
    use_opacity: u8,
    image: (usize, usize),
    w: f64,
    h: f64,
    file: String,
}

#[derive(Debug)]
struct MaterialV {
    name: String,
    rgba: [u8; 4],
    opacity: f64,
    use_opacity: u8,
    tex: Option<TexV>,
}

#[derive(Debug)]
struct InstV {
    cls: Name,
    db: Db,
    def: Option<u32>,
    xf: [f64; 13],
    name: String,
}

type Ent = (Option<u32>, Option<Name>, Option<ValId>);

#[derive(Debug)]
struct DefV {
    name: String,
    guid: String,
    ents: Vec<Ent>,
}

#[derive(Debug)]
enum Val {
    Vertex(P3),
    Edge { db: Db, v1: Option<u32>, v2: Option<u32> },
    Curve,
    Use { edge: Option<u32>, sense: u8 },
    Loop(Vec<Option<ValId>>),
    Face(Box<FaceV>),
    Attrs(Vec<Option<ValId>>),
    Dict(String),
    Layer(Box<(String, bool, [u8; 4])>),
    Material(Box<MaterialV>),
    Dib { start: usize, len: usize },
    Ftc(Box<([f64; 9], bool)>),
    Camera,
    Thumbnail,
    Inst(Box<InstV>),
    Guide,
    Section,
    Font,
    Dim,
    Text,
    Rel,
    Def(Box<DefV>),
}

impl Val {
    /// `v.name` (undefined → None).
    fn name(&self) -> Option<&str> {
        match self {
            Val::Dict(n) => Some(n),
            Val::Layer(l) => Some(&l.0),
            Val::Material(m) => Some(&m.name),
            Val::Inst(i) => Some(&i.name),
            Val::Def(d) => Some(&d.name),
            _ => None,
        }
    }
    /// `v.rgba`.
    fn rgba(&self) -> Option<[u8; 4]> {
        match self {
            Val::Layer(l) => Some(l.2),
            Val::Material(m) => Some(m.rgba),
            _ => None,
        }
    }
}

// ── the archive: store map + object walk ───────────────────────────────────────────────────────────────

/// Object nesting cap: a definition inlined in an instance inlined in a definition … costs ~2 levels per
/// model nesting level, plus face → loop → edge use → edge → vertex and attribute containers.
fn max_object_depth(limits: &Limits) -> usize {
    limits.depth.saturating_mul(2).saturating_add(32)
}
const MAX_SLOTS: u32 = 64_000_000;

/// Layout switches. Each comes from the class's schema number in the file's own CVersionMap when it is there,
/// else from the major version as OpenSKP calibrated it.
#[derive(Debug, Clone, Copy)]
struct Layout {
    major: f64,
    v3: bool,
    pid: bool,
    attr_tail: bool,
    inst_guid: bool,
}

fn layout_for(major: f64, schemas: &HashMap<String, u32>) -> Layout {
    let sch = |name: &str, fallback: u32| schemas.get(name).copied().unwrap_or(fallback);
    Layout {
        major,
        v3: major < 4.0,
        pid: sch("CEntity", if major >= 17.0 { 5 } else { 3 }) >= 5,
        attr_tail: sch("CAttributeNamed", if major >= 7.0 { 1 } else { 0 }) >= 1,
        inst_guid: sch("CComponentInstance", if major >= 14.0 { 5 } else { 4 }) >= 5,
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    class: bool,
    name: Name,
    val: Option<ValId>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Skipped {
    text: u64,
    dims: u64,
    sections: u64,
    guides: u64,
}

/// Raw geometry of one entity list (store-map indexes for materials/layers/definitions, resolved later).
#[derive(Debug, Default)]
struct Built {
    name: String,
    guid: String,
    faces: Vec<BFace>,
    edges: Vec<BEdge>,
    soft: Vec<[P3; 2]>,
    insts: Vec<ValId>,
}

#[derive(Debug)]
struct BFace {
    loops: Vec<Vec<P3>>,
    plane: [f64; 4],
    front: Option<u32>,
    back: Option<u16>,
    layer: Option<u32>,
    hidden: bool,
    ftc: Option<ValId>,
}

#[derive(Debug)]
struct BEdge {
    a: P3,
    b: P3,
    layer: Option<u32>,
    soft: bool,
    smooth: bool,
    hidden: bool,
}

struct Archive<'a> {
    b: &'a [u8],
    lay: Layout,
    limits: Limits,
    r: Reader<'a>,
    names: Vec<String>,
    vals: Vec<Val>,
    /// store-map entries for indexes base..next (allocated in order)
    dense: Vec<Entry>,
    /// entries outside that range (classes learned from the settings region we skipped)
    side: FxMap<u32, Entry>,
    class_slot: FxMap<Name, u32>,
    next: u32,
    base: u32,
    depth: usize,
    vertices: u64,
    current_loop: Option<u32>,
    current_slot: Option<u32>,
    // Annotation "burns" (OpenSKP's finding): attaching a dimension or leader text to geometry maps
    // connection-point objects into the store map without writing any bytes, so later back-references run
    // ahead of our numbering. We record each band [fileStart, width] and translate references through them.
    burns: Vec<(i64, i64)>,
    burn_total: i64,
    annot_mark: Option<u32>,
    burn_credits: Vec<u32>,
    cline_tail: Option<usize>,
    skipped: Skipped,
    root_cut: usize,
    /// build each CComponentDefinition as it completes (false in the slot-base probe)
    build_defs: bool,
    built: Vec<(u32, Built)>,
    bad_faces: u64,
}

impl<'a> Archive<'a> {
    fn new(b: &'a [u8], lay: Layout, limits: Limits) -> Self {
        Archive {
            b,
            lay,
            limits,
            r: Reader::new(b, 0),
            names: KNOWN.iter().map(|s| (*s).to_string()).chain(["premodel".to_string(), "reserved".to_string()]).collect(),
            vals: Vec::new(),
            dense: Vec::new(),
            side: FxMap::default(),
            class_slot: FxMap::default(),
            next: 0,
            base: 0,
            depth: 0,
            vertices: 0,
            current_loop: None,
            current_slot: None,
            burns: Vec::new(),
            burn_total: 0,
            annot_mark: None,
            burn_credits: Vec::new(),
            cline_tail: None,
            skipped: Skipped::default(),
            root_cut: 0,
            build_defs: false,
            built: Vec::new(),
            bad_faces: 0,
        }
    }

    fn start_at(&mut self, base: u32) {
        self.next = base;
        self.base = base;
    }

    fn intern(&mut self, s: &str) -> Name {
        let found =
            self.names.iter().enumerate().find(|&(i, n)| n == s && i != PREMODEL as usize && i != RESERVED as usize).map(|(i, _)| i);
        match found {
            Some(i) => Name::try_from(i).unwrap_or(0),
            None => {
                self.names.push(s.to_string());
                Name::try_from(self.names.len().saturating_sub(1)).unwrap_or(0)
            }
        }
    }

    fn name_text(&self, n: Name) -> &str {
        self.names.get(n as usize).map_or("", String::as_str)
    }

    #[inline]
    fn get(&self, slot: u32) -> Option<&Entry> {
        if slot >= self.base && slot < self.next { self.dense.get(slot.saturating_sub(self.base) as usize) } else { self.side.get(&slot) }
    }

    #[inline]
    fn set(&mut self, slot: u32, e: Entry) {
        if slot >= self.base && slot < self.next {
            if let Some(x) = self.dense.get_mut(slot.saturating_sub(self.base) as usize) {
                *x = e;
            }
        } else {
            self.side.insert(slot, e);
        }
    }

    #[inline]
    fn val(&self, id: ValId) -> Option<&Val> {
        self.vals.get(id as usize)
    }

    /// The value now in a slot (`slots.get(s)?.[2]`).
    #[inline]
    fn slot_val(&self, slot: Option<u32>) -> Option<&Val> {
        let id = self.get(slot?)?.val?;
        self.val(id)
    }

    fn alloc(&mut self, e: Entry) -> LRes<u32> {
        if self.next.saturating_sub(self.base) > MAX_SLOTS {
            return Err(err("too_big", "ccskp.too_big.objects", &[]).into());
        }
        let s = self.next;
        self.next = self.next.saturating_add(1);
        self.dense.push(e);
        Ok(s)
    }

    /// Read one object reference / object → (slot, class name, value); (None, None, None) for a null.
    fn obj(&mut self, expect: Option<Name>) -> LRes<Ent> {
        let tag = self.r.u16()?;
        if tag == 0 {
            return Ok((None, None, None));
        }
        if tag == 0x7fff {
            let big = self.r.u32()?;
            return if big & 0x8000_0000 != 0 { self.of_class(big & 0x7fff_ffff, expect) } else { self.backref(big) };
        }
        if tag == 0xffff {
            let _schema = self.r.u16()?;
            let len = self.r.u16()?;
            if len > 40 {
                return Err(mis_at("ccskp.legacy.class_name", self.r.pos));
            }
            let text = self.r.ascii(usize::from(len))?;
            let name = self.intern(&text);
            let s = self.alloc(Entry { class: true, name, val: None })?;
            self.class_slot.insert(name, s);
            return self.new_obj(name);
        }
        if tag & 0x8000 != 0 {
            return self.of_class(u32::from(tag & 0x7fff), expect);
        }
        self.backref(u32::from(tag))
    }

    fn of_class(&mut self, cslot: u32, expect: Option<Name>) -> LRes<Ent> {
        let ent = match self.get(cslot) {
            Some(e) => *e,
            None => {
                // a class first defined in the settings region we skipped: learn it from what the reader expects here
                let Some(x) = expect else { return Err(mis_at("ccskp.legacy.class_unknown", self.r.pos)) };
                let e = Entry { class: true, name: x, val: None };
                self.set(cslot, e);
                self.class_slot.insert(x, cslot);
                e
            }
        };
        if !ent.class {
            return Err(mis_at("ccskp.legacy.ref_as_class", self.r.pos));
        }
        self.new_obj(ent.name)
    }

    fn new_obj(&mut self, name: Name) -> LRes<Ent> {
        if name as usize >= KNOWN.len() {
            let t = self.name_text(name).to_string();
            return Err(mis("ccskp.legacy.type_unknown", &[("type", &t), ("at", &self.r.pos.to_string())]));
        }
        self.depth = self.depth.saturating_add(1);
        if self.depth > max_object_depth(&self.limits) {
            return Err(err("too_big", "ccskp.too_big.object_depth", &[("levels", &self.limits.depth.to_string())]).into());
        }
        let slot = self.alloc(Entry { class: false, name, val: None })?;
        let prev = self.current_slot;
        self.current_slot = Some(slot);
        let v = self.read_class(name);
        self.current_slot = prev;
        self.depth = self.depth.saturating_sub(1);
        let v = v?;
        let id = ValId::try_from(self.vals.len()).map_err(|_| LErr::Skp(err("too_big", "ccskp.too_big.objects", &[])))?;
        self.vals.push(v);
        self.set(slot, Entry { class: false, name, val: Some(id) });
        if name == C_DEFINITION && self.build_defs {
            self.on_definition(slot, id)?;
        }
        if name == C_DIM_LINEAR || name == C_TEXT {
            self.annot_mark = Some(self.next);
        }
        Ok((Some(slot), Some(name), Some(id)))
    }

    fn read_class(&mut self, name: Name) -> LRes<Val> {
        match name {
            C_VERTEX => self.read_vertex(),
            C_EDGE => self.read_edge(),
            C_CURVE => {
                self.preamble()?;
                self.r.u8()?;
                self.r.u32()?;
                Ok(Val::Curve)
            }
            C_ARC_CURVE => {
                self.preamble()?;
                self.r.skip(5)?;
                self.r.skip(14 * 8)?;
                Ok(Val::Curve)
            }
            C_EDGE_USE => self.read_edge_use(),
            C_LOOP => self.read_loop(),
            C_FACE => self.read_face(),
            C_LAYER => self.read_layer(),
            C_MATERIAL => self.read_material(),
            C_DIB => self.read_dib(),
            C_ATTR_CONTAINER => self.read_attr_container(),
            C_ATTR_NAMED => self.read_attr_named(),
            C_CAMERA => {
                self.r.skip(137)?;
                self.r.u16()?;
                self.str()?;
                self.r.skip(33)?;
                Ok(Val::Camera)
            }
            C_THUMBNAIL => {
                self.preamble()?;
                if self.lay.v3 {
                    self.r.skip(129)?;
                    return Ok(Val::Thumbnail);
                }
                self.obj(Some(C_CAMERA))?;
                self.obj(Some(C_DIB))?;
                Ok(Val::Thumbnail)
            }
            C_RELATIONSHIP => {
                self.preamble()?;
                self.entity_ref()?;
                self.entity_ref()?;
                Ok(Val::Rel)
            }
            C_DEFINITION => self.read_definition(),
            C_IMAGE => self.read_image(),
            C_INSTANCE | C_GROUP => self.read_instance(),
            C_FTC => self.read_ftc(),
            C_CLINE => self.read_construction_line(),
            C_CPOINT => {
                self.preamble()?;
                self.drawbase()?;
                self.r.skip(48)?;
                self.r.u8()?;
                self.skipped.guides = self.skipped.guides.saturating_add(1);
                Ok(Val::Guide)
            }
            C_SECTION => self.read_section_plane(),
            C_SKFONT => {
                self.obj(Some(C_ATTR_CONTAINER))?;
                if self.lay.pid {
                    self.r.u8()?;
                }
                self.str()?;
                self.r.skip(15)?;
                Ok(Val::Font)
            }
            C_DIM_LINEAR => self.read_dim_linear(),
            C_TEXT => self.read_text(),
            _ => {
                let t = self.name_text(name).to_string();
                Err(mis("ccskp.legacy.type_unknown", &[("type", &t), ("at", &self.r.pos.to_string())]))
            }
        }
    }

    /// A file index → our numbering, through the burn bands; None when it points INTO a band.
    fn translate(&self, slot: i64) -> Option<i64> {
        let mut off = 0i64;
        for &(start, width) in &self.burns {
            if slot < start {
                break;
            }
            if slot < start.saturating_add(width) {
                return None;
            }
            off = off.saturating_add(width);
        }
        Some(slot.saturating_sub(off))
    }

    fn backref(&mut self, slot: u32) -> LRes<Ent> {
        let mut slot = i64::from(slot);
        if let Some(&(first, _)) = self.burns.first()
            && slot >= first
        {
            match self.translate(slot) {
                None => return Ok((u32::try_from(slot).ok(), Some(RESERVED), None)),
                Some(t) => slot = t,
            }
        }
        let s = u32::try_from(slot).ok();
        let ent = s.and_then(|s| self.get(s)).copied();
        match ent {
            None => {
                if slot < i64::from(self.base) {
                    return Ok((s, Some(PREMODEL), None));
                }
                Err(mis_at("ccskp.legacy.ref_unread", self.r.pos))
            }
            Some(e) if e.class => Err(mis_at("ccskp.legacy.ref_class_record", self.r.pos)),
            Some(e) => Ok((s, Some(e.name), e.val)),
        }
    }

    /// Is there a CLayer record at `at` (new class record, or a class-ref to the known CLayer class)?
    fn layer_record_at(&self, at: usize, any_class_ref: bool) -> bool {
        let t = u16at(self.b, at);
        if t == 0xffff && u16at(self.b, at.saturating_add(4)) == 6 && ascii_at(self.b, at.saturating_add(6), "CLayer") {
            return true;
        }
        if let Some(&cs) = self.class_slot.get(&C_LAYER)
            && is_class_ref(self.b, at, cs)
        {
            return true;
        }
        any_class_ref && t != -1 && (t & 0x8000) != 0 && t != 0xffff
    }

    /// Layer records past the declared count (seen in SketchUp 2018/2020 files), with null separators.
    fn trailing_layers(&mut self, out: &mut Vec<(Option<u32>, ValId)>, any_class_ref: bool) -> LRes<()> {
        while self.r.pos.saturating_add(2) <= self.r.end {
            if u16at(self.b, self.r.pos) == 0 {
                self.r.pos = self.r.pos.saturating_add(2);
                continue;
            }
            if !self.layer_record_at(self.r.pos, any_class_ref) {
                break;
            }
            let (s, _, v) = self.obj(Some(C_LAYER))?;
            if let Some(v) = v {
                out.push((s, v));
            }
        }
        Ok(())
    }

    /// Is the entry at `slot` a class record (`slots.get(slot)?.[0] === 'class'`)?
    fn is_class_slot(&self, slot: u32) -> bool {
        self.get(slot).is_some_and(|e| e.class)
    }

    // ── shared record blocks ───────────────────────────────────────────────────────────────────────────

    /// CString: FF FE FF, length u8 (0xFF → u16, 0xFFFF → u32) in UTF-16 code units, then the text.
    fn str(&mut self) -> LRes<String> {
        str_read(&mut self.r)
    }

    #[inline]
    fn peek16(&self) -> LRes<i64> {
        self.r.need(2)?;
        Ok(u16at(self.b, self.r.pos))
    }

    fn f64s<const N: usize>(&mut self) -> LRes<[f64; N]> {
        let mut a = [0.0; N];
        for v in &mut a {
            *v = self.r.f64()?;
        }
        Ok(a)
    }

    /// Entity preamble: attribute-container pointer, then (2017+) a persistent-id mask + its bytes.
    fn preamble(&mut self) -> LRes<(Option<u32>, Option<ValId>)> {
        let (attr_slot, _, attrs) = self.obj(Some(C_ATTR_CONTAINER))?;
        if self.lay.pid {
            let mask = self.r.u8()?;
            for bit in 0..8 {
                if mask & (1 << bit) != 0 {
                    self.r.u8()?;
                }
            }
        }
        Ok((attr_slot, attrs))
    }

    /// Vertex, loop, edge use, material and layer have no attribute pointer in SketchUp 3.
    fn early_preamble(&mut self) -> LRes<()> {
        if !self.lay.v3 {
            self.preamble()?;
        }
        Ok(())
    }

    /// Drawing-element block: 8 bytes (material u16, hidden, ?, ?, soft, smooth, ?) then the layer. SketchUp 3 has
    /// 5 bytes we can't decode. The layer is a reference to a CLayer: a plain index, or (seen on 2018 instances) a
    /// whole CLayer record on first use / an escaped back-ref.
    fn drawbase(&mut self) -> LRes<Db> {
        let blk = self.r.bytes(if self.lay.v3 { 5 } else { 8 })?;
        let mut layer = None;
        let t = self.peek16()?;
        if t == 0x7fff || (t & 0x8000) != 0 {
            let (s, n, _) = self.obj(Some(C_LAYER))?;
            if n == Some(C_LAYER) {
                layer = s;
            }
        } else {
            self.r.pos = self.r.pos.saturating_add(2);
            let s = if t != 0 && !self.burns.is_empty() { self.translate(t) } else { Some(t) };
            if let Some(s) = s.filter(|&s| s != 0).and_then(|s| u32::try_from(s).ok())
                && self.get(s).is_some_and(|e| e.name == C_LAYER)
            {
                layer = Some(s);
            }
        }
        if self.lay.v3 {
            return Ok(Db { mat: 0, hidden: false, soft: false, smooth: false, layer });
        }
        let at = |i: usize| blk.get(i).copied().unwrap_or(0);
        Ok(Db { mat: u32::from(at(0)) | (u32::from(at(1)) << 8), hidden: at(2) != 0, soft: at(5) != 0, smooth: at(6) != 0, layer })
    }

    // ── entity readers ─────────────────────────────────────────────────────────────────────────────────

    fn read_vertex(&mut self) -> LRes<Val> {
        self.early_preamble()?;
        self.vertices = self.vertices.saturating_add(1);
        if self.vertices > self.limits.vertices {
            return Err(err("too_big", "ccskp.too_big.vertices", &[("max", &self.limits.vertices.to_string())]).into());
        }
        let p = self.f64s::<3>()?;
        if !p.iter().all(|&x| ok_num(x)) {
            return Err(mis_at("ccskp.legacy.vertex_range", self.r.pos));
        }
        Ok(Val::Vertex(p))
    }

    fn read_edge(&mut self) -> LRes<Val> {
        self.preamble()?;
        let db = self.drawbase()?;
        let (v1, _, _) = self.obj(Some(C_VERTEX))?;
        let (v2, _, _) = self.obj(Some(C_VERTEX))?;
        let (_, cn, _) = self.obj(None)?;
        if let Some(cn) = cn
            && cn != C_CURVE
            && cn != C_ARC_CURVE
        {
            let t = self.name_text(cn).to_string();
            return Err(mis("ccskp.legacy.edge_curve", &[("type", &t), ("at", &self.r.pos.to_string())]));
        }
        Ok(Val::Edge { db, v1, v2 })
    }

    fn read_edge_use(&mut self) -> LRes<Val> {
        self.early_preamble()?;
        let (edge, _, _) = self.obj(Some(C_EDGE))?;
        let sense = self.r.u8()?;
        // The parent-loop back-ref keeps us honest: it must name the loop being read. When it runs AHEAD of our
        // numbering right after an annotation, the writer burned indexes (see Archive.burns) — record the band.
        let p0 = self.r.pos;
        let tag = self.r.u16()?;
        let parent: Option<i64> = if tag == 0x7fff {
            let big = self.r.u32()?;
            if big & 0x8000_0000 != 0 {
                return Err(mis_at("ccskp.legacy.edgeuse_parent", self.r.pos));
            }
            Some(i64::from(big))
        } else if tag == 0xffff || (tag & 0x8000) != 0 {
            return Err(mis_at("ccskp.legacy.edgeuse_parent", self.r.pos));
        } else if tag == 0 {
            None
        } else {
            Some(i64::from(tag))
        };
        let want = self.current_loop.map(|l| i64::from(l).saturating_add(self.burn_total));
        if parent != want {
            let delta = match (parent, want) {
                (Some(p), Some(w)) => p.saturating_sub(w),
                _ => 0,
            };
            match self.annot_mark {
                Some(mark) if delta > 0 && delta <= 4096 => {
                    self.burns.push((i64::from(mark).saturating_add(self.burn_total), delta));
                    self.burn_total = self.burn_total.saturating_add(delta);
                    self.annot_mark = None;
                    if let Some(c) = self.burn_credits.last_mut() {
                        *c = c.saturating_add(1);
                    }
                }
                _ => {
                    self.r.pos = p0;
                    return Err(mis_at("ccskp.legacy.edgeuse_loop", self.r.pos));
                }
            }
        }
        Ok(Val::Use { edge, sense })
    }

    fn read_loop(&mut self) -> LRes<Val> {
        let prev = self.current_loop;
        self.current_loop = self.current_slot;
        // (on a failure the JS leaves currentLoop pointing at this loop; so do we)
        self.early_preamble()?;
        self.r.skip(2)?;
        let mut uses = Vec::new();
        while self.peek16()? != 0 {
            uses.push(self.obj(Some(C_EDGE_USE))?.2);
        }
        self.r.pos = self.r.pos.saturating_add(2);
        self.current_loop = prev;
        Ok(Val::Loop(uses))
    }

    fn read_face(&mut self) -> LRes<Val> {
        let (attr_slot, mut attrs) = self.preamble()?;
        let mut db = self.drawbase()?;
        if self.lay.v3 && matches!(attrs.and_then(|a| self.val(a)), Some(Val::Material(_))) {
            // SketchUp 3: the opening pointer is the front material
            db.mat = attr_slot.unwrap_or(0);
            attrs = None;
        }
        let plane = self.f64s::<4>()?;
        let n = self.r.u32()?;
        if n > 10000 {
            return Err(mis_at("ccskp.legacy.loop_count", self.r.pos));
        }
        let mut loops = Vec::with_capacity(n as usize);
        for _ in 0..n {
            loops.push(self.obj(Some(C_LOOP))?.2);
        }
        let back = self.r.u16()?;
        let mut ftc = None;
        if let Some(Val::Attrs(children)) = attrs.and_then(|a| self.val(a)) {
            for c in children.iter().flatten() {
                if matches!(self.val(*c), Some(Val::Ftc(_))) {
                    ftc = Some(*c);
                }
            }
        }
        Ok(Val::Face(Box::new(FaceV { db, plane, loops, back, ftc })))
    }

    fn read_attr_container(&mut self) -> LRes<Val> {
        self.preamble()?;
        let mut children = Vec::new();
        while self.peek16()? != 0 {
            let (_, _, v) = self.obj(Some(C_ATTR_NAMED))?;
            children.push(v);
        }
        self.r.pos = self.r.pos.saturating_add(2);
        Ok(Val::Attrs(children))
    }

    fn read_attr_named(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.r.skip(4)?;
        let name = self.str()?;
        loop {
            let key = self.str()?;
            if key.is_empty() {
                break;
            }
            typed_value(&mut self.r, 0)?;
        }
        if self.lay.attr_tail {
            self.r.u32()?;
        }
        Ok(Val::Dict(name))
    }

    fn read_layer(&mut self) -> LRes<Val> {
        self.early_preamble()?;
        let name = self.str()?;
        let mut mid: Vec<u8> = Vec::new();
        while mid.len() < 8 && !str_at(self.b, self.r.pos) {
            mid.push(self.r.u8()?); // flag bytes (3–5); the first is "hidden"
        }
        self.str()?; // internal name "Layer_<name>"
        let flags = self.r.u16()?;
        let visible = mid.first().is_none_or(|&h| h == 0);
        if flags & 0xff != 0 {
            // colour by layer with a TEXTURED material: the layer carries a whole texture block
            let tex = self.texture_block()?;
            self.r.skip(4)?;
            return Ok(Val::Layer(Box::new((name, visible, tex.rgba))));
        }
        let rgba = rgba4(self.r.bytes(4)?);
        self.str()?;
        self.r.skip(if self.lay.v3 { 17 } else { 21 })?;
        // v18 zero-material files: 12 zero bytes + u32 before another CLayer
        let p = self.r.pos;
        if p.saturating_add(18) <= self.r.end
            && self.b.get(p..p.saturating_add(12)).is_some_and(|z| z.iter().all(|&x| x == 0))
            && self.layer_record_at(p.saturating_add(16), true)
        {
            self.r.pos = p.saturating_add(16);
        }
        // v18 template Layer0 after a custom tag: <parent u16><active u16> when both name CLayer objects
        let p = self.r.pos;
        let (a, b) = (u16at(self.b, p), u16at(self.b, p.saturating_add(2)));
        let cur = self.current_slot.map(i64::from);
        if a > 0 && b > 0 && (a & 0x8000) == 0 && (b & 0x8000) == 0 && a != 0x7fff && b != 0x7fff && Some(a) != cur {
            let is_layer_obj = |x: i64| u32::try_from(x).ok().and_then(|s| self.get(s)).is_some_and(|e| !e.class && e.name == C_LAYER);
            if is_layer_obj(a) && is_layer_obj(b) {
                self.r.pos = p.saturating_add(2);
            }
        }
        Ok(Val::Layer(Box::new((name, visible, rgba))))
    }

    /// A material's texture: embedded image (CDib) + tile size in inches + source name + average colour + opacity.
    /// From SketchUp 4 on it is an entity of its own (preamble, then a CDib object); in SketchUp 3 the image follows
    /// directly. The caller has read the "has texture" flag as a u16, whose high byte is the preamble's first byte.
    fn texture_block(&mut self) -> LRes<TexV> {
        let image = if self.lay.v3 {
            match self.read_dib()? {
                Val::Dib { start, len, .. } => (start, len),
                _ => (0, 0),
            }
        } else {
            self.r.pos = self.r.pos.saturating_sub(1);
            self.preamble()?;
            let (_, _, v) = self.obj(Some(C_DIB))?;
            match v.and_then(|v| self.val(v)) {
                Some(Val::Dib { start, len, .. }) => (*start, *len),
                _ => return Err(mis_at("ccskp.legacy.texture_image", self.r.pos)),
            }
        };
        let p = self.r.pos;
        let m = find(self.b, &STR, p, p.saturating_add(28));
        match m.map(|m| m.saturating_sub(p)) {
            Some(20) => {
                self.r.u32()?;
            }
            Some(16) => {}
            _ => return Err(mis_at("ccskp.legacy.texture_size", self.r.pos)),
        }
        let w = self.r.f64()?;
        let h = self.r.f64()?;
        let file = self.str()?;
        let avg = self.r.bytes(9)?;
        self.str()?;
        self.r.skip(8)?;
        let opacity = self.r.f64()?;
        let use_opacity = self.r.u8()?;
        Ok(TexV { rgba: rgba4(avg), opacity, use_opacity, image, w, h, file })
    }

    fn read_material(&mut self) -> LRes<Val> {
        self.early_preamble()?;
        let name = self.str()?;
        let tex_flag = self.r.u16()?;
        if tex_flag == 0 {
            let rgba = rgba4(self.r.bytes(4)?);
            self.str()?;
            self.r.skip(8)?;
            let opacity = self.r.f64()?;
            let use_opacity = self.r.u8()?;
            return Ok(Val::Material(Box::new(MaterialV { name, rgba, opacity, use_opacity, tex: None })));
        }
        let tex = self.texture_block()?;
        Ok(Val::Material(Box::new(MaterialV { name, rgba: tex.rgba, opacity: tex.opacity, use_opacity: tex.use_opacity, tex: Some(tex) })))
    }

    /// CDib: subtype u32, length u32, the image bytes. A JPEG (subtype 1) is followed by a u32 quality (70, 75 …).
    /// OpenSKP absorbs that u32 only inside material textures; in a component's thumbnail nothing did, and every
    /// list after a JPEG thumbnail went out of step (found by the JS authors on the SketchUp 6 PCB file).
    fn read_dib(&mut self) -> LRes<Val> {
        let subtype = self.r.u32()?;
        let len = self.r.u32()? as usize;
        if len > self.r.left() {
            return Err(mis_at("ccskp.legacy.image_long", self.r.pos));
        }
        let start = self.r.pos;
        self.r.skip(len)?;
        if subtype == 1 && len > 0 {
            self.r.u32()?;
        }
        Ok(Val::Dib { start, len })
    }

    /// Texture positioning: a 3×3 (front) and 3×3 (back) mapping inside 24 doubles, pins, per-side flags.
    fn read_ftc(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.r.u32()?;
        let ks = self.f64s::<24>()?;
        for _ in 0..2 {
            let n = self.r.u32()?;
            if n > 64 {
                return Err(mis_at("ccskp.legacy.texture_pins", self.r.pos));
            }
            self.r.skip((n as usize).saturating_mul(32))?;
        }
        let ff = self.r.u32()?;
        self.r.u32()?;
        let mut front = [0.0; 9];
        front.copy_from_slice(ks.get(..9).unwrap_or(&[0.0; 9]));
        Ok(Val::Ftc(Box::new((front, (ff & 2) != 0))))
    }

    /// An Image entity: placed like an instance of the definition holding its textured face.
    fn read_image(&mut self) -> LRes<Val> {
        self.preamble()?;
        let db = self.drawbase()?;
        let (def, _, _) = self.obj(None)?;
        let xf12 = self.f64s::<12>()?;
        self.r.f64()?;
        self.str()?;
        self.r.skip(16)?;
        let mut xf = [1.0; 13];
        xf.get_mut(..12).unwrap_or(&mut []).copy_from_slice(&xf12);
        Ok(Val::Inst(Box::new(InstV { cls: C_IMAGE, db, def, xf, name: String::new() })))
    }

    /// A reference to an entity that may point FORWARD (annotations are written before what they label).
    fn entity_ref(&mut self) -> LRes<Option<u32>> {
        let tag = self.r.u16()?;
        if tag == 0 {
            return Ok(None);
        }
        if tag == 0x7fff {
            let big = self.r.u32()?;
            if big & 0x8000_0000 != 0 {
                return Err(mis_at("ccskp.legacy.entity_ref", self.r.pos));
            }
            return Ok(Some(big));
        }
        if tag == 0xffff || (tag & 0x8000) != 0 {
            return Err(mis_at("ccskp.legacy.entity_ref", self.r.pos));
        }
        Ok(Some(u32::from(tag)))
    }

    /// A dimension connection's instance paths: [ref][u32 n1][n1 refs][u32 n2][n2 refs]. A path entry is usually
    /// a reference, but MFC writes an object in full the first time anything points at it (a whole group, even).
    fn connection_paths(&mut self) -> LRes<()> {
        self.entity_ref()?;
        for _ in 0..2 {
            let n = self.r.u32()?;
            if n > 1000 {
                return Err(mis_at("ccskp.legacy.dim_path", self.r.pos));
            }
            for _ in 0..n {
                let t = self.peek16()?;
                let mut is_new = t == 0xffff || (t != 0x7fff && (t & 0x8000) != 0);
                if t == 0x7fff {
                    let big = u32at(self.b, self.r.pos.saturating_add(2));
                    is_new = big == -1 || (big & 0x8000_0000) != 0;
                }
                if is_new {
                    self.obj(None)?;
                } else {
                    self.entity_ref()?;
                }
            }
        }
        Ok(())
    }

    fn read_construction_line(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.r.skip(8 * 8)?;
        // The tail length depends on the build that wrote the file (0, 4 or 7 bytes): calibrate on the first one —
        // the length that lands on an unambiguous next tag — and keep it.
        if self.cline_tail.is_none() {
            let dflt = if self.lay.major == 17.0 { 7 } else { 4 };
            let mut order = vec![dflt];
            order.extend([0usize, 4, 7].into_iter().filter(|&c| c != dflt));
            let mut k = None;
            for allow_null in [false, true] {
                for &c in &order {
                    if strict_tag(self, self.r.pos.saturating_add(c), allow_null) {
                        k = Some(c);
                        break;
                    }
                }
                if k.is_some() {
                    break;
                }
            }
            self.cline_tail = Some(k.unwrap_or(dflt));
        }
        self.r.skip(self.cline_tail.unwrap_or(4))?;
        self.skipped.guides = self.skipped.guides.saturating_add(1);
        Ok(Val::Guide)
    }

    fn read_section_plane(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.r.need(8)?;
        let first = {
            let mut a = [0u8; 8];
            a.copy_from_slice(self.b.get(self.r.pos..self.r.pos.saturating_add(8)).unwrap_or(&[0; 8]));
            f64::from_le_bytes(a)
        };
        if first.abs().partial_cmp(&1.0001).is_none_or(|o| o == std::cmp::Ordering::Greater) {
            self.obj(None)?; // optional object pointer before the plane
        }
        self.r.skip(32)?;
        if str_at(self.b, self.r.pos) {
            self.str()?;
            self.str()?;
        }
        self.skipped.sections = self.skipped.sections.saturating_add(1);
        Ok(Val::Section)
    }

    fn read_dim_linear(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.str()?;
        self.obj(Some(C_SKFONT))?;
        self.r.skip(37)?;
        self.entity_ref()?;
        self.connection_paths()?;
        self.r.skip(32)?;
        self.entity_ref()?;
        self.connection_paths()?;
        self.r.skip(72)?;
        self.skipped.dims = self.skipped.dims.saturating_add(1);
        Ok(Val::Dim)
    }

    fn read_text(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.obj(Some(C_SKFONT))?;
        // variable middle, closed by `01 00 00 00 ?? 00 03 00 00 00 01` right before the text record
        let b = self.b;
        let pos = self.r.pos;
        let mut p = pos;
        let at = loop {
            let Some(at) = find(b, &STR, p, pos.saturating_add(512)) else {
                return Err(mis_at("ccskp.legacy.text_label", self.r.pos));
            };
            if let Some(k) = at.checked_sub(11)
                && k >= pos
                && u32at(b, k) == 1
                && b.get(k.saturating_add(5)) == Some(&0)
                && u32at(b, k.saturating_add(6)) == 3
                && b.get(k.saturating_add(10)) == Some(&1)
            {
                break at;
            }
            p = at.saturating_add(3);
        };
        self.r.pos = at;
        self.str()?;
        self.r.skip(5)?;
        // optional leader attachments in the escaped 6-byte form only (a 2-byte one is indistinguishable from the next tag)
        while u16at(b, self.r.pos) == 0x7fff
            && self.r.pos.saturating_add(6) <= self.r.end
            && (u32at(b, self.r.pos.saturating_add(2)) & 0x8000_0000) == 0
        {
            self.r.pos = self.r.pos.saturating_add(6);
        }
        self.skipped.text = self.skipped.text.saturating_add(1);
        Ok(Val::Text)
    }

    fn read_entity_list(&mut self, count: u64, owner_def: bool, owner_root: bool) -> LRes<Vec<Ent>> {
        self.burn_credits.push(0);
        let out = self.read_entity_list_body(count, owner_def, owner_root);
        self.burn_credits.pop();
        out
    }

    fn read_entity_list_body(&mut self, count: u64, owner_def: bool, owner_root: bool) -> LRes<Vec<Ent>> {
        let mut ents: Vec<Ent> = Vec::new();
        while (ents.len() as u64) < count {
            let p = self.r.pos;
            let credit = owner_def && self.burn_credits.last().is_some_and(|&c| c > 0);
            // burned indexes also count as phantom list items: the definition tail (nrel = 0, padding, GUID, name
            // record at +22) marks the list's real end
            if credit && u32at(self.b, p) == 0 && str_at(self.b, p.saturating_add(22)) {
                break;
            }
            match self.obj(None) {
                Ok(e) => ents.push(e),
                Err(LErr::Mis(m)) => {
                    if owner_root || credit {
                        self.r.pos = p;
                        if owner_root {
                            self.root_cut = usize::try_from(count.saturating_sub(ents.len() as u64)).unwrap_or(usize::MAX);
                        }
                        break;
                    }
                    return Err(LErr::Mis(m));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(ents)
    }

    fn read_definition(&mut self) -> LRes<Val> {
        self.preamble()?;
        self.r.skip(if self.lay.pid {
            22
        } else if self.lay.v3 {
            13
        } else {
            20
        })?; // undecoded block
        let nl = self.r.u32()?;
        if nl > 10000 {
            return Err(mis_at("ccskp.legacy.def_layers", self.r.pos));
        }
        let mut got = 0u32;
        while got < nl {
            if self.peek16()? == 0 {
                self.r.pos = self.r.pos.saturating_add(2); // SketchUp 2020 puts null separators between layers
                continue;
            }
            self.obj(Some(C_LAYER))?;
            got = got.saturating_add(1);
        }
        if self.r.u16()? == 0x7fff {
            self.r.u32()?;
        }
        let mut count: Option<u64> = None;
        if self.lay.major >= 20.0 {
            count = retry_after_filler(self, self.r.pos, 5_000_000);
        }
        let mut count = match count {
            Some(c) => c,
            None => {
                self.r.u32()?;
                u64::from(self.r.u32()?)
            }
        };
        if (count > 5_000_000 || count == 0)
            && let Some(n) = retry_after_filler(self, self.r.pos.saturating_sub(4), 5_000_000)
        {
            count = n;
        }
        if count > 5_000_000 {
            return Err(mis_at("ccskp.legacy.def_entities", self.r.pos));
        }
        let ents = self.read_entity_list(count, true, false)?;
        let mut nrel = u64::from(self.r.u32()?);
        if nrel > 100_000
            && let Some(n) = retry_after_filler(self, self.r.pos.saturating_sub(4), 100_000)
        {
            nrel = n;
        }
        if nrel > 100_000 {
            return Err(mis_at("ccskp.legacy.def_misaligned", self.r.pos));
        }
        for _ in 0..nrel {
            self.obj(Some(C_RELATIONSHIP))?;
        }
        self.r.u16()?;
        // the 16-byte GUID is followed by the name record; SketchUp 2020 can put 2 more bytes before the GUID
        if !str_at(self.b, self.r.pos.saturating_add(16)) {
            for s in 1..=4usize {
                if str_at(self.b, self.r.pos.saturating_add(s).saturating_add(16)) {
                    self.r.pos = self.r.pos.saturating_add(s);
                    break;
                }
            }
        }
        let guid = hex(self.r.bytes(16)?);
        let name = self.str()?;
        self.str()?;
        self.str()?;
        self.r.u32()?;
        // an undecoded block (~39–47 bytes), then this definition's CThumbnail
        let ts = self.class_slot.get(&C_THUMBNAIL).copied();
        let mut tpos = None;
        for off in 0..96usize {
            let p = self.r.pos.saturating_add(off);
            if (u16at(self.b, p) == 0xffff
                && u16at(self.b, p.saturating_add(4)) == 10
                && ascii_at(self.b, p.saturating_add(6), "CThumbnail"))
                || ts.is_some_and(|ts| is_class_ref(self.b, p, ts))
            {
                tpos = Some(p);
                break;
            }
        }
        let Some(tpos) = tpos else { return Err(mis_at("ccskp.legacy.def_end", self.r.pos)) };
        self.r.pos = tpos;
        self.obj(Some(C_THUMBNAIL))?;
        Ok(Val::Def(Box::new(DefV { name, guid, ents })))
    }

    fn read_instance(&mut self) -> LRes<Val> {
        self.preamble()?;
        let db = self.drawbase()?;
        let (def, dn, _) = self.obj(Some(C_DEFINITION))?;
        if dn != Some(C_DEFINITION) {
            return Err(mis_at("ccskp.legacy.inst_not_def", self.r.pos));
        }
        let xf = self.f64s::<13>()?;
        let name = if !self.lay.v3 || str_at(self.b, self.r.pos) { self.str()? } else { String::new() }; // SketchUp 3 instances have no name
        if self.lay.inst_guid {
            self.r.skip(16)?;
        }
        let cls = self.current_slot.and_then(|s| self.get(s)).map_or(C_INSTANCE, |e| e.name);
        Ok(Val::Inst(Box::new(InstV { cls, db, def, xf, name })))
    }

    // ── building ───────────────────────────────────────────────────────────────────────────────────────

    /// Build each definition as soon as its record completes, then release its entity objects.
    fn on_definition(&mut self, slot: u32, id: ValId) -> LRes<()> {
        let (name, guid, ents) = match self.vals.get_mut(id as usize) {
            Some(Val::Def(d)) => (d.name.clone(), d.guid.clone(), std::mem::take(&mut d.ents)),
            _ => return Ok(()),
        };
        let mut g = self.build_entities(&ents)?;
        g.name = name;
        g.guid = guid;
        self.built.push((slot, g));
        // Definitions' entity objects are no longer needed once the definition is built: forget them in the store
        // map (later back-refs still type-check by class name; the JS swaps in a shared placeholder).
        let mut k = slot.saturating_add(1);
        while k < self.next {
            if let Some(e) = self.dense.get_mut(k.saturating_sub(self.base) as usize)
                && !e.class
                && e.val.is_some()
                && matches!(e.name, C_VERTEX | C_EDGE | C_EDGE_USE | C_LOOP | C_FACE)
            {
                e.val = None;
            }
            k = k.saturating_add(1);
        }
        Ok(())
    }

    fn point(&self, s: Option<u32>) -> Option<(u32, P3)> {
        match self.slot_val(s) {
            Some(Val::Vertex(p)) => Some((s.unwrap_or(0), *p)),
            _ => None,
        }
    }

    fn build_entities(&mut self, ents: &[Ent]) -> LRes<Built> {
        let mut out = Built::default();
        let mut edges: Vec<(u32, ValId)> = Vec::new();
        let mut edge_ix: FxMap<u32, usize> = FxMap::default();
        let mut used: FxSet<u32> = FxSet::default();
        let add_edge = |edges: &mut Vec<(u32, ValId)>, edge_ix: &mut FxMap<u32, usize>, s: u32, e: ValId| {
            if let std::collections::hash_map::Entry::Vacant(v) = edge_ix.entry(s) {
                v.insert(edges.len());
                edges.push((s, e));
            }
        };
        for &(s, _, v) in ents {
            let Some(v) = v else { continue };
            match self.val(v) {
                Some(Val::Edge { .. }) => {
                    if let Some(s) = s {
                        add_edge(&mut edges, &mut edge_ix, s, v);
                    }
                }
                Some(Val::Inst(_)) => out.insts.push(v),
                Some(Val::Face(f)) => {
                    let mut loops: Vec<Vec<P3>> = Vec::new();
                    for lp in &f.loops {
                        let Some(lp) = *lp else { continue };
                        let Some(Val::Loop(uses)) = self.val(lp) else {
                            return Err(LErr::Internal("lp.uses is not iterable".to_string()));
                        };
                        let mut pts: Vec<P3> = Vec::with_capacity(uses.len());
                        let (mut first, mut last_slot): (Option<u32>, Option<u32>) = (None, None);
                        let mut prev: Option<Option<u32>> = Some(None); // the JS starts with prev = null
                        for u in uses {
                            let Some(Val::Use { edge, sense }) = u.and_then(|u| self.val(u)) else { continue };
                            let Some(eid) = edge.and_then(|e| self.get(e)).and_then(|e| e.val) else { continue };
                            let Some(Val::Edge { v1, v2, .. }) = self.val(eid) else { continue };
                            if let Some(es) = *edge {
                                add_edge(&mut edges, &mut edge_ix, es, eid);
                                used.insert(es);
                            }
                            let vs = if *sense != 0 { *v2 } else { *v1 }; // the use runs v1 → v2, or reversed
                            if Some(vs) != prev
                                && let Some((ps, p)) = self.point(vs)
                            {
                                pts.push(p);
                                if first.is_none() {
                                    first = Some(ps);
                                }
                                last_slot = Some(ps);
                                prev = Some(vs);
                            }
                        }
                        if pts.len() > 1 && first == last_slot {
                            pts.pop();
                        }
                        if pts.len() >= 3 {
                            loops.push(pts);
                        }
                    }
                    if loops.is_empty() {
                        self.bad_faces = self.bad_faces.saturating_add(1);
                        continue;
                    }
                    out.faces.push(BFace {
                        loops,
                        plane: f.plane,
                        front: (f.db.mat != 0).then_some(f.db.mat),
                        back: (f.back != 0).then_some(f.back),
                        layer: f.db.layer,
                        hidden: f.db.hidden,
                        ftc: f.ftc,
                    });
                }
                _ => {}
            }
        }
        for (s, e) in edges {
            let Some(Val::Edge { db, v1, v2 }) = self.val(e) else { continue };
            let (Some((_, a)), Some((_, b))) = (self.point(*v1), self.point(*v2)) else { continue };
            if !used.contains(&s) {
                out.edges.push(BEdge { a, b, layer: db.layer, soft: db.soft, smooth: db.smooth, hidden: db.hidden });
            } else if db.soft || db.smooth {
                out.soft.push([a, b]);
            }
        }
        Ok(out)
    }
}

/// Class-ref to `slot` at p, short (0x8000|slot) or escaped (0x7FFF + 0x80000000|slot).
fn is_class_ref(b: &[u8], p: usize, slot: u32) -> bool {
    if slot <= 0x7fff {
        return u16at(b, p) == i64::from(0x8000 | slot);
    }
    u16at(b, p) == 0x7fff && u32at(b, p.saturating_add(2)) == i64::from(0x8000_0000 | slot)
}

/// Can the u16 at `at` start an object read without doubt (null, escape, class record, known class-ref)?
fn strict_tag(ar: &Archive<'_>, at: usize, allow_null: bool) -> bool {
    let t = u16at(ar.b, at);
    if t == -1 {
        return false;
    }
    if t == 0 {
        return allow_null;
    }
    if t == 0x7fff || t == 0xffff {
        return true;
    }
    if t & 0x8000 != 0 {
        return u32::try_from(t & 0x7fff).is_ok_and(|s| ar.is_class_slot(s));
    }
    false
}

/// Like strictTag, but a plain back-ref inside the walked range also counts.
fn plausible_tag(ar: &Archive<'_>, at: usize) -> bool {
    let t = u16at(ar.b, at);
    if t == -1 {
        return false;
    }
    if t == 0 || t == 0x7fff || t == 0xffff {
        return true;
    }
    if t & 0x8000 != 0 {
        return u32::try_from(t & 0x7fff).is_ok_and(|s| ar.is_class_slot(s));
    }
    t < i64::from(ar.next)
}

/// SketchUp 2020 sometimes writes an empty text record + zero padding ahead of a count, which leaves us a few
/// bytes early. Given where the bad count was read, find the real one: past the marker, at +4+pad with
/// pad ≡ 1 (mod 4), the first plausible non-zero u32. Returns the count (cursor moved) or None.
fn retry_after_filler(ar: &mut Archive<'_>, count_pos: usize, limit: u64) -> Option<u64> {
    let b = ar.b;
    let mut m = None;
    let mut i = count_pos;
    while i < count_pos.saturating_add(12) && i.saturating_add(4) <= b.len() {
        if str_at(b, i) {
            m = Some(i);
            break;
        }
        i = i.saturating_add(1);
    }
    let m = m?;
    if b.get(m.saturating_add(3)) != Some(&0) {
        return None;
    }
    let mut pad = 1usize;
    while pad <= 29 {
        let at = m.saturating_add(4).saturating_add(pad);
        let n = u32at(b, at);
        if n == -1 {
            break;
        }
        let n = u64::try_from(n).unwrap_or(0);
        if n > 0 && n <= limit && plausible_tag(ar, at.saturating_add(4)) {
            ar.r.pos = at.saturating_add(4);
            return Some(n);
        }
        pad = pad.saturating_add(4);
    }
    None
}

fn rgba4(b: &[u8]) -> [u8; 4] {
    let mut a = [0u8; 4];
    for (x, y) in a.iter_mut().zip(b) {
        *x = *y;
    }
    a
}

/// CString record at r.pos.
fn str_read(r: &mut Reader<'_>) -> LRes<String> {
    if !str_at(r.b, r.pos) {
        return Err(mis_at("ccskp.legacy.text_record", r.pos));
    }
    r.pos = r.pos.saturating_add(3);
    let mut n = u32::from(r.u8()?);
    if n == 0xff {
        n = u32::from(r.u16()?);
        if n == 0xffff {
            n = r.u32()?;
        }
    }
    if (n as usize).saturating_mul(2) > r.left() {
        return Err(mis_at("ccskp.legacy.text_long", r.pos));
    }
    Ok(r.utf16(n as usize)?)
}

/// A typed attribute / option value (only what readUnits needs is kept).
#[derive(Debug, Clone, PartialEq)]
enum Tv {
    Null,
    Num(f64),
    Str(String),
    Arr(Vec<Tv>),
}

fn typed_value(r: &mut Reader<'_>, depth: u32) -> LRes<Tv> {
    let t = r.u8()?;
    Ok(match t {
        0x00 => Tv::Null,
        0x04 => Tv::Num(f64::from(r.i32()?)),
        0x06 | 0x0c => Tv::Num(r.f64()?),
        0x07 => Tv::Num(f64::from(r.u8()?)),
        0x09 => Tv::Num(f64::from(r.u32()?)),
        0x0a => Tv::Str(str_read(r)?),
        0x11 | 0x12 => {
            let a = [r.f64()?, r.f64()?, r.f64()?];
            Tv::Arr(a.iter().map(|&x| Tv::Num(x)).collect())
        }
        0x0b => {
            let n = r.u32()?;
            if n > 100_000 || depth > 8 {
                return Err(mis_at("ccskp.legacy.attr_array", r.pos));
            }
            let mut a = Vec::new();
            for _ in 0..n {
                a.push(typed_value(r, depth.saturating_add(1))?);
            }
            Tv::Arr(a)
        }
        _ => return Err(mis("ccskp.legacy.attr_type", &[("type", &t.to_string()), ("at", &r.pos.to_string())])),
    })
}

/// `['in', 'ft', 'mm', 'cm', 'm'][v]`: v used as a property key (a number, or its String() form).
fn unit_index(v: &Tv) -> Option<&'static str> {
    const U: [&str; 5] = ["in", "ft", "mm", "cm", "m"];
    let key = match v {
        Tv::Num(x) => js_num_text(*x),
        Tv::Str(s) => s.clone(),
        Tv::Arr(a) if a.len() == 1 => match a.first()? {
            Tv::Num(x) => js_num_text(*x),
            Tv::Str(s) => s.clone(),
            _ => return None,
        },
        _ => return None,
    };
    match key.as_str() {
        "0" => U.first().copied(),
        "1" => U.get(1).copied(),
        "2" => U.get(2).copied(),
        "3" => U.get(3).copied(),
        "4" => U.get(4).copied(),
        _ => None,
    }
}

// ── header, version map, document start ──────────────────────────────────────────────────────────────

/// "{6.0.1}" from the second header string, when read.rs did not hand us a usable version.
fn header_version(b: &[u8]) -> Option<String> {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len().min(0x80) {
        let c = u16::from(b.get(i).copied().unwrap_or(0)) | (u16::from(b.get(i.saturating_add(1)).copied().unwrap_or(0)) << 8);
        s.push(if (32..127).contains(&c) { char::from(u8::try_from(c).unwrap_or(b' ')) } else { ' ' });
        i = i.saturating_add(2);
    }
    // /\{(\d+)\.([\d.]*)\}/
    let sb = s.as_bytes();
    for (k, _) in s.match_indices('{') {
        let d = sb.get(k.saturating_add(1)..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
        let dot = k.saturating_add(1).saturating_add(d);
        if d == 0 || sb.get(dot) != Some(&b'.') {
            continue;
        }
        let rest = sb.get(dot.saturating_add(1)..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count());
        let close = dot.saturating_add(1).saturating_add(rest);
        // greedy [\d.]* then '}': backtracking cannot help ('}' is not in the class)
        if sb.get(close) == Some(&b'}') {
            return Some(format!("{}.{}", s.get(k.saturating_add(1)..dot)?, s.get(dot.saturating_add(1)..close)?));
        }
    }
    None
}

struct VersionMap {
    schemas: HashMap<String, u32>,
    end: usize,
}

/// CVersionMap: FF FF 00 00 0B 00 "CVersionMap", then (name text, u32 schema) pairs up to End-Of-Version-Map.
fn read_version_map(b: &[u8], from: Option<usize>) -> LRes<Option<VersionMap>> {
    let pat = class_record("CVersionMap");
    let mut at = from.and_then(|f| find(b, &pat, f, f.saturating_add(1 << 16)));
    if at.is_none() {
        at = find(b, &pat, 0, b.len().min(1 << 16));
    }
    let Some(at) = at else { return Ok(None) };
    let mut r = Reader::new(b, at.saturating_add(6).saturating_add(11));
    let mut schemas = HashMap::new();
    for _ in 0..500 {
        let name = str_read(&mut r)?;
        let v = r.u32()?;
        if name == "End-Of-Version-Map" {
            return Ok(Some(VersionMap { schemas, end: r.pos }));
        }
        schemas.insert(name, v);
    }
    Ok(None)
}

/// The model thumbnail: a CDib record within a few bytes of the version map's end (SketchUp 6+).
fn read_thumbnail_image(b: &[u8], from: usize) -> Option<Image> {
    let at = find(b, &class_record("CDib"), from, from.saturating_add(64))?;
    let mut r = Reader::new(b, at.saturating_add(10));
    r.u32().ok()?;
    let n = r.u32().ok()? as usize;
    let data = r.bytes(n).ok()?;
    match sniff_image(data)? {
        "image/x-dib" => Some(Image { mime: "image/bmp", bytes: dib_to_bmp(data) }),
        mime => Some(Image { mime, bytes: data.to_vec() }),
    }
}

fn sniff_image(d: &[u8]) -> Option<&'static str> {
    let at = |i: usize| d.get(i).copied();
    if d.len() > 8 && d.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return Some("image/png");
    }
    if d.len() > 3 && d.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if d.len() > 2 && d.starts_with(&[0x42, 0x4d]) {
        return Some("image/bmp");
    }
    if d.len() > 6 && d.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if d.len() > 4 && ((at(0) == Some(0x49) && at(1) == Some(0x49)) || (at(0) == Some(0x4d) && at(1) == Some(0x4d))) {
        return Some("image/tiff");
    }
    if d.len() > 4 && d.starts_with(b"8BPS") {
        return Some("image/vnd.adobe.photoshop"); // SketchUp kept the .psd as is
    }
    if d.len() > 40 && u32at(d, 0) == 40 {
        return Some("image/x-dib"); // a bare BITMAPINFOHEADER
    }
    None
}

/// A bare Windows DIB (BITMAPINFOHEADER + palette + pixels) → a .bmp file.
fn dib_to_bmp(d: &[u8]) -> Vec<u8> {
    let bits = u16at(d, 14);
    let clr_used = u32at(d, 32);
    let comp = u32at(d, 16);
    let colors: i64 = if clr_used != 0 {
        clr_used
    } else if bits <= 8 {
        1i64 << bits.clamp(0, 8)
    } else {
        0
    };
    let palette = colors.saturating_mul(4).saturating_add(if comp == 3 { 12 } else { 0 });
    let total = d.len().saturating_add(14);
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&[0x42, 0x4d]);
    // setUint32 = ToUint32 (mod 2^32)
    out.extend_from_slice(&(u32::try_from(total & 0xffff_ffff).unwrap_or(0)).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    let off = palette.saturating_add(54).rem_euclid(1 << 32);
    out.extend_from_slice(&u32::try_from(off).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(d);
    out
}

/// Display units: the "UnitsOptions" options dictionary → LengthUnit (0 in, 1 ft, 2 mm, 3 cm, 4 m).
fn read_units(b: &[u8]) -> Option<&'static str> {
    let mut key: Vec<Option<u8>> = vec![Some(0xff), Some(0xfe), Some(0xff), Some(12)];
    for c in "UnitsOptions".bytes() {
        key.push(Some(c));
        key.push(Some(0));
    }
    let at = find(b, &key, 0, b.len())?;
    let mut r = Reader::new(b, at);
    str_read(&mut r).ok()?;
    let mut i = 0u32;
    while i < 24 && str_at(b, r.pos) {
        let k = str_read(&mut r).ok()?;
        let v = typed_value(&mut r, 0).ok()?;
        if k == "LengthUnit" {
            return unit_index(&v);
        }
        i = i.saturating_add(1);
    }
    None
}

// ── the walk ─────────────────────────────────────────────────────────────────────────────────────────

/// Where the model data starts: the material manager (count u32 right before the first CMaterial class
/// record), or — with no materials — the layer list just before the first CLayer class record.
fn model_start(b: &[u8], lay: &Layout, from: usize) -> LRes<(usize, u64)> {
    if let Some(mat) = find(b, &class_record("CMaterial"), from, b.len()) {
        let n = match mat.checked_sub(4) {
            Some(p) => u32at(b, p),
            None => 0,
        };
        if n > 100_000 {
            return Err(mis("ccskp.legacy.material_count", &[]));
        }
        return Ok((mat, u64::try_from(n).unwrap_or(0)));
    }
    let layer = find(b, &class_record("CLayer"), from, b.len()).ok_or_else(|| mis("ccskp.legacy.no_lists", &[]))?;
    Ok((layer.saturating_sub(if lay.pid { 9 } else { 8 }), 0))
}

/// Candidate slot bases. With 2+ materials: read material 1 with a throwaway numbering; material 2's
/// class-ref names CMaterial's true index, which fixes the base exactly. Otherwise read the materials and the
/// layer list with a throwaway base: the next object is an ABSOLUTE back-ref to the active layer, an object
/// we just numbered relatively — one candidate per layer read.
fn slot_bases(b: &[u8], lay: Layout, limits: &Limits, start: usize, mat_count: u64) -> LRes<Vec<u32>> {
    let mut probe = Archive::new(b, lay, *limits);
    const B0: u32 = 1 << 20;
    probe.start_at(B0);
    probe.r.pos = start;
    if mat_count >= 2 {
        probe.obj(Some(C_MATERIAL))?;
        let t = u16at(b, probe.r.pos);
        if t == 0xffff || (t != -1 && (t & 0x8000) == 0) {
            return Err(mis("ccskp.legacy.numbering", &[]));
        }
        return Ok(vec![u32::try_from(t & 0x7fff).unwrap_or(0x7fff)]);
    }
    for _ in 0..mat_count {
        probe.obj(Some(C_MATERIAL))?;
    }
    probe.r.u32()?;
    if lay.pid {
        probe.r.u8()?;
    }
    let mut n = 0u32;
    if !lay.v3 {
        n = probe.r.u32()?;
        if !(1..=100_000).contains(&n) {
            return Err(mis("ccskp.legacy.layer_count", &[]));
        }
    }
    let mut got: Vec<(Option<u32>, ValId)> = Vec::new();
    let mut rels: Vec<Option<u32>> = Vec::new();
    for _ in 0..n {
        rels.push(probe.obj(Some(C_LAYER))?.0);
    }
    probe.trailing_layers(&mut got, true)?;
    rels.extend(got.iter().map(|g| g.0));
    let (s, name, _) = probe.obj(None)?;
    if name != Some(PREMODEL) {
        return Err(mis("ccskp.legacy.layer_end", &[]));
    }
    let s = i64::from(s.unwrap_or(0));
    Ok(rels
        .into_iter()
        .filter_map(|rel| {
            let rel = rel?;
            let c = s.saturating_sub(i64::from(rel).saturating_sub(i64::from(B0)));
            (c > 0 && c < i64::from(B0)).then(|| u32::try_from(c).ok()).flatten()
        })
        .collect())
}

struct Walked<'a> {
    ar: Archive<'a>,
    materials: Vec<(Option<u32>, Option<ValId>)>,
    layers: Vec<(Option<u32>, ValId)>,
    root: Vec<Ent>,
}

fn walk_model<'a>(b: &'a [u8], lay: Layout, limits: &Limits, start: usize, mat_count: u64, base: u32) -> LRes<Walked<'a>> {
    let mut ar = Archive::new(b, lay, *limits);
    ar.start_at(base);
    ar.build_defs = true;
    ar.r.pos = start;
    let mut materials = Vec::new();
    for _ in 0..mat_count {
        let (s, _, v) = ar.obj(Some(C_MATERIAL))?;
        materials.push((s, v));
    }

    // layer list: <u32><u8 if pid><u32 count> — SketchUp 3 writes no count, its records run to the anchor
    ar.r.u32()?;
    if lay.pid {
        ar.r.u8()?;
    }
    let declared: u64 = if lay.v3 { u64::MAX } else { u64::from(ar.r.u32()?) };
    if declared > 100_000 && !lay.v3 {
        return Err(mis("ccskp.legacy.layer_count", &[]));
    }
    let mut layers: Vec<(Option<u32>, ValId)> = Vec::new();
    while (layers.len() as u64) < declared {
        let t = ar.peek16()?;
        if t == 0 {
            ar.r.pos = ar.r.pos.saturating_add(2); // SketchUp 2020 separators
            continue;
        }
        if t != 0xffff && (t & 0x8000) == 0 {
            break; // a back-ref: the definition-list anchor
        }
        let (s, _, v) = ar.obj(Some(C_LAYER))?;
        if let Some(v) = v {
            layers.push((s, v));
        }
    }
    ar.trailing_layers(&mut layers, false)?;

    // definition list: a reference to the ACTIVE layer, then the count
    let (_, an, _) = ar.obj(None)?;
    if an != Some(C_LAYER) {
        return Err(mis("ccskp.legacy.def_list", &[]));
    }
    let mut nd = u64::from(ar.r.u32()?);
    let at = ar.r.pos.saturating_sub(4);
    if nd > 1_000_000
        && let Some(n) = retry_after_filler(&mut ar, at, 1_000_000)
    {
        nd = n;
    }
    if nd > 1_000_000 {
        return Err(mis("ccskp.legacy.def_count", &[]));
    }
    for _ in 0..nd {
        ar.obj(Some(C_DEFINITION))?;
    }
    // definitions written back to back past the count
    loop {
        let ds = ar.class_slot.get(&C_DEFINITION).copied();
        let t = u16at(b, ar.r.pos);
        let is_def = ds.is_some_and(|ds| t == i64::from(0x8000 | ds))
            || (t == 0xffff && ascii_at(b, ar.r.pos.saturating_add(6), "CComponentDefinition"));
        if !is_def {
            break;
        }
        ar.obj(None)?;
    }

    // the model's own entity list
    let mut nr = u64::from(ar.r.u32()?);
    let at = ar.r.pos.saturating_sub(4);
    if nr > 5_000_000
        && let Some(n) = retry_after_filler(&mut ar, at, 5_000_000)
    {
        nr = n;
    }
    if nr > 5_000_000 {
        return Err(mis("ccskp.legacy.entity_count", &[]));
    }
    let root = ar.read_entity_list(nr, false, true)?;
    Ok(Walked { ar, materials, layers, root })
}

// ── turning walked objects into the spec's model ───────────────────────────────────────────────────

/// SketchUp's 13 doubles → column-major 4×4 (the 3×3 part ROW by row, then the translation, then w as the
/// homogeneous divisor).
fn matrix_of(xf: &[f64; 13]) -> Option<[f64; 16]> {
    let w = if ok_num(xf[12]) && xf[12].abs() > 1e-12 { xf[12] } else { 1.0 };
    let mut m = [xf[0], xf[3], xf[6], 0.0, xf[1], xf[4], xf[7], 0.0, xf[2], xf[5], xf[8], 0.0, xf[9], xf[10], xf[11], w];
    if !m.iter().all(|&x| ok_num(x)) {
        return None;
    }
    if w != 1.0 {
        for v in m.iter_mut().take(15) {
            *v /= w;
        }
        m[15] = 1.0;
    }
    Some(m)
}

/// Texture coordinates for a face's loops (OpenSKP's recipe): plane basis xr = Z × n, yr = n × xr; untouched
/// textures project straight onto it, positioned ones go through the inverse of their 3×3 mapping.
fn face_uv(loops: &[Vec<P3>], n: [f64; 3], ftc: Option<&[f64; 9]>, tile_w: f64, tile_h: f64) -> Vec<Vec<[f64; 2]>> {
    let [nx, ny, nz] = n;
    let cl = hypot(&[nx, ny]);
    let (xr, yr) = if cl < 1e-9 {
        ([1.0, 0.0, 0.0], [0.0, if nz >= 0.0 { 1.0 } else { -1.0 }, 0.0])
    } else {
        let xr = [-ny / cl, nx / cl, 0.0];
        let yr = [-nz * xr[1], nz * xr[0], nx * xr[1] - ny * xr[0]];
        (xr, yr)
    };
    let mut inv: Option<[f64; 9]> = None;
    if let Some(&[a, b, c, d, e, f, g, h, i]) = ftc {
        let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if det.is_finite() && det.abs() > 1e-12 {
            let k = 1.0 / det;
            inv = Some([
                (e * i - f * h) * k,
                (c * h - b * i) * k,
                (b * f - c * e) * k,
                (f * g - d * i) * k,
                (a * i - c * g) * k,
                (c * d - a * f) * k,
                (d * h - e * g) * k,
                (b * g - a * h) * k,
                (a * e - b * d) * k,
            ]);
        }
    }
    let tw = if ok_num(tile_w) && tile_w > 1e-9 { tile_w } else { 1.0 };
    let th = if ok_num(tile_h) && tile_h > 1e-9 { tile_h } else { 1.0 };
    loops
        .iter()
        .map(|pts| {
            pts.iter()
                .map(|p| {
                    let px = p[0] * xr[0] + p[1] * xr[1] + p[2] * xr[2];
                    let py = p[0] * yr[0] + p[1] * yr[1] + p[2] * yr[2];
                    let Some(inv) = inv else { return [px / tw, py / th] };
                    let mut q = px * inv[2] + py * inv[5] + inv[8];
                    if q.abs() < 1e-12 {
                        q = 1.0;
                    }
                    [(px * inv[0] + py * inv[3] + inv[6]) / q / tw, (px * inv[1] + py * inv[4] + inv[7]) / q / th]
                })
                .collect()
        })
        .collect()
}

/// Newell normal of a loop (for checking orientation against the stored plane).
fn newell(pts: &[P3]) -> P3 {
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let n = pts.len();
    for (i, a) in pts.iter().enumerate() {
        let b = pts.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).unwrap_or(a);
        x += (a[1] - b[1]) * (a[2] + b[2]);
        y += (a[2] - b[2]) * (a[0] + b[0]);
        z += (a[0] - b[0]) * (a[1] + b[1]);
    }
    [x, y, z]
}

fn is_web_image(m: Option<&str>) -> bool {
    matches!(m, Some("image/png" | "image/jpeg" | "image/gif" | "image/bmp"))
}

// ── entry point ──────────────────────────────────────────────────────────────────────────────────────

/// Read a classic file (`readLegacy(bytes, header)`).
pub fn read_legacy(bytes: &[u8], header: &Header, limits: &Limits) -> Result<Body, SkpError> {
    if bytes.len() as u64 > limits.file_bytes {
        return Err(err("too_big", "ccskp.too_big.file", &[("size", "1 GB")]));
    }
    match read(bytes, header, limits) {
        Ok(b) => Ok(b),
        Err(LErr::Skp(e)) => Err(e),
        Err(LErr::Mis(m)) => Err(SkpError::new("damaged", msg("ccskp.legacy.failed", &[("reason", &m)]))),
        Err(LErr::Internal(m)) => Err(SkpError::new("damaged", msg("ccskp.legacy.failed_internal", &[("reason", &m)]))),
    }
}

fn is_version_text(s: &str) -> bool {
    // /^\d+(\.\d+)*$/
    !s.is_empty() && s.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

fn is_integer(v: f64) -> bool {
    v.is_finite() && v.trunc() == v
}

fn read(b: &[u8], header: &Header, limits: &Limits) -> LRes<Body> {
    let version = if is_version_text(&header.version) { Some(header.version.clone()) } else { header_version(b) };
    let major = if is_integer(header.major) {
        header.major
    } else {
        crate::util::parse_int10(version.as_deref().unwrap_or("")).unwrap_or(f64::NAN)
    };
    if !is_integer(major) {
        return Err(err("not_skp", "ccskp.legacy.no_version", &[]).into());
    }
    if major < 3.0 {
        return Err(err("unsupported", "ccskp.legacy.too_old", &[("major", &js_num_text(major))]).into());
    }
    if major > 20.0 {
        return Err(err("unsupported", "ccskp.legacy.unknown_major", &[("major", &js_num_text(major))]).into());
    }

    let vmap = read_version_map(b, Some(header.body_start))?
        .ok_or_else(|| LErr::Skp(SkpError::new("damaged", crate::messages::damaged_text("ccskp.damaged.no_version_map", &[]))))?;
    let lay = layout_for(major, &vmap.schemas);
    let thumbnail = read_thumbnail_image(b, vmap.end);
    let (start, mat_count) = model_start(b, &lay, vmap.end)?;

    let mut walked = None;
    let mut last = None;
    for base in slot_bases(b, lay, limits, start, mat_count)? {
        match walk_model(b, lay, limits, start, mat_count, base) {
            Ok(w) => {
                walked = Some(w);
                break;
            }
            Err(LErr::Mis(m)) => last = Some(m),
            Err(e) => return Err(e),
        }
    }
    let Some(walked) = walked else {
        return Err(match last {
            Some(m) => LErr::Mis(m),
            None => mis("ccskp.legacy.no_numbering", &[]),
        });
    };
    let Walked { mut ar, materials: wmats, layers: wlayers, root } = walked;
    let root_built = ar.build_entities(&root)?;
    let ar = ar;

    // materials (spec order = file order), keyed by store-map index
    let mut mat_index: FxMap<u32, usize> = FxMap::default();
    let mut bad_textures = 0u64;
    let mut materials: Vec<Material> = Vec::with_capacity(wmats.len());
    for (i, (s, v)) in wmats.iter().enumerate() {
        if let Some(s) = s {
            mat_index.insert(*s, i);
        }
        let Some(v) = v.and_then(|v| ar.val(v)) else {
            return Err(LErr::Internal("Cannot read properties of null (reading 'useOpacity')".to_string()));
        };
        let (alpha, texture) = match v {
            Val::Material(m) => {
                // the stored double is a transparency gated by the use-flag (OpenSKP); alpha = 1 − it
                let alpha = if m.use_opacity != 0 {
                    let t = 1.0 - m.opacity;
                    if t.is_nan() { f64::NAN } else { t.clamp(0.0, 1.0) }
                } else {
                    1.0
                };
                let mut texture = None;
                if let Some(t) = &m.tex
                    && t.image.1 > 0
                {
                    let img = b.get(t.image.0..t.image.0.saturating_add(t.image.1)).unwrap_or(&[]);
                    let mut mime = sniff_image(img);
                    let data = if mime == Some("image/x-dib") {
                        mime = Some("image/bmp");
                        dib_to_bmp(img)
                    } else {
                        img.to_vec()
                    };
                    if !is_web_image(mime) {
                        bad_textures = bad_textures.saturating_add(1);
                    }
                    let src = if t.file.is_empty() { m.name.as_str() } else { t.file.as_str() };
                    let last = src.rsplit(['/', '\\']).next().unwrap_or("");
                    let name = if last.is_empty() { m.name.clone() } else { last.to_string() };
                    texture = Some(Texture {
                        name,
                        mime: mime.unwrap_or("application/octet-stream"),
                        bytes: data,
                        width: if ok_num(t.w) { t.w } else { 1.0 },
                        height: if ok_num(t.h) { t.h } else { 1.0 },
                    });
                }
                (alpha, texture)
            }
            _ => (1.0, None),
        };
        materials.push(Material { name: v.name().map(str::to_string), color: v.rgba().map(|c| [c[0], c[1], c[2]]), alpha, texture });
    }

    // layers: Layer0 first (spec: index 0 = Layer0 / Untagged)
    let mut layer_list: Vec<(Option<u32>, Option<ValId>)> = wlayers.iter().map(|&(s, v)| (s, Some(v))).collect();
    let l0 = layer_list.iter().position(|(_, v)| v.and_then(|v| ar.val(v)).and_then(Val::name) == Some("Layer0"));
    if let Some(l0) = l0.filter(|&i| i > 0) {
        let x = layer_list.remove(l0);
        layer_list.insert(0, x);
    }
    let mut layers: Vec<Layer> = layer_list
        .iter()
        .map(|(_, v)| {
            let v = v.and_then(|v| ar.val(v));
            Layer {
                name: v.and_then(Val::name).map(str::to_string),
                visible: match v {
                    Some(Val::Layer(l)) => Some(l.1),
                    _ => None,
                },
                color: v.and_then(Val::rgba).map(|c| [c[0], c[1], c[2]]),
            }
        })
        .collect();
    if l0.is_none() {
        layers.insert(0, Layer { name: Some("Layer0".to_string()), visible: Some(true), color: None });
        layer_list.insert(0, (None, None));
    }
    let mut layer_index: FxMap<u32, usize> = FxMap::default();
    for (i, (s, _)) in layer_list.iter().enumerate() {
        if let Some(s) = s {
            layer_index.insert(*s, i);
        }
    }
    let lix = |s: Option<u32>| s.map_or(0, |s| layer_index.get(&s).copied().unwrap_or(0));

    // definitions: the model first, then every definition in file order
    let mut def_slots: Vec<u32> = ar.built.iter().map(|(s, _)| *s).collect();
    def_slots.sort_unstable();
    let def_index: FxMap<u32, usize> = def_slots.iter().enumerate().map(|(i, &s)| (s, i.saturating_add(1))).collect();
    let mut kind_of: FxMap<Option<u32>, &'static str> = FxMap::default();
    for g in std::iter::once(&root_built).chain(ar.built.iter().map(|(_, g)| g)) {
        for &it in &g.insts {
            let Some(Val::Inst(iv)) = ar.val(it) else { continue };
            let k = match iv.cls {
                C_GROUP => "group",
                C_IMAGE => "image",
                _ => "component",
            };
            if !kind_of.contains_key(&iv.def) || k != "component" {
                kind_of.insert(iv.def, k);
            }
        }
    }
    let mut unknown_mat = 0u64;
    let mut bad_inst = 0u64;
    let mut projected = 0u64;
    let mut flipped = 0u64;
    let mut mix = |s: Option<u32>| -> Option<usize> {
        let s = s.filter(|&s| s != 0)?;
        match mat_index.get(&s) {
            Some(&i) => Some(i),
            None => {
                unknown_mat = unknown_mat.saturating_add(1);
                None
            }
        }
    };
    let mut to_def = |g: &Built, name: String, kind: &'static str, guid: Option<String>| -> Def {
        let mut faces = Vec::with_capacity(g.faces.len());
        for f in &g.faces {
            // keep SketchUp's front side: outer loop counter-clockwise seen along the stored plane normal
            let n = [f.plane[0], f.plane[1], f.plane[2]];
            let nn = newell(f.loops.first().map_or(&[][..], Vec::as_slice));
            if nn[0] * n[0] + nn[1] * n[1] + nn[2] * n[2] < 0.0 {
                flipped = flipped.saturating_add(1);
            }
            let front = mix(f.front);
            let tex = front.and_then(|i| materials.get(i)).and_then(|m| m.texture.as_ref());
            let mut uv = None;
            if let Some(tex) = tex {
                let ftc = f.ftc.and_then(|id| match ar.val(id) {
                    Some(Val::Ftc(x)) => Some(x),
                    _ => None,
                });
                if ftc.is_some_and(|x| x.1) {
                    projected = projected.saturating_add(1);
                }
                uv = Some(face_uv(&f.loops, n, ftc.map(|x| &x.0), tex.width, tex.height));
            }
            let back = mix(f.back.map(u32::from));
            faces.push(Face { loops: f.loops.clone(), front, back, layer: Some(lix(f.layer)), uv, hidden: f.hidden.then_some(true) });
        }
        let edges = g
            .edges
            .iter()
            .map(|e| Edge { a: e.a, b: e.b, layer: Some(lix(e.layer)), soft: e.soft, smooth: e.smooth, hidden: e.hidden })
            .collect();
        let mut insts = Vec::new();
        for &it in &g.insts {
            let Some(Val::Inst(iv)) = ar.val(it) else { continue };
            let d = iv.def.and_then(|d| def_index.get(&d)).copied();
            let m = matrix_of(&iv.xf);
            let (Some(d), Some(m)) = (d, m) else {
                bad_inst = bad_inst.saturating_add(1);
                continue;
            };
            insts.push(Inst {
                def: d,
                m,
                name: iv.name.clone(),
                layer: Some(lix(iv.db.layer)),
                hidden: iv.db.hidden,
                material: mix(Some(iv.db.mat)),
            });
        }
        Def { name, kind, guid, faces, edges, soft: g.soft.clone(), insts }
    };
    let built_at: FxMap<u32, usize> = ar.built.iter().enumerate().map(|(i, (s, _))| (*s, i)).collect();
    let mut defs = vec![to_def(&root_built, "Model".to_string(), "model", None)];
    for s in &def_slots {
        let Some((_, g)) = built_at.get(s).and_then(|&i| ar.built.get(i)) else { continue };
        let kind = kind_of.get(&Some(*s)).copied().unwrap_or("component");
        let guid = (!g.guid.is_empty()).then(|| g.guid.clone());
        defs.push(to_def(g, g.name.clone(), kind, guid));
    }
    let bad_faces = ar.bad_faces;

    // notes: one line per kind of thing skipped
    let mut notes = Vec::new();
    let note = |notes: &mut Vec<String>, key: &str, n: u64| notes.push(msg(key, &[("n", &n.to_string())]));
    let sk = ar.skipped;
    if sk.text > 0 {
        note(&mut notes, "ccskp.note.text_labels", sk.text);
    }
    if sk.dims > 0 {
        note(&mut notes, "ccskp.note.dimensions", sk.dims);
    }
    if sk.sections > 0 {
        note(&mut notes, "ccskp.note.section_planes", sk.sections);
    }
    if sk.guides > 0 {
        note(&mut notes, "ccskp.note.guides", sk.guides);
    }
    if bad_faces > 0 {
        note(&mut notes, "ccskp.note.face_corners", bad_faces);
    }
    if bad_inst > 0 {
        note(&mut notes, "ccskp.note.inst_broken", bad_inst);
    }
    if unknown_mat > 0 {
        note(&mut notes, "ccskp.note.material_unknown", unknown_mat);
    }
    if bad_textures > 0 {
        note(&mut notes, "ccskp.note.texture_format", bad_textures);
    }
    if projected > 0 {
        note(&mut notes, "ccskp.note.texture_projected", projected);
    }
    if flipped > 0 {
        note(&mut notes, "ccskp.note.face_flipped", flipped);
    }
    if ar.root_cut > 0 {
        note(&mut notes, "ccskp.note.root_cut", ar.root_cut as u64);
    }
    if lay.v3 {
        notes.push(msg("ccskp.note.v3", &[]));
    }
    notes.push(msg("ccskp.note.not_read", &[]));

    Ok(Body { materials, layers, defs, root: 0, thumbnail, units: read_units(b), notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let b = [0u8, 0xff, 0xff, 1, 2, 4, 0, b'C', b'D', b'i', b'b'];
        assert_eq!(find(&b, &class_record("CDib"), 0, b.len()), Some(1));
        assert_eq!(find(&b, &class_record("CDib"), 2, b.len()), None);
        assert_eq!(find(&b, &class_record("CDib"), 0, b.len() - 1), None);
        let s = [0xffu8, 0xfe, 0xff, 0xfe, 0xff, 7];
        assert_eq!(find(&s, &STR, 1, s.len()), Some(2));
        assert_eq!(find(&s, &[Some(0xfe), None, Some(7)], 0, s.len()), Some(3));
        let mut v = b"..{6.0.1}".iter().flat_map(|&c| [c, 0]).collect::<Vec<u8>>();
        v.extend([0u8; 4]);
        assert_eq!(header_version(&v).as_deref(), Some("6.0.1"));
        assert!(is_version_text("17.0.18899"));
        assert!(!is_version_text("17..1"));
        assert_eq!(unit_index(&Tv::Num(2.0)), Some("mm"));
        assert_eq!(unit_index(&Tv::Str("4".into())), Some("m"));
        assert_eq!(unit_index(&Tv::Num(2.5)), None);
        let m = matrix_of(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0, 0.5]).unwrap();
        assert_eq!(m[12], 10.0);
        assert_eq!(m[15], 1.0);
    }
}
