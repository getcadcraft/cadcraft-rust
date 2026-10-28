//! The one-pass GeoJSON reader (geojson::read_geojson over gjparse) against the tree reader it replaced
//! (read_geojson_tree over serde_json): the same layers, features, fields, notes and errors on generated texts — odd
//! shapes, odd values, broken syntax — and gjparse's JSON values against serde_json's.

use cc_io_civil::{geojson, gjparse};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const NUMS: &[&str] = &[
    "0",
    "-0",
    "1",
    "-1",
    "12.5",
    "-97.962375564",
    "30.000164143",
    "1e3",
    "1E-7",
    "-0.0",
    "1.7976931348623157e308",
    "5e-324",
    "123456789012345678901234567890",
    "-9223372036854775808",
    "18446744073709551615",
    "0.1",
    "2.5e+2",
];
const SCALARS: &[&str] = &["null", "true", "false", "\"\"", "\"x\"", "{}", "[]", "\"2026-10-17\"", "\"a\\u00e9\\n\"", "\"\\ud83d\\ude00\""];
const TYPES: &[&str] = &[
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
    "GeometryCollection",
    "Feature",
    "FeatureCollection",
    "point",
    "",
];

fn num(r: &mut Rng) -> String {
    // past the double range now and then (the whole text is refused)
    if r.below(3000) == 0 { r.pick(&["1e400", "-1e400"]).to_string() } else { r.pick(NUMS).to_string() }
}
fn pos(r: &mut Rng) -> String {
    let n = match r.below(10) {
        0 => 1,
        1 => 5,
        2 | 3 => 3,
        4 => 4,
        _ => 2,
    };
    let items: Vec<String> = (0..n)
        .map(|i| match r.below(14) {
            0 if i >= 2 => "null".into(),
            1 if r.below(4) == 0 => r.pick(SCALARS).to_string(),
            _ => num(r),
        })
        .collect();
    format!("[{}]", items.join(","))
}
fn list(r: &mut Rng, f: &mut dyn FnMut(&mut Rng) -> String, max: usize) -> String {
    let n = r.below(max + 1);
    let items: Vec<String> = (0..n).map(|_| f(r)).collect();
    format!("[{}]", items.join(","))
}
fn coords(r: &mut Rng, ty: &str) -> String {
    if r.below(12) == 0 {
        return r.pick(SCALARS).to_string();
    }
    match ty {
        "Point" => pos(r),
        "MultiPoint" | "LineString" => list(r, &mut pos, 5),
        "MultiLineString" | "Polygon" => {
            if r.below(3) == 0 {
                // a closed ring
                let a = pos(r);
                let mid: Vec<String> = (0..3).map(|_| pos(r)).collect();
                format!("[[{a},{},{a}]]", mid.join(","))
            } else {
                list(r, &mut |r| list(r, &mut pos, 5), 3)
            }
        }
        _ => list(r, &mut |r| list(r, &mut |r| list(r, &mut pos, 5), 2), 3),
    }
}
fn geometry(r: &mut Rng, depth: u32) -> String {
    match r.below(16) {
        0 => return r.pick(SCALARS).to_string(),
        1 => return format!("[{}]", num(r)),
        _ => {}
    }
    let ty = r.pick(TYPES);
    let mut members = vec![format!("\"type\":\"{ty}\"")];
    if ty == "GeometryCollection" && depth < 7 {
        members.push(format!("\"geometries\":{}", list(r, &mut |r| geometry(r, depth + 1), 3)));
    } else {
        members.push(format!("\"coordinates\":{}", coords(r, ty)));
    }
    if r.below(8) == 0 {
        members.push("\"type\":7".into());
    }
    if r.below(8) == 0 {
        members.push(format!("\"bbox\":[{}]", num(r)));
    }
    if r.below(2) == 0 {
        members.reverse();
    }
    format!("{{{}}}", members.join(","))
}
fn properties(r: &mut Rng) -> String {
    if r.below(10) == 0 {
        return r.pick(SCALARS).to_string();
    }
    const KEYS: &[&str] = &["NAME", "ELEV", "kind", "1", "0", "10", "01", "a\\u0001b", "", "\\u0007", "x y", "ELEV", "code"];
    let n = r.below(6);
    let items: Vec<String> = (0..n)
        .map(|_| {
            let k = r.pick(KEYS);
            let v = match r.below(4) {
                0 => num(r),
                1 => format!("[{},{}]", num(r), r.pick(SCALARS)),
                2 => "\"tab\\there\\u0002\"".into(),
                _ => r.pick(SCALARS).to_string(),
            };
            format!("\"{k}\":{v}")
        })
        .collect();
    format!("{{{}}}", items.join(","))
}
fn feature(r: &mut Rng) -> String {
    match r.below(20) {
        0 => return r.pick(SCALARS).to_string(),
        1 => return "[1,2]".into(),
        _ => {}
    }
    let mut members = vec!["\"type\":\"Feature\"".to_string()];
    if r.below(10) != 0 {
        members.push(format!("\"properties\":{}", properties(r)));
    }
    if r.below(12) != 0 {
        members.push(format!("\"geometry\":{}", if r.below(10) == 0 { "null".into() } else { geometry(r, 0) }));
    }
    if r.below(2) == 0 {
        members.reverse();
    }
    format!("{{{}}}", members.join(","))
}
fn document(r: &mut Rng) -> String {
    match r.below(12) {
        0 => feature(r),
        1 => geometry(r, 0),
        2 => r.pick(SCALARS).to_string(),
        3 => format!("[{}]", feature(r)),
        _ => {
            let mut members = vec![
                "\"type\":\"FeatureCollection\"".to_string(),
                format!("\"features\":{}", if r.below(15) == 0 { "{}".into() } else { list(r, &mut feature, 6) }),
            ];
            match r.below(6) {
                0 => members.push("\"crs\":{\"type\":\"name\",\"properties\":{\"name\":\"urn:ogc:def:crs:EPSG::2277\"}}".into()),
                1 => members.push("\"crs\":{\"type\":\"EPSG\",\"properties\":{\"code\":4326}}".into()),
                2 => members.push("\"crs\":{\"properties\":{\"name\":\"weird\"}}".into()),
                _ => {}
            }
            if r.below(10) == 0 {
                members.push("\"features\":[]".into());
            }
            if r.below(2) == 0 {
                members.reverse();
            }
            format!("{{{}}}", members.join(","))
        }
    }
}

/// A text broken in a small way (a byte dropped, doubled or swapped for a JSON-significant one).
fn mutate(r: &mut Rng, s: &str) -> String {
    let mut b: Vec<u8> = s.as_bytes().to_vec();
    for _ in 0..1 + r.below(2) {
        if b.is_empty() {
            break;
        }
        let i = r.below(b.len());
        const SIG: &[u8] = b"{}[],:\"\\0-.e \n\x01";
        match r.below(3) {
            0 => {
                b.remove(i);
            }
            1 => b.insert(i, b[i]),
            _ => b[i] = SIG[r.below(SIG.len())],
        }
    }
    String::from_utf8_lossy(&b).into_owned()
}

/// Both readers agree; true when they read layers.
fn same(text: &str, max: usize) -> bool {
    let a = geojson::read_geojson(text, "t", max);
    let b = geojson::read_geojson_tree(text, "t", max);
    match (&a, &b) {
        (Ok(a), Ok(b)) => assert_eq!(format!("{a:?}"), format!("{b:?}"), "{text}"),
        (Err(a), Err(b)) => assert_eq!(a.0, b.0, "{text}"),
        _ => panic!("one reader failed: {text}\n fast {a:?}\n tree {b:?}"),
    }
    let v = gjparse::parse_value(text).ok();
    let w = serde_json::from_str::<serde_json::Value>(text).ok();
    assert_eq!(v, w, "{text}");
    a.is_ok_and(|a| !a.layers.is_empty())
}

#[test]
fn one_pass_reader_matches_the_tree_reader() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut read, mut broken_read) = (0, 0);
    for i in 0..30_000 {
        let d = document(&mut r);
        let max = if i % 7 == 0 { 2 } else { usize::MAX };
        read += usize::from(same(&d, max));
        broken_read += usize::from(same(&mutate(&mut r, &d), max));
    }
    // the generator reaches both sides: texts with layers, and broken texts (most refused, some still read)
    assert!(read > 5_000 && broken_read > 1_000 && broken_read < read, "{read} {broken_read}");
}

#[test]
fn hand_written_texts() {
    for t in [
        "",
        " ",
        "\u{feff}{\"type\":\"Point\",\"coordinates\":[1,2]}",
        "{\"type\":\"Point\",\"coordinates\":[1,2]} x",
        "{\"type\":\"Point\",\"coordinates\":[1,2]}\n\t\r ",
        "{\"type\":\"Point\",\"coordinates\":[01,2]}",
        "{\"type\":\"Point\",\"coordinates\":[1.,2]}",
        "{\"type\":\"Point\",\"coordinates\":[.5,2]}",
        "{\"type\":\"Point\",\"coordinates\":[1,2,]}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],}",
        "{\"type\":\"Point\",\"coordinates\":[-,2]}",
        "{\"type\":\"Point\",\"coordinates\":[1e,2]}",
        "{\"type\":\"Point\",\"coordinates\":[0e99999999999,2]}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\ud800\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\udc00\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\ud800\\u0041\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\ud800x\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\x\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"a\u{1}\"}",
        "{\"type\":\"Point\",\"coordinates\":[1,2],\"p\":\"\\u00G0\"}",
        "{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[1,2]},\"properties\":{\"b\":1,\"a\":2,\"b\":3}}",
        "{\"type\":\"Feature\",\"properties\":{\"2\":1,\"1\":2,\"x\":-0,\"y\":-0.0,\"z\":7}}",
        "{\"type\":\"FeatureCollection\",\"features\":[],\"features\":[{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[1,2]}}]}",
        "{\"type\":\"Point\",\"type\":\"LineString\",\"coordinates\":[[1,2],[3,4]]}",
        "{\"type\":\"Point\",\"coordinates\":0}",
        "{\"type\":\"Point\",\"geometries\":\"\"}",
        "{\"type\":\"Point\",\"geometries\":\"x\"}",
        "[]",
        "7",
        "null",
        "\"FeatureCollection\"",
    ] {
        let _ = same(t, usize::MAX);
    }
    // nesting: serde_json's limit (127 levels read, the 128th refused) in properties, coordinates and skipped members
    for depth in [120usize, 126, 127, 128, 129, 300] {
        let deep = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        for t in [
            deep.clone(),
            format!("{{\"type\":\"Feature\",\"properties\":{{\"a\":{deep}}}}}"),
            format!("{{\"type\":\"Point\",\"coordinates\":{deep}}}"),
            format!("{{\"type\":\"Point\",\"coordinates\":[1,2],\"x\":{deep}}}"),
        ] {
            let _ = same(&t, usize::MAX);
        }
    }
}

/// The writer straight into text against the writer through a JSON tree: every layer the generated texts read (with
/// and without z, a conversion that sends some points to NaN, metres to feet), written both ways.
#[test]
fn text_writer_matches_the_tree_writer() {
    let mut r = Rng(0x2545_f491_4f6c_dd1d);
    let to_ll = |x: f64, y: f64| if x > 1e17 { (f64::NAN, y) } else { (x * 0.5 - 0.0, -y) };
    let z = |z: f64| z * 1200.0 / 3937.0;
    let mut written = 0;
    for _ in 0..20_000 {
        let d = document(&mut r);
        let Ok(read) = geojson::read_geojson(&d, "t", usize::MAX) else { continue };
        let groups: Vec<geojson::WGroup> = read
            .layers
            .into_iter()
            .enumerate()
            .map(|(i, l)| geojson::WGroup {
                kind: l.name,
                geom: l.geom,
                has_z: l.has_z || i % 2 == 1,
                fields: l.fields,
                features: l.features,
            })
            .collect();
        for name in [Some("n\u{1}\"q"), None, Some("")] {
            let a = geojson::write_geojson(&groups, &to_ll, &z, name);
            let b = geojson::write_geojson_tree(&groups, &to_ll, &z, name);
            assert_eq!(a, b, "{d}");
        }
        written += 1;
    }
    assert!(written > 3_000, "{written}");
}
