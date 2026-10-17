//! `embed.js` — the takeoff saved inside a PDF (T10): attached as "takeoff.takeoffcraft.json" (an older copy of that
//! attachment is replaced), with the hash of the PDF it stands for; read back from the document's EmbeddedFiles name
//! tree. The copy is an incremental update of the original.

use hayro::hayro_interpret::hayro_syntax::object::{Array, Dict, MaybeRef, Object, ObjectIdentifier, Stream};
use serde_json::{Value, json};

use cc_takeoff::jv;

use crate::pdfread::Doc;
use crate::pdfw::{PO, Writer, copy_obj, dict_entries, read_trailer};

pub const EMBED_NAME: &str = "takeoff.takeoffcraft.json";
pub const EMBED_FORMAT: &str = "takeoffcraft-embedded";

fn text_of(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let u: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&u);
    }
    bytes.iter().map(|b| char::from(*b)).collect()
}
fn utf16be(s: &str) -> Vec<u8> {
    let mut b = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_be_bytes());
    }
    b
}

/// A copy of the PDF with the takeoff attached.
pub fn embed_takeoff(bytes: &[u8], takeoff: &Value, stands_for: &str, now_ms: f64) -> Result<Vec<u8>, String> {
    let tr = read_trailer(bytes)?;
    if tr.encrypted {
        return Err("The PDF is encrypted.".into());
    }
    let doc = crate::pdfread::open(bytes)?;
    let xref = doc.pdf.xref();
    let root = tr.root.ok_or("no catalog")?;
    let catalog = xref.get::<Dict<'_>>(xref.root_id()).ok_or("no catalog")?;
    let mut w = Writer::new(u32::try_from(tr.size).unwrap_or(1));
    let payload = jv::stringify(&json!({ "format": EMBED_FORMAT, "version": 1, "standsFor": stands_for, "takeoff": takeoff }));
    let date = crate::pdfw::pdf_date(&crate::xlsx::Stamp::from_ms(now_ms));
    let ef = w.add_stream(
        vec![
            ("Type", PO::name("EmbeddedFile")),
            ("Subtype", PO::name("application/json")),
            (
                "Params",
                PO::dict(vec![
                    ("Size", PO::Num(payload.len() as f64)),
                    ("CreationDate", PO::Str(date.clone().into_bytes())),
                    ("ModDate", PO::Str(date.into_bytes())),
                ]),
            ),
        ],
        payload.as_bytes(),
    );
    let spec = w.add(&PO::dict(vec![
        ("Type", PO::name("Filespec")),
        ("F", PO::Str(EMBED_NAME.as_bytes().to_vec())),
        ("UF", PO::Str(utf16be(EMBED_NAME))),
        ("EF", PO::dict(vec![("F", ef.clone())])),
        ("Desc", PO::Str(utf16be("TakeoffCraft takeoff"))),
    ]));
    // the Names tree's root array, minus an earlier copy of the takeoff
    let names = catalog.get::<Dict<'_>>(b"Names".as_slice());
    let efd = names.as_ref().and_then(|n| n.get::<Dict<'_>>(b"EmbeddedFiles".as_slice()));
    let mut arr: Vec<PO> = Vec::new();
    if let Some(a) = efd.as_ref().and_then(|e| e.get::<Array<'_>>(b"Names".as_slice())) {
        let items: Vec<MaybeRef<Object<'_>>> = a.raw_iter().take(200_000).collect();
        for pair in items.chunks(2) {
            let [k, v] = pair else { continue };
            let key = match k {
                MaybeRef::NotRef(Object::String(s)) => Some(text_of(s.as_bytes())),
                _ => None,
            };
            if key.as_deref() == Some(EMBED_NAME) {
                continue;
            }
            let (mut kb, mut vb) = (Vec::new(), Vec::new());
            copy_obj(&mut kb, k, 1);
            copy_obj(&mut vb, v, 1);
            arr.push(PO::Raw(kb));
            arr.push(PO::Raw(vb));
        }
    }
    arr.push(PO::Str(utf16be(EMBED_NAME)));
    arr.push(spec.clone());
    let mut efe = efd.as_ref().map(|e| dict_entries(e, &["Names"])).unwrap_or_default();
    efe.push(("Names".into(), PO::Arr(arr)));
    let mut ne = names.as_ref().map(|n| dict_entries(n, &["EmbeddedFiles"])).unwrap_or_default();
    ne.push(("EmbeddedFiles".into(), PO::Dict(efe)));
    let mut ce = dict_entries(&catalog, &["Names", "AF"]);
    let mut af: Vec<PO> = Vec::new();
    if let Some(a) = catalog.get::<Array<'_>>(b"AF".as_slice()) {
        for x in a.raw_iter().take(100_000) {
            let mut b = Vec::new();
            copy_obj(&mut b, &x, 1);
            af.push(PO::Raw(b));
        }
    }
    af.push(spec);
    ce.push(("Names".into(), PO::Dict(ne)));
    ce.push(("AF".into(), PO::Arr(af)));
    let mut body = Vec::new();
    crate::pdfw::write(&mut body, &PO::Dict(ce));
    w.put(root.0, root.1, body);
    let mut trailer = vec![("Root", PO::Ref(root.0, root.1)), ("Prev", PO::Num(tr.startxref as f64))];
    if let Some((n, g)) = tr.info {
        trailer.push(("Info", PO::Ref(n, g)));
    }
    Ok(w.finish(bytes, trailer))
}

/// The attachments of a document: (file name, content).
pub fn attachments(doc: &Doc) -> Vec<(String, Vec<u8>)> {
    let xref = doc.pdf.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
    let Some(efd) = catalog.get::<Dict<'_>>(b"Names".as_slice()).and_then(|n| n.get::<Dict<'_>>(b"EmbeddedFiles".as_slice())) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![(efd, 0u32)];
    while let Some((node, depth)) = stack.pop() {
        if depth > 16 || out.len() > 10_000 {
            break;
        }
        if let Some(a) = node.get::<Array<'_>>(b"Names".as_slice()) {
            let items: Vec<Object<'_>> = a.iter::<Object<'_>>().take(200_000).collect();
            for pair in items.chunks(2) {
                let [Object::String(k), Object::Dict(spec)] = pair else { continue };
                let key = text_of(k.as_bytes());
                let name = spec
                    .get::<hayro::hayro_interpret::hayro_syntax::object::String<'_>>(b"UF".as_slice())
                    .or_else(|| spec.get::<hayro::hayro_interpret::hayro_syntax::object::String<'_>>(b"F".as_slice()))
                    .map_or(key, |s| text_of(s.as_bytes()));
                let data = spec
                    .get::<Dict<'_>>(b"EF".as_slice())
                    .and_then(|e| e.get::<Stream<'_>>(b"F".as_slice()))
                    .and_then(|s| s.decoded().ok().map(|c| c.to_vec()));
                if let Some(d) = data {
                    out.push((name, d));
                }
            }
        }
        if let Some(kids) = node.get::<Array<'_>>(b"Kids".as_slice()) {
            for k in kids.iter::<Dict<'_>>().take(1000) {
                stack.push((k, depth.saturating_add(1)));
            }
        }
    }
    let _ = ObjectIdentifier::new(0, 0);
    out
}

/// `readEmbedded(doc)` → (standsFor, takeoff) or None.
pub fn read_embedded(doc: &Doc) -> Option<(String, Value)> {
    let (_, content) = attachments(doc).into_iter().find(|(n, _)| n == EMBED_NAME)?;
    let j = jv::parse_json(&String::from_utf8_lossy(&content), 512).ok()?;
    if jv::str(j.get("format")) != Some(EMBED_FORMAT) {
        return None;
    }
    let sf = jv::str(j.get("standsFor"))?;
    if !(sf.len() == 64 && sf.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))) {
        return None;
    }
    let tk = j.get("takeoff").filter(|v| v.is_object() || v.is_array())?;
    Some((sf.to_string(), tk.clone()))
}
