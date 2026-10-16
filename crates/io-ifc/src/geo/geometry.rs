//! geometry.js — the geometry of IFC products. For each product with a placement and a shape: the pieces to draw (a
//! mesh in the file's length unit, the matrix that places it in the world (file units, Z up) and a colour).
//! Representation items: mapped items, swept solids and CSG primitives (solid.rs), breps and face sets (brep.rs,
//! nurbs.rs), booleans, half-spaces and CSG trees (bool_ops.rs); openings (IfcRelVoidsElement) are cut from their
//! element. Items shared through a representation map are built once.

use std::collections::HashMap;
use std::sync::Arc;

use super::align::sectioned_solid;
use super::bool_ops::{BoolOp, SolidOps, boolean, half_space_cut, subtract_all};
use super::brep::{bounding_box, brep, face_set, shell_faces, surface_model};
use super::ctx::{GR, GeoCtx, enm, refs, str_};
use super::math::{I4, M4, invert, mul};
use super::mesh::Mesh;
use super::nurbs::advanced_brep;
use super::solid::{extruded, primitive, revolved, swept_area, swept_disk};
use super::style::{Colour, DEFAULT, Styles};
use crate::model::{Id, IfcFile, id_key};

const BODY_IDS: &[&str] = &["BODY", "FACETATION", "MESH", "TESSELLATION", "SURFACE", "BODY-FALLBACK"];
const SKIP_IDS: &[&str] =
    &["AXIS", "FOOTPRINT", "BOX", "ANNOTATION", "PROFILE", "CLEARANCE", "REFERENCE", "LIGHTING", "COG", "PLAN", "SURVEYPOINTS", "TEXT"];
const SOLID_TYPES: &[&str] = &[
    "SWEPTSOLID",
    "ADVANCEDSWEPTSOLID",
    "BREP",
    "ADVANCEDBREP",
    "CSG",
    "CLIPPING",
    "SURFACEMODEL",
    "TESSELLATION",
    "SOLIDMODEL",
    "MAPPEDREPRESENTATION",
    "BOUNDINGBOX",
    "SECTIONEDSPINE",
    "ADVANCEDSURFACE",
    "SURFACE3D",
    "CURVE3D",
];
/// openings are cut from their elements and spaces are rooms (an app streams them on their own)
const DEFAULT_EXCLUDE: &[&str] = &["IFCOPENINGELEMENT", "IFCSPACE"];

/// One piece of a product: a stored mesh, the matrix placing it, its colour.
#[derive(Clone, Debug)]
pub struct Piece {
    pub geom: usize,
    pub matrix: M4,
    pub color: Colour,
}

#[derive(Clone, Debug)]
struct ItemPiece {
    geom: usize,
    matrix: M4,
    color: Option<Colour>,
}

pub struct Geometry<'f> {
    pub g: GeoCtx<'f>,
    st: Styles,
    store: Vec<Mesh>,
    item_geom: HashMap<u64, usize>,
    voids: Option<Arc<HashMap<u64, Vec<Id>>>>,
    ops: &'f dyn SolidOps,
    /// representation items and boolean operands visited (shared sub-trees are walked again each time they are met)
    work: u64,
}

/// Item and operand visits past which a file is refused: shared mapped items or boolean operands nest into DAGs whose
/// walk grows exponentially (the JS never finishes them); real files stay far below.
pub const MAX_WORK: u64 = 10_000_000;

impl<'f> Geometry<'f> {
    pub fn new(f: &'f IfcFile<'f>, ops: &'f dyn SolidOps) -> GR<Geometry<'f>> {
        Ok(Geometry { g: GeoCtx::new(f)?, st: Styles::default(), store: Vec::new(), item_geom: HashMap::new(), voids: None, ops, work: 0 })
    }

    pub fn notes(&self) -> &HashMap<String, u32> {
        &self.g.notes
    }

    /// Ids to draw (default: every product with a shape except openings and spaces), sorted.
    pub fn products(&self) -> GR<Vec<Id>> {
        let f = self.g.f;
        let s = f.schema;
        let mut skip: Vec<Box<str>> = Vec::new();
        for t in DEFAULT_EXCLUDE {
            skip.extend(s.subtypes(t).iter().cloned());
        }
        let mut out = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let roots = s.subtypes("IFCPRODUCT");
        for t in roots.iter() {
            if !seen.insert(t) || skip.contains(t) {
                continue;
            }
            for id in f.of_type(t, false) {
                if self.g.a(Some(id), "Representation")?.is_some_and(crate::value::Value::truthy)
                    && self.g.a(Some(id), "ObjectPlacement")?.is_some()
                {
                    out.push(id);
                }
            }
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// The body representations of a product (IfcShapeRepresentation ids).
    fn body_reps(&self, pid: Option<Id>) -> GR<Vec<Id>> {
        let g = &self.g;
        let pds = g.ra(pid, "Representation")?;
        if pds.is_none() {
            return Ok(Vec::new());
        }
        let reps = refs(g.a(pds, "Representations")?);
        let mut body = Vec::new();
        let mut other = Vec::new();
        for r in reps {
            let ident = str_(g.a(Some(r), "RepresentationIdentifier")?).map(|s| s.to_uppercase()).unwrap_or_default();
            let ty: String = str_(g.a(Some(r), "RepresentationType")?)
                .map(|s| s.to_uppercase())
                .unwrap_or_default()
                .chars()
                .filter(|c| !(c.is_whitespace_js() || *c == '_' || *c == '-'))
                .collect();
            let solid = SOLID_TYPES.contains(&ty.as_str());
            if BODY_IDS.contains(&ident.as_str()) {
                body.push(r);
            } else if (ident.is_empty() && solid) || (!SKIP_IDS.contains(&ident.as_str()) && solid && ty != "CURVE3D") {
                other.push(r);
            }
        }
        Ok(if body.is_empty() { other } else { body })
    }

    /// Opening meshes of an element (one per opening item), in the element's own frame; None when it has none.
    fn openings(&mut self, pid: Id, world: &M4) -> GR<Option<Vec<Mesh>>> {
        if self.voids.is_none() {
            self.voids = Some(self.g.f.inverse("IFCRELVOIDSELEMENT", "RelatingBuildingElement")?);
        }
        let rels = self.voids.as_ref().and_then(|v| v.get(&id_key(pid))).cloned().unwrap_or_default();
        if rels.is_empty() {
            return Ok(None);
        }
        let inv = invert(world).unwrap_or(I4);
        let mut out = Vec::new();
        for r in rels {
            let op = self.g.ra(Some(r), "RelatedOpeningElement")?;
            if op.is_none() {
                continue;
            }
            let pl = self.g.ra(op, "ObjectPlacement")?;
            let ow = self.g.placement(pl, 0)?;
            let to_local = mul(&inv, &ow);
            for rep in self.body_reps(op)? {
                for it in refs(self.g.a(Some(rep), "Items")?) {
                    for p in self.item(Some(it), &I4, None, 0)? {
                        let mut m = Mesh::new();
                        if let Some(src) = self.store.get(p.geom) {
                            m.append(src, Some(&mul(&to_local, &p.matrix)));
                        }
                        let m = m.check()?;
                        if !m.is_empty() {
                            out.push(m);
                        }
                    }
                }
            }
        }
        Ok(if out.is_empty() { None } else { Some(out) })
    }

    pub fn pieces(&mut self, pid: Id) -> GR<Vec<Piece>> {
        let pl = self.g.ra(Some(pid), "ObjectPlacement")?;
        let world = self.g.placement(pl, 0)?;
        let fallback = self.st.product_color(&self.g, pid)?;
        let mut raw = Vec::new();
        for rep in self.body_reps(Some(pid))? {
            for it in refs(self.g.a(Some(rep), "Items")?) {
                raw.extend(self.item(Some(it), &I4, None, 0)?);
            }
        }
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let cuts = self.openings(pid, &world)?;
        let mut out = Vec::new();
        for p in raw {
            let color = p.color.or(fallback).unwrap_or(DEFAULT);
            if let Some(cuts) = &cuts {
                // bake the piece into the element's frame and cut the openings from it
                let mut baked = Mesh::new();
                if let Some(src) = self.store.get(p.geom) {
                    baked.append(src, Some(&p.matrix));
                }
                let baked = baked.check()?;
                let cut = subtract_all(self.ops, &baked, cuts, &mut self.g).check()?;
                if cut.is_empty() && !baked.is_empty() {
                    self.g.note("An element lies wholly inside its own openings and is cut away (as the file says)");
                }
                let id = self.keep(cut);
                out.push(Piece { geom: id, matrix: world, color });
            } else {
                out.push(Piece { geom: p.geom, matrix: mul(&world, &p.matrix), color });
            }
        }
        out.retain(|p| self.store.get(p.geom).is_some_and(|m| !m.is_empty()));
        Ok(out)
    }

    fn spend(&mut self) -> GR<()> {
        self.work = self.work.saturating_add(1);
        if self.work > MAX_WORK { Err(super::ctx::GeoErr("too much work: a representation graph that never ends".into())) } else { Ok(()) }
    }

    fn keep(&mut self, mesh: Mesh) -> usize {
        self.store.push(mesh);
        self.store.len().saturating_sub(1)
    }

    pub fn mesh(&self, id: usize) -> Option<&Mesh> {
        self.store.get(id)
    }

    /// One representation item → pieces (stored mesh, matrix item → representation frame, colour).
    fn item(&mut self, id: Option<Id>, m: &M4, color: Option<Colour>, depth: u32) -> GR<Vec<ItemPiece>> {
        let Some(iid) = id else { return Ok(Vec::new()) };
        self.spend()?;
        if depth > 24 {
            return Ok(Vec::new());
        }
        let t = self.g.t(id);
        let own = self.st.item_color(&self.g, iid)?.or(color);
        if t == Some("IFCMAPPEDITEM") {
            let src = self.g.ra(id, "MappingSource")?;
            let origin = self.g.ra(src, "MappingOrigin")?;
            let om = if self.g.t(origin).is_some_and(|t| t.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR")) {
                self.g.operator(origin)?
            } else {
                self.g.axis2(origin)?
            };
            let target = self.g.ra(id, "MappingTarget")?;
            let tm = mul(m, &mul(&self.g.operator(target)?, &om));
            let rep = self.g.ra(src, "MappedRepresentation")?;
            let mut out = Vec::new();
            for it in refs(self.g.a(rep, "Items")?) {
                out.extend(self.item(Some(it), &tm, own, depth.saturating_add(1))?);
            }
            return Ok(out);
        }
        let geom = match self.item_geom.get(&id_key(iid)) {
            Some(&g) => g,
            None => {
                let mesh = match self.leaf(id, t, depth).and_then(|m| m.map(Mesh::check).transpose()) {
                    Ok(m) => m,
                    Err(e) => {
                        let name = t.and_then(|t| self.g.s.pretty(t)).unwrap_or("shape").to_string();
                        self.g.note(format!("A {name} could not be built: {e}"));
                        None
                    }
                };
                let Some(mesh) = mesh else { return Ok(Vec::new()) };
                let g = self.keep(mesh);
                self.item_geom.insert(id_key(iid), g);
                g
            }
        };
        Ok(vec![ItemPiece { geom, matrix: *m, color: own }])
    }

    /// A leaf item's mesh in its own frame (booleans resolved here), or None.
    pub fn leaf(&mut self, id: Option<Id>, t: Option<&str>, depth: u32) -> GR<Option<Mesh>> {
        let g = &mut self.g;
        Ok(Some(match t.unwrap_or("") {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => extruded(g, id)?,
            "IFCREVOLVEDAREASOLID" | "IFCREVOLVEDAREASOLIDTAPERED" => revolved(g, id)?,
            "IFCSWEPTDISKSOLID" | "IFCSWEPTDISKSOLIDPOLYGONAL" => swept_disk(g, id)?,
            "IFCSURFACECURVESWEPTAREASOLID" | "IFCFIXEDREFERENCESWEPTAREASOLID" | "IFCDIRECTRIXCURVESWEPTAREASOLID" => swept_area(g, id)?,
            "IFCBLOCK" | "IFCRIGHTCIRCULARCYLINDER" | "IFCRIGHTCIRCULARCONE" | "IFCSPHERE" | "IFCRECTANGULARPYRAMID" => primitive(g, id)?,
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" => brep(g, id)?,
            "IFCADVANCEDBREP" | "IFCADVANCEDBREPWITHVOIDS" => {
                let mut m = Mesh::new();
                advanced_brep(g, id, &mut m)?;
                m
            }
            "IFCPOLYGONALFACESET" | "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => face_set(g, id)?,
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => surface_model(g, id)?,
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => {
                let mut m = Mesh::new();
                shell_faces(g, id, &mut m)?;
                m
            }
            "IFCBOUNDINGBOX" => bounding_box(g, id)?,
            "IFCSECTIONEDSOLIDHORIZONTAL" => sectioned_solid(g, id)?,
            "IFCCSGSOLID" => {
                let root = g.ra(id, "TreeRootExpression")?;
                self.operand(root, depth.saturating_add(1))?
            }
            "IFCBOOLEANRESULT" | "IFCBOOLEANCLIPPINGRESULT" => self.boolean_result(id, depth)?,
            "IFCGEOMETRICSET"
            | "IFCGEOMETRICCURVESET"
            | "IFCPOLYLINE"
            | "IFCINDEXEDPOLYCURVE"
            | "IFCCOMPOSITECURVE"
            | "IFCTRIMMEDCURVE"
            | "IFCCARTESIANPOINT"
            | "IFCTEXTLITERAL"
            | "IFCTEXTLITERALWITHEXTENT"
            | "IFCANNOTATIONFILLAREA"
            | "IFCCIRCLE"
            | "IFCLINE"
            | "IFCSTYLEDITEM" => return Ok(None), // curves, points and text: not part of a body
            other => {
                if !other.is_empty() {
                    let p = g.pretty(other);
                    g.note(format!("{p} is not drawn yet"));
                }
                return Ok(None);
            }
        }))
    }

    /// A boolean operand: a solid, a half-space (only as a second operand), a nested boolean or a CSG primitive.
    fn operand(&mut self, id: Option<Id>, depth: u32) -> GR<Mesh> {
        self.spend()?;
        if depth > 1000 {
            // the JS has no limit of its own here; a cycle of booleans ends in a stack overflow
            return Err(super::ctx::stack_overflow());
        }
        let t = self.g.t(id);
        Ok(self.leaf(id, t, depth.saturating_add(1))?.unwrap_or_default())
    }

    fn boolean_result(&mut self, id: Option<Id>, depth: u32) -> GR<Mesh> {
        let op = enm(self.g.a(id, "Operator")?).map_or(BoolOp::Difference, BoolOp::from_enum);
        let first = self.g.ra(id, "FirstOperand")?;
        let a = self.operand(first, depth)?;
        let b_id = self.g.ra(id, "SecondOperand")?;
        let bt = self.g.tt(b_id);
        if bt.is_some_and(|bt| self.g.s.is_a(bt, "IFCHALFSPACESOLID")) {
            if op == BoolOp::Difference {
                return half_space_cut(self.ops, &mut self.g, a, b_id);
            }
            self.g.note("Only differences with half-spaces are cut");
            return Ok(a);
        }
        let b = self.operand(b_id, depth)?;
        let joined = |a: &Mesh, b: &Mesh| {
            let mut o = Mesh::new();
            o.append(a, None);
            o.append(b, None);
            o
        };
        if op == BoolOp::Union && !self.ops.loaded() {
            return Ok(joined(&a, &b));
        }
        if let Some(r) = boolean(self.ops, op, &a, &b, Some(&mut self.g)) {
            return Ok(r);
        }
        self.g.note(if self.ops.loaded() {
            "A boolean was left uncut (an operand is not a closed solid)"
        } else {
            "Booleans are not cut (no boolean engine loaded)"
        });
        Ok(if op == BoolOp::Union { joined(&a, &b) } else { a })
    }
}

trait JsSpace {
    fn is_whitespace_js(&self) -> bool;
}
impl JsSpace for char {
    /// `\s` in a JS regex: JS white space and line terminators.
    fn is_whitespace_js(&self) -> bool {
        crate::util::is_js_space(u32::from(*self))
    }
}
