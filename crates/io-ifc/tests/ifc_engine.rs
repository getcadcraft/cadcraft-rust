//! cc-ifc's own checks (ported from the 0.5 `tools/ifc-test/test.mjs` and `nurbs.mjs` at site commit 594ab3e), each
//! against a known answer. Parts that need a boolean engine (manifold-3d in the JS) wait for a real `SolidOps`; the
//! writer (formatString, save) and the properties API are not ported yet. Checks on public sample files run when
//! /var/www/CADCRAFT/testfiles is there and skip otherwise.

use cc_io_ifc::geo::bool_ops::{NoSolidOps, clip_plane, subtract_all};
use cc_io_ifc::geo::ctx::GeoCtx;
use cc_io_ifc::geo::curve::{arc3, curve_points};
use cc_io_ifc::geo::geometry::Geometry;
use cc_io_ifc::geo::mesh::Mesh;
use cc_io_ifc::geo::profile::{Shape, profile_shapes};
use cc_io_ifc::geo::solid::{extrude_shapes, extruded, primitive, revolved, swept_disk};
use cc_io_ifc::geo::tri::triangulate;
use cc_io_ifc::model::IfcFile;
use cc_io_ifc::schema::{crc32, schema_for, type_code};
use cc_io_ifc::step::{decode_string, parse_params, scan_step};
use cc_io_ifc::{JsStr, Value};

const TESTFILES: &str = "/var/www/CADCRAFT/testfiles";

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
fn rel(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs()
}

/// A tiny IFC4 file around some DATA lines (ids from 100 up are free); millimetres unless `milli` is false.
fn ifc(data: &str, schema: &str, milli: bool) -> Vec<u8> {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');\nFILE_NAME('t.ifc','2026-10-13T00:00:00',('cc-ifc test'),('getcadcraft'),'cc-ifc','cc-ifc','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'P',$,$,$,$,(#2),#3);\n#2=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);\n#3=IFCUNITASSIGNMENT((#4,#9));\n#4=IFCSIUNIT(*,.LENGTHUNIT.,{},.METRE.);\n#5=IFCAXIS2PLACEMENT3D(#6,$,$);\n#6=IFCCARTESIANPOINT((0.,0.,0.));\n#7=IFCDIRECTION((0.,0.,1.));\n#8=IFCDIRECTION((1.,0.,0.));\n#9=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);\n{data}\nENDSEC;\nEND-ISO-10303-21;\n",
        if milli { ".MILLI." } else { "$" }
    )
    .into_bytes()
}

fn p(s: &str) -> Vec<Value> {
    parse_params(s.as_bytes(), 0, i64::try_from(s.len()).unwrap()).unwrap()
}
fn s(t: &str) -> JsStr {
    decode_string(t.as_bytes(), 0, i64::try_from(t.len()).unwrap()).unwrap()
}
fn st(t: &str) -> JsStr {
    JsStr::Utf8(t.into())
}

// ---- I.1 STEP reading ----

#[test]
fn i1_tokens() {
    let v = p("('a''b',$,*,#12,.T.,(1,2.5,-3.E-2),IFCLABEL('x'),\"0FF\")");
    assert_eq!(v[0], Value::Str(st("a'b")));
    assert_eq!(v[1], Value::Null);
    assert_eq!(v[2], Value::Derived);
    assert_eq!(v[3], Value::Ref(12.0));
    assert_eq!(v[4], Value::Enum("T".into()));
    assert_eq!(v[5], Value::List(vec![Value::Num(1.0), Value::Num(2.5), Value::Num(-0.03)].into()));
    assert_eq!(v[6], Value::Typed(Box::new(("IFCLABEL".into(), Value::Str(st("x"))))));
    assert_eq!(v[7], Value::Bin("0FF".into()));
    assert_eq!(p("( 1 , /* a comment */ 2 )"), vec![Value::Num(1.0), Value::Num(2.0)], "blanks and comments between tokens");
    assert_eq!(
        p("(1.,1.E5,-5.E-2,+2.)"),
        vec![Value::Num(1.0), Value::Num(1e5), Value::Num(-0.05), Value::Num(2.0)],
        "reals \"1.\", \"1.E5\", signs"
    );
    let t = p("(IFCCOMPOUNDPLANEANGLEMEASURE((51,30,0)))");
    let Value::Typed(tv) = &t[0] else { panic!("typed") };
    assert_eq!(tv.1, Value::List(vec![Value::Num(51.0), Value::Num(30.0), Value::Num(0.0)].into()), "a typed aggregate keeps its list");
}

#[test]
fn i1_escapes() {
    assert_eq!(s("caf\\X\\E9"), st("café"), "\\X\\hh (Latin-1)");
    assert_eq!(s("\\X2\\00D8\\X0\\resund \\X2\\6587\\X0\\"), st("Øresund 文"), "\\X2\\…\\X0\\ (UTF-16)");
    assert_eq!(s("\\X4\\0001F600\\X0\\"), st("😀"), "\\X4\\…\\X0\\ (beyond the BMP)");
    assert_eq!(s("a\\\\b"), st("a\\b"));
    assert_eq!(s("\\S\\i"), st("é"));
    assert_eq!(s("ü raw"), st("ü raw"), "raw UTF-8 that exporters write is accepted");
}

#[test]
fn i1_index_and_damage() {
    let data =
        "#100=IFCWALL('2O2Fr$t4X7Zf8NOew3FNbT',$,'W ;1',$,$,$,$,$,$);\n#101=(IFCA(1)IFCB(2));\n#102 = IFCCARTESIANPOINT ( ( 1. , 2. ) ) ;";
    let b = ifc(data, "IFC4", true);
    let ix = scan_step(&b).unwrap();
    assert_eq!(ix.count, 12);
    assert_eq!(ix.schema, st("IFC4"));
    assert_eq!(ix.header.name.as_ref().unwrap()[0], Value::Str(st("t.ifc")));
    let f = IfcFile::open(&b).unwrap();
    assert_eq!(f.ty(100.0), Some("IFCWALL"));
    assert_eq!(f.params(100.0).unwrap().unwrap()[2], Value::Str(st("W ;1")), "a semicolon inside a string does not end the instance");
    assert_eq!(f.ty(101.0), Some(""));
    let parts = f.complex(101.0).unwrap().unwrap();
    assert_eq!(parts.iter().map(|p| p.ty.as_str()).collect::<Vec<_>>(), ["IFCA", "IFCB"], "complex instance read as its parts");
    assert_eq!(f.params(102.0).unwrap().unwrap()[0], Value::List(vec![Value::Num(1.0), Value::Num(2.0)].into()));
    let bad = ifc("#100=IFCWALL(;\n#101=IFCCARTESIANPOINT((1.,2.));\nnonsense;\n#102=IFCCARTESIANPOINT((3.,4.));", "IFC4", true);
    let bf = IfcFile::open(&bad).unwrap();
    assert!(bf.has(101.0) && bf.has(102.0) && bf.ix.errors.len() == 1, "a damaged file still opens: broken text listed, the rest indexed");
    let e = scan_step(b"PK\x03\x04 not a step file").unwrap_err();
    assert!(e.0.contains("ISO-10303-21"), "a file that is not STEP is refused in words");
}

#[test]
fn i1_big_file() {
    // 300 000 instances (about 20 MB)
    let mut lines = String::new();
    for i in 0..300_000u32 {
        lines.push_str(&format!("#{}=IFCCARTESIANPOINT(({}.,{:.1},0.));\n", 1000 + i, i, f64::from(i) * 0.5));
    }
    let big = ifc(&lines, "IFC4", true);
    let t0 = std::time::Instant::now();
    let f = IfcFile::open(&big).unwrap();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(f.of_type("IFCCARTESIANPOINT", false).len(), 300_001);
    let last = f.params(300_999.0).unwrap().unwrap();
    assert_eq!(last[0].as_list().unwrap()[0], Value::Num(299_999.0), "the last line read");
    assert!(ms < 3000.0, "indexed in {ms:.0} ms");
}

// ---- I.2 schema tables ----

#[test]
fn i2_schema_tables() {
    let (s4, s2, s3) = (schema_for("IFC4").0, schema_for("IFC2X3").0, schema_for("IFC4X3_ADD2").0);
    let names: Vec<&str> = s4.entity("IFCWALLSTANDARDCASE").unwrap().attrs.iter().map(|a| &*a.name).collect();
    assert_eq!(names.join(","), "GlobalId,OwnerHistory,Name,Description,ObjectType,ObjectPlacement,Representation,Tag,PredefinedType");
    assert_eq!(s2.entity("IFCWALLSTANDARDCASE").unwrap().attrs.len(), 8, "IFC2X3: no PredefinedType");
    assert!(s4.is_a("IFCWALLSTANDARDCASE", "IFCELEMENT") && !s4.is_a("IFCPROPERTYSET", "IFCELEMENT"));
    assert!(s3.name == "IFC4X3" && s3.has("IFCROADPART") && s3.has("IFCALIGNMENT") && !s4.has("IFCROADPART"));
    assert_eq!(type_code("IFCWALL"), 2391406946);
    assert_eq!(type_code("IFCLABEL"), 3258342251);
    assert_eq!(type_code("FILE_NAME"), 1390159747);
    assert_eq!(crc32("IFCSPACE"), 3856911033);
    assert!(s4.entity("IFCSIUNIT").unwrap().derived.iter().any(|d| &**d == "Dimensions"), "IfcSIUnit.Dimensions is derived");
    let t = &s4.entity("IFCCARTESIANPOINTLIST3D").unwrap().attrs[0].t;
    assert!(t.agg == 2 && &*t.base == "r" && t.tname.as_deref() == Some("IFCLENGTHMEASURE"));
}

// ---- I.3 placements, operators, grids, units, georeference ----

#[test]
fn i3_placements_and_units() {
    let b = ifc(
        "#100=IFCLOCALPLACEMENT($,#101);\n#101=IFCAXIS2PLACEMENT3D(#102,#7,#103);\n#102=IFCCARTESIANPOINT((1000.,2000.,300.));\n#103=IFCDIRECTION((0.,1.,0.));\n#110=IFCLOCALPLACEMENT(#100,#111);\n#111=IFCAXIS2PLACEMENT3D(#112,$,$);\n#112=IFCCARTESIANPOINT((500.,0.,0.));\n#120=IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM(#121,#122,#6,0.5,#7,$,2.);\n#121=IFCDIRECTION((1.,1.,0.));\n#122=IFCDIRECTION((-1.,1.,0.));",
        "IFC4",
        true,
    );
    let f = IfcFile::open(&b).unwrap();
    let mut g = GeoCtx::new(&f).unwrap();
    let w = g.placement(Some(110.0), 0).unwrap();
    assert!(near(w[12], 1000.0, 1e-9) && near(w[13], 2500.0, 1e-9) && near(w[14], 300.0, 1e-9), "placement chain");
    assert!(near(w[0], 0.0, 1e-12) && near(w[1], 1.0, 1e-12), "RefDirection (0,1,0) turns x onto y");
    assert!(g.length_unit == 0.001 && g.angle_unit == 1.0, "millimetres, radians");
    let op = g.operator(Some(120.0)).unwrap();
    let h = |a: f64, b: f64, c: f64| (a * a + b * b + c * c).sqrt();
    assert!(
        near(h(op[0], op[1], op[2]), 0.5, 1e-12) && near(h(op[4], op[5], op[6]), 0.5, 1e-12) && near(h(op[8], op[9], op[10]), 2.0, 1e-12)
    );
    assert!(near(op[0], std::f64::consts::FRAC_1_SQRT_2 * 0.5, 1e-12) && near(op[1], std::f64::consts::FRAC_1_SQRT_2 * 0.5, 1e-12));
    // conversion-based units: foot, degree
    let ft = String::from_utf8(ifc("#100=IFCCONVERSIONBASEDUNIT(#101,.LENGTHUNIT.,'FOOT',#102);\n#101=IFCDIMENSIONALEXPONENTS(1,0,0,0,0,0,0);\n#102=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE(0.3048),#103);\n#103=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#104=IFCCONVERSIONBASEDUNIT(#101,.PLANEANGLEUNIT.,'DEGREE',#105);\n#105=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.0174532925199433),#9);", "IFC4", true))
        .unwrap()
        .replace("#3=IFCUNITASSIGNMENT((#4,#9));", "#3=IFCUNITASSIGNMENT((#100,#104));");
    let ff = IfcFile::open(ft.as_bytes()).unwrap();
    let gf = GeoCtx::new(&ff).unwrap();
    assert!(near(gf.length_unit, 0.3048, 1e-12) && near(gf.angle_unit, std::f64::consts::PI / 180.0, 1e-12));
}

fn georef_of(schema: &str, conv: &str) -> cc_io_ifc::geo::ctx::GeoRef {
    let text = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('g.ifc','',(''),(''),'','','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n#1=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#2,$);\n#2=IFCAXIS2PLACEMENT3D(#3,$,$);\n#3=IFCCARTESIANPOINT((0.,0.,0.));\n#4=IFCPROJECTEDCRS('EPSG:28356',$,$,$,$,$,$);\n#5={conv};\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    let f = IfcFile::open(text.as_bytes()).unwrap();
    let g = GeoCtx::new(&f).unwrap();
    g.georef().unwrap().unwrap()
}

#[test]
fn i3_georeference() {
    let a = georef_of("IFC4", "IFCMAPCONVERSION(#1,#4,300000.,6000000.,40.,1.,0.,0.001)");
    assert!(near(a.matrix[0], 0.001, 1e-15) && near(a.matrix[5], 0.001, 1e-15) && near(a.matrix[10], 0.001, 1e-15) && a.matrix[14] == 40.0);
    let b = georef_of("IFC4", "IFCMAPCONVERSION(#1,#4,300000.,6000000.,40.,1.,0.,$)");
    assert!(b.matrix[0] == 1.0 && b.matrix[10] == 1.0);
    let c = georef_of("IFC4X3_ADD2", "IFCMAPCONVERSIONSCALED(#1,#4,300000.,6000000.,40.,1.,0.,0.001,0.001,0.002,0.5)");
    assert!(near(c.matrix[5], 0.002, 1e-15) && near(c.matrix[10], 0.5, 1e-15));
    assert_eq!(c.crs.unwrap().name.as_deref(), Some("EPSG:28356"));
}

fn spec(p: &str) -> Option<Vec<u8>> {
    let f = format!("{TESTFILES}/ifc-corpus/IFC4.x-specification-models-main/models/{p}");
    match std::fs::read(&f) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("(skipped: {f} is not on this machine)");
            None
        }
    }
}

#[test]
fn i3_spec_models() {
    if let Some(b) = spec("placements-and-gis-referencing/grid-placement/grid-placement.ifc") {
        let f = IfcFile::open(&b).unwrap();
        let mut g = GeoCtx::new(&f).unwrap();
        let pl = g.ra(Some(293.0), "ObjectPlacement").unwrap();
        let m = g.placement(pl, 0).unwrap();
        assert!(near(m[12], -14000.0, 1e-6) && near(m[13], 4000.0, 1e-6), "grid placement: column CRE-001 on E/1");
    }
    if let Some(b) = spec("alignment-geometries-and-linear-positioning/linear-placement-of-signal/linear-placement-of-signal.ifc") {
        let f = IfcFile::open(&b).unwrap();
        let mut g = GeoCtx::new(&f).unwrap();
        let (mut worst, mut n) = (0.0f64, 0);
        for lp in f.of_type("IFCLINEARPLACEMENT", false) {
            let cp = g.ra(Some(lp), "CartesianPosition").unwrap();
            if cp.is_none() {
                continue;
            }
            let rp = g.ra(Some(lp), "RelativePlacement").unwrap();
            let m = cc_io_ifc::geo::align::linear_placement(&mut g, rp).unwrap().unwrap();
            let c = g.axis2(cp).unwrap();
            worst = worst.max(((m[12] - c[12]).powi(2) + (m[13] - c[13]).powi(2) + (m[14] - c[14]).powi(2)).sqrt());
            n += 1;
        }
        assert!(n == 25 && worst < 0.003, "25 referents within {:.1} mm of the cached positions", worst * 1000.0);
    }
    if let Some(b) = spec("placements-and-gis-referencing/geographic-referencing-utm/geographic-referencing-utm.ifc") {
        let f = IfcFile::open(&b).unwrap();
        let r = GeoCtx::new(&f).unwrap().georef().unwrap().unwrap();
        assert_eq!(r.crs.unwrap().name.as_deref(), Some("EPSG:25832"));
        assert!(r.eastings == 458657.3 && r.northings == 5438232.25 && near(r.rotation, 0.962691746426f64.atan2(0.270600445976), 1e-12));
    }
}

// ---- I.4 curves and profiles ----

fn ring_area(r: &[[f64; 2]]) -> f64 {
    let mut t = 0.0;
    for i in 0..r.len() {
        let (p, q) = (r[i], r[(i + 1) % r.len()]);
        t += p[0] * q[1] - q[0] * p[1];
    }
    t / 2.0
}
fn area(shapes: &[Shape]) -> f64 {
    shapes.iter().map(|s| ring_area(&s.outer) + s.holes.iter().map(|h| ring_area(h)).sum::<f64>()).sum()
}
fn prof(line: &str) -> Vec<Shape> {
    let b = ifc(&format!("#100={line};\n#101=IFCAXIS2PLACEMENT2D(#102,$);\n#102=IFCCARTESIANPOINT((0.,0.));"), "IFC4", true);
    let f = IfcFile::open(&b).unwrap();
    let mut g = GeoCtx::new(&f).unwrap();
    profile_shapes(&mut g, Some(100.0), 0).unwrap()
}

#[test]
fn i4_profiles() {
    use std::f64::consts::PI;
    assert!(near(area(&prof("IFCRECTANGLEPROFILEDEF(.AREA.,$,#101,300.,200.)")), 60000.0, 1e-6));
    assert!(rel(area(&prof("IFCCIRCLEPROFILEDEF(.AREA.,$,#101,100.)")), PI * 1e4, 0.006));
    assert!(rel(area(&prof("IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,#101,100.,10.)")), PI * (1e4 - 8100.0), 0.006));
    let ipe = area(&prof("IFCISHAPEPROFILEDEF(.AREA.,'IPE220',#101,110.,220.,5.9,9.2,12.,$,$)"));
    assert!(
        rel(ipe, 2.0 * 110.0 * 9.2 + (220.0 - 18.4) * 5.9 + (4.0 - PI) * 144.0, 0.002) && near(ipe / 100.0, 33.4, 0.1),
        "IPE 220: {ipe}"
    );
    let hea = area(&prof("IFCISHAPEPROFILEDEF(.AREA.,'HEA300',#101,300.,290.,8.5,14.,27.,$,$)"));
    assert!(near(hea / 100.0, 112.5, 0.3), "HE 300 A: {hea}");
    let t = area(&prof("IFCTSHAPEPROFILEDEF(.AREA.,$,#101,150.,150.,7.1,10.7,15.,$,$,$,$)"));
    assert!(rel(t, 150.0 * 10.7 + (150.0 - 10.7) * 7.1 + (2.0 - PI / 2.0) * 225.0, 0.002));
    assert!(near(area(&prof("IFCLSHAPEPROFILEDEF(.AREA.,$,#101,100.,80.,10.,$,$,$)")), 1700.0, 1e-6));
    assert!(near(area(&prof("IFCUSHAPEPROFILEDEF(.AREA.,$,#101,200.,75.,8.5,11.5,$,$,$)")), 200.0 * 8.5 + 2.0 * (75.0 - 8.5) * 11.5, 1e-6));
    assert!(near(area(&prof("IFCZSHAPEPROFILEDEF(.AREA.,$,#101,200.,75.,8.,10.,$,$)")), 200.0 * 8.0 + 2.0 * 67.0 * 10.0, 1e-6));
    assert!(near(
        area(&prof("IFCCSHAPEPROFILEDEF(.AREA.,$,#101,200.,70.,3.,20.,$)")),
        2.0 * 70.0 * 3.0 + 194.0 * 3.0 + 2.0 * 17.0 * 3.0,
        1e-6
    ));
    assert!(near(area(&prof("IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,#101,200.,100.,10.,$,$)")), 200.0 * 100.0 - 180.0 * 80.0, 1e-6));
    assert!(near(area(&prof("IFCTRAPEZIUMPROFILEDEF(.AREA.,$,#101,400.,200.,100.,50.)")), 30000.0, 1e-6));
    assert!(rel(area(&prof("IFCROUNDEDRECTANGLEPROFILEDEF(.AREA.,$,#101,300.,200.,20.)")), 60000.0 - (4.0 - PI) * 400.0, 0.001));
}

#[test]
fn i4_arbitrary_and_mirrored() {
    let b = ifc(
        "#100=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#101,(#105));\n#101=IFCPOLYLINE((#110,#111,#112,#113,#110));\n#105=IFCPOLYLINE((#114,#115,#116,#117,#114));\n#110=IFCCARTESIANPOINT((0.,0.));\n#111=IFCCARTESIANPOINT((100.,0.));\n#112=IFCCARTESIANPOINT((100.,100.));\n#113=IFCCARTESIANPOINT((0.,100.));\n#114=IFCCARTESIANPOINT((20.,20.));\n#115=IFCCARTESIANPOINT((40.,20.));\n#116=IFCCARTESIANPOINT((40.,40.));\n#117=IFCCARTESIANPOINT((20.,40.));\n#120=IFCMIRROREDPROFILEDEF(.AREA.,$,#100,$,$);",
        "IFC4",
        true,
    );
    let f = IfcFile::open(&b).unwrap();
    let mut g = GeoCtx::new(&f).unwrap();
    assert!(near(area(&profile_shapes(&mut g, Some(100.0), 0).unwrap()), 9600.0, 1e-9));
    let mir = profile_shapes(&mut g, Some(120.0), 0).unwrap();
    assert!(
        near(area(&mir), 9600.0, 1e-9) && mir[0].outer.iter().all(|p| p[0] <= 0.0),
        "mirrored: same area, other side, counter-clockwise"
    );
}

#[test]
fn i4_trimmed_circles_and_arcs() {
    for k in ["degrees", "radians"] {
        let Some(b) = spec(&format!("trimmed-curve-parameters/curve-parameters-in-{k}/curve-parameters-in-{k}.ifc")) else { continue };
        let f = IfcFile::open(&b).unwrap();
        let mut g = GeoCtx::new(&f).unwrap();
        let tc = f
            .of_type("IFCTRIMMEDCURVE", false)
            .into_iter()
            .find(|&id| {
                let bc = g.ra(Some(id), "BasisCurve").unwrap();
                f.ty(bc.unwrap_or(-1.0)) == Some("IFCCIRCLE") && g.na(bc, "Radius", f64::NAN).unwrap() == 1000.0
            })
            .unwrap();
        let bc = g.ra(Some(tc), "BasisCurve").unwrap();
        let pos = g.ra(bc, "Position").unwrap();
        let cm = g.axis2(pos).unwrap();
        let pts = curve_points(&mut g, Some(tc), 0).unwrap();
        let c = [cm[12], cm[13], cm[14]];
        let d = |p: &[f64; 3]| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt();
        let ang = |p: &[f64; 3]| ((p[1] - c[1]).atan2(p[0] - c[0]).to_degrees() + 360.0) % 360.0;
        let (a, z) = (pts[0], *pts.last().unwrap());
        assert!(pts.len() > 10 && pts.iter().all(|p| near(d(p), 1000.0, 1e-6)), "{k}: on the circle");
        assert!(
            near(((a[0] - z[0]).powi(2) + (a[1] - z[1]).powi(2)).sqrt(), 2000.0, 1e-6)
                && near(ang(&a), 315.0, 1e-6)
                && near(ang(&z), 135.0, 1e-6),
            "{k}: 315° → 135°"
        );
    }
    let a = arc3(Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0]), Some([-1.0, 0.0, 0.0])).unwrap();
    assert!(a.len() > 10 && a.iter().all(|p| near(p[0].hypot(p[1]), 1.0, 1e-9) && p[1] >= -1e-12), "arc through three points");
    let tri = triangulate(
        &[[0., 0.], [10., 0.], [10., 5.], [8., 5.], [8., 1.], [6., 1.], [6., 5.], [4., 5.], [4., 1.], [2., 1.], [2., 5.], [0., 5.]],
        &[],
    );
    assert_eq!(tri.len() / 3, 10, "ear clipping a comb: 12 corners → 10 triangles");
}

// ---- I.5 swept solids ----

#[test]
fn i5_swept_solids() {
    use std::f64::consts::PI;
    let b = ifc(
        "#100=IFCEXTRUDEDAREASOLID(#101,#5,#7,2000.);\n#101=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,300.,200.);\n#110=IFCREVOLVEDAREASOLID(#111,#5,#112,1.5707963267949);\n#111=IFCRECTANGLEPROFILEDEF(.AREA.,$,#114,100.,100.);\n#112=IFCAXIS1PLACEMENT(#113,#115);\n#113=IFCCARTESIANPOINT((0.,0.,0.));\n#114=IFCAXIS2PLACEMENT2D(#116,$);\n#115=IFCDIRECTION((0.,1.,0.));\n#116=IFCCARTESIANPOINT((500.,0.));\n#120=IFCSWEPTDISKSOLID(#121,10.,$,$,$);\n#121=IFCPOLYLINE((#122,#123,#124));\n#122=IFCCARTESIANPOINT((0.,0.,0.));\n#123=IFCCARTESIANPOINT((1000.,0.,0.));\n#124=IFCCARTESIANPOINT((1000.,1000.,0.));\n#130=IFCEXTRUDEDAREASOLID(#101,#5,#131,1000.);\n#131=IFCDIRECTION((0.,0.,-1.));\n#140=IFCRIGHTCIRCULARCYLINDER(#5,1000.,100.);\n#141=IFCBLOCK(#5,100.,200.,300.);\n#142=IFCSPHERE(#5,100.);\n#150=IFCSWEPTDISKSOLID(#121,10.,6.,$,$);",
        "IFC4",
        true,
    );
    let f = IfcFile::open(&b).unwrap();
    let mut g = GeoCtx::new(&f).unwrap();
    assert!(near(extruded(&mut g, Some(100.0)).unwrap().volume(), 300.0 * 200.0 * 2000.0, 1e-3));
    assert!(near(extruded(&mut g, Some(130.0)).unwrap().volume(), 300.0 * 200.0 * 1000.0, 1e-3), "downward: faces still outward");
    assert!(rel(revolved(&mut g, Some(110.0)).unwrap().volume(), 100.0 * 100.0 * 500.0 * PI / 2.0, 0.01), "Pappus");
    assert!(rel(swept_disk(&mut g, Some(120.0)).unwrap().volume(), PI * 100.0 * 2000.0, 0.02));
    assert!(rel(swept_disk(&mut g, Some(150.0)).unwrap().volume(), PI * (100.0 - 36.0) * 2000.0, 0.02), "hollow swept disk");
    assert!(rel(primitive(&mut g, Some(140.0)).unwrap().volume(), PI * 1e4 * 1000.0, 0.006));
    assert!(near(primitive(&mut g, Some(141.0)).unwrap().volume(), 100.0 * 200.0 * 300.0, 1e-3));
    assert!(rel(primitive(&mut g, Some(142.0)).unwrap().volume(), 4.0 / 3.0 * PI * 1e6, 0.03));
}

// ---- I.6 breps and face sets ----

#[test]
fn i6_breps_and_face_sets() {
    let pts = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
    let faces = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
    let c = |p: &[i32; 3]| p.iter().map(|x| format!("{}.", x * 1000)).collect::<Vec<_>>().join(",");
    let mut data = String::new();
    for (i, p) in pts.iter().enumerate() {
        data += &format!("#{}=IFCCARTESIANPOINT(({}));\n", 200 + i, c(p));
    }
    for (k, fc) in faces.iter().enumerate() {
        let refs = fc.iter().map(|i| format!("#{}", 200 + i)).collect::<Vec<_>>().join(",");
        data += &format!(
            "#{}=IFCPOLYLOOP(({refs}));\n#{}=IFCFACEOUTERBOUND(#{},.T.);\n#{}=IFCFACE((#{}));\n",
            300 + k,
            310 + k,
            300 + k,
            320 + k,
            310 + k
        );
    }
    data += &format!(
        "#330=IFCCLOSEDSHELL(({}));\n#331=IFCFACETEDBREP(#330);\n",
        (0..6).map(|k| format!("#{}", 320 + k)).collect::<Vec<_>>().join(",")
    );
    data += &format!("#340=IFCCARTESIANPOINTLIST3D(({}));\n", pts.iter().map(|p| format!("({})", c(p))).collect::<Vec<_>>().join(","));
    let tris: Vec<String> = faces
        .iter()
        .flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]])
        .map(|t| format!("({},{},{})", t[0] + 1, t[1] + 1, t[2] + 1))
        .collect();
    data += &format!("#341=IFCTRIANGULATEDFACESET(#340,$,.T.,({}),$);\n", tris.join(","));
    for (k, fc) in faces.iter().enumerate() {
        data +=
            &format!("#{}=IFCINDEXEDPOLYGONALFACE(({}));\n", 350 + k, fc.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(","));
    }
    data += &format!("#360=IFCPOLYGONALFACESET(#340,.T.,({}),$);\n", (0..6).map(|k| format!("#{}", 350 + k)).collect::<Vec<_>>().join(","));
    let b = ifc(&data, "IFC4", true);
    let f = IfcFile::open(&b).unwrap();
    let mut geo = Geometry::new(&f, &NoSolidOps).unwrap();
    for (id, name) in [(331.0, "faceted brep"), (341.0, "triangulated face set"), (360.0, "polygonal face set")] {
        let m = geo.leaf(Some(id), f.ty(id), 0).unwrap().unwrap();
        assert!(near(m.volume(), 1e9, 1e-3) && m.triangles() == 12, "{name}: a 1 m cube, 12 triangles, outward");
    }
    if let Some(b) = spec("tessellated-shape/polygonal-face-tessellation/polygonal-face-tessellation.ifc") {
        let f = IfcFile::open(&b).unwrap();
        let mut g = Geometry::new(&f, &NoSolidOps).unwrap();
        let id = g.products().unwrap()[0];
        assert!(!g.pieces(id).unwrap().is_empty(), "buildingSMART polygonal face set sample drawn");
    }
}

// ---- I.7 openings and half-spaces (the parts that need no boolean engine) ----

fn bx(x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64) -> Mesh {
    let mut m = extrude_shapes(
        &[Shape { outer: vec![[x, y], [x + dx, y], [x + dx, y + dy], [x, y + dy]], holes: vec![], open: false }],
        &[0.0, 0.0, 1.0],
        dz,
        None,
    );
    m.transform(&[1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., z, 1.]);
    m
}

#[test]
fn i7_half_spaces_and_convex_openings() {
    let half = clip_plane(&bx(0.0, 0.0, 0.0, 1.0, 1.0, 1.0), &[0.0, 0.0, 0.25], &[0.0, 0.0, 1.0]);
    assert!(near(half.volume(), 0.75, 1e-9), "half-space clip keeps 0.75 of a unit cube and caps the cut");
    let slant = clip_plane(&bx(0.0, 0.0, 0.0, 1.0, 1.0, 1.0), &[0.5, 0.5, 0.5], &[1.0, 1.0, 1.0]);
    assert!(near(slant.volume(), 0.5, 1e-9), "a cube cut through its centre on the diagonal");
    // a broken mesh (two copies of one face): openings are still cut from the triangles
    let mut broken = bx(0.0, 0.0, 0.0, 4000.0, 200.0, 3000.0);
    let mut face = vec![[0.0, 0.0, 0.0], [4000.0, 0.0, 0.0], [4000.0, 0.0, 3000.0], [0.0, 0.0, 3000.0]];
    face.reverse();
    broken.polygon(&[face]);
    let hole = bx(1000.0, -100.0, 500.0, 1000.0, 400.0, 1500.0);
    let b = ifc("", "IFC4", true);
    let f = IfcFile::open(&b).unwrap();
    let mut g = GeoCtx::new(&f).unwrap();
    let bc = subtract_all(&NoSolidOps, &broken, std::slice::from_ref(&hole), &mut g);
    assert!(bc.triangles() > broken.triangles() && bc.bounds().unwrap().1[0] == 4000.0, "convex openings cut from the triangles");
    let b = ifc(
        "#100=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#101,#102);\n#101=IFCEXTRUDEDAREASOLID(#103,#5,#7,1000.);\n#103=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.);\n#102=IFCHALFSPACESOLID(#104,.F.);\n#104=IFCPLANE(#105);\n#105=IFCAXIS2PLACEMENT3D(#106,$,$);\n#106=IFCCARTESIANPOINT((0.,0.,600.));",
        "IFC4",
        true,
    );
    let f = IfcFile::open(&b).unwrap();
    let mut geo = Geometry::new(&f, &NoSolidOps).unwrap();
    let m = geo.leaf(Some(100.0), Some("IFCBOOLEANCLIPPINGRESULT"), 0).unwrap().unwrap();
    assert!(near(m.volume(), 1000.0 * 1000.0 * 600.0, 1.0), "IfcHalfSpaceSolid (AgreementFlag .F.) cuts a 1 m cube at 600 mm");
}

#[test]
fn i7_alignment_solids() {
    for file in ["fixed-reference-swept-area-solid", "sectioned-solid-horizontal"] {
        let Some(b) = spec(&format!("alignment-geometries-and-linear-positioning/{file}/{file}.ifc")) else { continue };
        let f = IfcFile::open(&b).unwrap();
        let mut g = Geometry::new(&f, &NoSolidOps).unwrap();
        let prod = g.products().unwrap().into_iter().find(|&x| f.ty(x) == Some("IFCBUILTELEMENT")).unwrap();
        let pcs = g.pieces(prod).unwrap();
        let m = g.mesh(pcs[0].geom).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        if file.starts_with("fixed") {
            assert!(near(hi[0] - lo[0], 300.0, 1.0), "{file}: drawn from station 300 m to 600 m");
        } else {
            assert!(m.volume() > 0.0, "{file}: drawn between its two sections");
        }
    }
}

// ---- nurbs.mjs: curved faces against the exporters' own tessellation ----

struct Measure {
    lo: [f64; 3],
    hi: [f64; 3],
    area: f64,
}

fn measure(path: &std::path::Path) -> Option<Measure> {
    let b = std::fs::read(path).ok()?;
    let f = IfcFile::open(&b).ok()?;
    let mut geo = Geometry::new(&f, &NoSolidOps).ok()?;
    let mut all = Mesh::new();
    for p in geo.products().ok()? {
        for pc in geo.pieces(p).ok()? {
            let m = geo.mesh(pc.geom)?.clone();
            all.append(&m, Some(&pc.matrix));
        }
    }
    let u = geo.g.length_unit;
    let (lo, hi) = all.bounds()?;
    let mut area = 0.0;
    for t in all.idx.chunks(3) {
        let v = |k: u32| [all.pos[k as usize * 3], all.pos[k as usize * 3 + 1], all.pos[k as usize * 3 + 2]];
        let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
        let (x1, y1, z1, x2, y2, z2) = (b[0] - a[0], b[1] - a[1], b[2] - a[2], c[0] - a[0], c[1] - a[1], c[2] - a[2]);
        area += ((y1 * z2 - z1 * y2).powi(2) + (z1 * x2 - x1 * z2).powi(2) + (x1 * y2 - y1 * x2).powi(2)).sqrt() / 2.0;
    }
    Some(Measure { lo: lo.map(|x| x * u), hi: hi.map(|x| x * u), area: area * u * u })
}

#[test]
fn nurbs_pairs_against_their_facets() {
    let dir = std::path::PathBuf::from(format!("{TESTFILES}/ifc-corpus/community/IFC 4.0.2.1 (IFC 4)/NURBS"));
    if !dir.is_dir() {
        eprintln!("(skipped: {} is not on this machine)", dir.display());
        return;
    }
    let mut pairs = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.to_string_lossy().ends_with(".facet.ifc") {
                let src = std::path::PathBuf::from(p.to_string_lossy().replace(".facet.ifc", ".ifc"));
                if src.exists() {
                    pairs.push((src, p));
                }
            }
        }
    }
    pairs.sort();
    // files where the exporter's own data is at fault for part of the shape (checked by hand in the JS suite)
    let loose = |name: &str| if name.ends_with("Bentley Building Designer/Faces/bspline surface.ifc") { 0.06 } else { 0.03 };
    let mut failures = Vec::new();
    for (src, fac) in &pairs {
        let name = src.strip_prefix(&dir).unwrap().to_string_lossy().to_string();
        // a CSG difference: needs a boolean engine (manifold-3d in the JS suite); NoSolidOps leaves it uncut
        if name.contains("InCSG") {
            continue;
        }
        let (Some(a), Some(b)) = (measure(src), measure(fac)) else {
            failures.push(format!("{name}: nothing drawn"));
            continue;
        };
        let size = (0..3).map(|k| b.hi[k] - b.lo[k]).fold(0.0, f64::max);
        let dev = (0..3).flat_map(|k| [(a.lo[k] - b.lo[k]).abs(), (a.hi[k] - b.hi[k]).abs()]).fold(0.0, f64::max);
        let d_area = (a.area - b.area).abs() / if b.area != 0.0 { b.area } else { 1.0 };
        if dev > size * 0.02 {
            failures.push(format!("{name}: box {:.2} %", 100.0 * dev / size));
        }
        if d_area > loose(&name) {
            failures.push(format!("{name}: area {:.2} %", 100.0 * d_area));
        }
    }
    assert!(!pairs.is_empty() && failures.is_empty(), "{failures:#?}");
}

// ---- hostile files (fuzz finds and the classes around them) ----

#[test]
fn mapped_items_that_fan_out_exponentially_are_refused() {
    // level k: a representation with three mapped items of level k + 1 (3^25 visits: the JS never finishes)
    let mut d = String::from(
        "#20=IFCBUILDINGELEMENTPROXY('2O2Fr$t4X7Zf8NOew3FNbV',$,'B',$,$,#10,#21,$,$);\n#10=IFCLOCALPLACEMENT($,#5);\n#21=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));\n#22=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#1000));\n",
    );
    for k in 0..26 {
        let (item, map, rep) = (1000 + k * 3, 1001 + k * 3, 1002 + k * 3);
        let next = item + 3;
        d += &format!(
            "#{item}=IFCMAPPEDITEM(#{map},#{});\n#{map}=IFCREPRESENTATIONMAP(#5,#{rep});\n#{rep}=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#{next},#{next},#{next}));\n",
            900
        );
    }
    d += "#900=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#6,$,$);\n";
    let b = ifc(&d, "IFC4", true);
    let t0 = std::time::Instant::now();
    assert!(cc_io_ifc::engine_geom::products(&b, &NoSolidOps).is_err());
    assert!(t0.elapsed().as_secs() < 60);
}

#[test]
fn a_truncated_data_section_is_refused_not_looped() {
    // fuzz find (ifc_read, timeout 2026-10-16): after ENDSEC a last "DATA" without ";" sends the JS back to byte 0 forever
    let b = b"ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCWALL($);\nENDSEC;\nDATA\n";
    assert!(scan_step(b).is_err());
    assert!(cc_io_ifc::engine_geom::products(b, &NoSolidOps).is_err());
}
