//! The cc-skp model (docs/SKP-LIB-SPEC.md "The model"), field for field the object `readSkp` returns.
//!
//! `Option` fields are `null` in the JS when they are `None`, except where a comment says the key is absent
//! (`undefined`, dropped by JSON) instead.

/// 3D point (inches).
pub type P3 = [f64; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// SketchUp 2021+ (header + ZIP archive).
    Vff,
    /// SketchUp 3 … 2020 (header + one MFC object stream).
    Legacy,
}

impl Container {
    pub fn as_str(self) -> &'static str {
        match self {
            Container::Vff => "vff",
            Container::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skp {
    pub container: Container,
    pub version: String,
    /// `parseInt` of the version's first number (a JS number: huge digit strings give a float).
    pub major: f64,
    pub materials: Vec<Material>,
    pub layers: Vec<Layer>,
    pub defs: Vec<Def>,
    pub root: usize,
    pub thumbnail: Option<Image>,
    /// `units.length`: 'in' | 'ft' | 'mm' | 'cm' | 'm' | null.
    pub units: Option<&'static str>,
    pub notes: Vec<String>,
    pub counts: Counts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub defs: usize,
    pub faces: usize,
    pub edges: usize,
    pub insts: usize,
    pub materials: usize,
    pub layers: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// `None` = key absent (only a damaged classic file can have a material without a name).
    pub name: Option<String>,
    pub color: Option<[u8; 3]>,
    pub alpha: f64,
    pub texture: Option<Texture>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    /// Inches per tile.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// `None` = key absent (damaged classic files only).
    pub name: Option<String>,
    /// `None` = key absent (damaged classic files only).
    pub visible: Option<bool>,
    pub color: Option<[u8; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub name: String,
    /// 'model' | 'component' | 'group' | 'image'.
    pub kind: &'static str,
    pub guid: Option<String>,
    pub faces: Vec<Face>,
    /// Loose edges only (no face uses them).
    pub edges: Vec<Edge>,
    /// Face edges marked soft or smooth.
    pub soft: Vec<[P3; 2]>,
    pub insts: Vec<Inst>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// First = outer, rest = holes.
    pub loops: Vec<Vec<P3>>,
    pub front: Option<usize>,
    pub back: Option<usize>,
    pub layer: Option<usize>,
    /// Front texture coordinates per loop vertex, when textured.
    pub uv: Option<Vec<Vec<[f64; 2]>>>,
    /// 2021+: always present; classic files: present (true) only on hidden faces.
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub a: P3,
    pub b: P3,
    pub layer: Option<usize>,
    pub soft: bool,
    pub smooth: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inst {
    pub def: usize,
    /// Column-major 4×4 (m[12..14] = translation).
    pub m: [f64; 16],
    pub name: String,
    pub layer: Option<usize>,
    pub hidden: bool,
    pub material: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}
