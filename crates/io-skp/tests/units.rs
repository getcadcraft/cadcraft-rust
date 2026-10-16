//! Port of the cc-skp tests (getcadcraft.com `tools/skp-test/test.mjs` at 594ab3e) that need only the reader:
//! units on hand-made bytes (header sniffing, the ZIP reader, a hand-built model.dat with known geometry, refusals
//! with the right code) and the truncation fuzz over the corpus (cut copies must give a model or an `SkpError`,
//! never a panic). Not ported: the OpenSKP oracle comparison (the fork-parity kit compares with the JS instead) and
//! the progress / abort checks (the Rust API has no callbacks).

use cc_io_skp::read::read_skp_with;
use cc_io_skp::zip::{crc32, open_zip};
use cc_io_skp::{LIMITS, Limits, SkpError, read_skp, sniff_skp};

// ------------------------------------------------------------------ hand-made bytes

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}
fn u16b(v: u16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn f64s(vs: &[f64]) -> Vec<u8> {
    vs.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn id_bytes(mut v: u64) -> Vec<u8> {
    let mut b = Vec::new();
    loop {
        b.push((v & 255) as u8);
        v /= 256;
        if v == 0 {
            break;
        }
    }
    b
}
/// One record: tag as its two file bytes (0xC409 → C4 09), u32 length, payload.
fn rec(tag: u16, payload: &[&[u8]]) -> Vec<u8> {
    let p = payload.concat();
    cat(&[&[(tag >> 8) as u8, (tag & 255) as u8], &u32b(p.len() as u32), &p])
}
fn id_rec(id: u64) -> Vec<u8> {
    rec(0xdc05, &[&rec(0xde05, &[&id_bytes(id)])])
}
fn info(id: u64, mat: Option<u64>, layer: Option<u64>, flags: u8) -> Vec<u8> {
    let m = mat.map(|m| rec(0xd107, &[&id_bytes(m)])).unwrap_or_default();
    let l = layer.map(|l| rec(0xd207, &[&id_bytes(l)])).unwrap_or_default();
    rec(0xd007, &[&id_rec(id), &m, &l, &rec(0xd307, &[&[flags]])])
}
fn str_rec(s: &str) -> Vec<u8> {
    let mut v = vec![0xff, 0xfe, 0xff, s.len() as u8];
    for c in s.encode_utf16() {
        v.extend(c.to_le_bytes());
    }
    v
}

fn deflate_raw(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; data.len() * 2 + 1024];
    let cfg = zlib_rs_config();
    let (c, rc) = zlib_rs::compress_slice(&mut out, data, cfg);
    assert_eq!(rc, zlib_rs::ReturnCode::Ok);
    c.to_vec()
}
fn zlib_rs_config() -> zlib_rs::DeflateConfig {
    zlib_rs::DeflateConfig { window_bits: -15, ..zlib_rs::DeflateConfig::default() }
}

struct ZEntry {
    name: String,
    data: Vec<u8>,
    method: u16,
    flags: u16,
    crc: Option<u32>,
    usize: Option<u32>,
    stored: Option<Vec<u8>>,
}
fn ze(name: &str, data: &[u8], method: u16) -> ZEntry {
    ZEntry { name: name.into(), data: data.to_vec(), method, flags: 0, crc: None, usize: None, stored: None }
}

/// Minimal ZIP writer.
fn make_zip(entries: &[ZEntry]) -> Vec<u8> {
    let (mut locals, mut centrals) = (Vec::new(), Vec::new());
    let mut off = 0u32;
    for e in entries {
        let name = e.name.as_bytes();
        let data = if e.method == 8 { deflate_raw(&e.data) } else { e.stored.clone().unwrap_or_else(|| e.data.clone()) };
        let crc = e.crc.unwrap_or_else(|| crc32(&e.data));
        let lh = cat(&[
            &u32b(0x04034b50),
            &u16b(20),
            &u16b(e.flags),
            &u16b(e.method),
            &u16b(0),
            &u16b(0),
            &u32b(crc),
            &u32b(data.len() as u32),
            &u32b(e.data.len() as u32),
            &u16b(name.len() as u16),
            &u16b(0),
            name,
        ]);
        centrals.push(cat(&[
            &u32b(0x02014b50),
            &u16b(20),
            &u16b(20),
            &u16b(e.flags),
            &u16b(e.method),
            &u16b(0),
            &u16b(0),
            &u32b(crc),
            &u32b(data.len() as u32),
            &u32b(e.usize.unwrap_or(e.data.len() as u32)),
            &u16b(name.len() as u16),
            &u16b(0),
            &u16b(0),
            &u16b(0),
            &u16b(0),
            &u32b(0),
            &u32b(off),
            name,
        ]));
        off += (lh.len() + data.len()) as u32;
        locals.push(lh);
        locals.push(data);
    }
    let cd = centrals.concat();
    cat(&[
        &locals.concat(),
        &cd,
        &u32b(0x06054b50),
        &u16b(0),
        &u16b(0),
        &u16b(entries.len() as u16),
        &u16b(entries.len() as u16),
        &u32b(cd.len() as u32),
        &u32b(off),
        &u16b(0),
    ])
}

fn vff_header(version: &str) -> Vec<u8> {
    cat(&[&str_rec("SketchUp Model"), &str_rec(&format!("{{{version}}}")), b"VFF", &[8, 0, 1, 0, 0x11, 0, 0x3f, 0x71, 0x52, 0x7c]])
}

fn mat_xml(name: &str, r: u8, g: u8, b: u8, tex: Option<&str>) -> Vec<u8> {
    let t = tex.map(|t| format!("<mat:texture textureFilename=\"C:\\x\\{t}\" xScale=\"10\" yScale=\"20\"><mat:images><mat:image id=\"1\" path=\"./{t}\" file_name=\"{t}\"/></mat:images></mat:texture>")).unwrap_or_default();
    format!("<?xml version=\"1.0\"?><materialDocument><mat:material name=\"{name}\" colorRed=\"{r}\" colorGreen=\"{g}\" colorBlue=\"{b}\" trans=\"0.25\" useTrans=\"1\">{t}</mat:material></materialDocument>").into_bytes()
}
const PNG: [u8; 12] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];

#[derive(Default)]
struct Opts {
    version: Option<&'static str>,
    extra_defs: Vec<Vec<u8>>,
    root_insts: Option<Vec<u8>>,
    verts_override: Option<Vec<Vec<u8>>>,
}

/// The known-geometry model (test.mjs knownModel):
///   root: square face 0..10 × 0..10 at z = 0 with a 4..6 hole, front material 1 (textured), back material 2, on tag 3;
///         one loose hidden+soft edge (20,0,0)–(20,5,0); one instance of definition D: rotate 90° about Z, scale × 2
///         (w = 0.5), move (100, 0, 0), painted material 2, tag 3, hidden, named "Inst".
///   D (component, id 500): unit triangle (0,0,0) (1,0,0) (0,1,0) whose two edges along the axes are smooth.
fn known_model(o: Opts) -> Vec<u8> {
    let mut id = 1000u64;
    let mut next = || {
        id += 1;
        id - 1
    };
    let mut verts: Vec<Vec<u8>> = Vec::new();
    let mut edges: Vec<Vec<u8>> = Vec::new();
    let mut vid: Vec<([i64; 3], u64)> = Vec::new();
    let mut vtx = |x: f64, y: f64, z: f64, next: &mut dyn FnMut() -> u64, verts: &mut Vec<Vec<u8>>| -> u64 {
        let k = [x as i64, y as i64, z as i64];
        if let Some((_, i)) = vid.iter().find(|(kk, _)| *kk == k) {
            return *i;
        }
        let i = next();
        vid.push((k, i));
        verts.push(rec(0xc409, &[&id_rec(i), &rec(0xc509, &[&f64s(&[x, y, z])])]));
        i
    };
    let edge = |a: u64, b: u64, flags: u8, next: &mut dyn FnMut() -> u64, edges: &mut Vec<Vec<u8>>| -> u64 {
        let i = next();
        edges.push(rec(0xb80b, &[&info(i, None, None, flags), &rec(0xb90b, &[&id_bytes(a)]), &rec(0xba0b, &[&id_bytes(b)])]));
        i
    };
    let loop_rec = |ids: &[u64], rev: &[bool]| -> Vec<u8> {
        let co: Vec<Vec<u8>> =
            ids.iter().zip(rev).map(|(e, r)| rec(0xa00f, &[&rec(0xa10f, &[&id_bytes(*e)]), &rec(0xa20f, &[&[u8::from(*r)]])])).collect();
        let refs: Vec<&[u8]> = co.iter().map(Vec::as_slice).collect();
        rec(0x9411, &[&rec(0x9511, &refs)])
    };
    // Outer square, counter-clockwise seen from +Z; edges stored one way, used forward.
    let sq: Vec<u64> = [(0., 0.), (10., 0.), (10., 10.), (0., 10.)].iter().map(|&(x, y)| vtx(x, y, 0., &mut next, &mut verts)).collect();
    let se: Vec<u64> = (0..4).map(|i| edge(sq[i], sq[(i + 1) % 4], 6, &mut next, &mut edges)).collect();
    // Hole stored counter-clockwise too; the loop uses its edges reversed (so it runs clockwise).
    let hl: Vec<u64> = [(4., 4.), (6., 4.), (6., 6.), (4., 6.)].iter().map(|&(x, y)| vtx(x, y, 0., &mut next, &mut verts)).collect();
    let he: Vec<u64> = (0..4).map(|i| edge(hl[i], hl[(i + 1) % 4], 6 | 0x08, &mut next, &mut edges)).collect();
    let hole_ids = [he[3], he[2], he[1], he[0]];
    let fid = next();
    let face = rec(
        0xac0d,
        &[
            &info(fid, Some(1), Some(3), 6),
            &rec(0xad0d, &[&f64s(&[0., 0., 1., 0.])]),
            &rec(0xae0d, &[&loop_rec(&se, &[false; 4]), &loop_rec(&hole_ids, &[true; 4])]),
            &rec(0xaf0d, &[&id_bytes(2)]),
        ],
    );
    let a = vtx(20., 0., 0., &mut next, &mut verts);
    let b = vtx(20., 5., 0., &mut next, &mut verts);
    edge(a, b, 6 | 0x01 | 0x08, &mut next, &mut edges);
    let iid = next();
    let inst = rec(
        0x8c13,
        &[&rec(
            0x6419,
            &[
                &info(iid, Some(2), Some(3), 7),
                &rec(0x6519, &[b"Inst"]),
                &rec(0x6619, &[&f64s(&[0., -1., 0., 1., 0., 0., 0., 0., 1., 50., 0., 0., 0.5])]),
                &rec(0x6719, &[&id_bytes(500)]),
                &rec(0x6819, &[&[0u8; 16]]),
            ],
        )],
    );
    let rid = next();
    let vlist = o.verts_override.clone().unwrap_or(verts);
    let vrefs: Vec<&[u8]> = vlist.iter().map(Vec::as_slice).collect();
    let erefs: Vec<&[u8]> = edges.iter().map(Vec::as_slice).collect();
    let root_ents = rec(
        0x8813,
        &[
            &info(rid, None, None, 6),
            &rec(0x8913, &vrefs),
            &rec(0x8a13, &erefs),
            &rec(0x8b13, &[&face]),
            o.root_insts.as_deref().unwrap_or(&inst),
        ],
    );
    // Definition D.
    let (mut dv, mut de) = (Vec::new(), Vec::new());
    let mut d_v = |x: f64, y: f64, z: f64| {
        let i = next();
        dv.push(rec(0xc409, &[&id_rec(i), &rec(0xc509, &[&f64s(&[x, y, z])])]));
        i
    };
    let t = [d_v(0., 0., 0.), d_v(1., 0., 0.), d_v(0., 1., 0.)];
    let mut d_e = |a: u64, b: u64, flags: u8| {
        let i = next();
        de.push(rec(0xb80b, &[&info(i, None, None, flags), &rec(0xb90b, &[&id_bytes(a)]), &rec(0xba0b, &[&id_bytes(b)])]));
        i
    };
    let te = [d_e(t[0], t[1], 6 | 0x10), d_e(t[1], t[2], 6), d_e(t[2], t[0], 6 | 0x10)];
    let dfid = next();
    let dface =
        rec(0xac0d, &[&info(dfid, None, None, 6), &rec(0xad0d, &[&f64s(&[0., 0., 1., 0.])]), &rec(0xae0d, &[&loop_rec(&te, &[false; 3])])]);
    let dvr: Vec<&[u8]> = dv.iter().map(Vec::as_slice).collect();
    let der: Vec<&[u8]> = de.iter().map(Vec::as_slice).collect();
    let def_d = rec(
        0x7c15,
        &[
            &rec(0x8813, &[&info(500, None, None, 6), &rec(0x8913, &dvr), &rec(0x8a13, &der), &rec(0x8b13, &[&dface])]),
            &rec(0x7d15, &[&[0x1b, 0xf0, 0xcc, 0xa1, 0xcf, 0x72, 0x5a, 0x4a, 0x86, 0xd6, 0xa4, 0x2d, 0x71, 0xd5, 0xb3, 0xa4]]),
            &rec(0x7e15, &[b"Triangle"]),
            &rec(0x8315, &[&u32b(0)]),
        ],
    );
    let mats = rec(
        0xf701,
        &[&rec(
            0xd430,
            &[&rec(
                0xd530,
                &[&rec(0xc832, &[&id_rec(1), &rec(0xcc32, &[b"Brick"])]), &rec(0xc832, &[&id_rec(2), &rec(0xcc32, &[b"Red"])])],
            )],
        )],
    );
    let layer_rec = |lid: u64, name: &str, hidden: bool| {
        rec(
            0x8c3c,
            &[
                &id_rec(lid),
                &rec(0x8d3c, &[name.as_bytes()]),
                &rec(0x8e3c, &[&[u8::from(hidden)]]),
                &rec(0x8f3c, &[&rec(0xc832, &[&id_rec(lid + 100), &rec(0xcc32, &[format!("Layer_{name}").as_bytes()])])]),
            ],
        )
    };
    let layers = rec(0xf801, &[&rec(0x983a, &[&rec(0x993a, &[&layer_rec(3, "Walls", true), &layer_rec(4, "Layer0", false)])])]);
    let mut dd: Vec<&[u8]> = vec![&def_d];
    dd.extend(o.extra_defs.iter().map(Vec::as_slice));
    let defs = rec(0xf901, &[&rec(0x7017, &[&rec(0x7117, &dd)])]);
    let md = rec(0xf401, &[&mats, &layers, &defs, &rec(0xf601, &[&root_ents])]);
    let meta = rec(0x6400, &[&rec(0x6d00, &[b"Millimeter"])]);
    cat(&[
        &vff_header(o.version.unwrap_or("25.0.575")),
        &make_zip(&[
            ze("meta/model_thumbnail.png", &PNG, 0),
            ze("meta/meta.dat", &meta, 0),
            ze("materials/Brick/material.xml", &mat_xml("Brick", 200, 100, 50, Some("brick.png")), 8),
            ze("materials/Brick/brick.png", &PNG, 0),
            ze("materials/Red/material.xml", &mat_xml("Red", 255, 0, 0, None), 0),
            ze("materials/Layer_Walls/material.xml", &mat_xml("Layer_Walls", 1, 2, 3, None), 0),
            ze("model.dat", &md, 8),
        ]),
    ])
}

fn code(r: Result<impl Sized, SkpError>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(e) => e.code,
    }
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}
fn apply(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

#[test]
fn sniffing() {
    let h = sniff_skp(&vff_header("25.0.575")).unwrap();
    assert_eq!((h.header.version.as_str(), h.header.major, h.container.as_str(), h.header.body_start), ("25.0.575", 25.0, "vff", 0x38));
    let legacy = cat(&[&str_rec("SketchUp Model"), &str_rec("{17.0.18899}"), &[0u8; 16], &[0xff, 0xfe, 0xff, 0]]);
    let hl = sniff_skp(&legacy).unwrap();
    assert_eq!((hl.container.as_str(), hl.header.major, hl.header.version.as_str()), ("legacy", 17.0, "17.0.18899"));
    let mut long = vec![0xff, 0xfe, 0xff, 0xff];
    long.extend(u16b(14));
    for c in "SketchUp Model".encode_utf16() {
        long.extend(c.to_le_bytes());
    }
    long.extend(str_rec("{3.0.1}"));
    assert_eq!(sniff_skp(&long).unwrap().header.major, 3.0, "long string length form");
    assert_eq!(code(read_skp(&vff_header("25.0.575")[..20])), "damaged", "cut inside \"SketchUp Model\"");
    assert_eq!(code(read_skp(&[])), "not_skp", "empty file");
    assert_eq!(code(read_skp(b"PK\x03\x04 hello")), "not_skp", "a ZIP");
    assert_eq!(code(read_skp(&cat(&[&str_rec("Sketchup Modex"), &str_rec("{25.0.1}")]))), "not_skp", "wrong magic");
    assert_eq!(code(read_skp(&cat(&[&str_rec("SketchUp Model"), &str_rec("25.0.1")]))), "damaged", "version without braces");
    assert_eq!(code(read_skp(&cat(&[&vff_header("25.0.575"), b"nothing here"]))), "damaged", "VFF without an archive");
}

#[test]
fn zip_reader() {
    let data = b"hello hello hello hello zip".to_vec();
    let z = make_zip(&[ze("a.txt", &data, 0), ze("b/\u{fc}.txt", &data, 8)]);
    let pre = cat(&[b"junk before the archive", &z]);
    let zr = open_zip(&pre, 23, &LIMITS).unwrap();
    assert_eq!(zr.names.join("|"), "a.txt|b/\u{fc}.txt", "names (UTF-8 without the flag)");
    let mut budget = 0;
    assert_eq!(&*zr.read("a.txt", &mut budget).unwrap(), &data[..], "stored entry");
    assert_eq!(&*zr.read("b/\u{fc}.txt", &mut budget).unwrap(), &data[..], "deflated entry, offsets relative to the archive start");
    assert_eq!(budget, 2 * data.len() as u64);
    let rd = |z: &[u8]| -> &'static str {
        match open_zip(z, 0, &LIMITS) {
            Err(e) => e.code,
            Ok(zz) => code(zz.read("a", &mut 0)),
        }
    };
    let mut e = ze("a", &data, 0);
    e.crc = Some(1234);
    assert_eq!(rd(&make_zip(&[e])), "damaged", "checksum mismatch");
    let mut e = ze("a", &data, 0);
    e.flags = 1;
    assert_eq!(rd(&make_zip(&[e])), "unsupported", "encrypted entry");
    let mut e = ze("a", &data, 12);
    e.stored = Some(data.clone());
    assert_eq!(rd(&make_zip(&[e])), "unsupported", "bzip2 entry");
    let z64 = cat(&[&z[..z.len() - 22], &u32b(0x07064b50), &[0u8; 16], &z[z.len() - 22..]]);
    assert_eq!(code(open_zip(&z64, 0, &LIMITS)), "unsupported", "ZIP64 locator");
    let mut zff = z.clone();
    let n = zff.len();
    zff[n - 22 + 16..n - 22 + 20].copy_from_slice(&u32b(0xffff_ffff));
    assert_eq!(code(open_zip(&zff, 0, &LIMITS)), "unsupported", "0xFFFFFFFF directory offset");
    assert_eq!(code(open_zip(&z[..z.len() - 5], 0, &LIMITS)), "damaged", "cut-off end record");
    let mut e = ze("a", &data, 0);
    e.usize = Some(LIMITS.entry_bytes as u32 + 1);
    assert_eq!(rd(&make_zip(&[e])), "too_big", "entry over LIMITS.entryBytes");
    let mut e = ze("a", &data, 8);
    e.usize = Some(5);
    assert_eq!(rd(&make_zip(&[e])), "too_big", "inflates past its declared size");
    let mut broken = make_zip(&[ze("a", &data, 8)]);
    broken[30 + 1 + 2] ^= 0xff; // corrupt the deflate stream
    assert_eq!(rd(&broken), "damaged", "broken deflate stream");
    // (Rust port) what DecompressionStream reports: a cut-off stream and bytes after the stream's end are damaged
    let comp = deflate_raw(&data);
    let mut e = ze("a", &data, 0);
    e.stored = Some(comp[..comp.len() - 2].to_vec());
    e.method = 8;
    assert_eq!(rd(&make_zip_raw(e, comp.len() - 2)), "damaged", "cut-off deflate stream");
    let mut e = ze("a", &data, 0);
    e.stored = Some(cat(&[&comp, &[1, 2, 3]]));
    e.method = 8;
    assert_eq!(rd(&make_zip_raw(e, comp.len() + 3)), "damaged", "trailing junk after the deflate stream");
}

/// An entry whose `stored` bytes are written as is under `method` (a hand-made compressed stream).
fn make_zip_raw(e: ZEntry, _len: usize) -> Vec<u8> {
    let m = e.method;
    let mut e = e;
    e.method = 0;
    let mut z = make_zip(&[e]);
    // patch the method in the local and central headers
    z[8..10].copy_from_slice(&u16b(m));
    let cd = z.windows(4).position(|w| w == u32b(0x02014b50)).unwrap();
    z[cd + 10..cd + 12].copy_from_slice(&u16b(m));
    z
}

#[test]
fn known_geometry_model() {
    let skp = read_skp(&known_model(Opts::default())).unwrap();
    assert!(skp.container.as_str() == "vff" && skp.major == 25.0, "container and version");
    assert!(
        skp.defs.len() == 2
            && skp.root == 0
            && skp.defs[0].kind == "model"
            && skp.defs[1].kind == "component"
            && skp.defs[1].name == "Triangle"
    );
    assert_eq!(skp.defs[1].guid.as_deref(), Some("1bf0cca1-cf72-5a4a-86d6-a42d71d5b3a4"), "definition guid");
    let tags: Vec<String> = skp.layers.iter().map(|l| format!("{}:{}", l.name.as_deref().unwrap(), l.visible.unwrap())).collect();
    assert_eq!(tags.join(","), "Layer0:true,Walls:false", "tags, Layer0 first, hidden tag");
    assert_eq!(skp.layers[1].color, Some([1, 2, 3]), "tag colour from Layer_<name> material");
    assert!(
        skp.materials.len() == 2
            && skp.materials[0].name.as_deref() == Some("Brick")
            && skp.materials[0].color == Some([200, 100, 50])
            && near(skp.materials[0].alpha, 0.75),
        "materials"
    );
    let tex = skp.materials[0].texture.as_ref().unwrap();
    assert!(tex.mime == "image/png" && tex.width == 10.0 && tex.height == 20.0 && tex.bytes.len() == PNG.len() && tex.name == "brick.png");
    assert!(skp.materials[1].texture.is_none(), "untextured material");
    let f = &skp.defs[0].faces[0];
    assert!(skp.defs[0].faces.len() == 1 && f.loops.len() == 2 && f.loops[0].len() == 4 && f.loops[1].len() == 4);
    assert_eq!(f.loops[0], vec![[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]], "outer loop CCW about the normal");
    assert_eq!(f.loops[1], vec![[4., 4., 0.], [4., 6., 0.], [6., 6., 0.], [6., 4., 0.]], "hole runs the other way");
    assert!(f.front == Some(0) && f.back == Some(1) && f.layer == Some(1) && f.hidden == Some(false), "face materials and tag");
    let uv = f.uv.as_ref().unwrap();
    assert!(near(uv[0][1][0], 1.0) && near(uv[0][1][1], 0.0) && near(uv[0][2][1], 0.5), "UVs from the tile size (10 × 20 in)");
    assert!(skp.defs[1].faces[0].uv.is_none(), "no UVs on an unpainted face");
    let le = &skp.defs[0].edges;
    assert!(le.len() == 1 && le[0].a == [20., 0., 0.] && le[0].hidden && le[0].soft && !le[0].smooth, "loose edge only, with its flags");
    assert_eq!(skp.defs[0].soft.len(), 4, "soft face edges listed (the hole's four)");
    assert!(skp.defs[1].soft.len() == 2 && skp.defs[1].edges.is_empty(), "smooth face edges in the definition");
    let inst = &skp.defs[0].insts[0];
    assert!(inst.def == 1 && inst.name == "Inst" && inst.hidden && inst.material == Some(1) && inst.layer == Some(1), "instance fields");
    // Rows (0,-1,0) (1,0,0) (0,0,1), t = (50,0,0), w = 0.5 → p' = 2·(R·p + t): (1,0,0) → (0,2,0) + (100,0,0).
    let (p1, p2) = (apply(&inst.m, [1., 0., 0.]), apply(&inst.m, [0., 1., 0.]));
    assert!(
        near(p1[0], 100.) && near(p1[1], 2.) && near(p1[2], 0.) && near(p2[0], 98.) && near(p2[1], 0.),
        "placement = rows, translation, homogeneous scale"
    );
    assert_eq!(skp.units, Some("mm"), "display unit from meta.dat");
    assert_eq!(skp.thumbnail.as_ref().map(|t| t.mime), Some("image/png"), "thumbnail");
    let c = skp.counts;
    assert_eq!((c.defs, c.faces, c.edges, c.insts, c.materials, c.layers), (2, 2, 1, 1, 2, 2), "counts");
    assert!(skp.notes.is_empty(), "nothing skipped: {:?}", skp.notes);
}

fn plain_inst(id: u64, def: u64, w: f64) -> Vec<u8> {
    rec(
        0x8c13,
        &[&rec(
            0x6419,
            &[
                &info(id, None, None, 6),
                &rec(0x6619, &[&f64s(&[1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., w])]),
                &rec(0x6719, &[&id_bytes(def)]),
            ],
        )],
    )
}

#[test]
fn crafted_refusals() {
    let self_def = rec(
        0x7c15,
        &[&rec(0x8813, &[&info(600, None, None, 6), &plain_inst(9000, 600, 1.0)]), &rec(0x7e15, &[b"Self"]), &rec(0x8315, &[&u32b(1)])],
    );
    assert_eq!(
        code(read_skp(&known_model(Opts { extra_defs: vec![self_def], ..Opts::default() }))),
        "damaged",
        "a component containing itself"
    );
    let n = LIMITS.depth + 2;
    let chain: Vec<Vec<u8>> = (0..n)
        .map(|i| {
            let child = if i + 1 < n { plain_inst(20000 + i as u64, 700 + i as u64 + 1, 1.0) } else { Vec::new() };
            rec(0x7c15, &[&rec(0x8813, &[&info(700 + i as u64, None, None, 6), &child]), &rec(0x7e15, &[format!("L{i}").as_bytes()])])
        })
        .collect();
    assert_eq!(code(read_skp(&known_model(Opts { extra_defs: chain, ..Opts::default() }))), "too_big", "nesting deeper than LIMITS.depth");
    let nan_vert = vec![rec(0xc409, &[&id_rec(1), &rec(0xc509, &[&f64s(&[f64::NAN, 0., 0.])])])];
    assert_eq!(code(read_skp(&known_model(Opts { verts_override: Some(nan_vert), ..Opts::default() }))), "damaged", "NaN coordinate");
    let small = Limits { vertices: 5, ..LIMITS };
    assert_eq!(code(read_skp_with(&known_model(Opts::default()), &small)), "too_big", "more vertices than LIMITS.vertices");
    assert_eq!(
        code(read_skp(&known_model(Opts { root_insts: Some(plain_inst(9100, 500, 0.0)), ..Opts::default() }))),
        "damaged",
        "placement with w = 0"
    );
    let gm = read_skp(&known_model(Opts { root_insts: Some(plain_inst(9200, 4242, 1.0)), ..Opts::default() })).unwrap();
    assert!(
        gm.defs[0].insts.is_empty() && gm.notes.iter().any(|n| n.contains("missing definition")),
        "instance of a missing definition skipped with a note: {:?}",
        gm.notes
    );
    // A record that overruns its container inside model.dat.
    let bad_md = cat(&[&rec(0xf401, &[&rec(0xf601, &[&[0u8; 3]])]), &[0xf6, 0x01, 255, 255, 0, 0]]);
    assert_eq!(
        code(read_skp(&cat(&[&vff_header("25.0.575"), &make_zip(&[ze("model.dat", &bad_md, 0)])]))),
        "damaged",
        "record runs past the end"
    );
    assert_eq!(
        code(read_skp(&cat(&[&vff_header("99.0.1"), &make_zip(&[ze("model.dat", &[1, 2, 3], 0)])]))),
        "too_new",
        "unreadable model from a newer SketchUp"
    );
    assert_eq!(
        code(read_skp(&cat(&[&vff_header("25.0.575"), &make_zip(&[ze("other.dat", b"x", 0)])]))),
        "damaged",
        "archive without model.dat"
    );
    let tiny = Limits { file_bytes: 100, ..LIMITS };
    let e = read_skp_with(&known_model(Opts::default()), &tiny).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        ("too_big", "This SketchUp file is larger than cc-skp reads (0 MB)."),
        "file over LIMITS.fileBytes"
    );
}

#[test]
fn messages_match_the_js() {
    let e = read_skp(&cat(&[&str_rec("SketchUp Model"), &str_rec("25.0.1")])).unwrap_err();
    assert_eq!(e.message, "This SketchUp file is damaged: its version is missing.");
    let e = read_skp(&cat(&[&vff_header("25.0.575"), &make_zip(&[ze("other.dat", b"x", 0)])])).unwrap_err();
    assert_eq!(e.message, "This SketchUp file is damaged: model.dat is missing from the archive.");
    let e = read_skp(&cat(&[&vff_header("99.0.1"), &make_zip(&[ze("model.dat", &[1, 2, 3], 0)])])).unwrap_err();
    assert_eq!(e.message, "This file was saved by SketchUp 99.0.1, newer than cc-skp reads (up to 2026).");
}

// ------------------------------------------------------------------ truncation fuzz over the corpus

fn corpus_files() -> Vec<std::path::PathBuf> {
    let root = std::env::var("SKP_CORPUS").unwrap_or_else(|_| "/var/www/CADCRAFT/testfiles/skp-corpus".to_string());
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Each corpus file cut at ~20 points → a model or an SkpError, never a panic (test.mjs "Truncation fuzz").
/// Skips cleanly when the corpus (outside every repo) is missing.
#[test]
fn truncation_fuzz_corpus() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!("skp corpus not found: truncation fuzz skipped");
        return;
    }
    for f in files {
        let full = std::fs::read(&f).unwrap();
        let size = full.len();
        let mut cuts: Vec<usize> = vec![0, 1, 3, 4, 10, 30, 60, 100, 200];
        cuts.extend((1..=12).map(|i| size * i / 13));
        cuts.push(size.saturating_sub(1));
        cuts.push(size.saturating_sub(22));
        cuts.sort_unstable();
        cuts.dedup();
        for cut in cuts.into_iter().filter(|&c| c < size) {
            let r = std::panic::catch_unwind(|| read_skp(&full[..cut]).map(|_| ()));
            assert!(r.is_ok(), "panic reading {} cut at {cut}", f.display());
        }
    }
}

/// Writes the synthetic known-geometry file as the fuzz seed (`SKP_WRITE_SEED=1 cargo test -p cc-io-skp -- --ignored`).
#[test]
#[ignore]
fn write_fuzz_seed() {
    if std::env::var("SKP_WRITE_SEED").is_ok() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fuzz/seeds/skp_read");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/known-model-vff.skp"), known_model(Opts::default())).unwrap();
    }
}
