//! geo-style.js — the colour each piece of geometry is drawn in (IFC4 ADD2 TC1 §8.20 Presentation appearance, §8.10
//! Material resource): 1. an IfcStyledItem on the representation item; 2. the element's material (also through its
//! type); 3. white. Colours are [r, g, b, alpha] (0–1).
//!
//! The parity output has no colours, but the JS reads these lines on the way, so a line that does not parse fails the
//! same calls here.

use std::collections::HashMap;

use super::ctx::{GR, GeoCtx, num, numd, refs};
use crate::jsmath::{max, min};
use crate::model::{Id, id_key};
use crate::value::Value;

pub type Colour = [f64; 4];
pub const DEFAULT: Colour = [1.0, 1.0, 1.0, 1.0];

#[derive(Default)]
pub struct Styles {
    by_item: Option<HashMap<u64, Colour>>,
    mat_color: HashMap<u64, Option<Colour>>,
    prod_color: HashMap<u64, Option<Colour>>,
}

impl Styles {
    fn index(&mut self, g: &GeoCtx<'_>) -> GR<()> {
        if self.by_item.is_some() {
            return Ok(());
        }
        let mut by = HashMap::new();
        for s in g.f.of_type("IFCSTYLEDITEM", true) {
            let Some(item) = g.ra(Some(s), "Item")? else { continue };
            let c = style_color(g, g.a(Some(s), "Styles")?, 0)?;
            if let Some(c) = c {
                by.entry(id_key(item)).or_insert(c);
            }
        }
        self.by_item = Some(by);
        Ok(())
    }

    pub fn item_color(&mut self, g: &GeoCtx<'_>, id: Id) -> GR<Option<Colour>> {
        self.index(g)?;
        Ok(self.by_item.as_ref().and_then(|m| m.get(&id_key(id)).copied()))
    }

    /// Colour from the product's material (or its type's).
    pub fn product_color(&mut self, g: &GeoCtx<'_>, pid: Id) -> GR<Option<Colour>> {
        if let Some(c) = self.prod_color.get(&id_key(pid)) {
            return Ok(*c);
        }
        let assoc = g.f.inverse("IFCRELASSOCIATESMATERIAL", "RelatedObjects")?;
        let mut c = None;
        for &r in assoc.get(&id_key(pid)).map(Vec::as_slice).unwrap_or(&[]) {
            let m = g.ra(Some(r), "RelatingMaterial")?;
            c = self.material_color(g, m, 0)?;
            if c.is_some() {
                break;
            }
        }
        if c.is_none() {
            let by_type = g.f.inverse("IFCRELDEFINESBYTYPE", "RelatedObjects")?;
            'outer: for &r in by_type.get(&id_key(pid)).map(Vec::as_slice).unwrap_or(&[]) {
                let ty = g.ra(Some(r), "RelatingType")?;
                let list = ty.and_then(|t| assoc.get(&id_key(t))).cloned().unwrap_or_default();
                for ar in list {
                    let m = g.ra(Some(ar), "RelatingMaterial")?;
                    c = self.material_color(g, m, 0)?;
                    if c.is_some() {
                        break;
                    }
                }
                if c.is_some() {
                    break 'outer;
                }
            }
        }
        self.prod_color.insert(id_key(pid), c);
        Ok(c)
    }

    fn material_color(&mut self, g: &GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Option<Colour>> {
        let Some(mid) = id else { return Ok(None) };
        if depth > 6 {
            return Ok(None);
        }
        if let Some(c) = self.mat_color.get(&id_key(mid)) {
            return Ok(*c);
        }
        let t = g.t(id).unwrap_or("");
        let d1 = depth.saturating_add(1);
        let mut c: Option<Colour> = None;
        match t {
            "IFCMATERIAL" => {
                let reps = g.f.inverse("IFCMATERIALDEFINITIONREPRESENTATION", "RepresentedMaterial")?;
                'a: for &r in reps.get(&id_key(mid)).map(Vec::as_slice).unwrap_or(&[]) {
                    for sr in refs(g.a(Some(r), "Representations")?) {
                        for it in refs(g.a(Some(sr), "Items")?) {
                            if g.t(Some(it)) == Some("IFCSTYLEDITEM") {
                                c = style_color(g, g.a(Some(it), "Styles")?, 0)?;
                            }
                            if c.is_some() {
                                break 'a;
                            }
                        }
                    }
                }
            }
            "IFCMATERIALLAYERSETUSAGE" => c = self.material_color(g, g.ra(id, "ForLayerSet")?, d1)?,
            "IFCMATERIALPROFILESETUSAGE" | "IFCMATERIALPROFILESETUSAGETAPERING" => {
                c = self.material_color(g, g.ra(id, "ForProfileSet")?, d1)?
            }
            "IFCMATERIALLAYERSET" | "IFCMATERIALPROFILESET" | "IFCMATERIALCONSTITUENTSET" => {
                let attr = match t {
                    "IFCMATERIALLAYERSET" => "MaterialLayers",
                    "IFCMATERIALPROFILESET" => "MaterialProfiles",
                    _ => "MaterialConstituents",
                };
                for l in refs(g.a(id, attr)?) {
                    c = self.material_color(g, g.ra(Some(l), "Material")?, d1)?;
                    if c.is_some() {
                        break;
                    }
                }
            }
            "IFCMATERIALLIST" => {
                for m in refs(g.a(id, "Materials")?) {
                    c = self.material_color(g, Some(m), d1)?;
                    if c.is_some() {
                        break;
                    }
                }
            }
            "IFCMATERIALLAYER" | "IFCMATERIALPROFILE" | "IFCMATERIALCONSTITUENT" => {
                c = self.material_color(g, g.ra(id, "Material")?, d1)?
            }
            _ => {}
        }
        self.mat_color.insert(id_key(mid), c);
        Ok(c)
    }
}

/// The first surface colour found in a list of styles (any nesting).
fn style_color(g: &GeoCtx<'_>, list: Option<&Value>, depth: u32) -> GR<Option<Colour>> {
    if depth > 6 {
        return Ok(None);
    }
    let items: Vec<&Value> = match list {
        Some(Value::List(l)) => l.iter().collect(),
        Some(v) => vec![v],
        None => vec![],
    };
    for v in items {
        let Some(id) = v.as_ref_id() else { continue };
        let t = g.t(Some(id));
        if t == Some("IFCPRESENTATIONSTYLEASSIGNMENT") || t == Some("IFCSURFACESTYLE") {
            if let Some(c) = style_color(g, g.a(Some(id), "Styles")?, depth.saturating_add(1))? {
                return Ok(Some(c));
            }
        } else if t == Some("IFCSURFACESTYLERENDERING") || t == Some("IFCSURFACESTYLESHADING") {
            let Some(rgb) = colour(g, g.ra(Some(id), "SurfaceColour")?)? else { continue };
            let tr = super::math::or(numd(g.a(Some(id), "Transparency")?, 0.0), 0.0);
            return Ok(Some([rgb[0], rgb[1], rgb[2], 1.0 - min(1.0, max(0.0, tr))]));
        }
    }
    Ok(None)
}

fn colour(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Option<[f64; 3]>> {
    if id.is_none() || g.t(id) != Some("IFCCOLOURRGB") {
        return Ok(None);
    }
    let c = |n: &str| -> GR<f64> { Ok(num(g.a(id, n)?).unwrap_or(1.0)) };
    Ok(Some([c("Red")?, c("Green")?, c("Blue")?]))
}
