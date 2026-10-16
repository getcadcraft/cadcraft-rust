//! CadCraft L1: the 2D drawing as the viewer draws it — the Rust form of BimCraft's `dwg-compile.js` output
//! (`compileDrawing(model)`): spaces (model first, then layouts in tab order) of batches per layer × colour × line
//! weight, each with polylines, fills and texts in the space's world coordinates.
//!
//! Units and conventions: drawing units, Y up, angles in radians, line weights in mm (0 = thinnest).
//! Serialises (serde) to exactly the JS shape, so the compile step is checked against `compileDrawing` field
//! for field (`Color` → `"ink"` / `"#rrggbb"`).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use serde::{Deserialize, Serialize};

/// A colour: `Ink` (ACI 7: black on paper, white on a dark screen) or a true colour 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    Ink,
    Rgb(u32),
}

impl Color {
    /// "ink" | "#rrggbb"
    pub fn css(self) -> String {
        match self {
            Color::Ink => "ink".to_string(),
            Color::Rgb(n) => format!("#{:06x}", n & 0xFF_FFFF),
        }
    }
    pub fn parse(s: &str) -> Color {
        match s.strip_prefix('#').and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(n) => Color::Rgb(n),
            None => Color::Ink,
        }
    }
}
impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.css())
    }
}
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Color::parse(&s))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl Default for Bounds {
    fn default() -> Self {
        Bounds { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY }
    }
}
impl Bounds {
    pub fn is_empty(&self) -> bool {
        !(self.max_x >= self.min_x && self.max_y >= self.min_y)
    }
    /// Grow by flat [x0, y0, x1, y1, …] points (non-finite points skipped).
    pub fn grow(&mut self, pts: &[f64]) {
        for &[x, y] in pts.as_chunks::<2>().0 {
            if x.is_finite() && y.is_finite() {
                self.min_x = self.min_x.min(x);
                self.max_x = self.max_x.max(x);
                self.min_y = self.min_y.min(y);
                self.max_y = self.max_y.max(y);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub color: Color,
    pub on: bool,
    /// mm
    pub lw: f64,
    /// false = never printed (plot flag off, DEFPOINTS)
    pub plot: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    /// flat rings [x0, y0, x1, y1, …], even-odd
    pub rings: Vec<Vec<f64>>,
    /// a pattern hatch (drawn lighter) rather than a solid fill
    pub pattern: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub x: f64,
    pub y: f64,
    /// height in world units
    pub h: f64,
    /// radians
    pub rot: f64,
    pub s: String,
    /// 0 left, 1 center, 2 right
    pub ha: u8,
    /// 0 baseline, 1 bottom, 2 middle, 3 top
    pub va: u8,
    pub mirror: bool,
}

/// Everything on one layer in one colour and one line weight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub layer: String,
    pub color: Color,
    pub lw: f64,
    /// flat polylines [x0, y0, x1, y1, …]
    pub lines: Vec<Vec<f64>>,
    pub fills: Vec<Fill>,
    pub texts: Vec<Text>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    /// centre on the sheet
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    /// model point at the viewport centre
    pub vx: f64,
    pub vy: f64,
    pub view_height: f64,
    pub twist: f64,
    /// frozen layer handles
    pub frozen: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceImage {
    /// pixel → world affine [a, b, c, d, e, f]
    pub m: [f64; 6],
    pub w: f64,
    pub h: f64,
    #[serde(rename = "ref")]
    pub file: String,
    pub layer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    Model,
    Paper,
}

/// Serialises like the JS: a model space has no `paper` / `paperUnit` keys; a paper space always has both
/// (`paper: null` when the layout has no usable limits), see the hand-written `Serialize` below.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    /// "model" | "layout:<handle>"
    pub key: String,
    pub name: String,
    pub kind: SpaceKind,
    pub batches: Vec<Batch>,
    pub bounds: Bounds,
    #[serde(default)]
    pub paper: Option<Bounds>,
    /// "mm" | "in" (paper spaces)
    #[serde(default)]
    pub paper_unit: Option<String>,
    pub viewports: Vec<Viewport>,
    pub images: Vec<SpaceImage>,
}

impl Serialize for Space {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let paper = self.kind == SpaceKind::Paper;
        let mut st = s.serialize_struct("Space", if paper { 9 } else { 7 })?;
        st.serialize_field("key", &self.key)?;
        st.serialize_field("name", &self.name)?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("batches", &self.batches)?;
        st.serialize_field("bounds", &self.bounds)?;
        if paper {
            st.serialize_field("paper", &self.paper)?;
            st.serialize_field("paperUnit", &self.paper_unit)?;
        } else {
            st.skip_field("paper")?;
            st.skip_field("paperUnit")?;
        }
        st.serialize_field("viewports", &self.viewports)?;
        st.serialize_field("images", &self.images)?;
        st.end()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct XrefInfo {
    pub name: String,
    pub loaded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageRef {
    #[serde(rename = "ref")]
    pub file: String,
    pub name: String,
    pub uses: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub entities: u64,
    /// type → count of entities not drawn
    pub unsupported: std::collections::BTreeMap<String, u64>,
    pub truncated: bool,
    pub prims: u64,
    pub xref_entities: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    /// "AC1027"
    pub version: String,
    /// "2013"
    pub release: String,
    /// "inches", "millimeters", … or ""
    pub units: String,
    pub xrefs: Vec<XrefInfo>,
    pub images: Vec<ImageRef>,
    pub layers: Vec<Layer>,
    pub spaces: Vec<Space>,
    pub stats: Stats,
}

impl Drawing {
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colours_round_trip() {
        assert_eq!(Color::Rgb(0xff0000).css(), "#ff0000");
        assert_eq!(Color::parse("ink"), Color::Ink);
        assert_eq!(Color::parse("#00ff00"), Color::Rgb(0x00ff00));
        let mut b = Bounds::default();
        assert!(b.is_empty());
        b.grow(&[1.0, 2.0, f64::NAN, 0.0, -1.0, 5.0]);
        assert_eq!((b.min_x, b.max_y), (-1.0, 5.0));
    }
}
