//! The material library of 0.5 (render/materials.js, RC.7 / RC.11 (b)): ids, categories and the colour each material
//! shows from afar (what the 3D view, exports and 3D printing use). Names are catalog words `render.mat.<id>`,
//! categories `render.matcat.<cat>`. The procedural textures (textures.js) are not drawn yet: swatches show the colour.

/// One library material: (id, category, colour, roughness, metalness, glass).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat {
    pub id: &'static str,
    pub cat: &'static str,
    pub color: &'static str,
    pub rough: f64,
    pub metal: f64,
    pub glass: f64,
}

const fn m(id: &'static str, cat: &'static str, color: &'static str, rough: f64, metal: f64, glass: f64) -> Mat {
    Mat { id, cat, color, rough, metal, glass }
}

/// The order the library lists its categories in.
pub const CATEGORIES: &[&str] = &["brick", "siding", "roofing", "wood", "stone", "tile", "concrete", "finish", "ground", "glass", "metal"];

/// MATERIALS, in the library's order.
pub const MATERIALS: &[Mat] = &[
    m("brick-red", "brick", "#94503c", 0.85, 0.0, 0.0),
    m("brick-brown", "brick", "#6f4a38", 0.85, 0.0, 0.0),
    m("brick-buff", "brick", "#c1a47c", 0.85, 0.0, 0.0),
    m("brick-painted", "brick", "#e3dfd6", 0.75, 0.0, 0.0),
    m("block-gray", "brick", "#a4a19b", 0.9, 0.0, 0.0),
    m("siding-white", "siding", "#e8e4dc", 0.55, 0.0, 0.0),
    m("siding-gray", "siding", "#949ca0", 0.55, 0.0, 0.0),
    m("siding-blue", "siding", "#6f8597", 0.55, 0.0, 0.0),
    m("siding-cedar", "siding", "#9a6542", 0.7, 0.0, 0.0),
    m("shingles-gray", "roofing", "#5c5e61", 0.92, 0.0, 0.0),
    m("shingles-brown", "roofing", "#584536", 0.92, 0.0, 0.0),
    m("shingles-cedar", "roofing", "#86593c", 0.85, 0.0, 0.0),
    m("roof-metal", "roofing", "#4b5a63", 0.4, 0.6, 0.0),
    m("tile-roof", "roofing", "#a3583a", 0.7, 0.0, 0.0),
    m("wood-oak", "wood", "#a97c4f", 0.55, 0.0, 0.0),
    m("wood-walnut", "wood", "#644633", 0.5, 0.0, 0.0),
    m("wood-pine", "wood", "#c9a675", 0.6, 0.0, 0.0),
    m("wood-deck", "wood", "#938776", 0.8, 0.0, 0.0),
    m("stone-field", "stone", "#8b867c", 0.85, 0.0, 0.0),
    m("stone-limestone", "stone", "#c8bb9f", 0.8, 0.0, 0.0),
    m("stone-slate", "stone", "#53585d", 0.6, 0.0, 0.0),
    m("tile-white", "tile", "#e6e3dc", 0.2, 0.0, 0.0),
    m("tile-terracotta", "tile", "#b2633f", 0.6, 0.0, 0.0),
    m("tile-gray", "tile", "#7e8185", 0.35, 0.0, 0.0),
    m("concrete", "concrete", "#aeaaa3", 0.85, 0.0, 0.0),
    m("concrete-ties", "concrete", "#a5a29c", 0.85, 0.0, 0.0),
    m("stucco-white", "finish", "#e3ddd1", 0.9, 0.0, 0.0),
    m("stucco-sand", "finish", "#d0bd9b", 0.9, 0.0, 0.0),
    m("grass", "ground", "#5a7a39", 0.95, 0.0, 0.0),
    m("gravel", "ground", "#968f84", 0.95, 0.0, 0.0),
    m("glass-clear", "glass", "#c9dbe6", 0.04, 0.0, 0.85),
    m("glass-tinted", "glass", "#6f8796", 0.04, 0.0, 0.7),
    m("metal-steel", "metal", "#9aa0a6", 0.35, 1.0, 0.0),
    m("metal-dark", "metal", "#3d4247", 0.45, 1.0, 0.0),
];

/// `material(id)` → the entry.
pub fn material(id: &str) -> Option<&'static Mat> {
    MATERIALS.iter().find(|x| x.id == id)
}

/// `#rrggbb` → (r, g, b).
pub fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library() {
        assert_eq!(MATERIALS.len(), 34);
        assert!(MATERIALS.iter().all(|x| CATEGORIES.contains(&x.cat) && rgb(x.color).is_some()));
        assert_eq!(material("wood-oak").map(|x| x.color), Some("#a97c4f"));
        assert!(material("nope").is_none());
        assert!(MATERIALS.iter().all(|x| cc_m3d::paint::is_material_paint(&format!("mat:{}", x.id))));
    }
}
